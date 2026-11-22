//! The quick photo viewer (Picasa's Photo Viewer): what opens when a photo is double-clicked
//! in Explorer once CasaPic handles that file type. It shows the photo over a dimmed screen,
//! with the other photos of the same folder one key away, and an "Edit in CasaPic" button.
//!
//! It is the same program started with a file name, but it never opens the library index,
//! the thumbnail worker or the folder watcher: only the folder's file names are read, and each
//! photo is decoded at screen size when shown (its neighbours a moment ahead). Stars and turns
//! go into the folder's `.casapic.ini`, exactly as the library writes them, so the library
//! shows them next time it reads that folder.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use casapic_core::render::{self, Img};
use casapic_core::{meta, recipe, scan, sidecar, thumbs, wic};
use serde::Serialize;
use tauri::http::{Response, StatusCode};
use tauri::{AppHandle, Manager, State};

use super::{Res, err, push_rotate};

/// Decoded photos kept ready: (file name, edge, recipe text) and the JPEG.
type Cache = Vec<((String, u32, String), Arc<Vec<u8>>)>;
const CACHE_SIZE: usize = 5;

pub(crate) struct Viewer {
    folder: PathBuf,
    /// the photos of the folder, in name order (as Explorer lists them by default)
    files: Mutex<Vec<String>>,
    /// the photo it was opened for
    first: String,
    cache: Mutex<Cache>,
}

/// Sorts names the way Explorer does by default: digits compare as numbers ("IMG_2" before
/// "IMG_10"), letters without case.
pub(crate) fn natural_key(name: &str) -> Vec<(u8, u64, String)> {
    let mut out = Vec::new();
    let mut chars = name.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            let mut n = String::new();
            while let Some(&d) = chars.peek().filter(|d| d.is_ascii_digit()) {
                n.push(d);
                chars.next();
            }
            out.push((0, n.parse().unwrap_or(u64::MAX), String::new()));
        } else {
            let mut s = String::new();
            while let Some(&d) = chars.peek().filter(|d| !d.is_ascii_digit()) {
                s.extend(d.to_lowercase());
                chars.next();
            }
            out.push((1, 0, s));
        }
    }
    out
}

/// The photos in a folder, in name order; `keep` is listed even if its type is not one CasaPic
/// lists by default (it was opened on purpose).
pub(crate) fn folder_photos(folder: &Path, keep: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n == keep || scan::is_image(Path::new(n)))
                .collect()
        })
        .unwrap_or_default();
    if !names.iter().any(|n| n == keep) {
        names.push(keep.to_string());
    }
    names.sort_by_cached_key(|n| natural_key(n));
    names
}

impl Viewer {
    pub(crate) fn new(path: &Path) -> Viewer {
        let folder = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let first = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let files = folder_photos(&folder, &first);
        Viewer { folder, files: Mutex::new(files), first, cache: Mutex::new(Vec::new()) }
    }

    fn name(&self, i: usize) -> Res<String> {
        self.files.lock().map_err(err)?.get(i).cloned().ok_or_else(|| "no such photo".to_string())
    }

    fn recipe_of(&self, name: &str) -> String {
        sidecar::read(&self.folder).get(name).map(|e| e.filters.clone()).unwrap_or_default()
    }

    /// The photo upright, with its CasaPic edits, as a JPEG fitting `edge` (0 = full size).
    fn jpeg(&self, name: &str, edge: u32) -> Res<Arc<Vec<u8>>> {
        let text = self.recipe_of(name);
        let key = (name.to_string(), edge, text.clone());
        if let Some((_, j)) = self.cache.lock().map_err(err)?.iter().find(|(k, _)| *k == key) {
            return Ok(j.clone());
        }
        let path = self.folder.join(name);
        let orientation = meta::read(&path).map(|m| m.orientation).unwrap_or(1);
        let rgb = wic::orient(wic::decode(&path, (edge > 0).then_some(edge)).map_err(err)?, orientation);
        let ops = recipe::parse(&text);
        let rgb = if ops.is_empty() {
            rgb
        } else {
            let (w, h) = wic::dimensions(&path).map_err(err)?;
            let scale = (rgb.width().max(rgb.height()) as f32 / w.max(h).max(1) as f32).min(1.0);
            render::apply_scaled(&Img::from_rgb8(&rgb), &ops, scale).to_rgb8()
        };
        let j = Arc::new(thumbs::encode_jpeg(&rgb, 90).map_err(err)?);
        let mut c = self.cache.lock().map_err(err)?;
        c.retain(|(k, _)| *k != key);
        c.push((key, j.clone()));
        if c.len() > CACHE_SIZE {
            c.remove(0);
        }
        Ok(j)
    }
}

/// `/v/<index>/<edge>`: a photo of the folder. The neighbours are decoded meanwhile.
pub(crate) fn serve(h: &AppHandle, path: &str) -> Response<Vec<u8>> {
    let v = h.state::<Viewer>();
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let (Some(&"v"), Some(Ok(i)), Some(Ok(edge))) = (parts.first(), parts.get(1).map(|s| s.parse::<usize>()), parts.get(2).map(|s| s.parse::<u32>())) else {
        return status(StatusCode::NOT_FOUND, "unknown address");
    };
    let res = v.name(i).and_then(|n| v.jpeg(&n, edge));
    if edge > 0 {
        let h = h.clone();
        std::thread::spawn(move || {
            let v = h.state::<Viewer>();
            for j in [i + 1, i.wrapping_sub(1)] {
                if let Ok(n) = v.name(j) {
                    let _ = v.jpeg(&n, edge);
                }
            }
        });
    }
    match res {
        Ok(j) => Response::builder().header("Content-Type", "image/jpeg").header("Access-Control-Allow-Origin", "*").body(j.to_vec()).unwrap(),
        Err(e) => status(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

fn status(code: StatusCode, text: &str) -> Response<Vec<u8>> {
    Response::builder().status(code).header("Access-Control-Allow-Origin", "*").body(text.as_bytes().to_vec()).unwrap()
}

#[derive(Serialize)]
pub(crate) struct ViewerState {
    folder: String,
    files: Vec<String>,
    index: usize,
}

#[tauri::command]
pub(crate) fn viewer_state(v: State<'_, Viewer>) -> Res<ViewerState> {
    let files = v.files.lock().map_err(err)?.clone();
    let index = files.iter().position(|n| *n == v.first).unwrap_or(0);
    Ok(ViewerState { folder: v.folder.to_string_lossy().to_string(), files, index })
}

#[derive(Serialize)]
pub(crate) struct PhotoInfo {
    name: String,
    path: String,
    /// as shown: upright, after rotate/crop
    width: u32,
    height: u32,
    bytes: u64,
    /// seconds since 1970 of the camera's wall-clock time
    taken: Option<i64>,
    /// the file's date on the same clock, shown when there is no camera date (as the library does)
    file_date: i64,
    star: bool,
    edited: bool,
}

#[tauri::command(async)]
pub(crate) fn viewer_info(v: State<'_, Viewer>, index: usize) -> Res<PhotoInfo> {
    let name = v.name(index)?;
    let path = v.folder.join(&name);
    let m = meta::read(&path).unwrap_or_default();
    let (mut w, mut h) = if m.width > 0 { (m.width as u32, m.height as u32) } else { wic::dimensions(&path).unwrap_or((0, 0)) };
    if (5..=8).contains(&m.orientation) {
        (w, h) = (h, w);
    }
    let entry = sidecar::read(&v.folder).remove(&name).unwrap_or_default();
    let ops = recipe::parse(&entry.filters);
    let (ow, oh) = render::output_size(w as f32, h as f32, &ops);
    let md = std::fs::metadata(&path).ok();
    let modified = md.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
    Ok(PhotoInfo {
        bytes: md.as_ref().map(|m| m.len()).unwrap_or(0),
        file_date: casapic_core::dates::local_clock(modified.map(|d| d.as_secs() as i64).unwrap_or(0)),
        path: path.to_string_lossy().to_string(),
        name,
        width: ow.round() as u32,
        height: oh.round() as u32,
        taken: m.taken,
        star: entry.star,
        edited: !ops.is_empty(),
    })
}

#[tauri::command(async)]
pub(crate) fn viewer_star(v: State<'_, Viewer>, index: usize, star: bool) -> Res<()> {
    let name = v.name(index)?;
    sidecar::update(&v.folder, &name, |e| e.star = star).map_err(err)?;
    Ok(())
}

/// Quarter turns (1 = clockwise), kept as an edit like the library's Rotate.
#[tauri::command(async)]
pub(crate) fn viewer_rotate(v: State<'_, Viewer>, index: usize, turns: i32) -> Res<()> {
    let name = v.name(index)?;
    sidecar::update(&v.folder, &name, |e| {
        let mut ops = recipe::parse(&e.filters);
        push_rotate(&mut ops, turns);
        e.filters = recipe::to_string(&ops);
    })
    .map_err(err)?;
    Ok(())
}

/// Moves the photo to the Recycle Bin; returns how many photos are left.
#[tauri::command(async)]
pub(crate) fn viewer_delete(v: State<'_, Viewer>, index: usize) -> Res<usize> {
    let name = v.name(index)?;
    casapic_core::files::delete_photo(&v.folder, &name).map_err(err)?;
    let mut files = v.files.lock().map_err(err)?;
    files.retain(|n| *n != name);
    Ok(files.len())
}

/// "Edit in CasaPic": the library opens (or comes forward) with this photo in the Edit view,
/// and the viewer closes.
#[tauri::command]
pub(crate) fn viewer_edit_in_casapic(h: AppHandle, v: State<'_, Viewer>, index: usize) -> Res<()> {
    let path = v.folder.join(v.name(index)?);
    let exe = std::env::current_exe().map_err(err)?;
    std::process::Command::new(exe).arg("--select").arg(&path).spawn().map_err(err)?;
    h.exit(0);
    Ok(())
}

#[tauri::command]
pub(crate) fn viewer_close(h: AppHandle) {
    h.exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_sort_like_explorer() {
        let mut v = vec!["IMG_10.jpg", "img_2.JPG", "IMG_1.jpg", "a.png", "IMG_10a.jpg"];
        v.sort_by_cached_key(|n| natural_key(n));
        assert_eq!(v, ["a.png", "IMG_1.jpg", "img_2.JPG", "IMG_10.jpg", "IMG_10a.jpg"]);
    }

    #[test]
    fn folder_lists_photos_and_the_opened_file() {
        let dir = std::env::temp_dir().join(format!("casapic-viewer-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for n in ["b10.jpg", "b9.jpg", "notes.txt", "odd.xyz"] {
            std::fs::write(dir.join(n), b"x").unwrap();
        }
        assert_eq!(folder_photos(&dir, "b9.jpg"), ["b9.jpg", "b10.jpg"], "only photos, in number order");
        assert_eq!(folder_photos(&dir, "odd.xyz"), ["b9.jpg", "b10.jpg", "odd.xyz"], "a file opened on purpose is kept");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
