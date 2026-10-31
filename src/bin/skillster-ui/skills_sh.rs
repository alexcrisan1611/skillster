//! Finding skills on skills.sh, the directory the `skills` CLI searches, and
//! installing one into the source, where the background process links it into
//! every target. Requests run on a thread of their own; the window sends one
//! and picks up the reply on a later frame.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use eframe::egui;
use serde::Deserialize;

const BASE: &str = "https://skills.sh";
/// As many results as `skills find` shows.
const LIMIT: &str = "20";
const TIMEOUT: Duration = Duration::from_secs(20);
/// Largest reply read; a skill's files all come in one.
const MAX_REPLY: u64 = 32 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Hit {
    /// `owner/repo/skill` for a GitHub repo, `domain/skill` for a site.
    pub id: String,
    /// `owner/repo`, or the domain of a site that publishes its skills under
    /// `/.well-known/skills/`.
    pub source: String,
    /// The skill's folder name.
    #[serde(rename = "skillId")]
    pub skill_id: String,
    pub name: String,
    #[serde(default)]
    pub installs: u64,
}

/// Where a skill's files come from.
#[derive(Debug, PartialEq)]
enum Origin<'a> {
    Repo { owner: &'a str, repo: &'a str },
    Site { domain: &'a str },
}

impl Hit {
    fn origin(&self) -> Result<Origin<'_>, String> {
        let origin = match self.source.split_once('/') {
            Some((owner, repo)) if safe_name(owner) && safe_name(repo) => Origin::Repo { owner, repo },
            None if safe_domain(&self.source) => Origin::Site { domain: &self.source },
            _ => return Err(format!("odd source {}", self.source)),
        };
        if !safe_name(&self.skill_id) {
            return Err(format!("odd name {}", self.id));
        }
        Ok(origin)
    }

    /// Its page on skills.sh.
    pub fn page(&self) -> String {
        match self.origin() {
            Ok(Origin::Site { domain }) => format!("{BASE}/site/{domain}/{}", self.skill_id),
            _ => format!("{BASE}/{}", self.id),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct File {
    /// Relative to the skill folder, `/`-separated.
    pub path: String,
    pub contents: String,
}

#[derive(Deserialize)]
struct SearchReply {
    skills: Vec<Hit>,
}

#[derive(Deserialize)]
struct DownloadReply {
    files: Vec<File>,
}

/// A site's `/.well-known/skills/index.json`.
#[derive(Deserialize)]
struct SiteIndex {
    skills: Vec<SiteSkill>,
}

#[derive(Deserialize)]
struct SiteSkill {
    name: String,
    #[serde(default)]
    files: Vec<String>,
}

pub enum Request {
    Search(String),
    /// A skill's files, to show.
    Fetch(Hit),
    /// A skill's files, written into this source folder.
    Install(Hit, PathBuf),
}

pub enum Reply {
    Found { query: String, hits: Result<Vec<Hit>, String> },
    Fetched { id: String, files: Result<Vec<File>, String> },
    Installed { id: String, result: Result<String, String> },
}

pub struct Client {
    pub replies: Receiver<Reply>,
    requests: Sender<Request>,
}

impl Client {
    pub fn spawn(ctx: egui::Context) -> Client {
        let (requests, rx) = mpsc::channel();
        let (tx, replies) = mpsc::channel();
        std::thread::spawn(move || run(&ctx, &rx, &tx));
        Client { replies, requests }
    }

    pub fn send(&self, request: Request) {
        let _ = self.requests.send(request);
    }
}

fn run(ctx: &egui::Context, rx: &Receiver<Request>, tx: &Sender<Reply>) {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(TIMEOUT)).build().into();
    while let Ok(first) = rx.recv() {
        // Of the searches queued while one ran, only the newest still matters.
        let mut queued: Vec<Request> = std::iter::once(first).chain(rx.try_iter()).collect();
        if let Some(last) = queued.iter().rposition(|r| matches!(r, Request::Search(_))) {
            let mut i = 0;
            queued.retain(|r| {
                i += 1;
                !matches!(r, Request::Search(_)) || i - 1 == last
            });
        }
        for request in queued {
            let reply = match request {
                Request::Search(query) => Reply::Found { hits: search(&agent, &query), query },
                Request::Fetch(hit) => Reply::Fetched { files: download(&agent, &hit), id: hit.id },
                Request::Install(hit, source) => Reply::Installed {
                    result: download(&agent, &hit).and_then(|files| install(&source, &hit.skill_id, &files)).inspect(|_| {
                        // Installed either way; without the record it shows no source.
                        let _ = crate::sources::record(&hit.skill_id, &hit.source);
                    }),
                    id: hit.id,
                },
            };
            if tx.send(reply).is_err() {
                return;
            }
            ctx.request_repaint();
        }
    }
}

fn get(agent: &ureq::Agent, url: &str, query: &[(&str, &str)]) -> Result<String, String> {
    let mut request = agent.get(url);
    for (k, v) in query {
        request = request.query(*k, *v);
    }
    let mut response = request.call().map_err(|e| e.to_string())?;
    response.body_mut().with_config().limit(MAX_REPLY).read_to_string().map_err(|e| e.to_string())
}

fn search(agent: &ureq::Agent, query: &str) -> Result<Vec<Hit>, String> {
    let body = get(agent, &format!("{BASE}/api/search"), &[("q", query), ("limit", LIMIT)])?;
    let mut hits = serde_json::from_str::<SearchReply>(&body).map_err(|e| e.to_string())?.skills;
    // Most installed first, as `skills find` lists them.
    hits.sort_by_key(|h| std::cmp::Reverse(h.installs));
    Ok(hits)
}

fn download(agent: &ureq::Agent, hit: &Hit) -> Result<Vec<File>, String> {
    let skill = &hit.skill_id;
    match hit.origin()? {
        Origin::Repo { owner, repo } => {
            let body = get(agent, &format!("{BASE}/api/download/{owner}/{repo}/{skill}"), &[])?;
            Ok(serde_json::from_str::<DownloadReply>(&body).map_err(|e| e.to_string())?.files)
        }
        // As the `skills` CLI fetches them: the site's index lists each skill's
        // files, which sit in a folder named after it.
        Origin::Site { domain } => {
            let root = format!("https://{domain}/.well-known/skills");
            let body = get(agent, &format!("{root}/index.json"), &[])?;
            let index = serde_json::from_str::<SiteIndex>(&body).map_err(|e| e.to_string())?;
            let entry = index
                .skills
                .into_iter()
                .find(|s| s.name == *skill)
                .ok_or_else(|| format!("{domain} no longer lists {skill}"))?;
            if let Some(bad) = entry.files.iter().find(|f| !safe_path(f)) {
                return Err(format!("{skill} has a file outside its folder: {bad}"));
            }
            entry
                .files
                .into_iter()
                .map(|path| Ok(File { contents: get(agent, &format!("{root}/{skill}/{path}"), &[])?, path }))
                .collect()
        }
    }
}

/// The skill's `SKILL.md`, from its files.
pub fn skill_md(files: &[File]) -> Option<&str> {
    files.iter().find(|f| f.path == "SKILL.md").map(|f| f.contents.as_str())
}

/// Writes a skill's files into `source/<name>`: into a hidden folder first, which
/// the background process ignores, then renamed into place, so it only ever
/// links a complete skill.
pub fn install(source: &Path, name: &str, files: &[File]) -> Result<String, String> {
    if !safe_name(name) {
        return Err(format!("odd skill name {name}"));
    }
    let dest = source.join(name);
    if fs::symlink_metadata(&dest).is_ok() {
        return Err(format!("{name} is already in your skills"));
    }
    // As the `skills` CLI installs them: without its metadata or version-control folders.
    let files: Vec<&File> = files.iter().filter(|f| installable(&f.path)).collect();
    if !files.iter().any(|f| f.path == "SKILL.md") {
        return Err(format!("{name} has no SKILL.md"));
    }
    if let Some(f) = files.iter().find(|f| !safe_path(&f.path)) {
        return Err(format!("{name} has a file outside its folder: {}", f.path));
    }
    let tmp = source.join(format!(".skillster-install-{name}"));
    let _ = fs::remove_dir_all(&tmp);
    let write = || -> io::Result<()> {
        for f in &files {
            let path = tmp.join(&f.path);
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir)?;
            }
            fs::write(path, &f.contents)?;
        }
        fs::rename(&tmp, &dest)
    };
    if let Err(e) = write() {
        let _ = fs::remove_dir_all(&tmp);
        return Err(format!("cannot install {name}: {e}"));
    }
    Ok(format!("Installed {name}"))
}

fn safe_name(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('.') && s.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

/// A host name: letters, digits, dots and hyphens, with a dot somewhere.
fn safe_domain(s: &str) -> bool {
    s.contains('.')
        && s.split('.').all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

/// A relative path that stays inside the skill folder on every OS.
fn safe_path(path: &str) -> bool {
    path.split('/').all(|part| {
        !part.is_empty() && part != "." && part != ".." && !part.contains(['\\', ':']) && !part.chars().any(char::is_control)
    })
}

fn installable(path: &str) -> bool {
    let mut parts: Vec<&str> = path.split('/').collect();
    let file = parts.pop().unwrap_or_default();
    file != "metadata.json" && parts.iter().all(|p| !matches!(*p, ".git" | "__pycache__" | "__pypackages__"))
}

/// `1234` as `1,234`, `85012` as `85K`, `1119688` as `1.1M`.
pub fn count(n: u64) -> String {
    match n {
        0..10_000 => {
            let s = n.to_string();
            if s.len() > 3 { format!("{},{}", &s[..s.len() - 3], &s[s.len() - 3..]) } else { s }
        }
        10_000..1_000_000 => format!("{}K", n / 1000),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, contents: &str) -> File {
        File { path: path.into(), contents: contents.into() }
    }

    #[test]
    fn installs_only_what_stays_inside() {
        let root = std::env::temp_dir().join(format!("skillster-install-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let files = [file("SKILL.md", "# A"), file("refs/b.md", "b"), file("metadata.json", "{}")];
        install(&root, "a", &files).unwrap();
        assert_eq!(fs::read_to_string(root.join("a/refs/b.md")).unwrap(), "b");
        assert!(!root.join("a/metadata.json").exists());
        assert!(install(&root, "a", &files).is_err(), "never over an existing skill");
        for bad in ["../x", "/x", "a//b", "C:/x", "a\\b"] {
            assert!(install(&root, "b", &[file("SKILL.md", ""), file(bad, "")]).is_err(), "{bad}");
        }
        assert!(install(&root, "c", &[file("README.md", "")]).is_err(), "needs a SKILL.md");
        assert!(install(&root, "../d", &files).is_err());
        let left: Vec<_> = fs::read_dir(&root).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left, ["a"], "nothing half-written is left behind");
        fs::remove_dir_all(&root).unwrap();
    }

    fn hit(id: &str, source: &str, skill: &str) -> Hit {
        Hit { id: id.into(), source: source.into(), skill_id: skill.into(), name: skill.into(), installs: 0 }
    }

    #[test]
    fn pages_and_origins() {
        let repo = hit("larksuite/cli/lark-shared", "larksuite/cli", "lark-shared");
        assert_eq!(repo.page(), "https://skills.sh/larksuite/cli/lark-shared");
        assert_eq!(repo.origin(), Ok(Origin::Repo { owner: "larksuite", repo: "cli" }));
        let site = hit("open.feishu.cn/lark-shared", "open.feishu.cn", "lark-shared");
        assert_eq!(site.page(), "https://skills.sh/site/open.feishu.cn/lark-shared");
        assert_eq!(site.origin(), Ok(Origin::Site { domain: "open.feishu.cn" }));
        for (source, skill) in [("localhost", "a"), ("a/../b", "a"), ("evil.com:8080", "a"), ("a.b", "../x"), ("-a.b", "a")] {
            assert!(hit("x", source, skill).origin().is_err(), "{source} {skill}");
        }
    }

    #[test]
    fn counts() {
        assert_eq!(count(14), "14");
        assert_eq!(count(1234), "1,234");
        assert_eq!(count(855_012), "855K");
        assert_eq!(count(1_119_688), "1.1M");
    }
}
