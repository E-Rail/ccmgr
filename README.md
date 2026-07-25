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

### Colors

ccmgr draws itself in your terminal's own palette rather than in fixed RGB
values, so whichever scheme you use — Solarized, Gruvbox, Dracula, light or
dark — recolors ccmgr along with everything else. It never paints a
background either, so transparency and background images show through.

The defaults are all palette entries:

```toml
[theme]
text = "reset"        # session titles, details, hints
background = "reset"  # the terminal's own background
accent = "cyan"       # selection, rules, "You" labels
border = "dark-gray"  # box borders, placeholder text
title = "yellow"      # the heading and "Claude" labels in a preview
danger = "red"        # delete confirmation and error status
```

`"reset"` means the terminal's own default foreground or background. Colors
also accept names such as `"blue"`, palette indexes such as `"5"`, and RGB
hex values such as `"#da7756"`. Naming a color pins it to exactly that value
and stops it following your terminal theme, which is the point if you want a
specific look — this restores the fixed palette used before v0.1.9:

```toml
[theme]
background = "#282c34"
accent = "#b0b9f9"
text = "#999999"
border = "#949699"
title = "#da7756"
danger = "#e06c75"
```

## Source layout

- `src/app.rs` owns application state and keyboard behavior.
- `src/config.rs` loads the optional config and defines the default palette.
- `src/preview.rs` reads Claude JSONL history and builds the active transcript.
- `src/ui.rs` renders the session picker and preview.
- `src/session.rs` discovers sessions; `src/actions.rs` changes or resumes them.

## How it works

`ccmgr` reads Claude Code's own session storage directly
(`~/.claude/projects/<project>/<session-id>.jsonl`) — it doesn't run any
extra background service. Renaming appends a new title event to a
session's log rather than rewriting it, matching how Claude Code itself
records changes.
