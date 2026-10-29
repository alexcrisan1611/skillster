//! `skills-sync install` and `skills-sync uninstall`.

use std::collections::HashSet;
use std::fs;

use std::path::Path;
use std::thread;
use std::time::Duration;

use crate::config::{self, ConfigFile};
use crate::{find, pick};
use crate::logger::Logger;
use crate::platform::{self, CLI_EXE, DAEMON_EXE, normalize};
use crate::sync::sync;

/// Asks for the folders, writes the config, copies both binaries into
/// `platform::install_dir()`, registers the background process to start at login,
/// and starts it.
pub fn install(config_path: &Path) -> Result<(), String> {
    let here = std::env::current_exe().map_err(|e| format!("cannot locate this program: {e}"))?;
    let daemon_src = here.with_file_name(DAEMON_EXE);
    if !daemon_src.is_file() {
        return Err(format!("{DAEMON_EXE} must be next to {}", here.display()));
    }

    let file = choose_settings(config_path)?;
    let config = file.resolve()?;
    file.write(config_path)?;
    println!("\nSaved {}", config_path.display());

    let dir = platform::install_dir();
    platform::stop_daemon();
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    copy_retrying(&here, &dir.join(CLI_EXE))?;
    copy_retrying(&daemon_src, &dir.join(DAEMON_EXE))?;

    println!("\nSyncing now:");
    sync(&config, &Logger::new(config.log_path.clone(), config.log_max_bytes, true), &mut HashSet::new());

    let custom_config = (normalize(config_path) != normalize(&config::default_path())).then_some(config_path);
    let autostart = platform::enable_autostart(&dir.join(DAEMON_EXE), custom_config)?;

    println!("\nInstalled to {}. It runs in the background and starts at login ({autostart}).", dir.display());
    println!("Edit {} to change folders; changes apply straight away.", config_path.display());
    println!("Log: {}", config.log_path.display());
    let on_path = std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| normalize(&d) == normalize(&dir)));
    if !on_path {
        println!("To run `skills-sync` by name, add {} to your PATH.", dir.display());
    }
    Ok(())
}

/// Stops the background process, removes the login entry and the installed binaries.
/// The config and the links already made are left in place.
pub fn uninstall(config_path: &Path) -> Result<(), String> {
    platform::stop_daemon();
    platform::disable_autostart()?;
    let dir = platform::install_dir();
    platform::remove_binaries(&dir)?;
    println!("Uninstalled from {}.", dir.display());
    println!("Kept the config ({}) and the links already made.", config_path.display());
    Ok(())
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
