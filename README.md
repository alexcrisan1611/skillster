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

The installer finds the `skills` folders on your computer and lets you pick, with arrow keys, the source and (with space) the targets; folders that already link to the source come ticked, and you can type other paths. On Windows it asks [Everything](https://www.voidtools.com/) (1.4 or 1.5) if it is running, which takes a moment; otherwise, and on Linux and macOS, it searches your home folder.

Only folders worth syncing are offered: a tool's own skills folder (`~/.claude/skills`, `~/.gemini/antigravity/skills`, `%APPDATA%\<app>\skills`, `~/.config/<app>/skills`) and standalone collections that hold skills (`~/Projects/my-skills/skills`). Plugin and marketplace caches, editor extensions, packages, virtualenvs, temp folders, backups, worktrees and projects' own `.claude/skills` are left out.

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
skills-sync install                 choose folders, install, start
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
| `src/everything.rs` | Asking a running Everything over its IPC (Windows) |
| `src/pick.rs` | The folder menus |
| `src/logger.rs` | The log file |
| `src/platform/` | What differs per OS: links, folders, path syntax, autostart |
