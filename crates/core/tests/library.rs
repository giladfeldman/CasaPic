//! Integration tests on real JPEGs from the test library (`tmp/testlib`, made by
//! `lab/scripts/make_photos.sh`). They fail, rather than skip, if the library is missing.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use casapic_core::{db::Db, export, recipe::Op, render, scan, sidecar, thumbs, watch, wic};

fn testlib() -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp/testlib");
    assert!(p.is_dir(), "test library missing at {} (see lab/scripts/make_photos.sh)", p.display());
    p
}

/// Copies the first `n` photos of two month folders into a fresh temp library.
fn small_library(n: usize) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for month in ["2015-01 Trip", "2015-02 Trip"] {
        let src = testlib().join(month);
        let dst = d.path().join(month);
        std::fs::create_dir_all(&dst).unwrap();
        let mut files: Vec<_> = std::fs::read_dir(&src).unwrap().flatten().map(|e| e.path()).collect();
        files.sort();
        for f in files.into_iter().take(n) {
            std::fs::copy(&f, dst.join(f.file_name().unwrap())).unwrap();
        }
    }
    d
}

fn fingerprints(root: &Path) -> BTreeMap<PathBuf, u64> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file() && scan::is_image(e.path()))
        .map(|e| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            std::fs::read(e.path()).unwrap().hash(&mut h);
            (e.path().to_path_buf(), h.finish())
        })
        .collect()
}

#[test]
fn scan_counts_match_disk_and_rescan_is_incremental() {
    let lib = small_library(10);
    let db = Db::open_in_memory().unwrap();
    let (s, changed) = scan::scan_root(&db, lib.path()).unwrap();
    assert_eq!((s.folders, s.photos, s.changed), (2, 20, 20));
    assert_eq!(changed.len(), 20);
    let folders = db.folders().unwrap();
    assert_eq!(folders.len(), 2);
    let photos = db.photos(folders[0].id).unwrap();
    assert_eq!(photos.len(), 10);
    let p = &photos[0];
    assert_eq!((p.width, p.height), (4000, 3000));
    assert!(p.taken.is_some(), "date taken read from EXIF");
    assert_eq!(p.camera.as_deref(), Some("CasaPicLab"));
    assert_eq!(p.thumb_level, thumbs::LEVEL_PREVIEW, "embedded preview stored during scan");
    // nothing changed -> nothing re-read
    let (s2, changed2) = scan::scan_root(&db, lib.path()).unwrap();
    assert_eq!((s2.photos, s2.changed), (20, 0));
    assert!(changed2.is_empty());
}

#[test]
fn rename_delete_and_folder_removal_are_picked_up() {
    let lib = small_library(5);
    let db = Db::open_in_memory().unwrap();
    scan::scan_root(&db, lib.path()).unwrap();
    let jan = lib.path().join("2015-01 Trip");
    let first = std::fs::read_dir(&jan).unwrap().flatten().next().unwrap().path();
    std::fs::rename(&first, jan.join("renamed.jpg")).unwrap();
    let second = std::fs::read_dir(&jan).unwrap().flatten().find(|e| e.file_name() != "renamed.jpg").unwrap().path();
    std::fs::remove_file(second).unwrap();
    std::fs::remove_dir_all(lib.path().join("2015-02 Trip")).unwrap();
    let (s, _) = scan::scan_root(&db, lib.path()).unwrap();
    assert_eq!((s.folders, s.photos, s.folders_removed), (1, 4, 1));
    let f = &db.folders().unwrap()[0];
    let names: Vec<String> = db.photos(f.id).unwrap().into_iter().map(|p| p.name).collect();
    assert!(names.contains(&"renamed.jpg".to_string()));
    assert_eq!(names.len(), 4);
}

#[test]
fn sidecar_stars_and_recipes_survive_a_fresh_index() {
    let lib = small_library(3);
    let jan = lib.path().join("2015-01 Trip");
    let name = "IMG_0012.jpg";
    assert!(jan.join(name).exists());
    sidecar::update(&jan, name, |e| {
        e.star = true;
        e.filters = "fill=1,0.3;".into();
    })
    .unwrap();
    let db = Db::open_in_memory().unwrap();
    scan::scan_root(&db, lib.path()).unwrap();
    let fid = db.folder_id(&jan.to_string_lossy()).unwrap().unwrap();
    let p = db.photos(fid).unwrap().into_iter().find(|p| p.name == name).unwrap();
    assert!(p.star);
    assert_eq!(p.recipe, "fill=1,0.3;");
}

#[test]
fn watcher_reports_a_new_file_within_two_seconds() {
    let lib = small_library(1);
    let (tx, rx) = std::sync::mpsc::channel();
    let _w = watch::start(&[lib.path().to_path_buf()], move |dirs| {
        let _ = tx.send(dirs);
    })
    .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let jan = lib.path().join("2015-01 Trip");
    let src = std::fs::read_dir(&jan).unwrap().flatten().next().unwrap().path();
    let t = Instant::now();
    std::fs::copy(&src, jan.join("new.jpg")).unwrap();
    let dirs = rx.recv_timeout(Duration::from_secs(2)).expect("no change reported within 2 s");
    assert!(dirs.iter().any(|d| d.ends_with("2015-01 Trip")), "{dirs:?}");
    eprintln!("watcher reported after {} ms", t.elapsed().as_millis());
}

#[test]
fn thumbs_edits_and_export_never_touch_originals() {
    let lib = small_library(8);
    let before = fingerprints(lib.path());
    assert_eq!(before.len(), 16, "fingerprint check must cover every original");
    let db = Db::open_in_memory().unwrap();
    let (_, ids) = scan::scan_root(&db, lib.path()).unwrap();
    let (done, failed) = thumbs::generate(&db, &ids, |_| {}).unwrap();
    assert_eq!((done, failed), (16, 0));
    let (level, jpeg) = db.thumb(ids[0]).unwrap().unwrap();
    assert_eq!(level, thumbs::LEVEL_FULL);
    let t = image::load_from_memory(&jpeg).unwrap();
    assert_eq!(t.width().max(t.height()), thumbs::EDGE);

    // edit one photo through the full pipeline and export it
    let id = ids[0];
    let path = db.photo_path(id).unwrap().unwrap();
    let preview = render::Img::from_rgb8(&wic::decode(&path, Some(1600)).unwrap());
    let ops = vec![
        render::lucky_op(&preview),
        Op::Fill(0.4),
        Op::Straighten(3.0),
        Op::Crop { x0: 0.1, y0: 0.1, x1: 0.9, y1: 0.9 },
        Op::Rotate(1),
    ];
    let out = tempfile::tempdir().unwrap();
    let opt = export::ExportOptions { max_edge: Some(800), quality: 90 };
    let dest = export::export(&path, 1, &ops, out.path(), &opt).unwrap();
    let e = image::open(&dest).unwrap();
    assert_eq!(e.width().max(e.height()), 800);
    assert!(e.height() > e.width(), "rotated to portrait");
    // camera data kept in the export
    let m = casapic_core::meta::read(&dest).unwrap();
    assert!(m.taken.is_some());
    assert_eq!(m.orientation, 1);
    assert!(m.preview.is_none(), "stale embedded preview unlinked");
    // full-size export too
    let full = export::export(&path, 1, &ops, out.path(), &export::ExportOptions { max_edge: None, quality: 85 }).unwrap();
    let (fw, fh) = render::output_size(4000.0, 3000.0, &ops);
    let f = image::image_dimensions(&full).unwrap();
    assert!((f.0 as f32 - fw).abs() <= 2.0 && (f.1 as f32 - fh).abs() <= 2.0, "{f:?} vs {fw}x{fh}");

    assert_eq!(fingerprints(lib.path()), before, "an original photo changed");
}
