//! Photo and folder file actions (core `files.rs`): Rename, Delete to the Recycle Bin, Move,
//! Copy, Move to New Folder, folder Rename/Move/Delete/New, and Save As / Save a Copy.
//! After each one the folders involved are read again and the window is told to reload.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use casapic_core::{export, files, recipe};
use tauri::{AppHandle, Emitter, Manager, State};

use super::{App, Job, Res, err, folder_and_name};
use crate::folders::{rescan_folder, sync_library};

/// Reads the given folders again and tells the window.
fn refresh(h: &AppHandle, app: &App, dirs: &BTreeSet<PathBuf>) {
    for d in dirs {
        rescan_folder(app, d);
    }
    let _ = h.emit("library-changed", ());
    app.queue(Job::Pending);
}

fn photo_file(app: &App, id: i64) -> Res<(PathBuf, String)> {
    let path = app.db.photo_path(id).map_err(err)?.ok_or("photo not found")?;
    let (dir, name) = folder_and_name(&path)?;
    Ok((dir.to_path_buf(), name))
}

/// Picture > Rename (F2). Returns the new file name.
#[tauri::command(async)]
pub(crate) fn rename_photo(h: AppHandle, app: State<'_, App>, id: i64, name: String) -> Res<String> {
    let (dir, old) = photo_file(&app, id)?;
    let new = files::rename_photo(&dir, &old, &name).map_err(err)?;
    app.db.relocate_photo(id, &dir.to_string_lossy(), &new).map_err(err)?;
    app.forget(id);
    refresh(&h, &app, &BTreeSet::from([dir]));
    Ok(new)
}

/// Delete from Disk: each photo goes to the Recycle Bin. Returns how many went.
#[tauri::command(async)]
pub(crate) fn delete_photos(h: AppHandle, app: State<'_, App>, ids: Vec<i64>) -> Res<usize> {
    let mut dirs = BTreeSet::new();
    let mut done = 0;
    let mut first_error = None;
    for id in ids {
        let (dir, name) = photo_file(&app, id)?;
        match files::delete_photo(&dir, &name) {
            Ok(()) => done += 1,
            Err(e) => {
                first_error.get_or_insert(format!("{e:#}"));
            }
        }
        app.forget(id);
        dirs.insert(dir);
    }
    refresh(&h, &app, &dirs);
    match first_error {
        Some(e) if done == 0 => Err(e),
        Some(e) => Err(format!("{done} moved to the Recycle Bin; then: {e}")),
        None => Ok(done),
    }
}

/// Paste or drag to a folder: moves (or copies) photos there. Returns how many.
#[tauri::command(async)]
pub(crate) fn transfer_photos(h: AppHandle, app: State<'_, App>, ids: Vec<i64>, dest: String, copy: bool) -> Res<usize> {
    let to = PathBuf::from(&dest);
    let mut dirs = BTreeSet::from([to.clone()]);
    let mut done = 0;
    for id in ids {
        let (dir, name) = photo_file(&app, id)?;
        let r = files::transfer_photo(&dir, &name, &to, copy);
        dirs.insert(dir);
        match r {
            Ok(new) => {
                if !copy {
                    app.db.relocate_photo(id, &dest, &new).map_err(err)?;
                    app.forget(id);
                }
                done += 1;
            }
            Err(e) => {
                refresh(&h, &app, &dirs);
                return Err(format!("{done} done; then: {e:#}"));
            }
        }
    }
    refresh(&h, &app, &dirs);
    Ok(done)
}

/// Move to New Folder (and Split Folder Here): makes `name` beside the first photo's folder and
/// moves the photos into it. Returns the new folder's path.
#[tauri::command(async)]
pub(crate) fn move_to_new_folder(h: AppHandle, app: State<'_, App>, ids: Vec<i64>, name: String) -> Res<String> {
    let first = *ids.first().ok_or("no photos chosen")?;
    let (dir, _) = photo_file(&app, first)?;
    let parent = dir.parent().ok_or("this folder has no parent folder")?;
    let new = files::new_folder(parent, &name).map_err(err)?;
    let dest = new.to_string_lossy().to_string();
    // index the (empty) folder first, so the photos keep their ids as they move in
    if let Some(start) = app.settings.lock().map_err(err)?.start_of(&new) {
        app.db.add_folder(&dest, &start.to_string_lossy()).map_err(err)?;
    }
    transfer_photos(h, app, ids, dest.clone(), false)?;
    Ok(dest)
}

/// Folder > New Folder. Returns its path.
#[tauri::command(async)]
pub(crate) fn new_folder(app: State<'_, App>, parent: String, name: String) -> Res<String> {
    let new = files::new_folder(Path::new(&parent), &name).map_err(err)?;
    let s = new.to_string_lossy().to_string();
    if let Some(start) = app.settings.lock().map_err(err)?.start_of(&new) {
        app.db.add_folder(&s, &start.to_string_lossy()).map_err(err)?;
    }
    Ok(s)
}

/// After a folder changed place on disk: the Folder Manager's marks follow it, then the
/// library is read again.
fn folder_changed(h: AppHandle, app: &App, from: &Path, to: Option<&Path>) -> Res<()> {
    {
        let mut s = app.settings.lock().map_err(err)?;
        match to {
            Some(to) => s.folder_moved(from, to),
            None => s.folder_deleted(from),
        }
        s.save().map_err(err)?;
    }
    app.db.remove_folder_tree(&from.to_string_lossy()).map_err(err)?;
    let to = to.map(Path::to_path_buf);
    std::thread::spawn(move || {
        sync_library(&h, false);
        // the folder's new place is read even inside a Scan Once folder, which a sync leaves alone
        let app = h.state::<App>();
        let settings = app.settings.lock().map(|s| s.clone()).unwrap_or_default();
        if let Some(start) = to.as_deref().and_then(|t| settings.start_of(t)) {
            let _ = casapic_core::scan::scan_start(&app.db, &start, &settings);
            let _ = h.emit("library-changed", ());
            app.queue(Job::Pending);
        }
    });
    Ok(())
}

/// Folder > Rename. Returns the new path.
#[tauri::command(async)]
pub(crate) fn rename_folder(h: AppHandle, app: State<'_, App>, path: String, name: String) -> Res<String> {
    let from = PathBuf::from(&path);
    let to = files::rename_folder(&from, &name).map_err(err)?;
    folder_changed(h, &app, &from, Some(&to))?;
    Ok(to.to_string_lossy().to_string())
}

/// Folder > Move Folder: into another folder. Returns the new path.
#[tauri::command(async)]
pub(crate) fn move_folder(h: AppHandle, app: State<'_, App>, path: String, into: String) -> Res<String> {
    let from = PathBuf::from(&path);
    let to = files::move_folder(&from, Path::new(&into)).map_err(err)?;
    folder_changed(h, &app, &from, Some(&to))?;
    Ok(to.to_string_lossy().to_string())
}

/// Folder > Delete Folder: the folder and everything in it go to the Recycle Bin.
#[tauri::command(async)]
pub(crate) fn delete_folder(h: AppHandle, app: State<'_, App>, path: String) -> Res<()> {
    let from = PathBuf::from(&path);
    files::delete_folder(&from).map_err(err)?;
    folder_changed(h, &app, &from, None)
}

/// File > Save a Copy (`dest` = None: beside the photo, as `name-1.jpg`) and File > Save As
/// (`dest` = the file chosen). The photo with its edits is written as a new JPEG; the photo
/// itself is not touched. Returns the file written.
#[tauri::command(async)]
pub(crate) fn save_copy(h: AppHandle, app: State<'_, App>, id: i64, dest: Option<String>) -> Res<String> {
    let p = app.db.photo(id).map_err(err)?.ok_or("photo not found")?;
    let src = app.db.photo_path(id).map_err(err)?.ok_or("photo not found")?;
    let ops = recipe::parse(&p.recipe);
    let jpeg = export::render_jpeg(&src, p.orientation, &ops, &export::ExportOptions { max_edge: None, quality: 95 }).map_err(err)?;
    let target = match dest {
        Some(d) => PathBuf::from(d),
        None => {
            let (dir, name) = folder_and_name(&src)?;
            let stem = Path::new(&name).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            dir.join(files::free_name(dir, &format!("{stem}-1.jpg")))
        }
    };
    if target.canonicalize().ok().is_some_and(|t| src.canonicalize().ok() == Some(t)) {
        return Err("Save As cannot write over the photo itself; use Save for that".into());
    }
    std::fs::write(&target, jpeg).map_err(|e| format!("write {}: {e}", target.display()))?;
    if let Some(dir) = target.parent() {
        refresh(&h, &app, &BTreeSet::from([dir.to_path_buf()]));
    }
    Ok(target.to_string_lossy().to_string())
}

/// Picture > Adjust Date and Time: moves each photo's date by `shift` seconds, writing it into
/// the file's camera data after keeping the untouched original (core `dates.rs`). Returns how
/// many photos changed.
#[tauri::command(async)]
pub(crate) fn adjust_dates(h: AppHandle, app: State<'_, App>, ids: Vec<i64>, shift: i64) -> Res<usize> {
    let mut dirs = BTreeSet::new();
    let mut done = 0;
    for id in ids {
        let p = app.db.photo(id).map_err(err)?.ok_or("photo not found")?;
        let (dir, name) = photo_file(&app, id)?;
        let r = casapic_core::dates::adjust(&dir, &name, casapic_core::dates::Change::Shift(shift), p.taken.unwrap_or(p.mtime));
        dirs.insert(dir);
        match r {
            Ok(changed) => done += changed as usize,
            Err(e) => {
                refresh(&h, &app, &dirs);
                return Err(format!("{name}: {e:#}{}", if done > 0 { format!(" ({done} changed before it)") } else { String::new() }));
            }
        }
        app.forget(id);
    }
    refresh(&h, &app, &dirs);
    Ok(done)
}

/// Options > Clear Thumbnail Cache: forgets every thumbnail; they are made again from the files.
#[tauri::command(async)]
pub(crate) fn clear_thumb_cache(app: State<'_, App>) -> Res<usize> {
    let n = app.db.clear_thumbs().map_err(err)?;
    app.queue(Job::Pending);
    Ok(n)
}

/// File > Open in an Editor ("editor": the program Windows uses to edit this kind of file,
/// else the one that opens it) and File > Open With ("choose": Windows' own chooser).
#[tauri::command(async)]
pub(crate) fn open_with(app: State<'_, App>, id: i64, how: String) -> Res<()> {
    let path = app.db.photo_path(id).map_err(err)?.ok_or("photo not found")?;
    match how.as_str() {
        "choose" => {
            std::process::Command::new("rundll32.exe").arg("shell32.dll,OpenAs_RunDLL").arg(&path).spawn().map_err(err)?;
        }
        "editor" => shell_open(&path, "edit").or_else(|_| shell_open(&path, "open"))?,
        _ => return Err(format!("unknown way to open: {how}")),
    }
    Ok(())
}

fn shell_open(path: &Path, verb: &str) -> Res<()> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::HSTRING;
    let r = unsafe { ShellExecuteW(None, &HSTRING::from(verb), &HSTRING::from(path.as_os_str()), None, None, SW_SHOWNORMAL) };
    // ShellExecute reports success with a value above 32
    if r.0 as isize > 32 { Ok(()) } else { Err(format!("Windows has no program to {verb} {}", path.display())) }
}
