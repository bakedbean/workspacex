//! Prior-session detection and Hermes session-resumption plumbing.
//!
//! Answers "does this worktree already have a persisted agent session, and if
//! so which one?" for each [`AgentKind`]. Claude/Pi/Codex detection is
//! filesystem-based; Hermes additionally records a per-worktree spawn marker
//! (under the gitdir) and queries `~/.hermes/state.db`. Re-exported from
//! `pty::session` so existing `crate::pty::session::*` call sites keep working.

use crate::pty::agent_kind::AgentKind;
use crate::pty::session::resolve_gitdir;
use std::path::Path;

/// True if Claude Code has a persisted session JSONL for this worktree.
/// Claude Code stores sessions at `~/.claude/projects/<encoded-cwd>/<uuid>.jsonl`,
/// where the cwd encoding maps every non-alphanumeric character to `-`
/// (see [`crate::activity::events::encode_cwd`], which delegates to sessionx).
pub fn has_prior_session(worktree: &Path) -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    let abs = match std::fs::canonicalize(worktree) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let encoded = crate::activity::events::encode_cwd(&abs);
    let session_dir = home.join(".claude/projects").join(encoded);
    if !session_dir.is_dir() {
        return false;
    }
    std::fs::read_dir(&session_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .any(|e| e.path().extension().map(|x| x == "jsonl").unwrap_or(false))
        })
        .unwrap_or(false)
}

/// Marker recorded when wsx first spawns Hermes for a worktree.
///
/// File format (`.git/info/wsx-hermes-spawn-at`):
/// ```text
/// <start_ts>\n
/// <session_id>\n   ← optional; absent on initial write, added by cache_hermes_session_id_in_marker
/// ```
///
/// Old single-line files (timestamp only) continue to parse correctly with
/// `session_id = None`.
#[derive(Debug, Clone)]
pub(crate) struct HermesSpawnMarker {
    /// Unix epoch seconds (fractional) when wsx first spawned Hermes for this worktree.
    pub(crate) start_ts: f64,
    /// Cached session id discovered by a previous lookup. `None` until the
    /// first successful call to `latest_hermes_session_id_default`.
    pub(crate) session_id: Option<String>,
}

/// Read the spawn marker for this worktree.
/// Returns None if absent or unparseable (best-effort: silent on IO/parse errors).
pub(crate) fn read_hermes_spawn_marker(worktree: &Path) -> Option<HermesSpawnMarker> {
    let path = resolve_gitdir(&worktree.join(".git"), worktree)?.join("info/wsx-hermes-spawn-at");
    let contents = std::fs::read_to_string(&path).ok()?;
    let mut lines = contents.lines();
    let start_ts: f64 = lines.next()?.trim().parse().ok()?;
    let session_id = lines
        .next()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    Some(HermesSpawnMarker {
        start_ts,
        session_id,
    })
}

/// Write a fresh spawn-timestamp marker for this worktree.
///
/// Writes only the first line (`<now>\n`). The `session_id` line is added
/// later by `cache_hermes_session_id_in_marker` once we discover which
/// session Hermes created for this spawn. Callers that want idempotent
/// behaviour must guard the call themselves (see `prepare_hermes_workspace`).
///
/// Best-effort: silent on IO error.
pub(crate) fn write_hermes_spawn_marker(worktree: &Path) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    if let Some(gitdir) = resolve_gitdir(&worktree.join(".git"), worktree) {
        let info_dir = gitdir.join("info");
        if !info_dir.exists() && std::fs::create_dir_all(&info_dir).is_err() {
            return;
        }
        let _ = std::fs::write(info_dir.join("wsx-hermes-spawn-at"), format!("{now}\n"));
    }
}

/// Update the cached session_id in the marker file, preserving the
/// original start_ts. Best-effort: silent on IO error.
fn cache_hermes_session_id_in_marker(worktree: &Path, session_id: &str) {
    let Some(existing) = read_hermes_spawn_marker(worktree) else {
        return;
    };
    if let Some(gitdir) = resolve_gitdir(&worktree.join(".git"), worktree) {
        let info_dir = gitdir.join("info");
        if !info_dir.exists() && std::fs::create_dir_all(&info_dir).is_err() {
            return;
        }
        let _ = std::fs::write(
            info_dir.join("wsx-hermes-spawn-at"),
            format!("{}\n{}\n", existing.start_ts, session_id),
        );
    }
}

/// True if pi has a persisted session JSONL for this worktree.
/// Pi stores sessions at `~/.pi/agent/sessions/--<encoded-cwd>--/<ts>_<uuid>.jsonl`,
/// where the encoding replaces `/` with `-`.
pub fn has_prior_pi_session(worktree: &Path) -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    let abs = match std::fs::canonicalize(worktree) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let encoded = abs.to_string_lossy().replace('/', "-");
    let session_dir = home
        .join(".pi/agent/sessions")
        .join(format!("--{}--", encoded));
    if !session_dir.is_dir() {
        return false;
    }
    std::fs::read_dir(&session_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .any(|e| e.path().extension().map(|x| x == "jsonl").unwrap_or(false))
        })
        .unwrap_or(false)
}

/// Return the most recent Hermes session ID started at or after `spawn_ts`.
/// Path-parameterized for testing; production callers use
/// `latest_hermes_session_id_default`.
///
/// `spawn_ts` is the Unix epoch (seconds, fractional) when wsx spawned
/// Hermes for the worktree of interest. The query uses a 2-second
/// look-back buffer to absorb clock skew between our marker-write time
/// and Hermes's `time.time()` call when it creates the row.
///
/// Opens the db read-only (no `immutable=1`) so the reader sees WAL-pending
/// writes from a live Hermes process. WAL mode supports concurrent
/// readers + 1 writer, so this neither blocks Hermes nor returns stale data.
fn latest_hermes_session_id(db_path: &Path, spawn_ts: f64) -> Option<String> {
    if !db_path.is_file() {
        return None;
    }
    // We open WITHOUT immutable=1 so the reader sees WAL-pending writes from
    // the live Hermes process. WAL mode allows concurrent readers + 1 writer,
    // so plain read-only access is non-blocking and returns the live view.
    // immutable=1 was a previous (wrong) choice that silently filtered out
    // WAL pages and made the dashboard show stale data.
    let uri = format!("file:{}?mode=ro", db_path.display());
    let conn = rusqlite::Connection::open_with_flags(
        &uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .ok()?;
    conn.query_row(
        "SELECT id FROM sessions WHERE started_at >= ?1 - 2.0 ORDER BY started_at DESC LIMIT 1",
        [&spawn_ts],
        |row| row.get::<_, String>(0),
    )
    .ok()
}

/// Production wrapper for `latest_hermes_session_id` that resolves
/// `~/.hermes/state.db` and reads the spawn marker for this worktree.
///
/// Uses a two-level lookup strategy:
/// 1. **Fast path**: if the marker already has a cached `session_id` and that
///    session still exists in the db, return it immediately. This avoids
///    cross-workspace pollution where the time-based query might return a
///    session from a different worktree that was started after this one.
/// 2. **Slow path**: time-based lookup via `latest_hermes_session_id`. On
///    success the result is written back into the marker so future calls use
///    the fast path. If the cached id is stale (session pruned/deleted), this
///    same slow path is used as fallback.
pub fn latest_hermes_session_id_default(worktree: &Path) -> Option<String> {
    let marker = read_hermes_spawn_marker(worktree)?;
    let db = dirs::home_dir()?.join(".hermes/state.db");

    // Fast path: cached session_id that is still alive in the db.
    if let Some(ref id) = marker.session_id {
        if session_exists(&db, id) {
            return Some(id.clone());
        }
        // Cached id is dead (session pruned/deleted); fall through to slow path.
    }

    // Slow path: time-based lookup, then cache the result for next time.
    let id = latest_hermes_session_id(&db, marker.start_ts)?;
    cache_hermes_session_id_in_marker(worktree, &id);
    Some(id)
}

/// Return true if `session_id` exists in the sessions table of `db_path`.
/// Opens the db read-only. Returns false on any IO/parse/query error.
fn session_exists(db_path: &Path, session_id: &str) -> bool {
    if !db_path.is_file() {
        return false;
    }
    let uri = format!("file:{}?mode=ro", db_path.display());
    let Ok(conn) = rusqlite::Connection::open_with_flags(
        &uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    ) else {
        return false;
    };
    conn.query_row("SELECT 1 FROM sessions WHERE id = ?1", [session_id], |_| {
        Ok(())
    })
    .is_ok()
}

/// True if a wsx-spawned Hermes session exists for this worktree.
pub fn has_prior_hermes_session(worktree: &Path) -> bool {
    latest_hermes_session_id_default(worktree).is_some()
}

/// Resolve whether a workspace has a prior session based on the agent kind.
pub fn has_prior_session_for(worktree: &Path, agent: AgentKind) -> bool {
    match agent {
        AgentKind::Claude => has_prior_session(worktree),
        AgentKind::Pi => has_prior_pi_session(worktree),
        AgentKind::Hermes => has_prior_hermes_session(worktree),
        AgentKind::Codex => has_prior_codex_session(worktree),
    }
}

/// True if Codex has a recorded session whose `cwd` matches this worktree.
/// Delegates to `codex_events::locate_session_file`, which scans
/// `~/.codex/sessions` for the newest rollout whose embedded cwd matches.
pub fn has_prior_codex_session(worktree: &Path) -> bool {
    crate::activity::codex_events::locate_session_file(worktree).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::EnvGuard;

    #[test]
    fn has_prior_session_finds_jsonl() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let abs = std::fs::canonicalize(work.path()).unwrap();
        let encoded = crate::activity::events::encode_cwd(&abs);
        let session_dir = home.path().join(".claude/projects").join(&encoded);
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(session_dir.join("abc.jsonl"), "{}").unwrap();

        let mut env = EnvGuard::new();
        env.set("HOME", home.path());
        let result = has_prior_session(work.path());
        assert!(
            result,
            "expected to find prior session at {}",
            session_dir.display()
        );
    }

    #[test]
    fn has_prior_session_finds_jsonl_for_path_with_space() {
        // Regression: a repo whose name contains a space (e.g. "meals backend")
        // yields a worktree path with a space. The encoder must map it to '-'
        // to match the real ~/.claude/projects directory Claude writes.
        let home = tempfile::TempDir::new().unwrap();
        let parent = tempfile::TempDir::new().unwrap();
        let work = parent.path().join("meals backend");
        std::fs::create_dir_all(&work).unwrap();
        let abs = std::fs::canonicalize(&work).unwrap();
        let encoded = crate::activity::events::encode_cwd(&abs);
        let session_dir = home.path().join(".claude/projects").join(&encoded);
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(session_dir.join("abc.jsonl"), "{}").unwrap();

        let mut env = EnvGuard::new();
        env.set("HOME", home.path());
        assert!(
            has_prior_session(&work),
            "expected to find prior session at {}",
            session_dir.display()
        );
    }

    #[test]
    fn has_prior_session_returns_false_for_empty_dir() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let mut env = EnvGuard::new();
        env.set("HOME", home.path());
        let result = has_prior_session(work.path());
        assert!(!result);
    }

    #[test]
    fn has_prior_hermes_session_false_when_no_marker() {
        // A brand-new tempdir has no spawn marker → no session detected.
        let tmp = tempfile::tempdir().unwrap();
        assert!(!super::has_prior_hermes_session(tmp.path()));
    }

    mod hermes_session_lookup {
        use super::latest_hermes_session_id;

        fn make_db(path: &std::path::Path) -> rusqlite::Connection {
            let conn = rusqlite::Connection::open(path).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions (
                    id TEXT PRIMARY KEY,
                    source TEXT NOT NULL,
                    started_at REAL NOT NULL
                );",
            )
            .unwrap();
            conn
        }

        fn insert(conn: &rusqlite::Connection, id: &str, source: &str, started_at: f64) {
            conn.execute(
                "INSERT INTO sessions (id, source, started_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![id, source, started_at],
            )
            .unwrap();
        }

        #[test]
        fn missing_db_returns_none() {
            let tmp = tempfile::tempdir().unwrap();
            let bogus = tmp.path().join("nope.db");
            assert!(latest_hermes_session_id(&bogus, 1000.0).is_none());
        }

        #[test]
        fn empty_sessions_returns_none() {
            let tmp = tempfile::tempdir().unwrap();
            let db_path = tmp.path().join("state.db");
            let _ = make_db(&db_path);
            assert!(latest_hermes_session_id(&db_path, 1000.0).is_none());
        }

        #[test]
        fn session_before_spawn_ts_returns_none() {
            let tmp = tempfile::tempdir().unwrap();
            let db_path = tmp.path().join("state.db");
            let conn = make_db(&db_path);
            insert(&conn, "old", "cli", 100.0);
            // Spawn was way later; even with -2s buffer, this row is too old.
            assert!(latest_hermes_session_id(&db_path, 1000.0).is_none());
        }

        #[test]
        fn session_after_spawn_ts_returns_id() {
            let tmp = tempfile::tempdir().unwrap();
            let db_path = tmp.path().join("state.db");
            let conn = make_db(&db_path);
            insert(&conn, "new", "cli", 1500.0);
            assert_eq!(
                latest_hermes_session_id(&db_path, 1000.0).as_deref(),
                Some("new")
            );
        }

        #[test]
        fn buffer_absorbs_small_clock_skew() {
            // Session row created 1.5s before our marker — buffer covers it.
            let tmp = tempfile::tempdir().unwrap();
            let db_path = tmp.path().join("state.db");
            let conn = make_db(&db_path);
            insert(&conn, "racy", "cli", 998.5);
            assert_eq!(
                latest_hermes_session_id(&db_path, 1000.0).as_deref(),
                Some("racy")
            );
        }

        #[test]
        fn returns_most_recent_when_multiple_match() {
            let tmp = tempfile::tempdir().unwrap();
            let db_path = tmp.path().join("state.db");
            let conn = make_db(&db_path);
            insert(&conn, "first", "cli", 1100.0);
            insert(&conn, "second", "cli", 1200.0);
            insert(&conn, "third", "cli", 1150.0);
            assert_eq!(
                latest_hermes_session_id(&db_path, 1000.0).as_deref(),
                Some("second")
            );
        }

        #[test]
        fn source_irrelevant_to_lookup() {
            // No source filtering; any row in the time range counts.
            let tmp = tempfile::tempdir().unwrap();
            let db_path = tmp.path().join("state.db");
            let conn = make_db(&db_path);
            insert(&conn, "telegram-sess", "telegram", 1500.0);
            assert_eq!(
                latest_hermes_session_id(&db_path, 1000.0).as_deref(),
                Some("telegram-sess")
            );
        }

        #[test]
        fn concurrent_writer_does_not_block_read_in_wal_mode() {
            let tmp = tempfile::tempdir().unwrap();
            let db_path = tmp.path().join("state.db");
            let writer = make_db(&db_path);
            // Switch to WAL mode (matches Hermes's real-world configuration).
            writer.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
            insert(&writer, "committed", "cli", 1000.0);
            // Start an explicit transaction that writes but doesn't commit yet.
            writer.execute_batch("BEGIN IMMEDIATE; INSERT INTO sessions (id, source, started_at) VALUES ('uncommitted', 'cli', 2000.0);").unwrap();

            // Our reader should see the committed row (the WAL pages from earlier commits
            // are visible) but NOT the uncommitted one. spawn_ts=0 sweeps everything.
            let result = latest_hermes_session_id(&db_path, 0.0);
            assert_eq!(
                result.as_deref(),
                Some("committed"),
                "expected to see committed row, not uncommitted; got: {result:?}"
            );

            writer.execute_batch("ROLLBACK;").unwrap();
        }

        #[test]
        fn reader_sees_wal_committed_writes() {
            let tmp = tempfile::tempdir().unwrap();
            let db_path = tmp.path().join("state.db");
            let writer = make_db(&db_path);
            writer.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
            // First commit goes through normal checkpoint behavior.
            insert(&writer, "first", "cli", 1000.0);
            // Subsequent commits land in WAL before checkpoint.
            insert(&writer, "second", "cli", 2000.0);
            insert(&writer, "third", "cli", 3000.0);
            // Without a manual checkpoint, "second" and "third" are WAL-pending.
            // The reader must still see them all.
            let result = latest_hermes_session_id(&db_path, 0.0);
            assert_eq!(
                result.as_deref(),
                Some("third"),
                "expected newest WAL-committed row; got: {result:?}"
            );
        }
    }

    mod hermes_spawn_marker {
        #[test]
        fn write_then_read_roundtrip() {
            let tmp = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(tmp.path().join(".git/info")).unwrap();
            super::write_hermes_spawn_marker(tmp.path());
            let marker =
                super::read_hermes_spawn_marker(tmp.path()).expect("marker should be present");
            // Within 60s of now (sanity check).
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs_f64();
            assert!(
                (now - marker.start_ts).abs() < 60.0,
                "marker ts {} too far from now {now}",
                marker.start_ts
            );
            assert!(
                marker.session_id.is_none(),
                "fresh marker should have no session_id"
            );
        }

        #[test]
        fn read_returns_none_when_absent() {
            let tmp = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(tmp.path().join(".git/info")).unwrap();
            assert!(super::read_hermes_spawn_marker(tmp.path()).is_none());
        }

        #[test]
        fn read_returns_none_when_unparseable() {
            let tmp = tempfile::tempdir().unwrap();
            let info = tmp.path().join(".git/info");
            std::fs::create_dir_all(&info).unwrap();
            std::fs::write(info.join("wsx-hermes-spawn-at"), "not a float\n").unwrap();
            assert!(super::read_hermes_spawn_marker(tmp.path()).is_none());
        }

        #[test]
        fn write_handles_worktree_style_git_file() {
            // `.git` is a file pointing to an external gitdir (real wsx worktree shape).
            let tmp = tempfile::tempdir().unwrap();
            let external = tempfile::tempdir().unwrap();
            let gitdir = external.path().join("worktrees/feature-x");
            std::fs::create_dir_all(&gitdir).unwrap();
            std::fs::write(
                tmp.path().join(".git"),
                format!("gitdir: {}\n", gitdir.display()),
            )
            .unwrap();
            super::write_hermes_spawn_marker(tmp.path());
            let marker = gitdir.join("info/wsx-hermes-spawn-at");
            assert!(marker.exists(), "expected marker at {}", marker.display());
        }

        #[test]
        fn read_tolerates_old_format() {
            // Old single-line format (no trailing newline, no second line) must parse
            // correctly with session_id=None.
            let tmp = tempfile::tempdir().unwrap();
            let info = tmp.path().join(".git/info");
            std::fs::create_dir_all(&info).unwrap();
            std::fs::write(info.join("wsx-hermes-spawn-at"), "1780002798.96").unwrap();
            let marker = super::read_hermes_spawn_marker(tmp.path())
                .expect("old-format marker should parse");
            assert!(
                (marker.start_ts - 1780002798.96).abs() < 0.001,
                "start_ts mismatch: {}",
                marker.start_ts
            );
            assert!(
                marker.session_id.is_none(),
                "old format should yield session_id=None"
            );
        }

        #[test]
        fn cache_session_id_preserves_start_ts() {
            let tmp = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(tmp.path().join(".git/info")).unwrap();
            // Write a marker with a specific timestamp.
            std::fs::write(tmp.path().join(".git/info/wsx-hermes-spawn-at"), "1000.0\n").unwrap();
            // Cache a session id.
            super::cache_hermes_session_id_in_marker(tmp.path(), "abc");
            let marker = super::read_hermes_spawn_marker(tmp.path())
                .expect("marker should exist after cache");
            assert!(
                (marker.start_ts - 1000.0).abs() < 0.001,
                "start_ts should be preserved; got {}",
                marker.start_ts
            );
            assert_eq!(
                marker.session_id.as_deref(),
                Some("abc"),
                "session_id should be cached"
            );
        }

        #[test]
        fn cache_session_id_no_op_when_marker_absent() {
            // tempdir with .git/info set up but no marker file.
            let tmp = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(tmp.path().join(".git/info")).unwrap();
            // Call cache — must not create the marker file.
            super::cache_hermes_session_id_in_marker(tmp.path(), "abc");
            assert!(
                !tmp.path().join(".git/info/wsx-hermes-spawn-at").exists(),
                "cache should not create marker when none exists"
            );
        }
    }
}
