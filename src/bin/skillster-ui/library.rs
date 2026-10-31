//! How often skills.sh says each skill in the source has been installed, for the
//! "All skills" list. A skill is looked up by its source (from `sources`) and
//! name, one at a time on a thread of its own; the answers are kept for as long
//! as the window is open.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use eframe::egui;
use serde::Deserialize;

const SEARCH: &str = "https://skills.sh/api/search";
/// Enough results that the skill searched for is among them, even with a
/// common name.
const LIMIT: &str = "100";
const TIMEOUT: Duration = Duration::from_secs(20);

/// How often skills.sh says a skill has been installed.
#[derive(Clone, Debug, PartialEq)]
pub enum Installs {
    Asking,
    Count(u64),
    /// skills.sh doesn't list it.
    Unlisted,
    Failed(String),
}

/// The skill's id on skills.sh, for a source skills.sh lists: `owner/repo` on
/// GitHub, or a site's domain.
pub fn skills_sh_id(source: &str, name: &str) -> Option<String> {
    let repo = source
        .split_once('/')
        .is_some_and(|(owner, repo)| !owner.is_empty() && !repo.is_empty() && !repo.contains('/'));
    let site = source.contains('.') && !source.contains(['/', '\\', ':', ' ']);
    (repo || site).then(|| format!("{source}/{name}"))
}

/// Where a source can be opened: the GitHub repo, or the site.
pub fn source_url(source: &str) -> Option<String> {
    skills_sh_id(source, "")?;
    Some(if source.contains('/') { format!("https://github.com/{source}") } else { format!("https://{source}") })
}

/// Looks up install counts on skills.sh.
pub struct Counts {
    counts: HashMap<String, Installs>,
    requests: Sender<(String, String)>,
    replies: Receiver<(String, Installs)>,
}

impl Counts {
    pub fn spawn(ctx: egui::Context) -> Counts {
        let (requests, rx) = mpsc::channel();
        let (tx, replies) = mpsc::channel();
        std::thread::spawn(move || run(&ctx, &rx, &tx));
        Counts { counts: HashMap::new(), requests, replies }
    }

    /// The count for skills.sh id `id`, the skill named `name`; asked for the
    /// first time it is wanted.
    pub fn get(&mut self, id: &str, name: &str) -> Installs {
        while let Ok((id, installs)) = self.replies.try_recv() {
            self.counts.insert(id, installs);
        }
        let key = id.to_lowercase();
        if let Some(installs) = self.counts.get(&key) {
            return installs.clone();
        }
        self.counts.insert(key.clone(), Installs::Asking);
        let _ = self.requests.send((key, name.to_string()));
        Installs::Asking
    }

    /// Forgets the failed lookups, so they are asked again.
    pub fn retry(&mut self) {
        self.counts.retain(|_, i| !matches!(i, Installs::Failed(_)));
    }

    pub fn any_failed(&self) -> bool {
        self.counts.values().any(|i| matches!(i, Installs::Failed(_)))
    }
}

#[derive(Deserialize)]
struct SearchReply {
    skills: Vec<SearchHit>,
}

#[derive(Deserialize)]
struct SearchHit {
    id: String,
    #[serde(default)]
    installs: u64,
}

fn run(ctx: &egui::Context, rx: &Receiver<(String, String)>, tx: &Sender<(String, Installs)>) {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(TIMEOUT)).build().into();
    while let Ok((id, name)) = rx.recv() {
        let installs = match look_up(&agent, &id, &name) {
            Ok(Some(n)) => Installs::Count(n),
            Ok(None) => Installs::Unlisted,
            Err(e) => Installs::Failed(e),
        };
        if tx.send((id, installs)).is_err() {
            return;
        }
        ctx.request_repaint();
    }
}

/// Searches skills.sh for the skill's name and picks out the one with its id.
fn look_up(agent: &ureq::Agent, id: &str, name: &str) -> Result<Option<u64>, String> {
    let mut response = agent.get(SEARCH).query("q", name).query("limit", LIMIT).call().map_err(|e| e.to_string())?;
    let body = response.body_mut().read_to_string().map_err(|e| e.to_string())?;
    let reply = serde_json::from_str::<SearchReply>(&body).map_err(|e| e.to_string())?;
    Ok(reply.skills.into_iter().find(|h| h.id.eq_ignore_ascii_case(id)).map(|h| h.installs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_urls() {
        assert_eq!(skills_sh_id("anthropics/skills", "pdf").as_deref(), Some("anthropics/skills/pdf"));
        assert_eq!(skills_sh_id("open.feishu.cn", "lark").as_deref(), Some("open.feishu.cn/lark"));
        assert_eq!(skills_sh_id("my-skills", "a"), None, "a local folder isn't on skills.sh");
        assert_eq!(skills_sh_id("a/b/c", "a"), None);
        assert_eq!(source_url("anthropics/skills").as_deref(), Some("https://github.com/anthropics/skills"));
        assert_eq!(source_url("open.feishu.cn").as_deref(), Some("https://open.feishu.cn"));
        assert_eq!(source_url("my-skills"), None);
    }
}
