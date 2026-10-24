//! `.casapic.ini`: the per-folder file that carries stars and edit recipes beside the photos,
//! modelled on Picasa's `.picasa.ini`. One section per photo; photos without a star or edits
//! have no section. Written atomically (temp file + rename) and marked hidden.
//!
//! ```text
//! [IMG_0001.jpg]
//! star=yes
//! filters=autolight=1,0.012,0.985;fill=1,0.4;
//! ```

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub const FILE_NAME: &str = ".casapic.ini";

/// The section holding the folder's own settings (its description), named as in Picasa's
/// `.picasa.ini`. Photo sections are named after files, which always have an extension.
pub const FOLDER_SECTION: &str = "Picasa";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Entry {
    pub star: bool,
    pub filters: String,
    /// every other `key=value` line, kept as written (so nothing CasaPic does not know about
    /// yet, or that a later version wrote, is lost when the file is rewritten)
    pub extra: BTreeMap<String, String>,
}

impl Entry {
    fn is_empty(&self) -> bool {
        !self.star && self.filters.is_empty() && self.extra.values().all(|v| v.is_empty())
    }

    pub fn get(&self, key: &str) -> &str {
        self.extra.get(key).map(String::as_str).unwrap_or("")
    }

    /// Sets a value; an empty one removes the line.
    pub fn set(&mut self, key: &str, value: &str) {
        let v = value.replace(['\r', '\n'], " ");
        if v.trim().is_empty() {
            self.extra.remove(key);
        } else {
            self.extra.insert(key.to_string(), v);
        }
    }
}

pub fn path_for(folder: &Path) -> PathBuf {
    folder.join(FILE_NAME)
}

pub fn parse(text: &str) -> HashMap<String, Entry> {
    let mut out: HashMap<String, Entry> = HashMap::new();
    let mut cur: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            cur = Some(name.to_string());
            out.entry(name.to_string()).or_default();
            continue;
        }
        let (Some(name), Some((k, v))) = (cur.as_ref(), line.split_once('=')) else { continue };
        let e = out.entry(name.clone()).or_default();
        match k.trim() {
            "star" => e.star = v.trim().eq_ignore_ascii_case("yes"),
            "filters" => e.filters = v.trim().to_string(),
            other => {
                e.extra.insert(other.to_string(), v.trim().to_string());
            }
        }
    }
    out.retain(|_, e| !e.is_empty());
    out
}

pub fn render(entries: &HashMap<String, Entry>) -> String {
    let sorted: BTreeMap<_, _> = entries.iter().filter(|(_, e)| !e.is_empty()).collect();
    let mut s = String::new();
    for (name, e) in sorted {
        s.push_str(&format!("[{name}]\r\n"));
        if e.star {
            s.push_str("star=yes\r\n");
        }
        if !e.filters.is_empty() {
            s.push_str(&format!("filters={}\r\n", e.filters));
        }
        for (k, v) in &e.extra {
            if !v.is_empty() {
                s.push_str(&format!("{k}={v}\r\n"));
            }
        }
    }
    s
}

pub fn read(folder: &Path) -> HashMap<String, Entry> {
    match std::fs::read_to_string(path_for(folder)) {
        Ok(t) => parse(&t),
        Err(_) => HashMap::new(),
    }
}

/// Updates one photo's entry and rewrites the folder's sidecar (or deletes it when empty).
pub fn update(folder: &Path, file_name: &str, f: impl FnOnce(&mut Entry)) -> Result<Entry> {
    let mut all = read(folder);
    let mut e = all.remove(file_name).unwrap_or_default();
    f(&mut e);
    if !e.is_empty() {
        all.insert(file_name.to_string(), e.clone());
    }
    write(folder, &all)?;
    Ok(e)
}

fn write(folder: &Path, entries: &HashMap<String, Entry>) -> Result<()> {
    let target = path_for(folder);
    let text = render(entries);
    if text.is_empty() {
        if target.exists() {
            std::fs::remove_file(&target).with_context(|| format!("remove {}", target.display()))?;
        }
        return Ok(());
    }
    let tmp = folder.join(".casapic.ini.tmp");
    std::fs::write(&tmp, text.as_bytes()).with_context(|| format!("write {}", tmp.display()))?;
    // A hidden target can refuse replacement on Windows; unhide it first.
    set_hidden(&target, false);
    std::fs::rename(&tmp, &target).with_context(|| format!("replace {}", target.display()))?;
    set_hidden(&target, true);
    Ok(())
}

pub(crate) fn set_hidden(path: &Path, hidden: bool) {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_NORMAL, FILE_FLAGS_AND_ATTRIBUTES, SetFileAttributesW,
    };
    if !path.exists() {
        return;
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let attr: FILE_FLAGS_AND_ATTRIBUTES = if hidden { FILE_ATTRIBUTE_HIDDEN } else { FILE_ATTRIBUTE_NORMAL };
    unsafe {
        let _ = SetFileAttributesW(windows::core::PCWSTR(wide.as_ptr()), attr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_empty_sections_dropped() {
        let text = "[a.jpg]\nstar=yes\n[b.jpg]\nfilters=fill=1,0.5;\n[c.jpg]\n";
        let m = parse(text);
        assert_eq!(m.len(), 2);
        assert!(m["a.jpg"].star);
        assert_eq!(m["b.jpg"].filters, "fill=1,0.5;");
        assert_eq!(parse(&render(&m)), m);
    }

    #[test]
    fn unknown_lines_survive_a_rewrite() {
        let text = "[Picasa]\ndescription=Summer at the lake\n[a.jpg]\nstar=yes\ncaption=Me, on the dock\nfuture=1,2,3\n";
        let m = parse(text);
        assert_eq!(m[FOLDER_SECTION].get("description"), "Summer at the lake");
        assert_eq!(m["a.jpg"].get("caption"), "Me, on the dock");
        let again = parse(&render(&m));
        assert_eq!(again, m);
        let mut e = again["a.jpg"].clone();
        e.set("caption", "two\nlines");
        assert_eq!(e.get("caption"), "two lines", "a value never breaks the file into lines");
        e.set("caption", "");
        assert_eq!(e.get("caption"), "");
    }

    #[test]
    fn update_writes_hidden_file_and_deletes_when_empty() {
        let d = tempfile::tempdir().unwrap();
        update(d.path(), "x.jpg", |e| e.star = true).unwrap();
        // second write must succeed although the file is now hidden
        update(d.path(), "x.jpg", |e| e.filters = "fill=1,0.3;".into()).unwrap();
        let m = read(d.path());
        assert_eq!(m["x.jpg"], Entry { star: true, filters: "fill=1,0.3;".into(), ..Default::default() });
        let attrs = std::fs::metadata(path_for(d.path())).unwrap();
        use std::os::windows::fs::MetadataExt;
        assert!(attrs.file_attributes() & 0x2 != 0, "sidecar should be hidden");
        update(d.path(), "x.jpg", |e| *e = Entry::default()).unwrap();
        assert!(!path_for(d.path()).exists());
    }
}
