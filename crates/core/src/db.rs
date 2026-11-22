//! The library index: one SQLite file holding folders, photos and thumbnails.
//!
//! The photo files and the `.casapic.ini` sidecars on disk are the source of truth; this
//! database is a fast cache of them and can be deleted at any time (it is rebuilt by a scan).

use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

/// Bumped whenever the tables change. The database is only a cache of the folders and their
/// sidecars, so an older one is simply dropped and rebuilt by the next scan.
/// 4: file dates (`mtime`) kept on the local clock, like camera dates (see
/// `dates::local_clock`); an index of version 3 is converted in place.
const SCHEMA_VERSION: i64 = 4;

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS folders (
    id        INTEGER PRIMARY KEY,
    path      TEXT NOT NULL UNIQUE,
    name      TEXT NOT NULL,
    root      TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    sort      TEXT NOT NULL DEFAULT '',
    hidden    INTEGER NOT NULL DEFAULT 0,
    date_min  INTEGER,
    date_max  INTEGER,
    count     INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS photos (
    id          INTEGER PRIMARY KEY,
    folder_id   INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    size        INTEGER NOT NULL,
    mtime       INTEGER NOT NULL,
    width       INTEGER NOT NULL DEFAULT 0,
    height      INTEGER NOT NULL DEFAULT 0,
    taken       INTEGER,
    orientation INTEGER NOT NULL DEFAULT 1,
    camera      TEXT,
    star        INTEGER NOT NULL DEFAULT 0,
    recipe      TEXT NOT NULL DEFAULT '',
    thumb_level INTEGER NOT NULL DEFAULT 0,
    tags        TEXT NOT NULL DEFAULT '',
    caption     TEXT NOT NULL DEFAULT '',
    albums      TEXT NOT NULL DEFAULT '',
    hidden      INTEGER NOT NULL DEFAULT 0,
    UNIQUE(folder_id, name)
);
CREATE TABLE IF NOT EXISTS albums (
    token       TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    date        TEXT NOT NULL DEFAULT '',
    location    TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS photos_folder ON photos(folder_id, taken, name);
CREATE TABLE IF NOT EXISTS thumbs (
    photo_id INTEGER PRIMARY KEY REFERENCES photos(id) ON DELETE CASCADE,
    level    INTEGER NOT NULL,
    jpeg     BLOB NOT NULL
);
";

#[derive(Debug, Clone, Serialize)]
pub struct Folder {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub root: String,
    /// "Add a description" text, from the folder's sidecar
    pub description: String,
    /// photo order chosen with Folder > Sort By: "", "name", "size" ("date" = ""), "-" = reversed
    pub sort: String,
    /// Folder > Hide (Picasa's `P2category=Hidden Folders`)
    pub hidden: bool,
    pub date_min: Option<i64>,
    pub date_max: Option<i64>,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Photo {
    pub id: i64,
    pub folder_id: i64,
    pub name: String,
    pub size: i64,
    pub mtime: i64,
    pub width: i64,
    pub height: i64,
    pub taken: Option<i64>,
    pub orientation: i64,
    pub camera: Option<String>,
    pub star: bool,
    pub recipe: String,
    pub thumb_level: i64,
    /// tags (keywords) and caption, as written inside the photo file
    pub tags: Vec<String>,
    pub caption: String,
    /// album tokens, from the folder's sidecar
    pub albums: Vec<String>,
    /// Picture > Hide
    pub hidden: bool,
}

/// What the scanner knows about one file, before it has an id.
#[derive(Debug, Clone, Default)]
pub struct PhotoInfo {
    pub name: String,
    pub size: i64,
    pub mtime: i64,
    pub width: i64,
    pub height: i64,
    pub taken: Option<i64>,
    pub orientation: i64,
    pub camera: Option<String>,
    pub tags: Vec<String>,
    pub caption: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AlbumRow {
    #[serde(flatten)]
    pub album: crate::albums::Album,
    pub count: i64,
    /// the first photo, shown as the album's picture
    pub cover: Option<i64>,
}

/// Tags are kept in one column, one per line (a tag never holds a line break).
pub fn join_tags(t: &[String]) -> String {
    t.iter().map(|s| s.replace(['\r', '\n'], " ").trim().to_string()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n")
}

fn split_list(s: String, sep: char) -> Vec<String> {
    s.split(sep).map(str::trim).filter(|x| !x.is_empty()).map(String::from).collect()
}

pub struct SearchRow {
    pub id: i64,
    pub name: String,
    pub folder: String,
    pub tags: String,
    pub caption: String,
    pub albums: String,
    pub star: bool,
    /// date taken, or the file's date when the camera gave none
    pub date: i64,
}

pub struct Db {
    conn: Mutex<Connection>,
}

fn photo_from_row(r: &rusqlite::Row) -> rusqlite::Result<Photo> {
    Ok(Photo {
        id: r.get(0)?,
        folder_id: r.get(1)?,
        name: r.get(2)?,
        size: r.get(3)?,
        mtime: r.get(4)?,
        width: r.get(5)?,
        height: r.get(6)?,
        taken: r.get(7)?,
        orientation: r.get(8)?,
        camera: r.get(9)?,
        star: r.get::<_, i64>(10)? != 0,
        recipe: r.get(11)?,
        thumb_level: r.get(12)?,
        tags: split_list(r.get(13)?, '\n'),
        caption: r.get(14)?,
        albums: split_list(r.get(15)?, ','),
        hidden: r.get::<_, i64>(16)? != 0,
    })
}

const PHOTO_COLS: &str =
    "id, folder_id, name, size, mtime, width, height, taken, orientation, camera, star, recipe, thumb_level, tags, caption, albums, hidden";

/// Version 3 to 4: every stored file date moves to the local clock, and the folder dates made
/// from them follow, in one transaction (all or nothing). Nothing else changes, so no photo
/// has to be read again.
fn to_local_file_dates(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    {
        let rows = {
            let mut st = tx.prepare("SELECT id, mtime FROM photos")?;
            st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut up = tx.prepare("UPDATE photos SET mtime = ?2 WHERE id = ?1")?;
        for (id, m) in rows {
            up.execute(params![id, crate::dates::local_clock(m)])?;
        }
        tx.execute_batch(
            "UPDATE folders SET
               date_min = (SELECT MIN(COALESCE(taken, mtime)) FROM photos WHERE folder_id = folders.id),
               date_max = (SELECT MAX(COALESCE(taken, mtime)) FROM photos WHERE folder_id = folders.id);
             PRAGMA user_version = 4;",
        )?;
    }
    tx.commit()?;
    Ok(())
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
        let mut version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version == 3 {
            to_local_file_dates(&conn)?;
            version = 4;
        }
        if version != SCHEMA_VERSION {
            conn.execute_batch("DROP TABLE IF EXISTS thumbs; DROP TABLE IF EXISTS photos; DROP TABLE IF EXISTS folders; DROP TABLE IF EXISTS albums;")?;
        }
        conn.execute_batch(SCHEMA)?;
        conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))?;
        Ok(Db { conn: Mutex::new(conn) })
    }

    pub fn open_in_memory() -> Result<Db> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Db { conn: Mutex::new(conn) })
    }

    fn c(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn folder_id(&self, path: &str) -> Result<Option<i64>> {
        Ok(self.c().query_row("SELECT id FROM folders WHERE path = ?1", [path], |r| r.get(0)).optional()?)
    }

    pub fn folders(&self) -> Result<Vec<Folder>> {
        let c = self.c();
        let mut st = c.prepare(
            "SELECT id, path, name, root, date_min, date_max, count, description, sort, hidden FROM folders
             WHERE count > 0 ORDER BY COALESCE(date_max, 0) DESC, name",
        )?;
        let rows = st.query_map([], |r| {
            Ok(Folder {
                id: r.get(0)?,
                path: r.get(1)?,
                name: r.get(2)?,
                root: r.get(3)?,
                date_min: r.get(4)?,
                date_max: r.get(5)?,
                count: r.get(6)?,
                description: r.get(7)?,
                sort: r.get(8)?,
                hidden: r.get::<_, i64>(9)? != 0,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Existing (name -> (id, size, mtime)) for a folder, used to skip unchanged files.
    pub fn folder_index(&self, folder_id: i64) -> Result<std::collections::HashMap<String, (i64, i64, i64)>> {
        let c = self.c();
        let mut st = c.prepare("SELECT name, id, size, mtime FROM photos WHERE folder_id = ?1")?;
        let rows = st.query_map([folder_id], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?, r.get(3)?))))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn photos(&self, folder_id: i64) -> Result<Vec<Photo>> {
        let c = self.c();
        let mut st = c.prepare(&format!(
            "SELECT {PHOTO_COLS} FROM photos WHERE folder_id = ?1 ORDER BY COALESCE(taken, mtime), name"
        ))?;
        let rows = st.query_map([folder_id], photo_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn photo(&self, id: i64) -> Result<Option<Photo>> {
        Ok(self
            .c()
            .query_row(&format!("SELECT {PHOTO_COLS} FROM photos WHERE id = ?1"), [id], photo_from_row)
            .optional()?)
    }

    /// Full path of a photo on disk.
    pub fn photo_path(&self, id: i64) -> Result<Option<std::path::PathBuf>> {
        Ok(self
            .c()
            .query_row(
                "SELECT f.path, p.name FROM photos p JOIN folders f ON f.id = p.folder_id WHERE p.id = ?1",
                [id],
                |r| Ok(Path::new(&r.get::<_, String>(0)?).join(r.get::<_, String>(1)?)),
            )
            .optional()?)
    }

    /// Writes one scanned folder in a single transaction: inserts/updates `changed`, keeps
    /// names in `present`, deletes everything else, applies sidecar stars/recipes, and
    /// stores embedded previews. Returns the ids of new or changed photos.
    pub fn apply_folder_scan(
        &self,
        path: &str,
        name: &str,
        root: &str,
        present: &[String],
        changed: &[(PhotoInfo, Option<Vec<u8>>)],
        sidecar: &std::collections::HashMap<String, crate::sidecar::Entry>,
    ) -> Result<(i64, Vec<i64>)> {
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO folders(path, name, root) VALUES(?1, ?2, ?3)
             ON CONFLICT(path) DO UPDATE SET name = excluded.name, root = excluded.root",
            params![path, name, root],
        )?;
        let fid: i64 = tx.query_row("SELECT id FROM folders WHERE path = ?1", [path], |r| r.get(0))?;
        let mut ids = Vec::with_capacity(changed.len());
        for (p, preview) in changed {
            let id: i64 = tx.query_row(
                "INSERT INTO photos(folder_id, name, size, mtime, width, height, taken, orientation, camera, thumb_level, tags, caption)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?11)
                 ON CONFLICT(folder_id, name) DO UPDATE SET size = excluded.size, mtime = excluded.mtime,
                   width = excluded.width, height = excluded.height, taken = excluded.taken,
                   orientation = excluded.orientation, camera = excluded.camera, thumb_level = 0,
                   tags = excluded.tags, caption = excluded.caption
                 RETURNING id",
                params![fid, p.name, p.size, p.mtime, p.width, p.height, p.taken, p.orientation, p.camera, join_tags(&p.tags), p.caption],
                |r| r.get(0),
            )?;
            tx.execute("DELETE FROM thumbs WHERE photo_id = ?1", [id])?;
            if let Some(jpeg) = preview {
                tx.execute("INSERT INTO thumbs(photo_id, level, jpeg) VALUES(?1, 1, ?2)", params![id, jpeg])?;
                tx.execute("UPDATE photos SET thumb_level = 1 WHERE id = ?1", [id])?;
            }
            ids.push(id);
        }
        // remove photos that are gone from disk
        {
            let keep: std::collections::HashSet<&str> = present.iter().map(|s| s.as_str()).collect();
            let mut st = tx.prepare("SELECT id, name FROM photos WHERE folder_id = ?1")?;
            let gone: Vec<i64> = st
                .query_map([fid], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
                .filter_map(|r| r.ok())
                .filter(|(_, n)| !keep.contains(n.as_str()))
                .map(|(id, _)| id)
                .collect();
            drop(st);
            for id in gone {
                tx.execute("DELETE FROM photos WHERE id = ?1", [id])?;
            }
        }
        // sidecar is the source of truth for star, recipe and the folder's description
        let folder = sidecar.get(crate::sidecar::FOLDER_SECTION);
        let description = folder.map(|e| e.get("description")).unwrap_or("");
        let sort = folder.map(|e| e.get("sortby")).unwrap_or("");
        let hidden = folder.is_some_and(|e| e.get("P2category") == "Hidden Folders");
        tx.execute("UPDATE folders SET description = ?1, sort = ?2, hidden = ?3 WHERE id = ?4", params![description, sort, hidden as i64, fid])?;
        tx.execute("UPDATE photos SET star = 0, recipe = '', albums = '', hidden = 0 WHERE folder_id = ?1", [fid])?;
        for (fname, e) in sidecar {
            tx.execute(
                "UPDATE photos SET star = ?1, recipe = ?2, albums = ?3, hidden = ?4 WHERE folder_id = ?5 AND name = ?6",
                params![e.star as i64, e.filters, e.get("albums"), (e.get("hidden") == "yes") as i64, fid, fname],
            )?;
            // files that cannot hold tags inside (not JPEG) keep them here, in Picasa's
            // `keywords=` and `caption=` lines
            if !e.get("keywords").is_empty() || !e.get("caption").is_empty() {
                let tags: Vec<String> = e.get("keywords").split(',').map(String::from).collect();
                tx.execute(
                    "UPDATE photos SET tags = ?1, caption = ?2 WHERE folder_id = ?3 AND name = ?4",
                    params![join_tags(&tags), e.get("caption"), fid, fname],
                )?;
            }
        }
        for a in crate::albums::in_sidecar(sidecar) {
            tx.execute(
                "INSERT INTO albums(token, name, date, location, description) VALUES(?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(token) DO UPDATE SET name = excluded.name, date = excluded.date,
                   location = excluded.location, description = excluded.description",
                params![a.token, a.name, a.date, a.location, a.description],
            )?;
        }
        tx.execute(
            "UPDATE folders SET
               count = (SELECT COUNT(*) FROM photos WHERE folder_id = ?1),
               date_min = (SELECT MIN(COALESCE(taken, mtime)) FROM photos WHERE folder_id = ?1),
               date_max = (SELECT MAX(COALESCE(taken, mtime)) FROM photos WHERE folder_id = ?1)
             WHERE id = ?1",
            [fid],
        )?;
        tx.commit()?;
        Ok((fid, ids))
    }

    /// Folders under `root` whose path is not in `seen` are removed (folder deleted on disk).
    pub fn remove_folders_not_in(&self, root: &str, seen: &[String]) -> Result<usize> {
        let c = self.c();
        let keep: std::collections::HashSet<&str> = seen.iter().map(|s| s.as_str()).collect();
        let mut st = c.prepare("SELECT id, path FROM folders WHERE root = ?1")?;
        let gone: Vec<i64> = st
            .query_map([root], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
            .filter_map(|r| r.ok())
            .filter(|(_, p)| !keep.contains(p.as_str()))
            .map(|(id, _)| id)
            .collect();
        drop(st);
        for id in &gone {
            c.execute("DELETE FROM folders WHERE id = ?1", [id])?;
        }
        Ok(gone.len())
    }

    /// Points an indexed photo at its new name (and folder, if that folder is indexed), after
    /// the file was renamed or moved, so it keeps its id, thumbnail and place in a selection.
    /// Returns false when the new folder is not indexed (the next scan adds the photo there).
    pub fn relocate_photo(&self, id: i64, folder: &str, name: &str) -> Result<bool> {
        let c = self.c();
        let fid: Option<i64> = c.query_row("SELECT id FROM folders WHERE path = ?1", [folder], |r| r.get(0)).optional()?;
        let Some(fid) = fid else { return Ok(false) };
        c.execute("DELETE FROM photos WHERE folder_id = ?1 AND name = ?2 AND id <> ?3", params![fid, name, id])?;
        Ok(c.execute("UPDATE photos SET folder_id = ?1, name = ?2 WHERE id = ?3", params![fid, name, id])? == 1)
    }

    /// Forgets every thumbnail (Options > Clear Thumbnail Cache). Returns how many there were.
    pub fn clear_thumbs(&self) -> Result<usize> {
        let c = self.c();
        let n = c.execute("DELETE FROM thumbs", [])?;
        c.execute("UPDATE photos SET thumb_level = 0", [])?;
        Ok(n)
    }

    /// Indexes a new, empty folder (photos are about to be moved into it).
    pub fn add_folder(&self, path: &str, root: &str) -> Result<()> {
        let name = Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string());
        self.c().execute("INSERT OR IGNORE INTO folders (path, name, root) VALUES (?1, ?2, ?3)", params![path, name, root])?;
        Ok(())
    }

    /// Removes one folder (not those below it) from the index.
    pub fn remove_folder(&self, path: &str) -> Result<usize> {
        Ok(self.c().execute("DELETE FROM folders WHERE path = ?1", [path])?)
    }

    /// Every indexed folder path with its scan start point.
    pub fn folder_roots(&self) -> Result<Vec<(String, String)>> {
        let c = self.c();
        let mut st = c.prepare("SELECT path, root FROM folders")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Removes a folder and everything indexed below it (it was deleted on disk).
    pub fn remove_folder_tree(&self, path: &str) -> Result<usize> {
        // prefix compare, not LIKE: "_" and "%" are common in folder names
        let sub = format!("{}\\", path.trim_end_matches('\\'));
        Ok(self.c().execute(
            "DELETE FROM folders WHERE path = ?1 OR substr(path, 1, length(?2)) = ?2",
            params![path, sub],
        )?)
    }

    pub fn thumb(&self, photo_id: i64) -> Result<Option<(i64, Vec<u8>)>> {
        Ok(self
            .c()
            .query_row("SELECT level, jpeg FROM thumbs WHERE photo_id = ?1", [photo_id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?)
    }

    pub fn put_thumbs(&self, items: &[(i64, i64, Vec<u8>)]) -> Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        for (id, level, jpeg) in items {
            // the photo may have been deleted meanwhile; ignore those
            let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM photos WHERE id = ?1)", [id], |r| r.get(0))?;
            if !exists {
                continue;
            }
            tx.execute(
                "INSERT INTO thumbs(photo_id, level, jpeg) VALUES(?1, ?2, ?3)
                 ON CONFLICT(photo_id) DO UPDATE SET level = excluded.level, jpeg = excluded.jpeg",
                params![id, level, jpeg],
            )?;
            tx.execute("UPDATE photos SET thumb_level = ?2 WHERE id = ?1", params![id, level])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Photos whose best thumbnail is below `level`, oldest folders last.
    pub fn needing_thumbs(&self, level: i64, limit: i64) -> Result<Vec<i64>> {
        let c = self.c();
        let mut st = c.prepare("SELECT id FROM photos WHERE thumb_level < ?1 ORDER BY folder_id DESC, id LIMIT ?2")?;
        let rows = st.query_map(params![level, limit], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn set_thumb_level(&self, id: i64, level: i64) -> Result<()> {
        self.c().execute("UPDATE photos SET thumb_level = ?2 WHERE id = ?1", params![id, level])?;
        Ok(())
    }

    pub fn set_star(&self, id: i64, star: bool) -> Result<()> {
        self.c().execute("UPDATE photos SET star = ?2 WHERE id = ?1", params![id, star as i64])?;
        Ok(())
    }

    pub fn set_recipe(&self, id: i64, recipe: &str) -> Result<()> {
        self.c().execute("UPDATE photos SET recipe = ?2 WHERE id = ?1", params![id, recipe])?;
        Ok(())
    }

    /// Calls `f` for every photo with what search looks at.
    pub fn for_each_search_row(&self, mut f: impl FnMut(SearchRow)) -> Result<()> {
        let c = self.c();
        let mut st = c.prepare(
            "SELECT p.id, p.name, f.name, p.tags, p.caption, p.albums, p.star, COALESCE(p.taken, p.mtime)
             FROM photos p JOIN folders f ON f.id = p.folder_id",
        )?;
        let rows = st.query_map([], |r| {
            Ok(SearchRow {
                id: r.get(0)?,
                name: r.get(1)?,
                folder: r.get(2)?,
                tags: r.get(3)?,
                caption: r.get(4)?,
                albums: r.get(5)?,
                star: r.get::<_, i64>(6)? != 0,
                date: r.get(7)?,
            })
        })?;
        for r in rows {
            f(r?);
        }
        Ok(())
    }

    pub fn put_album(&self, a: &crate::albums::Album) -> Result<()> {
        self.c().execute(
            "INSERT INTO albums(token, name, date, location, description) VALUES(?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(token) DO UPDATE SET name = excluded.name, date = excluded.date,
               location = excluded.location, description = excluded.description",
            params![a.token, a.name, a.date, a.location, a.description],
        )?;
        Ok(())
    }

    pub fn delete_album(&self, token: &str) -> Result<()> {
        self.c().execute("DELETE FROM albums WHERE token = ?1", [token])?;
        Ok(())
    }

    /// Every album, with how many photos it holds and its first photo, by name.
    pub fn albums(&self) -> Result<Vec<AlbumRow>> {
        let c = self.c();
        let mut st = c.prepare(
            "SELECT a.token, a.name, a.date, a.location, a.description,
               (SELECT COUNT(*) FROM photos p WHERE instr(',' || p.albums || ',', ',' || a.token || ',') > 0),
               (SELECT p.id FROM photos p WHERE instr(',' || p.albums || ',', ',' || a.token || ',') > 0
                  ORDER BY COALESCE(p.taken, p.mtime), p.name LIMIT 1)
             FROM albums a ORDER BY a.name COLLATE NOCASE",
        )?;
        let rows = st.query_map([], |r| {
            Ok(AlbumRow {
                album: crate::albums::Album { token: r.get(0)?, name: r.get(1)?, date: r.get(2)?, location: r.get(3)?, description: r.get(4)? },
                count: r.get(5)?,
                cover: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The photos in an album, oldest first.
    pub fn album_photos(&self, token: &str) -> Result<Vec<Photo>> {
        let c = self.c();
        let mut st = c.prepare(&format!(
            "SELECT {PHOTO_COLS} FROM photos WHERE instr(',' || albums || ',', ',' || ?1 || ',') > 0
             ORDER BY COALESCE(taken, mtime), name"
        ))?;
        let rows = st.query_map([token], photo_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The folders holding a member of the album (each carries a copy of its section).
    pub fn album_folders(&self, token: &str) -> Result<Vec<std::path::PathBuf>> {
        let c = self.c();
        let mut st = c.prepare(
            "SELECT DISTINCT f.path FROM photos p JOIN folders f ON f.id = p.folder_id
             WHERE instr(',' || p.albums || ',', ',' || ?1 || ',') > 0",
        )?;
        let rows = st.query_map([token], |r| Ok(std::path::PathBuf::from(r.get::<_, String>(0)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Every tag in the library with how many photos carry it, most used first. The same tag
    /// in other capitals counts as one, shown as first seen.
    pub fn all_tags(&self) -> Result<Vec<(String, i64)>> {
        let c = self.c();
        let mut st = c.prepare("SELECT tags FROM photos WHERE tags != ''")?;
        let mut n: std::collections::HashMap<String, (String, i64)> = Default::default();
        for t in st.query_map([], |r| r.get::<_, String>(0))?.flatten() {
            for tag in t.split('\n') {
                n.entry(tag.to_lowercase()).or_insert((tag.to_string(), 0)).1 += 1;
            }
        }
        let mut v: Vec<(String, i64)> = n.into_values().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
        Ok(v)
    }

    pub fn counts(&self) -> Result<(i64, i64)> {
        Ok(self.c().query_row(
            "SELECT (SELECT COUNT(*) FROM folders WHERE count > 0), (SELECT COUNT(*) FROM photos)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }
}

#[cfg(test)]
mod upgrade_tests {
    use super::*;

    /// an index of version 3 (file dates in UTC) keeps everything and gets local-clock dates
    #[test]
    fn version_3_index_moves_file_dates_to_the_local_clock() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("index.db");
        let (summer, winter, taken) = (1_782_000_000_i64, 1_767_000_000_i64, 1_700_000_000_i64);
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(SCHEMA).unwrap();
            c.execute_batch(
                "INSERT INTO folders(id, path, name, root) VALUES (1, 'C:/p/a', 'a', 'C:/p');
                 INSERT INTO albums(token, name, date, location, description) VALUES ('t1', 'Trip', '', '', '');
                 PRAGMA user_version = 3;",
            )
            .unwrap();
            c.execute("INSERT INTO photos(folder_id, name, size, mtime, star) VALUES (1, 'a.jpg', 5, ?1, 1)", [summer]).unwrap();
            c.execute("INSERT INTO photos(folder_id, name, size, mtime, taken) VALUES (1, 'b.jpg', 5, ?1, ?2)", [winter, taken]).unwrap();
        }
        let db = Db::open(&path).unwrap();
        let c = db.c();
        let v: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        let m: Vec<(i64, bool)> = c
            .prepare("SELECT mtime, star FROM photos ORDER BY name")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(m, vec![(crate::dates::local_clock(summer), true), (crate::dates::local_clock(winter), false)], "dates moved, stars kept");
        let (lo, hi): (i64, i64) = c.query_row("SELECT date_min, date_max FROM folders WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((lo, hi), (taken, crate::dates::local_clock(summer)), "a camera date stays; a file date stands in only when there is none");
        let albums: i64 = c.query_row("SELECT COUNT(*) FROM albums", [], |r| r.get(0)).unwrap();
        assert_eq!(albums, 1, "albums kept");
        drop(c);
        drop(db);
        // opening again changes nothing more
        let db = Db::open(&path).unwrap();
        let again: i64 = db.c().query_row("SELECT mtime FROM photos WHERE name = 'a.jpg'", [], |r| r.get(0)).unwrap();
        assert_eq!(again, crate::dates::local_clock(summer));
    }
}
