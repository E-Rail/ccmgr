use crate::actions;
use crate::session::{self, Scope, Session};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const STATUS_TIMEOUT: Duration = Duration::from_secs(3);

pub enum Mode {
    Browsing,
    Renaming { input: String },
    ConfirmingDelete,
}

pub struct App {
    claude_dir: PathBuf,
    current_dir: PathBuf,
    pub scope: Scope,
    pub sessions: Vec<Session>,
    pub query: String,
    pub filtered: Vec<usize>,
    pub selected: usize,
    pub mode: Mode,
    pub status: Option<String>,
    status_set_at: Option<Instant>,
    pub should_quit: bool,
    pub resume_target: Option<(String, PathBuf)>,
}

impl App {
    pub fn new(claude_dir: PathBuf, current_dir: PathBuf) -> Self {
        let mut app = App {
            claude_dir,
            current_dir,
            scope: Scope::CurrentProject,
            sessions: Vec::new(),
            query: String::new(),
            filtered: Vec::new(),
            selected: 0,
            mode: Mode::Browsing,
            status: None,
            status_set_at: None,
            should_quit: false,
            resume_target: None,
        };
        app.reload();
        app
    }

    fn set_status(&mut self, message: impl Into<String>) {
        self.status = Some(message.into());
        self.status_set_at = Some(Instant::now());
    }

    fn clear_status(&mut self) {
        self.status = None;
        self.status_set_at = None;
    }

    /// Called on every event-loop tick so transient status messages (errors,
    /// "Renamed.", "Deleted.") clear themselves after a few seconds instead
    /// of sitting there until the next action overwrites them.
    pub fn tick(&mut self) {
        if let Some(set_at) = self.status_set_at {
            if set_at.elapsed() >= STATUS_TIMEOUT {
                self.clear_status();
            }
        }
    }

    pub fn reload(&mut self) {
        self.sessions = match self.scope {
            Scope::CurrentProject => {
                session::scan_current_project(&self.claude_dir, &self.current_dir)
            }
            Scope::AllProjects => session::scan_all_projects(&self.claude_dir),
        };
        self.recompute_filter();
    }

    fn recompute_filter(&mut self) {
        let query = self.query.to_lowercase();
        self.filtered = self
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| query.is_empty() || s.title.to_lowercase().contains(&query))
            .map(|(i, _)| i)
            .collect();
        if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len().saturating_sub(1);
        }
    }

    pub fn selected_session(&self) -> Option<&Session> {
        self.filtered.get(self.selected).map(|&i| &self.sessions[i])
    }

    pub fn project_label(&self) -> String {
        match self.scope {
            Scope::CurrentProject => self
                .current_dir
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    let path = self.current_dir.display().to_string();
                    if path.is_empty() {
                        "Current project".to_string()
                    } else {
                        path
                    }
                }),
            Scope::AllProjects => "All projects".to_string(),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        match self.mode {
            Mode::Browsing => self.handle_browsing_key(key),
            Mode::Renaming { .. } => self.handle_renaming_key(key),
            Mode::ConfirmingDelete => self.handle_confirm_delete_key(key),
        }
    }

    fn handle_browsing_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.selected + 1 < self.filtered.len() {
                    self.selected += 1;
                }
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.recompute_filter();
            }
            KeyCode::Tab => {
                self.scope = self.scope.toggled();
                self.query.clear();
                self.selected = 0;
                self.clear_status();
                self.reload();
            }
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(session) = self.selected_session() {
                    self.mode = Mode::Renaming {
                        input: session.title.clone(),
                    };
                }
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.selected_session().is_some() {
                    self.mode = Mode::ConfirmingDelete;
                }
            }
            KeyCode::Enter => {
                self.try_resume();
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.query.push(c);
                self.recompute_filter();
            }
            _ => {}
        }
    }

    fn try_resume(&mut self) {
        let Some(session) = self.selected_session() else {
            return;
        };
        let cwd = match self.scope {
            Scope::CurrentProject => self.current_dir.clone(),
            Scope::AllProjects => match &session.cwd {
                Some(c) => PathBuf::from(c),
                None => {
                    self.set_status("Can't resume: unknown project directory for this session");
                    return;
                }
            },
        };
        if !cwd.is_dir() {
            self.set_status(format!(
                "Can't resume: project directory no longer exists: {}",
                cwd.display()
            ));
            return;
        }
        self.resume_target = Some((session.id.clone(), cwd));
        self.should_quit = true;
    }

    fn handle_renaming_key(&mut self, key: KeyEvent) {
        let mut input = match &self.mode {
            Mode::Renaming { input } => input.clone(),
            _ => return,
        };
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Browsing;
                return;
            }
            KeyCode::Backspace => {
                input.pop();
            }
            KeyCode::Enter => {
                self.commit_rename(input);
                return;
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                input.push(c);
            }
            _ => {}
        }
        self.mode = Mode::Renaming { input };
    }

    fn commit_rename(&mut self, new_title: String) {
        self.mode = Mode::Browsing;
        let trimmed = new_title.trim();
        if trimmed.is_empty() {
            return;
        }
        let Some(&idx) = self.filtered.get(self.selected) else {
            return;
        };
        let session = &self.sessions[idx];
        match actions::rename_session(&session.path, &session.id, trimmed) {
            Ok(()) => {
                self.sessions[idx].title = trimmed.to_string();
                self.set_status("Renamed.");
            }
            Err(e) => {
                self.set_status(format!("Rename failed: {e}"));
            }
        }
    }

    fn handle_confirm_delete_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                self.commit_delete();
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.mode = Mode::Browsing;
            }
            _ => {}
        }
    }

    fn commit_delete(&mut self) {
        self.mode = Mode::Browsing;
        let Some(&idx) = self.filtered.get(self.selected) else {
            return;
        };
        let session = self.sessions[idx].clone();
        match actions::delete_session(&session.path) {
            Ok(()) => {
                self.sessions.remove(idx);
                self.set_status("Deleted.");
                self.recompute_filter();
            }
            Err(e) => {
                self.set_status(format!("Delete failed: {e}"));
            }
        }
    }
}
