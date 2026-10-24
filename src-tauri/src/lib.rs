//! CasaPic desktop app: the Tauri shell around `casapic-core`.
//!
//! The window talks to this file through commands (`invoke`) and three events
//! (`library-changed`, `thumbs-updated`, `status`). Pictures never travel as JSON: thumbnails
//! and the Edit-view picture come through the `casapic` protocol
//! (on Windows `http://casapic.localhost/thumb/<id>` and `/preview/<version>`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use casapic_core::db::Db;
use casapic_core::recipe::{self, Op};
use casapic_core::render::{self, Img};
use casapic_core::{export, paths, save, sidecar, thumbs, watch, wic};
use rayon::prelude::*;
use serde::Serialize;
use tauri::http::{Response, StatusCode};
use tauri::{AppHandle, Emitter, Manager, State};

mod files;
mod folders;

type Res<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Work for the thumbnail thread.
enum Job {
    /// make level-2 thumbnails for every photo still lacking one
    Pending,
    /// remake these (their edits changed)
    Regen(Vec<i64>),
}

/// A rendered Edit-view picture, ready to send: JPEG (a 7 MB raw picture took ~150 ms to
/// cross into the window; the JPEG takes a few) plus its histogram.
#[derive(Clone)]
struct Frame {
    w: u32,
    h: u32,
    jpeg: Arc<Vec<u8>>,
    /// 64 bins each for R, G, B
    hist: Arc<Vec<u32>>,
}

fn frame_of(rgb: &image::RgbImage) -> Res<Frame> {
    let mut hist = vec![0u32; 192];
    for p in rgb.as_raw().chunks_exact(3).step_by(4) {
        for c in 0..3 {
            hist[c * 64 + (p[c] >> 2) as usize] += 1;
        }
    }
    Ok(Frame { w: rgb.width(), h: rgb.height(), jpeg: Arc::new(thumbs::encode_jpeg(rgb, 92).map_err(err)?), hist: Arc::new(hist) })
}

/// A photo decoded and rendered ahead of time (the neighbours of the one on screen), so
/// that moving to it only has to send the picture.
struct Prepared {
    recipe: String,
    rgb: Arc<image::RgbImage>,
    /// pixels of `rgb` per pixel of the original photo
    scale: f32,
    frame: Frame,
}

/// Decodes a photo upright at about `edge` pixels and renders its recipe.
fn prepare(db: &Db, id: i64, edge: u32) -> Res<Prepared> {
    let p = db.photo(id).map_err(err)?.ok_or("photo not found")?;
    let path = db.photo_path(id).map_err(err)?.ok_or("photo not found")?;
    let rgb = wic::orient(wic::decode(&path, Some(edge)).map_err(err)?, p.orientation);
    let scale = (rgb.width().max(rgb.height()) as f32 / p.width.max(p.height).max(1) as f32).min(1.0);
    let ops = recipe::parse(&p.recipe);
    let frame =
        if ops.is_empty() { frame_of(&rgb)? } else { frame_of(&render::apply_scaled(&Img::from_rgb8(&rgb), &ops, scale).to_rgb8())? };
    Ok(Prepared { recipe: p.recipe, rgb: Arc::new(rgb), scale, frame })
}

type Slot = Arc<std::sync::OnceLock<Option<Arc<Prepared>>>>;

/// One photo open in the Edit view, with its undo history.
struct Session {
    photo_id: i64,
    /// the decode size asked for (to reopen the photo after Save/Revert)
    edge: u32,
    /// the upright original, decoded at screen size
    base_rgb: Arc<image::RgbImage>,
    /// the same as floats, made on the first edit
    base: Option<Img>,
    /// half-size copy for quick previews while a slow effect's slider moves
    small: Option<Img>,
    /// pixels of the decoded picture per pixel of the original
    scale: f32,
    /// every recipe state; `history[pos]` is the current one
    history: Vec<Vec<Op>>,
    /// `labels[i]` names the action that produced `history[i]`
    labels: Vec<String>,
    pos: usize,
    /// renders of recipe prefixes (all but the last fix), so a slider only redoes its own fix:
    /// (half size?, prefix, render); the last three used
    cache: Vec<(bool, Vec<Op>, Img)>,
}

/// The photo's Fill Light amount, wherever it sits in the recipe (there is at most one).
fn fill_of(ops: &[Op]) -> f32 {
    ops.iter().find_map(|o| if let Op::Fill(a) = o { Some(*a) } else { None }).unwrap_or(0.0)
}

/// The recipe with its Fill Light set to `a`: changed in place if present (so the slider
/// always edits the same fix), appended if not, removed at 0.
fn with_fill(ops: &[Op], a: f32) -> Vec<Op> {
    let mut out = ops.to_vec();
    match out.iter().position(|o| matches!(o, Op::Fill(_))) {
        Some(i) if a > 0.0 => out[i] = Op::Fill(a),
        Some(i) => {
            out.remove(i);
        }
        None if a > 0.0 => out.push(Op::Fill(a)),
        None => {}
    }
    out
}

/// Effects whose sliders are previewed at half size while they move, then refined.
fn is_slow(op: &Op) -> bool {
    matches!(op, Op::Sharpen(_) | Op::Glow { .. } | Op::SoftFocus { .. } | Op::Grain { .. })
}

impl Session {
    fn ops(&self) -> &[Op] {
        &self.history[self.pos]
    }

    fn render(&mut self, ops: &[Op]) -> Img {
        self.render_at(ops, false)
    }

    /// Renders `ops`; `quick` uses the half-size copy (for a slider being dragged).
    fn render_at(&mut self, ops: &[Op], quick: bool) -> Img {
        let (prefix, last) = match ops.split_last() {
            Some((l, p)) => (p, Some(l)),
            None => (ops, None),
        };
        let scale = if quick { self.scale / 2.0 } else { self.scale };
        let img = match self.cache.iter().position(|(q, p, _)| *q == quick && p.as_slice() == prefix) {
            Some(i) => {
                let hit = self.cache.remove(i);
                let img = hit.2.clone();
                self.cache.push(hit);
                img
            }
            None => {
                let base = if quick {
                    let rgb = &self.base_rgb;
                    self.small
                        .get_or_insert_with(|| {
                            let half = image::imageops::resize(
                                rgb.as_ref(),
                                (rgb.width() / 2).max(1),
                                (rgb.height() / 2).max(1),
                                image::imageops::FilterType::Triangle,
                            );
                            Img::from_rgb8(&half)
                        })
                        .clone()
                } else {
                    let rgb = &self.base_rgb;
                    self.base.get_or_insert_with(|| Img::from_rgb8(rgb)).clone()
                };
                let img = render::apply_scaled(&base, prefix, scale);
                self.cache.push((quick, prefix.to_vec(), img.clone()));
                if self.cache.len() > 3 {
                    self.cache.remove(0);
                }
                img
            }
        };
        match last {
            Some(op) => render::apply_one_scaled(img, op, scale),
            None => img,
        }
    }

    /// Records a new recipe state for Undo/Redo.
    fn push(&mut self, ops: Vec<Op>, label: &str) {
        self.history.truncate(self.pos + 1);
        self.labels.truncate(self.pos + 1);
        self.history.push(ops);
        self.labels.push(label.to_string());
        self.pos += 1;
    }
}

pub struct App {
    db: Arc<Db>,
    jobs: Mutex<mpsc::Sender<Job>>,
    session: Mutex<Option<Session>>,
    preview: Mutex<Option<Frame>>,
    /// the latest strip of effect thumbnails (JPEG)
    fx: Mutex<Option<Arc<Vec<u8>>>>,
    version: AtomicU64,
    watcher: Mutex<Option<watch::Watch>>,
    /// the Folder Manager's marks and the Options (saved in settings.json)
    settings: Mutex<casapic_core::settings::Settings>,
    /// photos prepared ahead, keyed by (id, edge); at most PREPARED_MAX, oldest dropped
    prepared: Mutex<std::collections::VecDeque<((i64, u32), Slot)>>,
}

/// the photo on screen, three ahead and one behind, plus one spare (~10 MB each at 2048 px)
const PREPARED_MAX: usize = 6;

impl App {
    fn queue(&self, job: Job) {
        let _ = self.jobs.lock().expect("jobs lock").send(job);
    }

    /// Stores a picture for the protocol and returns its version number.
    fn publish(&self, f: Frame) -> u64 {
        *self.preview.lock().expect("preview lock") = Some(f);
        self.version.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// The slot for a prepared photo, and whether this call created it (and must fill it).
    fn slot(&self, key: (i64, u32)) -> (Slot, bool) {
        let mut q = self.prepared.lock().expect("prepared lock");
        if let Some((_, s)) = q.iter().find(|(k, _)| *k == key) {
            return (s.clone(), false);
        }
        let s: Slot = Arc::new(std::sync::OnceLock::new());
        q.push_back((key, s.clone()));
        while q.len() > PREPARED_MAX {
            q.pop_front();
        }
        (s, true)
    }

    /// A prepared photo: from the cache (waiting if it is being prepared right now) or made now.
    fn prepared(&self, id: i64, edge: u32) -> Res<Arc<Prepared>> {
        let (slot, mine) = self.slot((id, edge));
        if mine {
            let r = prepare(&self.db, id, edge).map(Arc::new);
            let _ = slot.set(r.as_ref().ok().cloned());
            if r.is_err() {
                self.forget(id);
            }
            return r;
        }
        slot.wait().clone().ok_or_else(|| "could not prepare photo".to_string())
    }

    /// Drops prepared copies of a photo (after its recipe changed, or a failure).
    fn forget(&self, id: i64) {
        self.prepared.lock().expect("prepared lock").retain(|((pid, _), _)| *pid != id);
    }

    /// Writes a photo's recipe to its folder's sidecar and the index, then queues a new thumbnail.
    fn save_recipe(&self, id: i64, ops: &[Op]) -> Res<()> {
        let text = recipe::to_string(ops);
        let path = self.db.photo_path(id).map_err(err)?.ok_or("photo not found")?;
        let (folder, name) = folder_and_name(&path)?;
        sidecar::update(folder, &name, |e| e.filters = text.clone()).map_err(err)?;
        self.db.set_recipe(id, &text).map_err(err)?;
        self.forget(id);
        self.queue(Job::Regen(vec![id]));
        Ok(())
    }
}

fn folder_and_name(path: &Path) -> Res<(&Path, String)> {
    let folder = path.parent().ok_or("photo has no folder")?;
    let name = path.file_name().ok_or("photo has no name")?.to_string_lossy().to_string();
    Ok((folder, name))
}

/// Appends a quarter turn, merging it with a turn just before it (four turns = none).
fn push_rotate(ops: &mut Vec<Op>, q: i32) {
    if let Some(Op::Rotate(r)) = ops.last_mut() {
        *r = (*r + q).rem_euclid(4);
        if *r == 0 {
            ops.pop();
        }
    } else if q.rem_euclid(4) != 0 {
        ops.push(Op::Rotate(q.rem_euclid(4)));
    }
}

// ---------- library ----------

#[derive(Serialize)]
struct FolderOut {
    id: i64,
    name: String,
    path: String,
    /// newest photo (the folder list sorts and groups by it)
    date: Option<i64>,
    /// oldest photo (shown under the folder title, as Picasa does)
    first: Option<i64>,
    /// "Add a description" text
    description: String,
    /// photo order (Folder > Sort By): "" = date, "name", "size"; "-" prefix = reversed
    sort: String,
    /// the Folder Manager folder it is scanned from (the top of its branch in the tree view)
    root: String,
    /// newest file change (seconds), for sorting folders by Recent Changes
    changed: i64,
    /// total size of its photos, for sorting folders by Size
    bytes: i64,
    /// one row per photo: [id, starred (0/1), thumbnail level]
    photos: Vec<[i64; 3]>,
}

#[tauri::command(async)]
fn library(app: State<'_, App>) -> Res<Vec<FolderOut>> {
    mark("library-cmd-start");
    let folders = app.db.folders().map_err(err)?;
    let mut out = Vec::with_capacity(folders.len());
    for f in folders {
        let mut photos = app.db.photos(f.id).map_err(err)?;
        let reverse = f.sort.starts_with('-');
        match f.sort.trim_start_matches('-') {
            "name" => photos.sort_by_key(|p| p.name.to_lowercase()),
            "size" => photos.sort_by_key(|p| p.size),
            _ => {}
        }
        if reverse {
            photos.reverse();
        }
        out.push(FolderOut {
            id: f.id,
            name: f.name,
            path: f.path,
            description: f.description,
            sort: f.sort,
            root: f.root,
            changed: photos.iter().map(|p| p.mtime).max().unwrap_or(0),
            bytes: photos.iter().map(|p| p.size).sum(),
            date: f.date_max,
            first: f.date_min,
            photos: photos.iter().map(|p| [p.id, p.star as i64, p.thumb_level]).collect(),
        });
    }
    Ok(out)
}

#[derive(Serialize)]
struct PhotoOut {
    id: i64,
    name: String,
    path: String,
    folder: String,
    size: i64,
    width: i64,
    height: i64,
    taken: Option<i64>,
    mtime: i64,
    camera: Option<String>,
    star: bool,
    /// names of the edits in its recipe, in order
    edits: Vec<String>,
}

#[tauri::command(async)]
fn photo(app: State<'_, App>, id: i64) -> Res<PhotoOut> {
    let p = app.db.photo(id).map_err(err)?.ok_or("photo not found")?;
    let path = app.db.photo_path(id).map_err(err)?.ok_or("photo not found")?;
    let folder = path.parent().and_then(|f| f.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    // width/height as shown: upright, then after the photo's edits (rotate, crop, straighten)
    let (w, h) = if (5..=8).contains(&p.orientation) { (p.height, p.width) } else { (p.width, p.height) };
    let ops = recipe::parse(&p.recipe);
    let (w, h) = render::output_size(w as f32, h as f32, &ops);
    let (width, height) = (w.round() as i64, h.round() as i64);
    Ok(PhotoOut {
        id,
        name: p.name,
        path: path.to_string_lossy().to_string(),
        folder,
        size: p.size,
        width,
        height,
        taken: p.taken,
        mtime: p.mtime,
        camera: p.camera,
        star: p.star,
        edits: ops.iter().map(|o| o.label().to_string()).collect(),
    })
}

#[tauri::command(async)]
fn set_star(app: State<'_, App>, ids: Vec<i64>, star: bool) -> Res<()> {
    for id in ids {
        let path = app.db.photo_path(id).map_err(err)?.ok_or("photo not found")?;
        let (folder, name) = folder_and_name(&path)?;
        sidecar::update(folder, &name, |e| e.star = star).map_err(err)?;
        app.db.set_star(id, star).map_err(err)?;
    }
    Ok(())
}

/// Rotates photos in the library by quarter turns (1 = clockwise, -1 = counter-clockwise).
#[tauri::command(async)]
fn rotate(app: State<'_, App>, ids: Vec<i64>, turns: i32) -> Res<()> {
    for id in ids {
        let p = app.db.photo(id).map_err(err)?.ok_or("photo not found")?;
        let mut ops = recipe::parse(&p.recipe);
        push_rotate(&mut ops, turns);
        app.save_recipe(id, &ops)?;
    }
    Ok(())
}

/// Copy All Effects: the photo's effects as recipe text (its Basic Fixes that change colour
/// and tone, Tuning and the Effects tab; rotation, crop and straighten stay with the photo,
/// as in Picasa).
#[tauri::command(async)]
fn copy_effects(app: State<'_, App>, id: i64) -> Res<String> {
    let p = app.db.photo(id).map_err(err)?.ok_or("photo not found")?;
    let ops: Vec<Op> = recipe::parse(&p.recipe).into_iter().filter(Op::is_effect).collect();
    Ok(recipe::to_string(&ops))
}

/// Adds fixes to many photos' recipes. `make` gets a photo's current recipe and a way to render
/// it (only called by fixes that look at the picture) and returns the fixes to add. Fixes are
/// worked out in parallel; the sidecars are written one photo at a time.
fn add_to_photos(app: &App, ids: &[i64], make: impl Fn(&[Op], &dyn Fn() -> Img) -> Res<Vec<Op>> + Sync) -> Res<usize> {
    let jobs: Vec<(i64, Vec<Op>, PathBuf, i64)> = ids
        .iter()
        .map(|&id| {
            let p = app.db.photo(id).map_err(err)?.ok_or("photo not found")?;
            let path = app.db.photo_path(id).map_err(err)?.ok_or("photo not found")?;
            Ok((id, recipe::parse(&p.recipe), path, p.orientation))
        })
        .collect::<Res<_>>()?;
    let results: Vec<(i64, Vec<Op>)> = jobs
        .par_iter()
        .map(|(id, ops, path, orientation)| {
            // the picture as it looks now, about 1024 pixels across (what the auto fixes measure)
            let look = || -> Img {
                let rgb = wic::decode(path, Some(1024)).map(|r| wic::orient(r, *orientation));
                match rgb {
                    Ok(rgb) => {
                        let long = app.db.photo(*id).ok().flatten().map(|p| p.width.max(p.height)).unwrap_or(0);
                        let scale = (rgb.width().max(rgb.height()) as f32 / long.max(1) as f32).min(1.0);
                        render::apply_scaled(&Img::from_rgb8(&rgb), ops, scale)
                    }
                    Err(_) => Img::from_rgb8(&image::RgbImage::new(1, 1)),
                }
            };
            let mut out = ops.clone();
            out.extend(make(ops, &look)?);
            Ok((*id, out))
        })
        .collect::<Res<_>>()?;
    let mut changed = 0;
    for ((id, new), (_, old, _, _)) in results.iter().zip(&jobs) {
        if new != old {
            app.save_recipe(*id, new)?;
            changed += 1;
        }
    }
    Ok(changed)
}

/// Paste All Effects: adds the copied effects (recipe text) on top of each photo's own edits.
#[tauri::command(async)]
fn paste_effects(app: State<'_, App>, ids: Vec<i64>, text: String) -> Res<usize> {
    let add: Vec<Op> = ops_from_text(&text)?.into_iter().filter(Op::is_effect).collect();
    if add.is_empty() {
        return Err("no effects were copied".into());
    }
    add_to_photos(&app, &ids, |_, _| Ok(add.clone()))
}

/// Batch Edit: one fix on every photo in `ids`. `kind` is lucky | autocontrast | autocolor
/// (each measured on that photo) or recipe text for a fixed effect (e.g. `sepia=1;`).
#[tauri::command(async)]
fn batch_edit(app: State<'_, App>, ids: Vec<i64>, kind: String) -> Res<usize> {
    match kind.as_str() {
        "lucky" | "autocontrast" | "autocolor" => add_to_photos(&app, &ids, |_, look| {
            let img = look();
            Ok(vec![match kind.as_str() {
                "autocontrast" => render::contrast_op(&img),
                "autocolor" => render::color_op(&img),
                _ => render::lucky_op(&img),
            }])
        }),
        text => {
            let add = ops_from_text(text)?;
            add_to_photos(&app, &ids, |_, _| Ok(add.clone()))
        }
    }
}

#[derive(Serialize)]
struct Counts {
    folders: i64,
    photos: i64,
}

#[tauri::command(async)]
fn counts(app: State<'_, App>) -> Res<Counts> {
    let (folders, photos) = app.db.counts().map_err(err)?;
    Ok(Counts { folders, photos })
}

/// Opens Explorer with the photo selected.
#[tauri::command(async)]
fn locate_on_disk(app: State<'_, App>, id: i64) -> Res<()> {
    use std::os::windows::process::CommandExt;
    let path = app.db.photo_path(id).map_err(err)?.ok_or("photo not found")?;
    std::process::Command::new("explorer")
        .raw_arg(format!("/select,\"{}\"", path.display()))
        .spawn()
        .map_err(err)?;
    Ok(())
}

// ---------- Edit view ----------

#[derive(Serialize)]
struct EditState {
    id: i64,
    version: u64,
    w: u32,
    h: u32,
    /// "Fill Light" when Undo would undo Fill Light
    undo: Option<String>,
    redo: Option<String>,
    /// current Fill Light amount (0–1), for the slider
    fill: f32,
    edits: Vec<String>,
    /// histogram of the picture shown: 64 bins each for R, G, B
    hist: Arc<Vec<u32>>,
    /// a half-size preview while a slider moves (the full one follows)
    quick: bool,
    /// the photo has a saved edit and its original is kept (Revert is possible)
    saved: bool,
    /// the recipe's last fix as recipe text (the Tuning tab shows its values when it is Tuning)
    last: String,
}

/// Publishes `f` and describes the session around it.
fn state_with(app: &App, s: &Session, f: Frame) -> EditState {
    let (w, h, hist) = (f.w, f.h, f.hist.clone());
    EditState {
        id: s.photo_id,
        version: app.publish(f),
        w,
        h,
        undo: (s.pos > 0).then(|| s.labels[s.pos].clone()),
        redo: (s.pos + 1 < s.history.len()).then(|| s.labels[s.pos + 1].clone()),
        fill: fill_of(s.ops()),
        edits: s.ops().iter().map(|o| o.label().to_string()).collect(),
        hist,
        quick: false,
        last: s.ops().last().map(|o| recipe::to_string(std::slice::from_ref(o))).unwrap_or_default(),
        saved: app
            .db
            .photo_path(s.photo_id)
            .ok()
            .flatten()
            .and_then(|p| folder_and_name(&p).ok().map(|(f, n)| save::is_saved(f, &n)))
            .unwrap_or(false),
    }
}

fn state_of(app: &App, s: &mut Session) -> Res<EditState> {
    let ops = s.ops().to_vec();
    let f = frame_of(&s.render(&ops).to_rgb8())?;
    Ok(state_with(app, s, f))
}

/// Screen-size decodes are rounded so neighbours prepared ahead match the next request.
fn edit_edge(edge: u32) -> u32 {
    (edge.clamp(400, 4096).div_ceil(256) * 256).min(4096)
}

/// Opens a photo in the Edit view, decoded so its longer edge is about `edge` pixels.
#[tauri::command(async)]
fn edit_open(app: State<'_, App>, id: i64, edge: u32) -> Res<EditState> {
    open_session(&app, id, edge)
}

fn open_session(app: &App, id: i64, edge: u32) -> Res<EditState> {
    let t = std::time::Instant::now();
    let edge = edit_edge(edge);
    let recipe_now = app.db.photo(id).map_err(err)?.ok_or("photo not found")?.recipe;
    let mut pre = app.prepared(id, edge)?;
    let hit = t.elapsed().as_secs_f32() * 1000.0;
    if pre.recipe != recipe_now {
        app.forget(id);
        pre = app.prepared(id, edge)?;
    }
    let s = Session {
        photo_id: id,
        edge,
        base_rgb: pre.rgb.clone(),
        base: None,
        scale: pre.scale,
        history: vec![recipe::parse(&recipe_now)],
        labels: vec![String::new()],
        pos: 0,
        small: None,
        cache: Vec::new(),
    };
    let st = state_with(app, &s, pre.frame.clone());
    *app.session.lock().map_err(err)? = Some(s);
    mark(&format!("edit-open-backend prepared-or-wait={hit:.1} total={:.1}", t.elapsed().as_secs_f32() * 1000.0));
    Ok(st)
}

/// Prepares photos in the background (the neighbours of the one on screen).
#[tauri::command]
fn edit_prepare(h: AppHandle, ids: Vec<i64>, edge: u32) {
    let edge = edit_edge(edge);
    for id in ids {
        let h = h.clone();
        rayon::spawn(move || {
            let _ = h.state::<App>().prepared(id, edge);
        });
    }
}

/// Applies one Basic Fix. `kind` is rotate | crop | straighten | autocontrast | autocolor |
/// lucky | fill | revert; `params` holds its numbers (crop: x0,y0,x1,y1 as 0–1 fractions of
/// the picture as currently shown; straighten: degrees; fill: 0–1; rotate: quarter turns).
#[tauri::command(async)]
fn edit_apply(app: State<'_, App>, kind: String, params: Vec<f32>) -> Res<EditState> {
    let mut guard = app.session.lock().map_err(err)?;
    let s = guard.as_mut().ok_or("no photo open")?;
    let mut ops = s.ops().to_vec();
    let p = |i: usize| params.get(i).copied().ok_or_else(|| format!("{kind}: missing value {i}"));
    let label = match kind.as_str() {
        "rotate" => {
            push_rotate(&mut ops, p(0)? as i32);
            "Rotate"
        }
        "crop" => {
            let (x0, y0, x1, y1) = (p(0)?, p(1)?, p(2)?, p(3)?);
            let ok = (0.0..=1.0).contains(&x0) && (0.0..=1.0).contains(&y1) && x1 <= 1.0 && y0 >= 0.0;
            if !ok || x1 - x0 < 0.01 || y1 - y0 < 0.01 {
                return Err("crop rectangle is outside the picture or too small".into());
            }
            ops.push(Op::Crop { x0, y0, x1, y1 });
            "Crop"
        }
        "straighten" => {
            let a = p(0)?;
            if a.abs() < 0.01 {
                return state_of(&app, s);
            }
            ops.push(Op::Straighten(a.clamp(-45.0, 45.0)));
            "Straighten"
        }
        "autocontrast" | "autocolor" | "lucky" => {
            let img = s.render(&ops);
            ops.push(match kind.as_str() {
                "autocontrast" => render::contrast_op(&img),
                "autocolor" => render::color_op(&img),
                _ => render::lucky_op(&img),
            });
            ops.last().expect("just pushed").label()
        }
        "fill" => {
            ops = with_fill(&ops, p(0)?.clamp(0.0, 1.0));
            "Fill Light"
        }
        "revert" => {
            ops.clear();
            "Undo All Edits"
        }
        other => return Err(format!("unknown fix: {other}")),
    };
    if ops.as_slice() != s.ops() {
        s.push(ops.clone(), label);
        app.save_recipe(s.photo_id, &ops)?;
    }
    state_of(&app, s)
}

/// Parses fixes written as recipe text (Picasa's entry format, e.g. `sat=1,0.3;`).
fn ops_from_text(text: &str) -> Res<Vec<Op>> {
    let ops = recipe::parse(text);
    if let Some(Op::Unknown(u)) = ops.iter().find(|o| matches!(o, Op::Unknown(_))) {
        return Err(format!("not a fix CasaPic knows: {u}"));
    }
    Ok(ops)
}

/// Shows the photo with `text` (fixes in recipe text) added on top, without saving or adding
/// to Undo: the live preview while an effect's or the Tuning tab's sliders move. With
/// `replace_last`, they replace the recipe's last fix instead (re-tuning the same Tuning
/// entry). `quick` renders slow effects at half size while a slider is being dragged.
#[tauri::command(async)]
fn edit_try(app: State<'_, App>, text: String, replace_last: bool, quick: bool) -> Res<EditState> {
    let mut guard = app.session.lock().map_err(err)?;
    let s = guard.as_mut().ok_or("no photo open")?;
    let extra = ops_from_text(&text)?;
    let mut ops = s.ops().to_vec();
    if replace_last {
        ops.pop();
    }
    let quick = quick && extra.iter().any(is_slow);
    ops.extend(extra);
    let f = frame_of(&s.render_at(&ops, quick).to_rgb8())?;
    let mut st = state_with(&app, s, f);
    st.quick = quick;
    Ok(st)
}

/// Adds `text` (fixes in recipe text) to the photo's recipe and saves it; with `replace_last`
/// they replace the last fix (the Tuning tab edits one entry per visit, as Picasa does).
#[tauri::command(async)]
fn edit_add(app: State<'_, App>, text: String, replace_last: bool) -> Res<EditState> {
    let mut guard = app.session.lock().map_err(err)?;
    let s = guard.as_mut().ok_or("no photo open")?;
    let extra = ops_from_text(&text)?;
    let label = extra.first().map(|o| o.label()).unwrap_or("Edit");
    let mut ops = s.ops().to_vec();
    if replace_last {
        ops.pop();
    }
    ops.extend(extra);
    if ops.as_slice() != s.ops() {
        s.push(ops.clone(), label);
        app.save_recipe(s.photo_id, &ops)?;
    }
    state_of(&app, s)
}

/// The photo's colour around (x, y) (fractions of the picture), as rendered by its recipe
/// without the last `skip` fixes: for the Neutral Color Picker.
#[tauri::command(async)]
fn edit_sample(app: State<'_, App>, x: f32, y: f32, skip: usize) -> Res<[f32; 3]> {
    let mut guard = app.session.lock().map_err(err)?;
    let s = guard.as_mut().ok_or("no photo open")?;
    let ops = s.ops().to_vec();
    let img = s.render(&ops[..ops.len().saturating_sub(skip)]);
    let (cx, cy) = ((x.clamp(0.0, 1.0) * img.w as f32) as isize, (y.clamp(0.0, 1.0) * img.h as f32) as isize);
    let (mut sum, mut n) = ([0f32; 3], 0f32);
    for yy in (cy - 2)..=(cy + 2) {
        for xx in (cx - 2)..=(cx + 2) {
            if xx >= 0 && yy >= 0 && (xx as usize) < img.w && (yy as usize) < img.h {
                let i = (yy as usize * img.w + xx as usize) * 3;
                for c in 0..3 {
                    sum[c] += img.px[i + c].clamp(0.0, 1.0);
                }
                n += 1.0;
            }
        }
    }
    Ok(sum.map(|v| v / n.max(1.0)))
}

/// The Tuning tab's one-click buttons, computed on the photo as rendered without the last
/// `skip` fixes. "light": [highlights, shadows] = Auto Contrast's white and black points
/// (matched the values Picasa wrote for a lab photo exactly). "color": the neutral colour
/// R, G, B (0–1) from Auto Color's estimate of the cast (approximate, as Auto Color is).
#[tauri::command(async)]
fn edit_magic(app: State<'_, App>, kind: String, skip: usize) -> Res<Vec<f32>> {
    let mut guard = app.session.lock().map_err(err)?;
    let s = guard.as_mut().ok_or("no photo open")?;
    let ops = s.ops().to_vec();
    let img = s.render(&ops[..ops.len().saturating_sub(skip)]);
    match kind.as_str() {
        "light" => match render::contrast_op(&img) {
            Op::AutoContrast { lo, hi } => Ok(vec![1.0 - hi, lo]),
            _ => Err("no levels".into()),
        },
        "color" => match render::color_op(&img) {
            Op::AutoColor { gain } => Ok(gain.map(|g| 0.5 / g.max(0.1)).to_vec()),
            _ => Err("no colour".into()),
        },
        other => Err(format!("unknown button: {other}")),
    }
}

/// Renders each of `texts` (one fix each) on a small square copy of the photo as it is now,
/// side by side in one strip of `size`-pixel tiles: the effect buttons' live thumbnails.
/// Returns a version number for `/fx/<version>`.
#[tauri::command(async)]
fn edit_fx_sheet(app: State<'_, App>, texts: Vec<String>, size: u32) -> Res<u64> {
    let size = size.clamp(32, 256);
    let (base, scale) = {
        let mut guard = app.session.lock().map_err(err)?;
        let s = guard.as_mut().ok_or("no photo open")?;
        let ops = s.ops().to_vec();
        let img = s.render(&ops).to_rgb8();
        // fill the square, as Picasa's buttons do
        let k = size as f32 / img.width().min(img.height()).max(1) as f32;
        let (w, h) = (((img.width() as f32 * k).round() as u32).max(size), ((img.height() as f32 * k).round() as u32).max(size));
        let r = image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle);
        let r = image::imageops::crop_imm(&r, (w - size) / 2, (h - size) / 2, size, size).to_image();
        (Img::from_rgb8(&r), s.scale * k)
    };
    let tiles: Vec<image::RgbImage> =
        texts.par_iter().map(|t| render::apply_scaled(&base, &recipe::parse(t), scale).to_rgb8()).collect();
    let mut sheet = image::RgbImage::new(size * tiles.len().max(1) as u32, size);
    for (i, tile) in tiles.iter().enumerate() {
        image::imageops::replace(&mut sheet, tile, (i as u32 * size) as i64, 0);
    }
    let jpeg = thumbs::encode_jpeg(&sheet, 88).map_err(err)?;
    *app.fx.lock().map_err(err)? = Some(Arc::new(jpeg));
    Ok(app.version.fetch_add(1, Ordering::Relaxed) + 1)
}


/// Shows a Fill Light amount while the slider moves, without saving or adding to Undo.
#[tauri::command(async)]
fn edit_fill_preview(app: State<'_, App>, value: f32) -> Res<EditState> {
    let mut guard = app.session.lock().map_err(err)?;
    let s = guard.as_mut().ok_or("no photo open")?;
    let ops = with_fill(s.ops(), value.clamp(0.0, 1.0));
    let f = frame_of(&s.render(&ops).to_rgb8())?;
    let mut st = state_with(&app, s, f);
    st.fill = value;
    Ok(st)
}

#[tauri::command(async)]
fn edit_undo(app: State<'_, App>, redo: bool) -> Res<EditState> {
    let mut guard = app.session.lock().map_err(err)?;
    let s = guard.as_mut().ok_or("no photo open")?;
    let moved = if redo && s.pos + 1 < s.history.len() {
        s.pos += 1;
        true
    } else if !redo && s.pos > 0 {
        s.pos -= 1;
        true
    } else {
        false
    };
    if moved {
        let ops = s.ops().to_vec();
        app.save_recipe(s.photo_id, &ops)?;
    }
    state_of(&app, s)
}

// ---------- Save / Revert ----------

/// Re-reads a photo's folder after its file changed (size, date, orientation now upright) and
/// makes a new thumbnail.
fn photo_file_changed(app: &App, id: i64, folder: &Path) {
    folders::rescan_folder(app, folder);
    app.forget(id);
    app.queue(Job::Regen(vec![id]));
}

/// Save: writes the open photo's edits into the photo file, keeping the untouched original in
/// the hidden `.casapicoriginals` folder beside it (core `save.rs`). The photo reopens with an
/// empty recipe, since its edits are now in the pixels.
#[tauri::command(async)]
fn edit_save(app: State<'_, App>) -> Res<EditState> {
    let (id, edge) = {
        let guard = app.session.lock().map_err(err)?;
        let s = guard.as_ref().ok_or("no photo open")?;
        let p = app.db.photo(s.photo_id).map_err(err)?.ok_or("photo not found")?;
        let path = app.db.photo_path(s.photo_id).map_err(err)?.ok_or("photo not found")?;
        let (folder, name) = folder_and_name(&path)?;
        save::save(folder, &name, p.orientation, s.ops()).map_err(err)?;
        app.db.set_recipe(s.photo_id, "").map_err(err)?;
        photo_file_changed(&app, s.photo_id, folder);
        (s.photo_id, s.edge)
    };
    open_session(&app, id, edge)
}

/// Revert: puts the original back exactly as it was before the first Save; the saved edits
/// are dropped.
#[tauri::command(async)]
fn edit_revert(app: State<'_, App>) -> Res<EditState> {
    let (id, edge) = {
        let guard = app.session.lock().map_err(err)?;
        let s = guard.as_ref().ok_or("no photo open")?;
        let path = app.db.photo_path(s.photo_id).map_err(err)?.ok_or("photo not found")?;
        let (folder, name) = folder_and_name(&path)?;
        if !save::revert(folder, &name).map_err(err)? {
            return Err("this photo has no saved edit to revert".into());
        }
        app.db.set_recipe(s.photo_id, "").map_err(err)?;
        photo_file_changed(&app, s.photo_id, folder);
        (s.photo_id, s.edge)
    };
    open_session(&app, id, edge)
}

// ---------- export ----------

#[tauri::command(async)]
fn default_export_dir(app: State<'_, App>, id: i64) -> Res<String> {
    let path = app.db.photo_path(id).map_err(err)?.ok_or("photo not found")?;
    let folder = path.parent().and_then(|f| f.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    Ok(paths::export_dir(&folder).to_string_lossy().to_string())
}

/// Exports photos with their edits as new JPEG files. `dest` = None puts each one in the
/// default export folder named after its folder. Returns the files written.
#[tauri::command(async)]
fn export_photos(app: State<'_, App>, ids: Vec<i64>, dest: Option<String>, max_edge: Option<u32>, quality: u8) -> Res<Vec<String>> {
    let opt = export::ExportOptions { max_edge, quality: quality.clamp(1, 100) };
    let jobs: Vec<(PathBuf, i64, Vec<Op>)> = ids
        .iter()
        .map(|&id| {
            let p = app.db.photo(id).map_err(err)?.ok_or("photo not found")?;
            let path = app.db.photo_path(id).map_err(err)?.ok_or("photo not found")?;
            Ok((path, p.orientation, recipe::parse(&p.recipe)))
        })
        .collect::<Res<_>>()?;
    jobs.par_iter()
        .map(|(path, o, ops)| {
            let dir = match &dest {
                Some(d) => PathBuf::from(d),
                None => {
                    let folder = path.parent().and_then(|f| f.file_name()).map(|n| n.to_string_lossy().to_string());
                    paths::export_dir(&folder.unwrap_or_default())
                }
            };
            std::fs::create_dir_all(&dir).map_err(err)?;
            export::export(path, *o, ops, &dir, &opt).map(|p| p.to_string_lossy().to_string()).map_err(err)
        })
        .collect()
}

// ---------- protocol ----------

fn respond(status: StatusCode, ctype: &str, cache: &str, body: Vec<u8>, extra: &[(&str, String)]) -> Response<Vec<u8>> {
    let mut b = Response::builder()
        .status(status)
        .header("Content-Type", ctype)
        .header("Cache-Control", cache)
        .header("Access-Control-Allow-Origin", "*");
    for (k, v) in extra {
        b = b.header(*k, v);
    }
    b.body(body).expect("valid response")
}

fn serve(app: &App, path: &str) -> Response<Vec<u8>> {
    let not_found = || respond(StatusCode::NOT_FOUND, "text/plain", "no-store", Vec::new(), &[]);
    let mut parts = path.trim_start_matches('/').split('/');
    match (parts.next(), parts.next()) {
        (Some("thumb"), Some(id)) => match id.parse::<i64>().ok().and_then(|id| app.db.thumb(id).ok().flatten()) {
            // the page adds ?v=<n> whenever a thumbnail changes, so a long cache is safe
            Some((_, jpeg)) => respond(StatusCode::OK, "image/jpeg", "max-age=31536000", jpeg, &[]),
            None => not_found(),
        },
        (Some("fx"), _) => match app.fx.lock().ok().and_then(|p| p.clone()) {
            Some(jpeg) => respond(StatusCode::OK, "image/jpeg", "no-store", jpeg.as_ref().clone(), &[]),
            None => not_found(),
        },
        (Some("preview"), _) => match app.preview.lock().ok().and_then(|p| p.as_ref().map(|p| p.jpeg.clone())) {
            Some(jpeg) => respond(StatusCode::OK, "image/jpeg", "no-store", jpeg.as_ref().clone(), &[]),
            None => not_found(),
        },
        _ => not_found(),
    }
}

// ---------- speed measurement (build-1 step 5; only with CASAPIC_TIMING set) ----------

/// Appends "name unix-ms" to the file named by CASAPIC_TIMING; does nothing otherwise.
fn mark(name: &str) {
    use std::io::Write as _;
    let Some(p) = std::env::var_os("CASAPIC_TIMING") else { return };
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
        let _ = writeln!(f, "{name} {ms}");
    }
}

#[tauri::command]
fn timing_mark(name: String) {
    mark(&name);
}

/// "off", "timing" (marks only) or "bench" (the window also steps through photos and quits).
#[tauri::command]
fn timing_mode() -> &'static str {
    match (std::env::var_os("CASAPIC_TIMING").is_some(), std::env::var_os("CASAPIC_BENCH").is_some()) {
        (false, _) => "off",
        (true, false) => "timing",
        (true, true) => "bench",
    }
}

#[tauri::command]
fn quit(h: AppHandle) {
    h.exit(0);
}

// ---------- background work ----------

fn emit_status(h: &AppHandle, text: &str) {
    let _ = h.emit("status", text.to_string());
}

fn thumb_worker(h: AppHandle, db: Arc<Db>, rx: mpsc::Receiver<Job>) {
    while let Ok(first) = rx.recv() {
        let (mut regen, mut pending) = (Vec::new(), false);
        for job in std::iter::once(first).chain(rx.try_iter()) {
            match job {
                Job::Pending => pending = true,
                Job::Regen(ids) => regen.extend(ids),
            }
        }
        let emit = |ids: &[i64]| {
            let _ = h.emit("thumbs-updated", ids.to_vec());
        };
        if !regen.is_empty() {
            regen.sort_unstable();
            regen.dedup();
            let _ = thumbs::generate(&db, &regen, emit);
        }
        if pending {
            loop {
                let ids = db.needing_thumbs(thumbs::LEVEL_FULL, 256).unwrap_or_default();
                if ids.is_empty() {
                    break;
                }
                emit_status(&h, "Making thumbnails…");
                if thumbs::generate(&db, &ids, emit).is_err() {
                    break;
                }
            }
            emit_status(&h, "");
            mark("thumbs-done");
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    mark("process-start");
    let data = paths::data_dir();
    std::fs::create_dir_all(&data).expect("cannot create the CasaPic data folder");
    let db = Arc::new(Db::open(&data.join("index.db")).expect("cannot open the photo index"));
    let (tx, rx) = mpsc::channel();
    let app_state = App {
        db: db.clone(),
        jobs: Mutex::new(tx),
        session: Mutex::new(None),
        preview: Mutex::new(None),
        fx: Mutex::new(None),
        version: AtomicU64::new(0),
        watcher: Mutex::new(None),
        settings: Mutex::new(casapic_core::settings::Settings::load()),
        prepared: Mutex::new(std::collections::VecDeque::new()),
    };
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .register_asynchronous_uri_scheme_protocol("casapic", |ctx, request, responder| {
            let h = ctx.app_handle().clone();
            let path = request.uri().path().to_string();
            tauri::async_runtime::spawn_blocking(move || responder.respond(serve(&h.state::<App>(), &path)));
        })
        .setup(move |a| {
            mark("setup");
            let h = a.handle().clone();
            let h2 = h.clone();
            std::thread::spawn(move || thumb_worker(h2, db, rx));
            std::thread::spawn(move || folders::sync_library(&h, false));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            library,
            photo,
            set_star,
            rotate,
            counts,
            locate_on_disk,
            edit_open,
            edit_prepare,
            edit_apply,
            edit_fill_preview,
            edit_try,
            edit_add,
            edit_sample,
            edit_magic,
            edit_fx_sheet,
            edit_save,
            edit_revert,
            copy_effects,
            paste_effects,
            batch_edit,
            edit_undo,
            default_export_dir,
            export_photos,
            timing_mark,
            timing_mode,
            quit,
            folders::settings_get,
            folders::fm_list,
            folders::fm_save,
            folders::fm_confirm,
            folders::add_folder,
            folders::remove_folder,
            folders::options_save,
            folders::set_folder_description,
            folders::set_folder_sort,
            folders::open_folder,
            files::rename_photo,
            files::delete_photos,
            files::transfer_photos,
            files::move_to_new_folder,
            files::new_folder,
            files::rename_folder,
            files::move_folder,
            files::delete_folder,
            files::save_copy,
            files::adjust_dates,
            files::clear_thumb_cache,
            files::open_with,
        ])
        .run(tauri::generate_context!())
        .expect("error while running CasaPic");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotations_merge_and_cancel() {
        let mut ops = vec![Op::AutoContrast { lo: 0.1, hi: 0.9 }];
        push_rotate(&mut ops, 1);
        push_rotate(&mut ops, 1);
        assert_eq!(ops.last(), Some(&Op::Rotate(2)));
        push_rotate(&mut ops, -1);
        push_rotate(&mut ops, -1);
        assert_eq!(ops, vec![Op::AutoContrast { lo: 0.1, hi: 0.9 }], "four turns back = no rotation");
        push_rotate(&mut ops, -1);
        assert_eq!(ops.last(), Some(&Op::Rotate(3)), "counter-clockwise = three clockwise turns");
    }

    #[test]
    fn fill_light_is_one_fix_edited_in_place() {
        let crop = Op::Crop { x0: 0.0, y0: 0.0, x1: 0.5, y1: 0.5 };
        let ops = with_fill(&[], 0.4);
        assert_eq!(ops, vec![Op::Fill(0.4)]);
        let ops = [ops, vec![crop.clone()]].concat();
        let ops = with_fill(&ops, 0.7);
        assert_eq!(ops, vec![Op::Fill(0.7), crop.clone()], "changed where it is, not appended");
        assert_eq!(fill_of(&ops), 0.7);
        assert_eq!(with_fill(&ops, 0.0), vec![crop], "zero removes it");
    }
}
