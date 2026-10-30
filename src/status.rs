//! What each target folder holds for each skill right now, and putting it right:
//! the window's view of the same work `sync` does in the background, plus the
//! skills some targets have that the source does not.

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::hash::{DefaultHasher, Hash as _, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

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
    /// A folder in some targets that the source lacks. The problem's target holds
    /// the copy that moves into the source: the one most copies match.
    Unshared {
        /// Other targets with an identical copy, linked once it has moved.
        same: Vec<PathBuf>,
        /// Targets with a different copy, left to review against the source after.
        differ: Vec<PathBuf>,
    },
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
    report.problems.extend(unshared(config, &report.skills));
    report
}

/// Skill folders that targets hold and the source lacks, one problem per skill.
/// Copies match when their SHA-256 over every file's path and bytes does. Links
/// are left alone, as `sync` leaves them.
fn unshared(config: &Config, skills: &[String]) -> Vec<Problem> {
    type Hash = Option<[u8; 32]>;
    let mut copies: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for target in &config.targets {
        let Ok(entries) = fs::read_dir(target) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if name.starts_with('.') || skills.contains(&name) || link_target(&path).is_some() || !path.is_dir() {
                continue;
            }
            copies.entry(name).or_default().push(target.clone());
        }
    }
    let mut problems = Vec::new();
    for (skill, copies) in copies {
        let mut groups: Vec<(Hash, Vec<PathBuf>)> = Vec::new();
        let alone = copies.len() == 1;
        for target in copies {
            // A copy with nothing to match needs no reading.
            let hash = if alone { Some([0; 32]) } else { digest(&target.join(&skill)).ok() };
            match groups.iter_mut().find(|g| hash.is_some() && g.0 == hash) {
                Some(g) => g.1.push(target),
                None => groups.push((hash, vec![target])),
            }
        }
        // The most copies win; a tie goes to the target listed first. A copy that
        // can't be read never moves.
        groups.sort_by_key(|g| (g.0.is_none(), Reverse(g.1.len())));
        if groups[0].0.is_none() {
            continue;
        }
        let mut same = groups.remove(0).1;
        let target = same.remove(0);
        let differ = groups.into_iter().flat_map(|g| g.1).collect();
        problems.push(Problem { skill, target, kind: Kind::Unshared { same, differ } });
    }
    problems
}

/// Digests by folder, with a stamp of every file's size and modification time:
/// the window rescans every few seconds, and reads again only what changed.
static DIGESTS: LazyLock<Mutex<HashMap<PathBuf, Stamped>>> = LazyLock::new(Mutex::default);
type Stamped = (u64, [u8; 32]);

fn digest(root: &Path) -> io::Result<[u8; 32]> {
    let files = files(root)?;
    let mut stamp = DefaultHasher::new();
    for (rel, path) in &files {
        let meta = fs::metadata(path)?;
        (rel, meta.len(), meta.modified().ok()).hash(&mut stamp);
    }
    let stamp = stamp.finish();
    let mut cache = DIGESTS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((s, d)) = cache.get(root)
        && *s == stamp
    {
        return Ok(*d);
    }
    let mut hash = Sha256::new();
    for (rel, path) in files {
        let bytes = fs::read(path)?;
        hash.update((rel.len() as u64).to_le_bytes());
        hash.update(rel.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(&bytes);
    }
    let d: [u8; 32] = hash.finalize().into();
    cache.insert(root.to_path_buf(), (stamp, d));
    Ok(d)
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
/// `backup` (keeping it), replaces a link to elsewhere, then links. A skill the
/// source lacks is moved into it first. Returns what was done, for the user.
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
            let to = back_up(backup, &problem.target, &problem.skill)?;
            format!("Replaced {} with a link; the copy is in {}", problem.skill, to.display())
        }
        Kind::Unshared { same, .. } => {
            if fs::symlink_metadata(&src).is_ok() {
                return Err(format!("{} is already in the source", problem.skill));
            }
            fs::rename(&dest, &src).map_err(|e| format!("cannot move {shown} to {}: {e}", src.display()))?;
            link(&src, &dest)?;
            for target in same {
                back_up(backup, target, &problem.skill)?;
                link(&src, &target.join(&problem.skill))?;
            }
            return Ok(format!("Moved {} into the source", problem.skill));
        }
    };
    link(&src, &dest)?;
    Ok(done)
}

/// Moves `target`'s copy of `skill` into `backup`, returning where it went.
fn back_up(backup: &Path, target: &Path, skill: &str) -> Result<PathBuf, String> {
    let from = target.join(skill);
    let to = backup_path(backup, target, skill);
    if let Some(dir) = to.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    fs::rename(&from, &to).map_err(|e| format!("cannot move {} to {}: {e}", from.display(), to.display()))?;
    Ok(to)
}

fn link(src: &Path, dest: &Path) -> Result<(), String> {
    match create_link(src, dest) {
        Ok(()) => Ok(()),
        // The background process may have linked it in the meantime.
        Err(_) if link_target(dest).is_some_and(|t| normalize(&t) == normalize(src)) => Ok(()),
        Err(e) => Err(format!("cannot link {}: {e}", dest.display())),
    }
}

/// A skill's `SKILL.md`, or `None` when there is none or it is not reasonably
/// sized text.
pub fn skill_text(dir: &Path) -> Option<String> {
    let p = dir.join("SKILL.md");
    if fs::metadata(&p).ok()?.len() > 256 * 1024 {
        return None;
    }
    fs::read_to_string(p).ok()
}

/// The `description` from a `SKILL.md`'s front matter, folded onto one line.
pub fn description(skill_md: &str) -> Option<String> {
    let mut lines = skill_md.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let front: Vec<&str> = lines.take_while(|l| l.trim() != "---").collect();
    let at = front.iter().position(|l| l.starts_with("description:"))?;
    let value = front[at]["description:".len()..].trim();
    let text = if value.is_empty() || value.starts_with('>') || value.starts_with('|') {
        // A block: the indented lines that follow.
        let block: Vec<&str> =
            front[at + 1..].iter().take_while(|l| l.starts_with([' ', '\t'])).map(|l| l.trim()).collect();
        block.join(" ")
    } else {
        value.trim_matches(|c| c == '"' || c == '\'').to_string()
    };
    (!text.is_empty()).then_some(text)
}

/// Removes a skill from the source, so every tool loses it: moves it into
/// `backup` (keeping it) and removes the links to it.
pub fn remove(config: &Config, skill: &str, backup: &Path) -> Result<String, String> {
    let src = config.source.join(skill);
    let to = back_up(backup, &config.source, skill)?;
    for target in &config.targets {
        let dest = target.join(skill);
        if link_target(&dest).is_some_and(|t| normalize(&t) == normalize(&src)) {
            remove_link(&dest).map_err(|e| format!("cannot remove the link {}: {e}", dest.display()))?;
        }
    }
    Ok(format!("Deleted {skill}; it is in {}", to.display()))
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

    #[test]
    fn moves_the_most_shared_copy_into_the_source() {
        let root = std::env::temp_dir().join(format!("skillster-unshared-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (source, backup) = (root.join("source"), root.join("backup"));
        let targets: Vec<PathBuf> = ["a", "b", "c", "d"].iter().map(|t| root.join(t)).collect();
        fs::create_dir_all(&source).unwrap();
        let skill = |target: &Path, name: &str, text: &str| {
            fs::create_dir_all(target.join(name)).unwrap();
            fs::write(target.join(name).join("SKILL.md"), text).unwrap();
        };
        // "shared": a's copy is edited, b, c and d agree. "lone": only in d.
        skill(&targets[0], "shared", "mine");
        for t in &targets[1..] {
            skill(t, "shared", "theirs");
        }
        skill(&targets[3], "lone", "x");
        create_link(&targets[3].join("lone"), &targets[2].join("elsewhere")).unwrap();

        let file = ConfigFile {
            source: source.to_string_lossy().into_owned(),
            targets: targets.iter().map(|t| t.to_string_lossy().into_owned()).collect(),
            ..Default::default()
        };
        let config = file.resolve().unwrap();
        let report = scan(&config);
        let shared = report.problems.iter().find(|p| p.skill == "shared").unwrap();
        assert_eq!(normalize(&shared.target), normalize(&targets[1]));
        let Kind::Unshared { same, differ } = &shared.kind else { panic!("{:?}", shared.kind) };
        assert_eq!(same.len(), 2);
        assert_eq!(differ.len(), 1);
        assert!(report.problems.iter().any(|p| p.skill == "lone"));
        assert!(!report.problems.iter().any(|p| p.skill == "elsewhere"), "links are left alone");

        for p in &report.problems {
            fix(&source, p, &backup).unwrap();
        }
        assert_eq!(fs::read_to_string(source.join("shared").join("SKILL.md")).unwrap(), "theirs");
        assert!(source.join("lone").is_dir());
        let after = scan(&config);
        let left: Vec<_> = after.problems.iter().map(|p| (p.skill.as_str(), p.kind.clone())).collect();
        // The edited copy is now just an edited copy of the source; the rest is
        // missing links the background process makes.
        assert!(left.iter().any(|(s, k)| *s == "shared" && matches!(k, Kind::Differs(_))), "{left:?}");
        assert!(left.iter().all(|(_, k)| matches!(k, Kind::Differs(_) | Kind::Missing)), "{left:?}");
        assert!(files(&backup).unwrap().len() == 2, "the identical copies are kept");

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn reads_descriptions() {
        assert_eq!(description("---
name: a
description: \"Does a thing.\"
---
# A").as_deref(), Some("Does a thing."));
        assert_eq!(description("---
description: >
  Folded
  text.
name: a
---").as_deref(), Some("Folded text."));
        assert_eq!(description("# No front matter"), None);
    }

    #[test]
    fn removes_a_skill_and_its_links() {
        let root = std::env::temp_dir().join(format!("skillster-remove-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (source, t, backup) = (root.join("source"), root.join("t"), root.join("backup"));
        fs::create_dir_all(source.join("gone")).unwrap();
        fs::write(source.join("gone").join("SKILL.md"), "x").unwrap();
        fs::create_dir_all(&t).unwrap();
        create_link(&source.join("gone"), &t.join("gone")).unwrap();
        let file = ConfigFile {
            source: source.to_string_lossy().into_owned(),
            targets: vec![t.to_string_lossy().into_owned()],
            ..Default::default()
        };
        remove(&file.resolve().unwrap(), "gone", &backup).unwrap();
        assert!(fs::symlink_metadata(source.join("gone")).is_err());
        assert!(fs::symlink_metadata(t.join("gone")).is_err());
        assert!(files(&backup).unwrap().keys().any(|k| k.ends_with("/gone/SKILL.md")));
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
