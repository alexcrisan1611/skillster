//! `skills-sync install` and `skills-sync uninstall`.

use std::collections::HashSet;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use crate::config::{self, ConfigFile};
use crate::find;
use crate::logger::Logger;
use crate::platform::{self, CLI_EXE, DAEMON_EXE, normalize};
use crate::sync::sync;

/// Asks for the folders, writes the config, copies both binaries into
/// `platform::install_dir()`, registers the background process to start at login,
/// and starts it.
pub fn install(config_path: &Path, es: Option<&Path>) -> Result<(), String> {
    let here = std::env::current_exe().map_err(|e| format!("cannot locate this program: {e}"))?;
    let daemon_src = here.with_file_name(DAEMON_EXE);
    if !daemon_src.is_file() {
        return Err(format!("{DAEMON_EXE} must be next to {}", here.display()));
    }

    let file = choose_settings(config_path, es)?;
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
fn choose_settings(config_path: &Path, es: Option<&Path>) -> Result<ConfigFile, String> {
    let mut file = ConfigFile::default();
    if config_path.exists() {
        match ConfigFile::read(config_path).and_then(|f| f.resolve().map(|c| (f, c))) {
            Ok((existing, c)) => {
                println!("Current config ({}):", config_path.display());
                println!("  source:  {}", c.source.display());
                for t in &c.targets {
                    println!("  target:  {}", t.display());
                }
                if ask_yes_no("Keep these folders?", true)? {
                    return Ok(existing);
                }
                file = existing;
            }
            Err(e) => println!("The existing config is invalid ({e}); choose the folders again."),
        }
    }
    let (source, targets) = choose_folders(es)?;
    file.source = config::contract(&source);
    file.targets = targets.iter().map(|t| config::contract(t)).collect();
    Ok(file)
}

/// Lists every `skills` folder found and asks for a source and targets.
fn choose_folders(es: Option<&Path>) -> Result<(PathBuf, Vec<PathBuf>), String> {
    let found = find::skills_folders(es);
    if found.is_empty() {
        println!("None found; type paths instead.");
    }
    let counts: Vec<usize> = found.iter().map(|p| find::skill_count(p)).collect();
    for (i, (p, n)) in found.iter().zip(&counts).enumerate() {
        let plural = if *n == 1 { "" } else { "s" };
        println!("  {:>3}. {}  ({n} skill{plural})", i + 1, p.display());
    }
    println!("\nAnswer with list numbers or full paths (comma-separated where several are allowed).");

    // Default source: ~/.agents/skills if listed, else the folder with the most skills.
    let agents = config::expand("~/.agents/skills")?;
    let default = found
        .iter()
        .position(|p| normalize(p) == normalize(&agents))
        .or_else(|| (0..found.len()).filter(|&i| counts[i] > 0).max_by_key(|&i| counts[i]));

    let source = loop {
        let hint = default.map(|i| format!(" [{}]", i + 1)).unwrap_or_default();
        let answer = ask(&format!("\nSource, the folder that holds your skills{hint}: "))?;
        let picked = if answer.is_empty() {
            default.map(|i| vec![found[i].clone()]).ok_or_else(|| "pick a folder".to_string())
        } else {
            parse_choices(&answer, &found)
        };
        match picked {
            Ok(p) if p.len() == 1 && p[0].is_dir() => break p.into_iter().next().unwrap(),
            Ok(p) if p.len() == 1 => println!("{} is not a folder", p[0].display()),
            Ok(_) => println!("pick exactly one folder"),
            Err(e) => println!("{e}"),
        }
    };

    let targets = loop {
        let answer = ask("Targets, the folders that should get links to those skills: ")?;
        match parse_choices(&answer, &found) {
            Ok(t) if t.iter().any(|t| normalize(t) == normalize(&source)) => {
                println!("the source cannot also be a target")
            }
            Ok(t) if !t.is_empty() => break t,
            Ok(_) => println!("pick at least one folder"),
            Err(e) => println!("{e}"),
        }
    };
    Ok((source, targets))
}

/// `"1, 3 4, ~/x/skills"` → the 1st, 3rd and 4th listed folders and the typed path.
fn parse_choices(answer: &str, found: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut out: Vec<PathBuf> = Vec::new();
    for piece in answer.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        if piece.chars().all(|c| c.is_ascii_digit() || c.is_whitespace()) {
            for n in piece.split_whitespace() {
                let i: usize = n.parse().map_err(|_| format!("{n} is not a number"))?;
                let p = i.checked_sub(1).and_then(|i| found.get(i)).ok_or_else(|| format!("there is no folder {i}"))?;
                out.push(p.clone());
            }
        } else {
            out.push(config::expand(piece.trim_matches(['"', '\'']))?);
        }
    }
    let mut seen = HashSet::new();
    out.retain(|p| seen.insert(normalize(p)));
    Ok(out)
}

fn ask(prompt: &str) -> Result<String, String> {
    print!("{prompt}");
    let _ = io::stdout().flush();
    let mut line = String::new();
    match io::stdin().lock().read_line(&mut line) {
        Ok(0) => Err("cancelled".into()),
        Ok(_) => Ok(line.trim().to_string()),
        Err(e) => Err(format!("cannot read input: {e}")),
    }
}

fn ask_yes_no(question: &str, default: bool) -> Result<bool, String> {
    let hint = if default { "[Y/n]" } else { "[y/N]" };
    loop {
        match ask(&format!("{question} {hint} "))?.to_lowercase().as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => {}
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// An absolute path on this OS, from a `/`-separated relative one.
    fn abs(rel: &str) -> PathBuf {
        if cfg!(windows) { PathBuf::from(format!(r"C:\{}", rel.replace('/', r"\"))) } else { PathBuf::from(format!("/{rel}")) }
    }

    #[test]
    fn parses_numbers_and_paths() {
        let found = vec![abs("a/skills"), abs("b/skills"), abs("c/skills")];
        let typed = abs("x y/skills");
        let got = parse_choices(&format!("1 3, \"{}\", 3", typed.display()), &found).unwrap();
        assert_eq!(got, vec![found[0].clone(), found[2].clone(), typed]);
        assert!(parse_choices("4", &found).is_err());
        assert!(parse_choices("0", &found).is_err());
        assert!(parse_choices("", &found).unwrap().is_empty());
    }
}
