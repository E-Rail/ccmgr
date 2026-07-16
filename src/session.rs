use serde_json::Value;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    CurrentProject,
    AllProjects,
}

impl Scope {
    pub fn toggled(self) -> Scope {
        match self {
            Scope::CurrentProject => Scope::AllProjects,
            Scope::AllProjects => Scope::CurrentProject,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Scope::CurrentProject => "current project",
            Scope::AllProjects => "all projects",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub cwd: Option<String>,
    pub mtime: SystemTime,
    pub path: PathBuf,
}

/// Encode an absolute cwd the same way Claude Code does: replace every `/`
/// with `-`. This is only ever used to locate *our own* current project's
/// directory - it must never be used to decode an arbitrary directory name
/// back into a path, since path segments can themselves contain dashes.
pub fn encode_cwd(path: &Path) -> String {
    path.to_string_lossy().replace('/', "-")
}

pub fn claude_projects_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".claude").join("projects"))
}

pub fn scan_current_project(claude_dir: &Path, current_dir: &Path) -> Vec<Session> {
    let dir = claude_dir.join(encode_cwd(current_dir));
    let mut sessions = scan_dir(&dir);
    sort_sessions(&mut sessions);
    sessions
}

pub fn scan_all_projects(claude_dir: &Path) -> Vec<Session> {
    let mut sessions = Vec::new();
    let Ok(entries) = fs::read_dir(claude_dir) else {
        return sessions;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            sessions.extend(scan_dir(&path));
        }
    }
    sort_sessions(&mut sessions);
    sessions
}

fn sort_sessions(sessions: &mut [Session]) {
    sessions.sort_by_key(|s| std::cmp::Reverse(s.mtime));
}

fn scan_dir(dir: &Path) -> Vec<Session> {
    let mut sessions = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return sessions;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            if let Some(session) = parse_session_file(&path) {
                sessions.push(session);
            }
        }
    }
    sessions
}

fn parse_session_file(path: &Path) -> Option<Session> {
    let id = path.file_stem()?.to_str()?.to_string();
    let mtime = fs::metadata(path)
        .ok()?
        .modified()
        .unwrap_or(SystemTime::now());

    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);

    let mut last_ai_title: Option<String> = None;
    let mut first_user_slug: Option<String> = None;
    let mut first_user_text: Option<String> = None;
    let mut cwd: Option<String> = None;

    for line in reader.lines() {
        let Ok(line) = line else { continue };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        if cwd.is_none() {
            if let Some(c) = value.get("cwd").and_then(Value::as_str) {
                cwd = Some(c.to_string());
            }
        }

        match value.get("type").and_then(Value::as_str) {
            Some("ai-title") => {
                if let Some(t) = value.get("aiTitle").and_then(Value::as_str) {
                    last_ai_title = Some(t.to_string());
                }
            }
            Some("user") => {
                if first_user_slug.is_none() {
                    if let Some(slug) = value.get("slug").and_then(Value::as_str) {
                        first_user_slug = Some(slug.to_string());
                    }
                }
                if first_user_text.is_none() {
                    if let Some(text) = user_message_text(&value) {
                        first_user_text = Some(truncate(&text, 50));
                    }
                }
            }
            _ => {}
        }
    }

    let title = last_ai_title
        .or(first_user_slug)
        .or(first_user_text)
        .unwrap_or_else(|| id.clone());

    Some(Session {
        id,
        title,
        cwd,
        mtime,
        path: path.to_path_buf(),
    })
}

fn user_message_text(value: &Value) -> Option<String> {
    let content = value.get("message")?.get("content")?;
    if let Some(s) = content.as_str() {
        return Some(s.to_string());
    }
    // content can also be an array of blocks like [{"type":"text","text":"..."}]
    if let Some(arr) = content.as_array() {
        for block in arr {
            if let Some(t) = block.get("text").and_then(Value::as_str) {
                return Some(t.to_string());
            }
        }
    }
    None
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max).collect();
        format!("{truncated}…")
    }
}
