//! Change Chronology: a newest-first, time-ordered series of individual file
//! changes the agent made, rebuilt from the on-disk session JSONL logs.
//!
//! The agent session logs are the source of truth (see
//! `docs/superpowers/specs/2026-06-05-change-chronology-view-design.md`).
//! This module scans ALL of a workspace's session files (not just the
//! live-tailed active one), extracts one `ChangeEvent` per mutating tool call,
//! and merges them into a timeline cached by each file's `(size, mtime)`.

use crate::activity::events::{encode_cwd, parse_iso8601_ms};
use std::path::{Path, PathBuf};

/// The mutating tool that produced a change. Read and non-mutating tools are
/// never recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeTool {
    Edit,
    MultiEdit,
    Write,
    NotebookEdit,
}

impl ChangeTool {
    /// Compact label for display (`edit` / `write` / …).
    pub fn label(self) -> &'static str {
        match self {
            ChangeTool::Edit => "edit",
            ChangeTool::MultiEdit => "edit",
            ChangeTool::Write => "write",
            ChangeTool::NotebookEdit => "edit",
        }
    }
}

/// Bounded change text retained for the expandable diff peek (C fidelity).
/// `None` when the agent did not expose the changed text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeDetail {
    Edit { old: String, new: String },
    Write { head: String },
    None,
}

/// One change the agent made at one moment in time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeEvent {
    /// Epoch milliseconds, parsed from the JSONL line's `timestamp`.
    pub timestamp_ms: i64,
    pub tool: ChangeTool,
    /// Absolute path as the agent reported it (display layer makes it relative).
    pub file_path: PathBuf,
    /// One-line "what" summary (B fidelity).
    pub summary: String,
    /// Change text for the C-expand peek.
    pub detail: ChangeDetail,
}
