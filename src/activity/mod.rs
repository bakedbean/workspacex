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
    use crate::pty::session::AgentKind;
    use std::path::Path;

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
