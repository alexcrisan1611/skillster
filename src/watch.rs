use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::SystemTime;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::config::{Config, ConfigFile};
use crate::logger::Logger;
use crate::sync::sync;

/// Sync forever: on every change to the source folder, every `rescan` regardless, and
/// with fresh settings whenever the config file changes. A bad config keeps the last good one.
pub fn run(config_path: &Path) -> ! {
    let fallback = ConfigFile::default().resolve().expect("default config is valid");
    let mut log = logger_for(&fallback);
    let mut config: Option<Config> = None;
    let mut stamp = None;
    let mut first = true;
    let mut reported = HashSet::new();

    let (tx, rx) = mpsc::channel();
    let mut watcher = match notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    }) {
        Ok(w) => Some(w),
        Err(e) => {
            log.line(&format!("watcher unavailable, rescanning only: {e}"));
            None
        }
    };
    // Watch the config's folder, not the file, so the file can be created or replaced.
    if let (Some(w), Some(dir)) = (watcher.as_mut(), config_path.parent()) {
        let _ = fs::create_dir_all(dir);
        let _ = w.watch(dir, RecursiveMode::NonRecursive);
    }
    let mut watched_source: Option<PathBuf> = None;

    loop {
        let now = file_stamp(config_path);
        if first || now != stamp {
            stamp = now;
            match Config::load(config_path) {
                Ok(c) if config.as_ref() != Some(&c) => {
                    log = logger_for(&c);
                    log.line(if first { "started" } else { "config reloaded" });
                    reported.clear();
                    config = Some(c);
                }
                Ok(_) => {}
                Err(e) => {
                    let keeping = if config.is_some() { "keeping previous settings" } else { "not syncing until it is fixed" };
                    log.line(&format!("config error, {keeping}: {e}"));
                }
            }
            first = false;
        }

        if let Some(c) = &config {
            if let Some(w) = watcher.as_mut() {
                rewatch_source(w, &mut watched_source, &c.source);
            }
            sync(c, &log, &mut reported);
        }

        let c = config.as_ref().unwrap_or(&fallback);
        match rx.recv_timeout(c.rescan) {
            Ok(_) => {
                // Let a burst of events settle, then sync once.
                while rx.recv_timeout(c.debounce).is_ok() {}
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                watcher = None;
                std::thread::sleep(c.rescan);
            }
        }
        // A deleted source folder silently stops reporting; watch it again once it is back.
        if watched_source.as_ref().is_some_and(|s| !s.exists()) {
            watched_source = None;
        }
    }
}

/// Points the watcher at `source` if it is not already watching it.
fn rewatch_source(w: &mut RecommendedWatcher, watched: &mut Option<PathBuf>, source: &Path) {
    if watched.as_deref() == Some(source) {
        return;
    }
    if let Some(old) = watched.take() {
        let _ = w.unwatch(&old);
    }
    let _ = w.unwatch(source);
    if w.watch(source, RecursiveMode::NonRecursive).is_ok() {
        *watched = Some(source.to_path_buf());
    }
}

fn logger_for(c: &Config) -> Logger {
    Logger::new(c.log_path.clone(), c.log_max_bytes, false)
}

/// Changes whenever the file is written, created or deleted.
fn file_stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let m = fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
}
