//! Keeps every skill in a source folder linked (junctions on Windows, symlinks
//! elsewhere) into each target folder, and removes links whose skill has gone.
//!
//! Two binaries share this crate: `skills-sync` (the command line, including the
//! installer) and `skills-syncd` (the background process).

pub mod config;
#[cfg(windows)]
pub mod everything;
pub mod find;
pub mod install;
pub mod logger;
pub mod platform;
pub mod sync;
pub mod watch;
