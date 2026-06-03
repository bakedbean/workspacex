# Clickable Attention Entries Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make each workspace listed in the agent-chat status row clickable, so a left click fully attaches to that workspace (same as `Enter` on the dashboard).

**Architecture:** Reuse the existing "clickable chip" contract — capture per-element rects during the render pass, store them on `App` (cleared each frame), and hit-test the click coordinate in `handle_mouse`. The attention formatter is extended to emit per-entry column geometry that render converts into absolute screen rects.

**Tech Stack:** Rust, ratatui (TUI), crossterm (mouse events).

---

## Background for the implementer

This is the `wsx` terminal UI. The "agent-chat" views (`View::Attached` and
`View::AttachedPm`) render a vertical chrome stack below the agent's PTY pane:
a pinned-command chip row, an optional "other workspaces that need attention"
status row, and a footer.

The chip row is already clickable. The pattern is:
1. **Layout** — `layout_chip_row` (`src/ui/attached.rs`) computes one `Rect`
   per chip during render.
2. **Storage** — rects are stored in `app.chip_rects`, cleared at the top of
   every frame (`src/app/render.rs`, in `draw`).
3. **Hit-test** — `handle_mouse` (`src/app/input.rs`) finds which rect contains
   the click and dispatches.

We replicate this for the attention entries. The switch action already exists:
`attach_workspace(app, ws_id)` (`src/app.rs`, `pub(crate)`, synchronous).

The attention line is built by `format_attention_line_styled`
(`src/ui/updates_bar.rs`), which today returns `Option<Line<'static>>` and
discards where each entry sits. We extend it to also return the column geometry
of each rendered entry, keyed by `WorkspaceId`.

### Key geometry facts (already in `format_attention_line_styled`)

- Each entry renders as: `<glyph><space><repo>/<name><space>(<age>)`.
- The function already computes `widths: Vec<usize>` — one visual width per
  entry — as `1 + 1 + repo.chars() + 1 + name.chars() + 2 + age.chars() + 1`.
- Entries are joined by a 3-column separator `" │ "` (`sep_w = 3`).
- Only the first `included` entries are drawn; the rest collapse into a
  non-clickable `… +N more` tail.
- The line is rendered flush at `status_area.x` (no prefix is prepended), so an
  entry's column offset within the line maps directly to a screen column.

So for rendered entry `i`, its start column within the line is:
`sum(widths[j] + sep_w for j in 0..i)` (the first entry starts at column 0),
and its width is `widths[i]`.

## File Structure

- `src/ui/updates_bar.rs` — add `AttentionLine` + `AttentionSegment` types;
  change `format_attention_line_styled` to return `Option<AttentionLine>` and
  emit one segment per rendered entry. Add a geometry unit test.
- `src/app.rs` — add the `attention_rects: Vec<(WorkspaceId, Rect)>` field and
  initialize it.
- `src/app/render.rs` — clear `attention_rects` each frame; change
  `compute_attention_line` to return `Option<AttentionLine>`; in the `Attached`
  and `AttachedPm` branches convert segments to absolute rects and store them.
- `src/app/input.rs` — hit-test `attention_rects` in `handle_mouse` and call
  `attach_workspace`.

---

## Task 1: Emit per-entry geometry from the attention formatter

**Files:**
- Modify: `src/ui/updates_bar.rs` (types near `AttentionEntry` ~line 49; function `format_attention_line_styled` ~lines 96-159; tests ~lines 499-543)
- Test: `src/ui/updates_bar.rs` (same file's `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test**

Add this test inside the `mod tests` block (after `styled_line_returns_none_when_empty`, before the closing `}` at the end of the file):

```rust
    #[test]
    fn styled_line_emits_clickable_segment_per_entry() {
        let theme = Theme::wsx();
        let entries = vec![
            AttentionEntry {
                workspace_id: WorkspaceId(1),
                repo_name: "a".into(),
                name: "q".into(),
                age_anchor_ms: 9_000,
                activity: ActivityState::AwaitingAnswer,
            },
            AttentionEntry {
                workspace_id: WorkspaceId(2),
                repo_name: "bb".into(),
                name: "ss".into(),
                age_anchor_ms: 9_000,
                activity: ActivityState::Stalled,
            },
        ];
        // now_ms - age_anchor_ms = 1_000 -> age "1s" (2 chars) for both.
        let out = format_attention_line_styled(&entries, 10_000, 200, &theme).expect("line");
        assert_eq!(out.segments.len(), 2, "one segment per rendered entry");

        // Entry 0: "? a/q (1s)" -> 1+1 + 1 +1+ 1 + 2 + 2 + 1 = 10 cols, at col 0.
        assert_eq!(out.segments[0].workspace_id, WorkspaceId(1));
        assert_eq!(out.segments[0].start_col, 0);
        assert_eq!(out.segments[0].width, 10);

        // Entry 1 width: "! bb/ss (1s)" -> 1+1 + 2 +1+ 2 + 2 + 2 + 1 = 12 cols.
        // start_col = entry0 width (10) + separator (3) = 13.
        assert_eq!(out.segments[1].workspace_id, WorkspaceId(2));
        assert_eq!(out.segments[1].start_col, 13);
        assert_eq!(out.segments[1].width, 12);
    }

    #[test]
    fn styled_line_segments_exclude_overflow_more_tail() {
        let theme = Theme::wsx();
        let entries = vec![
            AttentionEntry {
                workspace_id: WorkspaceId(1),
                repo_name: "a".into(),
                name: "q".into(),
                age_anchor_ms: 9_000,
                activity: ActivityState::AwaitingAnswer,
            },
            AttentionEntry {
                workspace_id: WorkspaceId(2),
                repo_name: "bb".into(),
                name: "ss".into(),
                age_anchor_ms: 9_000,
                activity: ActivityState::Stalled,
            },
        ];
        // max_width 10 fits only entry 0 ("? a/q (1s)" is exactly 10).
        let out = format_attention_line_styled(&entries, 10_000, 10, &theme).expect("line");
        assert_eq!(out.segments.len(), 1, "only the included entry is clickable");
        assert_eq!(out.segments[0].workspace_id, WorkspaceId(1));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p wsx --lib updates_bar::tests::styled_line_emits 2>&1 | tail -20`
Expected: compile error — `out.segments` and the `AttentionLine`/`AttentionSegment` types don't exist yet (or `format_attention_line_styled` returns a `Line`, not a struct).

(If the crate name isn't `wsx`, run `cargo test --lib updates_bar 2>&1 | tail -20` instead; the workspace's package name is in `Cargo.toml`.)

- [ ] **Step 3: Add the new public types**

In `src/ui/updates_bar.rs`, immediately after the `AttentionEntry` struct
definition (the block ending around line 62), add:

```rust
/// The rendered attention line plus the clickable geometry of each entry.
/// Returned by [`format_attention_line_styled`] so the render pass can map
/// entries to screen rects for mouse hit-testing.
#[derive(Debug, Clone)]
pub struct AttentionLine {
    pub line: Line<'static>,
    /// One segment per *rendered* entry (the `included` ones, not the
    /// `… +N more` overflow). Columns are 0-based from the line's left edge.
    pub segments: Vec<AttentionSegment>,
}

/// The clickable extent of one attention entry: which workspace it points to
/// and where it sits within the line (column offset + width, in cells).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttentionSegment {
    pub workspace_id: WorkspaceId,
    pub start_col: u16,
    pub width: u16,
}
```

- [ ] **Step 4: Change `format_attention_line_styled` to build segments**

Change the function's return type from `Option<Line<'static>>` to
`Option<AttentionLine>`. The signature line becomes:

```rust
pub fn format_attention_line_styled(
    entries: &[AttentionEntry],
    now_ms: i64,
    max_width: usize,
    theme: &Theme,
) -> Option<AttentionLine> {
```

Inside the entry loop, track the running column offset and push a segment per
entry. Replace the existing `for (i, e) in entries.iter().take(included).enumerate()`
loop body so it accumulates geometry. The full loop + return become:

```rust
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut segments: Vec<AttentionSegment> = Vec::new();
    // Always render at least one entry; if the first doesn't fit we emit
    // it as-is and rely on ratatui's clipping.
    if included == 0 {
        included = 1;
    }
    let mut col: usize = 0;
    for (i, e) in entries.iter().take(included).enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ".to_string(), theme.dim_style()));
            col += sep_w;
        }
        let entry_start = col;
        let status = status_for_activity(e.activity);
        let glyph = status.glyph().to_string();
        spans.push(Span::styled(glyph, theme.status_style(status)));
        spans.push(Span::raw(" ".to_string()));
        spans.push(Span::styled(
            format!("{}/{}", e.repo_name, e.name),
            ratatui::style::Style::default().fg(theme.path),
        ));
        let age = format_age(now_ms.saturating_sub(e.age_anchor_ms));
        spans.push(Span::styled(format!(" ({age})"), theme.dim_style()));
        col += widths[i];
        segments.push(AttentionSegment {
            workspace_id: e.workspace_id,
            start_col: entry_start as u16,
            width: widths[i] as u16,
        });
    }
    let remaining = entries.len().saturating_sub(included);
    if remaining > 0 {
        spans.push(Span::styled(
            format!(" … +{remaining} more"),
            theme.dim_style(),
        ));
    }
    Some(AttentionLine {
        line: Line::from(spans),
        segments,
    })
```

Note: `widths` and `sep_w` are already computed earlier in the function and are
still in scope. `widths[i]` is the exact visual width of entry `i`, so reusing
it keeps `start_col`/`width` perfectly in sync with what is drawn. The
`included == 0` fallback can index `widths[0]` safely because `entries` is
non-empty here (the `entries.is_empty()` early return at the top guarantees it).

- [ ] **Step 5: Fix the two existing tests for the new return type**

In `styled_line_colors_each_entry_by_status` (~line 518), change:

```rust
        let line = format_attention_line_styled(&entries, 10_000, 200, &theme).expect("line");
```
to:
```rust
        let line = format_attention_line_styled(&entries, 10_000, 200, &theme)
            .expect("line")
            .line;
```

`styled_line_returns_none_when_empty` (~line 542) calls `.is_none()` on the
`Option`, which still type-checks unchanged — leave it as-is.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p wsx --lib updates_bar 2>&1 | tail -20`
Expected: PASS — all `updates_bar::tests` green, including the two new segment tests.

- [ ] **Step 7: Commit**

```bash
git add src/ui/updates_bar.rs
git commit -m "feat(updates_bar): emit per-entry click geometry from attention line"
```

---

## Task 2: Add and clear the `attention_rects` field on `App`

**Files:**
- Modify: `src/app.rs` (struct field near `chip_rects` ~line 209; struct init near `chip_rects: Vec::new()` ~line 303)
- Modify: `src/app/render.rs` (`draw`, clear site ~line 16)

- [ ] **Step 1: Add the struct field**

In `src/app.rs`, immediately after the `chip_rects` field (line 209), add:

```rust
    /// Rects of the rendered attention-row entries from the last draw tick,
    /// each paired with the workspace it points to. Consumed by `handle_mouse`
    /// to attach on click. Mirrors the `chip_rects` draw-populates /
    /// input-reads pattern; cleared each frame.
    pub attention_rects: Vec<(crate::data::store::WorkspaceId, ratatui::layout::Rect)>,
```

- [ ] **Step 2: Initialize the field**

In `src/app.rs`, in the `App` constructor struct literal, immediately after
`chip_rects: Vec::new(),` (line 303), add:

```rust
            attention_rects: Vec::new(),
```

- [ ] **Step 3: Clear it each frame**

In `src/app/render.rs`, in `draw`, immediately after `app.chip_rects.clear();`
(line 16), add:

```rust
    app.attention_rects.clear();
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build -p wsx 2>&1 | tail -20`
Expected: builds (a `compute_attention_line` type mismatch may NOT yet appear
because that change is Task 3; if the build is clean, good — `attention_rects`
is just unused for now, which is allowed for a `pub` field).

- [ ] **Step 5: Commit**

```bash
git add src/app.rs src/app/render.rs
git commit -m "feat(app): add attention_rects field cleared each frame"
```

---

## Task 3: Populate `attention_rects` in the render pass

**Files:**
- Modify: `src/app/render.rs` (`compute_attention_line` ~lines 692-730; `View::Attached` branch ~lines 373-455; `View::AttachedPm` branch ~lines 459-492)

- [ ] **Step 1: Change `compute_attention_line`'s return type**

In `src/app/render.rs`, change the signature (line 692-696) from
`-> Option<ratatui::text::Line<'static>>` to
`-> Option<crate::ui::updates_bar::AttentionLine>`. The final line of the
function body (line 729) already returns the value of
`format_attention_line_styled(...)`, which now yields `Option<AttentionLine>`,
so no body change is needed:

```rust
fn compute_attention_line(
    app: &App,
    attached_id: Option<crate::data::store::WorkspaceId>,
    max_width: usize,
) -> Option<crate::ui::updates_bar::AttentionLine> {
```

**Borrow-checker note (important).** This branch matches `&app.view`, so
`state` is a live immutable borrow of `app` right up through
`state.layout(pane_area)` (~line 399). You therefore CANNOT assign
`app.attention_rects = ...` next to `layout_chrome` — that would be a mutable
borrow while `state` is still borrowed. Instead, build the rects into a **local
variable** at the `layout_chrome` site, and assign it into `app.attention_rects`
later, next to the existing `app.chip_rects = out.chip_rects;` (~line 453),
which already runs after `state`'s last use. The existing code relies on exactly
this ordering, so follow the same pattern.

- [ ] **Step 2: Update the `View::Attached` branch to build rects**

In the `View::Attached` branch, the local is currently named `line`
(assigned ~line 373 from `compute_attention_line`). Rename it to `attention`.

Change the assignment (lines 373-380) so the variable is `attention`:

```rust
            let attention = if matches!(
                app.modal,
                Some(crate::ui::modal::Modal::UpdatesPanel { .. })
            ) {
                None
            } else {
                compute_attention_line(app, Some(focused_id), max_width)
            };
```

Change the `layout_chrome` call (line 397-398) to test `attention`:

```rust
            let (pane_area, chip_area, status_area, footer_area) =
                attached::layout_chrome(area, attention.is_some(), !pinned.is_empty());
```

Immediately AFTER that `layout_chrome` line (now that `status_area` is known),
build the rects and the line into **locals** (no `app` mutation here, so no
borrow conflict with `state`):

```rust
            let attention_rects: Vec<(
                crate::data::store::WorkspaceId,
                ratatui::layout::Rect,
            )> = attention
                .as_ref()
                .map(|a| {
                    a.segments
                        .iter()
                        .map(|s| {
                            (
                                s.workspace_id,
                                ratatui::layout::Rect {
                                    x: status_area.x.saturating_add(s.start_col),
                                    y: status_area.y,
                                    width: s.width,
                                    height: 1,
                                },
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            let attention_line = attention.map(|a| a.line);
```

Change the `render_panes` call (line 449) so the attention argument is the
extracted `attention_line` instead of `line` (it is the 9th positional
argument, the `attention_line: Option<Line<'static>>` parameter — replace the
bare `line,`):

```rust
                attention_line,
```

Then, where the branch already assigns `app.chip_rects = out.chip_rects;`
(~line 453), add directly below it:

```rust
            app.attention_rects = attention_rects;
```

- [ ] **Step 3: Update the `View::AttachedPm` branch to build rects**

Same borrow constraint applies: this branch holds a `session` borrow of
`app.pm` through the `render_panes` call, so assign `app.attention_rects` only
**after** `render_panes` returns, next to the existing
`app.attached_pane_rects = out.pane_rects;` (~line 492).

Rename the `line` local (assigned ~line 460-467) to `attention`:

```rust
                let attention = if matches!(
                    app.modal,
                    Some(crate::ui::modal::Modal::UpdatesPanel { .. })
                ) {
                    None
                } else {
                    compute_attention_line(app, None, max_width)
                };
```

Change the `layout_chrome` call (line 470-471):

```rust
                let (pane_area, chip_area, status_area, footer_area) =
                    attached::layout_chrome(area, attention.is_some(), false);
```

Immediately AFTER that `layout_chrome` line, build the **locals** (note the
deeper indentation in this branch):

```rust
                let attention_rects: Vec<(
                    crate::data::store::WorkspaceId,
                    ratatui::layout::Rect,
                )> = attention
                    .as_ref()
                    .map(|a| {
                        a.segments
                            .iter()
                            .map(|s| {
                                (
                                    s.workspace_id,
                                    ratatui::layout::Rect {
                                        x: status_area.x.saturating_add(s.start_col),
                                        y: status_area.y,
                                        width: s.width,
                                        height: 1,
                                    },
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let attention_line = attention.map(|a| a.line);
```

Change the `render_panes` call (line 488, the `line,` positional argument) to:

```rust
                    attention_line,
```

Then, where the branch already assigns `app.attached_pane_rects = out.pane_rects;`
(~line 492), add directly below it:

```rust
                app.attention_rects = attention_rects;
```

- [ ] **Step 4: Verify it compiles and existing tests pass**

Run: `cargo build -p wsx 2>&1 | tail -20`
Expected: clean build (no type-mismatch errors on `compute_attention_line` or
`render_panes`).

Run: `cargo test -p wsx --lib 2>&1 | tail -20`
Expected: PASS — all library tests green.

- [ ] **Step 5: Commit**

```bash
git add src/app/render.rs
git commit -m "feat(render): populate attention_rects from attention segments"
```

---

## Task 4: Attach on click in the mouse handler

**Files:**
- Modify: `src/app/input.rs` (`handle_mouse`, `Down(MouseButton::Left)` arm ~lines 1547-1556)

- [ ] **Step 1: Add the attention hit-test branch**

In `src/app/input.rs`, in `handle_mouse`, replace the existing
`MouseEventKind::Down(MouseButton::Left)` arm (lines 1547-1556):

```rust
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(idx) = app.chip_rects.iter().position(|r| {
                m.column >= r.x
                    && m.column < r.x.saturating_add(r.width)
                    && m.row >= r.y
                    && m.row < r.y.saturating_add(r.height)
            }) {
                fire_chip(app, idx).await;
            }
        }
```

with:

```rust
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(idx) = app.chip_rects.iter().position(|r| {
                m.column >= r.x
                    && m.column < r.x.saturating_add(r.width)
                    && m.row >= r.y
                    && m.row < r.y.saturating_add(r.height)
            }) {
                fire_chip(app, idx).await;
            } else if let Some((ws_id, _)) =
                app.attention_rects.iter().copied().find(|(_, r)| {
                    m.column >= r.x
                        && m.column < r.x.saturating_add(r.width)
                        && m.row >= r.y
                        && m.row < r.y.saturating_add(r.height)
                })
            {
                // Clicking an attention entry attaches to that workspace,
                // identical to `Enter` on the dashboard.
                let _ = crate::app::attach_workspace(app, ws_id);
            }
        }
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p wsx 2>&1 | tail -20`
Expected: clean build. `attach_workspace` is `pub(crate)` and synchronous, so
no `.await` and no import beyond the `crate::app::` path is needed.

- [ ] **Step 3: Run clippy and the full test suite**

Run: `cargo clippy -p wsx 2>&1 | tail -20`
Expected: no new warnings on the touched files.

Run: `cargo test -p wsx 2>&1 | tail -20`
Expected: PASS — all tests green.

- [ ] **Step 4: Commit**

```bash
git add src/app/input.rs
git commit -m "feat(input): attach to workspace on attention-entry click"
```

---

## Manual verification

After Task 4, verify end-to-end in a real terminal (the unit tests cover the
geometry but not the click wiring, which depends on live render rects):

- [ ] Run `wsx` with at least two workspaces, one of which needs attention
  (e.g. its agent finished a task or is awaiting an answer).
- [ ] Attach to a *different* workspace so the attention row shows the
  needs-attention one below the chips.
- [ ] Left-click the attention entry's text (`repo/name (age)`). Confirm the
  view switches to that workspace's agent chat (full attach, layout restored).
- [ ] Click the `" │ "` separator or empty space in the row — confirm nothing
  happens.
- [ ] With 3+ needs-attention workspaces and a narrow terminal so `… +N more`
  appears, confirm clicking the `… +N more` text does nothing, and the visible
  entries still attach correctly.

---

## Self-review notes

- **Spec coverage:** formatter geometry (Task 1), `attention_rects` field +
  clear (Task 2), render population for both `Attached` and `AttachedPm`
  (Task 3), click → `attach_workspace` (Task 4), tests + manual checks. All
  spec sections map to a task.
- **Type consistency:** `AttentionLine`/`AttentionSegment` defined in Task 1 are
  used unchanged in Tasks 2-4; `attention_rects: Vec<(WorkspaceId, Rect)>` is
  consistent across struct, init, clear, populate, and read sites.
- **No placeholders:** every code step shows the exact code; commands include
  expected output.
