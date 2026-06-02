//! Tail Codex CLI session events from `~/.codex/sessions/**/rollout-*.jsonl`.
//!
//! Codex rollout files are date-partitioned (`YYYY/MM/DD/`) and store the
//! originating directory INSIDE the file (first line is `session_meta` with a
//! `cwd` field), so locating "this worktree's session" matches by content,
//! not by directory path. Real implementations land in later tasks.

use crate::activity::events::TailUpdate;
use crate::error::Result;
use std::path::{Path, PathBuf};

/// Locate the newest Codex rollout file whose recorded `cwd` matches `worktree`.
/// STUB — real implementation in the locate task.
pub fn locate_session_file(_worktree: &Path) -> Option<PathBuf> {
    None
}

/// Tail Codex rollout JSONL from `offset`. STUB — real implementation in the
/// tail/parse task.
pub fn tail_session(_path: &Path, offset: u64) -> Result<TailUpdate> {
    Ok(TailUpdate {
        new_offset: offset,
        ..TailUpdate::default()
    })
}
