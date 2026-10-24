//! Save and Revert: the one place CasaPic changes a photo file, and only when asked.
//!
//! Save writes the edited picture over the photo (full size, JPEG, the original's camera data
//! kept) after moving the untouched original into the hidden `.casapicoriginals` folder beside
//! it, Picasa's pattern. That folder's own `.casapic.ini` keeps the recipe that was saved, so the
//! edits stay on record. Saving again keeps the first original: the backup is never replaced.
//! Revert moves the original back, byte for byte, and clears the saved recipe.
//!
//! Order of steps, so a failure at any point leaves a complete photo in place: the edited file is
//! written to a temporary name first; only then is the original moved (a rename on the same
//! disk, so its bytes are never rewritten); a failed final rename moves it back.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::export::{ExportOptions, render_jpeg};
use crate::recipe::{self, Op};
use crate::sidecar;

pub const ORIGINALS: &str = ".casapicoriginals";

/// JPEG quality of a saved photo (exports let the user choose; Save keeps as much as it can).
const QUALITY: u8 = 95;

fn originals_dir(folder: &Path) -> PathBuf {
    folder.join(ORIGINALS)
}

/// Where the untouched original of `folder/file_name` is kept once it has been saved.
pub fn backup_of(folder: &Path, file_name: &str) -> PathBuf {
    originals_dir(folder).join(file_name)
}

/// Whether the photo has a saved edit that Revert can undo.
pub fn is_saved(folder: &Path, file_name: &str) -> bool {
    backup_of(folder, file_name).is_file()
}

/// Writes `ops` into the photo itself. The photo's sidecar recipe is cleared (its edits are now
/// in the pixels); the full recipe applied to the original is kept with the backup.
pub fn save(folder: &Path, file_name: &str, orientation: i64, ops: &[Op]) -> Result<()> {
    if ops.is_empty() {
        bail!("nothing to save: the photo has no edits");
    }
    let photo = folder.join(file_name);
    let jpeg = render_jpeg(&photo, orientation, ops, &ExportOptions { max_edge: None, quality: QUALITY })?;
    let first_save = install(folder, file_name, &jpeg)?;

    // the recipe that turns the original into the saved picture
    let before = if first_save { String::new() } else { sidecar::read(&originals_dir(folder)).remove(file_name).unwrap_or_default().filters };
    let total = format!("{before}{}", recipe::to_string(ops));
    sidecar::update(&originals_dir(folder), file_name, |e| e.filters = total)?;
    sidecar::update(folder, file_name, |e| e.filters.clear())?;
    Ok(())
}

/// Replaces the photo file with `bytes`, keeping the untouched original first (only the first
/// time: a kept original is never replaced). Returns whether this was the first change.
/// Used by Save and by Adjust Date and Time.
pub fn install(folder: &Path, file_name: &str, bytes: &[u8]) -> Result<bool> {
    let photo = folder.join(file_name);
    let backup = backup_of(folder, file_name);
    let tmp = folder.join(format!(".{file_name}.casapic-save"));
    std::fs::write(&tmp, bytes).with_context(|| format!("write {}", tmp.display()))?;

    let first = !backup.exists();
    if first {
        let dir = originals_dir(folder);
        std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        sidecar::set_hidden(&dir, true);
        if let Err(e) = std::fs::rename(&photo, &backup) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e).with_context(|| format!("move the original to {}", backup.display()));
        }
    }
    if let Err(e) = std::fs::rename(&tmp, &photo) {
        if first {
            let _ = std::fs::rename(&backup, &photo);
        }
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("replace {}", photo.display()));
    }
    Ok(first)
}

/// Puts the original back (byte for byte) and forgets the saved edits. Returns false when the
/// photo was never saved. Removes the originals folder once it holds nothing else.
pub fn revert(folder: &Path, file_name: &str) -> Result<bool> {
    let photo = folder.join(file_name);
    let backup = backup_of(folder, file_name);
    if !backup.is_file() {
        return Ok(false);
    }
    // rename replaces the saved file in one step on the same disk
    std::fs::rename(&backup, &photo).with_context(|| format!("restore {}", photo.display()))?;
    let dir = originals_dir(folder);
    sidecar::update(&dir, file_name, |e| e.filters.clear())?;
    sidecar::update(folder, file_name, |e| e.filters.clear())?;
    if std::fs::read_dir(&dir).map(|mut d| d.next().is_none()).unwrap_or(false) {
        sidecar::set_hidden(&dir, false);
        let _ = std::fs::remove_dir(&dir);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo(dir: &Path, name: &str) -> Vec<u8> {
        let img = image::RgbImage::from_fn(64, 48, |x, y| image::Rgb([(x * 4) as u8, (y * 5) as u8, 90]));
        let bytes = crate::thumbs::encode_jpeg(&img, 90).unwrap();
        std::fs::write(dir.join(name), &bytes).unwrap();
        bytes
    }

    #[test]
    fn save_then_revert_restores_the_original_byte_for_byte() {
        let dir = std::env::temp_dir().join(format!("casapic-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let original = photo(&dir, "a.jpg");
        let other = photo(&dir, "b.jpg");
        sidecar::update(&dir, "a.jpg", |e| {
            e.star = true;
            e.filters = "bw=1;".into()
        })
        .unwrap();

        save(&dir, "a.jpg", 1, &recipe::parse("bw=1;")).unwrap();
        let saved = std::fs::read(dir.join("a.jpg")).unwrap();
        assert_ne!(saved, original, "the photo now carries the edit");
        let img = image::load_from_memory(&saved).unwrap().to_rgb8();
        assert!(img.pixels().all(|p| p[0].abs_diff(p[1]) <= 3 && p[1].abs_diff(p[2]) <= 3), "saved picture is black and white");
        assert_eq!(std::fs::read(backup_of(&dir, "a.jpg")).unwrap(), original, "backup is the untouched original");
        let side = sidecar::read(&dir);
        assert!(side["a.jpg"].star && side["a.jpg"].filters.is_empty(), "star kept, recipe now in the pixels");
        assert_eq!(sidecar::read(&dir.join(ORIGINALS))["a.jpg"].filters, "bw=1;");

        // a second save keeps the first original and records both edits
        save(&dir, "a.jpg", 1, &recipe::parse("sepia=1;")).unwrap();
        assert_eq!(std::fs::read(backup_of(&dir, "a.jpg")).unwrap(), original);
        assert_eq!(sidecar::read(&dir.join(ORIGINALS))["a.jpg"].filters, "bw=1;sepia=1;");

        assert!(revert(&dir, "a.jpg").unwrap());
        assert_eq!(std::fs::read(dir.join("a.jpg")).unwrap(), original, "Revert restores byte for byte");
        assert!(!dir.join(ORIGINALS).exists(), "empty originals folder removed");
        assert!(sidecar::read(&dir)["a.jpg"].star);
        assert!(!revert(&dir, "a.jpg").unwrap(), "nothing left to revert");
        assert_eq!(std::fs::read(dir.join("b.jpg")).unwrap(), other, "other photos untouched");
        let names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into()).collect();
        assert!(!names.iter().any(|n| n.contains("casapic-save")), "no temporary files left: {names:?}");
        assert!(save(&dir, "a.jpg", 1, &[]).is_err(), "saving without edits is refused");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
