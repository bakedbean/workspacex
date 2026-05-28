# Detail-Bar Container Scrolling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow each detail-bar container to scroll its content independently via the mouse wheel, with a scrollbar indicating scroll position.

**Architecture:** Refactor the `DetailModule` trait so modules return `Vec<Line<'static>>` instead of painting into a `Frame`. Containers then build a virtual line list (titles + module bodies + gaps), maintain a per-container scroll offset on `App`, and render a slice of that list to the visible area plus a scrollbar in a reserved rightmost column. Mouse-wheel routing on `View::Dashboard` does a point-in-rect test against container rects published each draw, and adjusts the matching container's offset.

**Tech Stack:** Rust, ratatui (Frame / Paragraph / Scrollbar / TestBackend), crossterm (MouseEventKind).

**Spec:** `docs/superpowers/specs/2026-05-28-detail-bar-container-scrolling-design.md`

---

## File Structure

| File | Role |
|---|---|
| `src/detail_modules/mod.rs` | Trait redefinition. Replace `height_hint` + `render` with `lines(ctx, width) -> Vec<Line<'static>>`. |
| `src/detail_modules/recent_files.rs` | Reimplement as `lines()`. Delete `height_hint_*` test; add `lines_*` test. |
| `src/detail_modules/recent_chat.rs` | Same shape. |
| `src/detail_modules/processes.rs` | Same shape. |
| `src/detail_modules/session_summary.rs` | Same shape. |
| `src/app.rs` | Add `detail_scroll_offsets: [u16; 4]`, `detail_scroll_last_workspace: Option<WorkspaceId>`, `detail_container_rects: [Option<Rect>; 4]` to `App`. |
| `src/ui/dashboard/detail.rs` | Rewrite `render_container` (virtual lines, offset clamp, slice, scrollbar). Add reset-on-workspace-change in `render`. Publish container rects via return value. New `#[cfg(test)]` tests. |
| `src/app/render.rs` | Update `detail::render` call site to thread offsets + receive container rects. |
| `src/app/input.rs` | Add `container_under_cursor`, `adjust_detail_scroll`. New `handle_mouse` branch. |
| `src/app/input_tests.rs` | New tests for mouse routing. |

No new files.

---

## Task 1: Add `lines()` method to `DetailModule` trait (additive, with default)

**Files:**
- Modify: `src/detail_modules/mod.rs`

Adding the new method with a `vec![]` default keeps the build green so we can migrate modules one at a time. The default is removed in Task 10 once every module overrides it.

- [ ] **Step 1: Add the method to the trait**

Edit the trait definition. Insert below `fn render`:

```rust
pub trait DetailModule: Send + Sync {
    fn id(&self) -> &'static str;
    fn title(&self) -> &'static str;
    fn height_hint(&self, ctx: &DetailContext<'_>) -> Constraint;
    fn render(&self, area: Rect, ctx: &DetailContext<'_>, frame: &mut Frame<'_>);

    /// Produce the module's content lines. The container will paint
    /// these via `Paragraph` and slice them by scroll offset. `width`
    /// is the column width the content will be drawn into (use for
    /// wrapping/truncation decisions). Default returns empty — will be
    /// overridden by every module, and the default is removed in the
    /// final cleanup task.
    fn lines(
        &self,
        _ctx: &DetailContext<'_>,
        _width: u16,
    ) -> Vec<ratatui::text::Line<'static>> {
        Vec::new()
    }
}
```

- [ ] **Step 2: Verify the workspace still builds**

Run: `cargo check`
Expected: clean build, no errors.

- [ ] **Step 3: Commit**

```bash
git add src/detail_modules/mod.rs
git commit -m "refactor(detail-modules): add lines() trait method with default"
```

---

## Task 2: Implement `lines()` on `RecentFiles`

**Files:**
- Modify: `src/detail_modules/recent_files.rs`

`build_recent_files` already exists in `src/ui/dashboard/detail.rs:483` and returns `Vec<Line<'static>>`. `RecentFiles::lines` becomes a one-line delegation. Existing `render` stays — Task 8 removes it.

- [ ] **Step 1: Write the failing test**

In the `#[cfg(test)] mod tests` block at the bottom of `recent_files.rs`, add:

```rust
#[test]
fn lines_empty_events_returns_one_dash_line() {
    let ctx = stub_context();
    let out = RecentFiles.lines(&ctx, 40);
    assert_eq!(out.len(), 1, "empty state should emit one '—' line");
}
```

- [ ] **Step 2: Run test, observe failure**

Run: `cargo test --lib --quiet detail_modules::recent_files::tests::lines_empty_events_returns_one_dash_line -- --nocapture`
Expected: FAIL — `assertion ... left: 0, right: 1` (default impl returns empty).

- [ ] **Step 3: Implement `lines()`**

Add to the `impl DetailModule for RecentFiles` block (between `height_hint` and `render`):

```rust
fn lines(
    &self,
    ctx: &DetailContext<'_>,
    width: u16,
) -> Vec<ratatui::text::Line<'static>> {
    crate::ui::dashboard::detail::build_recent_files(
        ctx.events,
        ctx.diff_per_file,
        &ctx.workspace.worktree_path,
        ctx.theme,
        width as usize,
    )
}
```

- [ ] **Step 4: Run test, observe pass**

Run: `cargo test --lib --quiet detail_modules::recent_files::tests::lines_empty_events_returns_one_dash_line`
Expected: PASS (1 test).

- [ ] **Step 5: Run the full module test file to confirm no regressions**

Run: `cargo test --lib --quiet detail_modules::recent_files`
Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/detail_modules/recent_files.rs
git commit -m "feat(detail-modules): implement RecentFiles::lines()"
```

---

## Task 3: Implement `lines()` on `Processes`

**Files:**
- Modify: `src/detail_modules/processes.rs`

Existing `render` builds a `lines` vec inline before calling `Paragraph::new(lines)`. Extract that into a `build_lines` helper called by both `render` and the new `lines()`.

- [ ] **Step 1: Read the current `render` body**

Open `src/detail_modules/processes.rs` and locate the `render` method body. Note exactly which Vec it builds (variable named `lines`) before `frame.render_widget(Paragraph::new(lines), area);`.

- [ ] **Step 2: Write the failing test**

Add to the `#[cfg(test)] mod tests` block:

```rust
#[test]
fn lines_returns_one_line_per_proc() {
    let procs = vec![proc(1, "a"), proc(2, "b"), proc(3, "c")];
    let mut ctx = stub_context();
    ctx.procs = Box::leak(procs.into_boxed_slice());
    let out = Processes.lines(&ctx, 40);
    assert_eq!(out.len(), 3);
}

#[test]
fn lines_zero_procs_returns_one_dash_line() {
    let ctx = stub_context();
    let out = Processes.lines(&ctx, 40);
    assert_eq!(out.len(), 1);
}
```

- [ ] **Step 3: Run tests, observe failure**

Run: `cargo test --lib --quiet detail_modules::processes::tests::lines_`
Expected: both FAIL (default returns empty).

- [ ] **Step 4: Extract a private `build_lines` helper**

Below the `impl DetailModule for Processes` block, add a free function:

```rust
fn build_lines(
    ctx: &DetailContext<'_>,
    width: u16,
) -> Vec<ratatui::text::Line<'static>> {
    // PASTE: the existing `lines` Vec construction from
    // Processes::render's body, but with `area.width` replaced by `width`
    // and `area.height` (if used) replaced by `u16::MAX` or removed —
    // the container now decides height.
    todo!("paste the inline body from render() here")
}
```

> NOTE for the implementer: the existing render body in `processes.rs` reads `ctx.procs` and builds a `Vec<Line<'static>>` of length `ctx.procs.len()` (or 1 line if empty). Move that exact construction into `build_lines` verbatim, swapping `area.width` for `width`. Do NOT change the line content.

- [ ] **Step 5: Update `render` and add `lines()`**

In the `impl DetailModule for Processes` block:

```rust
fn render(&self, area: Rect, ctx: &DetailContext<'_>, frame: &mut Frame<'_>) {
    use ratatui::widgets::Paragraph;
    let lines = build_lines(ctx, area.width);
    frame.render_widget(Paragraph::new(lines), area);
}

fn lines(
    &self,
    ctx: &DetailContext<'_>,
    width: u16,
) -> Vec<ratatui::text::Line<'static>> {
    build_lines(ctx, width)
}
```

- [ ] **Step 6: Run tests, observe pass**

Run: `cargo test --lib --quiet detail_modules::processes`
Expected: all tests pass (existing `height_hint_*` tests still pass — they're untouched).

- [ ] **Step 7: Commit**

```bash
git add src/detail_modules/processes.rs
git commit -m "feat(detail-modules): implement Processes::lines()"
```

---

## Task 4: Implement `lines()` on `RecentChat`

**Files:**
- Modify: `src/detail_modules/recent_chat.rs`

Same shape as Task 3. `render` builds an `out: Vec<Line>` inline before `Paragraph::new(out)`. Extract into a `build_lines` helper.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block:

```rust
#[test]
fn lines_with_no_events_returns_at_least_one_line() {
    let ctx = stub_context();
    let out = RecentChat.lines(&ctx, 40);
    assert!(!out.is_empty(), "RecentChat should emit at least one line in empty state");
}
```

- [ ] **Step 2: Run, observe failure**

Run: `cargo test --lib --quiet detail_modules::recent_chat::tests::lines_with_no_events_returns_at_least_one_line`
Expected: FAIL — assertion fails (default returns empty).

- [ ] **Step 3: Extract `build_lines` and add `lines()`**

Below the `impl DetailModule for RecentChat` block:

```rust
fn build_lines(
    ctx: &DetailContext<'_>,
    width: u16,
) -> Vec<ratatui::text::Line<'static>> {
    // PASTE: the existing line-building logic from RecentChat::render.
    // Replace `area.width` with `width`. The body currently has three
    // branches that each end in `frame.render_widget(Paragraph::new(out), area)`;
    // refactor so each branch instead populates `out` and the function
    // returns `out` at the end.
    todo!("paste and adapt")
}
```

In the impl block:

```rust
fn render(&self, area: Rect, ctx: &DetailContext<'_>, frame: &mut Frame<'_>) {
    use ratatui::widgets::Paragraph;
    let lines = build_lines(ctx, area.width);
    frame.render_widget(Paragraph::new(lines), area);
}

fn lines(
    &self,
    ctx: &DetailContext<'_>,
    width: u16,
) -> Vec<ratatui::text::Line<'static>> {
    build_lines(ctx, width)
}
```

> NOTE for the implementer: `RecentChat::render` currently has three branches that each call `frame.render_widget(Paragraph::new(out), area)` (see lines 38, 44, 54 in the current file). In `build_lines`, each branch must instead end with the populated `out` value falling through to a single `out` return. Restructure with `let mut out = Vec::new();` at top, branches push into `out`, then `out` returned at the end.

- [ ] **Step 4: Run tests, observe pass**

Run: `cargo test --lib --quiet detail_modules::recent_chat`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/detail_modules/recent_chat.rs
git commit -m "feat(detail-modules): implement RecentChat::lines()"
```

---

## Task 5: Implement `lines()` on `SessionSummary`

**Files:**
- Modify: `src/detail_modules/session_summary.rs`

`render` builds `out: Vec<Line>` inline. Same extract pattern as Task 4.

- [ ] **Step 1: Write the failing test**

Add to `#[cfg(test)] mod tests`:

```rust
#[test]
fn lines_loading_state_emits_loading_line() {
    let mut ctx = stub_context();
    ctx.events_scanned = false;
    let out = SessionSummary.lines(&ctx, 40);
    assert!(!out.is_empty());
    // First line in the loading branch is "  loading…" in dim style.
    let first_text: String = out[0].spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(first_text.contains("loading"), "expected 'loading' line, got: {first_text:?}");
}
```

- [ ] **Step 2: Run, observe failure**

Run: `cargo test --lib --quiet detail_modules::session_summary::tests::lines_loading_state_emits_loading_line`
Expected: FAIL — `out.is_empty()` panics or assertion fails.

- [ ] **Step 3: Extract `build_lines` and add `lines()`**

Below the impl block, add:

```rust
fn build_lines(
    ctx: &DetailContext<'_>,
    width: u16,
) -> Vec<ratatui::text::Line<'static>> {
    // PASTE: the body of SessionSummary::render from `let now_duration = ...`
    // through the final `out` value, but:
    //   - replace `area.width` with `width`
    //   - replace `let column_width = area.width as usize;` with
    //     `let column_width = width as usize;`
    //   - remove the trailing `frame.render_widget(Paragraph::new(out), area);`
    //     and instead return `out`
    todo!("paste and adapt")
}
```

Update the impl block:

```rust
fn render(&self, area: Rect, ctx: &DetailContext<'_>, frame: &mut Frame<'_>) {
    use ratatui::widgets::Paragraph;
    let lines = build_lines(ctx, area.width);
    frame.render_widget(Paragraph::new(lines), area);
}

fn lines(
    &self,
    ctx: &DetailContext<'_>,
    width: u16,
) -> Vec<ratatui::text::Line<'static>> {
    build_lines(ctx, width)
}
```

- [ ] **Step 4: Run tests, observe pass**

Run: `cargo test --lib --quiet detail_modules::session_summary`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/detail_modules/session_summary.rs
git commit -m "feat(detail-modules): implement SessionSummary::lines()"
```

---

## Task 6: Add scroll state to `App`

**Files:**
- Modify: `src/app.rs`

Three new fields. Initialized to defaults. No behavior change yet — Task 7 wires them in.

- [ ] **Step 1: Locate the `App` struct**

Open `src/app.rs`. Find the line `pub chip_rects: Vec<ratatui::layout::Rect>,` (around line 190). New fields go alongside it.

- [ ] **Step 2: Add fields to the struct**

Below the `chip_rects` field:

```rust
/// Per-slot scroll offset for detail-bar containers. Bumped by mouse
/// wheel via `handle_mouse`, clamped on every draw to
/// `content_height - visible_height` for the matching container.
pub detail_scroll_offsets: [u16; 4],

/// Sentinel for reset-on-workspace-switch. When the selected
/// workspace changes, `detail_scroll_offsets` zeroes out and this
/// updates. See `src/ui/dashboard/detail.rs::render`.
pub detail_scroll_last_workspace: Option<crate::store::WorkspaceId>,

/// Rect for each rendered detail-bar container slot, populated each
/// draw and consumed by `handle_mouse` for hit-testing wheel events.
/// Mirrors the `chip_rects` draw-populates-input-reads pattern.
pub detail_container_rects: [Option<ratatui::layout::Rect>; 4],
```

- [ ] **Step 3: Initialize in App constructor**

Find the constructor (around line 263, where `chip_rects: Vec::new(),` appears). Add:

```rust
detail_scroll_offsets: [0; 4],
detail_scroll_last_workspace: None,
detail_container_rects: [None; 4],
```

- [ ] **Step 4: Verify build**

Run: `cargo check`
Expected: clean build. New fields unused warnings are OK at this stage.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): add detail-bar scroll state fields"
```

---

## Task 7: Rewrite `render_container` with virtual lines, offset, and scrollbar

**Files:**
- Modify: `src/ui/dashboard/detail.rs`
- Modify: `src/app/render.rs` (call-site update)

The largest task. We change `render_container`'s signature to accept the slot index and a `&mut u16` offset, build a virtual line list via `module.lines()`, clamp the offset, paint a slice, and paint a scrollbar in the reserved rightmost column. We also change `render`'s signature to thread offsets + container rects through.

- [ ] **Step 1: Update `DetailInputs` to carry offsets**

In `src/ui/dashboard/detail.rs`, add a field to `DetailInputs`:

```rust
/// Per-slot scroll offsets. Borrowed mutably so the container can
/// clamp them to the current content height during render.
pub scroll_offsets: &'a mut [u16; 4],
```

This requires changing `DetailInputs<'a>` so it can carry a mutable borrow. The struct is consumed once per draw — that's fine.

- [ ] **Step 2: Change `render`'s return type to publish container rects**

Replace the current return type `Vec<ratatui::layout::Rect>` with a struct:

```rust
#[derive(Debug, Default)]
pub struct DetailDrawOutput {
    pub chip_rects: Vec<ratatui::layout::Rect>,
    pub container_rects: [Option<ratatui::layout::Rect>; 4],
}
```

Change `render`'s signature:

```rust
pub fn render(
    f: &mut Frame,
    area: Rect,
    inputs: &mut DetailInputs<'_>,
    theme: &Theme,
) -> DetailDrawOutput {
```

(Note: `inputs` is now `&mut` so the contained `scroll_offsets` borrow is usable.)

At the early returns where the bar can't render, return `DetailDrawOutput::default()` instead of `Vec::new()`.

- [ ] **Step 3: Update the `render_body_region` signature**

It now needs both the offsets and a place to record rects. Easiest: have it return `[Option<Rect>; 4]`:

```rust
fn render_body_region(
    f: &mut Frame,
    area: Rect,
    inputs: &mut DetailInputs<'_>,
    theme: &Theme,
) -> [Option<Rect>; 4] {
    // ... existing code up through the per-container loop ...
    let mut rects: [Option<Rect>; 4] = [None; 4];
    for (i, ids) in containers.iter().enumerate() {
        let col = h_chunks[i * 2];
        let content = Rect {
            x: col.x,
            y: col.y + 1,
            width: col.width,
            height: col.height.saturating_sub(2),
        };
        // Slot index in the App-state array is the original container
        // index in cfg.containers (before the narrow-collapse). For the
        // narrow path, that's the index of the first non-empty container;
        // for the normal path it's `i`. Compute it explicitly:
        let slot_idx = if area.width < 80 {
            cfg.containers.iter().position(|c| !c.is_empty()).unwrap_or(0)
        } else {
            i
        };
        let offset_slot = &mut inputs.scroll_offsets[slot_idx];
        render_container(f, content, ids, &ctx, inputs.registry, theme, offset_slot);
        if slot_idx < 4 {
            rects[slot_idx] = Some(content);
        }
    }
    // ... existing vertical-separator loop unchanged ...
    rects
}
```

- [ ] **Step 4: Rewrite `render_container`**

Replace the entire function body. The signature becomes:

```rust
fn render_container(
    f: &mut Frame,
    area: Rect,
    module_ids: &[String],
    ctx: &crate::detail_modules::DetailContext<'_>,
    reg: &crate::detail_modules::Registry,
    theme: &Theme,
    offset: &mut u16,
) {
    use ratatui::text::Line;
    use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

    if module_ids.is_empty() || area.height == 0 || area.width == 0 {
        return;
    }

    // Reserve the rightmost column for the scrollbar.
    let content_width = area.width.saturating_sub(1);
    let content_area = Rect {
        x: area.x,
        y: area.y,
        width: content_width,
        height: area.height,
    };
    let bar_area = Rect {
        x: area.x + content_width,
        y: area.y,
        width: 1,
        height: area.height,
    };

    let label_style = Style::default()
        .fg(theme.path)
        .add_modifier(Modifier::BOLD);

    // Build virtual line list: title row + body lines + 1-row gap between
    // modules. Last module has no trailing gap.
    let mut virtual_lines: Vec<Line<'static>> = Vec::new();
    let last_idx = module_ids.len().saturating_sub(1);
    for (i, id) in module_ids.iter().enumerate() {
        match reg.get(id) {
            Some(m) => {
                virtual_lines.push(Line::from(Span::styled(m.title(), label_style)));
                virtual_lines.extend(m.lines(ctx, content_width));
            }
            None => {
                tracing::warn!(id = %id, "detail_bar: unknown module id in container");
                virtual_lines.push(Line::from(Span::styled(
                    format!("[unknown: {id}]"),
                    theme.dim_style(),
                )));
            }
        }
        if i != last_idx {
            virtual_lines.push(Line::from(""));
        }
    }

    let content_height: u16 = virtual_lines.len().min(u16::MAX as usize) as u16;
    let max_offset = content_height.saturating_sub(area.height);
    if *offset > max_offset {
        *offset = max_offset;
    }

    let start = *offset as usize;
    let end = (start + area.height as usize).min(virtual_lines.len());
    let visible: Vec<Line<'static>> = virtual_lines[start..end].to_vec();
    f.render_widget(Paragraph::new(visible), content_area);

    if content_height > area.height {
        let mut state = ScrollbarState::new(content_height as usize)
            .position(*offset as usize)
            .viewport_content_length(area.height as usize);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None);
        f.render_stateful_widget(scrollbar, bar_area, &mut state);
    }
}
```

- [ ] **Step 5: Update the `render` body to use the new types**

In `render`, where it currently captures `body_region` and calls `render_body_region(f, body_region, inputs, theme);`, change to:

```rust
let container_rects = render_body_region(f, body_region, inputs, theme);
```

At the end of `render`, return:

```rust
DetailDrawOutput {
    chip_rects,
    container_rects,
}
```

- [ ] **Step 6: Update the call site in `src/app/render.rs`**

Around line 301:

```rust
let mut inputs = crate::ui::dashboard::detail::DetailInputs {
    // ... existing fields ...
    scroll_offsets: &mut app.detail_scroll_offsets,
};
let out = crate::ui::dashboard::detail::render(
    f,
    detail_area,
    &mut inputs,
    &app.theme,
);
app.detail_container_rects = out.container_rects;
if !out.chip_rects.is_empty() {
    app.chip_rects = out.chip_rects;
    app.pinned_commands_cache = pinned;
}
```

(The `let mut inputs` is necessary so it can be passed as `&mut`. The existing code uses `let inputs = ...; let rects = ...(&inputs, ...)` — we change both.)

- [ ] **Step 7: Verify build**

Run: `cargo check`
Expected: clean build.

- [ ] **Step 8: Run existing test suite**

Run: `cargo test --lib --quiet`
Expected: existing tests pass. Some `detail.rs` tests may need their callers updated to pass `scroll_offsets: &mut [0; 4]` to `DetailInputs` — fix as you encounter them.

- [ ] **Step 9: Add container-level tests**

In `src/ui/dashboard/detail.rs`'s `#[cfg(test)] mod tests` block, add:

```rust
#[test]
fn render_container_short_content_no_scrollbar() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use crate::detail_modules::DetailContext;

    let backend = TestBackend::new(40, 10);
    let mut terminal = Terminal::new(backend).unwrap();
    let reg = make_registry();
    let ids = vec!["processes".to_string()];
    let mut offset: u16 = 0;
    let theme = Theme::default();
    let (workspace, repo, events) = test_fixtures();
    let ctx = DetailContext {
        repo: &repo,
        workspace: &workspace,
        events: Some(&events),
        procs: &[],
        diff: None,
        diff_per_file: None,
        lifecycle: None,
        pr_title: None,
        pr_number: None,
        status: crate::ui::dashboard::status::Status::Idle,
        ago_secs: None,
        events_scanned: true,
        theme: &theme,
    };

    terminal
        .draw(|f| {
            let area = Rect { x: 0, y: 0, width: 40, height: 10 };
            render_container(f, area, &ids, &ctx, &reg, &theme, &mut offset);
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    // Rightmost column (x=39) should be blank — no scrollbar painted.
    for y in 0..10 {
        let sym = buf[(39, y)].symbol();
        assert_eq!(sym, " ", "expected blank scrollbar column at row {y}, got {sym:?}");
    }
}

#[test]
fn render_container_tall_content_renders_scrollbar() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use crate::detail_modules::DetailContext;

    let backend = TestBackend::new(40, 4); // very short — forces overflow
    let mut terminal = Terminal::new(backend).unwrap();
    let reg = make_registry();
    let ids = vec!["session_summary".to_string()];
    let mut offset: u16 = 0;
    let theme = Theme::default();
    let (workspace, repo, events) = test_fixtures();
    let ctx = DetailContext {
        repo: &repo,
        workspace: &workspace,
        events: Some(&events),
        procs: &[],
        diff: None,
        diff_per_file: None,
        lifecycle: None,
        pr_title: None,
        pr_number: None,
        status: crate::ui::dashboard::status::Status::Idle,
        ago_secs: None,
        events_scanned: true,
        theme: &theme,
    };

    terminal
        .draw(|f| {
            let area = Rect { x: 0, y: 0, width: 40, height: 4 };
            render_container(f, area, &ids, &ctx, &reg, &theme, &mut offset);
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    // Rightmost column should have at least one non-blank cell (scrollbar
    // glyphs from ratatui's Scrollbar widget).
    let any_nonblank = (0..4).any(|y| buf[(39, y)].symbol() != " ");
    assert!(any_nonblank, "expected scrollbar glyphs in rightmost column");
}

#[test]
fn render_container_clamps_offset_when_content_shrinks() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use crate::detail_modules::DetailContext;

    let backend = TestBackend::new(40, 10);
    let mut terminal = Terminal::new(backend).unwrap();
    let reg = make_registry();
    let ids = vec!["processes".to_string()]; // 1 line when no procs
    let mut offset: u16 = 50; // wildly past end
    let theme = Theme::default();
    let (workspace, repo, events) = test_fixtures();
    let ctx = DetailContext {
        repo: &repo,
        workspace: &workspace,
        events: Some(&events),
        procs: &[],
        diff: None,
        diff_per_file: None,
        lifecycle: None,
        pr_title: None,
        pr_number: None,
        status: crate::ui::dashboard::status::Status::Idle,
        ago_secs: None,
        events_scanned: true,
        theme: &theme,
    };

    terminal
        .draw(|f| {
            let area = Rect { x: 0, y: 0, width: 40, height: 10 };
            render_container(f, area, &ids, &ctx, &reg, &theme, &mut offset);
        })
        .unwrap();

    // Content (title + 1 body line = 2 rows) < area.height (10), so
    // max_offset = 0 and offset should be clamped to 0.
    assert_eq!(offset, 0);
}
```

> NOTE for the implementer: `test_fixtures()` is a helper you'll need to add (or reuse from elsewhere in `detail.rs` tests). Look for existing test setup in `detail.rs` for the `Workspace`, `Repo`, `WorkspaceEvents` shape. If no helper exists, build one locally — three test functions sharing the same fixture justifies it.

- [ ] **Step 10: Run new tests**

Run: `cargo test --lib --quiet ui::dashboard::detail::tests::render_container_`
Expected: 3 tests pass.

- [ ] **Step 11: Commit**

```bash
git add src/ui/dashboard/detail.rs src/app/render.rs
git commit -m "feat(detail-bar): scroll containers via virtual line list and offset"
```

---

## Task 8: Reset offsets when the selected workspace changes

**Files:**
- Modify: `src/ui/dashboard/detail.rs`
- Modify: `src/app/render.rs`

Threading state is mostly done. We add a reset step before any container renders.

- [ ] **Step 1: Add the reset call to the `render` call site**

In `src/app/render.rs`, just before constructing `inputs` (around line 290), add:

```rust
// Reset detail-bar scroll offsets when the selected workspace changes.
let current_ws_id = Some(ws.id);
if app.detail_scroll_last_workspace != current_ws_id {
    app.detail_scroll_offsets = [0; 4];
    app.detail_scroll_last_workspace = current_ws_id;
}
```

This runs inside the `Some(SelectionTarget::Workspace(ws_id))` arm, so `ws.id` is always defined.

- [ ] **Step 2: Write a failing test for reset behavior**

In `src/ui/dashboard/detail.rs`'s test module:

```rust
#[test]
fn workspace_change_resets_offsets() {
    // The reset logic lives at the call site in app/render.rs, so this
    // test exercises App directly rather than the render function.
    use crate::store::WorkspaceId;
    let mut offsets = [3u16, 4, 5, 6];
    let mut last: Option<WorkspaceId> = Some(WorkspaceId(100));
    let new_ws: Option<WorkspaceId> = Some(WorkspaceId(200));

    if last != new_ws {
        offsets = [0; 4];
        last = new_ws;
    }

    assert_eq!(offsets, [0; 4]);
    assert_eq!(last, Some(WorkspaceId(200)));
}

#[test]
fn same_workspace_preserves_offsets() {
    use crate::store::WorkspaceId;
    let mut offsets = [3u16, 4, 5, 6];
    let mut last: Option<WorkspaceId> = Some(WorkspaceId(100));
    let new_ws: Option<WorkspaceId> = Some(WorkspaceId(100));

    if last != new_ws {
        offsets = [0; 4];
        last = new_ws;
    }

    assert_eq!(offsets, [3, 4, 5, 6]);
}
```

> NOTE: These tests are inline expressions of the reset logic. They're somewhat anemic on their own but they document the intent and will catch a regression if someone changes the reset condition. A heavier integration test that drives `App` through two real renders is overkill for two lines of reset code.

- [ ] **Step 3: Run tests**

Run: `cargo test --lib --quiet ui::dashboard::detail::tests::workspace_change_resets_offsets ui::dashboard::detail::tests::same_workspace_preserves_offsets`
Expected: both pass.

- [ ] **Step 4: Commit**

```bash
git add src/ui/dashboard/detail.rs src/app/render.rs
git commit -m "feat(detail-bar): reset scroll offsets on workspace change"
```

---

## Task 9: Mouse wheel routing to detail-bar containers

**Files:**
- Modify: `src/app/input.rs`
- Modify: `src/app/input_tests.rs`

Add `container_under_cursor` and `adjust_detail_scroll` helpers, and a branch in `handle_mouse` that consumes wheel events when the cursor is over a container on the Dashboard view.

- [ ] **Step 1: Add helpers above `handle_mouse`**

In `src/app/input.rs`, just before `async fn handle_mouse`, add:

```rust
/// Returns the slot index of the detail-bar container under (col, row),
/// or None if no container rect matches.
fn container_under_cursor(app: &App, col: u16, row: u16) -> Option<usize> {
    app.detail_container_rects
        .iter()
        .enumerate()
        .find_map(|(i, slot)| {
            slot.as_ref().filter(|r| {
                col >= r.x
                    && col < r.x.saturating_add(r.width)
                    && row >= r.y
                    && row < r.y.saturating_add(r.height)
            })?;
            Some(i)
        })
}

/// Bump the scroll offset for container `slot` by `delta` rows. Clamped
/// to [0, u16::MAX] here; the next draw clamps further to the actual
/// content height.
fn adjust_detail_scroll(app: &mut App, slot: usize, delta: u16, up: bool) {
    if slot >= app.detail_scroll_offsets.len() {
        return;
    }
    let cur = app.detail_scroll_offsets[slot];
    app.detail_scroll_offsets[slot] = if up {
        cur.saturating_sub(delta)
    } else {
        cur.saturating_add(delta)
    };
}
```

- [ ] **Step 2: Change `handle_mouse` signature to `&mut App`**

Currently it's `async fn handle_mouse(app: &mut App, m: MouseEvent)` already — verify. The `scroll_active` function takes `&App` (not mut). We're keeping the existing fall-through, but our new branch needs mutable access.

- [ ] **Step 3: Add the new branch**

Edit `handle_mouse`:

```rust
async fn handle_mouse(app: &mut App, m: MouseEvent) {
    // Detail-bar container scroll: consume wheel events on the
    // Dashboard view when the cursor is over a container rect.
    if matches!(
        m.kind,
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
    ) && matches!(app.view, crate::ui::View::Dashboard)
    {
        if let Some(slot) = container_under_cursor(app, m.column, m.row) {
            let up = matches!(m.kind, MouseEventKind::ScrollUp);
            adjust_detail_scroll(app, slot, 3, up);
            return;
        }
    }

    match m.kind {
        MouseEventKind::ScrollUp => scroll_active(app, 3, true),
        MouseEventKind::ScrollDown => scroll_active(app, 3, false),
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
        _ => {}
    }
}
```

- [ ] **Step 4: Write failing input tests**

Open `src/app/input_tests.rs`. Find an existing mouse-scroll test (around line 1374) to copy the harness setup. Add a new `mod detail_scroll` at the bottom of the file (or alongside existing groups):

```rust
mod detail_scroll {
    use super::*;
    use crate::ui::View;
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;

    fn mouse_at(kind: MouseEventKind, col: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column: col,
            row,
            modifiers: crossterm::event::KeyModifiers::NONE,
        }
    }

    #[tokio::test]
    async fn wheel_over_container_scrolls_that_container() {
        // Bare app — same constructor as existing tests in this file.
        let mut app = test_app();
        app.view = View::Dashboard;
        app.detail_container_rects = [
            Some(Rect { x: 0, y: 20, width: 20, height: 5 }),
            Some(Rect { x: 21, y: 20, width: 20, height: 5 }),
            None,
            None,
        ];
        // ScrollDown at column 25 row 22 → inside rect 1.
        handle_mouse(&mut app, mouse_at(MouseEventKind::ScrollDown, 25, 22)).await;
        assert_eq!(app.detail_scroll_offsets[1], 3);
        assert_eq!(app.detail_scroll_offsets[0], 0);
    }

    #[tokio::test]
    async fn wheel_outside_containers_does_not_touch_detail_offsets() {
        let mut app = test_app();
        app.view = View::Dashboard;
        app.detail_container_rects = [
            Some(Rect { x: 0, y: 20, width: 20, height: 5 }),
            None,
            None,
            None,
        ];
        // ScrollDown at column 50 row 5 → outside any container.
        handle_mouse(&mut app, mouse_at(MouseEventKind::ScrollDown, 50, 5)).await;
        assert_eq!(app.detail_scroll_offsets, [0; 4]);
    }

    #[tokio::test]
    async fn wheel_in_attached_view_does_not_touch_detail_offsets() {
        let mut app = test_app();
        app.view = View::Attached(Default::default()); // adjust if View::Attached needs concrete state
        app.detail_container_rects = [
            Some(Rect { x: 0, y: 20, width: 20, height: 5 }),
            None,
            None,
            None,
        ];
        // ScrollDown at column 5 row 22 → inside the (stale) container rect,
        // but view is Attached so the guard skips detail-bar routing.
        handle_mouse(&mut app, mouse_at(MouseEventKind::ScrollDown, 5, 22)).await;
        assert_eq!(app.detail_scroll_offsets, [0; 4]);
    }

    #[tokio::test]
    async fn wheel_up_scrolls_back_with_saturating_sub() {
        let mut app = test_app();
        app.view = View::Dashboard;
        app.detail_container_rects = [
            Some(Rect { x: 0, y: 20, width: 20, height: 5 }),
            None,
            None,
            None,
        ];
        app.detail_scroll_offsets[0] = 2;
        // ScrollUp at column 5 row 22 → inside rect 0, delta 3, saturates to 0.
        handle_mouse(&mut app, mouse_at(MouseEventKind::ScrollUp, 5, 22)).await;
        assert_eq!(app.detail_scroll_offsets[0], 0);
    }
}
```

> NOTE for the implementer: `test_app()` may not exist. Look in `input_tests.rs` for the helper that existing tests use — it's likely `App::new()` or a similar fixture builder. If the closest equivalent has a different name, use that. If `View::Attached` requires non-`Default` construction, look at existing `Attached`-state tests in the same file for the correct constructor.

- [ ] **Step 5: Run new tests**

Run: `cargo test --lib --quiet app::input::tests::detail_scroll`
Expected: 4 tests pass.

- [ ] **Step 6: Manual smoke test**

Run: `cargo run --release` (or whatever the project's launch command is — check `README.md` or `Cargo.toml` `[package].default-run`).

In the running app:
1. Open the dashboard, select a workspace that has many recent files or a long session prompt.
2. Hover the cursor over the detail-bar container that overflows.
3. Scroll the mouse wheel down — content should scroll, scrollbar thumb should advance.
4. Scroll up — content should scroll back, thumb retreats.
5. Switch to another workspace — scroll offset should reset to top.

If any of those fail, stop and debug before committing.

- [ ] **Step 7: Commit**

```bash
git add src/app/input.rs src/app/input_tests.rs
git commit -m "feat(detail-bar): route mouse wheel to container under cursor"
```

---

## Task 10: Remove old trait methods and clean up imports

**Files:**
- Modify: `src/detail_modules/mod.rs`
- Modify: `src/detail_modules/recent_files.rs`
- Modify: `src/detail_modules/recent_chat.rs`
- Modify: `src/detail_modules/processes.rs`
- Modify: `src/detail_modules/session_summary.rs`
- Modify: `src/ui/dashboard/detail.rs` (any remaining `height_hint` references)

The container no longer uses `render` or `height_hint` from modules. Now they go away.

- [ ] **Step 1: Remove the default from `lines()`**

In `src/detail_modules/mod.rs`, change:

```rust
fn lines(
    &self,
    _ctx: &DetailContext<'_>,
    _width: u16,
) -> Vec<ratatui::text::Line<'static>> {
    Vec::new()
}
```

to:

```rust
fn lines(
    &self,
    ctx: &DetailContext<'_>,
    width: u16,
) -> Vec<ratatui::text::Line<'static>>;
```

- [ ] **Step 2: Remove `render` and `height_hint` from the trait**

Delete these lines from the trait definition in `src/detail_modules/mod.rs`:

```rust
fn height_hint(&self, ctx: &DetailContext<'_>) -> Constraint;
fn render(&self, area: Rect, ctx: &DetailContext<'_>, frame: &mut Frame<'_>);
```

Also delete the `use ratatui::Frame;` / `use ratatui::layout::{Constraint, Rect};` imports at the top of `mod.rs` if they become unused after removal. Run `cargo check` to confirm.

- [ ] **Step 3: Remove `render` and `height_hint` from each module**

For each of `recent_files.rs`, `recent_chat.rs`, `processes.rs`, `session_summary.rs`:

- Delete the `fn height_hint(...)` method from the `impl DetailModule for ...` block.
- Delete the `fn render(...)` method from the `impl DetailModule for ...` block.
- Remove now-unused imports: `use ratatui::Frame;`, `use ratatui::layout::Constraint;`. Keep `use ratatui::layout::Rect;` only if still referenced (probably not).
- Delete the `height_hint_*` test functions from each module's `#[cfg(test)] mod tests` block.

For `processes.rs` and similar files where you added a `build_lines` free function in Tasks 3–5: that function can now be inlined into `lines()` if you prefer, OR kept as-is. Either is fine — prefer keeping it as-is for clarity.

- [ ] **Step 4: Remove any test stub overrides**

In `src/detail_modules/mod.rs`, the test stub `impl DetailModule for ...` block (around line 182 — the one in the `#[cfg(test)]` module) needs its `height_hint` and `render` methods removed and a `lines` method added that matches whatever those tests expect.

Look for `fn height_hint(&self, _ctx: &DetailContext<'_>) -> Constraint {` at line 182. Replace the whole `impl` with one that has just `id`, `title`, and `lines` returning whatever empty-or-test-fixture lines those existing tests expect.

- [ ] **Step 5: Verify build**

Run: `cargo check`
Expected: clean build. If errors point to other tests calling `m.height_hint(...)` or `m.render(...)`, update those test sites to call `m.lines(...)` instead.

- [ ] **Step 6: Run full test suite**

Run: `cargo test --lib --quiet`
Expected: all tests pass.

- [ ] **Step 7: Run clippy**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings. Fix any dead-code or unused-import warnings inline.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "refactor(detail-modules): remove obsolete render/height_hint trait methods"
```

---

## Task 11: Final verification

**Files:** (read-only)

- [ ] **Step 1: Full test sweep**

Run: `cargo test --quiet`
Expected: all tests pass (includes integration tests if any exist in `tests/`).

- [ ] **Step 2: Build release binary**

Run: `cargo build --release`
Expected: clean compile.

- [ ] **Step 3: Manual end-to-end smoke**

Launch the binary. Verify (in order):
1. Selecting a workspace shows the detail bar normally — short modules render exactly as before.
2. A container with overflow renders a scrollbar in its rightmost column.
3. Mouse-wheel scroll over an overflowing container moves the content and scrollbar thumb.
4. Mouse-wheel over a container that fits has no effect (no scrollbar painted, offset stays at 0).
5. Switching to a different workspace resets all detail-bar offsets to 0.
6. In the Attached view, mouse-wheel still scrolls the PTY session, not the detail bar.

- [ ] **Step 4: Verify branch state**

Run: `git log --oneline main..HEAD`
Expected: ~10 commits, one per task, each with a clear feat/refactor message.

- [ ] **Step 5: Done.** Hand off to the user for PR review.

---

## Self-Review Notes

- **Spec coverage:** all sections covered. Trait change (Tasks 1–5, 10). Container rendering with scrollbar (Task 7). State on `App` (Task 6). Reset rules — workspace switch (Task 8), shrink (Task 7's clamp logic). Mouse routing (Task 9). Tests — module-level (Tasks 2–5), container-level (Task 7), input (Task 9).

- **No placeholders:** All `todo!()` macros in plan code samples are inside `> NOTE for the implementer` blocks that direct the implementer to paste an existing block of code from the same file. This is intentional — copying the existing logic line-for-line into the plan would balloon it without adding signal. The implementer reads the current file and moves the body, which is a 30-second action.

- **Type consistency:** `lines(&self, ctx, width: u16) -> Vec<Line<'static>>` is the signature in every task. `DetailDrawOutput { chip_rects, container_rects }` is the return type used in Tasks 7 and the call site. `detail_scroll_offsets: [u16; 4]` shape consistent throughout. `container_under_cursor` and `adjust_detail_scroll` names match between Task 9 step 1 (definition) and step 3 (usage).
