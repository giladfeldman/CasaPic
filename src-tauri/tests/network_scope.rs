//! CasaPic is offline except for its update check. CONTRIBUTING.md and SECURITY.md say so, and
//! this test keeps the code and those promises together: no other network crate, no `fetch` to
//! the internet from the web pages, and both documents still name the update check as the only
//! exception.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn read(rel: &str) -> String {
    fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

const NETWORK_CRATES: &[&str] = &[
    "reqwest", "ureq", "hyper", "curl", "isahc", "surf", "attohttpc", "tauri-plugin-http", "tauri-plugin-websocket",
];

#[test]
fn only_the_updater_may_reach_the_network() {
    for manifest in ["src-tauri/Cargo.toml", "crates/core/Cargo.toml", "Cargo.toml"] {
        let text = read(manifest);
        for line in text.lines() {
            let name = line.split('=').next().unwrap_or("").trim();
            assert!(!NETWORK_CRATES.contains(&name), "{manifest}: network crate `{name}`; CasaPic's only network access is the updater");
        }
    }
    let mut pages = vec![root().join("src/viewer.ts"), root().join("src/main.ts")];
    for dir in ["src/lib", "src/components"] {
        for e in fs::read_dir(root().join(dir)).unwrap() {
            pages.push(e.unwrap().path());
        }
    }
    for p in pages {
        let text = fs::read_to_string(&p).unwrap_or_default();
        for needle in ["fetch(\"http", "fetch('http", "fetch(`http", "new WebSocket(", "XMLHttpRequest"] {
            assert!(!text.contains(needle), "{}: `{needle}` reaches the network", p.display());
        }
    }
}

#[test]
fn the_documents_name_the_update_check_as_the_only_exception() {
    let contributing = read("CONTRIBUTING.md");
    assert!(contributing.contains("No network access except the update check"), "CONTRIBUTING.md must name the update check");
    assert!(contributing.contains("src-tauri/src/updates.rs"), "CONTRIBUTING.md must point at updates.rs");
    let security = read("SECURITY.md");
    assert!(security.contains("works offline, except for its update check"), "SECURITY.md must name the update check");
    for (name, text) in [("CONTRIBUTING.md", &contributing), ("SECURITY.md", &security)] {
        assert!(!text.contains("works entirely offline and sends nothing anywhere"), "{name} still promises no network at all");
    }
}
