//! `skillster install` and `skillster uninstall`.

use std::collections::HashSet;
use std::fs;

use std::path::Path;
use std::thread;
use std::time::Duration;

use crate::config::{self, ConfigFile, LogFile};
use crate::{find, pick};
use crate::logger::Logger;
use crate::platform::{self, CLI_EXE, DAEMON_EXE, LEGACY_NAME, UI_EXE, normalize};
use crate::sync::sync;

/// Asks for the folders, writes the config, copies the binaries into
/// `platform::install_dir()`, adds the window to the system's applications,
/// registers the background process to start at login, and starts it. An install from before the rename to Skillster is replaced, its
/// config carried over.
pub fn install(config_path: &Path) -> Result<(), String> {
    let here = std::env::current_exe().map_err(|e| format!("cannot locate this program: {e}"))?;
    let daemon_src = here.with_file_name(DAEMON_EXE);
    if !daemon_src.is_file() {
        return Err(format!("{DAEMON_EXE} must be next to {}", here.display()));
    }

    carry_over_legacy_config(config_path);
    let file = choose_settings(config_path)?;
    let config = file.resolve()?;
    file.write(config_path)?;
    println!("\nSaved {}", config_path.display());

    let dir = platform::install_dir();
    platform::stop_daemon();
    platform::stop_ui();
    platform::remove_legacy_install()?;
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    copy_retrying(&here, &dir.join(CLI_EXE))?;
    copy_retrying(&daemon_src, &dir.join(DAEMON_EXE))?;
    // Builds without the window (the static Linux ones) ship without it.
    let ui_src = here.with_file_name(UI_EXE);
    let app_entry = if ui_src.is_file() {
        copy_retrying(&ui_src, &dir.join(UI_EXE))?;
        Some(platform::add_app_entry(&dir.join(UI_EXE))?)
    } else {
        None
    };

    println!("\nSyncing now:");
    sync(&config, &Logger::new(config.log_path.clone(), config.log_max_bytes, true), &mut HashSet::new());

    let custom_config = (normalize(config_path) != normalize(&config::default_path())).then_some(config_path);
    let autostart = platform::enable_autostart(&dir.join(DAEMON_EXE), custom_config)?;

    println!("\nInstalled to {}. It runs in the background and starts at login ({autostart}).", dir.display());
    if let Some(entry) = app_entry {
        println!("Open Skillster from your applications ({entry}) to see and fix what isn't linked.");
    }
    println!("Edit {} to change folders; changes apply straight away.", config_path.display());
    println!("Log: {}", config.log_path.display());
    let on_path = std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| normalize(&d) == normalize(&dir)));
    if !on_path {
        println!("To run `skillster` by name, add {} to your PATH.", dir.display());
    }
    Ok(())
}

/// Stops the background process, removes the login entry and the installed binaries.
/// The config and the links already made are left in place.
pub fn uninstall(config_path: &Path) -> Result<(), String> {
    platform::stop_daemon();
    platform::stop_ui();
    platform::disable_autostart()?;
    platform::remove_app_entry()?;
    let dir = platform::install_dir();
    platform::remove_binaries(&dir)?;
    println!("Uninstalled from {}.", dir.display());
    println!("Kept the config ({}) and the links already made.", config_path.display());
    Ok(())
}

/// Copies the config of a `skills-sync` install to `config_path` when that is the
/// default and does not exist yet, moving its log to the new default if it used the old.
fn carry_over_legacy_config(config_path: &Path) {
    let legacy = platform::config_dir().with_file_name(LEGACY_NAME).join("config.toml");
    if config_path.exists() || normalize(config_path) != normalize(&config::default_path()) || !legacy.is_file() {
        return;
    }
    let Ok(mut file) = ConfigFile::read(&legacy) else { return };
    let old_log = platform::state_dir().with_file_name(LEGACY_NAME).join(format!("{LEGACY_NAME}.log"));
    if file.log.path == config::contract(&old_log) {
        file.log.path = LogFile::default().path;
    }
    if file.write(config_path).is_ok() {
        println!("Carried over the config of skills-sync ({}).", legacy.display());
    }
}

/// The existing config if the user keeps it, else one with freshly chosen folders.
fn choose_settings(config_path: &Path) -> Result<ConfigFile, String> {
    let mut file = ConfigFile::default();
    if config_path.exists() {
        match ConfigFile::read(config_path).and_then(|f| f.resolve().map(|c| (f, c))) {
            Ok((existing, c)) => {
                println!("Current config ({}):", config_path.display());
                println!("  source:  {}", c.source.display());
                for t in &c.targets {
                    println!("  target:  {}", t.display());
                }
                if pick::confirm("Keep these folders?", true)? {
                    return Ok(existing);
                }
                file = existing;
            }
            Err(e) => println!("The existing config is invalid ({e}); choose the folders again."),
        }
    }
    let (source, targets) = pick::folders(&find::skills_folders())?;
    file.source = config::contract(&source);
    file.targets = targets.iter().map(|t| config::contract(t)).collect();
    Ok(file)
}

/// Copies `from` to `to` through a temporary file and a rename, so a binary that is
/// still running is replaced rather than written over. Retries briefly while a
/// just-stopped process still holds `to` (Windows will not rename over a running exe).
fn copy_retrying(from: &Path, to: &Path) -> Result<(), String> {
    if normalize(from) == normalize(to) {
        return Ok(());
    }
    let tmp = to.with_extension("new");
    let mut attempt = 0;
    loop {
        match fs::copy(from, &tmp).and_then(|_| fs::rename(&tmp, to)) {
            Ok(()) => return Ok(()),
            Err(_) if attempt < 10 => {
                attempt += 1;
                thread::sleep(Duration::from_millis(200));
            }
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                return Err(format!("cannot copy to {}: {e}", to.display()));
            }
        }
    }
}
