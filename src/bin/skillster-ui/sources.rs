//! Where each skill in the source came from: the `skills` CLI records the ones
//! it installs in its lock file, and Skillster records the ones it installs from
//! skills.sh in `sources.json` in `platform::state_dir()`. Skills in neither have
//! no known source.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use skillster::platform;

#[derive(Deserialize)]
struct Lock {
    #[serde(default)]
    skills: HashMap<String, LockEntry>,
}

#[derive(Deserialize)]
struct LockEntry {
    #[serde(default)]
    source: String,
    #[serde(default, rename = "sourceType")]
    source_type: String,
}

/// Each skill's source by folder name: `owner/repo`, a site's domain, or for a
/// skill installed from a folder on disk, that folder's name.
pub fn read() -> HashMap<String, String> {
    let mut sources = read_own(&own_path());
    // The CLI's record wins: it is the newer one if both installed a skill.
    sources.extend(read_lock(&lock_path()));
    sources
}

/// Records that Skillster installed `skill` from `source`.
pub fn record(skill: &str, source: &str) -> Result<(), String> {
    edit(|s| {
        s.insert(skill.to_string(), source.to_string());
    })
}

/// Forgets where `skill` came from, once it is deleted.
pub fn forget(skill: &str) -> Result<(), String> {
    edit(|s| {
        s.remove(skill);
    })
}

fn edit(change: impl FnOnce(&mut BTreeMap<String, String>)) -> Result<(), String> {
    let path = own_path();
    let mut sources: BTreeMap<String, String> = read_own(&path).into_iter().collect();
    change(&mut sources);
    let text = serde_json::to_string_pretty(&sources).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn own_path() -> PathBuf {
    platform::state_dir().join("sources.json")
}

/// Where the `skills` CLI keeps its lock file.
fn lock_path() -> PathBuf {
    match std::env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()) {
        Some(state) => PathBuf::from(state).join("skills").join(".skill-lock.json"),
        None => platform::home().join(".agents").join(".skill-lock.json"),
    }
}

fn read_own(path: &Path) -> HashMap<String, String> {
    fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

fn read_lock(path: &Path) -> HashMap<String, String> {
    let Some(lock) = fs::read_to_string(path).ok().and_then(|text| serde_json::from_str::<Lock>(&text).ok()) else {
        return HashMap::new();
    };
    lock.skills
        .into_iter()
        .filter_map(|(name, entry)| {
            let source = entry.source.trim().trim_end_matches(".git").trim_end_matches(['/', '\\']);
            let label = match entry.source_type.as_str() {
                // A path: its last folder says enough.
                "local" => source.rsplit(['/', '\\']).next().unwrap_or(source),
                _ => source,
            };
            (!label.is_empty()).then(|| (name, label.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_cli_lock() {
        let dir = std::env::temp_dir().join(format!("skillster-sources-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let lock = dir.join("lock.json");
        let text = r#"{"version":3,"skills":{
            "a":{"source":"owner/repo","sourceType":"github","sourceUrl":"https://github.com/owner/repo.git"},
            "b":{"source":"C:\\work\\my-skills\\","sourceType":"local"},
            "c":{"source":"","sourceType":"github"},
            "d":{"source":"open.feishu.cn","sourceType":"well-known"}}}"#;
        fs::write(&lock, text).unwrap();
        let s = read_lock(&lock);
        assert_eq!(s["a"], "owner/repo");
        assert_eq!(s["b"], "my-skills");
        assert!(!s.contains_key("c"));
        assert_eq!(s["d"], "open.feishu.cn");
        assert!(read_lock(&dir.join("missing.json")).is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }
}
