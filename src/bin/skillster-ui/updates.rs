//! Whether skills.sh has a newer version of the skills in the source that came
//! from it, and putting that version in place. Checked only when asked, one skill
//! at a time on a thread of its own (so searching isn't held up behind it); the
//! answers are kept for as long as the window is open.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;
use skillster::config::contract;
use skillster::platform::{self, link_target};
use skillster::status;

use crate::library;
use crate::skills_sh::{self, File, Hit};

/// Largest file shown as a diff.
const MAX_DIFF: usize = 256 * 1024;

/// How the version on skills.sh differs from the one in the source.
#[derive(Clone, Debug)]
pub struct Changes {
    /// The version on skills.sh, which Update writes.
    pub files: Vec<File>,
    pub changed: Vec<String>,
    /// Files only on skills.sh.
    pub added: Vec<String>,
    /// Text files only in the source, which updating moves to backups with the rest.
    pub removed: Vec<String>,
    /// Other files only in the source, which updating keeps: skills.sh serves a
    /// skill's text files only, so its images and the like are never among them.
    pub kept: Vec<String>,
    /// Unified diffs of the changed text files, the source's copy first.
    pub diffs: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub enum State {
    Checking,
    Current,
    Available(Changes),
    /// Kept, to offer again if the update fails.
    Updating(Changes),
    Failed(String),
}

enum Job {
    Check { name: String, source: String, dir: PathBuf },
    Update { name: String, files: Vec<File>, keep: Vec<String>, source_dir: PathBuf },
}

enum Outcome {
    Checked(Result<Option<Changes>, String>),
    Updated(Result<String, String>),
}

pub struct Updates {
    states: HashMap<String, State>,
    jobs: Sender<Job>,
    replies: Receiver<(String, Outcome)>,
}

/// Whether skills.sh can say if `name`, from `source`, has a newer version.
pub fn checkable(source: &str, name: &str) -> bool {
    library::skills_sh_id(source, name).is_some()
}

impl Updates {
    pub fn spawn(ctx: egui::Context) -> Updates {
        let (jobs, rx) = mpsc::channel();
        let (tx, replies) = mpsc::channel();
        std::thread::spawn(move || run(&ctx, &rx, &tx));
        Updates { states: HashMap::new(), jobs, replies }
    }

    /// Checks each of `skills` (name and source) in the folder `source_dir`,
    /// except the ones already being checked or updated.
    pub fn check(&mut self, skills: Vec<(String, String)>, source_dir: &Path) {
        for (name, source) in skills {
            if matches!(self.states.get(&name), Some(State::Checking | State::Updating(_))) {
                continue;
            }
            self.states.insert(name.clone(), State::Checking);
            let dir = source_dir.join(&name);
            let _ = self.jobs.send(Job::Check { name, source, dir });
        }
    }

    /// Puts skills.sh's version of `name` in place of the one in `source_dir`;
    /// false if no newer version is known.
    pub fn update(&mut self, name: &str, source_dir: &Path) -> bool {
        if !matches!(self.states.get(name), Some(State::Available(_))) {
            return false;
        }
        let Some(State::Available(changes)) = self.states.remove(name) else { return false };
        let (files, keep) = (changes.files.clone(), changes.kept.clone());
        self.states.insert(name.to_string(), State::Updating(changes));
        let source_dir = source_dir.to_path_buf();
        let _ = self.jobs.send(Job::Update { name: name.to_string(), files, keep, source_dir });
        true
    }

    /// Picks up the answers so far, returning each finished update's skill and
    /// what came of it, for a notice.
    pub fn take(&mut self) -> Vec<(String, Result<String, String>)> {
        let mut done = Vec::new();
        while let Ok((name, outcome)) = self.replies.try_recv() {
            let state = match outcome {
                Outcome::Checked(Ok(None)) => State::Current,
                Outcome::Checked(Ok(Some(changes))) => State::Available(changes),
                Outcome::Checked(Err(e)) => State::Failed(e),
                Outcome::Updated(result) => {
                    // A failed update leaves the old version, so the update is offered again.
                    let state = match (&result, self.states.remove(&name)) {
                        (Ok(_), _) => State::Current,
                        (Err(_), Some(State::Updating(changes))) => State::Available(changes),
                        (Err(e), _) => State::Failed(e.clone()),
                    };
                    done.push((name.clone(), result));
                    state
                }
            };
            self.states.insert(name, state);
        }
        done
    }

    pub fn get(&self, name: &str) -> Option<&State> {
        self.states.get(name)
    }

    /// Forgets `name`, once it is deleted.
    pub fn forget(&mut self, name: &str) {
        self.states.remove(name);
    }

    pub fn asked(&self) -> bool {
        !self.states.is_empty()
    }

    pub fn count(&self, want: impl Fn(&State) -> bool) -> usize {
        self.states.values().filter(|s| want(s)).count()
    }

    /// The skills with a newer version, by name.
    pub fn available(&self) -> Vec<String> {
        let mut names: Vec<String> =
            self.states.iter().filter(|(_, s)| matches!(s, State::Available(_))).map(|(n, _)| n.clone()).collect();
        names.sort();
        names
    }
}

fn run(ctx: &egui::Context, rx: &Receiver<Job>, tx: &Sender<(String, Outcome)>) {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(skills_sh::TIMEOUT)).build().into();
    while let Ok(job) = rx.recv() {
        let reply = match job {
            Job::Check { name, source, dir } => {
                let outcome = Outcome::Checked(check(&agent, &name, &source, &dir));
                (name, outcome)
            }
            Job::Update { name, files, keep, source_dir } => {
                let backups = platform::backups_dir();
                let outcome = Outcome::Updated(skills_sh::update(&source_dir, &name, &files, &keep, &backups));
                (name, outcome)
            }
        };
        if tx.send(reply).is_err() {
            return;
        }
        ctx.request_repaint();
    }
}

/// How skills.sh's version of `name` differs from the one in `dir`, or `None`
/// when they are the same.
fn check(agent: &ureq::Agent, name: &str, source: &str, dir: &Path) -> Result<Option<Changes>, String> {
    // A skill in the source that is a link (to a checkout, say) is updated where it lives.
    if let Some(t) = link_target(dir) {
        return Err(format!("it links to {}; update it there", contract(&t)));
    }
    let hit = Hit {
        id: format!("{source}/{name}"),
        source: source.to_string(),
        skill_id: name.to_string(),
        name: name.to_string(),
        installs: 0,
    };
    let files = skills_sh::download(agent, &hit).map_err(|e| {
        if e.contains("404") { "skills.sh doesn't have it".to_string() } else { e }
    })?;
    let files: Vec<File> = files.into_iter().filter(|f| skills_sh::installable(&f.path)).collect();
    if !files.iter().any(|f| f.path == "SKILL.md") {
        return Err("skills.sh has no SKILL.md for it".into());
    }
    if let Some(f) = files.iter().find(|f| !skills_sh::safe_path(&f.path)) {
        return Err(format!("skills.sh lists a file outside its folder: {}", f.path));
    }
    let local = status::skill_files(dir).map_err(|e| format!("cannot read {}: {e}", contract(dir)))?;
    let local: BTreeMap<String, PathBuf> = local.into_iter().filter(|(rel, _)| skills_sh::installable(rel)).collect();
    Ok(compare(&local, files))
}

/// Compares the source's files with skills.sh's, ignoring line endings: git
/// often checks text out with CRLF on Windows, and skills.sh serves LF.
fn compare(local: &BTreeMap<String, PathBuf>, files: Vec<File>) -> Option<Changes> {
    let mut changes = Changes {
        files: Vec::new(),
        changed: Vec::new(),
        added: Vec::new(),
        removed: Vec::new(),
        kept: Vec::new(),
        diffs: BTreeMap::new(),
    };
    for f in &files {
        let Some(path) = local.get(&f.path) else {
            changes.added.push(f.path.clone());
            continue;
        };
        let theirs = lf(f.contents.as_bytes());
        let ours = fs::read(path).map(|b| lf(&b)).unwrap_or_default();
        if ours == theirs {
            continue;
        }
        changes.changed.push(f.path.clone());
        if let (Ok(a), Ok(b)) = (std::str::from_utf8(&ours), std::str::from_utf8(&theirs))
            && a.len().max(b.len()) <= MAX_DIFF
        {
            let diff = similar::TextDiff::from_lines(a, b);
            changes.diffs.insert(f.path.clone(), diff.unified_diff().context_radius(2).header("yours", "skills.sh").to_string());
        }
    }
    for (rel, path) in local {
        if files.iter().any(|f| f.path == *rel) {
            continue;
        }
        // Unreadable counts as not text, so it is kept rather than moved.
        let text = fs::read(path).is_ok_and(|b| !b.contains(&0) && std::str::from_utf8(&b).is_ok());
        if text { &mut changes.removed } else { &mut changes.kept }.push(rel.clone());
    }
    if changes.changed.is_empty() && changes.added.is_empty() && changes.removed.is_empty() {
        return None;
    }
    changes.files = files;
    Some(changes)
}

/// `bytes` with every CRLF as LF.
fn lf(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for (i, &b) in bytes.iter().enumerate() {
        if !(b == b'\r' && bytes.get(i + 1) == Some(&b'\n')) {
            out.push(b);
        }
    }
    out
}
