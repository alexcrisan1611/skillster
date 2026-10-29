use std::fs;
use std::io;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegSetKeyValueW};
use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, DETACHED_PROCESS};

pub const CLI_EXE: &str = "skills-sync.exe";
pub const DAEMON_EXE: &str = "skills-syncd.exe";

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_NAME: &str = "skills-sync";

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("USERPROFILE").expect("USERPROFILE is not set"))
}

fn local_appdata() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| home().join("AppData").join("Local"))
}

/// `%APPDATA%\skills-sync`.
pub fn config_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join("AppData").join("Roaming"))
        .join("skills-sync")
}

/// `%LOCALAPPDATA%\skills-sync`: the log and the lock file.
pub fn state_dir() -> PathBuf {
    local_appdata().join("skills-sync")
}

/// `%LOCALAPPDATA%\Programs\skills-sync`.
pub fn install_dir() -> PathBuf {
    local_appdata().join("Programs").join("skills-sync")
}

pub fn create_link(src: &Path, dest: &Path) -> io::Result<()> {
    junction::create(src, dest)
}

/// Where the junction at `p` points, including broken ones (`junction::exists` is
/// false when the target is gone); `None` if `p` is not a junction.
pub fn link_target(p: &Path) -> Option<PathBuf> {
    junction::get_target(p).ok()
}

/// `remove_dir` on a junction deletes the link only, never what it points to.
pub fn remove_link(p: &Path) -> io::Result<()> {
    fs::remove_dir(p)
}

/// Comparable form of a path: no `\\?\` / `\??\` prefix, no trailing slash, lowercase.
pub fn normalize(p: &Path) -> String {
    let s = p.to_string_lossy();
    let s = s.strip_prefix(r"\\?\").or_else(|| s.strip_prefix(r"\??\")).unwrap_or(&s);
    s.trim_end_matches(['\\', '/']).to_lowercase()
}

/// Expands `%VAR%` references.
pub fn expand_vars(s: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find('%').ok_or_else(|| format!("unclosed `%` in {s:?}"))?;
        let name = &after[..end];
        let value = std::env::var_os(name).ok_or_else(|| format!("%{name}% is not set (in {s:?})"))?;
        out.push_str(&value.to_string_lossy());
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Junction targets must use backslashes.
pub fn native_path(s: &str) -> PathBuf {
    PathBuf::from(s.replace('/', "\\"))
}

/// Stops the background process. Builds from before the CLI/daemon split ran in the
/// background as skills-sync.exe, so those are stopped too (sparing this process).
pub fn stop_daemon() {
    let not_me = format!("PID ne {}", std::process::id());
    for exe in [DAEMON_EXE, CLI_EXE] {
        let _ = Command::new("taskkill")
            .args(["/F", "/FI", &format!("IMAGENAME eq {exe}"), "/FI", &not_me])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// Registers `daemon` in `HKCU\...\Run` and starts it. Returns where it was registered.
pub fn enable_autostart(daemon: &Path, config: Option<&Path>) -> Result<String, String> {
    let mut run = format!("\"{}\"", daemon.display());
    if let Some(c) = config {
        run += &format!(" --config \"{}\"", c.display());
    }
    set_run_entry(&run)?;
    let mut start = Command::new(daemon);
    if let Some(c) = config {
        start.arg("--config").arg(c);
    }
    start
        .creation_flags(DETACHED_PROCESS)
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", daemon.display()))?;
    Ok(format!(r"HKCU\{RUN_KEY}\{RUN_NAME}"))
}

pub fn disable_autostart() -> Result<(), String> {
    let err = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), wide(RUN_NAME).as_ptr()) };
    if err == 0 || err == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(format!("cannot remove the Run registry entry (error {err})"))
    }
}

/// Deletes the install folder, even when this exe is running from it.
pub fn remove_binaries(dir: &Path) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    let here = std::env::current_exe().map(|p| normalize(&p)).unwrap_or_default();
    if here.starts_with(&(normalize(dir) + "\\")) {
        // A running exe cannot delete itself; let a detached shell finish once we exit.
        Command::new("cmd")
            .raw_arg(format!("/c ping -n 3 127.0.0.1 >nul & rmdir /s /q \"{}\"", dir.display()))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(drop)
    } else {
        fs::remove_dir_all(dir)
    }
    .map_err(|e| format!("cannot remove {}: {e}", dir.display()))
}

fn set_run_entry(command: &str) -> Result<(), String> {
    let data = wide(command);
    let err = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            wide(RUN_KEY).as_ptr(),
            wide(RUN_NAME).as_ptr(),
            REG_SZ,
            data.as_ptr().cast(),
            (data.len() * 2) as u32,
        )
    };
    if err == 0 { Ok(()) } else { Err(format!("cannot write the Run registry key (error {err})")) }
}

/// NUL-terminated UTF-16.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
