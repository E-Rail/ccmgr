# ccmgr — Claude Code session manager

A very simple terminal UI for browsing, searching, renaming, deleting, and
resuming your [Claude Code](https://claude.com/claude-code) CLI sessions.

## Install

**macOS / Linux, via install script:**

```sh
curl -fsSL https://e-rail.github.io/ccmgr/install.sh | bash
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
ccmgr update        Update ccmgr to the latest release
ccmgr uninstall     Remove the installed ccmgr binary
```

| Key | Action |
|---|---|
| `↑` / `↓` | Move selection |
| Type | Live-filter the list by title |
| `Space` | Preview the selected session |
| `Enter` | Resume the selected session (`claude --resume <id>`) |
| `Ctrl+R` | Rename the selected session |
| `Ctrl+D` | Delete the selected session (asks for confirmation) |
| `Tab` | Toggle between the current project and all projects |
| `Esc` | Close the session preview |
| `Ctrl+C` | Quit |

Inside a preview, use `↑` / `↓` or `PageUp` / `PageDown` to scroll,
`Enter` to resume, and `Esc` to return to the session list.

## Configuration

ccmgr loads an optional TOML config from the platform config directory:

- Linux: `$XDG_CONFIG_HOME/ccmgr/config.toml`, or
  `~/.config/ccmgr/config.toml`
- macOS: `~/Library/Application Support/ccmgr/config.toml`

Set `CCMGR_CONFIG` to use a different file. Everything is optional, so a
config only needs the settings you actually want to change.

### Light and dark terminals

ccmgr never paints its own background — your terminal's background (and any
transparency) shows through. It asks the terminal what that background is at
startup and picks a palette that stays readable against it, so it looks right
on light and dark themes alike without any configuration.

Terminals that don't answer the question are assumed to be dark. To skip the
detection entirely:

```toml
appearance = "dark"   # or "light", or "auto" (the default)
```

### Colors

Any color left unset comes from the palette for the detected appearance:

```toml
[theme]
accent = "#b0b9f9"    # selection, rules, "You" labels
text = "#999999"      # session titles, details, hints
border = "#949699"    # box borders, placeholder text
title = "#da7756"     # the heading and "Claude" labels in a preview
danger = "#e06c75"    # delete confirmation and error status
background = "#282c34"  # unset by default - the terminal's own background
```

Colors accept names such as `"blue"`, RGB hex values, and terminal palette
indexes. Setting `background` opts back into ccmgr painting a solid
background of its own.

## Source layout

- `src/app.rs` owns application state and keyboard behavior.
- `src/appearance.rs` asks the terminal whether it is light or dark.
- `src/config.rs` loads the optional config and defines the two palettes.
- `src/preview.rs` reads Claude JSONL history and builds the active transcript.
- `src/ui.rs` renders the session picker and preview.
- `src/session.rs` discovers sessions; `src/actions.rs` changes or resumes them.

## How it works

`ccmgr` reads Claude Code's own session storage directly
(`~/.claude/projects/<project>/<session-id>.jsonl`) — it doesn't run any
extra background service. Renaming appends a new title event to a
session's log rather than rewriting it, matching how Claude Code itself
records changes.
