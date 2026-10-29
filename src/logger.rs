use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Appends timestamped lines to a log file (reset past `max_bytes`), optionally echoing to stdout.
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
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let _ = (|| -> io::Result<()> {
            if let Some(dir) = self.path.parent() {
                fs::create_dir_all(dir)?;
            }
            if fs::metadata(&self.path).is_ok_and(|m| m.len() > self.max_bytes) {
                fs::remove_file(&self.path)?;
            }
            let mut f = OpenOptions::new().create(true).append(true).open(&self.path)?;
            writeln!(f, "[{secs}] {msg}")
        })();
    }
}
