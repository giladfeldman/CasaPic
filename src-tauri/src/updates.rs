//! Release-based self-update.
//!
//! CasaPic updates itself from the signed releases published at
//! https://github.com/giladfeldman/CasaPic/releases, using Tauri's official updater plugin.
//!
//! * The endpoint and the minisign PUBLIC key live in tauri.conf.json (`plugins.updater`). The
//!   private key never enters the repository (`scripts/Publish-CasaPicRelease.ps1`).
//! * `Update::download` checks the signature over the downloaded bytes in memory BEFORE
//!   anything is written or run. A failed download, a tampered file or an unsigned file is
//!   refused and the installed copy is untouched.
//! * The version is bound to the signature: the signed (trusted) comment names the installer
//!   file, and a file signed for another version is refused, so an old genuine installer
//!   cannot be served as a "newer" release.
//! * `Update::install` writes the verified installer to a temp folder, runs it (a small
//!   progress window) and exits. The installer replaces only the program files in
//!   `%LOCALAPPDATA%\CasaPic`; the library in `%LOCALAPPDATA%\org.casapic.app\library` and the
//!   photos are never touched.
//!
//! Schedule: one check a few seconds after the library starts, which installs straight away
//! if a newer release exists (one short restart at launch); then one check every 24 hours
//! while it runs (every hour after a failed check), which downloads and verifies but waits for
//! "Restart and install". An install that did not finish is not retried automatically.
//!
//! Only the library process checks (never the quick viewer), and only in a copy installed by
//! the CasaPic setup program (`uninstall.exe` next to the exe): a dev build has nothing for an
//! installer to update.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use casapic_core::paths;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

const START_DELAY: Duration = Duration::from_secs(5);
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// After a failed check (no network yet, GitHub down) try again sooner than daily.
const RETRY_INTERVAL: Duration = Duration::from_secs(60 * 60);
const LOG_MAX_BYTES: u64 = 64 * 1024;

#[derive(Default)]
pub struct UpdateState {
    /// A downloaded, signature-verified update waiting for "Restart and install".
    pending: Mutex<Option<(Update, Vec<u8>)>>,
    /// Keeps the daily timer and the "Check now" button from downloading twice.
    busy: AtomicBool,
}

struct BusyGuard<'a>(&'a AtomicBool);

impl<'a> BusyGuard<'a> {
    fn acquire(flag: &'a AtomicBool) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).ok().map(|_| BusyGuard(flag))
    }
}

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

// --- Paths and preferences -------------------------------------------------

/// The folder holding the library (`%LOCALAPPDATA%\org.casapic.app`): the update log and the
/// on/off choice live here, beside the library, per Windows user, and survive reinstalls.
fn state_dir() -> PathBuf {
    let data = paths::data_dir();
    data.parent().map(|p| p.to_path_buf()).unwrap_or(data)
}

fn log_path() -> PathBuf {
    state_dir().join("update.log")
}

fn prefs_path() -> PathBuf {
    state_dir().join("update-prefs.json")
}

/// The outcome of the last check, shown in Options > Updates (kept apart from the log so a
/// line such as "turned off" never poses as a check result).
fn status_path() -> PathBuf {
    state_dir().join("update-status.json")
}

/// Written just before the installer starts: `{"version": "...", "failed": bool}`. The next
/// start compares it with the running version to learn whether the install finished.
fn attempt_path() -> PathBuf {
    state_dir().join("update-attempt.json")
}

fn read_json(path: PathBuf) -> Option<serde_json::Value> {
    fs::read_to_string(path).ok().and_then(|raw| serde_json::from_str(&raw).ok())
}

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(|p| p.to_path_buf())
}

/// True when this exe was installed by the CasaPic setup program.
fn is_setup_install() -> bool {
    exe_dir().is_some_and(|d| d.join("uninstall.exe").is_file())
}

/// On unless the user turned it off (`{"auto": false}`).
fn auto_enabled() -> bool {
    read_json(prefs_path()).and_then(|v| v.get("auto").and_then(|a| a.as_bool())).unwrap_or(true)
}

fn write_auto_enabled(enabled: bool) -> Result<(), String> {
    fs::create_dir_all(state_dir()).map_err(|e| e.to_string())?;
    fs::write(prefs_path(), serde_json::json!({ "auto": enabled }).to_string()).map_err(|e| e.to_string())?;
    if auto_enabled() != enabled {
        return Err("The update setting did not save.".to_string());
    }
    Ok(())
}

// --- Log -------------------------------------------------------------------

/// Days since 1970-01-01 -> (year, month, day) (Howard Hinnant's civil_from_days).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// UTC timestamp, ISO 8601 ("2026-11-19T14:05:09Z"); the Options page shows it in local time.
fn utc_timestamp(secs: u64) -> String {
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    let rem = secs % 86_400;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, (rem % 3600) / 60, rem % 60)
}

fn now_utc() -> String {
    utc_timestamp(SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0))
}

/// Appends "[timestamp] message", keeping the newest half once the file passes
/// LOG_MAX_BYTES. Logging never breaks an update, so its errors are ignored.
fn log(msg: &str) {
    static LOG_LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOG_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let path = log_path();
    let _ = fs::create_dir_all(state_dir());
    if fs::metadata(&path).map(|m| m.len() > LOG_MAX_BYTES).unwrap_or(false)
        && let Ok(text) = fs::read_to_string(&path)
    {
        let lines: Vec<&str> = text.lines().collect();
        let _ = fs::write(&path, lines[lines.len() / 2..].join("\n") + "\n");
    }
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{}] {}", now_utc(), msg);
    }
}

/// Logs a check outcome AND makes it the "Last check" shown in Options.
fn record(msg: &str) {
    log(msg);
    let body = serde_json::json!({ "checked": now_utc(), "result": msg }).to_string();
    let tmp = status_path().with_extension("json.tmp");
    if fs::write(&tmp, body).is_ok() {
        let _ = fs::rename(&tmp, status_path());
    }
}

fn read_status() -> (Option<String>, Option<String>) {
    match read_json(status_path()) {
        Some(v) => (
            v.get("checked").and_then(|x| x.as_str()).map(str::to_string),
            v.get("result").and_then(|x| x.as_str()).map(str::to_string),
        ),
        None => (None, None),
    }
}

// --- Version checks ----------------------------------------------------------

/// Numeric parts of "1.2.3" (pre-release suffixes ignored).
fn version_parts(v: &str) -> Vec<u64> {
    v.trim_start_matches('v')
        .split(['-', '+'])
        .next()
        .unwrap_or("")
        .split('.')
        .map(|p| p.parse::<u64>().unwrap_or(0))
        .collect()
}

fn version_at_least(have: &str, want: &str) -> bool {
    let (a, b) = (version_parts(have), version_parts(want));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    true
}

/// The version a signature was made for, read from its TRUSTED comment (covered by minisign's
/// global signature, so only trustworthy after `download` verified it). Newer Tauri CLIs write
/// `version:X`; others only `file:<Product>_<X>_x64-setup.exe`.
fn signed_version(signature_b64: &str) -> Option<String> {
    use base64::Engine;
    let raw = base64::engine::general_purpose::STANDARD.decode(signature_b64.trim()).ok()?;
    let text = String::from_utf8(raw).ok()?;
    let comment = text.lines().find_map(|l| l.strip_prefix("trusted comment: "))?;
    let fields: Vec<&str> = comment.split('\t').collect();
    if let Some(v) = fields.iter().find_map(|f| f.strip_prefix("version:")) {
        return Some(v.trim().to_string());
    }
    let file = fields.iter().find_map(|f| f.strip_prefix("file:"))?;
    let stem = file.trim().strip_suffix("_x64-setup.exe")?;
    stem.rsplit_once('_').map(|(_, v)| v.to_string())
}

/// True for errors that mean "this file is not a genuine release" (not a network problem).
fn is_signature_error(e: &tauri_plugin_updater::Error) -> bool {
    use tauri_plugin_updater::Error as E;
    matches!(e, E::Minisign(_) | E::Base64(_) | E::SignatureUtf8(_) | E::SignedVersionMismatch { .. } | E::MissingSignedVersion)
}

// --- Install attempts ----------------------------------------------------------

fn failed_attempt() -> Option<String> {
    let v = read_json(attempt_path())?;
    if v.get("failed").and_then(|f| f.as_bool()).unwrap_or(false) {
        v.get("version").and_then(|x| x.as_str()).map(str::to_string)
    } else {
        None
    }
}

fn write_attempt(version: &str, failed: bool) {
    let _ = fs::create_dir_all(state_dir());
    let _ = fs::write(attempt_path(), serde_json::json!({ "version": version, "failed": failed }).to_string());
}

/// At start: did the last install finish? The installer reports nothing back, so the only
/// evidence is which version now runs. A failed version is remembered so the start-up check
/// does not close CasaPic again for it on every start.
fn reconcile_attempt(running: &str) {
    let Some(v) = read_json(attempt_path()) else { return };
    let Some(attempted) = v.get("version").and_then(|x| x.as_str()).map(str::to_string) else {
        let _ = fs::remove_file(attempt_path());
        return;
    };
    if version_at_least(running, &attempted) {
        let _ = fs::remove_file(attempt_path());
        record(&format!("Updated to version {running}."));
    } else if failed_attempt().is_none() {
        write_attempt(&attempted, true);
        record(&format!(
            "Installing {attempted} did not finish - still running {running}. It will not be retried automatically; use \"Check now\" and then \"Restart and install\" to try again."
        ));
    }
}

// --- Check / download / install --------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    /// Start-up check: install straight away, unless this version already failed once.
    Auto,
    /// "Restart and install": install now, also retrying a failed version.
    InstallNow,
    /// Download and verify, then wait for the user (daily check, "Check now").
    Stage,
}

/// An error's text with any closing full stops removed, so a message that continues after it
/// does not show "..". (Library errors sometimes end in a full stop, sometimes not.)
fn sentence(e: &impl std::fmt::Display) -> String {
    e.to_string().trim_end().trim_end_matches('.').to_string()
}

fn pending_version(state: &UpdateState) -> Option<String> {
    state.pending.lock().ok().and_then(|p| p.as_ref().map(|(u, _)| u.version.clone()))
}

fn install(update: &Update, bytes: &[u8]) -> Result<String, String> {
    log(&format!("Installing {} - CasaPic will restart.", update.version));
    write_attempt(&update.version, false);
    // On Windows a successful install() starts the installer and EXITS this process.
    match update.install(bytes) {
        Ok(()) => Ok(format!("Installing {}.", update.version)),
        Err(e) => {
            let msg = format!("Installing {} failed: {}. The installed version is untouched.", update.version, sentence(&e));
            write_attempt(&update.version, true);
            record(&msg);
            Err(msg)
        }
    }
}

async fn run_check(app: &AppHandle, mode: Mode) -> Result<String, String> {
    if !is_setup_install() {
        return Err(not_installed_reason());
    }
    let state = app.state::<UpdateState>();
    let _busy = BusyGuard::acquire(&state.busy).ok_or_else(|| "An update check is already running.".to_string())?;

    let staged = state.pending.lock().map_err(|e| e.to_string())?.take();
    if let Some((update, bytes)) = staged {
        if mode != Mode::Stage {
            let result = install(&update, &bytes);
            *state.pending.lock().map_err(|e| e.to_string())? = Some((update, bytes));
            return result;
        }
        let msg = format!("Version {} is downloaded and verified - restart to install.", update.version);
        *state.pending.lock().map_err(|e| e.to_string())? = Some((update, bytes));
        return Ok(msg);
    }

    let current = app.package_info().version.to_string();
    let updater = app.updater_builder().build().map_err(|e| {
        let msg = format!("Update check failed: {e}");
        record(&msg);
        msg
    })?;
    let update = match updater.check().await {
        Ok(Some(u)) => u,
        Ok(None) => {
            let msg = format!("Up to date (version {current}).");
            record(&msg);
            return Ok(msg);
        }
        Err(e) => {
            let msg = format!("Update check failed: {e}");
            record(&msg);
            return Err(msg);
        }
    };

    let version = update.version.clone();
    log(&format!("Version {version} available (running {current}) - downloading."));
    let bytes = match update.download(|_, _| {}, || {}).await {
        Ok(b) => b,
        Err(e) => {
            let msg = if is_signature_error(&e) {
                format!("Version {version} REFUSED - signature check failed: {}. The installed version is untouched.", sentence(&e))
            } else {
                format!("Downloading version {version} failed: {}. The installed version is untouched; it will try again later.", sentence(&e))
            };
            record(&msg);
            return Err(msg);
        }
    };
    match signed_version(&update.signature) {
        Some(signed) if signed == version => {}
        other => {
            let msg = format!(
                "Version {version} REFUSED - signature check failed: the installer was signed for version {}, not {version}. The installed version is untouched.",
                other.as_deref().unwrap_or("(none recorded)")
            );
            record(&msg);
            return Err(msg);
        }
    }
    log(&format!("Version {version} downloaded and signature verified ({} bytes).", bytes.len()));

    let stage = match mode {
        Mode::Stage => true,
        Mode::InstallNow => false,
        Mode::Auto => failed_attempt().as_deref() == Some(version.as_str()),
    };
    if !stage {
        return install(&update, &bytes);
    }
    *state.pending.lock().map_err(|e| e.to_string())? = Some((update, bytes));
    let msg = format!("Version {version} is ready - restart to install.");
    record(&msg);
    let _ = app.emit("update-ready", version);
    Ok(msg)
}

fn not_installed_reason() -> String {
    if cfg!(debug_assertions) {
        "Development build - updates apply only to an installed copy.".to_string()
    } else {
        "This copy was not installed with the CasaPic setup program, so it cannot update itself. Install the latest release from GitHub.".to_string()
    }
}

/// Called once from the library's `setup`: the start-up check, then the daily check.
pub fn start(app: &AppHandle) {
    if !is_setup_install() {
        return;
    }
    reconcile_attempt(&app.package_info().version.to_string());
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(START_DELAY);
        let mut mode = Mode::Auto;
        loop {
            let ok = if auto_enabled() { tauri::async_runtime::block_on(run_check(&handle, mode)).is_ok() } else { true };
            mode = Mode::Stage;
            std::thread::sleep(if ok { CHECK_INTERVAL } else { RETRY_INTERVAL });
        }
    });
}

// --- Commands ---------------------------------------------------------------

#[derive(Clone, serde::Serialize)]
pub struct UpdateStatus {
    running_version: String,
    /// False for dev builds and copies not installed by the setup program.
    can_update: bool,
    unavailable_reason: Option<String>,
    auto_update: bool,
    pending_version: Option<String>,
    checking: bool,
    last_checked: Option<String>,
    last_result: Option<String>,
}

#[tauri::command]
pub async fn update_status(app: AppHandle) -> Result<UpdateStatus, String> {
    let state = app.state::<UpdateState>();
    let can_update = is_setup_install();
    let (last_checked, last_result) = read_status();
    Ok(UpdateStatus {
        running_version: app.package_info().version.to_string(),
        can_update,
        unavailable_reason: (!can_update).then(not_installed_reason),
        auto_update: auto_enabled(),
        pending_version: pending_version(&state),
        checking: state.busy.load(Ordering::SeqCst),
        last_checked,
        last_result,
    })
}

#[tauri::command]
pub async fn update_set_auto(enabled: bool) -> Result<(), String> {
    write_auto_enabled(enabled)?;
    log(if enabled { "Automatic updates turned on." } else { "Automatic updates turned off." });
    Ok(())
}

/// "Check now": downloads and verifies a newer release, then waits for `update_install`.
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<String, String> {
    run_check(&app, Mode::Stage).await
}

/// "Restart and install": installs the downloaded update (or checks first).
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<String, String> {
    run_check(&app, Mode::InstallNow).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn continued_error_text_has_one_full_stop() {
        assert_eq!(super::sentence(&"different release."), "different release");
        assert_eq!(super::sentence(&"no stop"), "no stop");
        assert_eq!(super::sentence(&"two.. "), "two");
    }

    use super::*;

    #[test]
    fn civil_dates_match_known_days() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(20_776), (2026, 11, 19));
    }

    #[test]
    fn timestamp_is_iso_utc() {
        assert_eq!(utc_timestamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_timestamp(20_776 * 86_400 + 13 * 3600 + 5 * 60 + 9), "2026-11-19T13:05:09Z");
    }

    fn sig_with_comment(comment: &str) -> String {
        use base64::Engine;
        let text = format!("untrusted comment: signature from tauri secret key\nRUQAAAAA\ntrusted comment: {comment}\nAAAA\n");
        base64::engine::general_purpose::STANDARD.encode(text)
    }

    #[test]
    fn signed_version_comes_from_the_trusted_comment() {
        assert_eq!(signed_version(&sig_with_comment("timestamp:1795141467\tfile:CasaPic_0.2.0_x64-setup.exe")), Some("0.2.0".into()));
        assert_eq!(signed_version(&sig_with_comment("timestamp:1\tfile:Casa_E2E_0.2.1_x64-setup.exe")), Some("0.2.1".into()));
        assert_eq!(signed_version(&sig_with_comment("timestamp:1\tfile:x.exe\tversion:1.4.0")), Some("1.4.0".into()));
        assert_eq!(signed_version(&sig_with_comment("timestamp:1")), None);
        assert_eq!(signed_version(&sig_with_comment("timestamp:1\tfile:other.msi")), None);
        assert_eq!(signed_version("not base64 !!"), None);
    }

    #[test]
    fn version_ordering() {
        assert!(version_at_least("0.2.1", "0.2.1"));
        assert!(version_at_least("0.2.10", "0.2.9"));
        assert!(version_at_least("1.0.0", "0.9.9"));
        assert!(!version_at_least("0.2.0", "0.2.1"));
        assert!(!version_at_least("0.2", "0.2.1"));
        assert!(version_at_least("v0.3.0", "0.2.9"));
    }

    #[test]
    fn busy_guard_is_exclusive_and_released_on_drop() {
        let flag = AtomicBool::new(false);
        let first = BusyGuard::acquire(&flag);
        assert!(first.is_some());
        assert!(BusyGuard::acquire(&flag).is_none());
        drop(first);
        assert!(BusyGuard::acquire(&flag).is_some());
    }
}
