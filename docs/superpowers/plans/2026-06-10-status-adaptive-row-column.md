# Status-adaptive Workspace Row Column Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the dashboard row's static "last agent message" flex column with status-adaptive content — the column shows the blocker when a workspace needs you, the live tool trace while it works, the cleaned recap when it finishes, and the original prompt when idle.

**Architecture:** A new pure module `src/ui/dashboard/column_content.rs` owns the state→content mapping (`row_column()`) plus the string formatters extracted from `session_summary.rs`, so the detail bar and the row share one source of truth. The column string is synthesized at the single production construction site (`src/app/render.rs`) where `WorkspaceEvents` and a `now_ms` time base are available; `row.rs::render` stays pure and only lays out and colors a precomputed `RowColumn { text, emphasis }`.

**Tech Stack:** Rust, ratatui (TUI), the existing `sessionx::activity::events::WorkspaceEvents` signal set, the `Theme` color helpers, and the canonical `Status` enum.

**Design reference:** `docs/superpowers/specs/2026-06-10-status-adaptive-row-column-design.md`

**Glyph note (deviation from spec):** The spec floated per-status emoji glyphs (`❓ ⚠ ✓ ⌖`). This plan **omits** them: those code points are double-width in many terminals and the flex column's truncation/padding math counts `chars()`, so a wide glyph would misalign the row. Color emphasis (Question/Stalled) plus the existing single-cell status glyph in column 3 already disambiguate each state. The existing `└ ` prefix is preserved unchanged, keeping the row width math intact.

---

### Task 1: Extract shared formatters into a `column_content` module

Pure refactor — move the string formatters the row will reuse out of `session_summary.rs` into a new shared module, with no behavior change. This keeps the detail bar and the row from drifting.

**Files:**
- Create: `src/ui/dashboard/column_content.rs`
- Modify: `src/ui/dashboard/mod.rs` (register module, near the other `pub mod` lines around line 14)
- Modify: `src/detail_modules/session_summary.rs` (delete moved fns, import them)

- [ ] **Step 1: Create the module with the moved formatters**

Create `src/ui/dashboard/column_content.rs`. Copy these three functions **verbatim** from `session_summary.rs` (lines ~138–247: `format_state_line`, `format_ago_short`, `format_tool_trace`, and `format_tool_trace`'s helper `plural`) and make them crate-visible:

```rust
//! Shared synthesizers for the workspace row's status-adaptive flex
//! column and the detail bar's SESSION SUMMARY. Pure string builders
//! over `WorkspaceEvents` + `Status`; no rendering, no wall-clock reads.

use crate::activity::events::{ToolUseCounts, WorkspaceEvents};
use crate::ui::dashboard::status::Status;

/// Canonical status label, optionally enriched with a why-detail drawn
/// from evt fields — pending question/permission tool for `Question`,
/// quiet duration for `Stalled`. Other states use the bare label.
pub(crate) fn format_state_line(status: Status, evt: &WorkspaceEvents, now_ms: i64) -> String {
    let base = status.label();
    let detail: Option<String> = match status {
        Status::Question => evt
            .pending_question_tool()
            .map(|n| n.to_string())
            .or_else(|| {
                evt.pending_permission_tool(now_ms, 3_000)
                    .map(|(name, _)| name)
            }),
        Status::Stalled => {
            if evt.last_log_activity_ms > 0 {
                let quiet_secs =
                    now_ms.saturating_sub(evt.last_log_activity_ms).max(0) as u64 / 1000;
                Some(format!("{} quiet", format_ago_short(Some(quiet_secs))))
            } else {
                None
            }
        }
        Status::Waiting | Status::Thinking | Status::Complete | Status::Idle => None,
    };
    match detail {
        Some(d) => format!("{base} · {d}"),
        None => base.to_string(),
    }
}

pub(crate) fn format_ago_short(secs: Option<u64>) -> String {
    match secs {
        None => "—".to_string(),
        Some(s) if s < 60 => format!("{s}s"),
        Some(s) if s < 3600 => format!("{}m", s / 60),
        Some(s) => format!("{}h", s / 3600),
    }
}

pub(crate) fn format_tool_trace(counts: &ToolUseCounts) -> String {
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
```

- [ ] **Step 2: Register the module in `mod.rs`**

In `src/ui/dashboard/mod.rs`, alongside the existing `pub mod status;` (~line 14), add:

```rust
pub mod column_content;
```

- [ ] **Step 3: Delete the moved fns from `session_summary.rs` and import them**

In `src/detail_modules/session_summary.rs`, delete the now-duplicated `format_state_line`, `format_ago_short`, `format_tool_trace`, and `plural` definitions. Keep `format_recent_files`, `truncate_to_chars`, and `wrap_lines` (still used only here). Add this import near the top of the file (below the existing `use` block):

```rust
use crate::ui::dashboard::column_content::{format_ago_short, format_state_line, format_tool_trace};
```

- [ ] **Step 4: Build and run the affected tests to confirm no behavior change**

Run: `cargo test -p wsx session_summary`
Expected: PASS — all existing SESSION SUMMARY tests still green (they exercise the moved formatters via `SessionSummary.lines`).

Run: `cargo build`
Expected: clean build, no unused-import warnings.

- [ ] **Step 5: Commit**

```bash
git add src/ui/dashboard/column_content.rs src/ui/dashboard/mod.rs src/detail_modules/session_summary.rs
git commit -m "refactor: extract row/detail-bar column formatters into column_content"
```

---

### Task 2: Add `RowColumn`, `ColumnEmphasis`, and `row_column()`

Add the public types and the state→content mapping with full TDD. Nothing consumes them yet (they're exercised only by their own tests this task), so the tree stays green.

**Files:**
- Modify: `src/ui/dashboard/column_content.rs`
- Test: `src/ui/dashboard/column_content.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing tests**

Append to `src/ui/dashboard/column_content.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::events::WorkspaceEvents;
    use std::collections::HashMap;

    fn evt() -> WorkspaceEvents {
        WorkspaceEvents::default()
    }

    #[test]
    fn none_events_yields_none() {
        assert!(row_column(Status::Idle, None, 0).is_none());
    }

    #[test]
    fn question_surfaces_pending_question_tool_with_status_emphasis() {
        let mut e = evt();
        e.pending_tool_uses
            .insert("tu_q".into(), ("AskUserQuestion".into(), 0));
        let c = row_column(Status::Question, Some(&e), 10_000).unwrap();
        assert_eq!(c.text, "AskUserQuestion");
        assert_eq!(c.emphasis, ColumnEmphasis::Status);
    }

    #[test]
    fn question_falls_back_to_permission_tool() {
        let mut pending = HashMap::new();
        // epoch-0 timestamp guarantees age > the 3s stale threshold.
        pending.insert("tu_b".to_string(), ("Bash".to_string(), 0_i64));
        let e = WorkspaceEvents {
            pending_tool_uses: pending,
            ..WorkspaceEvents::default()
        };
        let c = row_column(Status::Question, Some(&e), 10_000).unwrap();
        assert_eq!(c.text, "Bash");
    }

    #[test]
    fn question_with_no_pending_tool_uses_bare_label() {
        let c = row_column(Status::Question, Some(&evt()), 10_000).unwrap();
        assert_eq!(c.text, "question");
        assert_eq!(c.emphasis, ColumnEmphasis::Status);
    }

    #[test]
    fn stalled_shows_quiet_duration_with_warn_emphasis() {
        let e = WorkspaceEvents {
            last_log_activity_ms: 1,
            ..WorkspaceEvents::default()
        };
        // now_ms = 240_000 → 240s quiet → "4m quiet"
        let c = row_column(Status::Stalled, Some(&e), 240_000).unwrap();
        assert_eq!(c.text, "stalled · 4m quiet");
        assert_eq!(c.emphasis, ColumnEmphasis::Warn);
    }

    #[test]
    fn thinking_shows_tool_trace_dim() {
        let mut e = evt();
        e.tool_use_counts.bash = 2;
        e.tool_use_counts.edit = 3;
        let c = row_column(Status::Thinking, Some(&e), 0).unwrap();
        assert_eq!(c.text, "edited 3 files, ran 2 commands");
        assert_eq!(c.emphasis, ColumnEmphasis::Dim);
    }

    #[test]
    fn thinking_with_no_tools_yet_shows_ellipsis_label() {
        let c = row_column(Status::Thinking, Some(&evt()), 0).unwrap();
        assert_eq!(c.text, "thinking…");
    }

    #[test]
    fn waiting_with_no_tools_yet_shows_ellipsis_label() {
        let c = row_column(Status::Waiting, Some(&evt()), 0).unwrap();
        assert_eq!(c.text, "waiting…");
    }

    #[test]
    fn complete_prefers_turn_recap() {
        let e = WorkspaceEvents {
            last_completed_turn_text: Some("split the quick-start into two".into()),
            first_user_text: Some("do the thing".into()),
            ..WorkspaceEvents::default()
        };
        let c = row_column(Status::Complete, Some(&e), 0).unwrap();
        assert_eq!(c.text, "split the quick-start into two");
        assert_eq!(c.emphasis, ColumnEmphasis::Dim);
    }

    #[test]
    fn complete_falls_back_to_first_user_text() {
        let e = WorkspaceEvents {
            first_user_text: Some("migrate auth".into()),
            ..WorkspaceEvents::default()
        };
        let c = row_column(Status::Complete, Some(&e), 0).unwrap();
        assert_eq!(c.text, "migrate auth");
    }

    #[test]
    fn complete_with_nothing_is_none() {
        assert!(row_column(Status::Complete, Some(&evt()), 0).is_none());
    }

    #[test]
    fn idle_shows_first_user_text() {
        let e = WorkspaceEvents {
            first_user_text: Some("backfill the 003 migration".into()),
            ..WorkspaceEvents::default()
        };
        let c = row_column(Status::Idle, Some(&e), 0).unwrap();
        assert_eq!(c.text, "backfill the 003 migration");
        assert_eq!(c.emphasis, ColumnEmphasis::Dim);
    }

    #[test]
    fn idle_with_no_prompt_is_none() {
        assert!(row_column(Status::Idle, Some(&evt()), 0).is_none());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p wsx column_content`
Expected: FAIL — `cannot find function row_column` / `RowColumn` / `ColumnEmphasis`.

- [ ] **Step 3: Implement the types and `row_column`**

Add near the top of `src/ui/dashboard/column_content.rs` (below the `use` lines):

```rust
/// Precomputed flex-column content for one workspace row, chosen by the
/// caller from the workspace's status + events. `None` renders as the
/// em-dash placeholder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowColumn {
    pub text: String,
    pub emphasis: ColumnEmphasis,
}

/// How the row renderer should color the column body. The leading `└ `
/// prefix always takes the status color; this controls the body only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnEmphasis {
    /// Default — non-attention states render dim, as the message line does today.
    Dim,
    /// `Question` — paint the body in the row's status color.
    Status,
    /// `Stalled` — paint the body in the warn color.
    Warn,
}

/// Build the status-adaptive flex-column content for one workspace row.
/// `now_ms` is the shared epoch-ms time base (same one `app.rs` uses), so
/// stall durations match the detail bar. Returns `None` when there is no
/// meaningful content (the caller renders the em-dash).
pub fn row_column(status: Status, events: Option<&WorkspaceEvents>, now_ms: i64) -> Option<RowColumn> {
    let evt = events?;
    match status {
        Status::Question => {
            let body = evt
                .pending_question_tool()
                .map(|n| n.to_string())
                .or_else(|| evt.pending_permission_tool(now_ms, 3_000).map(|(n, _)| n))
                .unwrap_or_else(|| "question".to_string());
            Some(RowColumn { text: body, emphasis: ColumnEmphasis::Status })
        }
        Status::Stalled => Some(RowColumn {
            text: format_state_line(status, evt, now_ms),
            emphasis: ColumnEmphasis::Warn,
        }),
        Status::Thinking | Status::Waiting => {
            let trace = format_tool_trace(&evt.tool_use_counts);
            let text = if trace.is_empty() {
                format!("{}…", status.label())
            } else {
                trace
            };
            Some(RowColumn { text, emphasis: ColumnEmphasis::Dim })
        }
        Status::Complete => {
            let body = evt
                .last_completed_turn_text
                .as_deref()
                .or(evt.first_user_text.as_deref())
                .map(str::trim)
                .filter(|s| !s.is_empty())?;
            Some(RowColumn { text: body.to_string(), emphasis: ColumnEmphasis::Dim })
        }
        Status::Idle => {
            let body = evt
                .first_user_text
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())?;
            Some(RowColumn { text: body.to_string(), emphasis: ColumnEmphasis::Dim })
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p wsx column_content`
Expected: PASS — all 13 tests green.

- [ ] **Step 5: Commit**

```bash
git add src/ui/dashboard/column_content.rs
git commit -m "feat: add row_column status-adaptive column synthesizer"
```

---

### Task 3: Wire the column into `RowInputs`, the renderer, and the production call site

Swap the `last_message` field for `column`, render it with per-emphasis styling, and feed it from the one production construction site. Rust compiles all-or-nothing on a struct-field rename, so **every step in this task lands in a single commit** — the tree only builds green after the last step. Do all edits, then build once.

**Files:**
- Modify: `src/ui/dashboard/row.rs` (field def ~line 76; render block ~lines 226–251; test fixtures ~lines 334, 411)
- Modify: `src/app/render.rs` (construction site ~lines 75–125)
- Modify: `src/ui/dashboard/mod.rs` (`matches_filter` ~lines 330–340)
- Modify: `src/ui/dashboard/by_attention.rs` (`#[cfg(test)]` builders ~lines 262, 419)
- Modify: `src/ui/dashboard/by_repo.rs` (`#[cfg(test)]` builder ~line 142)

- [ ] **Step 1: Swap the `RowInputs` field and import the types in `row.rs`**

In `src/ui/dashboard/row.rs`, add to the `use` block at the top:

```rust
use crate::ui::dashboard::column_content::{ColumnEmphasis, RowColumn};
```

Replace the field (currently `pub last_message: Option<String>,` at ~line 76):

```rust
    pub column: Option<RowColumn>,
```

- [ ] **Step 2: Render the column with per-emphasis styling in `row.rs`**

Replace the message block (the `if let Some(msg) = inputs.last_message.as_deref() { … } else { … }` at ~lines 239–251) with:

```rust
    if let Some(col) = inputs.column.as_ref() {
        let prefix = "└ ";
        let body_width = message_width.saturating_sub(prefix.chars().count());
        let body = truncate(&col.text, body_width);
        spans.push(Span::styled(
            prefix.to_string(),
            theme.status_style(inputs.status),
        ));
        let body_padded = right_pad(&body, body_width);
        let body_style = match col.emphasis {
            ColumnEmphasis::Dim => theme.dim_style(),
            ColumnEmphasis::Status => theme.status_style(inputs.status),
            ColumnEmphasis::Warn => theme.warn_style(),
        };
        spans.push(Span::styled(body_padded, body_style));
    } else {
        let body = truncate_pad("—", message_width);
        spans.push(Span::styled(body, theme.dim_style()));
    }
```

(The `left_consumed` / `message_width` computation just above this block is unchanged.)

- [ ] **Step 3: Update the `row.rs` test fixtures**

In the `base()` test builder (~line 334), replace `last_message: Some("I have enough to give you a grounded tour.".into()),` with:

```rust
            column: Some(RowColumn {
                text: "I have enough to give you a grounded tour.".into(),
                emphasis: ColumnEmphasis::Dim,
            }),
```

At ~line 411 (inside the `missing_message_renders_em_dash` test), replace `inputs.last_message = None;` with:

```rust
        inputs.column = None;
```

Then add a new test in the same `#[cfg(test)] mod tests` block asserting each `ColumnEmphasis` maps to the expected body color (the spec's render-test requirement). The body span is the one immediately following the `└ ` prefix span; using the `*_style().fg` accessors avoids depending on `Theme` field visibility:

```rust
    #[test]
    fn column_emphasis_maps_to_body_style() {
        let theme = Theme::wsx();
        let body_after_prefix = |line: &Line<'_>| -> Style {
            let i = line
                .spans
                .iter()
                .position(|s| s.content.as_ref() == "└ ")
                .expect("prefix span present");
            line.spans[i + 1].style
        };

        // Warn emphasis → warn color.
        let mut inputs = base();
        inputs.status = Status::Stalled;
        inputs.column = Some(RowColumn {
            text: "stalled · 4m quiet".into(),
            emphasis: ColumnEmphasis::Warn,
        });
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        assert_eq!(body_after_prefix(&line).fg, theme.warn_style().fg);

        // Status emphasis → the row's status color.
        inputs.status = Status::Question;
        inputs.column = Some(RowColumn {
            text: "AskUserQuestion".into(),
            emphasis: ColumnEmphasis::Status,
        });
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        assert_eq!(
            body_after_prefix(&line).fg,
            theme.status_style(Status::Question).fg
        );

        // Dim emphasis → dim color.
        inputs.status = Status::Idle;
        inputs.column = Some(RowColumn {
            text: "backfill the migration".into(),
            emphasis: ColumnEmphasis::Dim,
        });
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        assert_eq!(body_after_prefix(&line).fg, theme.dim_style().fg);
    }
```

This test references `Style` — confirm `ratatui::style::Style` is imported in the test module (the file already imports it at module scope for `render`; add `use ratatui::style::Style;` to the test module if the compiler reports it missing).

- [ ] **Step 4: Feed the column from the production site in `app/render.rs`**

In `src/app/render.rs`, compute a shared `now_ms` once, immediately before the `let mut workspaces … = Vec::new();` line (~line 76):

```rust
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
```

Then in the `RowInputs { … }` literal (~line 113), replace `last_message: latest.map(|ev| ev.display),` with:

```rust
                        column: crate::ui::dashboard::column_content::row_column(
                            status,
                            app.workspace_events.get(&ws.id),
                            now_ms,
                        ),
```

The `latest` binding (~lines 97–100) becomes unused after this swap — delete it to avoid an unused-variable warning.

- [ ] **Step 5: Update `matches_filter` in `mod.rs`**

In `src/ui/dashboard/mod.rs` (~lines 335–339), replace the `last_message` arm:

```rust
        || w.row
            .column
            .as_ref()
            .map(|c| c.text.to_lowercase().contains(&needle))
            .unwrap_or(false)
```

- [ ] **Step 6: Update the `#[cfg(test)]` builders in `by_attention.rs` and `by_repo.rs`**

In `src/ui/dashboard/by_attention.rs`, ensure the test module imports the types (add to its `use super::*;` block if not already in scope):

```rust
    use crate::ui::dashboard::column_content::{ColumnEmphasis, RowColumn};
```

At ~line 262 (the `make_rows` builder), replace `last_message: w.last_message.clone(),` with:

```rust
                        column: w
                            .last_message
                            .clone()
                            .map(|t| RowColumn { text: t, emphasis: ColumnEmphasis::Dim }),
```

At ~line 419 (the standalone render test), replace `last_message: Some("hi".into()),` with:

```rust
                column: Some(RowColumn { text: "hi".into(), emphasis: ColumnEmphasis::Dim }),
```

In `src/ui/dashboard/by_repo.rs`, add the same import to its test module's `use` block, and at ~line 142 replace `last_message: w.last_message.clone(),` with:

```rust
                column: w
                    .last_message
                    .clone()
                    .map(|t| RowColumn { text: t, emphasis: ColumnEmphasis::Dim }),
```

(`FixtureWorkspace.last_message` in `fixture.rs` stays as-is — it is the test's source string, mapped into a `RowColumn` here.)

- [ ] **Step 7: Build and run the full dashboard test suite**

Run: `cargo build`
Expected: clean build, no errors, no unused-variable warnings.

Run: `cargo test -p wsx dashboard`
Expected: PASS — row render tests, by_attention, by_repo, and filter tests all green.

- [ ] **Step 8: Commit**

```bash
git add src/ui/dashboard/row.rs src/app/render.rs src/ui/dashboard/mod.rs src/ui/dashboard/by_attention.rs src/ui/dashboard/by_repo.rs
git commit -m "feat: render status-adaptive column in workspace rows"
```

---

### Task 4: Full verification

**Files:** none (verification only)

- [ ] **Step 1: Full build, lint, and test**

Run: `cargo build && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: all green, zero clippy warnings.

- [ ] **Step 2: Manual smoke test**

Run: `cargo run` (or the project's launch flow — see the `run` skill / `AGENTS.md`).
Verify against a workspace in each state where reachable:
- A workspace awaiting a question/permission shows the tool name in the status color.
- A stalled workspace shows `stalled · Nm quiet` in the warn color.
- A working workspace shows the tool trace (`edited N files, ran N commands`) dim.
- A just-finished workspace shows its cleaned recap dim.
- An idle workspace shows its original prompt dim.
- A workspace with no events yet shows the em-dash.
Confirm columns stay aligned (no width drift) and the filter (`/`) still matches against the visible column text.

- [ ] **Step 3: Commit any fixups**

```bash
git add -A
git commit -m "test: fixups from status-adaptive column verification"
```

(Skip if nothing changed.)
