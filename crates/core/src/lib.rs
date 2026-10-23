//! CasaPic core: everything except the window. Tested on its own (`cargo test -p casapic-core`).
//!
//! Safety rule for the whole crate: photo files are only ever READ. The only files CasaPic
//! writes inside photo folders are `.casapic.ini` sidecars (stars and edit recipes);
//! exports go to a folder the user chooses and never overwrite an existing file.

pub mod db;
pub mod export;
pub mod meta;
pub mod paths;
pub mod recipe;
pub mod render;
pub mod scan;
pub mod sidecar;
pub mod thumbs;
pub mod watch;
pub mod wic;
