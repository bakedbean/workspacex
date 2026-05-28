//! Tail Hermes session events from `~/.hermes/state.db` (sqlite).
//!
//! Hermes stores all sessions in a single sqlite database rather than
//! per-cwd JSONL files. This module provides the two functions that
//! `src/app/background.rs` calls to drive the dashboard's detail-bar
//! modules (RECENT CHAT, SESSION SUMMARY) for Hermes workspaces.
//!
//! ## Virtual-path identity key
//!
//! Because the background tailer uses the file path as a session-change
//! detector (comparing the current "file" against the last-seen one), we
//! produce a virtual path of the form `hermes:<session_id>`. When
//! `latest_hermes_session_id_default` returns a different id (e.g., the
//! user ran `/new` inside Hermes), the virtual path changes, triggering a
//! session reset in the caller just as a JSONL file rotation would.

use std::io;
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::events::{StopReason, TailUpdate};

const HERMES_PREFIX: &str = "hermes:";

/// Returns a virtual "session file" path for the dashboard tailer to use as
/// an identity key. The path encodes the Hermes session id so that
/// `tail_workspace_events`'s file-change detection triggers when the session
/// changes (e.g., user opens a new session via Hermes's /new command).
///
/// Returns None if no wsx-spawned Hermes session exists for this worktree.
pub fn locate_session_file(worktree: &Path) -> Option<PathBuf> {
    let session_id = crate::pty::session::latest_hermes_session_id_default(worktree)?;
    Some(PathBuf::from(format!("{}{}", HERMES_PREFIX, session_id)))
}

/// Tail Hermes session events for the given virtual path, since the last
/// observed messages.id (passed as `from_offset`).
///
/// The virtual path is `hermes:<session_id>` (produced by
/// `locate_session_file`). `from_offset` is the highest `messages.id` we've
/// already processed; 0 means "from the beginning of the session."
///
/// Returns a [`TailUpdate`] populated for MVP fields, or an error if the db
/// can't be opened or the path doesn't start with `hermes:`.
pub fn tail_session(virtual_path: &Path, from_offset: u64) -> Result<TailUpdate> {
    let path_str = virtual_path.to_string_lossy();
    let session_id = path_str
        .strip_prefix(HERMES_PREFIX)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "hermes_events::tail_session: path {:?} does not start with '{}'",
                    virtual_path, HERMES_PREFIX
                ),
            )
        })?;

    let db_path = dirs::home_dir()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "cannot resolve HOME"))?
        .join(".hermes/state.db");

    tail_session_from_db(&db_path, session_id, from_offset)
}

/// Path-parameterized implementation used by both production code (via
/// `tail_session`) and tests (which pass a tempdir db path).
fn tail_session_from_db(
    db_path: &Path,
    session_id: &str,
    from_offset: u64,
) -> Result<TailUpdate> {
    if !db_path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("hermes db not found: {}", db_path.display()),
        )
        .into());
    }

    let uri = format!("file:{}?mode=ro&immutable=1", db_path.display());
    let conn = rusqlite::Connection::open_with_flags(
        &uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )?;

    let mut stmt = conn.prepare(
        "SELECT id, role, content, tool_call_id, tool_calls, tool_name, timestamp, finish_reason \
         FROM messages \
         WHERE session_id = ?1 AND id > ?2 \
         ORDER BY id ASC",
    )?;

    let mut update = TailUpdate {
        new_offset: from_offset,
        ..TailUpdate::default()
    };

    let rows = stmt.query_map(
        rusqlite::params![session_id, from_offset as i64],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,            // id
                row.get::<_, String>(1)?,          // role
                row.get::<_, Option<String>>(2)?,  // content
                row.get::<_, Option<String>>(3)?,  // tool_call_id (unused for MVP)
                row.get::<_, Option<String>>(4)?,  // tool_calls (unused for MVP)
                row.get::<_, Option<String>>(5)?,  // tool_name
                row.get::<_, f64>(6)?,             // timestamp
                row.get::<_, Option<String>>(7)?,  // finish_reason
            ))
        },
    )?;

    for row_result in rows {
        let (id, role, content, _tool_call_id, _tool_calls, tool_name, _timestamp, finish_reason) =
            row_result?;

        // Advance the high-water mark.
        if id > 0 {
            let id_u64 = id as u64;
            if id_u64 > update.new_offset {
                update.new_offset = id_u64;
            }
        }

        match role.as_str() {
            "user" => {
                // Capture the first non-empty user text in this batch.
                if update.first_user_text.is_none() {
                    if let Some(text) = content.as_deref() {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            update.first_user_text = Some(trimmed.to_string());
                        }
                    }
                }
            }
            "assistant" => {
                // Last non-empty assistant text in batch wins.
                if let Some(text) = content.as_deref() {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        let owned = trimmed.to_string();
                        update.longest_assistant_text_in_batch = Some(owned.clone());
                        update.last_assistant_text = Some(owned);
                    }
                }
                // Last finish_reason in batch wins.
                if let Some(reason) = finish_reason.as_deref() {
                    let trimmed = reason.trim();
                    if !trimmed.is_empty() {
                        update.last_stop_reason = Some(StopReason::from_json_str(trimmed));
                    }
                }
            }
            _ => {
                // "tool", "system", and any unknown roles: ignored for MVP.
            }
        }

        // tool_name increments for any row that has a non-empty tool_name
        // (Hermes typically sets this on tool-result rows).
        if let Some(name) = tool_name.as_deref() {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                update.tool_use_counts.increment(trimmed);
            }
        }
    }

    Ok(update)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::EnvGuard;

    // ── helpers ──────────────────────────────────────────────────────────────

    /// Build a minimal Hermes messages+sessions db at `path`.
    fn make_db(path: &Path) -> rusqlite::Connection {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                started_at REAL NOT NULL
            );
            CREATE TABLE messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL REFERENCES sessions(id),
                role TEXT NOT NULL,
                content TEXT,
                tool_call_id TEXT,
                tool_calls TEXT,
                tool_name TEXT,
                timestamp REAL NOT NULL,
                finish_reason TEXT
            );",
        )
        .unwrap();
        conn
    }

    fn insert_session(conn: &rusqlite::Connection, id: &str, source: &str) {
        insert_session_at(conn, id, source, 1000.0);
    }

    fn insert_session_at(conn: &rusqlite::Connection, id: &str, source: &str, started_at: f64) {
        conn.execute(
            "INSERT INTO sessions (id, source, started_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![id, source, started_at],
        )
        .unwrap();
    }

    fn insert_message(
        conn: &rusqlite::Connection,
        session_id: &str,
        role: &str,
        content: Option<&str>,
        tool_name: Option<&str>,
        finish_reason: Option<&str>,
    ) -> i64 {
        conn.execute(
            "INSERT INTO messages (session_id, role, content, tool_name, timestamp, finish_reason)
             VALUES (?1, ?2, ?3, ?4, 1000.0, ?5)",
            rusqlite::params![session_id, role, content, tool_name, finish_reason],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    // ── locate_session_file tests ────────────────────────────────────────────

    #[test]
    fn locate_session_file_returns_hermes_prefixed_path_when_session_exists() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        // Set up spawn marker in worktree's .git/info directory.
        std::fs::create_dir_all(work.path().join(".git/info")).unwrap();
        std::fs::write(work.path().join(".git/info/wsx-hermes-spawn-at"), "1000.0\n").unwrap();
        // Set up db with a session after spawn_ts.
        let db_dir = home.path().join(".hermes");
        std::fs::create_dir_all(&db_dir).unwrap();
        let db_path = db_dir.join("state.db");
        let conn = make_db(&db_path);
        insert_session_at(&conn, "sess-abc", "cli", 1234.5);

        let mut env = EnvGuard::new();
        env.set("HOME", home.path());

        let result = locate_session_file(work.path());
        assert!(result.is_some(), "expected Some but got None");
        let vpath = result.unwrap();
        let s = vpath.to_string_lossy();
        assert_eq!(s, "hermes:sess-abc");
    }

    #[test]
    fn locate_session_file_returns_none_when_no_session() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        // No db at all.
        let mut env = EnvGuard::new();
        env.set("HOME", home.path());

        let result = locate_session_file(work.path());
        assert!(result.is_none(), "expected None but got {result:?}");
    }

    // ── tail_session tests ───────────────────────────────────────────────────

    #[test]
    fn tail_session_returns_last_assistant_text_and_first_user_text() {
        let tmp = tempfile::TempDir::new().unwrap();
        let db_path = tmp.path().join("state.db");
        let conn = make_db(&db_path);
        insert_session(&conn, "s1", "wsx:test");
        insert_message(&conn, "s1", "user", Some("Hello Hermes!"), None, None);
        insert_message(&conn, "s1", "assistant", Some("Hi there!"), None, Some("end_turn"));
        insert_message(&conn, "s1", "assistant", Some("How can I help?"), None, Some("end_turn"));

        let update = tail_session_from_db(&db_path, "s1", 0).unwrap();

        assert_eq!(
            update.last_assistant_text.as_deref(),
            Some("How can I help?"),
            "last assistant text should be the latest"
        );
        assert_eq!(
            update.first_user_text.as_deref(),
            Some("Hello Hermes!"),
            "first user text should be captured"
        );
    }

    #[test]
    fn tail_session_captures_stop_reason() {
        let tmp = tempfile::TempDir::new().unwrap();
        let db_path = tmp.path().join("state.db");
        let conn = make_db(&db_path);
        insert_session(&conn, "s2", "wsx:test");
        insert_message(&conn, "s2", "assistant", Some("Done."), None, Some("end_turn"));

        let update = tail_session_from_db(&db_path, "s2", 0).unwrap();
        assert!(
            matches!(update.last_stop_reason, Some(StopReason::EndTurn)),
            "expected EndTurn stop reason, got {:?}",
            update.last_stop_reason
        );
    }

    #[test]
    fn tail_session_advances_new_offset_to_max_id() {
        let tmp = tempfile::TempDir::new().unwrap();
        let db_path = tmp.path().join("state.db");
        let conn = make_db(&db_path);
        insert_session(&conn, "s3", "wsx:test");
        insert_message(&conn, "s3", "user", Some("msg1"), None, None);
        let last_id = insert_message(&conn, "s3", "assistant", Some("resp1"), None, Some("end_turn"));

        let update = tail_session_from_db(&db_path, "s3", 0).unwrap();
        assert_eq!(
            update.new_offset, last_id as u64,
            "new_offset should equal the last messages.id"
        );
    }

    #[test]
    fn tail_session_second_call_with_same_offset_returns_empty_batch() {
        let tmp = tempfile::TempDir::new().unwrap();
        let db_path = tmp.path().join("state.db");
        let conn = make_db(&db_path);
        insert_session(&conn, "s4", "wsx:test");
        insert_message(&conn, "s4", "user", Some("prompt"), None, None);
        insert_message(&conn, "s4", "assistant", Some("answer"), None, Some("end_turn"));

        // First call — consume all rows.
        let first = tail_session_from_db(&db_path, "s4", 0).unwrap();
        assert!(first.new_offset > 0);

        // Second call with the advanced offset — should see no new rows.
        let second = tail_session_from_db(&db_path, "s4", first.new_offset).unwrap();
        assert_eq!(
            second.new_offset, first.new_offset,
            "offset should not advance when no new rows"
        );
        assert!(second.last_assistant_text.is_none());
        assert!(second.first_user_text.is_none());
        assert!(second.last_stop_reason.is_none());
    }

    #[test]
    fn tail_session_increments_tool_use_counts_for_tool_name_rows() {
        let tmp = tempfile::TempDir::new().unwrap();
        let db_path = tmp.path().join("state.db");
        let conn = make_db(&db_path);
        insert_session(&conn, "s5", "wsx:test");
        insert_message(&conn, "s5", "tool", None, Some("bash"), None);
        insert_message(&conn, "s5", "tool", None, Some("bash"), None);
        insert_message(&conn, "s5", "tool", None, Some("read_file"), None);

        let update = tail_session_from_db(&db_path, "s5", 0).unwrap();
        // Hermes tool names are lowercase ("bash", "read_file", etc.) while
        // ToolUseCounts::increment is case-sensitive and Claude-flavored
        // ("Bash", "Read", etc.). MVP: all Hermes tool uses count as "other".
        // Full categorization is a follow-up.
        assert_eq!(update.tool_use_counts.other, 3, "expected 3 other tool uses (MVP: hermes names are lowercase, all fall through to other)");
        assert_eq!(update.tool_use_counts.bash, 0, "bash bucket empty until hermes tool name normalization is added");
    }

    #[test]
    fn tail_session_errors_on_non_hermes_path() {
        let result = tail_session(Path::new("/some/real/file.jsonl"), 0);
        assert!(result.is_err(), "expected Err for non-hermes: path");
    }

    #[test]
    fn tail_session_last_assistant_wins_over_earlier_in_batch() {
        let tmp = tempfile::TempDir::new().unwrap();
        let db_path = tmp.path().join("state.db");
        let conn = make_db(&db_path);
        insert_session(&conn, "s6", "wsx:test");
        insert_message(&conn, "s6", "assistant", Some("first response"), None, Some("tool_use"));
        insert_message(&conn, "s6", "assistant", Some("second response"), None, Some("end_turn"));

        let update = tail_session_from_db(&db_path, "s6", 0).unwrap();
        assert_eq!(
            update.last_assistant_text.as_deref(),
            Some("second response"),
            "last assistant text in batch should win"
        );
        assert!(
            matches!(update.last_stop_reason, Some(StopReason::EndTurn)),
            "last stop reason should be end_turn (last in batch)"
        );
    }

    #[test]
    fn tail_session_empty_session_from_zero_returns_zero_offset_and_empty_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("state.db");
        let conn = make_db(&db_path);
        insert_session(&conn, "sess-empty", "wsx:test");
        // No messages inserted.

        let update = super::tail_session_from_db(&db_path, "sess-empty", 0).unwrap();
        assert_eq!(update.new_offset, 0);
        assert!(update.last_assistant_text.is_none());
        assert!(update.first_user_text.is_none());
        assert!(update.last_stop_reason.is_none());
    }
}
