//! Albums, stored as Picasa 3.9 stores them (lab check, `docs/picasa-organize-format.md`):
//! each photo's sidecar entry lists its albums as `albums=<token>,<token>`, and every folder
//! holding a member carries a copy of the album's own section, `[.album:<token>]` with
//! `name`, `token`, `date`, `location`, `description`. CasaPic's full album list (including
//! empty albums) is kept in its settings, as Picasa keeps its own in its database.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::sidecar::{self, Entry};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Album {
    pub token: String,
    pub name: String,
    /// as Picasa writes it, e.g. `2026-10-24T23:08:56+00:00`; may be empty
    pub date: String,
    pub location: String,
    pub description: String,
}

const PREFIX: &str = ".album:";

pub fn section(token: &str) -> String {
    format!("{PREFIX}{token}")
}

/// A new 32-digit hex token, like Picasa's.
pub fn new_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let half = |salt: u64| {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(t);
        h.write_u64(salt);
        h.finish()
    };
    format!("{:016x}{:016x}", half(1), half(2))
}

/// The album sections found in one folder's sidecar.
pub fn in_sidecar(entries: &HashMap<String, Entry>) -> Vec<Album> {
    entries
        .iter()
        .filter_map(|(k, e)| {
            let token = k.strip_prefix(PREFIX)?;
            Some(Album {
                token: token.to_string(),
                name: e.get("name").to_string(),
                date: e.get("date").to_string(),
                location: e.get("location").to_string(),
                description: e.get("description").to_string(),
            })
        })
        .collect()
}

fn fill(a: &Album, e: &mut Entry) {
    e.set("name", &a.name);
    e.set("token", &a.token);
    e.set("date", &a.date);
    e.set("location", &a.location);
    e.set("description", &a.description);
}

/// The albums a photo's sidecar entry says it is in.
pub fn members_of(e: &Entry) -> Vec<String> {
    e.get("albums").split(',').map(str::trim).filter(|t| !t.is_empty()).map(String::from).collect()
}

fn set_members(e: &mut Entry, tokens: &[String]) {
    e.set("albums", &tokens.join(","));
}

/// Drops album sections that no photo in this folder belongs to any more.
pub fn tidy(entries: &mut HashMap<String, Entry>) {
    let used: std::collections::HashSet<String> = entries.values().flat_map(members_of).collect();
    entries.retain(|k, _| k.strip_prefix(PREFIX).is_none_or(|t| used.contains(t)));
}

fn by_folder(photos: &[(PathBuf, String)]) -> HashMap<&Path, Vec<&str>> {
    let mut m: HashMap<&Path, Vec<&str>> = HashMap::new();
    for (d, n) in photos {
        m.entry(d.as_path()).or_default().push(n);
    }
    m
}

/// Puts photos (folder, file name) into an album.
pub fn add(a: &Album, photos: &[(PathBuf, String)]) -> Result<()> {
    for (dir, names) in by_folder(photos) {
        sidecar::update_all(dir, |m| {
            fill(a, m.entry(section(&a.token)).or_default());
            for n in names {
                let e = m.entry(n.to_string()).or_default();
                let mut t = members_of(e);
                if !t.contains(&a.token) {
                    t.push(a.token.clone());
                }
                set_members(e, &t);
            }
        })?;
    }
    Ok(())
}

/// Takes photos out of an album (the photos themselves are not touched).
pub fn remove(token: &str, photos: &[(PathBuf, String)]) -> Result<()> {
    for (dir, names) in by_folder(photos) {
        sidecar::update_all(dir, |m| {
            for n in names {
                if let Some(e) = m.get_mut(n) {
                    let t: Vec<String> = members_of(e).into_iter().filter(|x| x != token).collect();
                    set_members(e, &t);
                }
            }
            tidy(m);
        })?;
    }
    Ok(())
}

/// Rewrites the album's section in each folder that holds a member (after Album Properties).
pub fn rewrite(a: &Album, folders: &[PathBuf]) -> Result<()> {
    for dir in folders {
        sidecar::update_all(dir, |m| {
            if let Some(e) = m.get_mut(&section(&a.token)) {
                fill(a, e);
            }
        })?;
    }
    Ok(())
}

/// Removes the album from every folder that holds a member. Photos stay where they are.
pub fn delete(token: &str, folders: &[PathBuf]) -> Result<()> {
    for dir in folders {
        sidecar::update_all(dir, |m| {
            for e in m.values_mut() {
                let t: Vec<String> = members_of(e).into_iter().filter(|x| x != token).collect();
                if t.len() != members_of(e).len() {
                    set_members(e, &t);
                }
            }
            tidy(m);
        })?;
    }
    Ok(())
}

/// The sidecar sections that must travel with a photo moved or copied to another folder.
pub fn sections_for(entries: &HashMap<String, Entry>, photo: &Entry) -> Vec<(String, Entry)> {
    members_of(photo).iter().filter_map(|t| entries.get(&section(t)).map(|e| (section(t), e.clone()))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_remove_edit_delete_as_picasa_writes_them() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let hol = Album { token: new_token(), name: "Holiday".into(), date: "2026-10-24T23:08:56+00:00".into(), ..Default::default() };
        let trip = Album { token: new_token(), name: "Trip".into(), ..Default::default() };
        assert_eq!(hol.token.len(), 32);
        assert_ne!(hol.token, trip.token);

        add(&hol, &[(a.clone(), "x.jpg".into()), (b.clone(), "y.jpg".into())]).unwrap();
        add(&trip, &[(a.clone(), "x.jpg".into())]).unwrap();
        let text = std::fs::read_to_string(sidecar::path_for(&a)).unwrap();
        assert!(text.contains(&format!("[.album:{}]", hol.token)));
        assert!(text.contains(&format!("albums={},{}", hol.token, trip.token)), "comma, no space, in order added");
        assert!(text.contains("date=2026-10-24T23:08:56+00:00") && text.contains("name=Holiday"));
        assert_eq!(in_sidecar(&sidecar::read(&b)), [hol.clone()], "each member's folder has its own copy");

        let mut renamed = hol.clone();
        renamed.name = "Summer".into();
        rewrite(&renamed, &[a.clone(), b.clone()]).unwrap();
        assert!(in_sidecar(&sidecar::read(&b)).iter().any(|x| x.name == "Summer"));

        remove(&hol.token, &[(b.clone(), "y.jpg".into())]).unwrap();
        assert!(!sidecar::path_for(&b).exists(), "nothing left in b: the sidecar goes");

        delete(&trip.token, &[a.clone()]).unwrap();
        let m = sidecar::read(&a);
        assert_eq!(members_of(&m["x.jpg"]), [hol.token.clone()]);
        assert_eq!(in_sidecar(&m).len(), 1, "Trip's section gone");
    }

    #[test]
    fn album_travels_with_a_moved_or_copied_photo() {
        let d = tempfile::tempdir().unwrap();
        let (a, b, c) = (d.path().join("a"), d.path().join("b"), d.path().join("c"));
        for x in [&a, &b, &c] {
            std::fs::create_dir_all(x).unwrap();
        }
        std::fs::write(a.join("x.jpg"), b"x").unwrap();
        let hol = Album { token: new_token(), name: "Holiday".into(), ..Default::default() };
        add(&hol, &[(a.clone(), "x.jpg".into())]).unwrap();

        crate::files::transfer_photo(&a, "x.jpg", &b, true).unwrap();
        assert_eq!(in_sidecar(&sidecar::read(&a)), [hol.clone()], "copy: source keeps it");
        assert_eq!(in_sidecar(&sidecar::read(&b)), [hol.clone()], "copy: destination gets a copy");
        assert_eq!(members_of(&sidecar::read(&b)["x.jpg"]), [hol.token.clone()]);

        crate::files::transfer_photo(&a, "x.jpg", &c, false).unwrap();
        assert!(!sidecar::path_for(&a).exists(), "move: nothing left behind in the source");
        assert_eq!(in_sidecar(&sidecar::read(&c)), [hol.clone()]);
    }
}
