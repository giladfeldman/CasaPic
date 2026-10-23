//! Watches the library roots and reports which folders changed, after a short quiet
//! period so that a camera import of 300 files triggers one rescan, not 300.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::Result;
use notify::{RecursiveMode, Watcher};

pub const QUIET: Duration = Duration::from_millis(700);

/// Keeps the watcher alive; dropping it stops watching.
pub struct Watch {
    _watcher: notify::RecommendedWatcher,
}

/// Calls `on_change` (on a background thread) with the set of folders to rescan.
/// A changed file reports its parent folder; a changed folder reports itself and its parent.
pub fn start(roots: &[PathBuf], on_change: impl Fn(Vec<PathBuf>) + Send + 'static) -> Result<Watch> {
    let (tx, rx) = mpsc::channel::<PathBuf>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            for p in ev.paths {
                let _ = tx.send(p);
            }
        }
    })?;
    for r in roots {
        watcher.watch(r, RecursiveMode::Recursive)?;
    }
    std::thread::spawn(move || {
        let mut pending: BTreeSet<PathBuf> = BTreeSet::new();
        loop {
            match rx.recv_timeout(QUIET) {
                Ok(p) => {
                    // ignore our own sidecar writes
                    if p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(".casapic.ini")) {
                        continue;
                    }
                    if p.is_dir() {
                        pending.insert(p.clone());
                    }
                    if let Some(parent) = p.parent() {
                        pending.insert(parent.to_path_buf());
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if !pending.is_empty() {
                        on_change(std::mem::take(&mut pending).into_iter().collect());
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    })
    ;
    Ok(Watch { _watcher: watcher })
}

/// The root a folder belongs to, if any.
pub fn root_of<'a>(roots: &'a [PathBuf], dir: &Path) -> Option<&'a PathBuf> {
    roots.iter().find(|r| dir.starts_with(r))
}
