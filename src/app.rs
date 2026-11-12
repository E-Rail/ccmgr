use crate::actions;
use crate::config::Theme;
use crate::preview::SessionPreview;
use crate::session::{self, Scope, Session};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const STATUS_TIMEOUT: Duration = Duration::from_secs(3);

pub enum Mode {
    Browsing,
    Renaming { input: String },
    Moving { input: String },
    ConfirmingDelete,
    Previewing { preview: SessionPreview },
}

pub struct App {
    pub theme: Theme,
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
    pub fn new(claude_dir: PathBuf, current_dir: PathBuf, theme: Theme) -> Self {
        let mut app = App {
            theme,
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
            Mode::Moving { .. } => self.handle_moving_key(key),
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
            KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Start from where the session lives now, so moving it
                // somewhere nearby is a matter of editing the tail.
                let start = self.selected_session().map(|session| {
                    session
                        .cwd
                        .clone()
                        .unwrap_or_else(|| self.current_dir.display().to_string())
                });
                if let Some(input) = start {
                    self.mode = Mode::Moving { input };
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
            // Only here, at the top level: in a preview or a prompt Esc backs
            // out to this list instead, so it takes a second press to leave.
            KeyCode::Esc => {
                self.should_quit = true;
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

    fn handle_moving_key(&mut self, key: KeyEvent) {
        let mut input = match &self.mode {
            Mode::Moving { input } => input.clone(),
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
                self.commit_move(input);
                return;
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                input.push(c);
            }
            _ => {}
        }
        self.mode = Mode::Moving { input };
    }

    fn commit_move(&mut self, folder: String) {
        self.mode = Mode::Browsing;
        let Some(&idx) = self.filtered.get(self.selected) else {
            return;
        };

        let destination = match resolve_folder(&folder, &self.current_dir) {
            Ok(destination) => destination,
            Err(reason) => {
                self.set_status(reason);
                return;
            }
        };

        let session = self.sessions[idx].clone();
        if session.path.parent()
            == Some(session::project_dir(&self.claude_dir, &destination).as_path())
        {
            self.set_status("That session is already in that folder.");
            return;
        }

        match actions::move_session(
            &session.path,
            session.cwd.as_deref(),
            &destination,
            &self.claude_dir,
        ) {
            Ok(moved) => {
                self.sessions[idx].path = moved.path;
                self.sessions[idx].cwd = Some(destination.display().to_string());
                let name = folder_name(&destination);
                self.set_status(match moved.replacements {
                    0 => format!("Moved to {name}."),
                    1 => format!("Moved to {name} · 1 path updated."),
                    n => format!("Moved to {name} · {n} paths updated."),
                });
                // The session may well have just left the list it was in.
                self.reload();
            }
            Err(e) => {
                self.set_status(format!("Move failed: {e}"));
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

/// Turn what was typed into a folder a session can actually be filed under,
/// or explain why it isn't one. Claude Code keys session storage by absolute
/// working directory, so the folder has to exist before anything is moved
/// into it - canonicalizing both proves that and settles on the same spelling
/// Claude Code itself would record.
fn resolve_folder(input: &str, relative_to: &Path) -> Result<PathBuf, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Move failed: type the folder to move this session to".to_string());
    }

    let expanded = expand_home(trimmed);
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        relative_to.join(expanded)
    };

    let folder = fs::canonicalize(&absolute)
        .map_err(|e| format!("Move failed: {}: {e}", absolute.display()))?;
    if !folder.is_dir() {
        return Err(format!("Move failed: not a folder: {}", folder.display()));
    }
    Ok(folder)
}

/// Expand a leading `~`, since that is how people write their own paths.
/// `~someone-else` is left alone rather than guessed at.
fn expand_home(path: &str) -> PathBuf {
    let Some(rest) = path.strip_prefix('~') else {
        return PathBuf::from(path);
    };
    if !(rest.is_empty() || rest.starts_with('/')) {
        return PathBuf::from(path);
    }
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home).join(rest.trim_start_matches('/')),
        None => PathBuf::from(path),
    }
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| path.display().to_string())
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
            Theme::default(),
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
        assert!(!app.should_quit, "Esc should only close the preview");

        fs::remove_file(path).expect("preview fixture should be removed");
    }

    #[test]
    fn escape_quits_from_the_session_list_without_resuming() {
        let (mut app, path) = previewable_app();

        app.handle_key(key(KeyCode::Esc));

        assert!(app.should_quit);
        assert!(app.resume_target.is_none());
        fs::remove_file(path).expect("preview fixture should be removed");
    }

    #[test]
    fn escape_backs_out_of_a_prompt_before_it_quits() {
        let (mut app, path) = previewable_app();

        for open_prompt in [KeyCode::Char('r'), KeyCode::Char('o'), KeyCode::Char('d')] {
            app.handle_key(KeyEvent::new(open_prompt, KeyModifiers::CONTROL));
            assert!(
                !matches!(app.mode, Mode::Browsing),
                "Ctrl+{open_prompt} should open a prompt"
            );

            app.handle_key(key(KeyCode::Esc));
            assert!(matches!(app.mode, Mode::Browsing));
            assert!(
                !app.should_quit,
                "Esc should cancel the Ctrl+{open_prompt} prompt, not quit"
            );
        }

        app.handle_key(key(KeyCode::Esc));
        assert!(app.should_quit);
        fs::remove_file(path).expect("preview fixture should be removed");
    }

    #[test]
    fn ctrl_c_quits_from_anywhere() {
        let (mut app, path) = previewable_app();
        app.handle_key(key(KeyCode::Char(' ')));
        assert!(matches!(app.mode, Mode::Previewing { .. }));

        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));

        assert!(app.should_quit);
        assert!(app.resume_target.is_none());
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

    /// An app holding one real session on disk, plus a real folder to move it
    /// to. Returns the temp root to clean up and the destination folder.
    fn app_with_a_movable_session() -> (App, PathBuf, PathBuf) {
        let sequence = NEXT_TEMP_PATH.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "ccmgr-app-move-test-{}-{sequence}",
            std::process::id()
        ));
        let claude_dir = root.join("projects");
        let old_cwd = root.join("work").join("app");
        let new_cwd = root.join("work").join("archive");
        fs::create_dir_all(&old_cwd).expect("old folder should be created");
        fs::create_dir_all(&new_cwd).expect("new folder should be created");
        // The temp dir is reached through a symlink on macOS, and the app
        // canonicalizes what it is given, so start from the same spelling.
        let old_cwd = fs::canonicalize(&old_cwd).expect("old folder should canonicalize");
        let new_cwd = fs::canonicalize(&new_cwd).expect("new folder should canonicalize");

        let project = session::project_dir(&claude_dir, &old_cwd);
        fs::create_dir_all(&project).expect("project folder should be created");
        fs::write(
            project.join("session-id.jsonl"),
            format!(
                "{{\"type\":\"user\",\"cwd\":\"{0}\",\
                   \"message\":{{\"content\":\"open {0}/src/main.rs\"}}}}\n",
                old_cwd.display()
            ),
        )
        .expect("session fixture should be written");

        let app = App::new(claude_dir, old_cwd, Theme::default());
        assert_eq!(app.sessions.len(), 1, "the fixture session should be found");
        (app, root, new_cwd)
    }

    #[test]
    fn ctrl_o_opens_the_move_prompt_at_the_sessions_current_folder() {
        let (mut app, root, _) = app_with_a_movable_session();
        let current = app.sessions[0].cwd.clone().expect("fixture records a cwd");

        app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));

        let Mode::Moving { input } = &app.mode else {
            panic!("Ctrl+O should open the move prompt");
        };
        assert_eq!(input, &current);

        app.handle_key(key(KeyCode::Esc));
        assert!(matches!(app.mode, Mode::Browsing));
        fs::remove_dir_all(root).expect("fixture should be removed");
    }

    #[test]
    fn moving_a_session_refiles_it_and_rewrites_the_old_folder() {
        let (mut app, root, destination) = app_with_a_movable_session();
        let origin = app.sessions[0].path.clone();

        app.mode = Mode::Moving {
            input: destination.display().to_string(),
        };
        app.handle_key(key(KeyCode::Enter));

        assert!(matches!(app.mode, Mode::Browsing));
        assert!(
            app.status
                .as_deref()
                .is_some_and(|s| s.starts_with("Moved to archive")),
            "status should confirm the move: {:?}",
            app.status
        );
        assert!(
            !origin.exists(),
            "the session should have left its old folder"
        );

        let moved = session::project_dir(&app.claude_dir, &destination).join("session-id.jsonl");
        let contents = fs::read_to_string(&moved).expect("moved session should be readable");
        assert!(contents.contains(&format!("\"cwd\":\"{}\"", destination.display())));
        assert!(contents.contains(&format!("open {}/src/main.rs", destination.display())));

        // The list is scoped to the folder the session just left.
        assert!(
            app.sessions.is_empty(),
            "a session moved elsewhere should leave the current project's list"
        );
        fs::remove_dir_all(root).expect("fixture should be removed");
    }

    #[test]
    fn moving_to_a_folder_that_does_not_exist_explains_why_and_moves_nothing() {
        let (mut app, root, _) = app_with_a_movable_session();
        let origin = app.sessions[0].path.clone();

        app.mode = Mode::Moving {
            input: root.join("nowhere").display().to_string(),
        };
        app.handle_key(key(KeyCode::Enter));

        assert!(matches!(app.mode, Mode::Browsing));
        assert!(
            app.status
                .as_deref()
                .is_some_and(|s| s.starts_with("Move failed:")),
            "an invalid folder should be reported: {:?}",
            app.status
        );
        assert!(origin.exists(), "the session should not have moved");
        fs::remove_dir_all(root).expect("fixture should be removed");
    }

    #[test]
    fn moving_a_session_to_the_folder_it_is_already_in_is_refused() {
        let (mut app, root, _) = app_with_a_movable_session();
        let origin = app.sessions[0].path.clone();
        let current = app.sessions[0].cwd.clone().expect("fixture records a cwd");

        app.mode = Mode::Moving { input: current };
        app.handle_key(key(KeyCode::Enter));

        assert_eq!(
            app.status.as_deref(),
            Some("That session is already in that folder.")
        );
        assert!(
            origin.exists(),
            "the session should be left exactly where it was"
        );
        fs::remove_dir_all(root).expect("fixture should be removed");
    }

    #[test]
    fn a_typed_folder_is_resolved_against_the_current_directory() {
        let (app, root, destination) = app_with_a_movable_session();

        let resolved = resolve_folder("../archive", &app.current_dir)
            .expect("a relative folder should resolve");

        assert_eq!(resolved, destination);
        assert!(resolve_folder("   ", &app.current_dir).is_err());
        fs::remove_dir_all(root).expect("fixture should be removed");
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
