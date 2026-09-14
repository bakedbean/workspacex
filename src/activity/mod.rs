//! Read-only introspection of live agent sessions and OS processes.
//!
//! The Claude Code / Codex / Pi JSONL parsers now live in the `sessionx`
//! crate and are re-exported here so existing `crate::activity::events` (and
//! `codex_events`/`pi_events`) paths keep resolving. `hermes_events`
//! (SQLite-backed, via `~/.hermes/state.db`) and `proc` (lsof) remain
//! wsx-local — they depend on wsx infrastructure, not JSONL files.
//!
//! `omp_events` is wsx-local for a different reason: oh-my-pi writes the same
//! JSONL schema pi does, so only the *location* differs. It reimplements the
//! cwd encoding and re-exports `pi_events::tail_session` unchanged.

pub use sessionx::activity::{codex_events, events, pi_events};

pub mod hermes_events;
pub mod omp_events;
pub mod proc;

use crate::pty::session::AgentKind;
use std::path::{Path, PathBuf};

/// Find the current session transcript for `worktree`, dispatching on the
/// agent kind's on-disk layout. `None` when no session has been recorded.
pub fn locate_session_file_for(kind: AgentKind, worktree: &Path) -> Option<PathBuf> {
    match kind {
        AgentKind::Claude => events::locate_session_file(worktree),
        AgentKind::Pi => pi_events::locate_session_file(worktree),
        AgentKind::Hermes => hermes_events::locate_session_file(worktree),
        AgentKind::Codex => codex_events::locate_session_file(worktree),
        AgentKind::Omp => omp_events::locate_session_file(worktree),
    }
}

/// Locate only this instance's transcript. A recorded identity is authoritative:
/// a missing file never permits borrowing another session in the same cwd.
/// Without an identity, cwd discovery is safe only for a singleton agent kind.
pub(crate) fn locate_instance_session_file(
    instance: &crate::data::agents::AgentInstance,
    worktree: &Path,
    same_kind_count: usize,
) -> Option<PathBuf> {
    use crate::pty::session::{
        claude_session_file, codex_session_file, omp_session_exists, pi_session_file,
    };

    if let Some(id) = instance.agent_session_id.as_deref() {
        return match instance.agent {
            AgentKind::Claude => claude_session_file(worktree, id),
            AgentKind::Pi => pi_session_file(worktree, id),
            AgentKind::Codex => codex_session_file(id),
            AgentKind::Omp => omp_session_exists(id).then(|| PathBuf::from(id)),
            // Hermes has no per-instance session identity in the roster.
            AgentKind::Hermes => None,
        };
    }
    if same_kind_count != 1 {
        return None;
    }
    locate_session_file_for(instance.agent, worktree)
}

/// Tail `path` from byte `offset` with the parser matching the agent kind.
/// Pass `offset = 0` to read the whole transcript.
pub fn tail_session_for(
    kind: AgentKind,
    path: &Path,
    offset: u64,
) -> crate::error::Result<events::TailUpdate> {
    match kind {
        AgentKind::Claude => events::tail_session(path, offset).map_err(Into::into),
        AgentKind::Pi => pi_events::tail_session(path, offset).map_err(Into::into),
        AgentKind::Hermes => hermes_events::tail_session(path, offset),
        AgentKind::Codex => codex_events::tail_session(path, offset).map_err(Into::into),
        AgentKind::Omp => omp_events::tail_session(path, offset).map_err(Into::into),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::agents::AgentInstance;
    use crate::data::store::{AgentInstanceId, WorkspaceId};
    use crate::pty::session::AgentKind;
    use crate::test_support::EnvGuard;
    use std::path::Path;

    fn instance(kind: AgentKind, session_id: Option<String>) -> AgentInstance {
        AgentInstance {
            id: AgentInstanceId(2),
            workspace_id: WorkspaceId(1),
            agent: kind,
            ordinal: 2,
            is_primary: false,
            session_ref: None,
            agent_session_id: session_id,
            created_at: 0,
        }
    }

    fn seed_instance_session(
        home: &Path,
        worktree: &Path,
        kind: AgentKind,
        id: &str,
        modified_secs: u64,
    ) -> (String, PathBuf) {
        let abs = std::fs::canonicalize(worktree).unwrap();
        let path = match kind {
            AgentKind::Claude => home
                .join(".claude/projects")
                .join(events::encode_cwd(&abs))
                .join(format!("{id}.jsonl")),
            AgentKind::Pi => home
                .join(".pi/agent/sessions")
                .join(pi_events::encode_cwd(&abs))
                .join(format!("2026-09-13T10-00-00_{id}.jsonl")),
            AgentKind::Codex => home
                .join(".codex/sessions/2026/09/13")
                .join(format!("rollout-2026-09-13T10-00-00-{id}.jsonl")),
            AgentKind::Omp => omp_events::session_dir(worktree)
                .unwrap()
                .join(format!("2026-09-13T10-00-00_{id}.jsonl")),
            AgentKind::Hermes => unreachable!("Hermes uses SQLite"),
        };
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let meta = serde_json::json!({
            "type": "session_meta",
            "payload": { "id": id, "cwd": abs }
        });
        std::fs::write(&path, format!("{meta}\n")).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(
                std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(modified_secs),
            )
            .unwrap();
        let pin = if kind == AgentKind::Omp {
            path.to_str().unwrap().to_owned()
        } else {
            id.to_owned()
        };
        (pin, path)
    }

    #[test]
    fn instance_locator_uses_exact_identity_not_newest_same_kind_sibling() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let mut env = EnvGuard::new();
        env.set("HOME", home.path());
        for kind in [
            AgentKind::Claude,
            AgentKind::Pi,
            AgentKind::Codex,
            AgentKind::Omp,
        ] {
            let (pin, mine) = seed_instance_session(home.path(), work.path(), kind, "mine", 1);
            seed_instance_session(home.path(), work.path(), kind, "peer-mine", 2);
            assert_eq!(
                locate_instance_session_file(&instance(kind, Some(pin)), work.path(), 2),
                Some(mine),
                "{kind:?} must not select the newer peer"
            );
        }
    }

    #[test]
    fn instance_locator_missing_pin_never_falls_back_to_sibling() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let mut env = EnvGuard::new();
        env.set("HOME", home.path());
        for kind in [
            AgentKind::Claude,
            AgentKind::Pi,
            AgentKind::Codex,
            AgentKind::Omp,
        ] {
            let (pin, mine) = seed_instance_session(home.path(), work.path(), kind, "mine", 1);
            seed_instance_session(home.path(), work.path(), kind, "peer-mine", 2);
            std::fs::remove_file(mine).unwrap();
            assert_eq!(
                locate_instance_session_file(&instance(kind, Some(pin)), work.path(), 1),
                None,
                "{kind:?} must not fall back even when only one instance is registered"
            );
        }
    }

    #[test]
    fn instance_locator_allows_cwd_fallback_only_for_unambiguous_unpinned_kind() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let mut env = EnvGuard::new();
        env.set("HOME", home.path());
        for kind in [
            AgentKind::Claude,
            AgentKind::Pi,
            AgentKind::Codex,
            AgentKind::Omp,
        ] {
            let (_, path) = seed_instance_session(home.path(), work.path(), kind, "peer", 1);
            let unpinned = instance(kind, None);
            assert_eq!(
                locate_instance_session_file(&unpinned, work.path(), 1),
                Some(path),
                "{kind:?} singleton may discover its session"
            );
            assert_eq!(
                locate_instance_session_file(&unpinned, work.path(), 2),
                None,
                "{kind:?} must not borrow a peer's session"
            );
        }
    }

    #[test]
    fn instance_locator_hermes_virtual_path_requires_singleton_kind() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(home.path().join(".hermes")).unwrap();
        let conn = rusqlite::Connection::open(home.path().join(".hermes/state.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (id TEXT PRIMARY KEY, started_at REAL);
             INSERT INTO sessions VALUES ('peer', 100);",
        )
        .unwrap();
        std::fs::create_dir_all(work.path().join(".git/info")).unwrap();
        std::fs::write(
            work.path().join(".git/info/wsx-hermes-spawn-at"),
            "100\npeer\n",
        )
        .unwrap();
        let mut env = EnvGuard::new();
        env.set("HOME", home.path());
        let unpinned = instance(AgentKind::Hermes, None);
        assert_eq!(
            locate_instance_session_file(&unpinned, work.path(), 1),
            Some(PathBuf::from("hermes:peer"))
        );
        assert_eq!(
            locate_instance_session_file(&unpinned, work.path(), 2),
            None
        );
    }

    /// A worktree nobody has ever opened an agent in has no session file
    /// for any kind. Exercises every dispatch arm without a fixture.
    #[test]
    fn locate_returns_none_for_unknown_worktree_for_every_kind() {
        let dir = tempfile::TempDir::new().unwrap();
        for kind in AgentKind::ALL {
            assert!(
                locate_session_file_for(kind, dir.path()).is_none(),
                "{kind:?} should find nothing"
            );
        }
    }

    /// Tailing a nonexistent path must surface an error, not panic, for
    /// every kind (the tail loop treats Err as "skip this tick").
    #[test]
    fn tail_missing_file_is_err_for_every_kind() {
        let missing = Path::new("/nonexistent/wsx-test/session.jsonl");
        for kind in AgentKind::ALL {
            assert!(
                tail_session_for(kind, missing, 0).is_err(),
                "{kind:?} should error on a missing file"
            );
        }
    }
}
