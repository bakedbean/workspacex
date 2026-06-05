# Change Chronology View Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a toggleable vertical bar in wsx's attached view showing a newest-first, time-ordered chronology of individual file changes the agent made, expandable to a diff peek and clickable to open the editor at the changed line.

**Architecture:** Approach 1 — the on-disk agent session JSONL logs are the source of truth. A standalone module scans all of a workspace's session logs, extracts a `ChangeEvent` per mutating tool call, merges them into a cached, newest-first timeline, and the attached view carves a configurable side column to render it. No new events table; config mirrors the existing `detail_bar_config` global-blob + per-repo-override pattern.

**Tech Stack:** Rust, `ratatui` (TUI), `rusqlite` (settings/repos), `serde_json` (JSONL parsing), `shlex` (editor command parsing). Tests are standard `#[cfg(test)]` unit tests run with `cargo test`.

**Design note on decomposition:** The brainstorming spec listed "extend the existing parsers" under files touched. During planning this is refined to a **standalone extractor** in `src/activity/chronology.rs`: whole-history requires scanning *all* session files (not just the live-tailed active one), so a self-contained scanner that re-uses the existing public helpers (`encode_cwd`, `parse_iso8601_ms`) is lower-risk than threading a new event type through four live tail loops. Behavior matches the spec; only the realization differs.

**Agent sequencing:** Claude is implemented end-to-end first (richest, fully-specified JSONL). Codex/Pi/Hermes extraction are separate, clearly-bounded tasks (Phase 8) that reuse the shared `ChangeEvent` types and each parser's existing file-path extraction. Until those land, non-Claude agents simply show an empty chronology (em-dash placeholder) — no crash, no fabricated data.

---

## File Structure

- `src/commands/external.rs` (modify) — add `{file}`/`{line}` placeholders + `open_in_editor_at`.
- `src/activity/chronology.rs` (create) — `ChangeEvent`/`ChangeTool`/`ChangeDetail` types, Claude extraction, summary heuristic, line resolution, session-file enumeration, timeline build/merge + `(size, mtime)` cache.
- `src/activity/mod.rs` (modify) — `pub mod chronology;`.
- `src/config/chronology.rs` (create) — `ChronologyConfig`/`WidthSpec`/`Side`, `Default`/`with_override`/`sanitize`, `resolve_global_only`/`resolve`.
- `src/config/mod.rs` (modify) — `pub mod chronology;`.
- `src/data/store.rs` (modify) — `repos.chronology_config` column + migration + `Repo` field + `set_repo_chronology_config`.
- `src/cli.rs` (modify) — `chronology_config` config key (validate/normalize/seed) + valid-keys list.
- `src/ui/chronology_bar.rs` (create) — pure rendering helpers (entry → `Vec<Line>`, width math, auto-hide).
- `src/ui/attached.rs` (modify) — carve the side column, draw the bar + divider, return per-entry click rects.
- `src/ui/mod.rs` (modify) — `pub mod chronology_bar;`.
- `src/app/input.rs` (modify) — `Ctrl-x c` (toggle), `Ctrl-x C` (swap side), scroll, click → expand/open.
- `src/app/render.rs` / `src/app.rs` (modify) — hold timeline + bar UI state, resolve config, pass to renderer.
- `README.md` (modify) — document feature, keybindings, config.

---

## Phase 1 — Editor open at file:line

### Task 1: `{file}`/`{line}` placeholders + `open_in_editor_at`

**Files:**
- Modify: `src/commands/external.rs`
- Test: same file's `#[cfg(test)] mod tests`

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/commands/external.rs`:

```rust
#[test]
fn editor_at_substitutes_file_and_line_placeholders() {
    let argv = resolve_editor_at_argv(
        "code --goto {file}:{line}",
        "/tmp/wt/src/main.rs",
        42,
    )
    .unwrap();
    assert_eq!(argv, vec!["code", "--goto", "/tmp/wt/src/main.rs:42"]);
}

#[test]
fn editor_at_vim_fallback_uses_plus_line() {
    let argv = resolve_editor_at_argv("nvim", "/tmp/wt/src/main.rs", 42).unwrap();
    assert_eq!(argv, vec!["nvim", "+42", "/tmp/wt/src/main.rs"]);
}

#[test]
fn editor_at_code_fallback_uses_goto() {
    let argv = resolve_editor_at_argv("code", "/tmp/wt/src/main.rs", 7).unwrap();
    assert_eq!(argv, vec!["code", "--goto", "/tmp/wt/src/main.rs:7"]);
}

#[test]
fn editor_at_emacs_fallback_uses_plus_line() {
    let argv = resolve_editor_at_argv("emacsclient", "/tmp/wt/a.rs", 3).unwrap();
    assert_eq!(argv, vec!["emacsclient", "+3", "/tmp/wt/a.rs"]);
}

#[test]
fn editor_at_unknown_editor_appends_file_only() {
    let argv = resolve_editor_at_argv("myeditor", "/tmp/wt/a.rs", 3).unwrap();
    assert_eq!(argv, vec!["myeditor", "/tmp/wt/a.rs"]);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib editor_at_`
Expected: FAIL — `cannot find function resolve_editor_at_argv`.

- [ ] **Step 3: Implement the resolver**

Add to `src/commands/external.rs` (non-test scope):

```rust
/// Resolve the editor command into argv that opens `file` at `line`.
///
/// If the template contains `{file}`/`{line}` placeholders, substitute them.
/// Otherwise fall back to the goto convention of common editors detected from
/// the program name: VS Code (`code --goto file:line`), vim/nvim/vi
/// (`+line file`), emacs/emacsclient (`+line file`); any other editor gets the
/// file appended with the line omitted.
fn resolve_editor_at_argv(cmd: &str, file: &str, line: u32) -> Result<Vec<String>> {
    let line_s = line.to_string();
    let mut parts = shlex::split(cmd)
        .ok_or_else(|| Error::UserInput(format!("could not parse command: {cmd}")))?;
    if parts.is_empty() {
        return Err(Error::UserInput("command is empty".into()));
    }
    let used_placeholder = parts
        .iter()
        .any(|p| p.contains("{file}") || p.contains("{line}"));
    if used_placeholder {
        for part in &mut parts {
            *part = part.replace("{file}", file).replace("{line}", &line_s);
        }
        return Ok(parts);
    }
    let prog = std::path::Path::new(&parts[0])
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&parts[0])
        .to_string();
    match prog.as_str() {
        "code" | "codium" | "cursor" => parts.push(format!("{file}:{line_s}")),
        "vim" | "nvim" | "vi" | "emacs" | "emacsclient" => {
            parts.push(format!("+{line_s}"));
            parts.push(file.to_string());
        }
        _ => parts.push(file.to_string()),
    }
    Ok(parts)
}

/// Resolve and launch the user's editor on `file`, positioned at `line`.
/// Spawns with cwd = `worktree`. Used by the chronology bar's entry clicks.
pub fn open_in_editor_at(
    worktree: &Path,
    file: &Path,
    line: u32,
    configured: Option<&str>,
) -> Result<()> {
    let cmd = resolve_editor_cmd(configured)?;
    let file_str = file.to_string_lossy();
    let mut parts = resolve_editor_at_argv(&cmd, file_str.as_ref(), line)?;
    let program = parts.remove(0);
    let mut command = std::process::Command::new(&program);
    command.args(&parts).current_dir(worktree);
    detach_io(&mut command);
    command
        .spawn()
        .map_err(|e| Error::UserInput(format!("spawn {program}: {e}")))?;
    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib editor_at_`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add src/commands/external.rs
git commit -m "feat(editor): open_in_editor_at with {file}/{line} placeholders and goto fallbacks"
```

---

## Phase 2 — ChangeEvent types, extraction, summary, line resolution

### Task 2: Shared types + module wiring

**Files:**
- Create: `src/activity/chronology.rs`
- Modify: `src/activity/mod.rs`

- [ ] **Step 1: Create the module with types**

Create `src/activity/chronology.rs`:

```rust
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
```

- [ ] **Step 2: Wire the module**

In `src/activity/mod.rs`, add after the existing `pub mod` lines:

```rust
pub mod chronology;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: builds clean (unused-code warnings are acceptable at this stage).

- [ ] **Step 4: Commit**

```bash
git add src/activity/chronology.rs src/activity/mod.rs
git commit -m "feat(chronology): add ChangeEvent/ChangeTool/ChangeDetail types"
```

### Task 3: Summary heuristic

**Files:**
- Modify: `src/activity/chronology.rs`

- [ ] **Step 1: Write the failing tests**

Append to `src/activity/chronology.rs`:

```rust
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
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib summary_tests`
Expected: FAIL — `cannot find function summarize_edit`.

- [ ] **Step 3: Implement the heuristic**

Add to `src/activity/chronology.rs` (non-test scope):

```rust
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
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib summary_tests`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add src/activity/chronology.rs
git commit -m "feat(chronology): summary heuristic for Edit/Write changes"
```

### Task 4: Extract ChangeEvents from a Claude JSONL line

**Files:**
- Modify: `src/activity/chronology.rs`

- [ ] **Step 1: Write the failing tests**

Append to `src/activity/chronology.rs`:

```rust
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
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib extract_tests`
Expected: FAIL — `cannot find function extract_change_events`.

- [ ] **Step 3: Implement extraction**

Add to `src/activity/chronology.rs` (non-test scope):

```rust
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
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib extract_tests`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add src/activity/chronology.rs
git commit -m "feat(chronology): extract ChangeEvents from Claude JSONL lines"
```

### Task 5: Resolve the changed line within the current file

**Files:**
- Modify: `src/activity/chronology.rs`

- [ ] **Step 1: Write the failing tests**

Append to `src/activity/chronology.rs`:

```rust
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
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib line_tests`
Expected: FAIL — `cannot find function resolve_line`.

- [ ] **Step 3: Implement line resolution**

Add to `src/activity/chronology.rs` (non-test scope):

```rust
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
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib line_tests`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add src/activity/chronology.rs
git commit -m "feat(chronology): resolve editor line from old_string in current file"
```

---

## Phase 3 — Timeline build, merge, and cache

### Task 6: Enumerate all session files for a worktree

**Files:**
- Modify: `src/activity/chronology.rs`

- [ ] **Step 1: Write the failing test**

Append to `src/activity/chronology.rs`:

```rust
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
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib locate_tests`
Expected: FAIL — `cannot find function session_files_in`.

- [ ] **Step 3: Implement enumeration**

Add to `src/activity/chronology.rs` (non-test scope). `session_files_in` is testable (explicit home); `claude_session_files` is the production entry point that uses the real home dir:

```rust
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
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib locate_tests`
Expected: PASS (2 tests). (`tempfile` is already a dev-dependency — used throughout `events.rs` tests.)

- [ ] **Step 5: Commit**

```bash
git add src/activity/chronology.rs
git commit -m "feat(chronology): enumerate all session jsonl files for a worktree"
```

### Task 7: Parse a file into ChangeEvents

**Files:**
- Modify: `src/activity/chronology.rs`

- [ ] **Step 1: Write the failing test**

Append to `src/activity/chronology.rs`:

```rust
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
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib parse_file_tests`
Expected: FAIL — `cannot find function parse_file`.

- [ ] **Step 3: Implement file parsing**

Add to `src/activity/chronology.rs` (non-test scope):

```rust
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
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib parse_file_tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/activity/chronology.rs
git commit -m "feat(chronology): parse a session file into ChangeEvents"
```

### Task 8: Cached timeline (merge across files, invalidate on size/mtime)

**Files:**
- Modify: `src/activity/chronology.rs`

- [ ] **Step 1: Write the failing tests**

Append to `src/activity/chronology.rs`:

```rust
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
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib timeline_tests`
Expected: FAIL — `cannot find type Timeline`.

- [ ] **Step 3: Implement the cached timeline**

Add to `src/activity/chronology.rs` (non-test scope):

```rust
use std::collections::HashMap;
use std::time::SystemTime;

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
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib timeline_tests`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src/activity/chronology.rs
git commit -m "feat(chronology): cached newest-first Timeline merging session files"
```

---

## Phase 4 — Config (global + per-repo) and storage

### Task 9: ChronologyConfig with default/override/sanitize

**Files:**
- Create: `src/config/chronology.rs`
- Modify: `src/config/mod.rs`

- [ ] **Step 1: Write the failing tests**

Create `src/config/chronology.rs` with the test module first (implementation added in Step 3):

```rust
//! Display config for the change-chronology bar. Resolved from a global JSON
//! blob in `settings` (`chronology_config`) + a per-repo JSON override on
//! `repos.chronology_config`. Scalar fields merge per-field; repo wins.
//! Mirrors `src/config/detail_bar_config.rs`.
//!
//! See `docs/superpowers/specs/2026-06-05-change-chronology-view-design.md`.

use crate::data::store::{Repo, Store};
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_visible_right_sane_width() {
        let c = ChronologyConfig::default();
        assert!(c.visible);
        assert_eq!(c.side, Side::Right);
        assert_eq!(c.width.percent, 32);
        assert!(c.width.min_cols <= c.width.max_cols);
    }

    #[test]
    fn override_merges_per_field() {
        let base = ChronologyConfig::default();
        let ovr = ChronologyOverride {
            visible: Some(false),
            side: Some(Side::Left),
            width: None,
        };
        let merged = base.with_override(&ovr);
        assert!(!merged.visible);
        assert_eq!(merged.side, Side::Left);
        assert_eq!(merged.width.percent, 32, "unspecified width inherits");
    }

    #[test]
    fn sanitize_clamps_and_swaps() {
        let mut c = ChronologyConfig::default();
        c.width.percent = 99;
        c.width.min_cols = 80;
        c.width.max_cols = 10;
        c.sanitize();
        assert!(c.width.percent <= 80);
        assert!(c.width.min_cols <= c.width.max_cols, "inverted min/max swapped");
    }

    #[test]
    fn resolved_width_clamps_to_min_and_max() {
        let mut c = ChronologyConfig::default();
        c.width.percent = 50;
        c.width.min_cols = 20;
        c.width.max_cols = 30;
        assert_eq!(c.resolved_width(200), 30, "50% of 200 = 100, clamped to max 30");
        assert_eq!(c.resolved_width(20), 20, "50% of 20 = 10, clamped to min 20");
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib config::chronology`
Expected: FAIL — `ChronologyConfig` not found. (Module not wired yet — Step 3 wires it.)

- [ ] **Step 3: Implement the config and wire the module**

Prepend the implementation above the test module in `src/config/chronology.rs`:

```rust
fn default_visible() -> bool { true }
fn default_percent() -> u8 { 32 }
fn default_min_cols() -> u16 { 24 }
fn default_max_cols() -> u16 { 60 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side { Left, Right }

impl Default for Side {
    fn default() -> Self { Side::Right }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WidthSpec {
    #[serde(default = "default_percent")]
    pub percent: u8,
    #[serde(default = "default_min_cols")]
    pub min_cols: u16,
    #[serde(default = "default_max_cols")]
    pub max_cols: u16,
}

impl Default for WidthSpec {
    fn default() -> Self {
        Self {
            percent: default_percent(),
            min_cols: default_min_cols(),
            max_cols: default_max_cols(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChronologyConfig {
    #[serde(default = "default_visible")]
    pub visible: bool,
    #[serde(default)]
    pub side: Side,
    #[serde(default)]
    pub width: WidthSpec,
}

impl Default for ChronologyConfig {
    fn default() -> Self {
        Self {
            visible: default_visible(),
            side: Side::default(),
            width: WidthSpec::default(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChronologyOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<Side>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<WidthSpec>,
}

impl ChronologyConfig {
    pub fn with_override(mut self, ovr: &ChronologyOverride) -> Self {
        if let Some(v) = ovr.visible {
            self.visible = v;
        }
        if let Some(s) = ovr.side {
            self.side = s;
        }
        if let Some(w) = &ovr.width {
            self.width = w.clone();
        }
        self
    }

    /// Clamp into legal ranges and swap inverted min/max. Idempotent.
    pub fn sanitize(&mut self) {
        self.width.percent = self.width.percent.clamp(10, 80);
        self.width.min_cols = self.width.min_cols.clamp(12, 120);
        self.width.max_cols = self.width.max_cols.clamp(12, 160);
        if self.width.min_cols > self.width.max_cols {
            std::mem::swap(&mut self.width.min_cols, &mut self.width.max_cols);
        }
    }

    /// Column width for an attach area `total` columns wide: `percent` of
    /// `total`, clamped to `[min_cols, max_cols]`.
    pub fn resolved_width(&self, total: u16) -> u16 {
        let target = (u32::from(total) * u32::from(self.width.percent) / 100) as u16;
        target.clamp(self.width.min_cols, self.width.max_cols)
    }
}

/// Resolve the global config only (no repo override). Defaults on missing key
/// or parse failure. Mirrors `detail_bar_config::resolve_global_only`.
pub fn resolve_global_only(store: &Store) -> ChronologyConfig {
    let mut cfg = match store.get_setting("chronology_config") {
        Ok(Some(raw)) => serde_json::from_str(&raw).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "chronology_config: global parse failed; using defaults");
            ChronologyConfig::default()
        }),
        _ => ChronologyConfig::default(),
    };
    cfg.sanitize();
    cfg
}

/// Resolve global config with the per-repo override applied. Mirrors
/// `detail_bar_config::resolve`.
pub fn resolve(repo: &Repo, store: &Store) -> ChronologyConfig {
    let mut cfg = resolve_global_only(store);
    if let Some(raw) = repo.chronology_config.as_deref() {
        match serde_json::from_str::<ChronologyOverride>(raw) {
            Ok(ovr) => cfg = cfg.with_override(&ovr),
            Err(e) => {
                tracing::warn!(error = %e, "chronology_config: repo override parse failed; ignoring");
            }
        }
    }
    cfg.sanitize();
    cfg
}
```

In `src/config/mod.rs`, add next to `pub mod detail_bar_config;`:

```rust
pub mod chronology;
```

> NOTE: `resolve`/`resolve_global_only` reference `repo.chronology_config`, added to the `Repo` struct in Task 10. Implement Task 10 before running the `resolve` paths; the `tests` module here does not touch the store, so it compiles and passes once Task 10's field exists. Sequence Task 10 immediately after this task (do not commit a non-compiling tree).

- [ ] **Step 4: (after Task 10) Run to verify pass**

Run: `cargo test --lib config::chronology`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit (jointly with Task 10)**

Commit message in Task 10.

### Task 10: `repos.chronology_config` column, migration, accessor

**Files:**
- Modify: `src/data/store.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/data/store.rs` (mirror `detail_bar_config_column_round_trips`):

```rust
#[test]
fn chronology_config_column_round_trips() {
    let store = Store::open_in_memory().unwrap();
    let id = store.add_repo("r", std::path::Path::new("/tmp/r"), "wsx/").unwrap();
    let repo = store.repos().unwrap().into_iter().find(|r| r.id == id).unwrap();
    assert!(repo.chronology_config.is_none());
    store
        .set_repo_chronology_config(id, Some(r#"{"visible":false}"#))
        .unwrap();
    let repo = store.repos().unwrap().into_iter().find(|r| r.id == id).unwrap();
    assert_eq!(repo.chronology_config.as_deref(), Some(r#"{"visible":false}"#));
}
```

> If `add_repo`'s signature differs, copy the exact call used by the adjacent `detail_bar_config_column_round_trips` test (read it first).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib chronology_config_column_round_trips`
Expected: FAIL — `no field chronology_config` / `no method set_repo_chronology_config`.

- [ ] **Step 3: Implement the column**

In `src/data/store.rs`:

1. Add the field to `struct Repo` immediately after `detail_bar_config`:

```rust
    pub chronology_config: Option<String>,
```

2. Add the migration alongside the others (after the `detail_bar_config` migration block near line 216):

```rust
            let has_chronology: i64 = self.conn.query_row(
                "SELECT count(*) FROM pragma_table_info('repos') WHERE name = 'chronology_config'",
                [],
                |r| r.get(0),
            )?;
            if has_chronology == 0 {
                self.conn
                    .execute("ALTER TABLE repos ADD COLUMN chronology_config TEXT", [])?;
            }
```

3. In `repos()`, add `chronology_config` to the SELECT list **after** `detail_bar_config` and before `created_at`, then bump the `created_at` index:

```rust
            "SELECT id, name, path, branch_prefix, custom_instructions, \
                    setup_script, archive_script, pinned_commands, \
                    related_repos, base_branch, detail_bar_config, \
                    chronology_config, created_at \
             FROM repos ORDER BY id",
```

and in the row mapping:

```rust
                detail_bar_config: r.get(10)?,
                chronology_config: r.get(11)?,
                created_at: r.get(12)?,
```

4. Add the setter next to `set_repo_detail_bar_config`:

```rust
    pub fn set_repo_chronology_config(&self, id: RepoId, value: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE repos SET chronology_config = ?1 WHERE id = ?2",
            rusqlite::params![value, id.0],
        )?;
        Ok(())
    }
```

> Check every other place that constructs a `Repo { .. }` literal (e.g. test helpers in `detail_bar_config.rs`/`chronology.rs` config tests) and add `chronology_config: None`. Search: `grep -rn "detail_bar_config:" src` and add the sibling field at each literal.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib chronology_config_column_round_trips && cargo test --lib config::chronology`
Expected: PASS (store round-trip + the 4 config tests from Task 9).

- [ ] **Step 5: Commit**

```bash
git add src/config/chronology.rs src/config/mod.rs src/data/store.rs
git commit -m "feat(chronology): config struct + repos.chronology_config column"
```

---

## Phase 5 — CLI config surface

### Task 11: `chronology_config` config key

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/cli.rs` (mirror `detail_bar_config_validate_and_normalize`):

```rust
#[test]
fn chronology_config_validate_accepts_partial_json() {
    let out = chronology_config_validate_and_normalize(r#"{"side":"left"}"#).unwrap();
    assert!(out.contains("\"side\""));
}

#[test]
fn chronology_config_validate_rejects_bad_json() {
    assert!(chronology_config_validate_and_normalize("{not json").is_err());
}

#[test]
fn chronology_config_seed_is_valid_json() {
    let seed = chronology_config_seed_for_empty();
    assert!(serde_json::from_str::<serde_json::Value>(&seed).is_ok());
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib chronology_config_`
Expected: FAIL — functions not found.

- [ ] **Step 3: Implement the key handling**

In `src/cli.rs`:

1. Add the validator/seed helpers next to `detail_bar_config_validate_and_normalize` (~line 1467):

```rust
fn chronology_config_validate_and_normalize(raw: &str) -> Result<String> {
    let cfg: crate::config::chronology::ChronologyConfig = serde_json::from_str(raw)
        .map_err(|e| Error::UserInput(format!("chronology_config: invalid JSON: {e}")))?;
    serde_json::to_string(&cfg)
        .map_err(|e| Error::UserInput(format!("chronology_config: serialize failed: {e}")))
}

fn chronology_config_seed_for_empty() -> String {
    serde_json::to_string_pretty(&crate::config::chronology::ChronologyConfig::default())
        .unwrap_or_else(|_| "{}".to_string())
}
```

2. Add `"chronology_config"` to the valid-keys match arm (the `| "detail_bar_config" | "usage_graph_window"` list near line 399):

```rust
            | "detail_bar_config"
            | "chronology_config"
            | "usage_graph_window"
```

3. In the `CliAction::ConfigSet` handler (~line 1171), extend the validate/normalize and seed branches the same way they handle `detail_bar_config`:

```rust
                let value = if key == "detail_bar_config" {
                    detail_bar_config_validate_and_normalize(&raw)?
                } else if key == "chronology_config" {
                    chronology_config_validate_and_normalize(&raw)?
                } else if key == "usage_graph_window" {
                    usage_graph_window_validate(&raw)?
                } else {
                    raw
                };
```

and the seed branch (~line 1205):

```rust
            let seed = if key == "detail_bar_config" && current.is_empty() {
                detail_bar_config_seed_for_empty()
            } else if key == "chronology_config" && current.is_empty() {
                chronology_config_seed_for_empty()
            } else {
                current
            };
```

and the normalized branch (~line 1218):

```rust
            let normalized = if key == "detail_bar_config" {
                detail_bar_config_validate_and_normalize(&edited)?
            } else if key == "chronology_config" {
                chronology_config_validate_and_normalize(&edited)?
            } else if key == "usage_graph_window" {
                usage_graph_window_validate(&edited)?
            } else {
                edited
            };
```

> Read the exact surrounding code at each `~line` before editing — variable names (`raw`/`edited`/`current`) must match what's there. Adapt to the actual structure rather than assuming.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib chronology_config_ && cargo build`
Expected: PASS (3 tests) and clean build.

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): chronology_config config key (validate/normalize/seed)"
```

---

## Phase 6 — Rendering helpers and attached-view integration

### Task 12: Pure render helpers (entry lines, relative path, auto-hide)

**Files:**
- Create: `src/ui/chronology_bar.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Write the failing tests**

Create `src/ui/chronology_bar.rs` with tests first:

```rust
//! Pure rendering helpers for the change-chronology bar. The host
//! (`src/ui/attached.rs`) carves the side column and calls these to build the
//! content lines; keeping the formatting pure makes it unit-testable.

use crate::activity::chronology::{ChangeDetail, ChangeEvent, ChangeTool};
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ev(file: &str, summary: &str) -> ChangeEvent {
        ChangeEvent {
            timestamp_ms: 0,
            tool: ChangeTool::Edit,
            file_path: PathBuf::from(file),
            summary: summary.to_string(),
            detail: ChangeDetail::Edit { old: "a".into(), new: "b".into() },
        }
    }

    #[test]
    fn relative_path_strips_worktree_prefix() {
        let p = relative_display(Path::new("/wt/src/main.rs"), Path::new("/wt"));
        assert_eq!(p, "src/main.rs");
    }

    #[test]
    fn relative_path_passthrough_when_not_prefixed() {
        let p = relative_display(Path::new("/other/x.rs"), Path::new("/wt"));
        assert_eq!(p, "/other/x.rs");
    }

    #[test]
    fn auto_hide_when_area_too_narrow() {
        // bar wants 30 cols, agent needs >= MIN_AGENT_COLS; 35-wide area hides it.
        assert!(should_auto_hide(35, 30));
        assert!(!should_auto_hide(120, 30));
    }

    #[test]
    fn entry_produces_header_and_summary_lines() {
        let lines = entry_lines(&ev("/wt/src/main.rs", "fn foo()"), Path::new("/wt"), false, 40);
        assert_eq!(lines.len(), 2, "B fidelity: header + summary, no diff peek");
    }

    #[test]
    fn expanded_entry_adds_diff_peek_lines() {
        let lines = entry_lines(&ev("/wt/src/main.rs", "fn foo()"), Path::new("/wt"), true, 40);
        assert!(lines.len() > 2, "expanded entry includes diff peek");
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib chronology_bar`
Expected: FAIL — functions/module not found.

- [ ] **Step 3: Implement the helpers and wire the module**

Prepend to `src/ui/chronology_bar.rs`:

```rust
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

/// Minimum columns the agent pane must keep for the bar to be allowed.
pub const MIN_AGENT_COLS: u16 = 40;

/// Worktree-relative display path, falling back to the full path when the file
/// is not under the worktree.
pub fn relative_display(file: &Path, worktree: &Path) -> String {
    match file.strip_prefix(worktree) {
        Ok(rel) => rel.to_string_lossy().to_string(),
        Err(_) => file.to_string_lossy().to_string(),
    }
}

/// Hide the bar when carving `bar_cols` would leave the agent < MIN_AGENT_COLS.
pub fn should_auto_hide(area_cols: u16, bar_cols: u16) -> bool {
    area_cols.saturating_sub(bar_cols) < MIN_AGENT_COLS
}

fn hhmm(timestamp_ms: i64) -> String {
    // Local wall-clock is not needed for a relative glance; show HH:MM in UTC
    // derived from epoch ms without pulling in chrono (matches events.rs style).
    let secs = timestamp_ms / 1000;
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    format!("{h:02}:{m:02}")
}

/// Render one entry into lines. Line 1: `HH:MM file`. Line 2: dim summary.
/// When `expanded`, appends up to a few diff-peek lines from `detail`.
pub fn entry_lines(
    ev: &ChangeEvent,
    worktree: &Path,
    expanded: bool,
    width: u16,
) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let rel = relative_display(&ev.file_path, worktree);
    out.push(Line::from(vec![
        Span::styled(hhmm(ev.timestamp_ms), Style::default().add_modifier(Modifier::DIM)),
        Span::raw(" "),
        Span::raw(rel),
    ]));
    out.push(Line::from(Span::styled(
        ev.summary.clone(),
        Style::default().add_modifier(Modifier::DIM | Modifier::ITALIC),
    )));
    if expanded {
        let peek: Vec<String> = match &ev.detail {
            ChangeDetail::Edit { old, new } => {
                let mut v = Vec::new();
                for l in old.lines().take(2) {
                    v.push(format!("- {l}"));
                }
                for l in new.lines().take(2) {
                    v.push(format!("+ {l}"));
                }
                v
            }
            ChangeDetail::Write { head } => {
                head.lines().take(3).map(|l| format!("+ {l}")).collect()
            }
            ChangeDetail::None => Vec::new(),
        };
        for l in peek {
            let clipped: String = l.chars().take(width as usize).collect();
            out.push(Line::from(Span::styled(
                clipped,
                Style::default().add_modifier(Modifier::DIM),
            )));
        }
    }
    out
}
```

In `src/ui/mod.rs`, add next to the other `pub mod` lines:

```rust
pub mod chronology_bar;
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib chronology_bar`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add src/ui/chronology_bar.rs src/ui/mod.rs
git commit -m "feat(chronology): pure render helpers for the chronology bar"
```

### Task 13: App state for the timeline and bar UI

**Files:**
- Modify: `src/app.rs` (App struct + construction) and the attached-view state

- [ ] **Step 1: Add state fields**

This task has no isolated unit test (it's plumbing verified by `cargo build` + later interaction tests). Add to the `App` struct in `src/app.rs`:

```rust
    /// Per-workspace change-chronology timelines, keyed by workspace id.
    /// Lazily built/refreshed while attached.
    pub chronology: std::collections::HashMap<crate::data::store::WorkspaceId, crate::activity::chronology::Timeline>,
    /// Scroll offset (entries from the top) of the chronology bar in the
    /// focused attached pane.
    pub chronology_scroll: usize,
    /// Index of the currently expanded chronology entry, if any.
    pub chronology_expanded: Option<usize>,
```

> Use the actual `WorkspaceId` type as declared in `src/data/store.rs` (confirm the name; the codebase uses `RepoId(pub i64)` and an `AgentInstanceId` — find the workspace-id type and match it). Initialize the three fields in every `App` constructor: `chronology: HashMap::new(), chronology_scroll: 0, chronology_expanded: None`.

- [ ] **Step 2: Add a refresh helper**

Add an `impl App` method in `src/app.rs`:

```rust
    /// Refresh the chronology timeline for `worktree`/`workspace_id` from the
    /// on-disk session logs. Cheap when nothing changed (per-file cache).
    pub fn refresh_chronology(
        &mut self,
        workspace_id: crate::data::store::WorkspaceId,
        worktree: &std::path::Path,
    ) {
        let files = crate::activity::chronology::claude_session_files(worktree);
        self.chronology
            .entry(workspace_id)
            .or_default()
            .refresh(&files);
    }
```

- [ ] **Step 3: Verify build**

Run: `cargo build`
Expected: clean build.

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "feat(chronology): app state + refresh helper for timelines"
```

### Task 14: Carve the side column and render the bar in the attached view

**Files:**
- Modify: `src/ui/attached.rs`, `src/app/render.rs`

This integrates the bar into `render_panes`. The exact wiring depends on how `render_panes` is called from `src/app/render.rs`; read both before editing.

- [ ] **Step 1: Extend `PanesDrawOutput` and `render_panes`**

In `src/ui/attached.rs`, add a field to `PanesDrawOutput`:

```rust
    /// `(entry_index, clickable_rect)` for each rendered chronology entry in the
    /// focused pane's bar. Empty when the bar isn't shown. Consumed by the input
    /// handler for click → expand / open-in-editor.
    pub chronology_entry_rects: Vec<(usize, Rect)>,
```

Add parameters to `render_panes` for the resolved config, the focused timeline events, the scroll offset, the expanded index, and the focused worktree path:

```rust
    chronology: Option<ChronologyDraw<'_>>,
```

with a small struct near `PaneSpec`:

```rust
/// Everything `render_panes` needs to draw the chronology bar for the focused
/// pane. `None` (passed at the call site) means the bar is disabled/hidden.
pub struct ChronologyDraw<'a> {
    pub config: &'a crate::config::chronology::ChronologyConfig,
    pub events: &'a [crate::activity::chronology::ChangeEvent],
    pub worktree: &'a std::path::Path,
    pub scroll: usize,
    pub expanded: Option<usize>,
}
```

- [ ] **Step 2: Carve the column before laying out panes**

At the top of `render_panes`, before computing `pane_rects`, compute the bar rect and shrink the pane area. The function currently treats the incoming pane rects as the full area; introduce a helper that, given the overall pane area and the `ChronologyDraw`, returns `(agent_area, Option<bar_rect>)`:

```rust
use crate::config::chronology::Side;
use crate::ui::chronology_bar::{entry_lines, should_auto_hide, MIN_AGENT_COLS};

/// Split `area` into (agent_area, Some(bar_rect)) per the chronology config,
/// or (area, None) when disabled/auto-hidden.
fn split_for_chronology(area: Rect, draw: &Option<ChronologyDraw<'_>>) -> (Rect, Option<Rect>) {
    let Some(draw) = draw else { return (area, None) };
    if !draw.config.visible {
        return (area, None);
    }
    let bar_cols = draw.config.resolved_width(area.width);
    if should_auto_hide(area.width, bar_cols) {
        return (area, None);
    }
    match draw.config.side {
        Side::Right => {
            let agent = Rect { width: area.width - bar_cols, ..area };
            let bar = Rect { x: area.x + area.width - bar_cols, width: bar_cols, ..area };
            (agent, Some(bar))
        }
        Side::Left => {
            let bar = Rect { width: bar_cols, ..area };
            let agent = Rect { x: area.x + bar_cols, width: area.width - bar_cols, ..area };
            (agent, Some(bar))
        }
    }
}
```

> `render_panes` receives pre-computed per-pane rects from the caller (`SplitTree::layout`). The cleanest integration is to move the chronology split **into the caller** (`src/app/render.rs`): compute `(agent_area, bar_rect)` from the full attach content area, run the existing split layout against `agent_area`, then pass `bar_rect` + `ChronologyDraw` into `render_panes` for it to paint. Implement the split in `render.rs` and pass the resulting `bar_rect: Option<Rect>` to `render_panes`. Adjust the signatures accordingly; keep `split_for_chronology` as the shared pure helper (unit-test it as in Step 4).

- [ ] **Step 3: Paint the bar and collect click rects**

In `render_panes`, after panes are drawn, if a `bar_rect` and `ChronologyDraw` are present, paint:
- a 1-column divider on the inner edge reusing the `render_dividers` style;
- the `CHANGE CHRONOLOGY` header line with a side indicator;
- entries from `draw.scroll` onward via `entry_lines(ev, draw.worktree, Some(i)==draw.expanded, inner_width)`, tracking the `Rect` each entry's header line occupies and pushing `(i, rect)` into `chronology_entry_rects`;
- an em-dash placeholder line when `draw.events` is empty.

Use a `Paragraph` per the existing detail-bar rendering style in `src/ui/dashboard/detail.rs` for reference.

- [ ] **Step 4: Add a unit test for the split helper**

Add to `src/ui/attached.rs` tests (or a small `#[cfg(test)]` block):

```rust
#[test]
fn split_right_carves_bar_on_right() {
    let cfg = crate::config::chronology::ChronologyConfig::default();
    let events: Vec<crate::activity::chronology::ChangeEvent> = Vec::new();
    let draw = ChronologyDraw {
        config: &cfg,
        events: &events,
        worktree: std::path::Path::new("/wt"),
        scroll: 0,
        expanded: None,
    };
    let area = Rect { x: 0, y: 0, width: 200, height: 50 };
    let (agent, bar) = split_for_chronology(area, &Some(draw));
    let bar = bar.expect("bar shown at 200 cols");
    assert_eq!(agent.width + bar.width, 200);
    assert!(bar.x > agent.x, "right side");
}

#[test]
fn split_hidden_when_too_narrow() {
    let cfg = crate::config::chronology::ChronologyConfig::default();
    let events: Vec<crate::activity::chronology::ChangeEvent> = Vec::new();
    let draw = ChronologyDraw {
        config: &cfg, events: &events, worktree: std::path::Path::new("/wt"),
        scroll: 0, expanded: None,
    };
    let area = Rect { x: 0, y: 0, width: 50, height: 50 };
    let (_agent, bar) = split_for_chronology(area, &Some(draw));
    assert!(bar.is_none(), "auto-hidden when agent would be < MIN_AGENT_COLS");
}
```

- [ ] **Step 5: Wire the call site in `render.rs`**

In `src/app/render.rs`, where the attached view is rendered:
- resolve the focused pane's repo + workspace, call `app.refresh_chronology(workspace_id, worktree)`;
- `let cfg = crate::config::chronology::resolve(repo, &app.store);`
- build `ChronologyDraw` from `cfg`, `app.chronology[&workspace_id].events()`, the worktree, `app.chronology_scroll`, `app.chronology_expanded`;
- perform the chronology split on the attach content area, lay out panes against the agent area, pass `bar_rect` + `ChronologyDraw` into `render_panes`.

> Store the returned `chronology_entry_rects` on `App` (e.g. a transient `app.chronology_entry_rects: Vec<(usize, Rect)>` field, set each draw) for the input handler in Task 15. Add and initialize that field as in Task 13.

- [ ] **Step 6: Run tests + build**

Run: `cargo test --lib attached && cargo build`
Expected: PASS (2 split tests) and clean build. Manually verify the bar appears when attached.

- [ ] **Step 7: Commit**

```bash
git add src/ui/attached.rs src/app/render.rs src/app.rs
git commit -m "feat(chronology): carve side column and render the bar in attached view"
```

### Task 15: Keybindings, scroll, click → expand / open editor

**Files:**
- Modify: `src/app/input.rs`

- [ ] **Step 1: Add the leader follow-ups**

In `src/app/input.rs`, inside the `if app.leader_pending { match k.code { … } }` block (the attached-mode leader, ~line 707, alongside `Char('e')`), add:

```rust
            KeyCode::Char('c') => {
                // Toggle the chronology bar (persist to the global setting so
                // the change survives detach and matches the CLI).
                toggle_chronology_visible(app);
                return Ok(());
            }
            KeyCode::Char('C') => {
                // Swap the chronology bar's side (left <-> right), persisted.
                swap_chronology_side(app);
                return Ok(());
            }
```

- [ ] **Step 2: Implement the toggle/swap helpers**

Add to `src/app/input.rs`:

```rust
fn toggle_chronology_visible(app: &mut App) {
    let mut cfg = crate::config::chronology::resolve_global_only(&app.store);
    cfg.visible = !cfg.visible;
    if let Ok(json) = serde_json::to_string(&cfg) {
        let _ = app.store.set_setting("chronology_config", &json);
    }
}

fn swap_chronology_side(app: &mut App) {
    use crate::config::chronology::Side;
    let mut cfg = crate::config::chronology::resolve_global_only(&app.store);
    cfg.side = match cfg.side {
        Side::Left => Side::Right,
        Side::Right => Side::Left,
    };
    if let Ok(json) = serde_json::to_string(&cfg) {
        let _ = app.store.set_setting("chronology_config", &json);
    }
}
```

> `set_setting` exists on `Store` (`src/data/store.rs:442`). These write the **global** config; per-repo overrides are managed via the CLI per the spec.

- [ ] **Step 3: Handle scroll and clicks**

In the attached-view mouse handling (find where `pane_rects`/`chip_rects` from `PanesDrawOutput` are hit-tested):
- On scroll-up/down with the cursor over the bar rect, adjust `app.chronology_scroll` (saturating; clamp to event count).
- On click within a `chronology_entry_rects` entry rect: if it's already `app.chronology_expanded`, open the editor; otherwise set `app.chronology_expanded = Some(index)`.

Opening the editor on a second click (or a dedicated modifier — keep it simple: click toggles expand, and a click on the already-expanded entry opens it):

```rust
// pseudocode at the hit-test site — adapt to the surrounding handler:
if let Some((idx, _)) = app.chronology_entry_rects.iter().find(|(_, r)| rect_contains(*r, col, row)) {
    let idx = *idx;
    if app.chronology_expanded == Some(idx) {
        // open editor at file:line
        if let Some(ev) = focused_timeline_events.get(idx) {
            let line = crate::activity::chronology::resolve_line_in_file(&ev.file_path, &ev.detail);
            let editor = app.store.get_setting("editor_cmd").ok().flatten();
            let _ = crate::commands::external::open_in_editor_at(
                worktree, &ev.file_path, line, editor.as_deref(),
            );
        }
    } else {
        app.chronology_expanded = Some(idx);
    }
    return Ok(());
}
```

> Match the exact mouse-event plumbing already used for `chip_rects` (column/row extraction, the `rect_contains`-equivalent helper). Reuse existing helpers rather than adding new ones where they exist.

- [ ] **Step 4: Build and smoke-test**

Run: `cargo build`
Expected: clean build. Manually: attach, press `Ctrl-x c` to toggle, `Ctrl-x C` to swap side, scroll the bar, click an entry to expand, click again to open the editor at the line.

- [ ] **Step 5: Commit**

```bash
git add src/app/input.rs
git commit -m "feat(chronology): Ctrl-x c/C toggle+swap, scroll, click to expand/open editor"
```

---

## Phase 7 — Documentation

### Task 16: README

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Document the feature**

Add a "Change chronology" subsection under "Configuration and customization" (near the detail-bar and usage-graph docs). Cover:
- what it is (newest-first, time-ordered series of agent file changes in the attached view);
- keybindings: `Ctrl-x c` (toggle), `Ctrl-x C` (swap side);
- click an entry to expand its diff peek; click again to open your editor at the changed line (requires `editor_cmd` supporting `{file}`/`{line}`, or a recognized editor — `code`, `vim`/`nvim`, `emacs`);
- config: `wsx config set chronology_config '<json>'` (global) and the per-repo override, with the field list (`visible`, `side`, `width.percent`, `width.min_cols`, `width.max_cols`);
- note that history is reconstructed from the agent's session logs (currently Claude; other agents land incrementally).

Add the two keybindings to the attached-view keybindings table as well.

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: document the change chronology view"
```

---

## Phase 8 — Other agents (incremental, post-Claude)

> Each task reuses the shared `ChangeEvent`/`ChangeTool`/`ChangeDetail` types and the `Timeline`. The work is per-agent extraction: read the agent's existing tool-use parser (each already extracts `edited_file_paths`), and add `extract_change_events_<agent>` plus an agent-aware variant of `claude_session_files` for that agent's log directory layout.

### Task 17: Codex extraction

**Files:**
- Modify: `src/activity/chronology.rs`, reading `src/activity/codex_events.rs`

- [ ] **Step 1:** Read `src/activity/codex_events.rs` and locate where it extracts edited file paths and tool calls (e.g. `apply_patch`). Note the on-disk session-log location for Codex.
- [ ] **Step 2:** Write a failing test `extract_codex_*` with a representative Codex JSONL/log line (copy a real shape from the existing codex tests).
- [ ] **Step 3:** Implement `extract_change_events_codex(&serde_json::Value) -> Vec<ChangeEvent>` and a `codex_session_files(worktree)` enumerator. When Codex doesn't expose old/new text, set `ChangeDetail::None` (B-only; no expand, line resolution returns 1).
- [ ] **Step 4:** Extend `App::refresh_chronology` to merge events from all agents present for the workspace (call each agent's enumerator + parser; merge into one `Timeline`). The `Timeline::refresh` already accepts a flat `&[PathBuf]`, so pass the union of all agents' session files **only if** they share the JSONL shape; otherwise add a `Timeline::refresh_with(parser, files)` variant that takes a per-file parse fn. Prefer the latter to keep formats isolated.
- [ ] **Step 5:** `cargo test` + commit `feat(chronology): Codex change extraction`.

### Task 18: Pi extraction

Same shape as Task 17 against `src/activity/pi_events.rs`. Commit `feat(chronology): Pi change extraction`.

### Task 19: Hermes extraction

Same shape as Task 17 against `src/activity/hermes_events.rs`. Commit `feat(chronology): Hermes change extraction`.

> When Phase 8 introduces a per-agent parse function, refactor `Timeline` to store events per `(agent, file)` and merge, so a single workspace running multiple agents shows a unified chronology. Add a test that merges a Claude file and a Codex file and asserts global newest-first ordering.

---

## Self-Review (completed during planning)

**Spec coverage:**
- Toggleable bar, left/right, global+per-repo → Tasks 9–11, 15 (config, store, CLI, keybindings). ✓
- B default / C on expand → Task 12 (`entry_lines` expanded flag), Task 15 (expand on click). ✓
- Click → editor at file:line → Tasks 1, 5, 15. ✓
- Wider default width, configurable min → Task 9 (`percent: 32`, `min_cols`). ✓
- Whole workspace history from logs → Tasks 6–8 (enumerate all files, merge, cache). ✓
- All agents → Claude in Phase 2; Codex/Pi/Hermes in Phase 8. ✓ (sequenced, not dropped)
- One entry per edit, newest first → Task 4 (MultiEdit → N events), Task 8 (sort desc). ✓
- Error handling (missing logs, malformed lines, narrow terminal, deleted files) → Tasks 6/7 (empty/skip), Task 12 (`should_auto_hide`), Task 5 (`resolve_line_in_file` returns 1 on read error). ✓
- Testing per component → every task is TDD. ✓

**Placeholder scan:** No "TBD"/"add error handling"-style steps; the `~line` references include an explicit instruction to read surrounding code and adapt, with concrete code shown. Phase 8 tasks are intentionally recipe-style because the per-agent wire formats must be read at implementation time — they are bounded and reuse already-defined types, not placeholders.

**Type consistency:** `ChangeEvent`/`ChangeTool`/`ChangeDetail`, `Timeline::refresh`/`events`/`parse_count`, `ChronologyConfig`/`ChronologyOverride`/`WidthSpec`/`Side`, `resolve_global_only`/`resolve`, `resolved_width`, `extract_change_events`, `resolve_line`/`resolve_line_in_file`, `session_files_in`/`claude_session_files`, `entry_lines`/`should_auto_hide`/`relative_display`/`MIN_AGENT_COLS`, `open_in_editor_at`/`resolve_editor_at_argv`, `set_repo_chronology_config` — names are used consistently across tasks.
