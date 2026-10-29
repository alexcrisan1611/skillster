//! Finding the `skills` folders on this computer, for the installer to offer.

use std::fs;
use std::path::{Path, PathBuf};

use crate::platform::{self, normalize};

/// Folder names never worth looking inside: package caches, VCS data, trash.
const SKIP: &[&str] = &[
    "node_modules", ".git", ".hg", ".svn", "$recycle.bin", ".trash", ".cache", "caches", "__pycache__", ".venv",
];

/// How deep below the home folder the fallback walk looks.
const MAX_DEPTH: usize = 12;

/// Every `skills` folder worth offering, sorted, minus ones nested inside another.
/// On Windows this asks Everything (`es`, else found automatically) and falls back to
/// walking the home folder; elsewhere it walks the home folder.
pub fn skills_folders(es: Option<&Path>) -> Vec<PathBuf> {
    #[cfg(windows)]
    match crate::everything::find_es(es).and_then(|es| {
        println!("Searching for `skills` folders with Everything...");
        crate::everything::skills_folders(&es)
    }) {
        Ok(found) => return tidy(found),
        Err(e) => println!("{e}"),
    }
    #[cfg(not(windows))]
    let _ = es;

    let home = platform::home();
    println!("Searching {} for `skills` folders (this can take a minute)...", home.display());
    let mut found = Vec::new();
    walk(&home, 0, &mut found);
    tidy(found)
}

/// Subfolders of `dir` that contain a `SKILL.md`.
pub fn skill_count(dir: &Path) -> usize {
    fs::read_dir(dir)
        .map(|entries| entries.flatten().filter(|e| e.path().join("SKILL.md").is_file()).count())
        .unwrap_or(0)
}

/// Collects `skills` folders under `dir`, without following links or entering a match.
fn walk(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        // `file_type` does not follow links, so junctions and symlinks are not entered.
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "skills" {
            found.push(entry.path());
        } else if depth < MAX_DEPTH && !SKIP.contains(&name.to_lowercase().as_str()) {
            walk(&entry.path(), depth + 1, found);
        }
    }
}

fn tidy(found: Vec<PathBuf>) -> Vec<PathBuf> {
    let temp = normalize(&std::env::temp_dir());
    let mut found: Vec<PathBuf> = found
        .into_iter()
        .filter(|p| {
            let skipped = p.components().any(|c| SKIP.contains(&c.as_os_str().to_string_lossy().to_lowercase().as_str()));
            !skipped && !inside(&normalize(p), &temp) && p.is_dir()
        })
        .collect();
    found.sort_by_key(|p| normalize(p));
    found.dedup_by_key(|p| normalize(p));
    let keys: Vec<String> = found.iter().map(|p| normalize(p)).collect();
    found.retain(|p| {
        let n = normalize(p);
        !keys.iter().any(|k| inside(&n, k))
    });
    found
}

/// True if `path` is strictly below `dir` (both normalized).
fn inside(path: &str, dir: &str) -> bool {
    path.strip_prefix(dir).is_some_and(|rest| rest.starts_with(std::path::MAIN_SEPARATOR))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_and_tidies() {
        let root = std::env::temp_dir().join(format!("skills-sync-find-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for dir in ["a/skills/one", "a/skills/one/skills", "b/c/skills", "b/node_modules/x/skills", "d"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        fs::write(root.join("a/skills/one/SKILL.md"), "x").unwrap();
        let mut found = Vec::new();
        walk(&root, 0, &mut found);
        found.sort();
        assert_eq!(found, vec![root.join("a").join("skills"), root.join("b").join("c").join("skills")]);
        assert_eq!(skill_count(&root.join("a").join("skills")), 1);

        // Everything returns nested and junk folders too; `tidy` drops them.
        let raw = vec![
            PathBuf::from("/no/such/skills"),
            root.join("a").join("skills").join("one").join("skills"),
            root.join("a").join("skills"),
            root.join("b").join("node_modules").join("x").join("skills"),
        ];
        // `root` is under the temp folder, which `tidy` skips, so check the rules directly.
        assert!(inside(&normalize(&raw[1]), &normalize(&raw[2])));
        assert!(tidy(raw).is_empty());
        fs::remove_dir_all(&root).unwrap();
    }
}
