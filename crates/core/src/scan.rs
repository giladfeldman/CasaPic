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

pub fn is_image(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

fn skip_dir(name: &str) -> bool {
    name.starts_with('.') || EXCLUDED_DIRS.iter().any(|x| name.eq_ignore_ascii_case(x))
}

fn mtime_of(md: &std::fs::Metadata) -> i64 {
    md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Scans every folder under `root`. Returns stats and the ids of new/changed photos.
pub fn scan_root(db: &Db, root: &Path) -> Result<(ScanStats, Vec<i64>)> {
    let t = Instant::now();
    let mut stats = ScanStats::default();
    let mut changed_ids = Vec::new();
    let mut seen = Vec::new();
    let dirs: Vec<PathBuf> = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !e.file_type().is_dir() || e.depth() == 0 || !skip_dir(&e.file_name().to_string_lossy()))
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
