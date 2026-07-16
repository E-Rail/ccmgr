use std::fs::{self, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

/// Rename a session by appending a new `ai-title` event to its JSONL file,
/// matching Claude Code's own append-only convention (never rewrite existing
/// lines - the last `ai-title` line in the file always wins).
pub fn rename_session(path: &Path, session_id: &str, new_title: &str) -> io::Result<()> {
    let mut file = OpenOptions::new().read(true).append(true).open(path)?;

    // Defensive: make sure we don't glue our new line onto an existing one
    // if the file happens not to end in a trailing newline.
    let len = file.metadata()?.len();
    if len > 0 {
        let mut last_byte = [0u8; 1];
        file.seek(SeekFrom::End(-1))?;
        file.read_exact(&mut last_byte)?;
        if last_byte[0] != b'\n' {
            file.write_all(b"\n")?;
        }
    }

    let line = serde_json::json!({
        "type": "ai-title",
        "aiTitle": new_title,
        "sessionId": session_id,
    });
    writeln!(file, "{}", serde_json::to_string(&line)?)
}

/// Delete a session's JSONL file and its sidecar directory (file-history
/// snapshots), if one exists. Caller is responsible for confirming first.
pub fn delete_session(path: &Path) -> io::Result<()> {
    let sidecar = path.with_extension("");
    if sidecar.is_dir() {
        fs::remove_dir_all(&sidecar)?;
    }
    fs::remove_file(path)
}

/// Replace the current process with `claude --resume <id>`, run from the
/// session's original working directory (session storage is keyed by cwd).
/// Only returns on failure.
pub fn resume_session(session_id: &str, cwd: &Path) -> io::Error {
    Command::new("claude")
        .arg("--resume")
        .arg(session_id)
        .current_dir(cwd)
        .exec()
}
