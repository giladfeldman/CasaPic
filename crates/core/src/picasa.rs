//! Import of what Picasa 3.9 left in the folders: its `.picasa.ini` files (lab check,
//! `docs/picasa-organize-format.md`). Only reads them, never changes them. Stars, hidden
//! photos and folders, folder descriptions and albums carry over as they are; edits are
//! translated into CasaPic recipes (most use the same names; crop, straighten and rotate are
//! converted; the one-click fixes are worked out from the photo, as Picasa does); edits CasaPic
//! cannot draw are kept in the recipe and listed. Captions and tags that older Picasa versions
//! kept in the file (`caption=`, `keywords=`) are returned so the caller can write them into
//! the photos as CasaPic does.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::albums::{self, Album};
use crate::recipe::{self, Op};
use crate::sidecar::{self, Entry};

pub const FILE_NAME: &str = ".picasa.ini";

/// Degrees of straightening (CasaPic: positive = clockwise) at the end of Picasa's slider
/// (`tilt=1,1.0`); measured against Picasa's exports at 1.0 and 0.50 (linear; format document).
pub const TILT_DEGREES: f32 = -11.40;

/// One fix of a translated recipe.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Op(Op),
    /// a one-click fix Picasa stores without numbers: "autocontrast", "autocolor" or "lucky";
    /// its numbers are measured on the photo as it looks at that point
    Auto(&'static str),
}

#[derive(Debug, Clone, Default)]
pub struct PhotoImport {
    pub name: String,
    pub star: bool,
    pub hidden: bool,
    pub albums: Vec<String>,
    pub steps: Vec<Step>,
    /// Picasa edits CasaPic cannot draw (kept in the recipe as they are)
    pub unknown: Vec<String>,
    pub caption: String,
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct FolderImport {
    pub dir: PathBuf,
    pub description: String,
    pub hidden: bool,
    pub albums: Vec<Album>,
    pub photos: Vec<PhotoImport>,
    /// photos saved in Picasa: their edits are already in the pixels and are not applied again
    pub saved_in_picasa: usize,
}

/// `crop64=1,<hex>`: four 16-bit fractions (left, top, right, bottom), leading zeros dropped.
fn crop64(hex: &str) -> Option<Op> {
    let hex = hex.trim();
    if hex.is_empty() || hex.len() > 16 {
        return None;
    }
    let v = u64::from_str_radix(hex, 16).ok()?;
    let f = |shift: u32| ((v >> shift) & 0xffff) as f32 / 65535.0;
    let (x0, y0, x1, y1) = (f(48), f(32), f(16), f(0));
    (x1 > x0 && y1 > y0).then_some(Op::Crop { x0, y0, x1, y1 })
}

/// Translates a Picasa `filters=` line and its separate `flipped=flipped(n)` and
/// `rotate=rotate(n)` lines.
pub fn translate(filters: &str, flipped: &str, rotate: &str) -> (Vec<Step>, Vec<String>) {
    let mut steps = Vec::new();
    let mut unknown = Vec::new();
    for item in filters.split(';').map(str::trim).filter(|x| !x.is_empty()) {
        let (key, rest) = item.split_once('=').unwrap_or((item, ""));
        let parts: Vec<&str> = rest.split(',').collect();
        let step = match (key, parts.as_slice()) {
            ("autolight", ["1"]) => Some(Step::Auto("autocontrast")),
            ("autocolor", ["1"]) => Some(Step::Auto("autocolor")),
            ("enhance", ["1"]) => Some(Step::Auto("lucky")),
            ("crop64", ["1", hex]) => crop64(hex).map(Step::Op),
            ("tilt", ["1", v, _]) => v.trim().parse::<f32>().ok().filter(|v| v.is_finite() && v.abs() <= 1.0).map(|v| Step::Op(Op::Straighten(v * TILT_DEGREES))),
            _ => match recipe::parse(item).pop() {
                Some(Op::Unknown(_)) | None => None,
                Some(op) => Some(Step::Op(op)),
            },
        };
        match step {
            Some(s) => steps.push(s),
            None => {
                unknown.push(item.to_string());
                steps.push(Step::Op(Op::Unknown(item.to_string())));
            }
        }
    }
    // Picasa's exports show the mirror coming after the crop (whose numbers are in the
    // unmirrored photo) and before everything else; lab, format document
    let bits = paren_number(flipped, "flipped").map(|n| (n & 3) as u8).unwrap_or(0);
    if bits != 0 {
        let at = steps.iter().take_while(|s| matches!(s, Step::Op(Op::Crop { .. }))).count();
        steps.insert(at, Step::Op(Op::Flip(bits)));
    }
    // the crop and straighten numbers are in the unrotated picture, so the turn comes last
    let turns = paren_number(rotate, "rotate").map(|n| n.rem_euclid(4)).unwrap_or(0);
    if turns != 0 {
        steps.push(Step::Op(Op::Rotate(turns)));
    }
    (steps, unknown)
}

/// `name(n)` -> n
fn paren_number(text: &str, name: &str) -> Option<i32> {
    text.trim().strip_prefix(name)?.strip_prefix('(')?.strip_suffix(')')?.trim().parse().ok()
}

/// What one folder's `.picasa.ini` holds for the photos still in it. None if it has none.
pub fn read_folder(dir: &Path) -> Option<FolderImport> {
    let text = std::fs::read_to_string(dir.join(FILE_NAME)).ok()?;
    let entries = sidecar::parse(&text);
    let mut fi = FolderImport { dir: dir.to_path_buf(), ..Default::default() };
    if let Some(e) = entries.get(sidecar::FOLDER_SECTION) {
        fi.description = e.get("description").to_string();
        fi.hidden = e.get("P2category") == "Hidden Folders";
    }
    fi.albums = albums::in_sidecar(&entries);
    let mut names: Vec<&String> = entries.keys().filter(|k| *k != sidecar::FOLDER_SECTION && !k.starts_with(".album:")).collect();
    names.sort();
    for name in names {
        let e = &entries[name];
        if !dir.join(name).is_file() {
            continue;
        }
        let (steps, unknown) = translate(&e.filters, e.get("flipped"), e.get("rotate"));
        let p = PhotoImport {
            name: name.clone(),
            star: e.star,
            hidden: e.get("hidden") == "yes",
            albums: albums::members_of(e),
            steps,
            unknown,
            caption: e.get("caption").to_string(),
            keywords: e.get("keywords").split(',').map(str::trim).filter(|k| !k.is_empty()).map(String::from).collect(),
        };
        if e.extra.contains_key("backuphash") && e.filters.is_empty() && dir.join(".picasaoriginals").join(name).is_file() {
            fi.saved_in_picasa += 1;
        }
        if p.star || p.hidden || !p.albums.is_empty() || !p.steps.is_empty() || !p.caption.is_empty() || !p.keywords.is_empty() {
            fi.photos.push(p);
        }
    }
    (!fi.photos.is_empty() || fi.hidden || !fi.description.is_empty() || !fi.albums.is_empty()).then_some(fi)
}

/// What an import did (or, before it runs, would do).
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Summary {
    pub folders: usize,
    pub stars: usize,
    pub hidden_photos: usize,
    pub hidden_folders: usize,
    pub descriptions: usize,
    pub albums: Vec<String>,
    pub album_photos: usize,
    pub edited: usize,
    /// photos whose CasaPic edits are kept (Picasa's are not applied over them)
    pub kept_casapic_edits: usize,
    pub captions: usize,
    pub saved_in_picasa: usize,
    /// Picasa edits CasaPic cannot draw yet: name -> number of photos
    pub unknown: BTreeMap<String, usize>,
}

pub fn summarize(folders: &[FolderImport], current: &dyn Fn(&Path) -> HashMap<String, Entry>) -> Summary {
    let mut s = Summary { folders: folders.len(), ..Default::default() };
    let mut album_names = std::collections::BTreeSet::new();
    for fi in folders {
        let mine = current(&fi.dir);
        s.hidden_folders += fi.hidden as usize;
        s.descriptions += !fi.description.is_empty() as usize;
        s.saved_in_picasa += fi.saved_in_picasa;
        for a in &fi.albums {
            album_names.insert(a.name.clone());
        }
        for p in &fi.photos {
            s.stars += p.star as usize;
            s.hidden_photos += p.hidden as usize;
            s.album_photos += !p.albums.is_empty() as usize;
            s.captions += (!p.caption.is_empty() || !p.keywords.is_empty()) as usize;
            if !p.steps.is_empty() {
                if mine.get(&p.name).is_some_and(|e| !e.filters.is_empty()) {
                    s.kept_casapic_edits += 1;
                } else {
                    s.edited += 1;
                }
            }
            for u in &p.unknown {
                *s.unknown.entry(u.split('=').next().unwrap_or(u).to_string()).or_default() += 1;
            }
        }
    }
    s.albums = album_names.into_iter().collect();
    s
}

/// Writes one folder's import into its `.casapic.ini`: stars and albums are added to what is
/// there, hidden flags and the description set when Picasa had them (a description CasaPic
/// already has is kept), and each photo's recipe (`recipes`, the steps worked out by the
/// caller) set only where CasaPic has no edits of its own.
pub fn apply_folder(fi: &FolderImport, recipes: &HashMap<String, Vec<Op>>) -> Result<()> {
    sidecar::update_all(&fi.dir, |m| {
        if fi.hidden || !fi.description.is_empty() {
            let f = m.entry(sidecar::FOLDER_SECTION.to_string()).or_default();
            if fi.hidden {
                f.set("P2category", "Hidden Folders");
            }
            if f.get("description").is_empty() {
                f.set("description", &fi.description);
            }
        }
        for a in &fi.albums {
            let e = m.entry(albums::section(&a.token)).or_default();
            if e.get("name").is_empty() {
                for (k, v) in [("name", &a.name), ("token", &a.token), ("date", &a.date), ("location", &a.location), ("description", &a.description)] {
                    e.set(k, v);
                }
            }
        }
        for p in &fi.photos {
            let e = m.entry(p.name.clone()).or_default();
            e.star |= p.star;
            if p.hidden {
                e.set("hidden", "yes");
            }
            let mut tokens = albums::members_of(e);
            for t in &p.albums {
                if !tokens.contains(t) {
                    tokens.push(t.clone());
                }
            }
            e.set("albums", &tokens.join(","));
            if e.filters.is_empty() {
                if let Some(ops) = recipes.get(&p.name) {
                    e.filters = recipe::to_string(ops);
                }
            }
        }
        albums::tidy(m);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picasa_lines_translate_as_measured_in_the_lab() {
        // the crop drawn in the lab, and the crop on the rotated photo (leading zero dropped)
        let (s, u) = translate("crop64=1,34dd34d2ca6eca3d;", "", "");
        assert!(u.is_empty());
        let Step::Op(Op::Crop { x0, y0, x1, y1 }) = s[0] else { panic!("{s:?}") };
        assert!((x0 - 0.2065).abs() < 1e-3 && (y0 - 0.2063).abs() < 1e-3 && (x1 - 0.7907).abs() < 1e-3 && (y1 - 0.7900).abs() < 1e-3);
        let (s, _) = translate("crop64=1,873806d66e9f596;", "", "rotate(1)");
        let Step::Op(Op::Crop { x0, y1, .. }) = s[0] else { panic!("{s:?}") };
        assert!((x0 - 0.0330).abs() < 1e-3 && (y1 - 0.9593).abs() < 1e-3);
        assert_eq!(s[1], Step::Op(Op::Rotate(1)), "the turn comes after the crop");

        let (s, u) = translate("fill=1,0.504673;enhance=1;autolight=1;autocolor=1;tilt=1,1.000000,0.000000;sepia=1;notyet=1,0.5;", "", "");
        assert_eq!(s[0], Step::Op(Op::Fill(0.504673)));
        assert_eq!(&s[1..4], &[Step::Auto("lucky"), Step::Auto("autocontrast"), Step::Auto("autocolor")]);
        assert_eq!(s[4], Step::Op(Op::Straighten(TILT_DEGREES)));
        assert_eq!(s[5], Step::Op(Op::Sepia));
        assert_eq!(u, ["notyet=1,0.5"], "kept and listed, not dropped");
        assert_eq!(s[6], Step::Op(Op::Unknown("notyet=1,0.5".into())));

        // the two flips exported from the lab: after the crop, before the straighten, and the
        // turn still last
        let (s, _) = translate("crop64=1,873806d66e9f596;tilt=1,0.501917,0.000000;", "flipped(1)", "rotate(1)");
        assert!(matches!(s[0], Step::Op(Op::Crop { .. })));
        assert_eq!(s[1], Step::Op(Op::Flip(1)));
        assert!(matches!(s[2], Step::Op(Op::Straighten(_))));
        assert_eq!(s[3], Step::Op(Op::Rotate(1)));
        let (s, _) = translate("tilt=1,1.000000,0.000000;", "flipped(2)", "");
        assert_eq!(s[0], Step::Op(Op::Flip(2)));
    }

    #[test]
    fn a_folder_imports_into_casapic_without_touching_picasa_files() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path();
        for n in ["a.jpg", "b.jpg", "c.jpg", "mine.jpg"] {
            std::fs::write(dir.join(n), b"x").unwrap();
        }
        let ini = "[a.jpg]\r\nstar=yes\r\nfilters=fill=1,0.4;\r\n[.album:42679c2762159d31a2a605ddcc716e7d]\r\nname=Holiday\r\ntoken=42679c2762159d31a2a605ddcc716e7d\r\ndate=2026-10-24T23:08:56+00:00\r\n[b.jpg]\r\nalbums=42679c2762159d31a2a605ddcc716e7d\r\nredo=sepia=1;\r\n[c.jpg]\r\nhidden=yes\r\n[mine.jpg]\r\nfilters=bw=1;\r\n[gone.jpg]\r\nstar=yes\r\n[Picasa]\r\nP2category=Hidden Folders\r\ndescription=Summer\r\n";
        std::fs::write(dir.join(FILE_NAME), ini).unwrap();
        sidecar::update(dir, "mine.jpg", |e| e.filters = "warm=1;".into()).unwrap();

        let fi = read_folder(dir).unwrap();
        assert_eq!(fi.photos.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["a.jpg", "b.jpg", "c.jpg", "mine.jpg"], "a photo no longer there is skipped");
        let s = summarize(std::slice::from_ref(&fi), &|d| sidecar::read(d));
        assert_eq!((s.stars, s.hidden_photos, s.hidden_folders, s.album_photos, s.edited, s.kept_casapic_edits), (1, 1, 1, 1, 1, 1));
        assert_eq!(s.albums, ["Holiday"]);

        let recipes: HashMap<String, Vec<Op>> = fi
            .photos
            .iter()
            .map(|p| (p.name.clone(), p.steps.iter().filter_map(|s| if let Step::Op(o) = s { Some(o.clone()) } else { None }).collect()))
            .collect();
        apply_folder(&fi, &recipes).unwrap();
        let m = sidecar::read(dir);
        assert!(m["a.jpg"].star);
        assert_eq!(m["a.jpg"].filters, "fill=1,0.4;");
        assert_eq!(m["b.jpg"].filters, "", "undone fixes (redo=) are not applied");
        assert_eq!(albums::members_of(&m["b.jpg"]), ["42679c2762159d31a2a605ddcc716e7d"]);
        assert_eq!(albums::in_sidecar(&m)[0].name, "Holiday");
        assert_eq!(m["c.jpg"].get("hidden"), "yes");
        assert_eq!(m["mine.jpg"].filters, "warm=1;", "CasaPic's own edits are kept");
        assert_eq!(m[sidecar::FOLDER_SECTION].get("P2category"), "Hidden Folders");
        assert_eq!(m[sidecar::FOLDER_SECTION].get("description"), "Summer");
        assert_eq!(std::fs::read_to_string(dir.join(FILE_NAME)).unwrap(), ini, "Picasa's file untouched");
    }
}
