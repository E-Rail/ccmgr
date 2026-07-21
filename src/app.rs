use crate::actions;
use crate::config::Config;
use crate::preview::SessionPreview;
use crate::session::{self, Scope, Session};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

const STATUS_TIMEOUT: Duration = Duration::from_secs(3);

pub enum Mode {
    Browsing,
    Renaming { input: String },
    ConfirmingDelete,
    Previewing { preview: SessionPreview },
}

pub struct App {
    pub config: Arc<Config>,
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
    pub fn new(claude_dir: PathBuf, current_dir: PathBuf, config: Arc<Config>) -> Self {
        let mut app = App {
            config,
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
            Mode::Previewing { .. } => self.handle_preview_key(key),
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
            KeyCode::Char(' ') => {
                self.open_preview();
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

    fn open_preview(&mut self) {
        let Some(session) = self.selected_session() else {
            return;
        };
        let title = session.title.clone();
        let path = session.path.clone();

        match SessionPreview::load(title, &path) {
            Ok(preview) => {
                self.clear_status();
                self.mode = Mode::Previewing { preview };
            }
            Err(error) => {
                self.set_status(format!("Preview failed: {error}"));
            }
        }
    }

    fn handle_preview_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Browsing;
            }
            KeyCode::Enter => {
                self.try_resume();
            }
            KeyCode::Up => {
                if let Mode::Previewing { preview } = &mut self.mode {
                    preview.scroll_from_bottom = preview.scroll_from_bottom.saturating_add(1);
                }
            }
            KeyCode::Down => {
                if let Mode::Previewing { preview } = &mut self.mode {
                    preview.scroll_from_bottom = preview.scroll_from_bottom.saturating_sub(1);
                }
            }
            KeyCode::PageUp => {
                if let Mode::Previewing { preview } = &mut self.mode {
                    preview.scroll_from_bottom = preview.scroll_from_bottom.saturating_add(10);
                }
            }
            KeyCode::PageDown => {
                if let Mode::Previewing { preview } = &mut self.mode {
                    preview.scroll_from_bottom = preview.scroll_from_bottom.saturating_sub(10);
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::SystemTime;

    static NEXT_TEMP_PATH: AtomicUsize = AtomicUsize::new(0);

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn previewable_app() -> (App, PathBuf) {
        let sequence = NEXT_TEMP_PATH.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ccmgr-preview-test-{}-{sequence}.jsonl",
            std::process::id()
        ));
        fs::write(
            &path,
            concat!(
                "{\"type\":\"user\",\"uuid\":\"u1\",\"parentUuid\":null,",
                "\"message\":{\"content\":\"Preview this\"}}\n",
                "{\"type\":\"assistant\",\"uuid\":\"a1\",\"parentUuid\":\"u1\",",
                "\"message\":{\"id\":\"m1\",\"content\":[",
                "{\"type\":\"text\",\"text\":\"Ready\"}]}}\n"
            ),
        )
        .expect("preview fixture should be written");

        let current_dir = std::env::temp_dir();
        let mut app = App::new(
            PathBuf::from("/definitely-not-a-real-claude-dir"),
            current_dir.clone(),
            Arc::new(Config::default()),
        );
        app.sessions = vec![Session {
            id: "session-id".to_string(),
            title: "Previewable session".to_string(),
            cwd: Some(current_dir.display().to_string()),
            git_branch: None,
            size_bytes: 1,
            mtime: SystemTime::now(),
            path: path.clone(),
        }];
        app.filtered = vec![0];
        (app, path)
    }

    #[test]
    fn space_opens_preview_and_escape_restores_browsing() {
        let (mut app, path) = previewable_app();

        app.handle_key(key(KeyCode::Char(' ')));
        let Mode::Previewing { preview } = &app.mode else {
            panic!("space should open the selected session preview");
        };
        assert_eq!(preview.messages.len(), 2);

        app.handle_key(key(KeyCode::Up));
        let Mode::Previewing { preview } = &app.mode else {
            panic!("up should keep preview open");
        };
        assert_eq!(preview.scroll_from_bottom, 1);

        app.handle_key(key(KeyCode::Char(' ')));
        assert!(matches!(app.mode, Mode::Previewing { .. }));

        app.handle_key(key(KeyCode::Esc));
        assert!(matches!(app.mode, Mode::Browsing));

        fs::remove_file(path).expect("preview fixture should be removed");
    }

    #[test]
    fn enter_from_preview_uses_the_existing_resume_path() {
        let (mut app, path) = previewable_app();
        app.handle_key(key(KeyCode::Char(' ')));

        app.handle_key(key(KeyCode::Enter));

        assert!(app.should_quit);
        assert_eq!(
            app.resume_target.as_ref().map(|(id, _)| id.as_str()),
            Some("session-id")
        );
        fs::remove_file(path).expect("preview fixture should be removed");
    }

    #[test]
    fn preview_load_errors_stay_in_browsing_mode() {
        let (mut app, path) = previewable_app();
        fs::remove_file(path).expect("preview fixture should be removed");

        app.handle_key(key(KeyCode::Char(' ')));

        assert!(matches!(app.mode, Mode::Browsing));
        assert!(app
            .status
            .as_deref()
            .is_some_and(|status| status.starts_with("Preview failed:")));
    }
}
