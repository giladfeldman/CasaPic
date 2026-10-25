//! Where CasaPic keeps its own data and which folders it watches.
//!
//! `CASAPIC_DATA` and `CASAPIC_ROOTS` (`;`-separated) override the defaults; development and
//! tests use them so that nothing touches the owner's real Pictures folder.

use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("CASAPIC_DATA") {
        return PathBuf::from(d);
    }
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("CasaPic")
}

pub fn roots() -> Vec<PathBuf> {
    if let Ok(r) = std::env::var("CASAPIC_ROOTS") {
        return r.split(';').map(str::trim).filter(|s| !s.is_empty()).map(PathBuf::from).collect();
    }
    known_pictures().into_iter().collect()
}

/// The user's Pictures folder as Windows reports it (follows OneDrive/relocation).
pub fn known_pictures() -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_Pictures, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_Pictures, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s.map(PathBuf::from)
    }
}

/// Where Picture Collage saves, like Picasa's "Pictures\Picasa\Collages" (lab check). Under
/// the data folder when a development or test override is set.
pub fn collage_dir() -> PathBuf {
    if std::env::var_os("CASAPIC_DATA").is_some() || std::env::var_os("CASAPIC_ROOTS").is_some() {
        data_dir().join("Collages")
    } else {
        known_pictures().unwrap_or_else(data_dir).join("CasaPic Collages")
    }
}

/// `dir/stem.ext`, or `stem-1.ext`, `stem-2.ext`… if that is taken.
pub fn unused_name(dir: &std::path::Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    (1..).map(|n| dir.join(format!("{stem}-{n}.{ext}"))).find(|p| !p.exists()).expect("a free name")
}

/// Default export folder, like Picasa's "Pictures\Picasa\Exports". When a development or
/// test override is set, exports go under the data folder instead, never the real Pictures.
pub fn export_dir(folder_name: &str) -> PathBuf {
    let base = if std::env::var_os("CASAPIC_DATA").is_some() || std::env::var_os("CASAPIC_ROOTS").is_some() {
        data_dir().join("Exports")
    } else {
        known_pictures().unwrap_or_else(data_dir).join("CasaPic Exports")
    };
    base.join(folder_name)
}
