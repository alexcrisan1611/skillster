//! Scans in the background: whenever the source, a target or the config changes,
//! when asked, and every few seconds regardless (to notice the background process
//! starting or stopping).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use eframe::egui;
use notify::{RecursiveMode, Watcher};
use skillster::config::Config;
use skillster::platform;
use skillster::status::{self, Report};

/// One look at everything.
pub struct Scan {
    /// The settings in effect, or why the config could not be read.
    pub config: Result<Config, String>,
    pub report: Report,
    pub daemon_running: bool,
}

pub struct Scanner {
    pub scans: Receiver<Scan>,
    wake: Sender<()>,
}

const EVERY: Duration = Duration::from_secs(4);
/// Lets a burst of changes, and the background process's own linking, settle first.
const SETTLE: Duration = Duration::from_millis(400);

impl Scanner {
    pub fn spawn(ctx: egui::Context, config_path: PathBuf) -> Scanner {
        let (scan_tx, scans) = mpsc::channel();
        let (wake, wake_rx) = mpsc::channel();
        let events = wake.clone();
        std::thread::spawn(move || run(&ctx, &config_path, &scan_tx, &wake_rx, events));
        Scanner { scans, wake }
    }

    /// Scan again now.
    pub fn refresh(&self) {
        let _ = self.wake.send(());
    }
}

fn run(ctx: &egui::Context, config_path: &Path, out: &Sender<Scan>, wake: &Receiver<()>, events: Sender<()>) {
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if res.is_ok() {
            let _ = events.send(());
        }
    })
    .ok();
    let mut watched: Vec<PathBuf> = Vec::new();
    loop {
        let config = Config::load(config_path);
        if let Some(w) = watcher.as_mut() {
            let mut want: Vec<PathBuf> = config_path.parent().map(Path::to_path_buf).into_iter().collect();
            if let Ok(c) = &config {
                want.push(c.source.clone());
                want.extend(c.targets.iter().cloned());
            }
            if want != watched {
                for p in watched.drain(..) {
                    let _ = w.unwatch(&p);
                }
                for p in want {
                    if w.watch(&p, RecursiveMode::NonRecursive).is_ok() {
                        watched.push(p);
                    }
                }
            }
        }
        let report = config.as_ref().map(status::scan).unwrap_or_default();
        let scan = Scan { config, report, daemon_running: platform::daemon_running() };
        if out.send(scan).is_err() {
            return;
        }
        ctx.request_repaint();
        match wake.recv_timeout(EVERY) {
            Ok(()) => while wake.recv_timeout(SETTLE).is_ok() {},
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}
