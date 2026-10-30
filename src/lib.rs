//! Keeps every skill in a source folder linked (junctions on Windows, symlinks
//! elsewhere) into each target folder, and removes links whose skill has gone.
//!
//! Three binaries share this crate: `skillster` (the command line, including the
//! installer), `skillsterd` (the background process) and `skillster-ui` (the window).

pub mod config;
#[cfg(windows)]
pub mod everything;
pub mod find;
pub mod install;
pub mod logger;
pub mod pick;
pub mod platform;
pub mod status;
pub mod sync;
pub mod watch;
