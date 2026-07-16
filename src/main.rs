mod actions;
mod app;
mod session;
mod time_fmt;
mod ui;

use app::App;
use crossterm::cursor::Show;
use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use serde_json::Value;
use session::Scope;
use std::fs;
use std::io::{self, Stdout, Write};
use std::os::unix::fs::PermissionsExt;
use std::process::{self, Command};

struct TerminalGuard;

impl TerminalGuard {
    fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        if let Err(e) = execute!(io::stdout(), EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(e);
        }
        Ok(TerminalGuard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return;
    }

    match args.first().map(String::as_str) {
        Some("uninstall") => {
            if let Err(e) = uninstall() {
                eprintln!("ccmgr: {e}");
                process::exit(1);
            }
            return;
        }
        Some("update") => {
            if let Err(e) = update() {
                eprintln!("ccmgr: {e}");
                process::exit(1);
            }
            return;
        }
        _ => {}
    }

    let start_all = args.iter().any(|a| a == "-a" || a == "--all");

    if let Err(e) = run(start_all) {
        eprintln!("ccmgr: {e}");
        process::exit(1);
    }
}

fn print_help() {
    println!(
        "ccmgr - a very simple TUI for managing Claude Code sessions

USAGE:
    ccmgr [OPTIONS]
    ccmgr update
    ccmgr uninstall

OPTIONS:
    -a, --all      Start showing sessions from all projects, not just the current one
    -h, --help     Print this help message

COMMANDS:
    update         Update ccmgr to the latest release
    uninstall      Remove the installed ccmgr binary

KEYS (inside the TUI):
    Up/Down        Move selection
    (type)         Live-filter the list
    Enter          Resume the selected session
    Ctrl+R         Rename the selected session
    Ctrl+D         Delete the selected session (asks for confirmation)
    Tab            Toggle between current project and all projects
    Ctrl+C         Quit"
    );
}

fn uninstall() -> io::Result<()> {
    let exe = std::env::current_exe()?;

    let installed_via_npm = exe.components().any(|c| c.as_os_str() == "node_modules");
    if installed_via_npm {
        println!("ccmgr was installed via npm. Run this instead:");
        println!();
        println!("    npm uninstall -g ccmgr");
        return Ok(());
    }

    print!("Remove {}? [y/N] ", exe.display());
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    if !matches!(input.trim().to_lowercase().as_str(), "y" | "yes") {
        println!("Cancelled.");
        return Ok(());
    }

    fs::remove_file(&exe)?;
    println!("Removed {}", exe.display());
    Ok(())
}

const REPO: &str = "E-Rail/ccmgr";

fn target_triple() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        _ => None,
    }
}

fn other_err(msg: impl Into<String>) -> io::Error {
    io::Error::other(msg.into())
}

fn update() -> io::Result<()> {
    let exe = std::env::current_exe()?;

    let installed_via_npm = exe.components().any(|c| c.as_os_str() == "node_modules");
    if installed_via_npm {
        println!("ccmgr was installed via npm. Run this instead:");
        println!();
        println!("    npm install -g ccmgr@latest");
        return Ok(());
    }

    let target = target_triple().ok_or_else(|| {
        other_err(format!(
            "unsupported platform: {}/{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ))
    })?;

    println!("Checking for updates...");
    let output = Command::new("curl")
        .args([
            "-fsSL",
            &format!("https://api.github.com/repos/{REPO}/releases/latest"),
        ])
        .output()?;
    if !output.status.success() {
        return Err(other_err("failed to look up the latest release"));
    }
    let release: Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| other_err(format!("failed to parse release info: {e}")))?;
    let tag = release
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or_else(|| other_err("release response is missing a tag_name"))?;
    let latest_version = tag.trim_start_matches('v');
    let current_version = env!("CARGO_PKG_VERSION");

    if latest_version == current_version {
        println!("Already up to date (v{current_version}).");
        return Ok(());
    }

    println!("Updating v{current_version} -> v{latest_version}...");

    let asset = format!("ccmgr-{target}.tar.gz");
    let url = format!("https://github.com/{REPO}/releases/download/{tag}/{asset}");

    let tmp_dir = std::env::temp_dir().join(format!("ccmgr-update-{}", std::process::id()));
    fs::create_dir_all(&tmp_dir)?;
    let cleanup = |tmp_dir: &std::path::Path| {
        let _ = fs::remove_dir_all(tmp_dir);
    };

    let tar_path = tmp_dir.join(&asset);
    let status = Command::new("curl")
        .args(["-fsSL", &url, "-o"])
        .arg(&tar_path)
        .status()?;
    if !status.success() {
        cleanup(&tmp_dir);
        return Err(other_err(format!("failed to download {url}")));
    }

    let status = Command::new("tar")
        .args(["-xzf"])
        .arg(&tar_path)
        .args(["-C"])
        .arg(&tmp_dir)
        .status()?;
    if !status.success() {
        cleanup(&tmp_dir);
        return Err(other_err("failed to extract the release archive"));
    }

    let new_binary = tmp_dir.join("ccmgr");
    if !new_binary.is_file() {
        cleanup(&tmp_dir);
        return Err(other_err(
            "downloaded archive did not contain a ccmgr binary",
        ));
    }

    // Stage the new binary next to the current one (same directory, so the
    // final rename is same-filesystem and therefore atomic) before replacing
    // it - this works even though the old binary is the one currently running.
    let install_dir = exe
        .parent()
        .ok_or_else(|| other_err("could not determine the install directory"))?;
    let staged = install_dir.join(".ccmgr.update");
    fs::copy(&new_binary, &staged)?;
    fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))?;
    fs::rename(&staged, &exe)?;
    cleanup(&tmp_dir);

    println!("Updated to v{latest_version}.");
    Ok(())
}

fn run(start_all: bool) -> io::Result<()> {
    let Some(claude_dir) = session::claude_projects_dir() else {
        eprintln!("ccmgr: could not determine home directory ($HOME not set)");
        process::exit(1);
    };
    let current_dir = std::env::current_dir()?;

    let mut app = App::new(claude_dir, current_dir);
    if start_all {
        app.scope = Scope::AllProjects;
        app.reload();
    }

    let guard = TerminalGuard::new()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let result = event_loop(&mut terminal, &mut app);

    // The guard must be dropped explicitly here, before any exec() call.
    // A successful exec() replaces this process image and never returns,
    // so it would silently skip terminal cleanup that would otherwise run
    // via Drop at the end of this function's scope.
    drop(guard);
    drop(terminal);

    result?;

    if let Some((session_id, cwd)) = app.resume_target {
        let err = actions::resume_session(&session_id, &cwd);
        eprintln!("ccmgr: failed to resume session: {err}");
        process::exit(1);
    }

    Ok(())
}

fn event_loop(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            app.handle_key(key);
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}
