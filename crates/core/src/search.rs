//! Search as you type (Ctrl+F), as in Picasa: every word typed must be found in the photo's
//! file name, its folder's name, its tags, its caption or the name of an album it is in
//! (ignoring capitals, accents included). Filters: starred only, and a date range.

use anyhow::Result;
use serde::Deserialize;

use crate::db::Db;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Query {
    pub text: String,
    pub starred: bool,
    /// seconds (the camera's wall-clock time, as stored); inclusive
    pub from: Option<i64>,
    /// seconds; exclusive
    pub to: Option<i64>,
}

impl Query {
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty() && !self.starred && self.from.is_none() && self.to.is_none()
    }
}

/// The ids of the photos that match, in no particular order.
pub fn run(db: &Db, q: &Query) -> Result<Vec<i64>> {
    let words: Vec<String> = q.text.split_whitespace().map(|w| w.to_lowercase()).collect();
    let albums: std::collections::HashMap<String, String> =
        db.albums()?.into_iter().map(|a| (a.album.token, a.album.name.to_lowercase())).collect();
    let mut out = Vec::new();
    db.for_each_search_row(|r| {
        if q.starred && !r.star {
            return;
        }
        if q.from.is_some_and(|f| r.date < f) || q.to.is_some_and(|t| r.date >= t) {
            return;
        }
        if !words.is_empty() {
            let mut hay = format!("{}\n{}\n{}\n{}", r.name, r.folder, r.tags, r.caption).to_lowercase();
            for t in r.albums.split(',').filter(|t| !t.is_empty()) {
                if let Some(n) = albums.get(t) {
                    hay.push('\n');
                    hay.push_str(n);
                }
            }
            if !words.iter().all(|w| hay.contains(w.as_str())) {
                return;
            }
        }
        out.push(r.id);
    })?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_must_all_match_any_field_and_filters_apply() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("Zürich Trip");
        std::fs::create_dir_all(&dir).unwrap();
        let img = image::RgbImage::from_pixel(8, 8, image::Rgb([90, 120, 200]));
        let jpeg = crate::thumbs::encode_jpeg(&img, 80).unwrap();
        for n in ["lake.jpg", "dinner.jpg", "other.jpg"] {
            std::fs::write(dir.join(n), &jpeg).unwrap();
        }
        crate::tags::change(&dir, "lake.jpg", |l| {
            l.tags = vec!["Strand".into()];
            l.caption = "Kühe am See".into();
        })
        .unwrap();
        crate::sidecar::update(&dir, "dinner.jpg", |e| e.star = true).unwrap();
        let fam = crate::albums::Album { token: crate::albums::new_token(), name: "Familie Müller".into(), ..Default::default() };
        crate::albums::add(&fam, &[(dir.clone(), "dinner.jpg".into())]).unwrap();
        let db = Db::open_in_memory().unwrap();
        crate::scan::scan_root(&db, d.path()).unwrap();
        let name = |ids: Vec<i64>| {
            let mut v: Vec<String> = ids.iter().map(|&i| db.photo(i).unwrap().unwrap().name).collect();
            v.sort();
            v
        };
        let q = |text: &str| Query { text: text.into(), ..Default::default() };
        assert_eq!(name(run(&db, &q("KÜHE")).unwrap()), ["lake.jpg"], "caption, capitals ignored, umlaut");
        assert_eq!(name(run(&db, &q("strand see")).unwrap()), ["lake.jpg"], "all words, any field");
        assert!(run(&db, &q("strand dinner")).unwrap().is_empty(), "every word must match");
        assert_eq!(name(run(&db, &q("müller")).unwrap()), ["dinner.jpg"], "album name");
        assert_eq!(run(&db, &q("zürich")).unwrap().len(), 3, "folder name");
        assert_eq!(name(run(&db, &Query { starred: true, ..Default::default() }).unwrap()), ["dinner.jpg"]);
        assert!(run(&db, &Query { from: Some(i64::MAX - 1), ..Default::default() }).unwrap().is_empty(), "date range");
        assert!(run(&db, &q("nothing-like-this")).unwrap().is_empty());
    }
}
