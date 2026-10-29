//! Finding `skills` folders through Everything's command-line tool, `es.exe` (Windows only).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const NOT_FOUND: &str = "es.exe (Everything's command-line tool) was not found. \
    For a faster search, install it with `winget install voidtools.Everything.Cli`, \
    or pass `--es <path to es.exe>`.";

/// `explicit`, else `es.exe` on PATH, else next to a standard Everything install.
pub fn find_es(explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(p) = explicit {
        return if p.is_file() { Ok(p.to_path_buf()) } else { Err(format!("{} does not exist", p.display())) };
    }
    let on_path = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).map(|d| d.join("es.exe")).collect::<Vec<_>>())
        .unwrap_or_default();
    let installs = ["ProgramFiles", "ProgramFiles(x86)"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(|d| PathBuf::from(d).join("Everything").join("es.exe"));
    on_path.into_iter().chain(installs).find(|p| p.is_file()).ok_or_else(|| NOT_FOUND.to_string())
}

/// Every folder named exactly `skills` in Everything's index.
pub fn skills_folders(es: &Path) -> Result<Vec<PathBuf>, String> {
    let out = std::env::temp_dir().join(format!("skills-sync-{}.txt", std::process::id()));
    let status = Command::new(es)
        .arg("-export-txt")
        .arg(&out)
        .args(["/ad", "wfn:skills"])
        .status()
        .map_err(|e| format!("cannot run {}: {e}", es.display()))?;
    if !status.success() {
        let _ = fs::remove_file(&out);
        return Err(match status.code() {
            Some(8) => "Everything is not running.".into(),
            code => format!("es.exe failed (exit code {code:?})."),
        });
    }
    let bytes = fs::read(&out).map_err(|e| format!("cannot read es.exe output: {e}"))?;
    let _ = fs::remove_file(&out);
    Ok(decode(&bytes).lines().map(str::trim).filter(|l| !l.is_empty()).map(PathBuf::from).collect())
}

/// es.exe writes UTF-8 or UTF-16 depending on version; accept either.
fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)).collect();
        return String::from_utf16_lossy(&units);
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf8_and_utf16() {
        assert_eq!(decode(b"\xEF\xBB\xBFC:/\xC8\x99\r\n"), "C:/ș\r\n");
        let utf16: Vec<u8> = [0xFF, 0xFE].into_iter().chain("C:/ș".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        assert_eq!(decode(&utf16), "C:/ș");
    }
}
