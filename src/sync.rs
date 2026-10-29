use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{MAIN_SEPARATOR, Path, PathBuf};

use crate::config::Config;
use crate::logger::Logger;
use crate::platform::{create_link, link_target, normalize, remove_link};

/// One pass. `reported` holds conflicts already logged, so they are logged once.
pub fn sync(config: &Config, log: &Logger, reported: &mut HashSet<PathBuf>) {
    let skills = match source_skills(&config.source) {
        Ok(s) => s,
        Err(e) => {
            log.line(&format!("cannot read {}: {e}", config.source.display()));
            return;
        }
    };

    for target in &config.targets {
        if let Err(e) = fs::create_dir_all(target) {
            log.line(&format!("cannot create {}: {e}", target.display()));
            continue;
        }
        link_missing(&config.source, target, &skills, log, reported);
        prune_stale(&config.source, target, log);
    }
}

/// Skill folders in the source (including ones that are themselves links).
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

/// Create a link in `target` for every skill that has none yet.
fn link_missing(source: &Path, target: &Path, skills: &[String], log: &Logger, reported: &mut HashSet<PathBuf>) {
    for name in skills {
        let src = source.join(name);
        let dest = target.join(name);
        if fs::symlink_metadata(&dest).is_err() {
            match create_link(&src, &dest) {
                Ok(()) => log.line(&format!("linked  {}", dest.display())),
                Err(e) => log.line(&format!("failed  {}: {e}", dest.display())),
            }
            continue;
        }
        let points_here = link_target(&dest).is_some_and(|t| normalize(&t) == normalize(&src));
        if points_here {
            reported.remove(&dest);
        } else if reported.insert(dest.clone()) {
            log.line(&format!("skipped {} (exists and is not a link to {})", dest.display(), src.display()));
        }
    }
}

/// Remove links in `target` that point into the source but whose skill no longer exists.
fn prune_stale(source: &Path, target: &Path, log: &Logger) {
    let source_root = normalize(source);
    let Ok(entries) = fs::read_dir(target) else { return };
    for entry in entries.flatten() {
        let dest = entry.path();
        let Some(t) = link_target(&dest) else { continue };
        let into_source = normalize(&t).strip_prefix(&source_root).is_some_and(|rest| rest.starts_with(MAIN_SEPARATOR));
        if into_source && !t.exists() {
            match remove_link(&dest) {
                Ok(()) => log.line(&format!("removed {} (skill is gone)", dest.display())),
                Err(e) => log.line(&format!("failed to remove {}: {e}", dest.display())),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigFile;

    #[test]
    fn links_skips_and_prunes() {
        let root = std::env::temp_dir().join(format!("skillster-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (source, a, b) = (root.join("source"), root.join("a"), root.join("b"));
        for skill in ["alpha", "beta", ".hidden"] {
            fs::create_dir_all(source.join(skill)).unwrap();
        }
        fs::create_dir_all(b.join("beta")).unwrap(); // a real folder in the way
        fs::create_dir_all(a.join("unrelated")).unwrap();

        let file = ConfigFile {
            source: source.to_string_lossy().into_owned(),
            targets: vec![a.to_string_lossy().into_owned(), b.to_string_lossy().into_owned()],
            ..Default::default()
        };
        let mut config = file.resolve().unwrap();
        config.log_path = root.join("log.txt");
        let log = Logger::new(config.log_path.clone(), 1 << 20, false);
        let mut reported = HashSet::new();

        sync(&config, &log, &mut reported);
        assert!(a.join("alpha").is_dir() && link_target(&a.join("alpha")).is_some());
        assert!(link_target(&a.join("beta")).is_some());
        assert!(!a.join(".hidden").exists());
        assert!(link_target(&b.join("beta")).is_none(), "real folder left alone");
        assert_eq!(reported.len(), 1);

        fs::write(source.join("alpha").join("SKILL.md"), "x").unwrap();
        assert!(a.join("alpha").join("SKILL.md").is_file(), "the link shows the source");

        fs::remove_dir_all(source.join("alpha")).unwrap();
        sync(&config, &log, &mut reported);
        assert!(fs::symlink_metadata(a.join("alpha")).is_err(), "stale link removed");
        assert!(fs::symlink_metadata(b.join("alpha")).is_err());
        assert!(a.join("unrelated").is_dir() && b.join("beta").is_dir());
        assert!(source.join("beta").is_dir(), "pruning never touches the source");

        fs::remove_dir_all(&root).unwrap();
    }
}
