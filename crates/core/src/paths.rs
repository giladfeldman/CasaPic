//! Where CasaPic keeps its own data and which folders it watches.
//!
//! `CASAPIC_DATA` and `CASAPIC_ROOTS` (`;`-separated) override the defaults; development and
//! tests use them so that nothing touches the owner's real Pictures folder.

use std::path::{Path, PathBuf};

fn local_app_data() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

/// The library: photo index, thumbnails and settings, one per Windows user. It sits in the
/// app's own data folder (named after its bundle id), which the uninstaller's "delete the
/// application data" box removes; the program itself is installed in `%LOCALAPPDATA%\CasaPic`.
pub fn data_dir() -> PathBuf {
    if let Some(d) = DATA_FALLBACK.get() {
        return d.clone();
    }
    if let Some(d) = std::env::var_os("CASAPIC_DATA") {
        return PathBuf::from(d);
    }
    local_app_data().join("org.casapic.app").join("library")
}

static DATA_FALLBACK: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Makes every part of CasaPic use `dir` as the library for the rest of this run (when the
/// library could not be copied from its old place).
pub fn use_data_dir(dir: PathBuf) {
    let _ = DATA_FALLBACK.set(dir);
}

/// Where the library was kept before the installer existed (now the program's folder).
pub fn old_data_dir() -> PathBuf {
    local_app_data().join("CasaPic")
}

/// The files of a library.
const LIBRARY_FILES: [&str; 4] = ["index.db", "index.db-wal", "index.db-shm", "settings.json"];

/// Copies a library kept in the old place to the new one, once: only when the new place has
/// no library yet. The copy is made beside the new place and renamed into it when complete,
/// so a failure part-way leaves no half library. The old files stay where they were, as a
/// backup. Returns whether a library was copied.
pub fn move_old_data(old: &Path, new: &Path) -> std::io::Result<bool> {
    if !old.join("index.db").is_file() || new.join("index.db").exists() {
        return Ok(false);
    }
    if new.exists() && std::fs::read_dir(new)?.next().is_some() {
        // something else is already there: leave both alone rather than mix them
        return Ok(false);
    }
    let parent = new.parent().unwrap_or(new);
    std::fs::create_dir_all(parent)?;
    let part = parent.join(format!("{}.partial", new.file_name().and_then(|n| n.to_str()).unwrap_or("library")));
    if part.exists() {
        std::fs::remove_dir_all(&part)?;
    }
    std::fs::create_dir(&part)?;
    let copied = LIBRARY_FILES.iter().filter(|f| old.join(f).is_file()).try_for_each(|f| {
        let (from, to) = (old.join(f), part.join(f));
        std::fs::copy(&from, &to)?;
        if std::fs::metadata(&to)?.len() != std::fs::metadata(&from)?.len() {
            return Err(std::io::Error::other(format!("{} did not copy whole", from.display())));
        }
        Ok(())
    });
    if let Err(e) = copied {
        let _ = std::fs::remove_dir_all(&part);
        return Err(e);
    }
    if new.exists() {
        std::fs::remove_dir(new)?; // empty, checked above
    }
    std::fs::rename(&part, new)?;
    Ok(true)
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

/// `dir/stem.ext`, or `stem-001.ext`, `stem-002.ext`… if that is taken.
pub fn unused_name(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    numbered_name(dir, stem, ext)
}

/// The first free `dir/stem-001.ext`, `stem-002.ext`…: Picasa 3.9's names for a copy (Save a
/// Copy, and Export into a folder that already has the name; checked in the sealed lab
/// 2026-11-20). CasaPic's one rule for every new file whose name is taken.
pub fn numbered_name(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    (1..).map(|n| dir.join(format!("{stem}-{n:03}.{ext}"))).find(|p| !p.exists()).expect("a free name")
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

#[cfg(test)]
mod move_tests {
    use super::*;

    /// Picasa 3.9 names a copy `a-001.jpg`, then `a-002.jpg` (sealed lab, 2026-11-20); every
    /// CasaPic path that needs a free name (paste, Save a Copy, Export, collage) goes through here.
    #[test]
    fn copies_are_numbered_as_picasa_numbers_them() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        assert_eq!(unused_name(d, "a", "jpg"), d.join("a.jpg"));
        std::fs::write(d.join("a.jpg"), b"x").unwrap();
        assert_eq!(unused_name(d, "a", "jpg"), d.join("a-001.jpg"));
        assert_eq!(numbered_name(d, "a", "jpg"), d.join("a-001.jpg"));
        std::fs::write(d.join("a-001.jpg"), b"x").unwrap();
        assert_eq!(numbered_name(d, "a", "jpg"), d.join("a-002.jpg"), "a second copy is -002, never a-001-001");
        assert_eq!(crate::files::free_name(d, "a.jpg"), "a-002.jpg");
        assert_eq!(crate::files::free_name(d, "b.jpg"), "b.jpg");
        std::fs::write(d.join("noext"), b"x").unwrap();
        assert_eq!(crate::files::free_name(d, "noext"), "noext-001");
    }

    #[test]
    fn old_library_is_copied_once_and_kept() {
        let t = tempfile::tempdir().unwrap();
        let (old, new) = (t.path().join("CasaPic"), t.path().join("org.casapic.app").join("library"));
        // nothing to copy: no old library
        assert!(!move_old_data(&old, &new).unwrap());
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("index.db"), vec![1u8; 300_000]).unwrap();
        std::fs::write(old.join("index.db-wal"), b"wal").unwrap();
        std::fs::write(old.join("settings.json"), b"{}").unwrap();
        std::fs::write(old.join("casapic.exe"), b"program").unwrap();
        assert!(move_old_data(&old, &new).unwrap());
        assert_eq!(std::fs::read(new.join("index.db")).unwrap().len(), 300_000);
        assert_eq!(std::fs::read(new.join("index.db-wal")).unwrap(), b"wal");
        assert_eq!(std::fs::read(new.join("settings.json")).unwrap(), b"{}");
        assert!(!new.join("casapic.exe").exists(), "program files are not library files");
        assert!(old.join("index.db").is_file(), "the old library stays as a backup");
        assert!(!t.path().join("org.casapic.app").join("library.partial").exists());
        // once there, never again: a changed new library is not overwritten
        std::fs::write(new.join("index.db"), b"newer").unwrap();
        assert!(!move_old_data(&old, &new).unwrap());
        assert_eq!(std::fs::read(new.join("index.db")).unwrap(), b"newer");
    }

    #[test]
    fn a_folder_with_other_things_is_left_alone() {
        let t = tempfile::tempdir().unwrap();
        let (old, new) = (t.path().join("old"), t.path().join("new"));
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(old.join("index.db"), b"db").unwrap();
        std::fs::write(new.join("something"), b"x").unwrap();
        assert!(!move_old_data(&old, &new).unwrap());
        assert!(!new.join("index.db").exists());
        // an empty new folder (made by an earlier start) is filled
        std::fs::remove_file(new.join("something")).unwrap();
        assert!(move_old_data(&old, &new).unwrap());
        assert_eq!(std::fs::read(new.join("index.db")).unwrap(), b"db");
    }
}
