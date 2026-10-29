//! `skills-sync`: the command line.

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::ExitCode;

use skills_sync::config::{self, Config};
use skills_sync::logger::Logger;
use skills_sync::{install, sync};

fn usage() -> String {
    format!(
        "usage: skills-sync <command> [--config <path>]

commands:
  install [--es <path>]  choose the folders to sync, then install and start the background process
                         (--es: Everything's es.exe, Windows only)
  uninstall              stop it and remove it (keeps the config and existing links)
  once                   sync once and print what changed
  config                 show the config file in use and the settings it resolves to
  version                print the version

--config defaults to {}",
        config::default_path().display()
    )
}

fn main() -> ExitCode {
    let mut command = None;
    let mut config_path = None;
    let mut es = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" | "--es" => {
                let Some(value) = args.next() else { return usage_error(&format!("{arg} needs a value")) };
                *(if arg == "--config" { &mut config_path } else { &mut es }) = Some(PathBuf::from(value));
            }
            "-V" | "--version" | "version" => {
                println!("skills-sync {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "-h" | "--help" | "help" => {
                println!("{}", usage());
                return ExitCode::SUCCESS;
            }
            // Kept from before subcommands existed.
            "--once" if command.is_none() => command = Some("once".to_string()),
            _ if command.is_none() && !arg.starts_with('-') => command = Some(arg),
            _ => return usage_error(&format!("unexpected argument {arg:?}")),
        }
    }
    let config_path = config_path.unwrap_or_else(config::default_path);

    let result = match command.as_deref() {
        Some("install") => install::install(&config_path, es.as_deref()),
        Some("uninstall") => install::uninstall(&config_path),
        Some("once") => Config::load(&config_path).map(|c| {
            let log = Logger::new(c.log_path.clone(), c.log_max_bytes, true);
            sync::sync(&c, &log, &mut HashSet::new());
        }),
        Some("config") => show_config(&config_path),
        Some(other) => return usage_error(&format!("unknown command {other:?}")),
        None => return usage_error("no command given"),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn show_config(path: &std::path::Path) -> Result<(), String> {
    let note = if path.exists() { "" } else { " (missing; using defaults)" };
    println!("config:   {}{note}", path.display());
    let c = Config::load(path)?;
    println!("source:   {}", c.source.display());
    for t in &c.targets {
        println!("target:   {}", t.display());
    }
    println!("rescan:   {} s", c.rescan.as_secs());
    println!("debounce: {} ms", c.debounce.as_millis());
    println!("log:      {} (reset past {} bytes)", c.log_path.display(), c.log_max_bytes);
    Ok(())
}

fn usage_error(msg: &str) -> ExitCode {
    eprintln!("error: {msg}\n\n{}", usage());
    ExitCode::from(2)
}
