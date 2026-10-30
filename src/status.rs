//! What each target folder holds for each skill right now, and putting it right:
//! the window's view of the same work `sync` does in the background.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::Config;
use crate::platform::{create_link, link_target, normalize, remove_link};

/// A skill a target folder does not get from the source.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub skill: String,
    /// The target folder.
    pub target: PathBuf,
    pub kind: Kind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// Nothing there yet; the background process links it on its next pass.
    Missing,
    /// A copy with the same files as the source: safe to replace with a link.
    Same,
    /// A copy whose files differ from the source's.
    Differs(Diff),
    /// A link to somewhere other than the source (possibly to nothing).
    Elsewhere(PathBuf),
}

impl Kind {
    /// Whether replacing it with a link loses nothing that is not backed up or
    /// already in the source.
    pub fn safe(&self) -> bool {
        !matches!(self, Kind::Differs(_))
    }
}

/// How a copy differs from the source, by file paths relative to the skill folder.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diff {
    pub changed: Vec<String>,
    pub only_here: Vec<String>,
    pub only_in_source: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub path: PathBuf,
    /// Skills linked to the source.
    pub linked: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub source: PathBuf,
    pub skills: Vec<String>,
    pub targets: Vec<Target>,
    pub problems: Vec<Problem>,
    /// Set when the source cannot be read; nothing else is filled in then.
    pub error: Option<String>,
}

impl Report {
    pub fn links(&self) -> usize {
        self.targets.iter().map(|t| t.linked).sum()
    }
}

pub fn scan(config: &Config) -> Report {
    let mut report = Report { source: config.source.clone(), ..Report::default() };
    match source_skills(&config.source) {
        Ok(s) => report.skills = s,
        Err(e) => {
            report.error = Some(format!("cannot read {}: {e}", config.source.display()));
            return report;
        }
    }
    for target in &config.targets {
        let mut linked = 0;
        for skill in &report.skills {
            let src = config.source.join(skill);
            let dest = target.join(skill);
            let kind = match link_target(&dest) {
                Some(t) if normalize(&t) == normalize(&src) => {
                    linked += 1;
                    continue;
                }
                Some(t) => Kind::Elsewhere(t),
                None if fs::symlink_metadata(&dest).is_err() => Kind::Missing,
                None => match compare(&src, &dest) {
                    Ok(d) if d == Diff::default() => Kind::Same,
                    Ok(d) => Kind::Differs(d),
                    Err(_) => Kind::Differs(Diff::default()),
                },
            };
            report.problems.push(Problem { skill: skill.clone(), target: target.clone(), kind });
        }
        report.targets.push(Target { path: target.clone(), linked });
    }
    report
}

/// Skill folders in the source, as `sync` sees them.
fn source_skills(source: &Path) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with('.') && entry.path().is_dir() {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

fn compare(source: &Path, copy: &Path) -> io::Result<Diff> {
    let (a, b) = (files(source)?, files(copy)?);
    let mut diff = Diff::default();
    for (rel, path) in &b {
        match a.get(rel) {
            None => diff.only_here.push(rel.clone()),
            Some(src) if !same_bytes(src, path) => diff.changed.push(rel.clone()),
            Some(_) => {}
        }
    }
    diff.only_in_source = a.keys().filter(|k| !b.contains_key(*k)).cloned().collect();
    Ok(diff)
}

/// Every file under `root`, keyed by its path relative to it with `/` separators.
fn files(root: &Path) -> io::Result<BTreeMap<String, PathBuf>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        if fs::metadata(&dir)?.is_file() {
            out.insert(prefix, dir);
            continue;
        }
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let rel = format!("{prefix}{}", entry.file_name().to_string_lossy());
            if entry.path().is_dir() {
                stack.push((entry.path(), rel + "/"));
            } else {
                out.insert(rel, entry.path());
            }
        }
    }
    Ok(out)
}

fn same_bytes(a: &Path, b: &Path) -> bool {
    let size = |p: &Path| fs::metadata(p).map(|m| m.len()).ok();
    size(a) == size(b) && matches!((fs::read(a), fs::read(b)), (Ok(x), Ok(y)) if x == y)
}

/// A unified diff of one file of a copy against the source, or `None` when either
/// side is not reasonably sized text.
pub fn file_diff(source: &Path, copy: &Path, rel: &str) -> Option<String> {
    const MAX: u64 = 256 * 1024;
    let read = |root: &Path| {
        let p = root.join(rel);
        if fs::metadata(&p).ok()?.len() > MAX {
            return None;
        }
        fs::read_to_string(p).ok()
    };
    let (a, b) = (read(source)?, read(copy)?);
    let diff = similar::TextDiff::from_lines(&a, &b);
    Some(diff.unified_diff().context_radius(2).header("source", "this copy").to_string())
}

/// Makes `problem.target` get the skill from the source: moves a copy into
/// `backup` (keeping it), replaces a link to elsewhere, then links. Returns what
/// was done, for the user.
pub fn fix(source: &Path, problem: &Problem, backup: &Path) -> Result<String, String> {
    let src = source.join(&problem.skill);
    let dest = problem.target.join(&problem.skill);
    let shown = dest.display();
    let done = match &problem.kind {
        Kind::Missing => format!("Linked {}", problem.skill),
        Kind::Elsewhere(_) => {
            remove_link(&dest).map_err(|e| format!("cannot remove the link {shown}: {e}"))?;
            format!("Relinked {}", problem.skill)
        }
        Kind::Same | Kind::Differs(_) => {
            let to = backup_path(backup, &problem.target, &problem.skill);
            if let Some(dir) = to.parent() {
                fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
            }
            fs::rename(&dest, &to).map_err(|e| format!("cannot move {shown} to {}: {e}", to.display()))?;
            format!("Replaced {} with a link; the copy is in {}", problem.skill, to.display())
        }
    };
    match create_link(&src, &dest) {
        Ok(()) => Ok(done),
        // The background process may have linked it in the meantime.
        Err(_) if link_target(&dest).is_some_and(|t| normalize(&t) == normalize(&src)) => Ok(done),
        Err(e) => Err(format!("cannot link {shown}: {e}")),
    }
}

/// `<backup>/<unix seconds>/<target, as one folder name>/<skill>`, so nothing
/// backed up is ever overwritten.
fn backup_path(backup: &Path, target: &Path, skill: &str) -> PathBuf {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let flat: String = crate::config::contract(target)
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect();
    let base = backup.join(secs.to_string()).join(flat.trim_matches('_')).join(skill);
    let mut path = base.clone();
    let mut n = 1;
    while fs::symlink_metadata(&path).is_ok() {
        n += 1;
        path = base.with_file_name(format!("{skill}-{n}"));
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigFile;

    #[test]
    fn finds_and_fixes_every_kind() {
        let root = std::env::temp_dir().join(format!("skillster-status-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (source, t, other, backup) = (root.join("source"), root.join("t"), root.join("other"), root.join("backup"));
        for skill in ["ok", "missing", "same", "edited", "elsewhere"] {
            fs::create_dir_all(source.join(skill).join("refs")).unwrap();
            fs::write(source.join(skill).join("SKILL.md"), format!("# {skill}\nline\n")).unwrap();
            fs::write(source.join(skill).join("refs").join("a.md"), "a").unwrap();
        }
        fs::create_dir_all(&t).unwrap();
        fs::create_dir_all(other.join("elsewhere")).unwrap();
        create_link(&source.join("ok"), &t.join("ok")).unwrap();
        create_link(&other.join("elsewhere"), &t.join("elsewhere")).unwrap();
        copy_dir(&source.join("same"), &t.join("same"));
        copy_dir(&source.join("edited"), &t.join("edited"));
        fs::write(t.join("edited").join("SKILL.md"), "# edited\nmine\n").unwrap();
        fs::write(t.join("edited").join("extra.md"), "x").unwrap();
        fs::remove_file(t.join("edited").join("refs").join("a.md")).unwrap();

        let file = ConfigFile {
            source: source.to_string_lossy().into_owned(),
            targets: vec![t.to_string_lossy().into_owned()],
            ..Default::default()
        };
        let report = scan(&file.resolve().unwrap());
        assert_eq!(report.links(), 1);
        let kind = |skill: &str| report.problems.iter().find(|p| p.skill == skill).map(|p| p.kind.clone());
        assert_eq!(kind("ok"), None);
        assert_eq!(kind("missing"), Some(Kind::Missing));
        assert_eq!(kind("same"), Some(Kind::Same));
        assert!(matches!(kind("elsewhere"), Some(Kind::Elsewhere(_))));
        let Some(Kind::Differs(d)) = kind("edited") else { panic!("{:?}", kind("edited")) };
        assert_eq!(d.changed, ["SKILL.md"]);
        assert_eq!(d.only_here, ["extra.md"]);
        assert_eq!(d.only_in_source, ["refs/a.md"]);
        let text = file_diff(&source.join("edited"), &t.join("edited"), "SKILL.md").unwrap();
        assert!(text.contains("-line") && text.contains("+mine"), "{text}");

        for p in &report.problems {
            fix(&source, p, &backup).unwrap();
        }
        let file = ConfigFile {
            source: source.to_string_lossy().into_owned(),
            targets: vec![t.to_string_lossy().into_owned()],
            ..Default::default()
        };
        let after = scan(&file.resolve().unwrap());
        assert!(after.problems.is_empty(), "{:?}", after.problems);
        assert_eq!(after.links(), 5);
        assert!(other.join("elsewhere").is_dir(), "the old link's target is left alone");
        let kept: Vec<_> = files(&backup).unwrap().into_keys().collect();
        assert!(kept.iter().any(|k| k.ends_with("/edited/extra.md")), "{kept:?}");
        assert!(kept.iter().any(|k| k.ends_with("/same/SKILL.md")), "{kept:?}");

        fs::remove_dir_all(&root).unwrap();
    }

    fn copy_dir(from: &Path, to: &Path) {
        for (rel, path) in files(from).unwrap() {
            let dest = to.join(rel);
            fs::create_dir_all(dest.parent().unwrap()).unwrap();
            fs::copy(path, dest).unwrap();
        }
    }
}
