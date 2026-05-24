# Dashboard workspace detail bar — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a bottom-pinned detail bar to the dashboard that surfaces per-workspace context (name, branch, lifecycle, diff, procs, derived chat summary, recent assistant lines, recent edited files, worktree path) and an inline reply input for the currently selected workspace.

**Architecture:** Extend `WorkspaceEvents` with three derived fields (`first_user_text`, `tool_use_counts`, `recent_edited_files`) populated by the existing JSONL tail loop. Add a new `src/ui/dashboard/detail.rs` renderer (peer of `layout.rs` / `row.rs`). Add a `PaneFocus::DetailBarReply` variant; route Tab/Enter/Esc/Char keys to a new `reply_draft: String` on `DashboardState`. Replace `app.rs::draw`'s 60/40 PM split with a 3-region `(list, detail, pm)` layout helper.

**Tech Stack:** Rust, ratatui, crossterm, serde_json (parsing), tokio. Existing wsx codebase patterns: `src/ui/dashboard/row.rs` for renderer style, `src/ui/dashboard/layout.rs` for chrome rendering, `src/events.rs` for tail loop, `src/app.rs::handle_key_dashboard` for key dispatch.

**Spec:** `docs/superpowers/specs/2026-05-24-dashboard-workspace-detail-design.md`.

---

## File touch summary

- **Create:** `src/ui/dashboard/detail.rs` — owns header strip, body columns, reply input row, responsive collapse. Pure data → `Line` / `Frame` rendering.
- **Create:** `docs/manual-tests/dashboard-detail-bar.md` — short walkthrough script.
- **Modify:** `src/events.rs` — add `ToolUseCounts` struct, three new fields on `WorkspaceEvents`, new fields on `TailUpdate`, populate them in `parse_user` / `parse_assistant` / `tail_session`, clear in `reset_session_state`.
- **Modify:** `src/ui/mod.rs` — add `PaneFocus::DetailBarReply` variant.
- **Modify:** `src/ui/dashboard/mod.rs` — add `pub mod detail`, add `reply_draft: String` to `DashboardState`.
- **Modify:** `src/app.rs` — apply new `TailUpdate` fields to `WorkspaceEvents`; replace `if app.pm_visible` block in `draw()` with `dashboard_regions` helper that produces up to three rects; carve out detail-bar area and call `detail::render`; set cursor position when detail-bar reply is focused; extend `handle_key_dashboard` to cycle Tab through the new focus and route keystrokes to `reply_draft` while focused; auto-return focus to Dashboard on selection change away from a workspace.

---

## Task 1: Add `ToolUseCounts` struct and extend `WorkspaceEvents` fields

**Files:**
- Modify: `src/events.rs` (around line 99 — `WorkspaceEvents` struct; around line 138 — `Default` impl; around line 159 — `reset_session_state`)
- Test: `src/events.rs` (tests module at bottom)

- [ ] **Step 1: Write the failing test**

Add to the `tests` module at the bottom of `src/events.rs`:

```rust
#[test]
fn workspace_events_new_fields_default_to_empty() {
    let evt = WorkspaceEvents::default();
    assert!(evt.first_user_text.is_none());
    assert_eq!(evt.tool_use_counts.read, 0);
    assert_eq!(evt.tool_use_counts.edit, 0);
    assert_eq!(evt.tool_use_counts.write, 0);
    assert_eq!(evt.tool_use_counts.bash, 0);
    assert_eq!(evt.tool_use_counts.other, 0);
    assert!(evt.recent_edited_files.is_empty());
}

#[test]
fn reset_session_state_clears_new_fields() {
    let mut evt = WorkspaceEvents::default();
    evt.first_user_text = Some("hello".to_string());
    evt.tool_use_counts.read = 3;
    evt.tool_use_counts.bash = 1;
    evt.recent_edited_files.push_front("src/main.rs".to_string());

    evt.reset_session_state();

    assert!(evt.first_user_text.is_none());
    assert_eq!(evt.tool_use_counts.read, 0);
    assert_eq!(evt.tool_use_counts.bash, 0);
    assert!(evt.recent_edited_files.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib events::tests::workspace_events_new_fields_default_to_empty events::tests::reset_session_state_clears_new_fields`
Expected: FAIL with "no field `first_user_text` on type `WorkspaceEvents`" or similar.

- [ ] **Step 3: Add `ToolUseCounts` struct**

In `src/events.rs`, immediately above `pub struct WorkspaceEvents` (around line 99), add:

```rust
/// Running tallies of tool_use blocks by category. Populated by the
/// tail loop as JSONL lines parse. Used by the dashboard detail bar
/// to synthesize a one-line action trace like "read 14 files, edited
/// 3 files, ran 2 commands".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ToolUseCounts {
    pub read: u32,
    pub edit: u32,
    pub write: u32,
    pub bash: u32,
    pub other: u32,
}

impl ToolUseCounts {
    /// Increment the appropriate field based on the Claude Code tool name.
    /// Edit/MultiEdit count as `edit`; Write/NotebookEdit count as `write`;
    /// Bash counts as `bash`; Read counts as `read`; everything else
    /// (Task, Glob, Grep, WebFetch, …) counts as `other`.
    pub fn increment(&mut self, tool_name: &str) {
        match tool_name {
            "Read" => self.read += 1,
            "Edit" | "MultiEdit" => self.edit += 1,
            "Write" | "NotebookEdit" => self.write += 1,
            "Bash" => self.bash += 1,
            _ => self.other += 1,
        }
    }
}
```

- [ ] **Step 4: Add the three new fields to `WorkspaceEvents`**

In `src/events.rs`, in the `WorkspaceEvents` struct (around line 100), add at the bottom of the field list (before the closing `}`):

```rust
    /// First plain-text user content block observed since the most
    /// recent session reset. Set once per session; preserved across
    /// log rotation past MAX_LOG. Used by the detail bar's SESSION
    /// SUMMARY column.
    pub first_user_text: Option<String>,

    /// Running tallies of tool_use blocks by category. Populated by
    /// the tail loop. Used by the detail bar to synthesize a
    /// one-line action trace.
    pub tool_use_counts: ToolUseCounts,

    /// Most-recent-first ring of file paths the agent has read or
    /// edited, bounded to 7. Consecutive duplicates collapse so a
    /// single repeated edit doesn't crowd out other recent files.
    pub recent_edited_files: VecDeque<String>,
```

- [ ] **Step 5: Update `Default` impl for `WorkspaceEvents`**

In `src/events.rs`, in the `impl Default for WorkspaceEvents` block (around line 138), add the three new fields to the literal:

```rust
            first_user_text: None,
            tool_use_counts: ToolUseCounts::default(),
            recent_edited_files: VecDeque::with_capacity(7),
```

- [ ] **Step 6: Update `reset_session_state`**

In `src/events.rs`, in `WorkspaceEvents::reset_session_state` (around line 159), add at the end of the function body:

```rust
        self.first_user_text = None;
        self.tool_use_counts = ToolUseCounts::default();
        self.recent_edited_files.clear();
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test --lib events::tests::workspace_events_new_fields_default_to_empty events::tests::reset_session_state_clears_new_fields`
Expected: PASS.

- [ ] **Step 8: Run the full events test module to catch regressions**

Run: `cargo test --lib events::tests`
Expected: All events tests pass.

- [ ] **Step 9: Commit**

```bash
git add src/events.rs
git commit -m "feat(events): add derived fields for detail bar summary"
```

---

## Task 2: Extend `TailUpdate` and `parse_user` / `parse_assistant` to surface derived data

**Files:**
- Modify: `src/events.rs` (around line 245 — `TailUpdate`; around line 397 — `ParsedLine`; around line 439 — `parse_user`; around line 491 — `parse_assistant`; around line 334 — `tail_session`)
- Test: `src/events.rs` (tests module at bottom)

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/events.rs`:

```rust
#[test]
fn parse_user_surfaces_first_user_text() {
    let line = r#"{"type":"user","message":{"role":"user","content":"summarize this repo"},"timestamp":"2026-05-14T17:32:02.744Z"}"#;
    let parsed = parse_jsonl_line(line);
    assert_eq!(
        parsed.first_user_text.as_deref(),
        Some("summarize this repo")
    );
}

#[test]
fn parse_user_omits_first_user_text_for_tool_results() {
    // A "user" line whose content is a tool_result array is not a real
    // user prompt — first_user_text must stay None.
    let line = r#"{"type":"user","message":{"role":"user","content":[{"tool_use_id":"t1","type":"tool_result","content":"ok","is_error":false}]},"timestamp":"2026-05-14T17:32:14.000Z"}"#;
    let parsed = parse_jsonl_line(line);
    assert!(parsed.first_user_text.is_none());
}

#[test]
fn parse_assistant_surfaces_edited_file_paths() {
    let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"/tmp/x/src/main.rs","old_string":"a","new_string":"b"}}]},"timestamp":"2026-05-14T17:32:14.000Z"}"#;
    let parsed = parse_jsonl_line(line);
    assert_eq!(parsed.edited_file_paths, vec!["/tmp/x/src/main.rs".to_string()]);
}

#[test]
fn parse_assistant_surfaces_read_paths() {
    let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/tmp/x/Cargo.toml"}}]},"timestamp":"2026-05-14T17:32:14.000Z"}"#;
    let parsed = parse_jsonl_line(line);
    assert_eq!(parsed.edited_file_paths, vec!["/tmp/x/Cargo.toml".to_string()]);
}

#[test]
fn parse_assistant_skips_paths_for_non_file_tools() {
    let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]},"timestamp":"2026-05-14T17:32:14.000Z"}"#;
    let parsed = parse_jsonl_line(line);
    assert!(parsed.edited_file_paths.is_empty());
}

#[test]
fn tail_session_aggregates_first_user_text_and_counts() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":"do the thing"}},"timestamp":"2026-05-14T17:32:02.744Z"}}"#).unwrap();
    writeln!(f, r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"t1","name":"Read","input":{{"file_path":"/a.rs"}}}}]}},"timestamp":"2026-05-14T17:32:03.744Z"}}"#).unwrap();
    writeln!(f, r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"t2","name":"Bash","input":{{"command":"ls"}}}}]}},"timestamp":"2026-05-14T17:32:04.744Z"}}"#).unwrap();
    drop(f);

    let upd = tail_session(&path, 0).unwrap();
    assert_eq!(upd.first_user_text.as_deref(), Some("do the thing"));
    assert_eq!(upd.tool_use_counts.read, 1);
    assert_eq!(upd.tool_use_counts.bash, 1);
    assert_eq!(upd.edited_file_paths, vec!["/a.rs".to_string()]);
}
```

This test uses `tempfile` — already a dev-dependency in the wsx workspace (it's used by existing tests in `src/events.rs` further down; verify with `grep -n tempfile Cargo.toml src/events.rs`).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib events::tests::parse_user_surfaces_first_user_text events::tests::parse_user_omits_first_user_text_for_tool_results events::tests::parse_assistant_surfaces_edited_file_paths events::tests::parse_assistant_surfaces_read_paths events::tests::parse_assistant_skips_paths_for_non_file_tools events::tests::tail_session_aggregates_first_user_text_and_counts`
Expected: FAIL with "no field `first_user_text` on `ParsedLine`" or similar.

- [ ] **Step 3: Add fields to `ParsedLine` and `TailUpdate`**

In `src/events.rs`, in `ParsedLine` (around line 397), add at the bottom of the struct (before the closing `}`):

```rust
    /// Plain user text content for the first real user message in this
    /// line (None for tool_result or non-user lines). Aggregated into
    /// `TailUpdate.first_user_text` upstream.
    pub first_user_text: Option<String>,
    /// File paths extracted from Read/Edit/MultiEdit/Write/NotebookEdit
    /// tool_use blocks on this line, in source order. Empty for any
    /// other tool / non-assistant line.
    pub edited_file_paths: Vec<String>,
```

In `src/events.rs`, in `TailUpdate` (around line 245), add at the bottom of the struct:

```rust
    /// First user-text content block observed in this batch (in line
    /// order). The caller assigns this to `WorkspaceEvents.first_user_text`
    /// only when the destination is currently `None` — once the first
    /// prompt is captured, subsequent user messages don't overwrite it.
    pub first_user_text: Option<String>,
    /// Tool-use category increments observed in this batch. The caller
    /// adds these into `WorkspaceEvents.tool_use_counts` (saturating).
    pub tool_use_counts: ToolUseCounts,
    /// File paths the agent touched in this batch, in source order
    /// (most-recent last). The caller push-fronts each entry into
    /// `WorkspaceEvents.recent_edited_files`, deduping consecutive
    /// same-path entries and bounding to 7.
    pub edited_file_paths: Vec<String>,
```

- [ ] **Step 4: Populate `first_user_text` in `parse_user`**

In `src/events.rs::parse_user` (around line 439), inside the `if let Some(text) = content.as_str()` branch, after `out.is_user_text = true;` and before the `return out;`, add:

```rust
        out.first_user_text = Some(text.to_string());
```

- [ ] **Step 5: Populate `edited_file_paths` in `parse_assistant`**

In `src/events.rs::parse_assistant` (around line 491), inside the `for block in blocks` loop, in the `"tool_use" =>` arm, after the existing `out.tool_use_starts.push(...)` block, add:

```rust
                if matches!(
                    name,
                    "Read" | "Edit" | "MultiEdit" | "Write" | "NotebookEdit"
                ) {
                    if let Some(p) = input.get("file_path").and_then(|p| p.as_str()) {
                        out.edited_file_paths.push(p.to_string());
                    }
                }
```

- [ ] **Step 6: Aggregate the new fields in `tail_session`**

In `src/events.rs::tail_session` (around line 334), inside the `loop { ... }` body, find the existing line `update.tool_use_starts.extend(parsed.tool_use_starts);` and the line below it `update.tool_use_resolves.extend(parsed.tool_use_resolves);`. Reorder so the borrow-based increments happen before the move-based extend, and add the three new aggregations:

```rust
        // Borrow parsed.tool_use_starts to increment counts BEFORE the
        // extend (which moves it).
        for (_id, name, _ts) in &parsed.tool_use_starts {
            update.tool_use_counts.increment(name);
        }
        update.tool_use_starts.extend(parsed.tool_use_starts);
        update.tool_use_resolves.extend(parsed.tool_use_resolves);
        update.edited_file_paths.extend(parsed.edited_file_paths);
        if update.first_user_text.is_none() {
            if let Some(t) = parsed.first_user_text {
                update.first_user_text = Some(t);
            }
        }
```

Leave the existing stop_reason / is_user_text / user_interrupt / last_assistant_text logic below this block UNCHANGED.

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test --lib events::tests::parse_user_surfaces_first_user_text events::tests::parse_user_omits_first_user_text_for_tool_results events::tests::parse_assistant_surfaces_edited_file_paths events::tests::parse_assistant_surfaces_read_paths events::tests::parse_assistant_skips_paths_for_non_file_tools events::tests::tail_session_aggregates_first_user_text_and_counts`
Expected: PASS.

- [ ] **Step 8: Run the full events test module**

Run: `cargo test --lib events::tests`
Expected: All existing events tests still pass.

- [ ] **Step 9: Commit**

```bash
git add src/events.rs
git commit -m "feat(events): extract first_user_text, tool_use_counts, edited file paths in tail loop"
```

---

## Task 3: Apply new `TailUpdate` fields to `WorkspaceEvents` in the tail loop (`app.rs`)

**Files:**
- Modify: `src/app.rs` (around line 2919 — `TailUpdate` destructure; around line 2950 — application of fields to `WorkspaceEvents`)

- [ ] **Step 1: Extend the `TailUpdate` destructure**

In `src/app.rs` around line 2919, replace the existing destructure:

```rust
                    let crate::events::TailUpdate {
                        new_offset,
                        events,
                        tool_use_starts,
                        tool_use_resolves,
                        last_stop_reason,
                        human_replied_after_last_stop,
                        reset_from_zero,
                        last_assistant_text,
                        last_user_interrupted,
                    } = update;
```

with:

```rust
                    let crate::events::TailUpdate {
                        new_offset,
                        events,
                        tool_use_starts,
                        tool_use_resolves,
                        last_stop_reason,
                        human_replied_after_last_stop,
                        reset_from_zero,
                        last_assistant_text,
                        last_user_interrupted,
                        first_user_text,
                        tool_use_counts,
                        edited_file_paths,
                    } = update;
```

- [ ] **Step 2: Apply the three new fields to `WorkspaceEvents`**

In the same block, immediately AFTER the `if let Some(text) = last_assistant_text { evt.last_assistant_text = Some(text); }` line (around line 2974), add:

```rust
                    if evt.first_user_text.is_none() {
                        if let Some(t) = first_user_text {
                            evt.first_user_text = Some(t);
                        }
                    }
                    evt.tool_use_counts.read =
                        evt.tool_use_counts.read.saturating_add(tool_use_counts.read);
                    evt.tool_use_counts.edit =
                        evt.tool_use_counts.edit.saturating_add(tool_use_counts.edit);
                    evt.tool_use_counts.write =
                        evt.tool_use_counts.write.saturating_add(tool_use_counts.write);
                    evt.tool_use_counts.bash =
                        evt.tool_use_counts.bash.saturating_add(tool_use_counts.bash);
                    evt.tool_use_counts.other =
                        evt.tool_use_counts.other.saturating_add(tool_use_counts.other);
                    for path in edited_file_paths {
                        if evt.recent_edited_files.front().map(|s| s.as_str()) != Some(&path) {
                            evt.recent_edited_files.push_front(path);
                            while evt.recent_edited_files.len() > 7 {
                                evt.recent_edited_files.pop_back();
                            }
                        }
                    }
```

- [ ] **Step 3: Build to verify the destructure matches**

Run: `cargo build --lib`
Expected: PASS (no missing-field-pattern errors).

- [ ] **Step 4: Run cargo check on the whole crate**

Run: `cargo check`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): apply derived TailUpdate fields to WorkspaceEvents"
```

---

## Task 4: Add `PaneFocus::DetailBarReply` variant and `reply_draft` on `DashboardState`

**Files:**
- Modify: `src/ui/mod.rs` (around line 23 — `PaneFocus` enum)
- Modify: `src/ui/dashboard/mod.rs` (around line 48 — `DashboardState` struct)
- Test: a new compile-only smoke test in `src/ui/dashboard/mod.rs`

- [ ] **Step 1: Write the failing test**

Add at the very bottom of `src/ui/dashboard/mod.rs` (NOT inside the existing `#[cfg(test)] mod tests;` reference — the actual tests file is `tests.rs`, but a quick default-check goes here):

```rust
#[cfg(test)]
mod state_defaults {
    use super::*;

    #[test]
    fn default_state_has_empty_reply_draft() {
        let s = DashboardState::default();
        assert_eq!(s.reply_draft, "");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib ui::dashboard::state_defaults::default_state_has_empty_reply_draft`
Expected: FAIL with "no field `reply_draft` on type `DashboardState`".

- [ ] **Step 3: Add the new variant to `PaneFocus`**

In `src/ui/mod.rs`, replace lines 22-26:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneFocus {
    Dashboard,
    ProjectManager,
}
```

with:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneFocus {
    Dashboard,
    ProjectManager,
    /// Reply input in the dashboard's detail bar. Active only while a
    /// workspace is selected. See `src/ui/dashboard/detail.rs`.
    DetailBarReply,
}
```

- [ ] **Step 4: Add `reply_draft` to `DashboardState`**

In `src/ui/dashboard/mod.rs`, in the `DashboardState` struct (around line 47-59), add at the bottom of the field list (before the closing `}`):

```rust
    /// In-flight reply text for the detail bar input. Tied to whichever
    /// workspace is selected at the time keystrokes arrived; cleared on
    /// selection change, Enter (send), or Esc (cancel).
    pub reply_draft: String,
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib ui::dashboard::state_defaults::default_state_has_empty_reply_draft`
Expected: PASS.

- [ ] **Step 6: Fix the one non-exhaustive `match focus` site**

The codebase has exactly one exhaustive `match focus { PaneFocus::ProjectManager => ..., PaneFocus::Dashboard => ... }` at `src/ui/pm_pane.rs:29-32`. Add the new arm to it (treat like `Dashboard` — when the user is in the detail-bar reply, the PM pane label should mirror the unfocused look):

```rust
    let label = match focus {
        PaneFocus::ProjectManager => "Project Manager [Tab/Esc back]",
        PaneFocus::Dashboard | PaneFocus::DetailBarReply => {
            "Project Manager [Tab to focus · r refresh]"
        }
    };
```

All other uses are `matches!(...)` patterns that don't need updating. Run:

Run: `cargo build --lib`
Expected: PASS.

- [ ] **Step 7: Run full test suite**

Run: `cargo test --lib`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src/ui/mod.rs src/ui/dashboard/mod.rs src/ui/pm_pane.rs
git commit -m "feat(ui): add PaneFocus::DetailBarReply variant + reply_draft state"
```

---

## Task 5: Create `src/ui/dashboard/detail.rs` skeleton with `preferred_height`

**Files:**
- Create: `src/ui/dashboard/detail.rs`
- Modify: `src/ui/dashboard/mod.rs` (around line 4 — module list)
- Test: `src/ui/dashboard/detail.rs` (new tests at bottom)

- [ ] **Step 1: Add the module declaration**

In `src/ui/dashboard/mod.rs`, in the `pub mod` declarations at the top (after line 8, where `pub mod status;` is), add:

```rust
pub mod detail;
```

- [ ] **Step 2: Write the failing tests for `preferred_height`**

Create `src/ui/dashboard/detail.rs` with:

```rust
//! Bottom-pinned detail bar shown when a workspace is selected on the
//! dashboard. Renders header strip, three-column body, and an inline
//! reply input.
//!
//! See `docs/superpowers/specs/2026-05-24-dashboard-workspace-detail-design.md`.

/// Minimum rows the bar needs to render usefully (1 header + 1 rule + 3
/// body + 1 rule + 1 input + 1 spacing slack).
pub const MIN_HEIGHT: u16 = 8;

/// Compute the detail bar's preferred height given the total available
/// height. Targets ~22% of the area, clamped to `[MIN_HEIGHT, 14]`.
pub fn preferred_height(total_height: u16) -> u16 {
    let target = (u32::from(total_height) * 22 / 100) as u16;
    target.clamp(MIN_HEIGHT, 14)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferred_height_clamps_to_min_on_short_terminal() {
        // 22% of 20 = 4 → clamps up to MIN_HEIGHT (8).
        assert_eq!(preferred_height(20), MIN_HEIGHT);
    }

    #[test]
    fn preferred_height_returns_22_percent_for_typical_terminal() {
        // 22% of 50 = 11 → within range.
        assert_eq!(preferred_height(50), 11);
    }

    #[test]
    fn preferred_height_clamps_to_14_on_tall_terminal() {
        // 22% of 100 = 22 → clamps down to 14.
        assert_eq!(preferred_height(100), 14);
    }

    #[test]
    fn preferred_height_handles_zero_height() {
        // 22% of 0 = 0 → clamps up to MIN_HEIGHT.
        assert_eq!(preferred_height(0), MIN_HEIGHT);
    }
}
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test --lib ui::dashboard::detail::tests`
Expected: All 4 tests PASS.

- [ ] **Step 4: Commit**

```bash
git add src/ui/dashboard/mod.rs src/ui/dashboard/detail.rs
git commit -m "feat(dashboard): add detail bar module skeleton with preferred_height"
```

---

## Task 6: Add `DetailInputs` struct and empty `render` entry point

**Files:**
- Modify: `src/ui/dashboard/detail.rs`
- Test: same file

- [ ] **Step 1: Write the failing test**

In `src/ui/dashboard/detail.rs` tests module, add:

```rust
    use crate::ui::dashboard::status::Status;
    use crate::ui::theme::Theme;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;

    fn render_to_text(inputs: &DetailInputs<'_>, w: u16, h: u16) -> String {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let theme = Theme::wsx();
                render(f, Rect::new(0, 0, w, h), inputs, &theme);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut s = String::new();
        for y in 0..h {
            for x in 0..w {
                s.push_str(buf[(x, y)].symbol());
            }
            s.push('\n');
        }
        s
    }

    fn seed_workspace() -> (crate::store::Store, crate::store::Repo, crate::store::Workspace) {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "ws",
                branch: "repo/ws",
                worktree_path: std::path::Path::new("/tmp/r/ws"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        store
            .set_workspace_state(id, WorkspaceState::Ready)
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == repo_id)
            .unwrap();
        let ws = store
            .workspaces(repo_id)
            .unwrap()
            .into_iter()
            .find(|w| w.id == id)
            .unwrap();
        (store, repo, ws)
    }

    #[test]
    fn render_into_zero_area_is_a_noop() {
        // Sanity: rendering into a zero-height area must not panic.
        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        let (_store, repo, ws) = seed_workspace();
        let result = terminal.draw(|f| {
            let theme = Theme::wsx();
            let inputs = DetailInputs {
                repo: &repo,
                workspace: &ws,
                events: None,
                procs: &[],
                diff: None,
                lifecycle: None,
                pr_title: None,
                pr_number: None,
                status: Status::Idle,
                ago_secs: None,
                reply_draft: "",
                reply_focused: false,
                events_scanned: false,
            };
            render(f, Rect::new(0, 0, 80, 0), &inputs, &theme);
        });
        assert!(result.is_ok());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib ui::dashboard::detail::tests::render_into_zero_area_is_a_noop`
Expected: FAIL with "cannot find type `DetailInputs`" or "function `render` is not defined".

- [ ] **Step 3: Add `DetailInputs` and `render` stub**

In `src/ui/dashboard/detail.rs`, immediately below the `preferred_height` function, add:

```rust
use crate::events::WorkspaceEvents;
use crate::forge::BranchLifecycle;
use crate::git::DiffStats;
use crate::proc::ProcInfo;
use crate::store::{Repo, Workspace};
use crate::ui::dashboard::status::Status;
use crate::ui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;

/// What `app.rs::draw` assembles for the detail bar. Borrowed for the
/// duration of a single draw call.
#[derive(Debug)]
pub struct DetailInputs<'a> {
    pub repo: &'a Repo,
    pub workspace: &'a Workspace,
    pub events: Option<&'a WorkspaceEvents>,
    pub procs: &'a [ProcInfo],
    pub diff: Option<DiffStats>,
    pub lifecycle: Option<BranchLifecycle>,
    pub pr_title: Option<String>,
    pub pr_number: Option<u32>,
    pub status: Status,
    pub ago_secs: Option<u64>,
    pub reply_draft: &'a str,
    pub reply_focused: bool,
    /// True once the workspace's JSONL has been scanned at least once
    /// (`workspace_events_scanned` on `App`). When false, SESSION
    /// SUMMARY and RECENT CHAT show `loading…` placeholders instead
    /// of derived content.
    pub events_scanned: bool,
}

/// Render the detail bar into `area`. No-op when `area.height < MIN_HEIGHT`
/// (caller is expected to fall back to a condensed banner — see
/// `app.rs::draw`).
pub fn render(f: &mut Frame, area: Rect, _inputs: &DetailInputs<'_>, _theme: &Theme) {
    if area.height == 0 || area.height < MIN_HEIGHT {
        return;
    }
    // Real rendering arrives in subsequent tasks.
    let _ = f;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib ui::dashboard::detail::tests::render_into_zero_area_is_a_noop`
Expected: PASS.

- [ ] **Step 5: Run cargo check**

Run: `cargo check --lib`
Expected: PASS (the new imports compile, but there will be `unused` warnings — acceptable for now).

- [ ] **Step 6: Commit**

```bash
git add src/ui/dashboard/detail.rs
git commit -m "feat(dashboard): add DetailInputs struct + render stub"
```

---

## Task 7: Implement header strip (top row of the detail bar)

**Files:**
- Modify: `src/ui/dashboard/detail.rs`
- Test: same file

The header strip is a single line, left to right:
`▍ <name>  ⎇ <branch>  <lifecycle-glyph> <pr-state>  +X −Y  ● Np procs  <status-glyph> <status-label> · <ago>`

- [ ] **Step 1: Write the failing test**

Add to the tests module in `src/ui/dashboard/detail.rs`:

```rust
    fn line_to_string(line: &ratatui::text::Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn header_strip_contains_all_chips_in_order() {
        let theme = Theme::wsx();
        let line = build_header_strip(
            "repo-overview",
            "bakedbean/repo-overview",
            Some(BranchLifecycle::PrOpen),
            Some(DiffStats { added: 12, removed: 3 }),
            2,
            Status::Question,
            Some(29),
            &theme,
            120,
        );
        let text = line_to_string(&line);
        assert!(text.contains("repo-overview"), "name missing: {text:?}");
        assert!(text.contains("bakedbean/repo-overview"), "branch missing: {text:?}");
        assert!(text.contains("+12") && text.contains("−3"), "diff missing: {text:?}");
        assert!(text.contains("● 2") || text.contains("2 procs"), "procs missing: {text:?}");
        assert!(text.contains("?"), "status glyph missing: {text:?}");
        assert!(text.contains("29s"), "ago missing: {text:?}");
    }

    #[test]
    fn header_strip_omits_diff_when_none() {
        let theme = Theme::wsx();
        let line = build_header_strip(
            "ws", "br", None, None, 0, Status::Idle, None, &theme, 80,
        );
        let text = line_to_string(&line);
        assert!(!text.contains("+"), "diff cell should be absent: {text:?}");
        assert!(!text.contains("−"), "diff cell should be absent: {text:?}");
    }

    #[test]
    fn header_strip_omits_lifecycle_when_none() {
        let theme = Theme::wsx();
        let line = build_header_strip(
            "ws", "br", None, None, 0, Status::Idle, None, &theme, 80,
        );
        let text = line_to_string(&line);
        // The PR lifecycle glyph set is { ⏺, ⏵, ⏷, ⏸ } (any specific
        // mapping in theme); none should appear when lifecycle is None.
        // Use a simple proxy: there's no "PR" or "open"/"merged" label.
        let lower = text.to_lowercase();
        assert!(!lower.contains("pr open"), "no pr label: {text:?}");
        assert!(!lower.contains("merged"), "no pr label: {text:?}");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib ui::dashboard::detail::tests::header_strip`
Expected: FAIL with "function `build_header_strip` not found".

- [ ] **Step 3: Implement `build_header_strip`**

In `src/ui/dashboard/detail.rs`, below the existing `render` stub, add:

```rust
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

const GUTTER: &str = "▍";

/// One-line header strip at the top of the bar.
pub(super) fn build_header_strip(
    name: &str,
    branch: &str,
    lifecycle: Option<BranchLifecycle>,
    diff: Option<DiffStats>,
    procs: u32,
    status: Status,
    ago_secs: Option<u64>,
    theme: &Theme,
    width: usize,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled(GUTTER.to_string(), theme.status_style(status)));
    spans.push(Span::raw(" ".to_string()));
    spans.push(Span::styled(
        name.to_string(),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::raw("  ".to_string()));
    spans.push(Span::styled(format!("⎇ {branch}"), theme.dim_style()));

    if let Some(lc) = lifecycle {
        spans.push(Span::raw("  ".to_string()));
        let (glyph, label) = lifecycle_chip(lc);
        spans.push(Span::styled(
            format!("{glyph} {label}"),
            theme.lifecycle_style(Some(lc)).unwrap_or_else(|| theme.dim_style()),
        ));
    }

    if let Some(d) = diff {
        if d.added > 0 || d.removed > 0 {
            spans.push(Span::raw("  ".to_string()));
            spans.push(Span::styled(format!("+{}", d.added), theme.ok_style()));
            spans.push(Span::raw(" ".to_string()));
            spans.push(Span::styled(format!("−{}", d.removed), theme.err_style()));
        }
    }

    spans.push(Span::raw("  ".to_string()));
    if procs > 0 {
        spans.push(Span::styled(
            format!("● {procs} procs"),
            theme.status_style(Status::Thinking),
        ));
    } else {
        spans.push(Span::styled("  · 0 procs".to_string(), theme.dim_style()));
    }

    spans.push(Span::raw("  ".to_string()));
    spans.push(Span::styled(
        status.glyph().to_string(),
        theme.status_style(status),
    ));
    spans.push(Span::raw(" ".to_string()));
    spans.push(Span::styled(
        status.label().to_string(),
        theme.status_style(status),
    ));

    let ago = format_ago_short(ago_secs);
    spans.push(Span::styled(format!("  · {ago}"), theme.dim_style()));

    // Right-truncate the full line to `width` cells by padding or
    // dropping spans — for v1 we trust the caller to give us enough
    // room (width >= 60); narrow-width handling is in Task 12.
    let _ = width;
    Line::from(spans)
}

fn lifecycle_chip(lc: BranchLifecycle) -> (&'static str, &'static str) {
    match lc {
        BranchLifecycle::PrOpen => ("⏺", "open"),
        BranchLifecycle::PrDraft => ("⏷", "draft"),
        BranchLifecycle::PrMerged => ("⏺", "merged"),
        BranchLifecycle::PrClosed => ("⏸", "closed"),
        BranchLifecycle::PrConflicted => ("⏺", "conflict"),
        BranchLifecycle::NoPr => ("", ""),
    }
}

fn format_ago_short(secs: Option<u64>) -> String {
    match secs {
        None => "—".to_string(),
        Some(s) if s < 60 => format!("{s}s"),
        Some(s) if s < 3600 => format!("{}m", s / 60),
        Some(s) => format!("{}h", s / 3600),
    }
}
```

NOTE on `lifecycle_style`: this is a `Theme` method already used by `src/ui/dashboard/row.rs:177`. If the signature differs from `Option<BranchLifecycle> -> Option<Style>` (e.g. it's `BranchLifecycle -> Style`), read `src/ui/theme.rs` and adjust the call.

NOTE on `NoPr`: `BranchLifecycle::NoPr` is an existing variant treated as "no PR exists". The chip suppresses to empty strings; the caller filters via the `if let Some(lc)` outside, so `NoPr` from outside is not expected — but the empty-string fallback makes the renderer safe if it ever arrives.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib ui::dashboard::detail::tests::header_strip`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ui/dashboard/detail.rs
git commit -m "feat(dashboard): render detail bar header strip"
```

---

## Task 8: Implement SESSION SUMMARY column

**Files:**
- Modify: `src/ui/dashboard/detail.rs`
- Test: same file

- [ ] **Step 1: Write the failing tests**

Add to the tests module in `src/ui/dashboard/detail.rs`:

```rust
    fn make_events_with(
        first: Option<&str>,
        counts: ToolUseCounts,
        last_assistant: Option<&str>,
    ) -> WorkspaceEvents {
        let mut e = WorkspaceEvents::default();
        e.first_user_text = first.map(str::to_string);
        e.tool_use_counts = counts;
        e.last_assistant_text = last_assistant.map(str::to_string);
        e
    }

    #[test]
    fn session_summary_renders_initial_prompt_when_present() {
        let theme = Theme::wsx();
        let evt = make_events_with(Some("summarize the repo"), ToolUseCounts::default(), None);
        let lines = build_session_summary(Some(&evt), &theme, 50, "/tmp/wt", 0);
        let joined: String = lines.iter().map(line_to_string).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("summarize the repo"), "{joined:?}");
    }

    #[test]
    fn session_summary_tool_trace_omits_zero_counts() {
        let theme = Theme::wsx();
        let evt = make_events_with(None, ToolUseCounts { read: 5, edit: 0, write: 0, bash: 2, other: 0 }, None);
        let lines = build_session_summary(Some(&evt), &theme, 50, "/tmp/wt", 0);
        let joined: String = lines.iter().map(line_to_string).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("read 5 files"), "{joined:?}");
        assert!(joined.contains("ran 2 commands"), "{joined:?}");
        assert!(!joined.contains("edited"), "edit fragment should be omitted: {joined:?}");
        assert!(!joined.contains("wrote"), "write fragment should be omitted: {joined:?}");
    }

    #[test]
    fn session_summary_singular_plural_forms() {
        let theme = Theme::wsx();
        let evt = make_events_with(None, ToolUseCounts { read: 1, edit: 1, write: 1, bash: 1, other: 1 }, None);
        let lines = build_session_summary(Some(&evt), &theme, 50, "/tmp/wt", 0);
        let joined: String = lines.iter().map(line_to_string).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("read 1 file") && !joined.contains("read 1 files"), "{joined:?}");
        assert!(joined.contains("edited 1 file"), "{joined:?}");
        assert!(joined.contains("ran 1 command"), "{joined:?}");
    }

    #[test]
    fn session_summary_shows_loading_when_events_none() {
        let theme = Theme::wsx();
        let lines = build_session_summary(None, &theme, 50, "/tmp/wt", 0);
        let joined: String = lines.iter().map(line_to_string).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("loading"), "{joined:?}");
    }

    #[test]
    fn session_summary_includes_worktree_path() {
        let theme = Theme::wsx();
        let evt = make_events_with(None, ToolUseCounts::default(), None);
        let lines = build_session_summary(Some(&evt), &theme, 60, "/tmp/very/long/path/workspaces/foo", 120);
        let joined: String = lines.iter().map(line_to_string).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("foo") || joined.contains("workspaces"), "basename retained: {joined:?}");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib ui::dashboard::detail::tests::session_summary`
Expected: FAIL with "function `build_session_summary` not found".

- [ ] **Step 3: Implement `build_session_summary`**

In `src/ui/dashboard/detail.rs`, add at the bottom (above the tests module):

```rust
use crate::events::ToolUseCounts;

/// Build the lines that make up the SESSION SUMMARY column. Returns a
/// Vec because the caller is responsible for slicing to fit the body
/// area height.
pub(super) fn build_session_summary(
    events: Option<&WorkspaceEvents>,
    theme: &Theme,
    column_width: usize,
    worktree_path: &str,
    created_secs: u64,
) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let label_style = Style::default().fg(theme.path).add_modifier(Modifier::BOLD);
    out.push(Line::from(Span::styled("SESSION SUMMARY".to_string(), label_style)));

    let Some(evt) = events else {
        out.push(Line::from(Span::styled(
            "  loading…".to_string(),
            theme.dim_style(),
        )));
        return out;
    };

    let prefix = Span::styled("▸ ".to_string(), theme.dim_style());

    if let Some(prompt) = evt.first_user_text.as_deref() {
        let truncated = truncate_to_chars(prompt, column_width.saturating_sub(4));
        out.push(Line::from(vec![
            prefix.clone(),
            Span::styled(
                format!("\"{truncated}\""),
                Style::default().add_modifier(Modifier::ITALIC),
            ),
        ]));
    }

    let trace = format_tool_trace(&evt.tool_use_counts);
    if !trace.is_empty() {
        out.push(Line::from(vec![
            prefix.clone(),
            Span::raw(truncate_to_chars(&trace, column_width.saturating_sub(2))),
        ]));
    }

    let now_signal = format_where_now(evt);
    if !now_signal.is_empty() {
        out.push(Line::from(vec![
            prefix.clone(),
            Span::raw(truncate_to_chars(&now_signal, column_width.saturating_sub(2))),
        ]));
    }

    // (PR row is wired but always omitted in v1 — pr_title/pr_number arrive as None.)

    let age = format_ago_short(Some(created_secs));
    let path_text = format!("{worktree_path} · created {age}");
    let path_truncated = truncate_to_chars_left(&path_text, column_width.saturating_sub(2));
    out.push(Line::from(vec![
        prefix.clone(),
        Span::styled(path_truncated, theme.dim_style()),
    ]));

    out
}

fn format_tool_trace(counts: &ToolUseCounts) -> String {
    let mut parts: Vec<String> = Vec::new();
    if counts.read > 0 {
        parts.push(format!("read {} {}", counts.read, plural("file", counts.read)));
    }
    if counts.edit > 0 {
        parts.push(format!("edited {} {}", counts.edit, plural("file", counts.edit)));
    }
    if counts.write > 0 {
        parts.push(format!("wrote {} {}", counts.write, plural("file", counts.write)));
    }
    if counts.bash > 0 {
        parts.push(format!("ran {} {}", counts.bash, plural("command", counts.bash)));
    }
    if counts.other > 0 {
        parts.push(format!("+{} other actions", counts.other));
    }
    parts.join(", ")
}

fn plural(noun: &str, n: u32) -> String {
    if n == 1 {
        noun.to_string()
    } else {
        format!("{noun}s")
    }
}

fn format_where_now(evt: &WorkspaceEvents) -> String {
    if let Some(q) = evt.pending_question_tool() {
        return format!("agent asked via {q}");
    }
    // Pending non-question permission tool, if any:
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    if let Some((name, _)) = evt.pending_permission_tool(now_ms, 0) {
        return format!("awaiting permission for {name}");
    }
    if let Some(t) = evt.last_assistant_text.as_deref() {
        let first_line = t.lines().next().unwrap_or(t);
        return first_line.to_string();
    }
    String::new()
}

fn truncate_to_chars(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let count = s.chars().count();
    if count <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn truncate_to_chars_left(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let count = s.chars().count();
    if count <= max {
        s.to_string()
    } else {
        let skip = count.saturating_sub(max.saturating_sub(1));
        let tail: String = s.chars().skip(skip).collect();
        format!("…{tail}")
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib ui::dashboard::detail::tests::session_summary`
Expected: PASS (all five tests).

- [ ] **Step 5: Commit**

```bash
git add src/ui/dashboard/detail.rs
git commit -m "feat(dashboard): build SESSION SUMMARY column for detail bar"
```

---

## Task 9: Implement RECENT CHAT column

**Files:**
- Modify: `src/ui/dashboard/detail.rs`
- Test: same file

- [ ] **Step 1: Write the failing tests**

Add to the tests module in `src/ui/dashboard/detail.rs`:

```rust
    #[test]
    fn recent_chat_renders_em_dash_when_no_assistant_text() {
        let theme = Theme::wsx();
        let evt = make_events_with(None, ToolUseCounts::default(), None);
        let lines = build_recent_chat(Some(&evt), &theme, 40, 6);
        let joined: String = lines.iter().map(line_to_string).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("—"), "{joined:?}");
    }

    #[test]
    fn recent_chat_renders_assistant_text_wrapped() {
        let theme = Theme::wsx();
        let evt = make_events_with(
            None,
            ToolUseCounts::default(),
            Some("This is a longer assistant message that should wrap across multiple lines when the column width is small."),
        );
        let lines = build_recent_chat(Some(&evt), &theme, 30, 6);
        // Expect at least 2 lines (label + ≥1 content line); total ≤ 1 (label) + 6 (max).
        assert!(lines.len() >= 2 && lines.len() <= 7, "got {} lines", lines.len());
        let joined: String = lines.iter().map(line_to_string).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("longer assistant"), "content present: {joined:?}");
    }

    #[test]
    fn recent_chat_shows_loading_when_events_none() {
        let theme = Theme::wsx();
        let lines = build_recent_chat(None, &theme, 40, 6);
        let joined: String = lines.iter().map(line_to_string).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("loading"), "{joined:?}");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib ui::dashboard::detail::tests::recent_chat`
Expected: FAIL with "function `build_recent_chat` not found".

- [ ] **Step 3: Implement `build_recent_chat`**

In `src/ui/dashboard/detail.rs`, add below `build_session_summary`:

```rust
/// Build the RECENT CHAT column. `max_body_lines` caps how many content
/// lines render below the column label.
pub(super) fn build_recent_chat(
    events: Option<&WorkspaceEvents>,
    theme: &Theme,
    column_width: usize,
    max_body_lines: usize,
) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let label_style = Style::default().fg(theme.path).add_modifier(Modifier::BOLD);
    out.push(Line::from(Span::styled("RECENT CHAT".to_string(), label_style)));

    let Some(evt) = events else {
        out.push(Line::from(Span::styled(
            "  loading…".to_string(),
            theme.dim_style(),
        )));
        return out;
    };

    let Some(text) = evt.last_assistant_text.as_deref() else {
        out.push(Line::from(Span::styled("—".to_string(), theme.dim_style())));
        return out;
    };

    // Word-wrap to column_width. Take the last `max_body_lines` after wrapping.
    let wrapped = wrap_lines(text, column_width);
    let take = wrapped.len().saturating_sub(0); // we want the last N
    let start = take.saturating_sub(max_body_lines);
    for line in wrapped.iter().skip(start) {
        out.push(Line::from(Span::styled(line.clone(), theme.dim_style())));
    }
    out
}

/// Greedy word-wrap. Splits long words at the column boundary.
fn wrap_lines(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    for paragraph in text.split('\n') {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if word.chars().count() > width {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
                let mut buf: String = String::new();
                for ch in word.chars() {
                    if buf.chars().count() == width {
                        out.push(std::mem::take(&mut buf));
                    }
                    buf.push(ch);
                }
                if !buf.is_empty() {
                    current = buf;
                }
                continue;
            }
            let projected = if current.is_empty() {
                word.chars().count()
            } else {
                current.chars().count() + 1 + word.chars().count()
            };
            if projected > width {
                out.push(std::mem::take(&mut current));
                current.push_str(word);
            } else {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
            }
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib ui::dashboard::detail::tests::recent_chat`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ui/dashboard/detail.rs
git commit -m "feat(dashboard): build RECENT CHAT column with word-wrap"
```

---

## Task 10: Implement PROCESSES + RECENT FILES column

**Files:**
- Modify: `src/ui/dashboard/detail.rs`
- Test: same file

- [ ] **Step 1: Write the failing tests**

Add to the tests module in `src/ui/dashboard/detail.rs`:

```rust
    fn proc(cmd: &str) -> ProcInfo {
        ProcInfo {
            pid: 1234,
            ppid: 1,
            command: cmd.into(),
            cwd: std::path::PathBuf::from("/tmp"),
        }
    }

    #[test]
    fn procs_column_shows_dash_when_empty() {
        let theme = Theme::wsx();
        let evt = make_events_with(None, ToolUseCounts::default(), None);
        let lines = build_procs_and_files(&[], Some(&evt), &theme, 30);
        let joined: String = lines.iter().map(line_to_string).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("—"), "expected em-dash when no procs/files: {joined:?}");
    }

    #[test]
    fn procs_column_truncates_with_plus_n_more() {
        let theme = Theme::wsx();
        let evt = make_events_with(None, ToolUseCounts::default(), None);
        let procs: Vec<ProcInfo> = (0..7).map(|i| proc(&format!("cmd{i}"))).collect();
        let lines = build_procs_and_files(&procs, Some(&evt), &theme, 30);
        let joined: String = lines.iter().map(line_to_string).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("+2 more"), "expected +2 more: {joined:?}");
    }

    #[test]
    fn recent_files_section_renders_paths() {
        let theme = Theme::wsx();
        let mut evt = make_events_with(None, ToolUseCounts::default(), None);
        evt.recent_edited_files.push_front("/tmp/x/src/main.rs".to_string());
        evt.recent_edited_files.push_front("/tmp/x/Cargo.toml".to_string());
        let lines = build_procs_and_files(&[], Some(&evt), &theme, 30);
        let joined: String = lines.iter().map(line_to_string).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("Cargo.toml"), "expected Cargo.toml in output: {joined:?}");
        assert!(joined.contains("main.rs"), "expected main.rs in output: {joined:?}");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib ui::dashboard::detail::tests::procs_column ui::dashboard::detail::tests::recent_files`
Expected: FAIL with "function `build_procs_and_files` not found".

- [ ] **Step 3: Implement `build_procs_and_files`**

In `src/ui/dashboard/detail.rs`, add below `build_recent_chat`:

```rust
/// Build the PROCESSES + RECENT FILES column. Procs go on top, recent
/// files (from `WorkspaceEvents.recent_edited_files`) below.
pub(super) fn build_procs_and_files(
    procs: &[ProcInfo],
    events: Option<&WorkspaceEvents>,
    theme: &Theme,
    column_width: usize,
) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let label_style = Style::default().fg(theme.path).add_modifier(Modifier::BOLD);

    out.push(Line::from(Span::styled("PROCESSES".to_string(), label_style)));
    if procs.is_empty() {
        out.push(Line::from(Span::styled("—".to_string(), theme.dim_style())));
    } else {
        let visible = procs.iter().take(5);
        for p in visible {
            let cmd = truncate_to_chars(&p.command, column_width.saturating_sub(4));
            out.push(Line::from(vec![
                Span::styled("● ".to_string(), theme.status_style(Status::Thinking)),
                Span::styled(cmd, theme.dim_style()),
            ]));
        }
        if procs.len() > 5 {
            out.push(Line::from(Span::styled(
                format!("+{} more", procs.len() - 5),
                theme.dim_style(),
            )));
        }
    }

    out.push(Line::from(Span::styled("RECENT FILES".to_string(), label_style)));
    let files: Vec<&String> = events
        .map(|e| e.recent_edited_files.iter().collect())
        .unwrap_or_default();
    if files.is_empty() {
        out.push(Line::from(Span::styled("—".to_string(), theme.dim_style())));
    } else {
        for f in files.iter().take(5) {
            let truncated = truncate_to_chars_left(f, column_width);
            out.push(Line::from(Span::styled(truncated, theme.dim_style())));
        }
    }
    out
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib ui::dashboard::detail::tests::procs_column ui::dashboard::detail::tests::recent_files`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ui/dashboard/detail.rs
git commit -m "feat(dashboard): build PROCESSES + RECENT FILES column"
```

---

## Task 11: Implement reply input row

**Files:**
- Modify: `src/ui/dashboard/detail.rs`
- Test: same file

- [ ] **Step 1: Write the failing tests**

Add to the tests module in `src/ui/dashboard/detail.rs`:

```rust
    #[test]
    fn reply_input_row_shows_chip_and_draft() {
        let theme = Theme::wsx();
        let line = build_reply_row("hello agent", false, &theme, 80);
        let text = line_to_string(&line);
        assert!(text.contains("Reply to agent"), "chip present: {text:?}");
        assert!(text.contains("hello agent"), "draft present: {text:?}");
    }

    #[test]
    fn reply_input_row_shows_send_hint_when_focused() {
        let theme = Theme::wsx();
        let line = build_reply_row("", true, &theme, 80);
        let text = line_to_string(&line);
        assert!(text.contains("send"), "send hint present when focused: {text:?}");
        assert!(text.contains("cancel"), "cancel hint present when focused: {text:?}");
    }

    #[test]
    fn reply_input_row_hides_hints_when_unfocused() {
        let theme = Theme::wsx();
        let line = build_reply_row("", false, &theme, 80);
        let text = line_to_string(&line);
        assert!(!text.contains("send"), "send hint absent when unfocused: {text:?}");
        assert!(!text.contains("cancel"), "cancel hint absent when unfocused: {text:?}");
    }

    #[test]
    fn reply_input_row_scrolls_long_drafts_to_end() {
        // A long draft must show its END (where the cursor lives), not
        // its beginning — otherwise the user can't see what they're typing.
        let theme = Theme::wsx();
        let long: String = "a".repeat(60);
        // Construct with " END" appended so we can detect that the tail is visible.
        let draft = format!("{long} END");
        let line = build_reply_row(&draft, true, &theme, 60);
        let text = line_to_string(&line);
        assert!(text.contains("END"), "tail of draft visible: {text:?}");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib ui::dashboard::detail::tests::reply_input_row`
Expected: FAIL with "function `build_reply_row` not found".

- [ ] **Step 3: Implement `build_reply_row`**

In `src/ui/dashboard/detail.rs`, add below `build_procs_and_files`:

```rust
const REPLY_CHIP: &str = "┃ Reply to agent ┃";
const REPLY_HINT: &str = "  ↵ send · Esc cancel";

/// Reply input row. Returns a `Line` plus an optional cursor X-offset
/// (within the line) that the caller passes to `f.set_cursor_position`
/// when `focused == true`. The caller adds `area.x` and the row's `y`.
pub(super) fn build_reply_row(
    draft: &str,
    focused: bool,
    theme: &Theme,
    width: usize,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let chip_style = if focused {
        Style::default().fg(theme.path).add_modifier(Modifier::BOLD)
    } else {
        theme.dim_style()
    };
    spans.push(Span::styled(REPLY_CHIP.to_string(), chip_style));
    spans.push(Span::raw(" ".to_string()));

    let hint_width = if focused { REPLY_HINT.chars().count() } else { 0 };
    let chip_width = REPLY_CHIP.chars().count() + 1; // chip + 1 trailing space
    let field_width = width
        .saturating_sub(chip_width)
        .saturating_sub(hint_width)
        .max(1);

    // Right-align the cursor in the visible window: take the LAST
    // `field_width - 1` chars (reserve 1 cell for the cursor when
    // focused; when unfocused that cell holds the trailing space).
    let cursor_room = if focused { 1 } else { 0 };
    let visible_chars = field_width.saturating_sub(cursor_room).max(1);
    let total = draft.chars().count();
    let skip = total.saturating_sub(visible_chars);
    let visible: String = draft.chars().skip(skip).collect();
    let padding = field_width.saturating_sub(visible.chars().count() + cursor_room);
    spans.push(Span::styled(visible, Style::default()));
    if padding > 0 {
        spans.push(Span::raw(" ".repeat(padding)));
    }

    if focused {
        spans.push(Span::styled(REPLY_HINT.to_string(), theme.dim_style()));
    }

    Line::from(spans)
}

/// Cursor x-offset (within the reply row) when focused. Returns the
/// column where `f.set_cursor_position` should be set.
pub(super) fn reply_cursor_x(draft: &str, width: usize) -> u16 {
    let chip_width = REPLY_CHIP.chars().count() + 1;
    let hint_width = REPLY_HINT.chars().count();
    let field_width = width
        .saturating_sub(chip_width)
        .saturating_sub(hint_width)
        .max(1);
    let visible_chars = field_width.saturating_sub(1).max(1);
    let total = draft.chars().count();
    let visible_count = total.min(visible_chars);
    (chip_width + visible_count) as u16
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib ui::dashboard::detail::tests::reply_input_row`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ui/dashboard/detail.rs
git commit -m "feat(dashboard): build reply input row with cursor offset helper"
```

---

## Task 12: Wire `render()` to compose header, body, rules, and reply row

**Files:**
- Modify: `src/ui/dashboard/detail.rs`
- Test: same file

- [ ] **Step 1: Write the failing test**

Add to the tests module in `src/ui/dashboard/detail.rs`:

```rust
    #[test]
    fn full_render_paints_header_body_and_reply_row() {
        let theme = Theme::wsx();
        let (_store, repo, ws) = seed_workspace();
        let mut evt = WorkspaceEvents::default();
        evt.first_user_text = Some("give me a tour".into());
        evt.tool_use_counts.read = 14;
        evt.tool_use_counts.bash = 2;
        evt.last_assistant_text = Some("Reading the repo now.".into());
        let inputs = DetailInputs {
            repo: &repo,
            workspace: &ws,
            events: Some(&evt),
            procs: &[],
            diff: Some(DiffStats { added: 12, removed: 3 }),
            lifecycle: Some(BranchLifecycle::PrOpen),
            pr_title: None,
            pr_number: None,
            status: Status::Question,
            ago_secs: Some(29),
            reply_draft: "",
            reply_focused: false,
            events_scanned: true,
        };
        let text = render_to_text(&inputs, 120, 10);
        assert!(text.contains("repo-overview"), "header name: {text:?}");
        assert!(text.contains("SESSION SUMMARY"), "summary label: {text:?}");
        assert!(text.contains("RECENT CHAT"), "chat label: {text:?}");
        assert!(text.contains("PROCESSES"), "procs label: {text:?}");
        assert!(text.contains("Reply to agent"), "reply chip: {text:?}");
        assert!(text.contains("give me a tour"), "initial prompt: {text:?}");
        assert!(text.contains("Reading the repo"), "recent chat: {text:?}");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib ui::dashboard::detail::tests::full_render_paints_header_body_and_reply_row`
Expected: FAIL (currently `render` is a no-op stub from Task 6, so labels are absent).

- [ ] **Step 3: Implement the composed `render`**

In `src/ui/dashboard/detail.rs`, REPLACE the existing `render` stub from Task 6 with:

```rust
pub fn render(f: &mut Frame, area: Rect, inputs: &DetailInputs<'_>, theme: &Theme) {
    if area.height == 0 || area.height < MIN_HEIGHT {
        return;
    }
    use ratatui::layout::{Constraint, Direction, Layout};
    use ratatui::widgets::Paragraph;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header strip
            Constraint::Length(1), // rule
            Constraint::Min(1),    // body (3 columns)
            Constraint::Length(1), // rule
            Constraint::Length(1), // reply row
        ])
        .split(area);

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let created_secs = now_secs.saturating_sub(inputs.workspace.created_at as u64);

    let header = build_header_strip(
        &inputs.workspace.name,
        &inputs.workspace.branch,
        inputs.lifecycle,
        inputs.diff,
        inputs.procs.len() as u32,
        inputs.status,
        inputs.ago_secs,
        theme,
        chunks[0].width as usize,
    );
    f.render_widget(Paragraph::new(header), chunks[0]);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "─".repeat(chunks[1].width as usize),
            theme.dim_style(),
        ))),
        chunks[1],
    );

    // Body: 3 columns 30/40/30.
    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Percentage(40),
            Constraint::Percentage(30),
        ])
        .split(chunks[2]);
    let summary_lines = build_session_summary(
        if inputs.events_scanned { inputs.events } else { None },
        theme,
        body_chunks[0].width as usize,
        &inputs.workspace.worktree_path.to_string_lossy(),
        created_secs,
    );
    let chat_lines = build_recent_chat(
        if inputs.events_scanned { inputs.events } else { None },
        theme,
        body_chunks[1].width as usize,
        (chunks[2].height as usize).saturating_sub(1).max(1),
    );
    let procs_lines = build_procs_and_files(
        inputs.procs,
        inputs.events,
        theme,
        body_chunks[2].width as usize,
    );
    f.render_widget(
        Paragraph::new(summary_lines),
        body_chunks[0],
    );
    f.render_widget(Paragraph::new(chat_lines), body_chunks[1]);
    f.render_widget(Paragraph::new(procs_lines), body_chunks[2]);

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "─".repeat(chunks[3].width as usize),
            theme.dim_style(),
        ))),
        chunks[3],
    );

    let reply = build_reply_row(
        inputs.reply_draft,
        inputs.reply_focused,
        theme,
        chunks[4].width as usize,
    );
    f.render_widget(Paragraph::new(reply), chunks[4]);

    if inputs.reply_focused {
        let cx = reply_cursor_x(inputs.reply_draft, chunks[4].width as usize);
        f.set_cursor_position((chunks[4].x + cx, chunks[4].y));
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib ui::dashboard::detail::tests::full_render_paints_header_body_and_reply_row`
Expected: PASS.

- [ ] **Step 5: Run the full detail tests**

Run: `cargo test --lib ui::dashboard::detail::tests`
Expected: All detail tests PASS.

- [ ] **Step 6: Commit**

```bash
git add src/ui/dashboard/detail.rs
git commit -m "feat(dashboard): compose full detail bar render"
```

---

## Task 13: Responsive collapse — narrow width + short height

**Files:**
- Modify: `src/ui/dashboard/detail.rs`
- Test: same file

- [ ] **Step 1: Write the failing tests**

Add to the tests module in `src/ui/dashboard/detail.rs`:

```rust
    #[test]
    fn narrow_terminal_drops_chat_and_procs_columns() {
        let theme = Theme::wsx();
        let (_store, repo, ws) = seed_workspace();
        let mut evt = WorkspaceEvents::default();
        evt.first_user_text = Some("hi".into());
        evt.last_assistant_text = Some("ack".into());
        let inputs = DetailInputs {
            repo: &repo,
            workspace: &ws,
            events: Some(&evt),
            procs: &[],
            diff: None,
            lifecycle: None,
            pr_title: None,
            pr_number: None,
            status: Status::Idle,
            ago_secs: None,
            reply_draft: "",
            reply_focused: false,
            events_scanned: true,
        };
        let text = render_to_text(&inputs, 70, 10);
        assert!(text.contains("SESSION SUMMARY"), "summary kept: {text:?}");
        assert!(!text.contains("RECENT CHAT"), "chat dropped on narrow: {text:?}");
        assert!(!text.contains("PROCESSES"), "procs dropped on narrow: {text:?}");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib ui::dashboard::detail::tests::narrow_terminal_drops_chat_and_procs_columns`
Expected: FAIL (current 3-column render emits all three labels regardless of width).

- [ ] **Step 3: Update `render` to branch on width**

In `src/ui/dashboard/detail.rs::render`, replace the body block (from `// Body: 3 columns 30/40/30.` through the three `f.render_widget(...)` calls into `body_chunks`) with:

```rust
    // Body: 3 columns on wide terminals, single column on narrow.
    if chunks[2].width >= 80 {
        let body_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(30),
                Constraint::Percentage(40),
                Constraint::Percentage(30),
            ])
            .split(chunks[2]);
        let summary_lines = build_session_summary(
            if inputs.events_scanned { inputs.events } else { None },
            theme,
            body_chunks[0].width as usize,
            &inputs.workspace.worktree_path.to_string_lossy(),
            created_secs,
        );
        let chat_lines = build_recent_chat(
            if inputs.events_scanned { inputs.events } else { None },
            theme,
            body_chunks[1].width as usize,
            (chunks[2].height as usize).saturating_sub(1).max(1),
        );
        let procs_lines = build_procs_and_files(
            inputs.procs,
            inputs.events,
            theme,
            body_chunks[2].width as usize,
        );
        f.render_widget(Paragraph::new(summary_lines), body_chunks[0]);
        f.render_widget(Paragraph::new(chat_lines), body_chunks[1]);
        f.render_widget(Paragraph::new(procs_lines), body_chunks[2]);
    } else {
        let summary_lines = build_session_summary(
            if inputs.events_scanned { inputs.events } else { None },
            theme,
            chunks[2].width as usize,
            &inputs.workspace.worktree_path.to_string_lossy(),
            created_secs,
        );
        f.render_widget(Paragraph::new(summary_lines), chunks[2]);
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib ui::dashboard::detail::tests::narrow_terminal_drops_chat_and_procs_columns`
Expected: PASS.

- [ ] **Step 5: Run full detail test module to verify the 120-cell tests still pass**

Run: `cargo test --lib ui::dashboard::detail::tests`
Expected: All detail tests PASS.

- [ ] **Step 6: Commit**

```bash
git add src/ui/dashboard/detail.rs
git commit -m "feat(dashboard): collapse detail bar to single column on narrow terminals"
```

---

## Task 14: Integrate the detail bar into `app.rs::draw`

**Files:**
- Modify: `src/app.rs` (around line 698 — `if app.pm_visible { ... }` block in `draw`)
- Test: `src/ui/dashboard/tests.rs`

- [ ] **Step 1: Write the failing tests**

Add to the existing `#[cfg(test)] mod pm_state_tests` in `src/app.rs` (which already uses the `Store::open_in_memory()` + `add_repo` + `insert_workspace` pattern — mirror it). Place these tests at the end of that module:

```rust
    fn seed_app_with_workspace() -> App {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "alpha",
                branch: "repo/alpha",
                worktree_path: std::path::Path::new("/tmp/wsx-test/alpha"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        store
            .set_workspace_state(id, WorkspaceState::Ready)
            .unwrap();
        App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap()
    }

    #[test]
    fn detail_bar_renders_when_workspace_is_selected() {
        let mut app = seed_app_with_workspace();
        let idx = app
            .selectable
            .iter()
            .position(|t| matches!(t, SelectionTarget::Workspace(_)))
            .expect("workspace target present");
        app.dashboard.selected = idx;

        let backend = TestBackend::new(120, 30);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw_for_test(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        let rendered = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("Reply to agent"), "bar visible: {rendered}");
    }

    #[test]
    fn detail_bar_absent_when_repo_header_is_selected() {
        let mut app = seed_app_with_workspace();
        let repo_idx = app
            .selectable
            .iter()
            .position(|t| matches!(t, SelectionTarget::Repo(_)))
            .expect("repo target present");
        app.dashboard.selected = repo_idx;

        let backend = TestBackend::new(120, 30);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw_for_test(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        let rendered = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !rendered.contains("Reply to agent"),
            "bar absent on repo header: {rendered}"
        );
    }
```

The `seed_app_with_workspace` helper is reused in Tasks 15 and 16 — keep it in the same `pm_state_tests` module (or move it to a shared `test_helpers` module if Tasks 15/16 are written in a different `mod` block).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib pm_state_tests::detail_bar_renders_when_workspace_is_selected pm_state_tests::detail_bar_absent_when_repo_header_is_selected`
Expected: FAIL with "bar visible" — the bar isn't yet wired into `app.rs::draw`.

- [ ] **Step 3: Add the `dashboard_regions` helper and `DetailInputs` assembly to `app.rs::draw`**

In `src/app.rs`, inside the `View::Dashboard` arm of `draw` (around line 697), REPLACE the current `if app.pm_visible { ... } else { (area, None) }` block AND everything down through the `dashboard::render(...)` call AND the `if let Some(pm_area) = pm_area` block with the following expanded logic. The full replacement (preserving the existing surrounding code that builds `workspaces`, `inputs`, `activity`, etc.) reads:

```rust
        View::Dashboard => {
            // Compute the three layout regions up front. Detail bar
            // only shows when a workspace is selected; PM keeps its
            // existing behavior.
            let selection_is_workspace = matches!(
                app.selected_target(),
                Some(SelectionTarget::Workspace(_))
            );
            let detail_visible = selection_is_workspace
                && area.height >= crate::ui::dashboard::detail::MIN_HEIGHT + 10;
            let (dashboard_area, detail_area, pm_area) =
                dashboard_regions(area, app.pm_visible, detail_visible);

            let notifications_on = notifications_enabled(&app.store);
            let nerd_fonts = nerd_fonts_enabled(&app.store);

            // ... [existing code: build `workspaces`, commit activity state,
            //      build `inputs`, recompute `selectable`, set
            //      `app.dashboard.selection`, call `dashboard::render(f,
            //      dashboard_area, ...)` etc. — UNCHANGED.]

            // Existing PM render — fires only when pm_area is Some.
            if let Some(pm_area) = pm_area {
                if let Some(session) = app.pm.as_ref() {
                    crate::ui::pm_pane::resize_session(session, pm_area);
                }
                crate::ui::pm_pane::render(f, pm_area, app.pm.as_ref(), app.focus, &app.theme);
            }

            // New: detail bar render.
            if let (Some(detail_area), Some(SelectionTarget::Workspace(ws_id))) =
                (detail_area, app.selected_target())
            {
                if let Some((rid, ws)) = app.workspaces.iter().find(|(_, w)| w.id == ws_id) {
                    if let Some(repo) = app.repos.iter().find(|r| r.id == *rid) {
                        let session = app.sessions.get(ws.id);
                        let ago_secs = session.as_ref().and_then(|s| {
                            let last = s.activity_ms.load(std::sync::atomic::Ordering::Relaxed);
                            if last == 0 {
                                return None;
                            }
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_millis() as u64)
                                .unwrap_or(0);
                            Some(now.saturating_sub(last) / 1000)
                        });
                        let status = app.classify_status(ws);
                        let procs: Vec<crate::proc::ProcInfo> = app
                            .workspace_processes
                            .get(&ws.id)
                            .cloned()
                            .unwrap_or_default();
                        let inputs = crate::ui::dashboard::detail::DetailInputs {
                            repo,
                            workspace: ws,
                            events: app.workspace_events.get(&ws.id),
                            procs: &procs,
                            diff: app.workspace_diff.get(&ws.id).copied(),
                            lifecycle: app.pr_lifecycle.get(&ws.id).copied(),
                            pr_title: None,
                            pr_number: None,
                            status,
                            ago_secs,
                            reply_draft: &app.dashboard.reply_draft,
                            reply_focused: matches!(app.focus, crate::ui::PaneFocus::DetailBarReply),
                            events_scanned: app.workspace_events_scanned.contains(&ws.id),
                        };
                        crate::ui::dashboard::detail::render(f, detail_area, &inputs, &app.theme);
                    }
                }
            }
        }
```

Add the helper at module scope near the existing `nerd_fonts_enabled` / `pm_enabled` helpers in `app.rs`:

```rust
/// Carve the dashboard area into list / detail / pm regions based on
/// whether PM is visible and whether a workspace is selected.
fn dashboard_regions(
    area: ratatui::layout::Rect,
    pm_visible: bool,
    detail_visible: bool,
) -> (
    ratatui::layout::Rect,
    Option<ratatui::layout::Rect>,
    Option<ratatui::layout::Rect>,
) {
    use ratatui::layout::{Constraint, Direction, Layout};
    let detail_h = crate::ui::dashboard::detail::preferred_height(area.height);
    match (pm_visible, detail_visible) {
        (false, false) => (area, None, None),
        (false, true) => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(detail_h)])
                .split(area);
            (chunks[0], Some(chunks[1]), None)
        }
        (true, false) => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(area);
            (chunks[0], None, Some(chunks[1]))
        }
        (true, true) => {
            let pm_h = ((u32::from(area.height) * 33 / 100) as u16).max(6);
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(0),
                    Constraint::Length(detail_h),
                    Constraint::Length(pm_h),
                ])
                .split(area);
            (chunks[0], Some(chunks[1]), Some(chunks[2]))
        }
    }
}
```

In the body above where the placeholder comment `// ... [existing code: build workspaces ...]` sits, leave the existing code intact — only the leading `let (dashboard_area, pm_area) = if app.pm_visible ...` is replaced (with the new 3-tuple assignment), and the trailing detail-render block is new.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib pm_state_tests::detail_bar_renders_when_workspace_is_selected pm_state_tests::detail_bar_absent_when_repo_header_is_selected`
Expected: PASS.

- [ ] **Step 5: Run the full library test suite to catch regressions**

Run: `cargo test --lib`
Expected: PASS (all existing tests still pass).

- [ ] **Step 6: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): integrate detail bar into dashboard draw with 3-region layout"
```

---

## Task 15: Tab cycle and input dispatch in `handle_key_dashboard`

**Files:**
- Modify: `src/app.rs` (around line 1436 — `handle_key_dashboard`)
- Test: `src/app.rs` tests module (or a new focused tests file)

- [ ] **Step 1: Write the failing tests**

Add a new `#[cfg(test)] mod detail_bar_focus_tests` block at the bottom of `src/app.rs` (peer of the existing `pm_state_tests`):

```rust
#[cfg(test)]
mod detail_bar_focus_tests {
    use super::*;
    use crate::store::{NewWorkspace, Store, WorkspaceState};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::PathBuf;

    fn make_app_with_workspace_selected() -> App {
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "alpha",
                branch: "repo/alpha",
                worktree_path: std::path::Path::new("/tmp/wsx-test/alpha"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        store
            .set_workspace_state(id, WorkspaceState::Ready)
            .unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let idx = app
            .selectable
            .iter()
            .position(|t| matches!(t, SelectionTarget::Workspace(_)))
            .unwrap();
        app.dashboard.selected = idx;
        app
    }

    #[tokio::test]
    async fn tab_on_workspace_moves_focus_to_detail_bar_reply() {
        let mut app = make_app_with_workspace_selected();
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .await
            .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::DetailBarReply));
    }

    #[tokio::test]
    async fn tab_in_detail_bar_returns_focus_to_dashboard() {
        let mut app = make_app_with_workspace_selected();
        app.focus = crate::ui::PaneFocus::DetailBarReply;
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .await
            .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
    }

    #[tokio::test]
    async fn esc_in_detail_bar_clears_draft_and_returns_to_dashboard() {
        let mut app = make_app_with_workspace_selected();
        app.focus = crate::ui::PaneFocus::DetailBarReply;
        app.dashboard.reply_draft = "half-typed message".to_string();
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .await
            .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
        assert_eq!(app.dashboard.reply_draft, "");
    }

    #[tokio::test]
    async fn char_in_detail_bar_appends_to_draft() {
        let mut app = make_app_with_workspace_selected();
        app.focus = crate::ui::PaneFocus::DetailBarReply;
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE))
            .await
            .unwrap();
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE))
            .await
            .unwrap();
        assert_eq!(app.dashboard.reply_draft, "hi");
        // Focus must NOT have changed (this is a regression guard
        // against accidentally letting dashboard hotkeys fire).
        assert!(matches!(app.focus, crate::ui::PaneFocus::DetailBarReply));
    }

    #[tokio::test]
    async fn backspace_in_detail_bar_pops_last_char() {
        let mut app = make_app_with_workspace_selected();
        app.focus = crate::ui::PaneFocus::DetailBarReply;
        app.dashboard.reply_draft = "abc".to_string();
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE))
            .await
            .unwrap();
        assert_eq!(app.dashboard.reply_draft, "ab");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib detail_bar_focus_tests`
Expected: FAIL (Tab on workspace either does nothing or toggles PM focus — but does not enter `DetailBarReply`).

- [ ] **Step 3: Update `handle_key_dashboard` to handle the new focus**

In `src/app.rs::handle_key_dashboard`, add a new branch at the very top of the function (after the existing `if app.pm_visible && matches!(app.focus, crate::ui::PaneFocus::ProjectManager)` block ends):

```rust
    // DetailBarReply focus: keystrokes go to the reply input.
    if matches!(app.focus, crate::ui::PaneFocus::DetailBarReply) {
        // If the selected target is no longer a workspace (e.g.
        // refresh moved selection), auto-return focus and discard.
        if !matches!(app.selected_target(), Some(SelectionTarget::Workspace(_))) {
            app.focus = crate::ui::PaneFocus::Dashboard;
            app.dashboard.reply_draft.clear();
            return Ok(());
        }
        match (k.code, k.modifiers) {
            (KeyCode::Tab, _) => {
                app.focus = crate::ui::PaneFocus::Dashboard;
                return Ok(());
            }
            (KeyCode::Esc, _) => {
                app.focus = crate::ui::PaneFocus::Dashboard;
                app.dashboard.reply_draft.clear();
                return Ok(());
            }
            (KeyCode::Enter, _) => {
                let draft = std::mem::take(&mut app.dashboard.reply_draft);
                if let Some(SelectionTarget::Workspace(ws_id)) = app.selected_target() {
                    if let Some(session) = app.sessions.get(ws_id) {
                        let mut bytes = draft.into_bytes();
                        bytes.push(b'\r');
                        session.scroll_to_live();
                        let _ = session.writer.send(bytes).await;
                    }
                }
                app.focus = crate::ui::PaneFocus::Dashboard;
                return Ok(());
            }
            (KeyCode::Backspace, _) => {
                app.dashboard.reply_draft.pop();
                return Ok(());
            }
            (KeyCode::Char(c), m)
                if m == KeyModifiers::NONE || m == KeyModifiers::SHIFT =>
            {
                app.dashboard.reply_draft.push(c);
                return Ok(());
            }
            _ => return Ok(()), // swallow everything else
        }
    }
```

Then, further down in the same function, find the Tab handler (search for `KeyCode::Tab` — there may be an existing arm that handles Tab when PM is visible). Add a new branch that fires Tab when a workspace is selected AND PM is hidden. The cleanest place is to extend the existing Tab arm. For example, if the existing arm reads:

```rust
            (KeyCode::Tab, _) => {
                if app.pm_visible {
                    app.focus = crate::ui::PaneFocus::ProjectManager;
                }
                return Ok(());
            }
```

replace it with:

```rust
            (KeyCode::Tab, _) => {
                if matches!(app.selected_target(), Some(SelectionTarget::Workspace(_))) {
                    app.focus = crate::ui::PaneFocus::DetailBarReply;
                } else if app.pm_visible {
                    app.focus = crate::ui::PaneFocus::ProjectManager;
                }
                return Ok(());
            }
```

If there is no Tab arm yet (the current dashboard binds Tab only via the PM-focused early return at the top of `handle_key_dashboard`), add the new arm explicitly inside the main `match` block.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib detail_bar_focus_tests`
Expected: PASS (all 5 tests).

- [ ] **Step 5: Run full library tests**

Run: `cargo test --lib`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): handle Tab/Enter/Esc/Char for detail bar reply focus"
```

---

## Task 16: Selection change while focused returns to Dashboard and clears draft

**Files:**
- Modify: `src/app.rs` (`handle_key_dashboard` arrow-key arms — search for `KeyCode::Up` / `KeyCode::Down`)
- Test: `src/app.rs` `detail_bar_focus_tests` module

- [ ] **Step 1: Write the failing test**

Add to the existing `detail_bar_focus_tests` module in `src/app.rs`:

```rust
    #[tokio::test]
    async fn arrow_down_while_focused_returns_to_dashboard_and_clears_draft() {
        let mut app = make_app_with_workspace_selected();
        app.focus = crate::ui::PaneFocus::DetailBarReply;
        app.dashboard.reply_draft = "draft".to_string();
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .await
            .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
        assert_eq!(app.dashboard.reply_draft, "");
    }
```

NOTE: At the end of Task 15, `Up` / `Down` while focused are swallowed (the `_ => return Ok(())` catch-all). This test will fail because the focus stays on `DetailBarReply` rather than yielding to the arrow nav.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib detail_bar_focus_tests::arrow_down_while_focused_returns_to_dashboard_and_clears_draft`
Expected: FAIL.

- [ ] **Step 3: Update the `DetailBarReply` focus branch to bail out on navigation keys**

In `src/app.rs::handle_key_dashboard`, in the `DetailBarReply` focus branch (added in Task 15), replace the existing `_ => return Ok(()), // swallow everything else` catch-all with:

```rust
            (KeyCode::Up, _)
            | (KeyCode::Down, _)
            | (KeyCode::Left, _)
            | (KeyCode::Right, _)
            | (KeyCode::PageUp, _)
            | (KeyCode::PageDown, _)
            | (KeyCode::Home, _)
            | (KeyCode::End, _) => {
                // Navigation keys: yield focus back so the dashboard
                // handles the move. Discard the draft per spec.
                app.focus = crate::ui::PaneFocus::Dashboard;
                app.dashboard.reply_draft.clear();
                // Fall through to the normal dashboard handler below.
            }
            _ => return Ok(()), // swallow everything else
```

Critical: this branch must NOT `return Ok(())` — it should fall through to the rest of `handle_key_dashboard` so the navigation key actually fires. To make that work, restructure the early-return so the arrow-key fallthrough doesn't return early. Concretely: change the outer `if matches!(app.focus, …DetailBarReply) { match … }` from an early-return guard to a guard that returns for "consume" keys but doesn't return for "yield to dashboard" keys.

Suggested restructure: wrap the existing match in an `if let Some(action) = …` style. The simplest realization:

```rust
    if matches!(app.focus, crate::ui::PaneFocus::DetailBarReply) {
        if !matches!(app.selected_target(), Some(SelectionTarget::Workspace(_))) {
            app.focus = crate::ui::PaneFocus::Dashboard;
            app.dashboard.reply_draft.clear();
            return Ok(());
        }
        let consumed = handle_detail_bar_reply_key(app, k).await;
        if consumed {
            return Ok(());
        }
        // Not consumed → fall through so the dashboard handler picks up
        // the key (e.g. arrow nav). `handle_detail_bar_reply_key` has
        // already cleared the draft and reset focus when bailing out.
    }
```

with a helper:

```rust
async fn handle_detail_bar_reply_key(
    app: &mut App,
    k: crossterm::event::KeyEvent,
) -> bool {
    use crossterm::event::{KeyCode, KeyModifiers};
    match (k.code, k.modifiers) {
        (KeyCode::Tab, _) => {
            app.focus = crate::ui::PaneFocus::Dashboard;
            true
        }
        (KeyCode::Esc, _) => {
            app.focus = crate::ui::PaneFocus::Dashboard;
            app.dashboard.reply_draft.clear();
            true
        }
        (KeyCode::Enter, _) => {
            let draft = std::mem::take(&mut app.dashboard.reply_draft);
            if let Some(SelectionTarget::Workspace(ws_id)) = app.selected_target() {
                if let Some(session) = app.sessions.get(ws_id) {
                    let mut bytes = draft.into_bytes();
                    bytes.push(b'\r');
                    session.scroll_to_live();
                    let _ = session.writer.send(bytes).await;
                }
            }
            app.focus = crate::ui::PaneFocus::Dashboard;
            true
        }
        (KeyCode::Backspace, _) => {
            app.dashboard.reply_draft.pop();
            true
        }
        (KeyCode::Char(c), m) if m == KeyModifiers::NONE || m == KeyModifiers::SHIFT => {
            app.dashboard.reply_draft.push(c);
            true
        }
        (KeyCode::Up, _)
        | (KeyCode::Down, _)
        | (KeyCode::Left, _)
        | (KeyCode::Right, _)
        | (KeyCode::PageUp, _)
        | (KeyCode::PageDown, _)
        | (KeyCode::Home, _)
        | (KeyCode::End, _) => {
            // Yield to dashboard: it will handle the navigation. Discard draft.
            app.focus = crate::ui::PaneFocus::Dashboard;
            app.dashboard.reply_draft.clear();
            false
        }
        _ => true, // unknown key — swallow rather than fall through
    }
}
```

Move the `handle_detail_bar_reply_key` helper to module scope alongside `dispatch_key` / `handle_key_dashboard`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib detail_bar_focus_tests`
Expected: All 6 tests PASS.

- [ ] **Step 5: Run full library tests**

Run: `cargo test --lib`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): yield detail-bar focus to dashboard on arrow nav, clear draft"
```

---

## Task 17: Add manual test walkthrough doc

**Files:**
- Create: `docs/manual-tests/dashboard-detail-bar.md`

- [ ] **Step 1: List existing manual-tests to match style**

Run: `ls docs/manual-tests/`
Read one of the existing files to mirror its tone and shape, e.g.: `cat docs/manual-tests/workspace-layout-persistence.md` (if present) or another.

- [ ] **Step 2: Write the doc**

Create `docs/manual-tests/dashboard-detail-bar.md`:

```markdown
# Dashboard workspace detail bar — manual test

Verifies the detail bar appears for workspace selections, collapses
for repo selections, stacks with the Project Manager pane, and
accepts an inline reply that lands in the selected workspace's PTY.

## Setup

Launch wsx with at least one repo registered and at least one running
claude session in a workspace:

```
wsx
```

## Scenarios

1. **Bar shows on workspace selection.** Move selection (↑/↓) onto a
   workspace row. Expected: the bottom ~22% of the terminal becomes
   the detail bar (header strip with name/branch/lifecycle/diff/procs/
   status; three columns SESSION SUMMARY / RECENT CHAT / PROCESSES;
   reply chip at the bottom). The workspace list above keeps the
   selection visible.

2. **Bar hides on repo selection.** Move selection onto a repo header
   row. Expected: the bar disappears; the list reclaims the freed
   space. The keybind footer stays at the bottom.

3. **Reply input via Tab.** With a workspace selected, press Tab.
   Expected: the cursor appears in the `┃ Reply to agent ┃` input
   field. Type `ping`. Press Enter. Expected: the field clears, focus
   returns to the dashboard list, and (when you attach into the
   workspace via Enter) the `ping` message appears as a user prompt
   in the session.

4. **Esc cancels the draft.** Tab into the input, type a few
   characters, press Esc. Expected: the field clears and focus
   returns to the dashboard list without sending anything.

5. **Arrow nav yields focus.** Tab into the input, type a few
   characters, press ↓. Expected: the draft is discarded, the
   selection moves to the next item, and the bar updates (or hides,
   if the move landed on a repo header).

6. **PM coexistence.** With a workspace selected, toggle the Project
   Manager pane (existing keybind). Expected: the screen stacks
   list → detail bar → PM → footer, with all three regions visible.
   Toggle PM off. Expected: bar moves back to the bottom (above
   footer); list reclaims the PM area.

7. **Narrow terminal.** Resize the terminal width below 80 columns
   with a workspace selected. Expected: the body collapses to a
   single column (SESSION SUMMARY only). Header strip and reply row
   remain.

8. **Short terminal.** Resize the terminal height below 18 rows with
   a workspace selected. Expected: the bar is suppressed; only the
   list and footer render.
```

- [ ] **Step 3: Commit**

```bash
git add docs/manual-tests/dashboard-detail-bar.md
git commit -m "docs: manual test walkthrough for detail bar"
```

---

## Task 18: Final integration sweep — full `cargo test` + `cargo clippy`

**Files:**
- None (verification only).

- [ ] **Step 1: Run the full test suite**

Run: `cargo test`
Expected: PASS (workspace-wide).

- [ ] **Step 2: Run clippy with workspace's default config**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: PASS. Any warnings introduced by the changes — fix them inline (most likely candidates: unused imports in `detail.rs`, unused fields on `DetailInputs`).

- [ ] **Step 3: Run `cargo build --release`**

Run: `cargo build --release`
Expected: PASS. Release-mode catches lint differences sometimes missed by debug.

- [ ] **Step 4: Manually launch wsx and walk through the manual test**

Follow each scenario in `docs/manual-tests/dashboard-detail-bar.md`. Note any UX surprises in the PR description.

- [ ] **Step 5: If any earlier task left a debug `println!` / `dbg!` / commented-out code, clean it up and commit**

```bash
git status
# If clean, no action. If dirty:
git add -p   # review hunk-by-hunk
git commit -m "chore: cleanup after detail bar integration"
```

---

## Self-review against the spec

| Spec section | Plan task(s) |
|---|---|
| Goals: detail panel pinned bottom, ~22% height, clamped | Task 5 (`preferred_height`), Task 14 (region carving) |
| Goals: PM coexistence (list / detail / pm) | Task 14 (`dashboard_regions`) |
| Goals: JSONL-derived summary, no LLM calls | Tasks 1–3 (events extensions) |
| Goals: inline reply via Tab | Tasks 4, 11, 15, 16 |
| Goals: workspace list independently scrollable | Existing ratatui List behavior — no new code; verified by Task 14's selection test |
| Non-goals: PR title/number | Not in plan — wired as `None` in Task 14 |
| Non-goals: mid-string editing | Task 16 (swallow / yield non-supported keys) |
| Data model: `ToolUseCounts`, three new fields, reset semantics | Task 1 (struct + fields + reset); Task 2 (population) |
| Detail bar contents: header strip | Task 7 |
| Detail bar contents: SESSION SUMMARY | Task 8 |
| Detail bar contents: RECENT CHAT | Task 9 |
| Detail bar contents: PROCESSES + RECENT FILES | Task 10 |
| Detail bar contents: reply input row + cursor | Tasks 11, 12 (cursor placement) |
| Layout integration table (4 cases) | Task 14 (`dashboard_regions` helper) |
| Responsive: narrow width → single column | Task 13 |
| Responsive: short height → suppress bar | Task 14 (`detail_visible = height >= MIN_HEIGHT + 10`) |
| Focus model: PaneFocus::DetailBarReply | Task 4 |
| Focus model: Tab cycle / key dispatch | Tasks 15, 16 |
| Edge case: events not yet scanned | Task 12 (`events_scanned ? … : None`) |
| Edge case: no PR / no diff / empty procs / empty files | Tasks 7, 10 (conditional rendering) |
| Edge case: long draft scrolls | Task 11 (visible-window math) |
| Testing: events tests | Tasks 1, 2 |
| Testing: detail.rs tests | Tasks 5–11, 12, 13 |
| Testing: dashboard tests | Task 14 |
| Testing: app.rs focus tests | Tasks 15, 16 |
| Testing: manual verification | Task 17 |
| Rollout: single PR, no feature flag | All commits target the feature branch directly |
