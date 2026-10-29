//! Everything that differs between operating systems: links, standard folders,
//! path syntax, and starting the background process at login.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

use std::fs::{self, File, OpenOptions};

/// Holds an exclusive lock on a file in `state_dir()` for the life of the process;
/// `None` if another instance has it.
pub fn single_instance() -> Option<File> {
    let dir = state_dir();
    fs::create_dir_all(&dir).ok()?;
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(dir.join("skills-syncd.lock"))
        .ok()?;
    file.try_lock().ok()?;
    Some(file)
}
