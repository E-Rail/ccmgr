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
use session::Scope;
use std::fs;
use std::io::{self, Stdout, Write};
use std::process;

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

    if args.first().map(String::as_str) == Some("uninstall") {
        if let Err(e) = uninstall() {
            eprintln!("ccmgr: {e}");
            process::exit(1);
        }
        return;
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
    ccmgr uninstall

OPTIONS:
    -a, --all      Start showing sessions from all projects, not just the current one
    -h, --help     Print this help message

COMMANDS:
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
