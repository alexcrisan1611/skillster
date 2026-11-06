use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;

use crate::platform::local_time;

/// Appends lines stamped with the local time to a log file, optionally echoing to
/// stdout. Past `max_bytes` the file becomes the previous log (`<name>.1`, replacing
/// the one before) and a new one starts.
pub struct Logger {
    path: PathBuf,
    max_bytes: u64,
    to_stdout: bool,
}

impl Logger {
    pub fn new(path: PathBuf, max_bytes: u64, to_stdout: bool) -> Self {
        Self { path, max_bytes, to_stdout }
    }

    pub fn line(&self, msg: &str) {
        if self.to_stdout {
            println!("{msg}");
        }
        let _ = (|| -> io::Result<()> {
            if let Some(dir) = self.path.parent() {
                fs::create_dir_all(dir)?;
            }
            if fs::metadata(&self.path).is_ok_and(|m| m.len() > self.max_bytes) {
                fs::rename(&self.path, self.previous())?;
            }
            let mut f = OpenOptions::new().create(true).append(true).open(&self.path)?;
            writeln!(f, "[{}] {msg}", local_time())
        })();
    }

    /// `skillster.log` becomes `skillster.log.1`.
    fn previous(&self) -> PathBuf {
        let mut name = self.path.file_name().unwrap_or_default().to_os_string();
        name.push(".1");
        self.path.with_file_name(name)
    }
}
