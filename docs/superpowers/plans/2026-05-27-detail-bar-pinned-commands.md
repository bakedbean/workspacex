# Detail-bar pinned commands — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Surface the same pinned-command chips that appear in the attached view in the workspace detail bar, between the body and the inline reply input. Chips fire on click and on `Ctrl-X` + `1..9` regardless of whether the reply input has focus.

**Architecture:** Reuse the attached view's existing chip layout/render functions verbatim. Insert a 1-row chip slot into the detail-bar layout only when pinned is non-empty. Resolve pinned in `app/render.rs`'s dashboard branch (mirror the attached branch). Reuse the existing `app.chip_rects` / `app.pinned_commands_cache` plumbing for both mouse and keyboard activation. Add a dashboard-side `Ctrl-X` leader paralleling the attached view's, sharing the `app.leader_pending` flag.

**Tech Stack:** Rust 2021, ratatui, tokio. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-05-27-detail-bar-pinned-commands-design.md`

---

## File Structure

| File | Role |
|---|---|
| `src/ui/attached.rs` | Promote `render_chip_row` from private to `pub(crate)` so the detail bar can call it. No behavior change. |
| `src/ui/dashboard/detail.rs` | Add `pinned: &[PinnedCommand]` to `DetailInputs`. Insert a 1-row chip slot between the body's bottom rule and the reply row when pinned is non-empty. Change `render` return type to `Vec<Rect>` (the chip rects). |
| `src/app/render.rs` | Dashboard branch: resolve `pinned` for the selected workspace's repo (mirror the attached branch); pass to `DetailInputs`; stash returned rects into `app.chip_rects` and the resolved pinned into `app.pinned_commands_cache`. |
| `src/app/input.rs` | New `chip_target_session(app)` helper. New `fire_chip(app, idx)` helper. `handle_mouse` chip branch uses the new helpers. Add `Ctrl-X` arming + chord completion in the dashboard key dispatcher. `handle_detail_bar_reply_key` arms on `Ctrl-X` and yields when the leader is pending. View transitions clear `leader_pending`. |
| `src/app/input_tests.rs` | Integration tests for click + chord activation across focus states; chord clear on view change. |

## Conventions

- **TDD:** every behavior task starts with a failing test.
- **Commits:** one logical change per commit. Run `cargo test --workspace` before each commit.
- **Reuse without divergence:** the chip row visual is `attached::render_chip_row`. Do not duplicate or restyle.
- **Existing references to read first:**
  - Chip renderer + layout: `src/ui/attached.rs:263-337`
  - Chip-row hit-test (mouse): `src/app/input.rs:1238-1257`
  - Attached `Ctrl-X` chord (`1..9` arm): `src/app/input.rs:727-735`
  - Attached `Ctrl-X` arming: `src/app/input.rs:740-743`
  - Reply-input keyhandler: `src/app/input.rs:1148-1207`
  - Reply-input focus entry (Tab): `src/app/input.rs:257-272`
  - Attached-view pinned resolution: `src/app/render.rs:354-365`
  - Detail-bar render: `src/ui/dashboard/detail.rs:51-116`
  - `DetailInputs` struct: `src/ui/dashboard/detail.rs:22-45`
  - `active_session` (do not modify): `src/app/input.rs:123-134`
  - `App::leader_pending`: `src/app.rs` (search for `leader_pending`)

Reading these references first will make every task below faster.

---

## Task 1: Promote `attached::render_chip_row` to `pub(crate)`

**Files:**
- Modify: `src/ui/attached.rs`

- [ ] **Step 1: Change visibility**

In `src/ui/attached.rs`, locate `fn render_chip_row` at line 289. Change its signature from:

```rust
fn render_chip_row(
    f: &mut Frame,
    area: Rect,
    pinned: &[PinnedCommand],
    theme: &Theme,
) -> Vec<Rect> {
```

to:

```rust
pub(crate) fn render_chip_row(
    f: &mut Frame,
    area: Rect,
    pinned: &[PinnedCommand],
    theme: &Theme,
) -> Vec<Rect> {
```

No other change. The existing callsite in `render_panes` (line 75) keeps working.

- [ ] **Step 2: Verify it builds**

Run: `cargo build`
Expected: completes successfully. The function is now reachable from `crate::ui::dashboard::detail`.

- [ ] **Step 3: Verify tests still pass**

Run: `cargo test --workspace`
Expected: all existing tests pass — this is a pure visibility change.

- [ ] **Step 4: Commit**

```bash
git add src/ui/attached.rs
git commit -m "refactor(attached): make render_chip_row pub(crate)"
```

---

## Task 2: Detail bar — add `pinned` to `DetailInputs` and render the chip row

**Files:**
- Modify: `src/ui/dashboard/detail.rs`

- [ ] **Step 1: Write the failing test — chip row appears with non-empty pinned**

In `src/ui/dashboard/detail.rs`, find the existing `#[cfg(test)] mod tests` block. Add this test alongside `full_render_paints_header_body_and_reply_row`:

```rust
#[test]
fn render_with_pinned_includes_chip_row_above_reply() {
    let (_store, repo, ws) = seed_workspace();
    let cfg = DetailBarConfig::default();
    let reg = make_registry();
    let pinned = vec![
        crate::pinned::PinnedCommand {
            label: "PR".into(),
            command: "/pull-request".into(),
        },
        crate::pinned::PinnedCommand {
            label: "FB".into(),
            command: "/feedback".into(),
        },
    ];
    let inputs = DetailInputs {
        repo: &repo,
        workspace: &ws,
        events: None,
        procs: &[],
        diff: None,
        diff_per_file: None,
        lifecycle: None,
        pr_title: None,
        pr_number: None,
        status: Status::Idle,
        ago_secs: None,
        reply_draft: "",
        reply_focused: false,
        events_scanned: true,
        config: &cfg,
        registry: &reg,
        pinned: &pinned,
    };
    let text = render_to_text(&inputs, 120, 12);
    // Chip labels must appear, and "Reply to agent" must still appear
    // (we only inserted a row, didn't remove the reply row).
    assert!(text.contains("PR"), "chip label PR present: {text:?}");
    assert!(text.contains("FB"), "chip label FB present: {text:?}");
    assert!(text.contains("Reply to agent"), "reply chip still present: {text:?}");

    // Chip row must sit ABOVE the reply row.
    let pr_line = text.lines().position(|l| l.contains(" PR ")).expect("PR line");
    let reply_line = text.lines().position(|l| l.contains("Reply to agent")).expect("reply line");
    assert!(pr_line < reply_line, "chip row above reply: pr={pr_line} reply={reply_line}");
}
```

Also extend the *existing* `DetailInputs` construction in `render_into_zero_area_is_a_noop`, `full_render_paints_header_body_and_reply_row`, `chrome_only_mode_renders_header_and_reply_no_body_labels`, `narrow_terminal_drops_chat_and_procs_columns`, `render_with_unknown_module_id_shows_placeholder`, and `render_one_container_fills_full_width` to include `pinned: &[]` so they still compile after Step 2.

- [ ] **Step 2: Run the failing test**

Run: `cargo test --package wsx --lib -- ui::dashboard::detail::tests::render_with_pinned_includes_chip_row_above_reply --nocapture`
Expected: FAIL — `DetailInputs` has no `pinned` field.

- [ ] **Step 3: Add `pinned` to `DetailInputs`**

In `src/ui/dashboard/detail.rs`, the struct `DetailInputs` ends around line 45. Add a new field after `registry`:

```rust
    pub registry: &'a crate::detail_modules::Registry,
    /// Pinned commands resolved for the selected workspace's repo. When
    /// empty, no chip row is rendered.
    pub pinned: &'a [crate::pinned::PinnedCommand],
}
```

- [ ] **Step 4: Change `render` return type and insert the chip slot**

Locate `pub fn render(f: &mut Frame, area: Rect, inputs: &DetailInputs<'_>, theme: &Theme)` at line 51. Change its signature to return `Vec<Rect>`:

```rust
pub fn render(
    f: &mut Frame,
    area: Rect,
    inputs: &DetailInputs<'_>,
    theme: &Theme,
) -> Vec<ratatui::layout::Rect> {
```

The early-return for too-small areas at line 52-54 should become:

```rust
    if area.height == 0 || area.height < inputs.config.minimum_height() {
        return Vec::new();
    }
```

Replace the `Layout::default()...split(area)` block (lines 63-72) so that the chip slot is conditional:

```rust
    let chip_present = !inputs.pinned.is_empty();
    let constraints: Vec<Constraint> = if chip_present {
        vec![
            Constraint::Length(1), // header
            Constraint::Length(1), // rule
            body_constraint,
            Constraint::Length(1), // rule
            Constraint::Length(1), // chips
            Constraint::Length(1), // reply
        ]
    } else {
        vec![
            Constraint::Length(1), // header
            Constraint::Length(1), // rule
            body_constraint,
            Constraint::Length(1), // rule
            Constraint::Length(1), // reply
        ]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);
```

The chunk indices for the existing rows shift when `chip_present` is true. Replace the rest of the function body (header / top rule / body / bottom rule / reply) so the index logic is explicit:

```rust
    let header_area = chunks[0];
    let top_rule_area = chunks[1];
    let body_area = chunks[2];
    let bottom_rule_area = chunks[3];
    let (chip_area, reply_area) = if chip_present {
        (Some(chunks[4]), chunks[5])
    } else {
        (None, chunks[4])
    };

    let header = build_header_strip(
        &inputs.workspace.name,
        &inputs.workspace.branch,
        inputs.lifecycle,
        inputs.diff,
        inputs.procs.len() as u32,
        inputs.status,
        inputs.ago_secs,
        theme,
        header_area.width as usize,
    );
    f.render_widget(Paragraph::new(header), header_area);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "─".repeat(top_rule_area.width as usize),
            theme.dim_style(),
        ))),
        top_rule_area,
    );

    render_body(f, body_area, inputs, theme);

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "─".repeat(bottom_rule_area.width as usize),
            theme.dim_style(),
        ))),
        bottom_rule_area,
    );

    let chip_rects = if let Some(area) = chip_area {
        crate::ui::attached::render_chip_row(f, area, inputs.pinned, theme)
    } else {
        Vec::new()
    };

    let reply = build_reply_row(
        inputs.reply_draft,
        inputs.reply_focused,
        theme,
        reply_area.width as usize,
    );
    f.render_widget(Paragraph::new(reply), reply_area);

    if inputs.reply_focused {
        let cx = reply_cursor_x(inputs.reply_draft, reply_area.width as usize);
        f.set_cursor_position((reply_area.x + cx, reply_area.y));
    }

    chip_rects
}
```

- [ ] **Step 5: Run the test**

Run: `cargo test --package wsx --lib -- ui::dashboard::detail::tests::render_with_pinned_includes_chip_row_above_reply --nocapture`
Expected: PASS.

- [ ] **Step 6: Verify existing detail-bar tests still pass**

Run: `cargo test --package wsx --lib -- ui::dashboard::detail::tests`
Expected: all PASS. The empty-pinned path must produce the same output as today — `chrome_only_mode_renders_header_and_reply_no_body_labels` and `full_render_paints_header_body_and_reply_row` both assert specific labels and would catch a layout regression.

- [ ] **Step 7: Write a test for the empty case**

Add this test alongside the others:

```rust
#[test]
fn render_without_pinned_omits_chip_row() {
    let (_store, repo, ws) = seed_workspace();
    let cfg = DetailBarConfig::default();
    let reg = make_registry();
    let inputs = DetailInputs {
        repo: &repo,
        workspace: &ws,
        events: None,
        procs: &[],
        diff: None,
        diff_per_file: None,
        lifecycle: None,
        pr_title: None,
        pr_number: None,
        status: Status::Idle,
        ago_secs: None,
        reply_draft: "",
        reply_focused: false,
        events_scanned: true,
        config: &cfg,
        registry: &reg,
        pinned: &[],
    };
    // Capture render's returned rects via a closure-bound outer mut
    // (Terminal::draw can't propagate values out of its closure).
    let mut terminal = ratatui::Terminal::new(
        ratatui::backend::TestBackend::new(120, 12),
    )
    .unwrap();
    let mut returned: Vec<ratatui::layout::Rect> = Vec::new();
    terminal
        .draw(|f| {
            let theme = Theme::wsx();
            returned = render(f, Rect::new(0, 0, 120, 12), &inputs, &theme);
        })
        .unwrap();
    assert!(returned.is_empty(), "no chip rects when pinned empty");
}
```

- [ ] **Step 8: Run the new test**

Run: `cargo test --package wsx --lib -- ui::dashboard::detail::tests::render_without_pinned_omits_chip_row --nocapture`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src/ui/dashboard/detail.rs
git commit -m "feat(detail-bar): render pinned chip row above reply input"
```

---

## Task 3: Resolve pinned in the dashboard render branch and populate caches

**Files:**
- Modify: `src/app/render.rs`

- [ ] **Step 1: Resolve pinned for the selected workspace**

In `src/app/render.rs`, locate the dashboard branch that builds `DetailInputs` (around lines 274-295). Just before the `let inputs = ...DetailInputs { ... };` line, add the pinned resolution block:

```rust
                        let global_pinned =
                            app.store.get_setting("pinned_commands").ok().flatten();
                        let repo_pinned = repo.pinned_commands.clone();
                        let pinned = crate::pinned::resolve(
                            global_pinned.as_deref(),
                            repo_pinned.as_deref(),
                        );
```

- [ ] **Step 2: Pass `pinned` into `DetailInputs`**

Still in the same block, extend the `DetailInputs { ... }` literal to include the new field. Locate the existing literal (it ends with `registry: &app.registry,`) and add immediately after it:

```rust
                            registry: &app.registry,
                            pinned: &pinned,
                        };
```

- [ ] **Step 3: Capture render's returned rects into `app`**

The current call site is:

```rust
                        crate::ui::dashboard::detail::render(f, detail_area, &inputs, &app.theme);
```

Change it so the returned `Vec<Rect>` is stored into `app.chip_rects`, and the resolved pinned into `app.pinned_commands_cache`, but only when non-empty (the frame-top clear handles the empty case):

```rust
                        let rects = crate::ui::dashboard::detail::render(
                            f, detail_area, &inputs, &app.theme,
                        );
                        if !rects.is_empty() {
                            app.chip_rects = rects;
                            app.pinned_commands_cache = pinned;
                        }
```

Note: `app.chip_rects` and `app.pinned_commands_cache` are cleared at the top of `draw()` (lines 16-17), so leaving them untouched when no chips render is correct.

- [ ] **Step 4: Verify it builds**

Run: `cargo build`
Expected: completes successfully.

- [ ] **Step 5: Verify tests still pass**

Run: `cargo test --workspace`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add src/app/render.rs
git commit -m "feat(detail-bar): resolve pinned commands in dashboard render"
```

---

## Task 4: Helper `chip_target_session` and `fire_chip` in input.rs

**Files:**
- Modify: `src/app/input.rs`

- [ ] **Step 1: Add `chip_target_session` helper**

In `src/app/input.rs`, locate `fn active_session` at line 123. Add a new helper immediately after it (around line 134):

```rust
/// Resolve the session that should receive a pinned-command dispatch.
/// In the attached view this is the focused pane; on the dashboard it
/// is the currently selected workspace.
fn chip_target_session(
    app: &App,
) -> Option<std::sync::Arc<crate::pty::session::Session>> {
    match &app.view {
        View::Attached(state) => state.focused_id().and_then(|id| app.sessions.get(id)),
        View::Dashboard => match app.selected_target() {
            Some(SelectionTarget::Workspace(id)) => app.sessions.get(id),
            _ => None,
        },
        _ => None,
    }
}
```

`SelectionTarget` is already in scope at the top of the file (it's the source of `selected_target()`'s return type). If the compiler complains about the import, add `use crate::app::SelectionTarget;` to the top of the function or to the existing `use` block.

- [ ] **Step 2: Add `fire_chip` helper**

Immediately after `chip_target_session`, add:

```rust
/// Dispatch the pinned command at `idx` to the chip-target session.
/// No-op when:
///   - `idx` exceeds the number of *visible* chip rects (the row may
///     have truncated some chips at narrow widths),
///   - the cache has no command at `idx` (defensive),
///   - no chip target can be resolved.
/// On dispatch, clears any in-flight reply draft and returns focus to
/// the dashboard so the reply input loses focus.
async fn fire_chip(app: &mut App, idx: usize) {
    if idx >= app.chip_rects.len() {
        return;
    }
    let cmd = match app.pinned_commands_cache.get(idx) {
        Some(c) => c.clone(),
        None => return,
    };
    let session = match chip_target_session(app) {
        Some(s) => s,
        None => return,
    };
    let mut bytes = cmd.command.into_bytes();
    bytes.push(b'\r');
    session.scroll_to_live();
    let _ = session.writer.send(bytes).await;
    app.dashboard.reply_draft.clear();
    app.focus = crate::ui::PaneFocus::Dashboard;
}
```

- [ ] **Step 3: Verify it builds**

Run: `cargo build`
Expected: completes successfully. Warnings about unused functions are expected at this point — Task 5 and Task 6 wire them up.

- [ ] **Step 4: Verify tests still pass**

Run: `cargo test --workspace`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app/input.rs
git commit -m "feat(input): add chip_target_session and fire_chip helpers"
```

---

## Task 5: Mouse — route detail-bar chip clicks through `fire_chip`

**Files:**
- Modify: `src/app/input.rs`
- Modify: `src/app/input_tests.rs`

- [ ] **Step 1: Write the failing test**

In `src/app/input_tests.rs`, near the existing chip-click tests (search for `app.chip_rects = vec!`), add:

```rust
#[tokio::test]
async fn detail_bar_chip_click_dispatches_to_selected_workspace() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    let (mut app, ws_id) = test_support::app_with_seeded_workspace().await;
    // Pretend we just drew a detail bar with one chip at row 5, columns 0..6.
    app.view = crate::ui::View::Dashboard;
    app.dashboard.selected = 0;
    app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
    app.pinned_commands_cache = vec![crate::pinned::PinnedCommand {
        label: "PR".into(),
        command: "/pr".into(),
    }];
    app.chip_rects = vec![ratatui::layout::Rect {
        x: 0,
        y: 5,
        width: 6,
        height: 1,
    }];

    let writer = app
        .sessions
        .get(ws_id)
        .expect("session for seeded workspace")
        .writer
        .clone();
    let mut rx = writer.subscribe_for_test();

    super::handle_mouse_for_test(
        &app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 5,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
    )
    .await;

    let bytes = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv())
        .await
        .expect("dispatch did not arrive")
        .expect("writer closed");
    assert_eq!(bytes, b"/pr\r");
}
```

This test references `test_support::app_with_seeded_workspace`, `super::handle_mouse_for_test`, and `writer.subscribe_for_test`. **Before writing the implementation**, check whether these exist:

```bash
grep -n "app_with_seeded_workspace\|handle_mouse_for_test\|subscribe_for_test" src/test_support.rs src/app/input_tests.rs src/pty/session.rs 2>/dev/null
```

If any are missing, the existing chip-click tests at `src/app/input_tests.rs:1606` and `:1648` will show the actual idioms in use — match those patterns instead of inventing new helpers. The mouse handler today is `async fn handle_mouse(app: &App, m: MouseEvent)` (line 1234); the test pattern is to call it directly via `super::` and observe `session.writer`.

If the chip click test cannot be written without new test infrastructure, write a smaller unit test that calls `chip_target_session` directly and asserts it returns the expected session for `View::Dashboard` + selected workspace.

- [ ] **Step 2: Run the failing test**

Run: `cargo test --package wsx --lib -- detail_bar_chip_click_dispatches_to_selected_workspace --nocapture`
Expected: FAIL — the mouse chip branch still calls `active_session(app)`, which returns `None` on the dashboard view, so no bytes are dispatched.

- [ ] **Step 3: Update the mouse handler**

In `src/app/input.rs`, locate `handle_mouse` at line 1234. The chip-click branch runs from line 1238 to line 1253:

```rust
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(idx) = app.chip_rects.iter().position(|r| {
                m.column >= r.x
                    && m.column < r.x.saturating_add(r.width)
                    && m.row >= r.y
                    && m.row < r.y.saturating_add(r.height)
            }) {
                if let Some(cmd) = app.pinned_commands_cache.get(idx) {
                    if let Some(session) = active_session(app) {
                        let mut bytes = cmd.command.as_bytes().to_vec();
                        bytes.push(b'\r');
                        session.scroll_to_live();
                        let _ = session.writer.send(bytes).await;
                    }
                }
            }
        }
```

Note that `handle_mouse` currently takes `app: &App` (shared ref). `fire_chip` takes `&mut App` because it clears the reply draft. Change `handle_mouse`'s signature to `app: &mut App` so it can call `fire_chip`. Update its caller (search with `grep -n "handle_mouse(" src/app/input.rs`) — there should be a single call site in the event-dispatch path; pass `&mut app` instead of `&app`.

Then replace the chip arm body with a single `fire_chip` call:

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

Routing both mouse and keyboard through `fire_chip` makes acceptance criteria #3 ("Clicking any chip dispatches… regardless of whether the reply input is focused") unambiguous: a click in either focus state clears the draft and returns focus to the dashboard.

- [ ] **Step 4: Run the test**

Run: `cargo test --package wsx --lib -- detail_bar_chip_click_dispatches_to_selected_workspace --nocapture`
Expected: PASS.

- [ ] **Step 5: Run the existing chip-click tests**

Run: `cargo test --package wsx --lib -- chip_click`
Expected: existing tests around `src/app/input_tests.rs:1606-1660` still PASS. These cover the attached view's mouse chip path, which is now routed through `chip_target_session`.

- [ ] **Step 6: Verify the full suite**

Run: `cargo test --workspace`
Expected: all PASS.

- [ ] **Step 7: Commit**

```bash
git add src/app/input.rs src/app/input_tests.rs
git commit -m "feat(input): route chip clicks through chip_target_session"
```

---

## Task 6: Keyboard — `Ctrl-X` chord on the dashboard view

**Files:**
- Modify: `src/app/input.rs`
- Modify: `src/app/input_tests.rs`

- [ ] **Step 1: Write a failing test — Ctrl-X then '1' from dashboard fires chip**

In `src/app/input_tests.rs`, add:

```rust
#[tokio::test]
async fn dashboard_ctrl_x_then_digit_fires_pinned_chip() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let (mut app, ws_id) = test_support::app_with_seeded_workspace().await;
    app.view = crate::ui::View::Dashboard;
    app.dashboard.selected = 0;
    app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
    app.pinned_commands_cache = vec![crate::pinned::PinnedCommand {
        label: "PR".into(),
        command: "/pr".into(),
    }];
    app.chip_rects = vec![ratatui::layout::Rect {
        x: 0,
        y: 5,
        width: 6,
        height: 1,
    }];

    let writer = app
        .sessions
        .get(ws_id)
        .unwrap()
        .writer
        .clone();
    let mut rx = writer.subscribe_for_test();

    // Ctrl-X arms the leader.
    super::dispatch_key_for_test(
        &mut app,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await;
    assert!(app.leader_pending, "leader should arm on Ctrl-X");

    // '1' fires chip 0.
    super::dispatch_key_for_test(
        &mut app,
        KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
    )
    .await;
    assert!(!app.leader_pending, "leader should clear after follow-up");

    let bytes = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv())
        .await
        .expect("dispatch did not arrive")
        .expect("writer closed");
    assert_eq!(bytes, b"/pr\r");
}
```

Same caveat as Task 5: if `dispatch_key_for_test` doesn't exist, use whatever pattern the existing chord tests use. Search:

```bash
grep -n "dispatch_key\|leader_pending" src/app/input_tests.rs | head
```

The attached-view `Ctrl-X`+digit tests (find with `grep -n "leader_pending" src/app/input_tests.rs`) are the model.

- [ ] **Step 2: Run the failing test**

Run: `cargo test --package wsx --lib -- dashboard_ctrl_x_then_digit_fires_pinned_chip --nocapture`
Expected: FAIL — no dashboard-side chord wiring yet.

- [ ] **Step 3: Add `Ctrl-X` arming + chord completion on the dashboard view**

In `src/app/input.rs`, locate the dashboard-view dispatch in the main key handler. The dashboard branch's main `match (k.code, k.modifiers)` starts at line 323. Immediately **before** that `match` (and after the filter-input block at line 277-301 and the z-leader block at line 302-322), insert:

```rust
    // Ctrl-X leader for pinned-command chord (mirrors the attached
    // view's binding). The next 1..9 fires the matching chip; any
    // other follow-up just clears the leader. Read by both the
    // dashboard main handler (below) and the reply-input handler.
    if k.code == LEADER_KEY && k.modifiers.contains(KeyModifiers::CONTROL) {
        app.leader_pending = true;
        return Ok(());
    }
    if app.leader_pending {
        app.leader_pending = false;
        if let KeyCode::Char(c @ '1'..='9') = k.code {
            let idx = (c as u8 - b'1') as usize;
            fire_chip(app, idx).await;
        }
        return Ok(());
    }
```

`LEADER_KEY` is the same constant used by the attached view. Verify it's in scope at this point in the file:

```bash
grep -n "LEADER_KEY" src/app/input.rs
```

If it's only imported in a narrower scope, hoist the `const` to the file's top section.

- [ ] **Step 4: Run the chord test**

Run: `cargo test --package wsx --lib -- dashboard_ctrl_x_then_digit_fires_pinned_chip --nocapture`
Expected: PASS.

- [ ] **Step 5: Write a failing test — Ctrl-X then non-digit clears leader without firing**

```rust
#[tokio::test]
async fn dashboard_ctrl_x_then_non_digit_clears_leader_no_fire() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let (mut app, ws_id) = test_support::app_with_seeded_workspace().await;
    app.view = crate::ui::View::Dashboard;
    app.dashboard.selected = 0;
    app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
    app.pinned_commands_cache = vec![crate::pinned::PinnedCommand {
        label: "PR".into(),
        command: "/pr".into(),
    }];
    app.chip_rects = vec![ratatui::layout::Rect {
        x: 0,
        y: 5,
        width: 6,
        height: 1,
    }];

    let writer = app.sessions.get(ws_id).unwrap().writer.clone();
    let mut rx = writer.subscribe_for_test();

    super::dispatch_key_for_test(
        &mut app,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await;
    super::dispatch_key_for_test(
        &mut app,
        KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
    )
    .await;
    assert!(!app.leader_pending, "leader cleared");

    let result = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await;
    assert!(result.is_err(), "no dispatch should fire: got {result:?}");
}
```

- [ ] **Step 6: Run the test**

Run: `cargo test --package wsx --lib -- dashboard_ctrl_x_then_non_digit_clears_leader_no_fire --nocapture`
Expected: PASS — the implementation from Step 3 already handles this (the `if let` falls through, the early `return Ok(())` swallows the key).

- [ ] **Step 7: Write a test — Ctrl-X then digit exceeds visible chips is a no-op**

```rust
#[tokio::test]
async fn dashboard_ctrl_x_digit_beyond_visible_chips_is_noop() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let (mut app, ws_id) = test_support::app_with_seeded_workspace().await;
    app.view = crate::ui::View::Dashboard;
    app.dashboard.selected = 0;
    app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
    // Cache has 3 commands but only 2 chips fit on screen.
    app.pinned_commands_cache = vec![
        crate::pinned::PinnedCommand { label: "A".into(), command: "/a".into() },
        crate::pinned::PinnedCommand { label: "B".into(), command: "/b".into() },
        crate::pinned::PinnedCommand { label: "C".into(), command: "/c".into() },
    ];
    app.chip_rects = vec![
        ratatui::layout::Rect { x: 0,  y: 5, width: 6, height: 1 },
        ratatui::layout::Rect { x: 8,  y: 5, width: 6, height: 1 },
    ];

    let writer = app.sessions.get(ws_id).unwrap().writer.clone();
    let mut rx = writer.subscribe_for_test();

    super::dispatch_key_for_test(&mut app, KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)).await;
    super::dispatch_key_for_test(&mut app, KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE)).await;

    let result = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await;
    assert!(result.is_err(), "no dispatch for chip 3 when only 2 visible");
}
```

- [ ] **Step 8: Run the test**

Run: `cargo test --package wsx --lib -- dashboard_ctrl_x_digit_beyond_visible_chips_is_noop --nocapture`
Expected: PASS — `fire_chip` guards on `idx >= chip_rects.len()`.

- [ ] **Step 9: Commit**

```bash
git add src/app/input.rs src/app/input_tests.rs
git commit -m "feat(input): wire Ctrl-X+digit chord on dashboard view"
```

---

## Task 7: Chord works while reply input is focused

**Files:**
- Modify: `src/app/input.rs`
- Modify: `src/app/input_tests.rs`

- [ ] **Step 1: Write a failing test — Ctrl-X while reply focused arms; '1' fires; draft cleared; focus returns**

```rust
#[tokio::test]
async fn ctrl_x_digit_works_while_reply_focused() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let (mut app, ws_id) = test_support::app_with_seeded_workspace().await;
    app.view = crate::ui::View::Dashboard;
    app.dashboard.selected = 0;
    app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
    app.focus = crate::ui::PaneFocus::DetailBarReply;
    app.dashboard.reply_draft = "half-typed message".into();
    app.pinned_commands_cache = vec![crate::pinned::PinnedCommand {
        label: "PR".into(),
        command: "/pr".into(),
    }];
    app.chip_rects = vec![ratatui::layout::Rect { x: 0, y: 5, width: 6, height: 1 }];

    let writer = app.sessions.get(ws_id).unwrap().writer.clone();
    let mut rx = writer.subscribe_for_test();

    super::dispatch_key_for_test(&mut app, KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)).await;
    assert!(app.leader_pending, "leader arms even while reply is focused");
    // Critically: '^X' should not have been pushed into the draft.
    assert_eq!(app.dashboard.reply_draft, "half-typed message");

    super::dispatch_key_for_test(&mut app, KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE)).await;

    let bytes = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv())
        .await
        .expect("dispatch did not arrive")
        .expect("writer closed");
    assert_eq!(bytes, b"/pr\r");
    assert!(app.dashboard.reply_draft.is_empty(), "draft cleared on fire");
    assert!(
        matches!(app.focus, crate::ui::PaneFocus::Dashboard),
        "focus returns to dashboard"
    );
}
```

- [ ] **Step 2: Run the failing test**

Run: `cargo test --package wsx --lib -- ctrl_x_digit_works_while_reply_focused --nocapture`
Expected: FAIL — `handle_detail_bar_reply_key` currently treats `Ctrl-X` as an unknown key (swallowed by the wildcard arm at line 1205), and even if it didn't, the leader-arming branch only exists below the reply-handler check.

- [ ] **Step 3: Special-case Ctrl-X and pending-leader in `handle_detail_bar_reply_key`**

In `src/app/input.rs`, locate `handle_detail_bar_reply_key` at line 1153. At the top of the `match (k.code, k.modifiers)` (line 1155), add two new arms **before** any other arm:

```rust
    use crossterm::event::{KeyCode, KeyModifiers};
    // If the leader is already armed (Ctrl-X from a previous tick),
    // yield to the dashboard dispatcher so the chord completes.
    if app.leader_pending {
        return false;
    }
    match (k.code, k.modifiers) {
        (KeyCode::Char(c), m)
            if (c == 'x' || c == 'X') && m.contains(KeyModifiers::CONTROL) =>
        {
            // Arm the leader; the dashboard dispatcher will consume
            // the next key as the chord completion.
            app.leader_pending = true;
            true
        }
        (KeyCode::Tab, _) => {
            // ...existing arm...
        }
```

(Keep the existing arms below; just prepend the two new ones. The `if app.leader_pending { return false; }` line sits before the `match` block — it doesn't add an arm.)

The `return false` for "yield to dashboard" relies on the caller's existing fall-through logic at `src/app/input.rs:249-256`, which falls through to the main dashboard handler when `handle_detail_bar_reply_key` returns `false`.

- [ ] **Step 4: Run the test**

Run: `cargo test --package wsx --lib -- ctrl_x_digit_works_while_reply_focused --nocapture`
Expected: PASS.

- [ ] **Step 5: Verify existing reply-input tests still pass**

Run: `cargo test --package wsx --lib -- handle_detail_bar_reply_key`
Expected: all PASS — the Tab / Enter / Esc / Char / Backspace arms are untouched.

- [ ] **Step 6: Commit**

```bash
git add src/app/input.rs src/app/input_tests.rs
git commit -m "feat(input): Ctrl-X chord works during reply input focus"
```

---

## Task 8: Clear `leader_pending` on view transitions

**Files:**
- Modify: `src/app/input.rs`
- Modify: `src/app/input_tests.rs`

- [ ] **Step 1: Write a failing test — armed leader doesn't survive Dashboard → Attached**

```rust
#[tokio::test]
async fn leader_clears_on_dashboard_to_attached_transition() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let (mut app, ws_id) = test_support::app_with_seeded_workspace().await;
    app.view = crate::ui::View::Dashboard;
    app.dashboard.selected = 0;
    app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];

    super::dispatch_key_for_test(&mut app, KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)).await;
    assert!(app.leader_pending);

    // Enter attached view (the actual key here depends on the codebase; the
    // existing tests reveal it — likely `Enter` or `a`). Use whatever the
    // existing "attach" test uses.
    super::dispatch_key_for_test(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).await;

    assert!(!app.leader_pending, "leader cleared on view transition");
}
```

If the test reveals that `Enter` doesn't enter the attached view in this fixture (because no session is wired for it), adjust the test to *manually* set `app.view = crate::ui::View::Attached(...)` after arming, or call whatever helper the codebase uses to drive a view transition. Search for an existing test that flips views:

```bash
grep -n "app.view = crate::ui::View::Attached\|enter_attached\|view = View::Attached" src/app/input_tests.rs
```

- [ ] **Step 2: Run the failing test**

Run: `cargo test --package wsx --lib -- leader_clears_on_dashboard_to_attached_transition --nocapture`
Expected: FAIL — the leader survives the view change today.

- [ ] **Step 3: Clear leader at each view transition site**

In `src/app/input.rs`, search for assignments of the form `app.view = `:

```bash
grep -n "app.view = " src/app/input.rs
```

At each callsite that transitions *between* dashboard and attached views, add `app.leader_pending = false;` immediately before (or after) the assignment. There are typically a handful of such sites — Enter from dashboard, `d` (detach) from attached, automatic bounce-back when a session goes away.

Example pattern:

```rust
    app.leader_pending = false;
    app.view = View::Dashboard;
```

- [ ] **Step 4: Run the test**

Run: `cargo test --package wsx --lib -- leader_clears_on_dashboard_to_attached_transition --nocapture`
Expected: PASS.

- [ ] **Step 5: Verify the whole suite**

Run: `cargo test --workspace`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add src/app/input.rs src/app/input_tests.rs
git commit -m "fix(input): clear leader_pending on dashboard/attached transitions"
```

---

## Task 9: Manual smoke test

**Files:** none.

- [ ] **Step 1: Build**

Run: `cargo build --release`
Expected: completes.

- [ ] **Step 2: Configure a global pinned command**

Run wsx and set a global pinned command via the UI (or via `cargo run -- set pinned_commands "PR=/pull-request"`; check `cargo run -- --help` for the exact CLI). This step exists so the smoke test has something to render.

- [ ] **Step 3: Launch wsx on a repo with at least one workspace**

Select a workspace on the dashboard. Confirm visually:
  - A 1-row chip strip appears between the body section and the reply input row.
  - The chip reads `1 PR` (numbered prefix + label) in the attached-view style.
  - The bar height is one row taller than it would be without pinned commands.

- [ ] **Step 4: Click the chip**

Move the mouse over the chip and click. Confirm:
  - The command `/pull-request\r` is written into the workspace's agent PTY (you can confirm by attaching to the workspace with Enter; the agent's input field will show the command echoed or already submitted).

- [ ] **Step 5: Keyboard chord (no reply focus)**

Without entering the reply input, press `Ctrl-X`, then `1`. Confirm same effect as Step 4.

- [ ] **Step 6: Keyboard chord (reply focused)**

Press `Tab` to focus the reply input, type a few characters, then press `Ctrl-X`, then `1`. Confirm:
  - The typed draft characters are **gone** (draft cleared on fire).
  - Focus returns to the dashboard (reply chip is dim).
  - The chip command was dispatched (same PTY effect as steps 4-5).

- [ ] **Step 7: No-op cases**

  - With pinned commands cleared, confirm the detail bar has no chip row.
  - With chips visible, press `Ctrl-X` then `a` — nothing should happen (leader clears, no PTY write).
  - With chips visible, press `Ctrl-X` then switch view (`Enter`), then press a digit — nothing should happen (leader cleared on view transition).

- [ ] **Step 8: Per-repo override**

Set a per-repo `pinned_commands` override that differs from the global. Confirm chips reflect the repo's override on that workspace and the global on workspaces of other repos.
