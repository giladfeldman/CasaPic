//! Folder scanning. Never writes to the photo folders; only reads them.
//!
//! Unchanged files (same size and modified time as in the index) are skipped, so a repeat
//! scan of a large library costs one directory listing per folder.

use std::path::{Path, PathBuf};
use std::time::{Instant, UNIX_EPOCH};

use anyhow::Result;
use rayon::prelude::*;
use serde::Serialize;

use crate::db::{Db, PhotoInfo};

pub const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "jpe", "png", "gif", "bmp", "tif", "tiff", "webp", "heic"];

/// Folder names never scanned (Picasa's defaults plus our own backup folder). Any folder
/// whose name starts with "." is skipped too (hidden backups such as `.picasaoriginals`).
pub const EXCLUDED_DIRS: &[&str] = &["windows", "winnt", "temp", "program files", "program files (x86)", "originals", "$recycle.bin"];

#[derive(Debug, Default, Clone, Serialize)]
pub struct ScanStats {
    pub folders: usize,
    pub photos: usize,
    pub changed: usize,
    pub folders_removed: usize,
    pub ms: u128,
}

/// File types included in the library (Options > File Types); all of IMAGE_EXTS until set.
static FILE_TYPES: std::sync::RwLock<Vec<String>> = std::sync::RwLock::new(Vec::new());

pub fn set_file_types(exts: &[String]) {
    *FILE_TYPES.write().unwrap_or_else(|p| p.into_inner()) = exts.iter().map(|e| e.to_lowercase()).collect();
}

pub fn is_image(p: &Path) -> bool {
    let Some(e) = p.extension().and_then(|e| e.to_str()) else { return false };
    if !IMAGE_EXTS.iter().any(|x| e.eq_ignore_ascii_case(x)) {
        return false;
    }
    let chosen = FILE_TYPES.read().unwrap_or_else(|p| p.into_inner());
    // "jpg" also covers .jpeg and .jpe, as in Picasa's File Types list
    let e = e.to_lowercase();
    let e = if e == "jpeg" || e == "jpe" { "jpg".to_string() } else if e == "tif" { "tiff".to_string() } else { e };
    chosen.is_empty() || chosen.iter().any(|c| *c == e || (c == "tif" && e == "tiff"))
}

fn skip_dir(name: &str) -> bool {
    name.starts_with('.') || EXCLUDED_DIRS.iter().any(|x| name.eq_ignore_ascii_case(x))
}

/// Whether a folder lies in (or is) a folder that scanning leaves out, such as the hidden
/// `.casapicoriginals`, counting only the part of its path below `start`.
pub fn skipped_below(dir: &Path, start: &Path) -> bool {
    dir.strip_prefix(start).map(|rest| rest.components().any(|c| skip_dir(&c.as_os_str().to_string_lossy()))).unwrap_or(false)
}

fn mtime_of(md: &std::fs::Metadata) -> i64 {
    md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Scans every folder under `root`. Returns stats and the ids of new/changed photos.
pub fn scan_root(db: &Db, root: &Path) -> Result<(ScanStats, Vec<i64>)> {
    scan_tree(db, root, |_| true)
}

/// Scans one Folder Manager start point and its sub-folders, leaving out those that another
/// mark takes over (removed, or a start point of their own).
pub fn scan_start(db: &Db, start: &Path, settings: &crate::settings::Settings) -> Result<(ScanStats, Vec<i64>)> {
    scan_tree(db, start, |d| settings.belongs_to(d, start))
}

fn scan_tree(db: &Db, root: &Path, include: impl Fn(&Path) -> bool) -> Result<(ScanStats, Vec<i64>)> {
    let t = Instant::now();
    let mut stats = ScanStats::default();
    let mut changed_ids = Vec::new();
    let mut seen = Vec::new();
    let dirs: Vec<PathBuf> = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            !e.file_type().is_dir() || e.depth() == 0 || (!skip_dir(&e.file_name().to_string_lossy()) && include(e.path()))
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir())
        .map(|e| e.into_path())
        .collect();
    let root_s = root.to_string_lossy().to_string();
    for dir in dirs {
        if let Some((n, ids)) = scan_folder(db, &dir, &root_s)? {
            stats.folders += 1;
            stats.photos += n;
            stats.changed += ids.len();
            changed_ids.extend(ids);
            seen.push(dir.to_string_lossy().to_string());
        }
    }
    stats.folders_removed = db.remove_folders_not_in(&root_s, &seen)?;
    stats.ms = t.elapsed().as_millis();
    Ok((stats, changed_ids))
}

/// Scans one folder (not its sub-folders). Returns None if it holds no images and was
/// never indexed; otherwise (number of photos, ids of new/changed photos).
pub fn scan_folder(db: &Db, dir: &Path, root: &str) -> Result<Option<(usize, Vec<i64>)>> {
    let path_s = dir.to_string_lossy().to_string();
    let mut files: Vec<(String, i64, i64)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            let Ok(md) = e.metadata() else { continue };
            if md.is_file() && is_image(&p) {
                files.push((e.file_name().to_string_lossy().to_string(), md.len() as i64, mtime_of(&md)));
            }
        }
    }
    let existing = db.folder_id(&path_s)?;
    if files.is_empty() && existing.is_none() {
        return Ok(None);
    }
    let index = match existing {
        Some(fid) => db.folder_index(fid)?,
        None => Default::default(),
    };
    let todo: Vec<&(String, i64, i64)> = files
        .iter()
        .filter(|(n, s, m)| index.get(n).is_none_or(|&(_, s0, m0)| s0 != *s || m0 != *m))
        .collect();
    let changed: Vec<(PhotoInfo, Option<Vec<u8>>)> = todo
        .par_iter()
        .map(|(name, size, mtime)| {
            let m = crate::meta::read(&dir.join(name)).unwrap_or_default();
            let preview = m.preview.and_then(|j| crate::thumbs::upright_preview(j, m.orientation.max(1)));
            (
                PhotoInfo {
                    name: name.clone(),
                    size: *size,
                    mtime: *mtime,
                    width: m.width,
                    height: m.height,
                    taken: m.taken,
                    orientation: m.orientation.max(1),
                    camera: m.camera,
                },
                preview,
            )
        })
        .collect();
    let present: Vec<String> = files.iter().map(|f| f.0.clone()).collect();
    let sidecar = crate::sidecar::read(dir);
    let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path_s.clone());
    let (_, ids) = db.apply_folder_scan(&path_s, &name, root, &present, &changed, &sidecar)?;
    Ok(Some((files.len(), ids)))
}

/// Brings the index in line with the Folder Manager: drops folders that are no longer in the
/// library (or now belong to another start point), then scans every Scan Always start point.
/// Scan Once start points are scanned when `rescan_once` is set (the user just pressed OK in
/// the Folder Manager) or when they are not indexed yet. Returns the ids of new/changed photos.
pub fn sync(db: &Db, settings: &crate::settings::Settings, rescan_once: bool) -> Result<Vec<i64>> {
    use crate::settings::Scan;
    let mut known = std::collections::HashSet::new();
    for (path, root) in db.folder_roots()? {
        let p = Path::new(&path);
        if settings.included(p) && settings.belongs_to(p, Path::new(&root)) && !skipped_below(p, Path::new(&root)) {
            known.insert(root);
        } else {
            db.remove_folder(&path)?;
        }
    }
    let mut ids = Vec::new();
    for (start, scan) in settings.scan_starts() {
        if !start.is_dir() {
            continue;
        }
        if scan == Scan::Always || rescan_once || !known.contains(&start.to_string_lossy().to_string()) {
            ids.extend(scan_start(db, &start, settings)?.1);
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{Scan, Settings};

    fn photo(dir: &Path, name: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let img = image::RgbImage::from_pixel(8, 8, image::Rgb([90, 120, 200]));
        std::fs::write(dir.join(name), crate::thumbs::encode_jpeg(&img, 80).unwrap()).unwrap();
    }

    fn indexed(db: &Db) -> Vec<String> {
        let mut v: Vec<String> = db.folders().unwrap().into_iter().map(|f| f.name).collect();
        v.sort();
        v
    }

    #[test]
    fn folder_manager_marks_decide_what_is_indexed() {
        let d = tempfile::tempdir().unwrap();
        let pics = d.path().join("Pics");
        for f in ["a", "b", "b/c", "other"] {
            photo(&pics.join(f), "x.jpg");
        }
        photo(&d.path().join("Elsewhere"), "y.png");
        let db = Db::open_in_memory().unwrap();
        let mut s = Settings::default();
        s.set(&pics, Some(Scan::Always));
        s.set(&pics.join("b"), Some(Scan::Remove));
        s.set(&pics.join("b").join("c"), Some(Scan::Once));
        sync(&db, &s, true).unwrap();
        assert_eq!(indexed(&db), ["a", "c", "other"]);

        // un-remove b: it joins Pics; c keeps its own Scan Once mark
        s.set(&pics.join("b"), None);
        sync(&db, &s, true).unwrap();
        assert_eq!(indexed(&db), ["a", "b", "c", "other"]);

        // a hidden folder indexed by mistake is dropped; never added by a scan
        assert!(skipped_below(&pics.join("a").join(".casapicoriginals"), &pics));
        assert!(!skipped_below(&pics.join("a"), &pics));

        // remove the whole of Pics except c; add another folder
        s.set(&pics, Some(Scan::Remove));
        s.set(&d.path().join("Elsewhere"), Some(Scan::Always));
        sync(&db, &s, false).unwrap();
        assert_eq!(indexed(&db), ["Elsewhere", "c"]);

        // a file type switched off in Options is left out
        set_file_types(&["jpg".to_string()]);
        sync(&db, &s, false).unwrap();
        set_file_types(&[]);
        assert_eq!(indexed(&db), ["c"], "Elsewhere holds only a PNG");
    }
}
