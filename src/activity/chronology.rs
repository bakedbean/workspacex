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

pub(crate) const SUMMARY_MAX_CHARS: usize = 80;

/// True if a line looks like a declaration worth surfacing.
fn looks_like_decl(line: &str) -> bool {
    let t = line.trim_start();
    const KW: [&str; 11] = [
        "fn ", "pub ", "def ", "class ", "struct ", "impl ", "enum ", "trait ",
        "func ", "type ", "const ",
    ];
    KW.iter().any(|k| t.starts_with(k))
}

fn truncate_summary(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= SUMMARY_MAX_CHARS {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(SUMMARY_MAX_CHARS - 1).collect();
    out.push('…');
    out
}

/// Summarize an Edit/MultiEdit: prefer a declaration among lines present in
/// `new` but not `old`; else the first non-blank line of `new` not in `old`;
/// else the first non-blank line of `new`.
pub(crate) fn summarize_edit(old: &str, new: &str) -> String {
    let old_lines: std::collections::HashSet<&str> = old.lines().collect();
    let changed: Vec<&str> = new
        .lines()
        .filter(|l| !old_lines.contains(*l) && !l.trim().is_empty())
        .collect();
    if let Some(decl) = changed.iter().find(|l| looks_like_decl(l)) {
        return truncate_summary(decl);
    }
    if let Some(first) = changed.first() {
        return truncate_summary(first);
    }
    match new.lines().find(|l| !l.trim().is_empty()) {
        Some(l) => truncate_summary(l),
        None => "edit".to_string(),
    }
}

/// Summarize a Write: the first declaration in the content, else "new file".
pub(crate) fn summarize_write(content: &str) -> String {
    match content.lines().find(|l| looks_like_decl(l)) {
        Some(decl) => truncate_summary(decl),
        None => "new file".to_string(),
    }
}

#[cfg(test)]
mod summary_tests {
    use super::*;

    #[test]
    fn prefers_declaration_line() {
        let s = summarize_edit("let x = 1;\n", "let x = 1;\npub fn foo() {}\n");
        assert_eq!(s, "pub fn foo() {}");
    }

    #[test]
    fn falls_back_to_first_nonblank_changed_line() {
        let s = summarize_edit("a = 1\n", "a = 2\n");
        assert_eq!(s, "a = 2");
    }

    #[test]
    fn write_new_file_when_no_decl() {
        let s = summarize_write("plain text\nmore text\n");
        assert_eq!(s, "new file");
    }

    #[test]
    fn write_uses_first_declaration_when_present() {
        let s = summarize_write("# header\nclass Thing:\n    pass\n");
        assert_eq!(s, "class Thing:");
    }

    #[test]
    fn truncates_long_summaries() {
        let long = "x".repeat(200);
        let s = summarize_edit("", &format!("{long}\n"));
        assert!(s.chars().count() <= SUMMARY_MAX_CHARS);
    }
}
