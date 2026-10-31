//! `skillster-ui`: the window. Shows which skills each target folder does not get
//! from the source, and puts them right.
//!
//! `skillster-ui [--config <path>]`

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod skills_sh;
mod sources;
mod theme;
mod watch;
mod window;

use std::path::PathBuf;

use eframe::egui;

fn main() -> eframe::Result {
    let mut args = std::env::args().skip(1);
    let mut config_path = None;
    while let Some(arg) = args.next() {
        if arg == "--config" {
            config_path = args.next().map(PathBuf::from);
        }
    }
    let config_path = config_path.unwrap_or_else(skillster::config::default_path);

    let mut viewport =
        window::builder(egui::ViewportBuilder::default().with_title("Skillster").with_inner_size([760.0, 620.0]));
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icon.png")) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("skillster", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, config_path)))))
}
