//! Which folders make up the library (Picasa's Folder Manager), the Options dialog, and
//! folder descriptions. The rules live in `settings.json` in CasaPic's data folder
//! (core `settings.rs`); a folder description lives in that folder's `.casapic.ini`.

use std::path::{Path, PathBuf};

use casapic_core::settings::{Options, Rule, Scan, Settings};
use casapic_core::{scan, sidecar, watch};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use super::{App, Job, Res, emit_status, err, mark};

/// Rescans one folder if it is in the library (scanned from its start point).
pub(crate) fn rescan_folder(app: &App, dir: &Path) {
    let start = app.settings.lock().ok().and_then(|s| s.start_of(dir));
    if let Some(start) = start.filter(|s| !scan::skipped_below(dir, s)) {
        let _ = scan::scan_folder(&app.db, dir, &start.to_string_lossy());
    }
}

/// Brings the index in line with the settings, then (re)starts watching the Scan Always folders.
pub(crate) fn sync_library(h: &AppHandle, rescan_once: bool) {
    let app = h.state::<App>();
    let settings = app.settings.lock().map(|s| s.clone()).unwrap_or_default();
    scan::set_file_types(&settings.options.file_types);
    emit_status(h, "Scanning folders…");
    if let Err(e) = scan::sync(&app.db, &settings, rescan_once) {
        emit_status(h, &format!("Scanning failed: {e}"));
    }
    let _ = h.emit("library-changed", ());
    emit_status(h, "");
    mark("scan-done");
    app.queue(Job::Pending);
    let watched: Vec<PathBuf> =
        settings.scan_starts().into_iter().filter(|(p, s)| *s == Scan::Always && p.is_dir()).map(|(p, _)| p).collect();
    let h2 = h.clone();
    // the old watcher stops when replaced
    *app.watcher.lock().expect("watcher lock") = None;
    match watch::start(&watched, move |dirs| on_folders_changed(&h2, dirs)) {
        Ok(w) => *app.watcher.lock().expect("watcher lock") = Some(w),
        Err(e) => emit_status(h, &format!("Not watching for changes: {e}")),
    }
}

/// Rescans folders the watcher reported; drops folders that no longer exist.
fn on_folders_changed(h: &AppHandle, dirs: Vec<PathBuf>) {
    let app = h.state::<App>();
    for d in &dirs {
        if d.is_dir() {
            rescan_folder(&app, d);
        }
        if let Ok(folders) = app.db.folders() {
            for f in folders {
                let fp = Path::new(&f.path);
                if fp.starts_with(d) && !fp.is_dir() {
                    let _ = app.db.remove_folder_tree(&f.path);
                }
            }
        }
    }
    let _ = h.emit("library-changed", ());
    app.queue(Job::Pending);
}

#[tauri::command]
pub(crate) fn settings_get(app: State<'_, App>) -> Res<Settings> {
    Ok(app.settings.lock().map_err(err)?.clone())
}

#[derive(Serialize)]
pub(crate) struct FmNode {
    name: String,
    path: String,
    /// has sub-folders (the tree shows an expand arrow)
    more: bool,
}

fn sub_folders(dir: &Path) -> Vec<FmNode> {
    let mut out: Vec<FmNode> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.starts_with('.') || name.starts_with('$') || is_hidden(&e.path()) {
                        return None;
                    }
                    let p = e.path();
                    let more = std::fs::read_dir(&p).is_ok_and(|mut r| r.any(|x| x.is_ok_and(|x| x.file_type().is_ok_and(|t| t.is_dir()))));
                    Some(FmNode { name, path: p.to_string_lossy().to_string(), more })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|n| n.name.to_lowercase());
    out
}

fn is_hidden(p: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    // hidden or system (Windows' own folders such as "My Music" links in Documents)
    std::fs::metadata(p).is_ok_and(|m| m.file_attributes() & 0x6 != 0)
}

/// The Folder Manager's tree: with no `path`, the top level (your user folders, then every
/// drive); otherwise the sub-folders of `path`.
#[tauri::command(async)]
pub(crate) fn fm_list(path: Option<String>) -> Res<Vec<FmNode>> {
    if let Some(p) = path {
        return Ok(sub_folders(Path::new(&p)));
    }
    let mut top = Vec::new();
    if let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
        let pictures = casapic_core::paths::known_pictures();
        let mut special: Vec<PathBuf> = pictures.into_iter().collect();
        for n in ["Desktop", "Documents", "Downloads", "Videos"] {
            special.push(home.join(n));
        }
        for p in special.into_iter().filter(|p| p.is_dir()) {
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            top.push(FmNode { more: !sub_folders(&p).is_empty(), name, path: p.to_string_lossy().to_string() });
        }
    }
    for letter in b'A'..=b'Z' {
        let root = format!("{}:\\", letter as char);
        if Path::new(&root).is_dir() {
            top.push(FmNode { name: format!("Drive ({}:)", letter as char), path: root, more: true });
        }
    }
    Ok(top)
}

/// The Folder Manager's OK: stores the marks, then rescans in the background.
#[tauri::command]
pub(crate) fn fm_save(h: AppHandle, app: State<'_, App>, rules: Vec<Rule>) -> Res<()> {
    {
        let mut s = app.settings.lock().map_err(err)?;
        s.folders = Vec::new();
        for r in rules {
            s.set(&r.path, Some(r.scan));
        }
        s.folders_confirmed = true;
        s.save().map_err(err)?;
    }
    std::thread::spawn(move || sync_library(&h, true));
    Ok(())
}

/// Closing the first-launch Folder Manager without changes still counts as confirming it.
#[tauri::command]
pub(crate) fn fm_confirm(app: State<'_, App>) -> Res<()> {
    let mut s = app.settings.lock().map_err(err)?;
    if !s.folders_confirmed {
        s.folders_confirmed = true;
        s.save().map_err(err)?;
    }
    Ok(())
}

/// File > Add Folder to CasaPic: marks one folder Scan Always and scans it.
#[tauri::command]
pub(crate) fn add_folder(h: AppHandle, app: State<'_, App>, path: String) -> Res<()> {
    let p = PathBuf::from(&path);
    if !p.is_dir() {
        return Err(format!("not a folder: {path}"));
    }
    {
        let mut s = app.settings.lock().map_err(err)?;
        s.set(&p, Some(Scan::Always));
        s.folders_confirmed = true;
        s.save().map_err(err)?;
    }
    std::thread::spawn(move || sync_library(&h, false));
    Ok(())
}

/// Folder > Remove from CasaPic: marks the folder Remove (nothing on disk changes).
#[tauri::command]
pub(crate) fn remove_folder(h: AppHandle, app: State<'_, App>, path: String) -> Res<()> {
    {
        let mut s = app.settings.lock().map_err(err)?;
        s.set(Path::new(&path), Some(Scan::Remove));
        s.save().map_err(err)?;
    }
    std::thread::spawn(move || sync_library(&h, false));
    Ok(())
}

/// The Options dialog's OK. A change of file types rescans the library.
#[tauri::command]
pub(crate) fn options_save(h: AppHandle, app: State<'_, App>, options: Options) -> Res<()> {
    let types_changed = {
        let mut s = app.settings.lock().map_err(err)?;
        let changed = s.options.file_types != options.file_types;
        s.options = options;
        s.save().map_err(err)?;
        changed
    };
    if types_changed {
        std::thread::spawn(move || sync_library(&h, true));
    }
    Ok(())
}

/// Folder > Sort By: "" (date), "name" or "size", "-" in front for reversed; kept in the
/// folder's `.casapic.ini` like the description.
#[tauri::command(async)]
pub(crate) fn set_folder_sort(h: AppHandle, app: State<'_, App>, path: String, sort: String) -> Res<()> {
    let ok = matches!(sort.trim_start_matches('-'), "" | "date" | "name" | "size");
    if !ok {
        return Err(format!("unknown order: {sort}"));
    }
    let dir = PathBuf::from(&path);
    let v = if sort.trim_start_matches('-') == "date" { sort.replace("date", "") } else { sort };
    sidecar::update(&dir, sidecar::FOLDER_SECTION, |e| e.set("sortby", &v)).map_err(err)?;
    rescan_folder(&app, &dir);
    let _ = h.emit("library-changed", ());
    Ok(())
}

/// Folder > Edit Description: kept in the folder's `.casapic.ini` ([Picasa] description=).
#[tauri::command(async)]
pub(crate) fn set_folder_description(h: AppHandle, app: State<'_, App>, path: String, text: String) -> Res<()> {
    let dir = PathBuf::from(&path);
    sidecar::update(&dir, sidecar::FOLDER_SECTION, |e| e.set("description", text.trim())).map_err(err)?;
    rescan_folder(&app, &dir);
    let _ = h.emit("library-changed", ());
    Ok(())
}

/// Folder > Locate on Disk: opens the folder in Explorer.
#[tauri::command(async)]
pub(crate) fn open_folder(path: String) -> Res<()> {
    if !Path::new(&path).is_dir() {
        return Err(format!("not a folder: {path}"));
    }
    std::process::Command::new("explorer").arg(&path).spawn().map_err(err)?;
    Ok(())
}
