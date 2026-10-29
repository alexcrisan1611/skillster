//! Asking which folder is the source and which are targets: arrow-key menus in a
//! terminal, typed answers when input is not a terminal (e.g. piped in).

use std::collections::HashSet;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{MAIN_SEPARATOR, Path, PathBuf};

use dialoguer::theme::ColorfulTheme;
use dialoguer::{Confirm, Input, MultiSelect, Select};

use crate::config;
use crate::find::Found;
use crate::platform::{link_target, normalize};

/// Rows a menu shows at once; it scrolls past that.
const MENU_ROWS: usize = 15;

fn interactive() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

pub fn confirm(question: &str, default: bool) -> Result<bool, String> {
    if interactive() {
        return Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(question)
            .default(default)
            .interact()
            .map_err(cancelled);
    }
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

/// The source and the targets, picked from `found` or typed as paths.
pub fn folders(found: &[Found]) -> Result<(PathBuf, Vec<PathBuf>), String> {
    if interactive() { menus(found) } else { typed(found) }
}

fn menus(found: &[Found]) -> Result<(PathBuf, Vec<PathBuf>), String> {
    let theme = ColorfulTheme::default();

    let mut items: Vec<String> = found.iter().map(label).collect();
    items.push("Another folder (type its path)".into());
    let pick = Select::with_theme(&theme)
        .with_prompt("Source, the folder that holds your skills (arrows to move, enter to pick)")
        .items(&items)
        .default(default_source(found).unwrap_or(0))
        .max_length(MENU_ROWS)
        .interact()
        .map_err(cancelled)?;
    let source = match found.get(pick) {
        Some(f) => f.path.clone(),
        None => type_path(&theme, "Source folder", true)?.ok_or("no source given")?,
    };

    let candidates: Vec<&Found> = found.iter().filter(|f| normalize(&f.path) != normalize(&source)).collect();
    // Folders that already link to the source are most likely wanted again.
    let checked: Vec<bool> = candidates.iter().map(|f| links_into(&f.path, &source)).collect();
    loop {
        let picked = MultiSelect::with_theme(&theme)
            .with_prompt("Targets, the folders that get links to those skills (space to tick, enter when done)")
            .items(candidates.iter().map(|f| label(f)))
            .defaults(&checked)
            .max_length(MENU_ROWS)
            .interact()
            .map_err(cancelled)?;
        let mut targets: Vec<PathBuf> = picked.into_iter().map(|i| candidates[i].path.clone()).collect();
        while let Some(extra) = type_path(&theme, "Another target folder, or enter to finish", false)? {
            if normalize(&extra) == normalize(&source) {
                println!("The source cannot also be a target.");
            } else {
                targets.push(extra);
            }
        }
        let mut seen = HashSet::new();
        targets.retain(|t| seen.insert(normalize(t)));
        if !targets.is_empty() {
            return Ok((source, targets));
        }
        println!("Pick at least one target.");
    }
}

/// A typed path, checked as it is typed; `None` if left empty (only when not `required`).
fn type_path(theme: &ColorfulTheme, prompt: &str, required: bool) -> Result<Option<PathBuf>, String> {
    let answer: String = Input::with_theme(theme)
        .with_prompt(prompt)
        .allow_empty(!required)
        .validate_with(|s: &String| -> Result<(), String> {
            if s.trim().is_empty() {
                return Ok(());
            }
            let p = config::expand(unquote(s))?;
            if required && !p.is_dir() { Err(format!("{} is not a folder", p.display())) } else { Ok(()) }
        })
        .interact_text()
        .map_err(cancelled)?;
    if answer.trim().is_empty() { Ok(None) } else { config::expand(unquote(&answer)).map(Some) }
}

/// The same questions as plain numbered prompts.
fn typed(found: &[Found]) -> Result<(PathBuf, Vec<PathBuf>), String> {
    if found.is_empty() {
        println!("None found; type paths instead.");
    }
    for (i, f) in found.iter().enumerate() {
        println!("  {:>3}. {}", i + 1, label(f));
    }
    println!("\nAnswer with list numbers or full paths (comma-separated where several are allowed).");
    let paths: Vec<PathBuf> = found.iter().map(|f| f.path.clone()).collect();
    let default = default_source(found);

    let source = loop {
        let hint = default.map(|i| format!(" [{}]", i + 1)).unwrap_or_default();
        let answer = ask(&format!("\nSource, the folder that holds your skills{hint}: "))?;
        let picked = if answer.is_empty() {
            default.map(|i| vec![paths[i].clone()]).ok_or_else(|| "pick a folder".to_string())
        } else {
            parse_choices(&answer, &paths)
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
        match parse_choices(&answer, &paths) {
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

/// `~/.agents/skills` if listed, else the folder with the most skills.
fn default_source(found: &[Found]) -> Option<usize> {
    let agents = config::expand("~/.agents/skills").ok()?;
    found
        .iter()
        .position(|f| normalize(&f.path) == normalize(&agents))
        .or_else(|| (0..found.len()).filter(|&i| found[i].skills > 0).max_by_key(|&i| found[i].skills))
}

fn label(f: &Found) -> String {
    let plural = if f.skills == 1 { "" } else { "s" };
    format!("{}  ({} skill{plural})", config::contract(&f.path), f.skills)
}

/// True if `dir` holds a link pointing into `source`.
fn links_into(dir: &Path, source: &Path) -> bool {
    let source = normalize(source);
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    entries.flatten().any(|e| {
        link_target(&e.path()).is_some_and(|t| {
            normalize(&t).strip_prefix(&source).is_some_and(|rest| rest.starts_with(MAIN_SEPARATOR))
        })
    })
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
            out.push(config::expand(unquote(piece))?);
        }
    }
    let mut seen = HashSet::new();
    out.retain(|p| seen.insert(normalize(p)));
    Ok(out)
}

fn unquote(s: &str) -> &str {
    s.trim().trim_matches(['"', '\''])
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

fn cancelled(e: dialoguer::Error) -> String {
    format!("cancelled ({e})")
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
