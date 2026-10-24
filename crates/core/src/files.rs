//! Photo and folder file actions: Rename, Delete (to the Windows Recycle Bin), Move and Copy.
//!
//! A photo is more than its file: its `.casapic.ini` entry (star, edits, caption) and, once it
//! has been saved, its untouched original in `.casapicoriginals` with that folder's own entry.
//! Every action here carries all of these together, so a photo never loses its edits or its
//! original by being renamed, moved or copied.
//!
//! Names never collide: a photo arriving where its name is taken becomes `name-1.jpg`,
//! `name-2.jpg`, ... (the same rule as Export). Nothing is ever overwritten.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::save::{ORIGINALS, backup_of};
use crate::sidecar;

/// A name for `file_name` that is free in `dir`: the name itself, else `stem-1.ext`, ...
pub fn free_name(dir: &Path, file_name: &str) -> String {
    if !dir.join(file_name).exists() {
        return file_name.to_string();
    }
    let p = Path::new(file_name);
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (1..).map(|n| format!("{stem}-{n}{ext}")).find(|n| !dir.join(n).exists()).expect("a free name")
}

/// Checks a name typed by the user for a file or folder (no path, nothing Windows refuses).
pub fn check_name(name: &str) -> Result<()> {
    let n = name.trim();
    if n.is_empty() || n == "." || n == ".." {
        bail!("the name is empty");
    }
    if let Some(c) = n.chars().find(|c| r#"\/:*?"<>|"#.contains(*c) || c.is_control()) {
        bail!("a name cannot contain {c:?}");
    }
    if n.ends_with('.') || n.ends_with(' ') {
        bail!("a name cannot end with a dot or a space");
    }
    Ok(())
}

/// Copies or moves a file; a move to another disk becomes copy-then-remove.
fn place(src: &Path, dst: &Path, keep_source: bool) -> Result<()> {
    if keep_source {
        std::fs::copy(src, dst).with_context(|| format!("copy {} to {}", src.display(), dst.display()))?;
        return Ok(());
    }
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    std::fs::copy(src, dst).with_context(|| format!("copy {} to {}", src.display(), dst.display()))?;
    if let Err(e) = std::fs::remove_file(src) {
        let _ = std::fs::remove_file(dst);
        return Err(e).with_context(|| format!("remove {} after copying it", src.display()));
    }
    Ok(())
}

/// Moves (or copies) a photo with its sidecar entry and saved original to `to_dir/to_name`.
/// The caller makes sure `to_name` is free.
fn carry(from_dir: &Path, name: &str, to_dir: &Path, to_name: &str, keep_source: bool) -> Result<()> {
    let src = from_dir.join(name);
    let dst = to_dir.join(to_name);
    if dst.exists() {
        bail!("{} already exists", dst.display());
    }
    place(&src, &dst, keep_source)?;

    let backup = backup_of(from_dir, name);
    if backup.is_file() {
        let odir = to_dir.join(ORIGINALS);
        std::fs::create_dir_all(&odir).with_context(|| format!("create {}", odir.display()))?;
        sidecar::set_hidden(&odir, true);
        place(&backup, &odir.join(to_name), keep_source)?;
        let entry = sidecar::read(&from_dir.join(ORIGINALS)).remove(name).unwrap_or_default();
        sidecar::update(&odir, to_name, |e| *e = entry)?;
        if !keep_source {
            sidecar::update(&from_dir.join(ORIGINALS), name, |e| *e = Default::default())?;
            remove_if_empty(&from_dir.join(ORIGINALS));
        }
    }

    let entry = sidecar::read(from_dir).remove(name).unwrap_or_default();
    sidecar::update(to_dir, to_name, |e| *e = entry)?;
    if !keep_source {
        sidecar::update(from_dir, name, |e| *e = Default::default())?;
    }
    Ok(())
}

fn remove_if_empty(dir: &Path) {
    if std::fs::read_dir(dir).map(|mut d| d.next().is_none()).unwrap_or(false) {
        sidecar::set_hidden(dir, false);
        let _ = std::fs::remove_dir(dir);
    }
}

/// Renames a photo in its folder. The extension is kept when the new name leaves it out.
/// Returns the new file name.
pub fn rename_photo(dir: &Path, name: &str, new_name: &str) -> Result<String> {
    check_name(new_name)?;
    let mut new_name = new_name.trim().to_string();
    let ext = Path::new(name).extension().map(|e| e.to_string_lossy().to_lowercase());
    let new_ext = Path::new(&new_name).extension().map(|e| e.to_string_lossy().to_lowercase());
    if let Some(ext) = ext
        && new_ext.as_deref() != Some(ext.as_str())
    {
        new_name = format!("{new_name}.{}", Path::new(name).extension().unwrap().to_string_lossy());
    }
    if new_name == name {
        return Ok(new_name);
    }
    // a change of case only ("img.jpg" -> "IMG.jpg") is the same file on Windows
    if new_name.eq_ignore_ascii_case(name) {
        let tmp = format!(".{name}.casapic-rename");
        carry(dir, name, dir, &tmp, false)?;
        carry(dir, &tmp, dir, &new_name, false)?;
        return Ok(new_name);
    }
    if dir.join(&new_name).exists() {
        bail!("there is already a file called {new_name} in this folder");
    }
    carry(dir, name, dir, &new_name, false)?;
    Ok(new_name)
}

/// Moves or copies a photo into another folder. Returns its name there (made free if needed).
pub fn transfer_photo(dir: &Path, name: &str, to_dir: &Path, copy: bool) -> Result<String> {
    if !to_dir.is_dir() {
        bail!("not a folder: {}", to_dir.display());
    }
    let same = dir.canonicalize().ok() == to_dir.canonicalize().ok();
    if same && !copy {
        return Ok(name.to_string());
    }
    let to_name = free_name(to_dir, name);
    carry(dir, name, to_dir, &to_name, copy)?;
    Ok(to_name)
}

/// Sends a photo to the Windows Recycle Bin, with its saved original; its sidecar entry goes.
pub fn delete_photo(dir: &Path, name: &str) -> Result<()> {
    let photo = dir.join(name);
    trash::delete(&photo).with_context(|| format!("move {} to the Recycle Bin", photo.display()))?;
    let backup = backup_of(dir, name);
    if backup.is_file() {
        trash::delete(&backup).with_context(|| format!("move {} to the Recycle Bin", backup.display()))?;
        sidecar::update(&dir.join(ORIGINALS), name, |e| *e = Default::default())?;
        remove_if_empty(&dir.join(ORIGINALS));
    }
    sidecar::update(dir, name, |e| *e = Default::default())?;
    Ok(())
}

/// Sends a whole folder (with everything in it) to the Recycle Bin.
pub fn delete_folder(dir: &Path) -> Result<()> {
    if !dir.is_dir() {
        bail!("not a folder: {}", dir.display());
    }
    trash::delete(dir).with_context(|| format!("move {} to the Recycle Bin", dir.display()))
}

/// Renames a folder in place. Returns the new path.
pub fn rename_folder(dir: &Path, new_name: &str) -> Result<PathBuf> {
    check_name(new_name)?;
    let parent = dir.parent().context("a drive cannot be renamed")?;
    let to = parent.join(new_name.trim());
    if to.exists() && !to.to_string_lossy().eq_ignore_ascii_case(&dir.to_string_lossy()) {
        bail!("there is already a folder called {}", new_name.trim());
    }
    std::fs::rename(dir, &to).with_context(|| format!("rename {}", dir.display()))?;
    Ok(to)
}

/// Moves a folder (with everything in it) into another folder. Returns its new path.
pub fn move_folder(dir: &Path, into: &Path) -> Result<PathBuf> {
    let name = dir.file_name().context("a drive cannot be moved")?;
    let to = into.join(name);
    if to.exists() {
        bail!("{} already has a folder called {}", into.display(), name.to_string_lossy());
    }
    if into.canonicalize()?.starts_with(dir.canonicalize()?) {
        bail!("a folder cannot be moved into itself");
    }
    std::fs::rename(dir, &to).with_context(|| format!("move {} into {} (moving between disks is not supported yet)", dir.display(), into.display()))?;
    Ok(to)
}

/// Makes a new, empty folder.
pub fn new_folder(parent: &Path, name: &str) -> Result<PathBuf> {
    check_name(name)?;
    let to = parent.join(name.trim());
    if to.exists() {
        bail!("there is already a folder called {}", name.trim());
    }
    std::fs::create_dir(&to).with_context(|| format!("create {}", to.display()))?;
    Ok(to)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let a = t.path().join("a");
        let b = t.path().join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        (t, a, b)
    }

    /// a photo with a star, an edit and a saved original (as Save leaves it)
    fn saved_photo(dir: &Path, name: &str) {
        std::fs::write(dir.join(name), b"edited picture").unwrap();
        let odir = dir.join(ORIGINALS);
        std::fs::create_dir_all(&odir).unwrap();
        std::fs::write(odir.join(name), b"untouched original").unwrap();
        sidecar::update(&odir, name, |e| e.filters = "bw=1;".into()).unwrap();
        sidecar::update(dir, name, |e| {
            e.star = true;
            e.filters = "sepia=1;".into();
        })
        .unwrap();
    }

    fn assert_carried(dir: &Path, name: &str) {
        assert_eq!(std::fs::read(dir.join(name)).unwrap(), b"edited picture");
        assert_eq!(std::fs::read(dir.join(ORIGINALS).join(name)).unwrap(), b"untouched original");
        let e = sidecar::read(dir).remove(name).unwrap();
        assert!(e.star);
        assert_eq!(e.filters, "sepia=1;");
        assert_eq!(sidecar::read(&dir.join(ORIGINALS)).remove(name).unwrap().filters, "bw=1;");
    }

    #[test]
    fn rename_carries_star_edits_and_original() {
        let (_t, a, _) = setup();
        saved_photo(&a, "x.jpg");
        assert_eq!(rename_photo(&a, "x.jpg", "beach").unwrap(), "beach.jpg", "extension kept");
        assert_carried(&a, "beach.jpg");
        assert!(!a.join("x.jpg").exists());
        assert!(sidecar::read(&a).get("x.jpg").is_none());
        assert_eq!(rename_photo(&a, "beach.jpg", "Beach.jpg").unwrap(), "Beach.jpg", "case-only rename");
        assert_carried(&a, "Beach.jpg");
        std::fs::write(a.join("other.jpg"), b"o").unwrap();
        assert!(rename_photo(&a, "Beach.jpg", "other.jpg").is_err(), "never overwrites");
        assert!(rename_photo(&a, "Beach.jpg", "a/b").is_err());
    }

    #[test]
    fn move_and_copy_carry_everything_and_never_overwrite() {
        let (_t, a, b) = setup();
        saved_photo(&a, "x.jpg");
        std::fs::write(b.join("x.jpg"), b"someone else").unwrap();
        let n = transfer_photo(&a, "x.jpg", &b, true).unwrap();
        assert_eq!(n, "x-1.jpg");
        assert_carried(&b, "x-1.jpg");
        assert_carried(&a, "x.jpg");
        assert_eq!(std::fs::read(b.join("x.jpg")).unwrap(), b"someone else");

        let n = transfer_photo(&a, "x.jpg", &b, false).unwrap();
        assert_eq!(n, "x-2.jpg");
        assert_carried(&b, "x-2.jpg");
        assert!(!a.join("x.jpg").exists());
        assert!(!a.join(ORIGINALS).exists(), "the empty originals folder goes");
        assert!(!sidecar::path_for(&a).exists(), "the empty sidecar goes");
    }

    #[test]
    fn folder_actions() {
        let (_t, a, b) = setup();
        std::fs::write(a.join("p.jpg"), b"p").unwrap();
        let n = new_folder(&a, "Sub").unwrap();
        assert!(n.is_dir());
        assert!(new_folder(&a, "Sub").is_err());
        let r = rename_folder(&n, "Renamed").unwrap();
        assert!(r.is_dir() && !n.exists());
        let m = move_folder(&r, &b).unwrap();
        assert_eq!(m, b.join("Renamed"));
        assert!(move_folder(&b, &m).is_err(), "not into itself");
    }

    /// Delete really goes to the Recycle Bin: the file can be restored from there byte for byte.
    #[test]
    fn delete_goes_to_the_recycle_bin_and_can_be_restored() {
        let (_t, a, _) = setup();
        let name = format!("casapic-test-{}.jpg", std::process::id());
        std::fs::write(a.join(&name), b"restore me").unwrap();
        sidecar::update(&a, &name, |e| e.star = true).unwrap();
        // as in the app: the image decoder has already set this thread's COM mode
        let _ = crate::wic::dimensions(&a.join(&name));
        delete_photo(&a, &name).unwrap();
        assert!(!a.join(&name).exists());
        assert!(sidecar::read(&a).get(&name).is_none());
        let items: Vec<_> = trash::os_limited::list().unwrap().into_iter().filter(|i| i.name == name.as_str()).collect();
        assert_eq!(items.len(), 1, "in the Recycle Bin");
        trash::os_limited::restore_all(items).unwrap();
        assert_eq!(std::fs::read(a.join(&name)).unwrap(), b"restore me");
    }
}
