use std::fs;
use std::io;
#[cfg(not(target_os = "macos"))]
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

pub const CLI_EXE: &str = "skills-sync";
pub const DAEMON_EXE: &str = "skills-syncd";

#[cfg(target_os = "macos")]
const LAUNCHD_LABEL: &str = "io.github.alexcrisan1611.skills-sync";
#[cfg(not(target_os = "macos"))]
const SYSTEMD_UNIT: &str = "skills-syncd.service";

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").expect("HOME is not set"))
}

/// `$XDG_CONFIG_HOME` if set and absolute, else `~/.config`.
fn xdg_config_home() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME").unwrap_or_else(|| home().join(".config"))
}

fn xdg_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute())
}

/// `~/.config/skills-sync` (honouring `$XDG_CONFIG_HOME`).
pub fn config_dir() -> PathBuf {
    xdg_config_home().join("skills-sync")
}

/// The log and the lock file: `~/Library/Application Support/skills-sync` on macOS,
/// `~/.local/state/skills-sync` (honouring `$XDG_STATE_HOME`) elsewhere.
pub fn state_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        home().join("Library").join("Application Support").join("skills-sync")
    } else {
        xdg_dir("XDG_STATE_HOME").unwrap_or_else(|| home().join(".local").join("state")).join("skills-sync")
    }
}

/// Folders whose subfolders each belong to one app: `~/.config`, plus
/// `~/Library/Application Support` on macOS and `~/.local/share` elsewhere.
pub fn app_data_roots() -> Vec<PathBuf> {
    let data = if cfg!(target_os = "macos") {
        home().join("Library").join("Application Support")
    } else {
        xdg_dir("XDG_DATA_HOME").unwrap_or_else(|| home().join(".local").join("share"))
    };
    vec![xdg_config_home(), data]
}

/// `~/.local/bin`.
pub fn install_dir() -> PathBuf {
    home().join(".local").join("bin")
}

pub fn create_link(src: &Path, dest: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(src, dest)
}

/// Where the symlink at `p` points (made absolute, `..` resolved), including broken
/// ones; `None` if `p` is not a symlink.
pub fn link_target(p: &Path) -> Option<PathBuf> {
    if !fs::symlink_metadata(p).ok()?.file_type().is_symlink() {
        return None;
    }
    let t = fs::read_link(p).ok()?;
    let t = if t.is_absolute() { t } else { p.parent()?.join(t) };
    let mut clean = PathBuf::new();
    for c in t.components() {
        match c {
            Component::ParentDir => {
                clean.pop();
            }
            Component::CurDir => {}
            c => clean.push(c),
        }
    }
    Some(clean)
}

/// Deletes the symlink only, never what it points to.
pub fn remove_link(p: &Path) -> io::Result<()> {
    fs::remove_file(p)
}

/// Comparable form of a path: no trailing slash.
pub fn normalize(p: &Path) -> String {
    p.to_string_lossy().trim_end_matches('/').to_string()
}

/// Expands `$VAR` and `${VAR}` references.
pub fn expand_vars(s: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('$') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let (name, next) = if let Some(braced) = after.strip_prefix('{') {
            let end = braced.find('}').ok_or_else(|| format!("unclosed `${{` in {s:?}"))?;
            (&braced[..end], &braced[end + 1..])
        } else {
            let end = after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(after.len());
            (&after[..end], &after[end..])
        };
        if name.is_empty() {
            // A lone `$` is just a character.
            out.push('$');
            rest = after;
            continue;
        }
        let value = std::env::var_os(name).ok_or_else(|| format!("${name} is not set (in {s:?})"))?;
        out.push_str(&value.to_string_lossy());
        rest = next;
    }
    out.push_str(rest);
    Ok(out)
}

pub fn native_path(s: &str) -> PathBuf {
    PathBuf::from(s)
}

/// Stops the background process, however it was started.
pub fn stop_daemon() {
    #[cfg(target_os = "macos")]
    quiet(Command::new("launchctl").args(["bootout", &launchd_target()]));
    #[cfg(not(target_os = "macos"))]
    quiet(Command::new("systemctl").args(["--user", "stop", SYSTEMD_UNIT]));
    quiet(Command::new("pkill").args(["-x", DAEMON_EXE]));
}

/// Registers `daemon` to start at login and starts it. Returns where it was registered.
#[cfg(target_os = "macos")]
pub fn enable_autostart(daemon: &Path, config: Option<&Path>) -> Result<String, String> {
    let plist = launch_agent_path();
    let mut args = vec![daemon.to_path_buf()];
    if let Some(c) = config {
        args.extend([PathBuf::from("--config"), c.to_path_buf()]);
    }
    let args: String = args
        .iter()
        .map(|a| format!("        <string>{}</string>\n", xml_escape(&a.to_string_lossy())))
        .collect();
    let text = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\
         \x20   <key>Label</key>\n    <string>{LAUNCHD_LABEL}</string>\n\
         \x20   <key>ProgramArguments</key>\n    <array>\n{args}    </array>\n\
         \x20   <key>RunAtLoad</key>\n    <true/>\n\
         </dict>\n</plist>\n"
    );
    write_file(&plist, &text)?;
    quiet(Command::new("launchctl").args(["bootout", &launchd_target()]));
    let domain = format!("gui/{}", uid()?);
    // `bootout` finishes asynchronously; retry while the old job is still unloading.
    for attempt in 0.. {
        if quiet(Command::new("launchctl").args(["bootstrap", &domain]).arg(&plist)) {
            break;
        }
        if attempt == 10 {
            return Err(format!("launchctl could not load {}", plist.display()));
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    Ok(format!("LaunchAgent {}", plist.display()))
}

#[cfg(target_os = "macos")]
pub fn disable_autostart() -> Result<(), String> {
    quiet(Command::new("launchctl").args(["bootout", &launchd_target()]));
    remove_if_exists(&launch_agent_path())
}

/// Registers `daemon` to start at login and starts it. Returns where it was registered.
/// Uses a systemd user service where there is one, else an XDG autostart entry.
#[cfg(not(target_os = "macos"))]
pub fn enable_autostart(daemon: &Path, config: Option<&Path>) -> Result<String, String> {
    let mut args = vec![daemon.to_path_buf()];
    if let Some(c) = config {
        args.extend([PathBuf::from("--config"), c.to_path_buf()]);
    }
    let command: Vec<String> = args.iter().map(|a| quote_exec(&a.to_string_lossy())).collect();
    let command = command.join(" ");

    if quiet(Command::new("systemctl").args(["--user", "show-environment"])) {
        let unit = systemd_unit_path();
        let text = format!(
            "[Unit]\nDescription=skills-sync: keep skills folders linked\n\n\
             [Service]\nExecStart={command}\nRestart=on-failure\n\n\
             [Install]\nWantedBy=default.target\n"
        );
        write_file(&unit, &text)?;
        for step in [&["daemon-reload"][..], &["enable", SYSTEMD_UNIT], &["restart", SYSTEMD_UNIT]] {
            if !quiet(Command::new("systemctl").arg("--user").args(step)) {
                return Err(format!("`systemctl --user {}` failed", step.join(" ")));
            }
        }
        return Ok(format!("systemd user service {SYSTEMD_UNIT}"));
    }

    let entry = autostart_entry_path();
    let text = format!(
        "[Desktop Entry]\nType=Application\nName=skills-sync\nExec={command}\nNoDisplay=true\nTerminal=false\n"
    );
    write_file(&entry, &text)?;
    let mut start = Command::new(daemon);
    if let Some(c) = config {
        start.arg("--config").arg(c);
    }
    start
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // Its own process group, so closing the terminal does not stop it.
        .process_group(0)
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", daemon.display()))?;
    Ok(format!("desktop autostart entry {}", entry.display()))
}

#[cfg(not(target_os = "macos"))]
pub fn disable_autostart() -> Result<(), String> {
    let unit = systemd_unit_path();
    if unit.exists() {
        quiet(Command::new("systemctl").args(["--user", "disable", "--now", SYSTEMD_UNIT]));
        remove_if_exists(&unit)?;
        quiet(Command::new("systemctl").args(["--user", "daemon-reload"]));
    }
    remove_if_exists(&autostart_entry_path())
}

/// Deletes the two binaries from `dir` (`~/.local/bin` holds other programs too).
pub fn remove_binaries(dir: &Path) -> Result<(), String> {
    remove_if_exists(&dir.join(CLI_EXE))?;
    remove_if_exists(&dir.join(DAEMON_EXE))
}

#[cfg(target_os = "macos")]
fn launch_agent_path() -> PathBuf {
    home().join("Library").join("LaunchAgents").join(format!("{LAUNCHD_LABEL}.plist"))
}

#[cfg(target_os = "macos")]
fn launchd_target() -> String {
    format!("gui/{}/{LAUNCHD_LABEL}", uid().unwrap_or_default())
}

#[cfg(target_os = "macos")]
fn uid() -> Result<String, String> {
    let out = Command::new("id").arg("-u").output().map_err(|e| format!("cannot run `id -u`: {e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(target_os = "macos")]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(not(target_os = "macos"))]
fn systemd_unit_path() -> PathBuf {
    xdg_config_home().join("systemd").join("user").join(SYSTEMD_UNIT)
}

#[cfg(not(target_os = "macos"))]
fn autostart_entry_path() -> PathBuf {
    xdg_config_home().join("autostart").join("skills-syncd.desktop")
}

/// One argument of a systemd `ExecStart=` or desktop-entry `Exec=` line.
#[cfg(not(target_os = "macos"))]
fn quote_exec(arg: &str) -> String {
    let escaped = arg.replace('\\', "\\\\").replace('"', "\\\"").replace('`', "\\`").replace('$', "\\$").replace('%', "%%");
    format!("\"{escaped}\"")
}

fn write_file(path: &Path, text: &str) -> Result<(), String> {
    (|| -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, text)
    })()
    .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn remove_if_exists(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(format!("cannot remove {}: {e}", path.display())),
        _ => Ok(()),
    }
}

/// Runs `cmd` with no output; true if it succeeded.
fn quiet(cmd: &mut Command) -> bool {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_dollar_vars() {
        let home = std::env::var("HOME").unwrap();
        assert_eq!(expand_vars("$HOME/a").unwrap(), format!("{home}/a"));
        assert_eq!(expand_vars("${HOME}x").unwrap(), format!("{home}x"));
        assert_eq!(expand_vars("/a$/b").unwrap(), "/a$/b");
        assert!(expand_vars("$NO_SUCH_VAR_SKILLS_SYNC/x").is_err());
    }

    #[test]
    fn resolves_relative_links() {
        let dir = std::env::temp_dir().join(format!("skills-sync-link-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("a").join("b")).unwrap();
        create_link(Path::new("../../src/x"), &dir.join("a").join("b").join("x")).unwrap();
        assert_eq!(link_target(&dir.join("a").join("b").join("x")).unwrap(), dir.join("src").join("x"));
        assert_eq!(link_target(&dir.join("a")), None);
        fs::remove_dir_all(&dir).unwrap();
    }
}
