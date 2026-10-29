//! The config file (`config.toml` in `platform::config_dir()`) and the settings it resolves to.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::platform::{self, normalize};

/// The config file as written on disk. Every key is optional; paths may use `~` and
/// environment variables (`%VAR%` on Windows, `$VAR` elsewhere).
#[derive(Serialize, Deserialize, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct ConfigFile {
    pub source: String,
    pub targets: Vec<String>,
    pub rescan_secs: u64,
    pub debounce_ms: u64,
    pub log: LogFile,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct LogFile {
    pub path: String,
    pub max_bytes: u64,
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self {
            source: "~/.agents/skills".into(),
            targets: vec!["~/.claude/skills".into()],
            rescan_secs: 60,
            debounce_ms: 500,
            log: LogFile::default(),
        }
    }
}

impl Default for LogFile {
    fn default() -> Self {
        Self { path: contract(&platform::state_dir().join("skills-sync.log")), max_bytes: 1024 * 1024 }
    }
}

/// Settings in effect, with paths expanded and checked.
#[derive(Clone, PartialEq, Debug)]
pub struct Config {
    pub source: PathBuf,
    pub targets: Vec<PathBuf>,
    pub rescan: Duration,
    pub debounce: Duration,
    pub log_path: PathBuf,
    pub log_max_bytes: u64,
}

pub fn default_path() -> PathBuf {
    platform::config_dir().join("config.toml")
}

impl ConfigFile {
    /// Reads `path`; a missing file gives the defaults.
    pub fn read(path: &Path) -> Result<Self, String> {
        match fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("cannot read {}: {e}", path.display())),
        }
    }

    pub fn write(&self, path: &Path) -> Result<(), String> {
        let text = toml::to_string(self).map_err(|e| e.to_string())?;
        (|| -> io::Result<()> {
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir)?;
            }
            fs::write(path, text)
        })()
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
    }

    pub fn resolve(&self) -> Result<Config, String> {
        let source = expand(&self.source)?;
        let targets = self.targets.iter().map(|t| expand(t)).collect::<Result<Vec<_>, _>>()?;
        if targets.is_empty() {
            return Err("`targets` is empty".into());
        }
        if let Some(t) = targets.iter().find(|t| normalize(t) == normalize(&source)) {
            return Err(format!("{} is both the source and a target", t.display()));
        }
        if self.rescan_secs == 0 {
            return Err("`rescan_secs` must be at least 1".into());
        }
        Ok(Config {
            source,
            targets,
            rescan: Duration::from_secs(self.rescan_secs),
            debounce: Duration::from_millis(self.debounce_ms),
            log_path: expand(&self.log.path)?,
            log_max_bytes: self.log.max_bytes,
        })
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, String> {
        ConfigFile::read(path)?.resolve()
    }
}

/// Expands a leading `~` and environment variables; the result must be absolute.
pub fn expand(s: &str) -> Result<PathBuf, String> {
    let mut out = String::new();
    let mut rest = s;
    if let Some(after) = rest.strip_prefix('~')
        && (after.is_empty() || after.starts_with(['/', '\\']))
    {
        out.push_str(&platform::home().to_string_lossy());
        rest = after;
    }
    out.push_str(&platform::expand_vars(rest)?);
    let path = platform::native_path(&out);
    if path.is_absolute() { Ok(path) } else { Err(format!("{s:?} is not an absolute path")) }
}

/// Inverse of the `~` part of `expand`, for writing paths back to the file.
pub fn contract(p: &Path) -> String {
    match p.strip_prefix(platform::home()) {
        Ok(rest) if !rest.as_os_str().is_empty() => format!("~/{}", rest.to_string_lossy().replace('\\', "/")),
        _ => p.to_string_lossy().into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An absolute path on this OS, from a `/`-separated relative one.
    fn abs(rel: &str) -> PathBuf {
        if cfg!(windows) { PathBuf::from(format!(r"D:\{}", rel.replace('/', "\\"))) } else { PathBuf::from(format!("/{rel}")) }
    }

    #[test]
    fn expands_home_and_vars() {
        let home = platform::home();
        assert_eq!(expand("~/a/b").unwrap(), home.join("a").join("b"));
        assert_eq!(expand("~").unwrap(), home);
        let (var, value) = if cfg!(windows) { ("%USERPROFILE%/x", "USERPROFILE") } else { ("$HOME/x", "HOME") };
        assert_eq!(expand(var).unwrap(), PathBuf::from(std::env::var_os(value).unwrap()).join("x"));
        assert!(expand("~user/x").is_err(), "only a bare ~ is the home folder");
        assert!(expand("relative/path").is_err());
    }

    #[test]
    fn contract_round_trips() {
        let p = platform::home().join(".claude").join("skills");
        assert_eq!(contract(&p), "~/.claude/skills");
        assert_eq!(expand(&contract(&p)).unwrap(), p);
        assert_eq!(contract(&abs("skills")), abs("skills").to_string_lossy());
    }

    #[test]
    fn partial_file_keeps_defaults() {
        let f: ConfigFile = toml::from_str("targets = ['~/a']\n[log]\nmax_bytes = 10").unwrap();
        let c = f.resolve().unwrap();
        assert_eq!(c.targets, vec![platform::home().join("a")]);
        assert_eq!(c.rescan, Duration::from_secs(60));
        assert_eq!(c.log_max_bytes, 10);
        assert_eq!(c.log_path, platform::state_dir().join("skills-sync.log"));
    }

    #[test]
    fn rejects_bad_settings() {
        assert!(toml::from_str::<ConfigFile>("sorce = '~/a'").is_err(), "typos are reported");
        let a = abs("a").to_string_lossy().into_owned();
        let same = ConfigFile { source: a.clone(), targets: vec![a + "/"], ..Default::default() };
        assert!(same.resolve().is_err());
        let empty = ConfigFile { targets: vec![], ..Default::default() };
        assert!(empty.resolve().is_err());
    }
}
