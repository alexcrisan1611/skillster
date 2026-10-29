//! `skills-syncd`: the background process, started at login. No window on Windows;
//! one instance at a time.
//!
//! `skills-syncd [--config <path>]`

#![windows_subsystem = "windows"]

use std::path::PathBuf;

use skills_sync::{config, platform, watch};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut config_path = None;
    while let Some(arg) = args.next() {
        if arg == "--config" {
            config_path = args.next().map(PathBuf::from);
        }
    }
    let config_path = config_path.unwrap_or_else(config::default_path);

    let Some(_lock) = platform::single_instance() else { return };
    watch::run(&config_path);
}
