//! Finding the `skills` folders worth offering in the installer.
//!
//! Searching turns up hundreds of folders named `skills`; most are copies some tool
//! manages (plugin caches, editor extensions, Python packages, temp files, worktrees,
//! per-project copies) or code that happens to use the name. What is worth offering:
//! - a tool's own skills folder: near the top of a tool's home folder (`~/.claude/skills`,
//!   `~/.gemini/antigravity/skills`) or of an app's data folder (`%APPDATA%\x\skills`,
//!   `~/.config/x/skills`), even when empty;
//! - elsewhere, a standalone collection that holds skills (`~/Projects/my-skills/skills`),
//!   but not a project's own `.claude/skills`.

use std::fs;
use std::path::{MAIN_SEPARATOR, Path, PathBuf};

use crate::platform::{self, normalize};

/// A `skills` folder worth offering, and how many skills it holds.
pub struct Found {
    pub path: PathBuf,
    pub skills: usize,
    /// A tool's own folder, as opposed to a standalone collection.
    pub tool: bool,
}

/// Folder names (lowercase) that mark something managed, generated or thrown away.
const JUNK_NAMES: &[&str] = &[
    "node_modules", ".git", ".hg", ".svn", "site-packages", "venv", ".venv", "__pycache__", "vendor", "dist",
    "build", "target", "packages", "bundled", "builtin", "worktrees", "scratch", "scratchpad", "sessions", "temp",
    "tmp", "$recycle.bin", ".trash", "windowsapps", "programs", "program files", "program files (x86)", "windows",
    "programdata", "rpm",
];
/// Parts of folder names (lowercase) with the same meaning: `plugins`, `marketplace-cache`,
/// `Claude Extensions`, `Claude-Backup-Data`, `local-agent-mode-sessions`...
const JUNK_PARTS: &[&str] = &["plugin", "extension", "cache", "marketplace", "backup", "-sessions"];
/// Inside a tool's home folder, `projects` holds copies of other repositories.
const JUNK_IN_TOOL_HOME: &[&str] = &["projects"];

/// How deep below the home folder the fallback walk looks.
const MAX_WALK_DEPTH: usize = 12;
/// How many folders below a tool's home its skills folder may be (`~/.x/skills` is 1).
/// An empty one counts only right at the top.
const MAX_TOOL_DEPTH: usize = 3;
const MAX_TOOL_DEPTH_EMPTY: usize = 1;

/// The `skills` folders worth offering: tools' own folders first, then collections.
/// On Windows this asks Everything, falling back to walking the home folder;
/// elsewhere it walks the home folder.
pub fn skills_folders() -> Vec<Found> {
    let places = Places::here();
    #[cfg(windows)]
    {
        println!("Searching for skills folders with Everything...");
        match crate::everything::skills_folders() {
            Ok(raw) => return places.offer(raw),
            Err(e) => println!("{e} Searching your home folder instead, which is slower..."),
        }
    }
    #[cfg(not(windows))]
    println!("Searching your home folder for skills folders...");
    let mut raw = Vec::new();
    walk(&places.home, 0, &mut raw);
    places.offer(raw)
}

/// Subfolders of `dir` that contain a `SKILL.md`.
pub fn skill_count(dir: &Path) -> usize {
    fs::read_dir(dir)
        .map(|entries| entries.flatten().filter(|e| e.path().join("SKILL.md").is_file()).count())
        .unwrap_or(0)
}

/// Collects `skills` folders under `dir`, without following links, entering junk,
/// or entering a match.
fn walk(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        // `file_type` does not follow links, so junctions and symlinks are not entered.
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name == "skills" {
            found.push(entry.path());
        } else if depth < MAX_WALK_DEPTH && !is_junk(&name) {
            walk(&entry.path(), depth + 1, found);
        }
    }
}

fn is_junk(name: &str) -> bool {
    JUNK_NAMES.contains(&name) || JUNK_PARTS.iter().any(|p| name.contains(p))
}

struct Places {
    home: PathBuf,
    app_roots: Vec<PathBuf>,
}

impl Places {
    fn here() -> Self {
        Self { home: platform::home(), app_roots: platform::app_data_roots() }
    }

    /// Filters `raw` to the folders worth offering, sorted, without duplicates or
    /// folders nested inside another one offered.
    fn offer(&self, raw: Vec<PathBuf>) -> Vec<Found> {
        let mut found: Vec<Found> = raw
            .into_iter()
            .filter(|p| p.is_dir())
            .filter_map(|path| {
                let skills = skill_count(&path);
                let tool = self.judge(&path, skills)?;
                Some(Found { path, skills, tool })
            })
            .collect();
        found.sort_by_key(|f| (!f.tool, normalize(&f.path)));
        found.dedup_by_key(|f| normalize(&f.path));
        let keys: Vec<String> = found.iter().map(|f| normalize(&f.path)).collect();
        found.retain(|f| !keys.iter().any(|k| below(&f.path, k).is_some()));
        found
    }

    /// `Some(true)` for a tool's own skills folder, `Some(false)` for a standalone
    /// collection, `None` if not worth offering.
    fn judge(&self, path: &Path, skills: usize) -> Option<bool> {
        let parts = below(path, "").unwrap_or_default();
        if parts.iter().any(|c| is_junk(c)) {
            return None;
        }
        if let Some(inside) = self.inside_tool_home(path) {
            if inside.iter().any(|c| JUNK_IN_TOOL_HOME.contains(&c.as_str())) {
                return None;
            }
            let limit = if skills > 0 { MAX_TOOL_DEPTH } else { MAX_TOOL_DEPTH_EMPTY };
            return (inside.len() <= limit).then_some(true);
        }
        // Anything under a dot-folder here is a project's own copy (`app/.claude/skills`).
        let in_dot_folder = parts.iter().any(|c| c.starts_with('.'));
        (skills > 0 && !in_dot_folder).then_some(false)
    }

    /// The folders from a tool's home folder down to `path`, if `path` is in one.
    /// A tool's home is a folder in an app-data root, or a dot-folder in the home folder.
    fn inside_tool_home(&self, path: &Path) -> Option<Vec<String>> {
        let tool_home = |rest: Vec<String>| (rest.len() >= 2).then(|| rest[1..].to_vec());
        for root in &self.app_roots {
            if let Some(rest) = below(path, &normalize(root)) {
                return tool_home(rest);
            }
        }
        let rest = below(path, &normalize(&self.home))?;
        if rest.first()?.starts_with('.') { tool_home(rest) } else { None }
    }
}

/// The (lowercase) folder names of `path` below the normalized folder `dir`, if inside it.
/// With an empty `dir`, all of them.
fn below(path: &Path, dir: &str) -> Option<Vec<String>> {
    let path = normalize(path);
    let rest = if dir.is_empty() { path.as_str() } else { path.strip_prefix(dir)?.strip_prefix(MAIN_SEPARATOR)? };
    Some(rest.split(MAIN_SEPARATOR).filter(|c| !c.is_empty()).map(str::to_lowercase).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(root: &Path, rel: &str) -> PathBuf {
        rel.split('/').fold(root.to_path_buf(), |p, c| p.join(c))
    }

    #[test]
    fn judges_folders() {
        // `judge` reads no files, so the home folder need not exist (and must not be
        // under the temp folder, which is junk).
        let home = PathBuf::from(if cfg!(windows) { r"C:\Users\someone" } else { "/home/someone" });
        let places = Places { home: home.clone(), app_roots: vec![at(&home, "AppData/Roaming"), at(&home, ".config")] };
        let cases: &[(&str, usize, Option<bool>)] = &[
            (".claude/skills", 0, Some(true)),
            (".agents/skills", 40, Some(true)),
            (".gemini/antigravity/skills", 14, Some(true)),
            (".gemini/config/skills", 0, None),
            ("AppData/Roaming/hermes/hermes-agent/skills", 0, None),
            (".config/opencode/skills", 0, Some(true)),
            ("AppData/Roaming/skillshare/skills", 0, Some(true)),
            ("AppData/Roaming/hermes/agent/tests/skills", 0, None),
            (".codex/plugins/cache/x/1.0/skills", 3, None),
            (".config/opencode/postman-plugin/skills", 16, None),
            (".grok/marketplace-cache/abc/plugins/hookify/skills", 1, None),
            (".grok/bundled/skills", 22, None),
            (".vscode/extensions/gitlens/.claude/skills", 14, None),
            (".t3/projects/app/.claude/skills", 21, None),
            (".t3/worktrees/app/t3-1/.agents/skills", 19, None),
            ("AppData/Roaming/uv/tools/x/Lib/site-packages/y/skills", 1, None),
            ("AppData/Local/Temp/x/skills", 6, None),
            ("AppData/Local/Claude-Backup-Data/x/skills", 13, None),
            ("Projects/my-skills/skills", 10, Some(false)),
            ("foundations/skills", 8, Some(false)),
            ("Projects/app/.claude/skills", 52, None),
            ("Projects/app/.claude/skills/x/container/skills", 1, None),
            ("Projects/app/src/skills", 0, None),
        ];
        for &(rel, skills, expected) in cases {
            assert_eq!(places.judge(&at(&home, rel), skills), expected, "{rel}");
        }
    }

    #[test]
    fn walks_without_entering_junk_or_matches() {
        let root = std::env::temp_dir().join(format!("skillster-find-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for dir in ["a/skills/one", "a/skills/one/skills", "b/c/skills", "b/node_modules/x/skills", "d"] {
            fs::create_dir_all(at(&root, dir)).unwrap();
        }
        fs::write(at(&root, "a/skills/one/SKILL.md"), "x").unwrap();
        let mut found = Vec::new();
        walk(&root, 0, &mut found);
        found.sort();
        assert_eq!(found, vec![at(&root, "a/skills"), at(&root, "b/c/skills")]);
        assert_eq!(skill_count(&at(&root, "a/skills")), 1);
        fs::remove_dir_all(&root).unwrap();
    }
}
