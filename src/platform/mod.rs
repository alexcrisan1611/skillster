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
use std::path::PathBuf;

/// The app icon, for the Linux desktop entry.
#[cfg(all(unix, not(target_os = "macos")))]
const ICON_PNG: &[u8] = include_bytes!("../../assets/icon.png");

/// The name used before the rename to Skillster.
pub const LEGACY_NAME: &str = "skills-sync";

/// Holds an exclusive lock on a file in `state_dir()` for the life of the process;
/// `None` if another instance has it.
pub fn single_instance() -> Option<File> {
    let dir = state_dir();
    fs::create_dir_all(&dir).ok()?;
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(dir.join("skillsterd.lock"))
        .ok()?;
    file.try_lock().ok()?;
    Some(file)
}

/// Whether a `skillsterd` holds the instance lock right now.
pub fn daemon_running() -> bool {
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(state_dir().join("skillsterd.lock"))
        .is_ok_and(|f| f.try_lock().is_err())
}

/// Where copies replaced by links are moved: `backups` in `state_dir()`.
pub fn backups_dir() -> PathBuf {
    state_dir().join("backups")
}
