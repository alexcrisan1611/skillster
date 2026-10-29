# skills-sync

Keeps every skill in one source folder (e.g. `~/.agents/skills`) linked into any number of target folders (e.g. `~/.claude/skills`), the way the `skills` CLI links them: junctions on Windows, symlinks on Linux and macOS. Runs in the background, syncs on every change to the source and rescans every 60 s.

- New skill in the source → link created in every target.
- Skill removed → its link is removed. Only links that point into the source are ever removed; real folders and other links are left alone (and logged once as `skipped`).
- One instance at a time.

Works on Windows, Linux and macOS, on x64 and ARM64.

## Install

**Windows** (PowerShell):

```powershell
irm https://github.com/alexcrisan1611/skills-sync/releases/latest/download/install.ps1 | iex
```

**Linux / macOS**:

```sh
curl -fsSL https://github.com/alexcrisan1611/skills-sync/releases/latest/download/install.sh | sh
```

The installer lists the `skills` folders on your computer and asks which one is the source and which are targets (answer with list numbers or paths). On Windows it searches with [Everything](https://www.voidtools.com/) if Everything is running and its command-line tool is installed (`winget install voidtools.Everything.Cli`; or pass `--es <path to es.exe>`). Otherwise, and on Linux and macOS, it searches your home folder.

It then writes the config, installs `skills-sync` and `skills-syncd`, and starts `skills-syncd` now and at every login:

| | Installed to | Starts at login via | Config | Log |
|---|---|---|---|---|
| Windows | `%LOCALAPPDATA%\Programs\skills-sync` | `HKCU\…\Run\skills-sync` | `%APPDATA%\skills-sync\config.toml` | `%LOCALAPPDATA%\skills-sync\skills-sync.log` |
| Linux | `~/.local/bin` | systemd user service `skills-syncd`, or `~/.config/autostart` without systemd | `~/.config/skills-sync/config.toml` | `~/.local/state/skills-sync/skills-sync.log` |
| macOS | `~/.local/bin` | LaunchAgent `io.github.alexcrisan1611.skills-sync` | `~/.config/skills-sync/config.toml` | `~/Library/Application Support/skills-sync/skills-sync.log` |

Run the installer again to update, or to change the folders. To install from a downloaded archive instead, unpack it and run `skills-sync install` from there.

## Config

Every key is optional. Paths may use `~` and environment variables (`%VAR%` on Windows, `$VAR` elsewhere). Changes apply straight away; a broken file is logged and the last good settings are kept.

```toml
source  = "~/.agents/skills"
targets = ["~/.claude/skills"]

rescan_secs = 60
debounce_ms = 500

[log]
path      = "~/.local/state/skills-sync/skills-sync.log"   # default depends on the OS, see above
max_bytes = 1048576                                         # the log is reset past this size
```

Dropping a target, or changing the source, leaves the links already made in place.

## Commands

```
skills-sync install [--es <path>]   choose folders, install, start
skills-sync uninstall               stop and remove (keeps the config and existing links)
skills-sync once                    sync once and print what changed
skills-sync config                  show the config in use and what it resolves to
skills-sync version
```

All take `--config <path>` to use another config file. `skills-syncd` is the background process and takes `--config` too.

## Build

```sh
cargo build --release      # target/release/skills-sync and skills-syncd
cargo test
```

Pushing a `v*` tag builds every platform and publishes a GitHub release (`.github/workflows/release.yml`).

## Layout

| File | Job |
|---|---|
| `src/main.rs` | `skills-sync`, the command line |
| `src/bin/skills-syncd.rs` | `skills-syncd`, the background process |
| `src/config.rs` | Reading, checking and writing the config |
| `src/sync.rs` | One sync pass |
| `src/watch.rs` | The background loop: file watching, rescans, config reload |
| `src/install.rs` | `install` / `uninstall` |
| `src/find.rs` | Finding `skills` folders for the installer |
| `src/everything.rs` | Asking Everything's `es.exe` (Windows) |
| `src/logger.rs` | The log file |
| `src/platform/` | What differs per OS: links, folders, path syntax, autostart |
