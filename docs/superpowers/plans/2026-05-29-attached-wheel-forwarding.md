# Attached Wheel Forwarding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** In the attached view, forward a plain mouse wheel to the inner agent when it has mouse reporting enabled (so its full-screen UI scrolls), keep wsx scrollback otherwise, and make Shift+wheel always drive wsx scrollback as an escape hatch.

**Architecture:** A new `Session::wheel_report_bytes` encodes a wheel event into the bytes the inner agent expects (or returns `None` when the agent isn't in mouse mode). The attached render path records each pane's `(Arc<Session>, Rect)` so the mouse handler can find the pane under the cursor and translate coordinates. A single new branch in `handle_mouse` forwards plain wheels over a mouse-aware pane; everything else falls through to the untouched `scroll_active`.

**Tech Stack:** Rust, ratatui, crossterm 0.28, `fnug-vt100` 0.15.2 (crate name `vt100`), tokio.

**Spec:** `docs/superpowers/specs/2026-05-29-attached-wheel-forwarding-design.md`

---

## File Structure

- `src/pty/session.rs` — add `Session::wheel_report_bytes` (encoder) + unit tests. Owns all `vt100` mouse types.
- `src/app.rs` — add `attached_pane_rects` field + its default.
- `src/app/render.rs` — clear `attached_pane_rects` each frame; populate it from the two attached render branches.
- `src/ui/attached.rs` — `render_one_pane` returns its terminal `Rect`; `render_panes` returns a `PanesDrawOutput { chip_rects, pane_rects }`.
- `src/app/input.rs` — `pane_under_cursor` helper + the new forward branch in `handle_mouse`.
- `src/app/input_tests.rs` — integration tests for the three routing cases.

---

## Task 1: `Session::wheel_report_bytes` encoder

**Files:**
- Modify: `src/pty/session.rs` (add method in `impl Session`, near `scroll_to_live` around line 187; add tests in the existing `#[cfg(test)] mod tests` that already contains `spawn_for_test`)

- [ ] **Step 1: Write the failing tests**

Add these to the test module in `src/pty/session.rs` (the same module that defines `spawn_for_test`, near the other `session_scroll_*` tests):

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wheel_report_none_when_mouse_mode_off() {
    let s = spawn_for_test();
    assert!(s.wheel_report_bytes(true, 5, 10).is_none());
    s.kill();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wheel_report_sgr_when_sgr_mode() {
    let s = spawn_for_test();
    {
        let mut p = s.parser.lock().unwrap();
        p.process(b"\x1b[?1000h\x1b[?1006h"); // mouse on + SGR encoding
    }
    assert_eq!(s.wheel_report_bytes(true, 5, 10), Some(b"\x1b[<64;5;10M".to_vec()));
    assert_eq!(s.wheel_report_bytes(false, 5, 10), Some(b"\x1b[<65;5;10M".to_vec()));
    s.kill();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wheel_report_x10_when_default_encoding() {
    let s = spawn_for_test();
    {
        let mut p = s.parser.lock().unwrap();
        p.process(b"\x1b[?1000h"); // mouse on, default (non-SGR) encoding
    }
    // up=64 -> 32+64=96; col 1 -> 33; row 1 -> 33
    assert_eq!(s.wheel_report_bytes(true, 1, 1), Some(vec![0x1b, b'[', b'M', 96, 33, 33]));
    s.kill();
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib wheel_report -- --nocapture`
Expected: FAIL — `no method named wheel_report_bytes found for struct Session`.

- [ ] **Step 3: Implement the method**

Add to `impl Session` in `src/pty/session.rs` (right after `scroll_to_live`, ~line 191):

```rust
/// Encode a wheel event for the inner program when it has mouse reporting
/// enabled. Returns `None` when mouse mode is off, in which case the caller
/// should fall back to wsx's own scrollback. `col`/`row` are 1-based cell
/// coordinates relative to the pane the cursor is over.
pub fn wheel_report_bytes(&self, up: bool, col: u16, row: u16) -> Option<Vec<u8>> {
    let p = self.parser.lock().unwrap();
    let screen = p.screen();
    if matches!(screen.mouse_protocol_mode(), vt100::MouseProtocolMode::None) {
        return None;
    }
    // Wheel-up = button 64, wheel-down = 65 (press-only -> trailing `M`).
    let cb: u16 = if up { 64 } else { 65 };
    match screen.mouse_protocol_encoding() {
        vt100::MouseProtocolEncoding::Sgr => {
            Some(format!("\x1b[<{cb};{col};{row}M").into_bytes())
        }
        // Default + Utf8: legacy X10 triplet. Agents request SGR in practice;
        // this path keeps a non-SGR mouse app from receiving a malformed
        // sequence. Coordinates clamp to 223 so `32 + coord` fits in a byte.
        _ => {
            let c = col.min(223) as u8;
            let r = row.min(223) as u8;
            Some(vec![0x1b, b'[', b'M', 32 + cb as u8, 32 + c, 32 + r])
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib wheel_report -- --nocapture`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src/pty/session.rs
git commit -m "feat(pty): Session::wheel_report_bytes mouse-wheel encoder"
```

---

## Task 2: Capture attached pane rects

**Files:**
- Modify: `src/app.rs` (add field ~line 212 near `detail_container_rects`; add default ~line 288 near `detail_container_rects: [None; 4]`)
- Modify: `src/ui/attached.rs` (`render_one_pane` return type; `render_panes` return type + new `PanesDrawOutput`)
- Modify: `src/app/render.rs` (clear field in `draw()` ~line 21; populate in both attached branches)

This task is structural (it feeds a `Frame`, so it's verified by `cargo build` + the existing suite staying green rather than a new unit test).

- [ ] **Step 1: Add the `App` field**

In `src/app.rs`, immediately after the `detail_container_rects` field (~line 212):

```rust
    /// Per-pane `(session, content rect)` from the last attached-view draw.
    /// Consumed by `handle_mouse` to find the pane under the cursor and
    /// forward wheel events to a mouse-aware agent. Storing the `Arc<Session>`
    /// directly lets the PM pane (which lives in `app.pm`, not `app.sessions`)
    /// be recorded the same way as workspace panes. Cleared each frame.
    pub attached_pane_rects: Vec<(std::sync::Arc<crate::pty::session::Session>, ratatui::layout::Rect)>,
```

In the `App` constructor where defaults are set (the struct literal containing `detail_container_rects: [None; 4],`, ~line 288), add:

```rust
            attached_pane_rects: Vec::new(),
```

- [ ] **Step 2: Make `render_one_pane` return its terminal rect, and `render_panes` return pane rects**

In `src/ui/attached.rs`:

Add the output struct near `PaneSpec` (after line 21):

```rust
/// What `render_panes` reports back to the caller for input hit-testing.
pub struct PanesDrawOutput {
    /// Clickable rects of the pinned-command chips (same as before).
    pub chip_rects: Vec<Rect>,
    /// `(session, terminal content rect)` for each rendered pane.
    pub pane_rects: Vec<(Arc<Session>, Rect)>,
}
```

Change `render_one_pane`'s signature to return the `term_area` it drew into. Its first line currently computes `(title_area, term_area)`; return `term_area` at the end:

```rust
fn render_one_pane(f: &mut Frame, pane: &PaneSpec<'_>, show_title: bool, theme: &Theme) -> Rect {
```

and add `term_area` as the final expression of the function (after the existing `let offset = ...; ... drop(parser);` block, replacing the implicit `()` return at the end of the body):

```rust
    drop(parser);
    term_area
}
```

Change `render_panes` to return `PanesDrawOutput`. Replace its signature return type `-> Vec<Rect>` with `-> PanesDrawOutput`, and replace the pane loop + final `render_chip_row` return:

```rust
    let show_titles = panes.len() > 1;

    let mut pane_rects = Vec::with_capacity(panes.len());
    for pane in panes {
        let term_area = render_one_pane(f, pane, show_titles, theme);
        pane_rects.push((Arc::clone(pane.session), term_area));
    }

    render_dividers(f, dividers, theme);

    if let Some(line) = attention_line {
        f.render_widget(Paragraph::new(line), status_area);
    }

    let footer_text = ratatui::text::Text::from(vec![
        Line::from(Vec::<Span<'static>>::new()),
        footer_line(footer_label, multi_pane_footer, theme),
    ]);
    f.render_widget(Paragraph::new(footer_text), footer_area);

    let chip_rects = render_chip_row(f, chip_area, pinned, theme);
    PanesDrawOutput { chip_rects, pane_rects }
```

- [ ] **Step 3: Clear the field each frame**

In `src/app/render.rs`, in `draw()` next to the other per-frame clears (after `app.detail_container_rects = [None; 4];`, ~line 21):

```rust
    app.attached_pane_rects.clear();
```

- [ ] **Step 4: Populate it from the attached branches**

In `src/app/render.rs`, `View::Attached` branch, replace:

```rust
            let chip_rects = attached::render_panes(
                f,
                &specs,
                &dividers,
                chip_area,
                status_area,
                footer_area,
                &focused_label,
                multi_pane,
                line,
                &pinned,
                &app.theme,
            );
            app.chip_rects = chip_rects;
            app.pinned_commands_cache = pinned;
```

with:

```rust
            let out = attached::render_panes(
                f,
                &specs,
                &dividers,
                chip_area,
                status_area,
                footer_area,
                &focused_label,
                multi_pane,
                line,
                &pinned,
                &app.theme,
            );
            app.chip_rects = out.chip_rects;
            app.attached_pane_rects = out.pane_rects;
            app.pinned_commands_cache = pinned;
```

In the `View::AttachedPm` branch, replace:

```rust
                let _chip_rects = attached::render_panes(
                    f,
                    &specs,
                    &[],
                    chip_area,
                    status_area,
                    footer_area,
                    "project-manager",
                    false,
                    line,
                    pinned,
                    &app.theme,
                );
```

with:

```rust
                let out = attached::render_panes(
                    f,
                    &specs,
                    &[],
                    chip_area,
                    status_area,
                    footer_area,
                    "project-manager",
                    false,
                    line,
                    pinned,
                    &app.theme,
                );
                app.attached_pane_rects = out.pane_rects;
```

- [ ] **Step 5: Build and run the full suite**

Run: `cargo build 2>&1 | tail -5 && cargo test --lib 2>&1 | tail -15`
Expected: build succeeds; all existing tests still pass (no regressions from the return-type change).

- [ ] **Step 6: Commit**

```bash
git add src/app.rs src/ui/attached.rs src/app/render.rs
git commit -m "feat(attached): capture per-pane rects for wheel hit-testing"
```

---

## Task 3: Forward the wheel in `handle_mouse`

**Files:**
- Modify: `src/app/input.rs` (add `pane_under_cursor` helper near `container_under_cursor` ~line 1426; add branch in `handle_mouse` ~line 1457)
- Test: `src/app/input_tests.rs` (new test module after the `detail_scroll` module ~line 3699)

- [ ] **Step 1: Write the failing tests**

Append a new module to `src/app/input_tests.rs` (after the `mod detail_scroll { ... }` block):

```rust
#[cfg(test)]
mod attached_wheel_forwarding {
    use super::*;
    use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;
    use std::path::PathBuf;
    use std::sync::atomic::Ordering;

    fn mouse_at_mod(kind: MouseEventKind, col: u16, row: u16, mods: KeyModifiers) -> MouseEvent {
        MouseEvent { kind, column: col, row, modifiers: mods }
    }

    // Enable SGR mouse reporting on the session's parser and register a
    // full-screen pane rect so the cursor at (10,10) is "over" the pane.
    fn arm_mouse_mode_and_pane(app: &mut App, ws_id: crate::store::WorkspaceId) {
        let session = app.sessions.get(ws_id).unwrap();
        {
            let mut p = session.parser.lock().unwrap();
            p.process(b"\x1b[?1000h\x1b[?1006h");
        }
        app.attached_pane_rects = vec![(session, Rect { x: 0, y: 0, width: 80, height: 24 })];
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn plain_wheel_forwards_when_mouse_mode_on() {
        let store = crate::store::Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);
        arm_mouse_mode_and_pane(&mut app, ws_id);
        handle_mouse(&mut app, mouse_at_mod(MouseEventKind::ScrollUp, 10, 10, KeyModifiers::NONE)).await;
        // Forwarded to the agent -> wsx scrollback must NOT move.
        assert_eq!(
            app.sessions.get(ws_id).unwrap().scrollback_offset.load(Ordering::Relaxed),
            0,
            "plain wheel over a mouse-aware pane is forwarded, not scrolled locally"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shift_wheel_is_escape_hatch_to_scrollback() {
        let store = crate::store::Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);
        arm_mouse_mode_and_pane(&mut app, ws_id);
        handle_mouse(&mut app, mouse_at_mod(MouseEventKind::ScrollUp, 10, 10, KeyModifiers::SHIFT)).await;
        // Shift bypasses the agent -> wsx scrollback moves.
        assert_eq!(
            app.sessions.get(ws_id).unwrap().scrollback_offset.load(Ordering::Relaxed),
            3,
            "shift+wheel drives wsx scrollback even when the agent has mouse mode on"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn plain_wheel_scrolls_when_mouse_mode_off() {
        let store = crate::store::Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);
        // Register the pane rect but do NOT enable mouse mode.
        let session = app.sessions.get(ws_id).unwrap();
        app.attached_pane_rects = vec![(session, Rect { x: 0, y: 0, width: 80, height: 24 })];
        handle_mouse(&mut app, mouse_at_mod(MouseEventKind::ScrollUp, 10, 10, KeyModifiers::NONE)).await;
        assert_eq!(
            app.sessions.get(ws_id).unwrap().scrollback_offset.load(Ordering::Relaxed),
            3,
            "without agent mouse mode, plain wheel falls through to wsx scrollback"
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib attached_wheel_forwarding -- --nocapture`
Expected: FAIL — `plain_wheel_forwards_when_mouse_mode_on` fails (offset is `3`, not `0`) because forwarding isn't implemented yet; the other two pass already.

- [ ] **Step 3: Add the `pane_under_cursor` helper**

In `src/app/input.rs`, after `container_under_cursor` (~line 1440):

```rust
/// Returns the `(session, rect)` of the attached-view pane under (col, row),
/// or None when the cursor is over chrome / no pane. Mirrors
/// `container_under_cursor`'s saturating bounds check.
fn pane_under_cursor(
    app: &App,
    col: u16,
    row: u16,
) -> Option<(std::sync::Arc<crate::pty::session::Session>, ratatui::layout::Rect)> {
    app.attached_pane_rects.iter().find_map(|(session, r)| {
        let in_rect = col >= r.x
            && col < r.x.saturating_add(r.width)
            && row >= r.y
            && row < r.y.saturating_add(r.height);
        if in_rect {
            Some((std::sync::Arc::clone(session), *r))
        } else {
            None
        }
    })
}
```

- [ ] **Step 4: Add the forward branch in `handle_mouse`**

In `src/app/input.rs`, in `handle_mouse`, immediately after the existing Dashboard detail-bar `if` block (the one ending at the `}` before `match m.kind {`, ~line 1471), insert:

```rust
    // Attached view: a plain wheel over a pane whose agent has mouse
    // reporting on is forwarded to that agent's PTY so it scrolls its own
    // view (notably its full-screen UI, where wsx has no scrollback).
    // Shift+wheel, panes without mouse mode, and scrolls over chrome all
    // fall through to `scroll_active` (wsx scrollback) below.
    if matches!(
        m.kind,
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
    ) && matches!(
        app.view,
        crate::ui::View::Attached(_) | crate::ui::View::AttachedPm
    ) && !m.modifiers.contains(KeyModifiers::SHIFT)
    {
        if let Some((session, rect)) = pane_under_cursor(app, m.column, m.row) {
            let up = matches!(m.kind, MouseEventKind::ScrollUp);
            let rel_col = m.column.saturating_sub(rect.x).saturating_add(1);
            let rel_row = m.row.saturating_sub(rect.y).saturating_add(1);
            if let Some(bytes) = session.wheel_report_bytes(up, rel_col, rel_row) {
                let _ = session.writer.send(bytes).await;
                return;
            }
        }
    }
```

Note: `KeyModifiers` is already imported in `src/app/input.rs` (line 19). No new import needed.

- [ ] **Step 5: Run the new tests to verify they pass**

Run: `cargo test --lib attached_wheel_forwarding -- --nocapture`
Expected: PASS (3 tests).

- [ ] **Step 6: Run the full suite for regressions**

Run: `cargo test --lib 2>&1 | tail -15`
Expected: all tests pass — in particular the existing `wheel_up_scrolls_attached_workspace` (its `mouse_event` is at col/row 0,0 with an empty `attached_pane_rects`, so `pane_under_cursor` returns `None` and it still scrolls scrollback).

- [ ] **Step 7: Commit**

```bash
git add src/app/input.rs src/app/input_tests.rs
git commit -m "feat(input): forward wheel to mouse-aware agent; shift+wheel scrollback hatch"
```

---

## Task 4: Verify and finalize

- [ ] **Step 1: Lint and format**

Run: `cargo clippy --all-targets 2>&1 | tail -20 && cargo fmt`
Expected: no new clippy warnings; `cargo fmt` leaves a clean tree (or commit the formatting).

- [ ] **Step 2: Manual smoke test (real terminal)**

Run wsx in Alacritty (or iTerm2), attach to a workspace whose agent uses a full-screen UI, and confirm:
- Plain wheel scrolls the agent's own view (was previously stuck).
- Shift+wheel drives wsx's scrollback on the primary screen.
- Dashboard detail-bar wheel scrolling is unchanged.

If anything misbehaves, return to systematic-debugging before patching.

- [ ] **Step 3: Final commit (if fmt produced changes)**

```bash
git add -A
git commit -m "style: cargo fmt"
```

---

## Self-Review Notes

- **Spec coverage:** encoder + `None` fallback (Task 1) → "Components §1"; pane-rect capture storing `Arc<Session>` (Task 2) → "Components §2"; forward branch leaving `scroll_active` untouched + Shift hatch (Task 3) → "Components §3" and the behavior table; wheel-only (no click forwarding) preserved; dashboard routing untouched (Task 3 guards on `View::Attached | AttachedPm`).
- **Types:** `wheel_report_bytes(up: bool, col: u16, row: u16) -> Option<Vec<u8>>` used identically in Task 1 (def) and Task 3 (call). `PanesDrawOutput { chip_rects, pane_rects }` defined in Task 2 and consumed in the same task's render edits. `attached_pane_rects: Vec<(Arc<Session>, Rect)>` defined in Task 2, read in Task 3.
- **No placeholders:** every code/command step is concrete.
