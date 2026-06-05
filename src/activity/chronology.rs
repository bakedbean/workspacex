//! Change Chronology: a newest-first, time-ordered series of individual file
//! changes the agent made, rebuilt from the on-disk session JSONL logs.
//!
//! The agent session logs are the source of truth (see
//! `docs/superpowers/specs/2026-06-05-change-chronology-view-design.md`).
//! This module scans ALL of a workspace's session files (not just the
//! live-tailed active one), extracts one `ChangeEvent` per mutating tool call,
//! and merges them into a timeline cached by each file's `(size, mtime)`.

use crate::activity::events::{encode_cwd, parse_iso8601_ms};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

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

/// Bounded number of characters retained per side of a diff peek.
const DETAIL_MAX_CHARS: usize = 600;

fn clip(s: &str) -> String {
    s.chars().take(DETAIL_MAX_CHARS).collect()
}

fn tool_from_name(name: &str) -> Option<ChangeTool> {
    match name {
        "Edit" => Some(ChangeTool::Edit),
        "MultiEdit" => Some(ChangeTool::MultiEdit),
        "Write" => Some(ChangeTool::Write),
        "NotebookEdit" => Some(ChangeTool::NotebookEdit),
        _ => None,
    }
}

/// Extract zero or more `ChangeEvent`s from one parsed Claude JSONL line.
/// Only `type == "assistant"` lines with mutating `tool_use` blocks produce
/// events. A `MultiEdit` produces one event per element of its `edits` array.
pub fn extract_change_events(v: &serde_json::Value) -> Vec<ChangeEvent> {
    let mut out = Vec::new();
    if v.get("type").and_then(|t| t.as_str()) != Some("assistant") {
        return out;
    }
    let ts = v
        .get("timestamp")
        .and_then(|t| t.as_str())
        .and_then(parse_iso8601_ms)
        .unwrap_or(0);
    let Some(blocks) = v
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
    else {
        return out;
    };
    for block in blocks {
        if block.get("type").and_then(|t| t.as_str()) != Some("tool_use") {
            continue;
        }
        let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let Some(tool) = tool_from_name(name) else {
            continue;
        };
        let input = block.get("input").unwrap_or(&serde_json::Value::Null);
        let file = input
            .get("file_path")
            .or_else(|| input.get("notebook_path"))
            .and_then(|p| p.as_str());
        let Some(file) = file else { continue };
        let file_path = PathBuf::from(file);

        match tool {
            ChangeTool::Write => {
                let content = input.get("content").and_then(|c| c.as_str()).unwrap_or("");
                out.push(ChangeEvent {
                    timestamp_ms: ts,
                    tool,
                    file_path,
                    summary: summarize_write(content),
                    detail: ChangeDetail::Write { head: clip(content) },
                });
            }
            ChangeTool::MultiEdit => {
                let edits = input.get("edits").and_then(|e| e.as_array());
                if let Some(edits) = edits {
                    for e in edits {
                        let old = e.get("old_string").and_then(|s| s.as_str()).unwrap_or("");
                        let new = e.get("new_string").and_then(|s| s.as_str()).unwrap_or("");
                        out.push(ChangeEvent {
                            timestamp_ms: ts,
                            tool,
                            file_path: file_path.clone(),
                            summary: summarize_edit(old, new),
                            detail: ChangeDetail::Edit { old: clip(old), new: clip(new) },
                        });
                    }
                }
            }
            ChangeTool::Edit | ChangeTool::NotebookEdit => {
                let old = input
                    .get("old_string")
                    .and_then(|s| s.as_str())
                    .unwrap_or("");
                let new = input
                    .get("new_string")
                    .or_else(|| input.get("new_source"))
                    .and_then(|s| s.as_str())
                    .unwrap_or("");
                out.push(ChangeEvent {
                    timestamp_ms: ts,
                    tool,
                    file_path,
                    summary: summarize_edit(old, new),
                    detail: ChangeDetail::Edit { old: clip(old), new: clip(new) },
                });
            }
        }
    }
    out
}

/// Summarize a Write: the first declaration in the content, else "new file".
pub(crate) fn summarize_write(content: &str) -> String {
    match content.lines().find(|l| looks_like_decl(l)) {
        Some(decl) => truncate_summary(decl),
        None => "new file".to_string(),
    }
}

#[cfg(test)]
mod extract_tests {
    use super::*;

    fn line(json: &str) -> serde_json::Value {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn extracts_edit_event() {
        let v = line(r#"{"type":"assistant","timestamp":"2026-05-14T17:32:02.744Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"/wt/a.rs","old_string":"let x=1;","new_string":"pub fn foo() {}"}}]}}"#);
        let evs = extract_change_events(&v);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].tool, ChangeTool::Edit);
        assert_eq!(evs[0].file_path, std::path::PathBuf::from("/wt/a.rs"));
        assert_eq!(evs[0].summary, "pub fn foo() {}");
        assert!(matches!(evs[0].detail, ChangeDetail::Edit { .. }));
        assert_eq!(evs[0].timestamp_ms, parse_iso8601_ms("2026-05-14T17:32:02.744Z").unwrap());
    }

    #[test]
    fn extracts_write_event() {
        let v = line(r#"{"type":"assistant","timestamp":"2026-05-14T17:32:02.744Z","message":{"content":[{"type":"tool_use","id":"t2","name":"Write","input":{"file_path":"/wt/new.rs","content":"pub struct Z;"}}]}}"#);
        let evs = extract_change_events(&v);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].tool, ChangeTool::Write);
        assert_eq!(evs[0].summary, "pub struct Z;");
        assert!(matches!(&evs[0].detail, ChangeDetail::Write { head } if head.contains("struct Z")));
    }

    #[test]
    fn multiedit_emits_one_event_per_edit() {
        let v = line(r#"{"type":"assistant","timestamp":"2026-05-14T17:32:02.744Z","message":{"content":[{"type":"tool_use","id":"t3","name":"MultiEdit","input":{"file_path":"/wt/a.rs","edits":[{"old_string":"a","new_string":"pub fn one(){}"},{"old_string":"b","new_string":"pub fn two(){}"}]}}]}}"#);
        let evs = extract_change_events(&v);
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].tool, ChangeTool::MultiEdit);
        assert_eq!(evs[1].summary, "pub fn two(){}");
    }

    #[test]
    fn ignores_read_and_bash() {
        let v = line(r#"{"type":"assistant","timestamp":"2026-05-14T17:32:02.744Z","message":{"content":[{"type":"tool_use","id":"t4","name":"Read","input":{"file_path":"/wt/a.rs"}},{"type":"tool_use","id":"t5","name":"Bash","input":{"command":"ls"}}]}}"#);
        assert!(extract_change_events(&v).is_empty());
    }

    #[test]
    fn ignores_non_assistant_lines() {
        let v = line(r#"{"type":"user","timestamp":"2026-05-14T17:32:02.744Z","message":{"role":"user","content":"hi"}}"#);
        assert!(extract_change_events(&v).is_empty());
    }
}

/// 1-based line to open the editor at, given the file's current contents and
/// the change detail. For an Edit, locate the first line of `old` in `contents`;
/// for a Write (or anything not found), line 1.
pub fn resolve_line(contents: &str, detail: &ChangeDetail) -> u32 {
    let needle = match detail {
        ChangeDetail::Edit { old, .. } => old.lines().find(|l| !l.trim().is_empty()),
        _ => None,
    };
    let Some(needle) = needle else { return 1 };
    for (i, line) in contents.lines().enumerate() {
        if line.contains(needle) {
            return (i + 1) as u32;
        }
    }
    1
}

/// Read the file at `path` and resolve the line for `detail`. Returns 1 when
/// the file can't be read (deleted/renamed since the edit).
pub fn resolve_line_in_file(path: &Path, detail: &ChangeDetail) -> u32 {
    match std::fs::read_to_string(path) {
        Ok(contents) => resolve_line(&contents, detail),
        Err(_) => 1,
    }
}

#[cfg(test)]
mod line_tests {
    use super::*;

    #[test]
    fn finds_line_of_old_string_first_line() {
        let file = "fn a() {}\nfn b() {}\nfn c() {}\n";
        let detail = ChangeDetail::Edit { old: "fn b() {}".into(), new: "fn b2() {}".into() };
        assert_eq!(resolve_line(file, &detail), 2);
    }

    #[test]
    fn write_resolves_to_line_one() {
        let detail = ChangeDetail::Write { head: "anything".into() };
        assert_eq!(resolve_line("whatever\n", &detail), 1);
    }

    #[test]
    fn missing_old_string_falls_back_to_line_one() {
        let detail = ChangeDetail::Edit { old: "nonexistent".into(), new: "x".into() };
        assert_eq!(resolve_line("fn a() {}\n", &detail), 1);
    }

    #[test]
    fn none_detail_falls_back_to_line_one() {
        assert_eq!(resolve_line("fn a() {}\n", &ChangeDetail::None), 1);
    }
}

/// Parse every line of a session file into `ChangeEvent`s. Malformed lines are
/// skipped silently (matches the existing tail-loop tolerance).
pub fn parse_file(path: &Path) -> Vec<ChangeEvent> {
    use std::io::{BufRead, BufReader};
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in BufReader::new(file).lines().map_while(|l| l.ok()) {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
            out.extend(extract_change_events(&v));
        }
    }
    out
}

/// All `.jsonl` session files under `<home>/.claude/projects/<encoded-cwd>/`.
/// Testable variant taking an explicit home dir and canonical worktree path.
pub(crate) fn session_files_in(home: &Path, abs_worktree: &Path) -> Vec<PathBuf> {
    let dir = home
        .join(".claude/projects")
        .join(encode_cwd(abs_worktree));
    let mut files = Vec::new();
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return files;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
    files
}

/// Production entry point: resolve the real home dir and canonical worktree.
pub fn claude_session_files(worktree: &Path) -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let Ok(abs) = std::fs::canonicalize(worktree) else {
        return Vec::new();
    };
    session_files_in(&home, &abs)
}

/// A per-file cache key. Reparse only when size or mtime changes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileStamp {
    size: u64,
    mtime: SystemTime,
}

fn stamp(path: &Path) -> Option<FileStamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some(FileStamp {
        size: meta.len(),
        mtime: meta.modified().ok()?,
    })
}

/// Merged, newest-first chronology of `ChangeEvent`s across a workspace's
/// session files. Caches parsed events per file by `(size, mtime)`.
#[derive(Debug, Default)]
pub struct Timeline {
    /// Per-file parsed events + the stamp they were parsed at.
    per_file: HashMap<PathBuf, (FileStamp, Vec<ChangeEvent>)>,
    /// Flattened, sorted view rebuilt on each refresh.
    merged: Vec<ChangeEvent>,
    /// Test/diagnostic counter of how many file parses have occurred.
    parses: usize,
}

impl Timeline {
    /// Re-scan `files`, reparsing only those whose `(size, mtime)` changed,
    /// dropping cache entries for files no longer present, then rebuild the
    /// merged newest-first view.
    pub fn refresh(&mut self, files: &[PathBuf]) {
        let present: std::collections::HashSet<&PathBuf> = files.iter().collect();
        self.per_file.retain(|p, _| present.contains(p));

        for path in files {
            let Some(st) = stamp(path) else { continue };
            let needs = match self.per_file.get(path) {
                Some((prev, _)) => *prev != st,
                None => true,
            };
            if needs {
                let evs = parse_file(path);
                self.parses += 1;
                self.per_file.insert(path.clone(), (st, evs));
            }
        }

        let mut merged: Vec<ChangeEvent> = self
            .per_file
            .values()
            .flat_map(|(_, evs)| evs.iter().cloned())
            .collect();
        // Newest first; stable so same-timestamp events keep file order.
        merged.sort_by(|a, b| b.timestamp_ms.cmp(&a.timestamp_ms));
        self.merged = merged;
    }

    /// The merged newest-first events.
    pub fn events(&self) -> &[ChangeEvent] {
        &self.merged
    }

    #[cfg(test)]
    pub fn parse_count(&self) -> usize {
        self.parses
    }
}

#[cfg(test)]
mod locate_tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn lists_all_jsonl_files_in_session_dir() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let abs = std::fs::canonicalize(work.path()).unwrap();
        let dir = home.path().join(".claude/projects").join(encode_cwd(&abs));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["a.jsonl", "b.jsonl", "notes.txt"] {
            let mut f = std::fs::File::create(dir.join(name)).unwrap();
            writeln!(f, "{{}}").unwrap();
        }
        let files = session_files_in(home.path(), &abs);
        assert_eq!(files.len(), 2, "only .jsonl files counted");
    }

    #[test]
    fn missing_dir_returns_empty() {
        let home = tempfile::TempDir::new().unwrap();
        let abs = std::path::PathBuf::from("/nonexistent/worktree");
        assert!(session_files_in(home.path(), &abs).is_empty());
    }
}

#[cfg(test)]
mod parse_file_tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parses_events_from_a_jsonl_file_skipping_garbage() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("s.jsonl");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, r#"{{"type":"assistant","timestamp":"2026-05-14T17:00:00.000Z","message":{{"content":[{{"type":"tool_use","name":"Write","input":{{"file_path":"/wt/x.rs","content":"pub fn x(){{}}"}}}}]}}}}"#).unwrap();
        writeln!(f, "not json at all").unwrap();
        writeln!(f, r#"{{"type":"user","message":{{"content":"hi"}}}}"#).unwrap();
        let evs = parse_file(&path);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].tool, ChangeTool::Write);
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

#[cfg(test)]
mod timeline_tests {
    use super::*;
    use std::io::Write;

    fn write_event(path: &Path, ts: &str, file: &str) {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        writeln!(
            f,
            r#"{{"type":"assistant","timestamp":"{ts}","message":{{"content":[{{"type":"tool_use","name":"Write","input":{{"file_path":"{file}","content":"x"}}}}]}}}}"#
        )
        .unwrap();
    }

    #[test]
    fn merges_files_newest_first() {
        let dir = tempfile::TempDir::new().unwrap();
        let a = dir.path().join("a.jsonl");
        let b = dir.path().join("b.jsonl");
        write_event(&a, "2026-05-14T17:00:00.000Z", "/wt/old.rs");
        write_event(&b, "2026-05-14T18:00:00.000Z", "/wt/new.rs");
        let mut tl = Timeline::default();
        tl.refresh(&[a.clone(), b.clone()]);
        let evs = tl.events();
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].file_path, PathBuf::from("/wt/new.rs"), "newest first");
        assert_eq!(evs[1].file_path, PathBuf::from("/wt/old.rs"));
    }

    #[test]
    fn unchanged_file_is_not_reparsed() {
        let dir = tempfile::TempDir::new().unwrap();
        let a = dir.path().join("a.jsonl");
        write_event(&a, "2026-05-14T17:00:00.000Z", "/wt/old.rs");
        let mut tl = Timeline::default();
        tl.refresh(&[a.clone()]);
        assert_eq!(tl.parse_count(), 1);
        tl.refresh(&[a.clone()]); // same size+mtime → cache hit
        assert_eq!(tl.parse_count(), 1, "should not reparse unchanged file");
    }

    #[test]
    fn grown_file_is_reparsed() {
        let dir = tempfile::TempDir::new().unwrap();
        let a = dir.path().join("a.jsonl");
        write_event(&a, "2026-05-14T17:00:00.000Z", "/wt/old.rs");
        let mut tl = Timeline::default();
        tl.refresh(&[a.clone()]);
        write_event(&a, "2026-05-14T19:00:00.000Z", "/wt/newer.rs");
        tl.refresh(&[a.clone()]);
        assert_eq!(tl.parse_count(), 2, "size changed → reparse");
        assert_eq!(tl.events().len(), 2);
    }
}
