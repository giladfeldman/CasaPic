//! CasaPic core: everything except the window. Tested on its own (`cargo test -p casapic-core`).
//!
//! Safety rule for the whole crate: photo files are only ever READ, with one exception the user
//! asks for by name: Save (`save.rs`), which first moves the untouched original into the hidden
//! `.casapicoriginals` folder beside it, where Revert finds it. Otherwise the only files CasaPic
//! writes inside photo folders are `.casapic.ini` sidecars (stars and edit recipes); exports go
//! to a folder the user chooses and never overwrite an existing file.

pub mod albums;
pub mod picasa;
pub mod dates;
pub mod db;
pub mod effects;
pub mod export;
pub mod meta;
pub mod paths;
pub mod recipe;
pub mod render;
pub mod files;
pub mod save;
pub mod tags;
pub mod scan;
pub mod search;
pub mod settings;
pub mod studio;
pub mod sidecar;
pub mod thumbs;
pub mod watch;
pub mod wic;
