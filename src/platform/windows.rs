use std::fs;
use std::io;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegSetKeyValueW};
use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, DETACHED_PROCESS};

use super::LEGACY_NAME;

pub const CLI_EXE: &str = "skillster.exe";
pub const DAEMON_EXE: &str = "skillsterd.exe";
pub const UI_EXE: &str = "skillster-ui.exe";

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_NAME: &str = "skillster";


pub fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("USERPROFILE").expect("USERPROFILE is not set"))
}

fn local_appdata() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| home().join("AppData").join("Local"))
}

/// `%APPDATA%\skillster`.
pub fn config_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join("AppData").join("Roaming"))
        .join("skillster")
}

/// `%LOCALAPPDATA%\skillster`: the log and the lock file.
pub fn state_dir() -> PathBuf {
    local_appdata().join("skillster")
}

/// Folders whose subfolders each belong to one app: `%APPDATA%`, `%LOCALAPPDATA%`, and
/// `~/.config`, which many cross-platform tools use on Windows too.
pub fn app_data_roots() -> Vec<PathBuf> {
    vec![config_dir().parent().map(Path::to_path_buf).unwrap_or_default(), local_appdata(), home().join(".config")]
}

/// `%LOCALAPPDATA%\Programs\skillster`.
pub fn install_dir() -> PathBuf {
    local_appdata().join("Programs").join("skillster")
}

pub fn create_link(src: &Path, dest: &Path) -> io::Result<()> {
    junction::create(src, dest)
}

/// Where the junction or symbolic link at `p` points, including broken ones
/// (`junction::exists` is false when the target is gone); `None` if `p` is neither.
/// Other tools make symbolic links, so both count.
pub fn link_target(p: &Path) -> Option<PathBuf> {
    if let Ok(t) = junction::get_target(p) {
        return Some(t);
    }
    if !fs::symlink_metadata(p).ok()?.file_type().is_symlink() {
        return None;
    }
    let t = fs::read_link(p).ok()?;
    Some(if t.is_absolute() { t } else { p.parent()?.join(t) })
}

/// Deletes the link only, never what it points to: `remove_dir` for a junction or a
/// folder's symbolic link, `remove_file` for a file's.
pub fn remove_link(p: &Path) -> io::Result<()> {
    fs::remove_dir(p).or_else(|_| fs::remove_file(p))
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

/// Stops the background process.
pub fn stop_daemon() {
    kill(DAEMON_EXE);
}

/// Closes the window, so its binary can be replaced or removed.
pub fn stop_ui() {
    kill(UI_EXE);
}

/// Stops every process running `exe`, sparing this one.
fn kill(exe: &str) {
    let _ = Command::new("taskkill")
        .args(["/F", "/FI", &format!("IMAGENAME eq {exe}"), "/FI", &format!("PID ne {}", std::process::id())])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Stops and removes an install from before the rename to Skillster: its processes
/// (builds from before the CLI/daemon split ran in the background as skills-sync.exe),
/// its Run entry and its install folder. Its config is left for `install` to carry over.
pub fn remove_legacy_install() -> Result<(), String> {
    kill(&format!("{LEGACY_NAME}d.exe"));
    kill(&format!("{LEGACY_NAME}.exe"));
    delete_run_entry(LEGACY_NAME)?;
    let dir = install_dir().with_file_name(LEGACY_NAME);
    // A just-stopped process may hold its exe for a moment.
    let mut attempt = 0;
    while dir.exists() {
        match fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(_) if attempt < 10 => {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(e) => return Err(format!("cannot remove {}: {e}", dir.display())),
        }
    }
    Ok(())
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

/// Starts `daemon` in the background, without registering it.
pub fn start_daemon(daemon: &Path, config: Option<&Path>) -> Result<(), String> {
    let mut start = Command::new(daemon);
    if let Some(c) = config {
        start.arg("--config").arg(c);
    }
    start
        .creation_flags(DETACHED_PROCESS)
        .spawn()
        .map(drop)
        .map_err(|e| format!("cannot start {}: {e}", daemon.display()))
}

/// Shows a folder in Explorer, or opens a file in its usual app.
pub fn open(p: &Path) {
    let _ = Command::new("explorer").arg(p).spawn();
}

/// Whether Windows is set to show fewer animations ("Animation effects" off).
pub fn reduced_motion() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW};
    let mut on: i32 = 1;
    // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL to the pointer given.
    let ok = unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&raw mut on).cast(), 0) };
    ok != 0 && on == 0
}

fn start_menu_shortcut() -> PathBuf {
    config_dir().with_file_name("Microsoft").join(r"Windows\Start Menu\Programs\Skillster.lnk")
}

/// Adds a Start Menu shortcut to the window. Returns where.
pub fn add_app_entry(ui: &Path) -> Result<String, String> {
    let link = start_menu_shortcut();
    let quote = |p: &Path| p.display().to_string().replace('\'', "''");
    let script = format!(
        "$s = (New-Object -ComObject WScript.Shell).CreateShortcut('{}'); $s.TargetPath = '{}'; $s.Save()",
        quote(&link),
        quote(ui)
    );
    let ok = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if ok { Ok(format!("Start Menu ({})", link.display())) } else { Err(format!("cannot create {}", link.display())) }
}

pub fn remove_app_entry() -> Result<(), String> {
    match fs::remove_file(start_menu_shortcut()) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(format!("cannot remove the Start Menu shortcut: {e}")),
        _ => Ok(()),
    }
}

pub fn disable_autostart() -> Result<(), String> {
    delete_run_entry(RUN_NAME)
}

fn delete_run_entry(name: &str) -> Result<(), String> {
    let err = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), wide(name).as_ptr()) };
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
