use crate::session;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
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

/// Where a moved session ended up, and how many references to its old folder
/// were rewritten on the way.
#[derive(Debug)]
pub struct Moved {
    pub path: PathBuf,
    pub replacements: usize,
}

/// Move a session into another folder.
///
/// Two things have to happen together for that to mean anything: the JSONL
/// file has to be filed under the new folder (Claude Code finds sessions by
/// the encoded working directory, not by reading them), and every reference
/// to the old folder inside it has to be rewritten, or the transcript's paths
/// all point at the place the session just left.
///
/// The rewritten copy is put fully in place before the original is unlinked,
/// so an interrupted move leaves the session where it was rather than losing
/// it.
pub fn move_session(
    session_path: &Path,
    old_cwd: Option<&str>,
    new_cwd: &Path,
    claude_dir: &Path,
) -> io::Result<Moved> {
    let file_name = session_path
        .file_name()
        .ok_or_else(|| io::Error::other("session file has no name"))?;
    let destination_dir = session::project_dir(claude_dir, new_cwd);
    let destination = destination_dir.join(file_name);
    if destination.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", destination.display()),
        ));
    }

    let contents = fs::read_to_string(session_path)?;
    let (rewritten, replacements) = match old_cwd {
        Some(old) if !old.is_empty() => rewrite_folder(&contents, old, &new_cwd.to_string_lossy())?,
        // Nothing recorded the old folder, so there is nothing to rewrite -
        // filing the session under the new one is the whole move.
        _ => (contents, 0),
    };

    fs::create_dir_all(&destination_dir)?;

    // Write beside the destination and rename into place so the session is
    // never visible half-written at its new home. The staged name has no
    // `.jsonl` extension, so a scan running concurrently skips it.
    let staged = destination_dir.join(format!(".{}.ccmgr-move", file_name.to_string_lossy()));
    fs::write(&staged, rewritten)?;
    if let Err(error) = fs::rename(&staged, &destination) {
        let _ = fs::remove_file(&staged);
        return Err(error);
    }

    // The sidecar directory holds this session's file-history snapshots, so
    // it travels with it.
    let sidecar = session_path.with_extension("");
    if sidecar.is_dir() {
        if let Err(error) = fs::rename(&sidecar, destination.with_extension("")) {
            let _ = fs::remove_file(&destination);
            return Err(error);
        }
    }

    fs::remove_file(session_path)?;
    Ok(Moved {
        path: destination,
        replacements,
    })
}

/// Rewrite every reference to `old` as `new`, in the escaped form paths take
/// inside JSON records.
fn rewrite_folder(contents: &str, old: &str, new: &str) -> io::Result<(String, usize)> {
    Ok(replace_whole_paths(
        contents,
        &json_fragment(old)?,
        &json_fragment(new)?,
    ))
}

/// How a string looks inside a JSON document, minus the quotes around it.
/// Session records are JSON, so this is the form their paths are stored in -
/// identical to the plain path for ordinary ones, and correctly escaped for
/// the rare path containing a quote or a backslash.
fn json_fragment(value: &str) -> io::Result<String> {
    let quoted = serde_json::to_string(value)?;
    Ok(quoted[1..quoted.len() - 1].to_string())
}

/// Replace `old` with `new`, but only where `old` is a whole path rather than
/// the start of a longer name. Moving `/work/app` must not touch `/work/app-2`
/// or `/work/apps`, which a plain substring replacement would quietly corrupt.
fn replace_whole_paths(haystack: &str, old: &str, new: &str) -> (String, usize) {
    if old.is_empty() {
        return (haystack.to_string(), 0);
    }

    let bytes = haystack.as_bytes();
    let mut out = String::with_capacity(haystack.len());
    let mut cursor = 0;
    let mut replacements = 0;

    while let Some(offset) = haystack[cursor..].find(old) {
        let start = cursor + offset;
        let end = start + old.len();
        let whole_path = (start == 0 || !is_name_byte(bytes[start - 1]))
            && (end == bytes.len() || !is_name_byte(bytes[end]));

        out.push_str(&haystack[cursor..start]);
        if whole_path {
            out.push_str(new);
            replacements += 1;
        } else {
            out.push_str(old);
        }
        cursor = end;
    }

    out.push_str(&haystack[cursor..]);
    (out, replacements)
}

/// Whether a byte could be part of the name of the folder that follows a
/// match - if it is, the match was only a prefix of some other path. Every
/// non-ASCII byte counts, since those only ever appear inside a name.
fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.') || byte >= 0x80
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEMP_PATH: AtomicUsize = AtomicUsize::new(0);

    /// A throwaway `~/.claude/projects` alongside the two real folders a
    /// session moves between. Cleaned up on drop so a failed assertion still
    /// leaves nothing behind.
    struct Fixture {
        root: PathBuf,
        claude_dir: PathBuf,
        old_cwd: PathBuf,
        new_cwd: PathBuf,
        session: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let sequence = NEXT_TEMP_PATH.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir()
                .join(format!("ccmgr-move-test-{}-{sequence}", std::process::id()));
            let claude_dir = root.join("projects");
            let old_cwd = root.join("work").join("app");
            let new_cwd = root.join("work").join("archive");
            fs::create_dir_all(&old_cwd).expect("old folder should be created");
            fs::create_dir_all(&new_cwd).expect("new folder should be created");
            let session = session::project_dir(&claude_dir, &old_cwd).join("session-id.jsonl");
            Fixture {
                root,
                claude_dir,
                old_cwd,
                new_cwd,
                session,
            }
        }

        fn old_path(&self) -> String {
            self.old_cwd.display().to_string()
        }

        fn new_path(&self) -> String {
            self.new_cwd.display().to_string()
        }

        fn write_session(&self, contents: &str) {
            let parent = self.session.parent().expect("session has a project folder");
            fs::create_dir_all(parent).expect("project folder should be created");
            fs::write(&self.session, contents).expect("session fixture should be written");
        }

        fn destination(&self) -> PathBuf {
            session::project_dir(&self.claude_dir, &self.new_cwd).join("session-id.jsonl")
        }

        fn move_it(&self, old_cwd: Option<&str>) -> io::Result<Moved> {
            move_session(&self.session, old_cwd, &self.new_cwd, &self.claude_dir)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn moving_a_session_rewrites_every_reference_to_the_old_folder() {
        let fixture = Fixture::new();
        let (old, new) = (fixture.old_path(), fixture.new_path());
        fixture.write_session(&format!(
            "{{\"type\":\"user\",\"cwd\":\"{old}\",\
               \"message\":{{\"content\":\"edit {old}/src/main.rs\"}}}}\n\
             {{\"type\":\"assistant\",\"cwd\":\"{old}\"}}\n"
        ));

        let moved = fixture.move_it(Some(&old)).expect("move should succeed");

        assert_eq!(moved.replacements, 3);
        assert_eq!(moved.path, fixture.destination());
        assert!(
            !fixture.session.exists(),
            "the session should not be left behind in the old folder"
        );

        let rewritten = fs::read_to_string(&moved.path).expect("moved session should be readable");
        assert!(
            !rewritten.contains(&old),
            "no reference to the old folder should survive: {rewritten}"
        );
        assert!(rewritten.contains(&format!("\"cwd\":\"{new}\"")));
        assert!(rewritten.contains(&format!("edit {new}/src/main.rs")));
    }

    #[test]
    fn a_folder_whose_name_only_starts_the_same_is_left_alone() {
        let fixture = Fixture::new();
        let (old, new) = (fixture.old_path(), fixture.new_path());
        fixture.write_session(&format!(
            "{{\"cwd\":\"{old}\",\"seen\":[\"{old}-2/notes.md\",\"{old}s/index.md\",\"{old}.bak\"]}}\n"
        ));

        let moved = fixture.move_it(Some(&old)).expect("move should succeed");

        assert_eq!(
            moved.replacements, 1,
            "only the folder itself should be rewritten, not its siblings"
        );
        let rewritten = fs::read_to_string(&moved.path).expect("moved session should be readable");
        assert!(rewritten.contains(&format!("\"cwd\":\"{new}\"")));
        for sibling in [
            format!("{old}-2/notes.md"),
            format!("{old}s/index.md"),
            format!("{old}.bak"),
        ] {
            assert!(
                rewritten.contains(&sibling),
                "{sibling} is a different path and should be untouched: {rewritten}"
            );
        }
    }

    #[test]
    fn an_existing_session_at_the_destination_is_never_overwritten() {
        let fixture = Fixture::new();
        let old = fixture.old_path();
        fixture.write_session(&format!("{{\"cwd\":\"{old}\"}}\n"));
        let destination = fixture.destination();
        fs::create_dir_all(destination.parent().unwrap()).expect("destination should be created");
        fs::write(&destination, "already here").expect("occupant should be written");

        let error = fixture
            .move_it(Some(&old))
            .expect_err("an existing session should not be clobbered");

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(
            fixture.session.exists(),
            "the original should survive a refused move"
        );
        assert_eq!(
            fs::read_to_string(&destination).expect("occupant should be readable"),
            "already here"
        );
    }

    #[test]
    fn the_file_history_sidecar_moves_with_the_session() {
        let fixture = Fixture::new();
        let old = fixture.old_path();
        fixture.write_session(&format!("{{\"cwd\":\"{old}\"}}\n"));
        let sidecar = fixture.session.with_extension("");
        fs::create_dir_all(&sidecar).expect("sidecar should be created");
        fs::write(sidecar.join("snapshot.json"), "{}").expect("snapshot should be written");

        let moved = fixture.move_it(Some(&old)).expect("move should succeed");

        assert!(!sidecar.exists(), "the sidecar should not be left behind");
        assert_eq!(
            fs::read_to_string(moved.path.with_extension("").join("snapshot.json"))
                .expect("snapshot should have moved"),
            "{}"
        );
    }

    #[test]
    fn a_session_with_no_recorded_folder_is_filed_under_the_new_one_unchanged() {
        let fixture = Fixture::new();
        let contents = "{\"type\":\"user\",\"message\":{\"content\":\"no cwd here\"}}\n";
        fixture.write_session(contents);

        let moved = fixture.move_it(None).expect("move should succeed");

        assert_eq!(moved.replacements, 0);
        assert_eq!(
            fs::read_to_string(&moved.path).expect("moved session should be readable"),
            contents
        );
    }

    #[test]
    fn paths_are_matched_in_the_escaped_form_json_stores_them_in() {
        let (rewritten, replacements) = rewrite_folder(
            "{\"cwd\":\"C:\\\\work\\\\app\"}",
            "C:\\work\\app",
            "/work/app",
        )
        .expect("escaped paths should be rewritten");

        assert_eq!(replacements, 1);
        assert_eq!(rewritten, "{\"cwd\":\"/work/app\"}");
    }
}
