//! The projects on this computer with skills of their own, for the "Projects"
//! list. They are found on a thread of their own (`find::projects`, which asks
//! Everything or walks the home folder), each skill's description read there
//! too. Projects can be ignored: the ones ignored are kept in
//! `ignored-projects.json` in `platform::state_dir()` and left out from then on.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;
use skillster::platform::{self, normalize};
use skillster::{find, status};

pub struct Project {
    pub root: PathBuf,
    /// Its skills, by name. A project often keeps the same skill in several
    /// folders (`.agents/skills` and `.claude/skills`); it is listed once.
    pub skills: Vec<Skill>,
}

pub struct Skill {
    pub name: String,
    pub description: Option<String>,
    /// The skills folders that have it, each the folder holding its `SKILL.md`'s
    /// folder.
    pub folders: Vec<PathBuf>,
}

impl Project {
    /// Its folder's name.
    pub fn name(&self) -> String {
        self.root.file_name().map_or_else(|| self.root.to_string_lossy(), |n| n.to_string_lossy()).into_owned()
    }
}

/// Looks for projects when asked, one search at a time.
pub struct Finder {
    /// The last search's projects; `None` before the first one finishes.
    pub found: Option<Vec<Project>>,
    pub searching: bool,
    requests: Sender<()>,
    replies: Receiver<Vec<Project>>,
}

impl Finder {
    pub fn spawn(ctx: egui::Context) -> Finder {
        let (requests, rx) = mpsc::channel::<()>();
        let (tx, replies) = mpsc::channel();
        std::thread::spawn(move || {
            while rx.recv().is_ok() {
                if tx.send(look()).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        });
        Finder { found: None, searching: false, requests, replies }
    }

    /// Looks again, unless a search is under way.
    pub fn search(&mut self) {
        if !self.searching {
            self.searching = true;
            let _ = self.requests.send(());
        }
    }

    /// Whether a search has finished; takes its projects if so.
    pub fn take(&mut self) -> bool {
        let mut changed = false;
        while let Ok(found) = self.replies.try_recv() {
            self.found = Some(found);
            self.searching = false;
            changed = true;
        }
        changed
    }
}

fn look() -> Vec<Project> {
    find::projects().into_iter().map(|p| Project { root: p.root, skills: read(&p.folders) }).collect()
}

/// The skills in `folders`, each once, sorted by name; the description is the
/// first folder's.
fn read(folders: &[PathBuf]) -> Vec<Skill> {
    let mut skills: Vec<Skill> = Vec::new();
    for folder in folders {
        let Ok(entries) = fs::read_dir(folder) else { continue };
        for entry in entries.flatten().filter(|e| e.path().join("SKILL.md").is_file()) {
            let name = entry.file_name().to_string_lossy().into_owned();
            match skills.iter_mut().find(|s| s.name == name) {
                Some(skill) => skill.folders.push(folder.clone()),
                None => skills.push(Skill {
                    description: status::skill_text(&entry.path()).as_deref().and_then(status::description),
                    name,
                    folders: vec![folder.clone()],
                }),
            }
        }
    }
    skills.sort_by_key(|s| s.name.to_lowercase());
    skills
}

/// The projects ignored, by path.
pub fn ignored() -> Vec<PathBuf> {
    fs::read_to_string(ignored_path())
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<PathBuf>>(&text).ok())
        .unwrap_or_default()
}

/// Leaves the project at `root` out from now on, or lists it again. Gives the
/// projects ignored now.
pub fn set_ignored(root: &Path, ignore: bool) -> Result<Vec<PathBuf>, String> {
    let mut list = ignored();
    list.retain(|p| normalize(p) != normalize(root));
    if ignore {
        list.push(root.to_path_buf());
        list.sort_by_key(|p| normalize(p));
    }
    let path = ignored_path();
    let text = serde_json::to_string_pretty(&list).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(list)
}

fn ignored_path() -> PathBuf {
    platform::state_dir().join("ignored-projects.json")
}
