//! Keeps every skill in a source folder linked (junctions on Windows, symlinks
//! elsewhere) into each target folder, and removes links whose skill has gone.
//!
//! Two binaries share this crate: `skillster` (the command line, including the
//! installer) and `skillsterd` (the background process).

pub mod config;
#[cfg(windows)]
pub mod everything;
pub mod find;
pub mod install;
pub mod logger;
pub mod pick;
pub mod platform;
pub mod sync;
pub mod watch;
