//! CasaPic's own settings, kept in `settings.json` in its data folder (never beside the photos):
//! which folders make up the library (Picasa's Folder Manager) and the Options dialog.
//!
//! Folder rules, as in Picasa: every folder chosen in the Folder Manager is marked Scan Always
//! (indexed and watched for changes), Scan Once (indexed now, not watched) or Remove (left out,
//! with everything below it). A folder takes the mark of the nearest marked folder at or above
//! it, so "Pictures = Scan Always, Pictures\Scans = Remove" includes all of Pictures except Scans.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scan {
    Always,
    Once,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub path: PathBuf,
    pub scan: Scan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    /// ask before Delete from Disk (the file goes to the Recycle Bin either way)
    pub confirm_delete: bool,
    /// a single click on the photo goes back to the Library (Picasa's option; off by default)
    pub single_click_exit_edit: bool,
    /// file types included in the library (extensions, lower case)
    pub file_types: Vec<String>,
    /// slideshow: seconds per photo
    pub slide_seconds: f32,
    /// slideshow: "fade" or "none"
    pub slide_transition: String,
    /// colours: "system" (follow Windows), "light" or "dark"
    pub theme: String,
    /// saved Looks: named sets of edits that can be put on any photo or on many at once
    pub looks: Vec<Look>,
}

/// A saved Look: its name and the edits, as recipe text (colour and tone edits only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Look {
    pub name: String,
    pub recipe: String,
}

/// The Look measured from 111 Snapseed edits (a vignette; see studio.rs), offered from the start.
pub const MY_SNAPSEED_LOOK: &str = "cp_vignette=1,-64.4,14,20.8,0.5,0.5;";

impl Default for Options {
    fn default() -> Self {
        Options {
            confirm_delete: true,
            single_click_exit_edit: false,
            file_types: crate::scan::IMAGE_EXTS.iter().map(|s| s.to_string()).collect(),
            slide_seconds: 3.0,
            slide_transition: "fade".into(),
            theme: "system".into(),
            looks: vec![Look { name: "My Snapseed look".into(), recipe: MY_SNAPSEED_LOOK.into() }],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// the Folder Manager's marks
    pub folders: Vec<Rule>,
    /// false until the first-launch Folder Manager has been closed
    pub folders_confirmed: bool,
    pub options: Options,
    /// CasaPic's list of albums (as Picasa keeps its own); each folder holding a member also
    /// carries a copy in its `.casapic.ini`, so albums travel with the photos
    pub albums: Vec<crate::albums::Album>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { folders: Vec::new(), folders_confirmed: false, options: Options::default(), albums: Vec::new() }
    }
}

fn file() -> PathBuf {
    crate::paths::data_dir().join("settings.json")
}

/// Normalised for comparison: no trailing separator; Windows paths compare without case.
fn key(p: &Path) -> String {
    p.to_string_lossy().trim_end_matches(['\\', '/']).replace('/', "\\").to_lowercase()
}

fn is_within(p: &Path, base: &Path) -> bool {
    let (p, b) = (key(p), key(base));
    p == b || p.starts_with(&format!("{b}\\"))
}

impl Settings {
    /// Reads the settings. On the very first launch (no file yet), the library is the Pictures
    /// folder, or the folders named by CASAPIC_ROOTS for development and tests.
    pub fn load() -> Settings {
        let mut s: Settings = std::fs::read_to_string(file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        if s.folders.is_empty() && !s.folders_confirmed {
            s.folders = crate::paths::roots().into_iter().map(|path| Rule { path, scan: Scan::Always }).collect();
        }
        s
    }

    pub fn save(&self) -> Result<()> {
        let f = file();
        if let Some(d) = f.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = f.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?).with_context(|| format!("write {}", tmp.display()))?;
        std::fs::rename(&tmp, &f).with_context(|| format!("replace {}", f.display()))?;
        Ok(())
    }

    /// The mark that applies to `dir` (from the nearest marked folder at or above it).
    pub fn scan_of(&self, dir: &Path) -> Option<Scan> {
        self.folders.iter().filter(|r| is_within(dir, &r.path)).max_by_key(|r| key(&r.path).len()).map(|r| r.scan)
    }

    /// Marks a folder (None clears its own mark, so it follows the folder above it again).
    pub fn set(&mut self, dir: &Path, scan: Option<Scan>) {
        self.folders.retain(|r| key(&r.path) != key(dir));
        if let Some(scan) = scan {
            self.folders.push(Rule { path: dir.to_path_buf(), scan });
        }
        self.folders.sort_by_key(|r| key(&r.path));
    }

    /// Folders scanned from the top with their sub-folders, and whether each one is watched.
    /// A marked folder inside another one is its own start point (with its own mark).
    pub fn scan_starts(&self) -> Vec<(PathBuf, Scan)> {
        self.folders.iter().filter(|r| r.scan != Scan::Remove).map(|r| (r.path.clone(), r.scan)).collect()
    }

    /// Whether a folder met while scanning down from `start` belongs to that scan: false where a
    /// deeper mark takes over (Remove, or a start point of its own).
    pub fn belongs_to(&self, dir: &Path, start: &Path) -> bool {
        self.folders.iter().filter(|r| is_within(dir, &r.path)).max_by_key(|r| key(&r.path).len()).is_some_and(|r| key(&r.path) == key(start))
    }

    /// The start point a folder is scanned from (its nearest marked folder), if it is included.
    pub fn start_of(&self, dir: &Path) -> Option<PathBuf> {
        let r = self.folders.iter().filter(|r| is_within(dir, &r.path)).max_by_key(|r| key(&r.path).len())?;
        (r.scan != Scan::Remove).then(|| r.path.clone())
    }

    /// After a folder was renamed or moved on disk: marks on it or below it follow it.
    pub fn folder_moved(&mut self, from: &Path, to: &Path) {
        let (f, t) = (key(from), to.to_string_lossy().trim_end_matches(['\\', '/']).to_string());
        for r in &mut self.folders {
            let k = key(&r.path);
            if k == f || k.starts_with(&format!("{f}\\")) {
                let rest = &r.path.to_string_lossy().trim_end_matches(['\\', '/']).to_string()[f.len()..];
                r.path = PathBuf::from(format!("{t}{rest}"));
            }
        }
        self.folders.sort_by_key(|r| key(&r.path));
    }

    /// After a folder was deleted: marks on it or below it go.
    pub fn folder_deleted(&mut self, dir: &Path) {
        self.folders.retain(|r| !is_within(&r.path, dir));
    }

    /// Whether a folder is in the library at all.
    pub fn included(&self, dir: &Path) -> bool {
        matches!(self.scan_of(dir), Some(Scan::Always | Scan::Once))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_mark_wins() {
        let mut s = Settings::default();
        s.set(Path::new(r"C:\Pics"), Some(Scan::Always));
        s.set(Path::new(r"C:\Pics\Scans"), Some(Scan::Remove));
        s.set(Path::new(r"C:\Pics\Scans\Keep"), Some(Scan::Once));
        assert_eq!(s.scan_of(Path::new(r"C:\Pics\2015")), Some(Scan::Always));
        assert_eq!(s.scan_of(Path::new(r"c:\pics\scans\old")), Some(Scan::Remove), "case does not matter");
        assert_eq!(s.scan_of(Path::new(r"C:\Pics\Scans\Keep\a")), Some(Scan::Once));
        assert_eq!(s.scan_of(Path::new(r"C:\Pictures")), None, "a name that only starts the same is not inside");
        assert!(s.belongs_to(Path::new(r"C:\Pics\2015"), Path::new(r"C:\Pics")));
        assert!(!s.belongs_to(Path::new(r"C:\Pics\Scans\x"), Path::new(r"C:\Pics")));
        assert!(!s.belongs_to(Path::new(r"C:\Pics\Scans\Keep"), Path::new(r"C:\Pics")), "has its own start");
        assert_eq!(s.scan_starts().len(), 2);
        s.set(Path::new(r"C:\Pics\Scans"), None);
        assert_eq!(s.scan_of(Path::new(r"C:\Pics\Scans\old")), Some(Scan::Always), "unmarked follows the folder above");
    }

    #[test]
    fn marks_follow_a_moved_folder() {
        let mut s = Settings::default();
        s.set(Path::new(r"C:\Pics\Trip"), Some(Scan::Always));
        s.set(Path::new(r"C:\Pics\Trip\Raw"), Some(Scan::Remove));
        s.set(Path::new(r"C:\Pics\Trips"), Some(Scan::Once));
        s.folder_moved(Path::new(r"c:\pics\trip"), Path::new(r"D:\Old\Trip 2019"));
        let paths: Vec<String> = s.folders.iter().map(|r| r.path.to_string_lossy().to_string()).collect();
        assert_eq!(paths, [r"C:\Pics\Trips", r"D:\Old\Trip 2019", r"D:\Old\Trip 2019\Raw"]);
        s.folder_deleted(Path::new(r"D:\Old\Trip 2019"));
        assert_eq!(s.folders.len(), 1);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let s: Settings = serde_json::from_str(r#"{"folders":[{"path":"C:\\P","scan":"once"}]}"#).unwrap();
        assert_eq!(s.folders[0].scan, Scan::Once);
        assert!(s.options.confirm_delete);
        assert!(s.options.file_types.contains(&"jpg".to_string()));
        assert_eq!(s.options.theme, "system");
        // settings from before Looks existed still get the measured Snapseed look
        assert_eq!(s.options.looks[0].name, "My Snapseed look");
        assert!(matches!(crate::recipe::parse(&s.options.looks[0].recipe)[..], [crate::recipe::Op::Vignette { .. }]));
    }
}
