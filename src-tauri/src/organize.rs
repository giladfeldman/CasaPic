//! Albums, tags and captions (core `albums.rs` and `tags.rs`). Tags and captions are written
//! into the photo files as Picasa does; albums live in the folders' `.casapic.ini` files and
//! in CasaPic's settings.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use casapic_core::albums::{self, Album};
use casapic_core::db::AlbumRow;
use casapic_core::recipe::Op;
use casapic_core::render::{self, Img};
use casapic_core::{picasa, tags, wic};
use rayon::prelude::*;
use tauri::{AppHandle, Emitter, State};

use super::{App, Job, Res, err, folder_and_name};
use crate::folders::rescan_folder;

fn photo_file(app: &App, id: i64) -> Res<(PathBuf, String)> {
    let path = app.db.photo_path(id).map_err(err)?.ok_or("photo not found")?;
    let (dir, name) = folder_and_name(&path)?;
    Ok((dir.to_path_buf(), name))
}

fn files_of(app: &App, ids: &[i64]) -> Res<Vec<(PathBuf, String)>> {
    ids.iter().map(|&id| photo_file(app, id)).collect()
}

fn refresh(h: &AppHandle, app: &App, dirs: impl IntoIterator<Item = PathBuf>) {
    for d in dirs.into_iter().collect::<BTreeSet<_>>() {
        rescan_folder(app, &d);
    }
    let _ = h.emit("library-changed", ());
}

/// Every album, by name. Albums found in a folder's sidecar (photos copied from elsewhere)
/// join CasaPic's own list, so they stay even after their last photo leaves.
#[tauri::command(async)]
pub(crate) fn albums_list(app: State<'_, App>) -> Res<Vec<AlbumRow>> {
    let rows = app.db.albums().map_err(err)?;
    let mut s = app.settings.lock().map_err(err)?;
    let mut added = false;
    for r in &rows {
        if !s.albums.iter().any(|a| a.token == r.album.token) {
            s.albums.push(r.album.clone());
            added = true;
        }
    }
    if added {
        s.save().map_err(err)?;
    }
    Ok(rows)
}

/// The photos of an album, oldest first, one row each as in the library.
#[tauri::command(async)]
pub(crate) fn album_photos(app: State<'_, App>, token: String) -> Res<Vec<[i64; 5]>> {
    Ok(app.db.album_photos(&token).map_err(err)?.iter().map(super::photo_row).collect())
}

/// New Album (with these photos in it, if any). Returns its token.
#[tauri::command(async)]
pub(crate) fn album_create(h: AppHandle, app: State<'_, App>, album: Album, ids: Vec<i64>) -> Res<String> {
    if album.name.trim().is_empty() {
        return Err("an album needs a name".into());
    }
    let a = Album { token: albums::new_token(), name: album.name.trim().to_string(), ..album };
    {
        let mut s = app.settings.lock().map_err(err)?;
        s.albums.push(a.clone());
        s.save().map_err(err)?;
    }
    app.db.put_album(&a).map_err(err)?;
    if !ids.is_empty() {
        let files = files_of(&app, &ids)?;
        albums::add(&a, &files).map_err(err)?;
        refresh(&h, &app, files.into_iter().map(|f| f.0));
    }
    Ok(a.token)
}

fn album_of(app: &App, token: &str) -> Res<Album> {
    let s = app.settings.lock().map_err(err)?;
    s.albums.iter().find(|a| a.token == token).cloned().ok_or_else(|| "album not found".into())
}

/// Add to Album (the tray button, or a drag onto the album). Returns how many photos.
#[tauri::command(async)]
pub(crate) fn album_add(h: AppHandle, app: State<'_, App>, token: String, ids: Vec<i64>) -> Res<usize> {
    let a = album_of(&app, &token)?;
    let files = files_of(&app, &ids)?;
    albums::add(&a, &files).map_err(err)?;
    refresh(&h, &app, files.into_iter().map(|f| f.0));
    Ok(ids.len())
}

/// Remove from Album: the photos stay on disk and in their folders.
#[tauri::command(async)]
pub(crate) fn album_remove(h: AppHandle, app: State<'_, App>, token: String, ids: Vec<i64>) -> Res<usize> {
    let files = files_of(&app, &ids)?;
    albums::remove(&token, &files).map_err(err)?;
    refresh(&h, &app, files.into_iter().map(|f| f.0));
    Ok(ids.len())
}

/// Album Properties: name, date, place taken, description.
#[tauri::command(async)]
pub(crate) fn album_edit(h: AppHandle, app: State<'_, App>, album: Album) -> Res<()> {
    if album.name.trim().is_empty() {
        return Err("an album needs a name".into());
    }
    let a = Album { name: album.name.trim().to_string(), ..album };
    {
        let mut s = app.settings.lock().map_err(err)?;
        let slot = s.albums.iter_mut().find(|x| x.token == a.token).ok_or("album not found")?;
        *slot = a.clone();
        s.save().map_err(err)?;
    }
    app.db.put_album(&a).map_err(err)?;
    let folders = app.db.album_folders(&a.token).map_err(err)?;
    albums::rewrite(&a, &folders).map_err(err)?;
    refresh(&h, &app, folders);
    Ok(())
}

/// Delete Album: the album goes; its photos stay where they are.
#[tauri::command(async)]
pub(crate) fn album_delete(h: AppHandle, app: State<'_, App>, token: String) -> Res<()> {
    let folders = app.db.album_folders(&token).map_err(err)?;
    albums::delete(&token, &folders).map_err(err)?;
    {
        let mut s = app.settings.lock().map_err(err)?;
        s.albums.retain(|a| a.token != token);
        s.save().map_err(err)?;
    }
    app.db.delete_album(&token).map_err(err)?;
    refresh(&h, &app, folders);
    Ok(())
}

/// Puts CasaPic's album list into the index at start (empty albums have no sidecar copy).
pub(crate) fn load_albums(app: &App) {
    let list = app.settings.lock().map(|s| s.albums.clone()).unwrap_or_default();
    for a in list {
        let _ = app.db.put_album(&a);
    }
}

/// Tags panel: adds and removes tags on each photo. Returns how many photo files changed.
#[tauri::command(async)]
pub(crate) fn set_tags(h: AppHandle, app: State<'_, App>, ids: Vec<i64>, add: Vec<String>, remove: Vec<String>) -> Res<usize> {
    let remove: Vec<String> = remove.iter().map(|t| t.trim().to_lowercase()).collect();
    let mut dirs = Vec::new();
    let mut changed = 0;
    let mut first_error = None;
    for id in &ids {
        let (dir, name) = photo_file(&app, *id)?;
        let r = tags::change(&dir, &name, |l| {
            l.tags.retain(|t| !remove.contains(&t.to_lowercase()));
            l.tags.extend(add.iter().cloned());
        });
        match r {
            Ok(true) => {
                changed += 1;
                app.forget(*id);
            }
            Ok(false) => {}
            Err(e) => {
                first_error.get_or_insert(format!("{name}: {e:#}"));
            }
        }
        dirs.push(dir);
    }
    refresh(&h, &app, dirs);
    app.queue(Job::Pending);
    match first_error {
        Some(e) => Err(format!("{changed} photos changed; then: {e}")),
        None => Ok(changed),
    }
}

/// The caption under the photo in the Edit view ("Make a caption!").
#[tauri::command(async)]
pub(crate) fn set_caption(h: AppHandle, app: State<'_, App>, id: i64, text: String) -> Res<()> {
    let (dir, name) = photo_file(&app, id)?;
    if tags::change(&dir, &name, |l| l.caption = text).map_err(err)? {
        app.forget(id);
        refresh(&h, &app, [dir]);
    }
    Ok(())
}

/// Search (Ctrl+F): the ids of the photos that match.
#[tauri::command(async)]
pub(crate) fn search(app: State<'_, App>, query: casapic_core::search::Query) -> Res<Vec<i64>> {
    casapic_core::search::run(&app.db, &query).map_err(err)
}

/// Picture > Hide / Unhide (`hidden=yes` in the folder's sidecar, as Picasa writes it).
#[tauri::command(async)]
pub(crate) fn set_hidden(h: AppHandle, app: State<'_, App>, ids: Vec<i64>, hidden: bool) -> Res<usize> {
    let files = files_of(&app, &ids)?;
    for (dir, name) in &files {
        casapic_core::sidecar::update(dir, name, |e| e.set("hidden", if hidden { "yes" } else { "" })).map_err(err)?;
    }
    refresh(&h, &app, files.into_iter().map(|f| f.0));
    Ok(ids.len())
}

/// Folder > Hide / Unhide (`P2category=Hidden Folders` in the folder's own section).
#[tauri::command(async)]
pub(crate) fn set_folder_hidden(h: AppHandle, app: State<'_, App>, path: String, hidden: bool) -> Res<()> {
    let dir = PathBuf::from(&path);
    casapic_core::sidecar::update(&dir, casapic_core::sidecar::FOLDER_SECTION, |e| {
        e.set("P2category", if hidden { "Hidden Folders" } else { "" })
    })
    .map_err(err)?;
    refresh(&h, &app, [dir]);
    Ok(())
}

/// Every tag in the library with how many photos carry it, most used first.
#[tauri::command(async)]
pub(crate) fn all_tags(app: State<'_, App>) -> Res<Vec<(String, i64)>> {
    app.db.all_tags().map_err(err)
}

/// Tools > Import Picasa Settings: reads the `.picasa.ini` Picasa left in every folder of the
/// library (never changes it). With `apply` false, only says what it would bring over.
#[tauri::command(async)]
pub(crate) fn picasa_import(h: AppHandle, app: State<'_, App>, apply: bool) -> Res<picasa::Summary> {
    let dirs: Vec<PathBuf> = app.db.folders().map_err(err)?.into_iter().map(|f| PathBuf::from(f.path)).collect();
    let found: Vec<picasa::FolderImport> = dirs.iter().filter_map(|d| picasa::read_folder(d)).collect();
    let summary = picasa::summarize(&found, &|d| casapic_core::sidecar::read(d));
    if !apply {
        return Ok(summary);
    }
    let mut first_error = None;
    let mut regen = Vec::new();
    for fi in &found {
        let mine = casapic_core::sidecar::read(&fi.dir);
        let fid = app.db.folder_id(&fi.dir.to_string_lossy()).map_err(err)?;
        let photos = match fid {
            Some(fid) => app.db.photos(fid).map_err(err)?,
            None => Vec::new(),
        };
        // each recipe CasaPic takes over: the one-click fixes are measured on the photo as it
        // looks at that point of the recipe, as Picasa does
        let jobs: Vec<&picasa::PhotoImport> = fi
            .photos
            .iter()
            .filter(|p| !p.steps.is_empty() && mine.get(&p.name).is_none_or(|e| e.filters.is_empty()))
            .collect();
        let recipes: HashMap<String, Vec<Op>> = jobs
            .par_iter()
            .map(|p| {
                let photo = photos.iter().find(|x| x.name == p.name);
                let mut ops: Vec<Op> = Vec::new();
                let mut base: Option<Img> = None;
                for s in &p.steps {
                    match s {
                        picasa::Step::Op(o) => ops.push(o.clone()),
                        picasa::Step::Auto(kind) => {
                            let Some(photo) = photo else { continue };
                            if base.is_none() {
                                let rgb = wic::decode(&fi.dir.join(&p.name), Some(1024)).map(|r| wic::orient(r, photo.orientation));
                                base = rgb.ok().map(|r| Img::from_rgb8(&r));
                            }
                            let Some(img) = &base else { continue };
                            let long = photo.width.max(photo.height).max(1) as f32;
                            let scale = (img.w.max(img.h) as f32 / long).min(1.0);
                            let look = render::apply_scaled(img, &ops, scale);
                            ops.push(match *kind {
                                "autocontrast" => render::contrast_op(&look),
                                "autocolor" => render::color_op(&look),
                                _ => render::lucky_op(&look),
                            });
                        }
                    }
                }
                (p.name.clone(), ops)
            })
            .collect();
        if let Err(e) = picasa::apply_folder(fi, &recipes) {
            first_error.get_or_insert(format!("{}: {e}", fi.dir.display()));
            continue;
        }
        regen.extend(photos.iter().filter(|x| recipes.contains_key(&x.name)).map(|x| x.id));
        // captions and tags older Picasa versions kept beside the photo go into the file, as
        // CasaPic keeps them; what the file already has wins
        for p in fi.photos.iter().filter(|p| !p.caption.is_empty() || !p.keywords.is_empty()) {
            let r = tags::change(&fi.dir, &p.name, |l| {
                if l.caption.is_empty() {
                    l.caption = p.caption.clone();
                }
                l.tags.extend(p.keywords.iter().cloned());
            });
            if let Err(e) = r {
                first_error.get_or_insert(format!("{}: {e}", p.name));
            }
        }
    }
    refresh(&h, &app, found.iter().map(|f| f.dir.clone()));
    for id in &regen {
        app.forget(*id);
    }
    if !regen.is_empty() {
        app.queue(Job::Regen(regen));
    }
    match first_error {
        Some(e) => Err(format!("imported, but some could not be written: {e}")),
        None => Ok(summary),
    }
}
