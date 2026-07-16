# ccmgr — Claude Code session manager

A very simple terminal UI for browsing, searching, renaming, deleting, and
resuming your [Claude Code](https://claude.com/claude-code) CLI sessions.

## Install

**macOS / Linux, via install script:**

```sh
curl -fsSL https://raw.githubusercontent.com/E-Rail/ClaudeCode-Manager/main/install.sh | bash
```

Installs the `ccmgr` binary to `~/.local/bin`.

**Via npm:**

```sh
npm install -g ccmgr
```

**From source:**

```sh
cargo install --path .
```

## Usage

Run `ccmgr` from anywhere:

```sh
ccmgr
```

By default it lists sessions belonging to the current directory's project.

```sh
ccmgr -a, --all     Start showing sessions from all projects, not just the current one
ccmgr -h, --help    Print usage and keybindings
ccmgr uninstall     Remove the installed ccmgr binary
```

| Key | Action |
|---|---|
| `↑` / `↓` | Move selection |
| Type | Live-filter the list by title |
| `Enter` | Resume the selected session (`claude --resume <id>`) |
| `Ctrl+R` | Rename the selected session |
| `Ctrl+D` | Delete the selected session (asks for confirmation) |
| `Tab` | Toggle between the current project and all projects |
| `Ctrl+C` | Quit |

## How it works

`ccmgr` reads Claude Code's own session storage directly
(`~/.claude/projects/<project>/<session-id>.jsonl`) — it doesn't run any
extra background service. Renaming appends a new title event to a
session's log rather than rewriting it, matching how Claude Code itself
records changes.
