# Scrollback for attached sessions and PM pane — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Trackpad/wheel scrolls a session's history exactly like a standalone terminal would; Up/Down arrows keep cycling Claude Code's prompt history (unchanged). No new keybinds, no UI changes — just wheel→scrollback.

**Architecture:** Add `scrollback_offset` to `Session`, apply it via `Parser::set_scrollback` before each render, enable terminal mouse capture, route `MouseEventKind::ScrollUp/ScrollDown` to the focused session, reset offset on any keystroke that reaches the PTY.

**Tech Stack:** Rust, crossterm (MouseEvent + EnableMouseCapture), ratatui, vt100 v0.15, tokio.

**Spec:** `docs/superpowers/specs/2026-05-15-scrollback-design.md`

---

## File Structure

- `src/pty/session.rs` — add `scrollback_offset: AtomicUsize` to `Session`, plus `scroll_up/scroll_down/scroll_to_live/is_scrolled` methods.
- `src/pty/render.rs` (and/or callers in `src/ui/attached.rs` + `src/ui/pm_pane.rs`) — apply `parser.set_scrollback(offset)` before reading the screen.
- `src/main.rs` — `EnableMouseCapture` / `DisableMouseCapture` at terminal setup/teardown (including any panic-recovery path).
- `src/app.rs` — new `CtEvent::Mouse` arm in `handle_event`, new `handle_mouse`/`scroll_active`/`active_session` helpers, single-line `scroll_to_live()` call at the two PTY-write sites.

No new files. No keybind changes.

---

### Task 1: Add `scrollback_offset` to `Session` with helper methods

**Files:**
- Modify: `src/pty/session.rs` (Session struct, impl block, tests module)

- [ ] **Step 1: Write the failing tests**

Inside the existing `#[cfg(test)] mod tests` block in `src/pty/session.rs`, append:

```rust
#[test]
fn session_scroll_offset_starts_at_zero() {
    let s = Session::spawn_for_test();
    assert_eq!(
        s.scrollback_offset.load(std::sync::atomic::Ordering::Relaxed),
        0
    );
    assert!(!s.is_scrolled());
}

#[test]
fn session_scroll_up_advances_offset() {
    let s = Session::spawn_for_test();
    s.scroll_up(5);
    assert_eq!(
        s.scrollback_offset.load(std::sync::atomic::Ordering::Relaxed),
        5
    );
    assert!(s.is_scrolled());
}

#[test]
fn session_scroll_down_is_saturating() {
    let s = Session::spawn_for_test();
    s.scroll_up(3);
    s.scroll_down(10);
    assert_eq!(
        s.scrollback_offset.load(std::sync::atomic::Ordering::Relaxed),
        0
    );
}

#[test]
fn session_scroll_to_live_zeroes_offset() {
    let s = Session::spawn_for_test();
    s.scroll_up(42);
    s.scroll_to_live();
    assert_eq!(
        s.scrollback_offset.load(std::sync::atomic::Ordering::Relaxed),
        0
    );
    assert!(!s.is_scrolled());
}
```

If `Session::spawn_for_test()` does not exist, check how nearby tests construct a `Session` (e.g., `spawn_and_echo` or `kill_all_terminates_child` in the same module) and inline that construction here. The Session needs to compile + exist, not actually be PTY-driven.

- [ ] **Step 2: Run tests to verify they fail**

```
cargo test --lib pty::session::tests::session_scroll -- --test-threads=1 2>&1 | tail -10
```

Expected: 4 compile errors — `scrollback_offset`, `scroll_up`, `scroll_down`, `scroll_to_live`, `is_scrolled` not found on `Session`.

- [ ] **Step 3: Add the field to `Session`**

Find the `pub struct Session { ... }` definition. Add the field alongside the other atomics:

```rust
/// Rows back from live tail. 0 = live. The render path calls
/// `parser.set_scrollback(offset)` before reading `parser.screen()`,
/// so vt100 clamps to whatever scrollback actually exists.
pub scrollback_offset: std::sync::atomic::AtomicUsize,
```

Find the Session constructor (search for `Session {` initialization, likely in `spawn` or `new`). Add:

```rust
scrollback_offset: std::sync::atomic::AtomicUsize::new(0),
```

- [ ] **Step 4: Add the methods**

In the `impl Session { ... }` block, add:

```rust
pub fn scroll_up(&self, rows: usize) {
    use std::sync::atomic::Ordering;
    let cur = self.scrollback_offset.load(Ordering::Relaxed);
    self.scrollback_offset
        .store(cur.saturating_add(rows), Ordering::Relaxed);
}

pub fn scroll_down(&self, rows: usize) {
    use std::sync::atomic::Ordering;
    let cur = self.scrollback_offset.load(Ordering::Relaxed);
    self.scrollback_offset
        .store(cur.saturating_sub(rows), Ordering::Relaxed);
}

pub fn scroll_to_live(&self) {
    self.scrollback_offset
        .store(0, std::sync::atomic::Ordering::Relaxed);
}

pub fn is_scrolled(&self) -> bool {
    self.scrollback_offset
        .load(std::sync::atomic::Ordering::Relaxed)
        > 0
}
```

- [ ] **Step 5: Run tests to verify they pass**

```
cargo test --lib pty::session::tests::session_scroll -- --test-threads=1 2>&1 | tail -10
```

Expected: 4 passed.

- [ ] **Step 6: Commit**

```bash
git add src/pty/session.rs
git commit -m "feat(pty): scrollback_offset + helpers on Session"
```

---

### Task 2: Apply scrollback offset in the render path

**Files:**
- Read first: `src/pty/render.rs`, `src/ui/attached.rs` (line ~33-35), `src/ui/pm_pane.rs`

- [ ] **Step 1: Locate every `parser.screen()` read site**

Search for `.parser.lock()` and `.screen()` across `src/pty/render.rs`, `src/ui/attached.rs`, and `src/ui/pm_pane.rs`. The current shape is:

```rust
let parser = session.parser.lock().unwrap();
let screen = parser.screen();
render_screen(screen, f.buffer_mut(), term_area);
```

- [ ] **Step 2: Change `parser` to `mut` and apply offset at each site**

At each site, replace the snippet with:

```rust
let offset = session
    .scrollback_offset
    .load(std::sync::atomic::Ordering::Relaxed);
let mut parser = session.parser.lock().unwrap();
parser.set_scrollback(offset);
let screen = parser.screen();
render_screen(screen, f.buffer_mut(), term_area);
```

`set_scrollback` clamps internally to whatever buffer exists, so no extra bounds check is needed.

- [ ] **Step 3: Build to verify it compiles**

```
cargo build 2>&1 | tail -5
```

Expected: clean.

- [ ] **Step 4: Write an integration test that scrolling reveals older content**

In `src/pty/session.rs` tests, add:

```rust
#[test]
fn scrollback_offset_reveals_older_content_via_set_scrollback() {
    let s = Session::spawn_for_test();
    // Feed enough output to overflow the screen so vt100 starts
    // moving rows into the scrollback buffer.
    {
        let mut p = s.parser.lock().unwrap();
        for i in 0..200 {
            p.process(format!("line {i}\r\n").as_bytes());
        }
    }
    // Live view at offset 0 should show the most recent line.
    {
        let mut p = s.parser.lock().unwrap();
        p.set_scrollback(0);
        let live = p.screen().contents();
        assert!(
            live.contains("line 199"),
            "live view should include latest: {live}"
        );
    }
    // Scroll up — set_scrollback to a large number should clamp and
    // produce a different (older) viewport.
    s.scroll_up(150);
    {
        let mut p = s.parser.lock().unwrap();
        p.set_scrollback(
            s.scrollback_offset
                .load(std::sync::atomic::Ordering::Relaxed),
        );
        let scrolled = p.screen().contents();
        assert!(
            !scrolled.contains("line 199"),
            "scrolled view should not show the latest line: {scrolled}"
        );
    }
}
```

- [ ] **Step 5: Run the new test**

```
cargo test --lib scrollback_offset_reveals_older_content -- --test-threads=1 2>&1 | tail -10
```

Expected: pass.

- [ ] **Step 6: Commit**

```bash
git add src/pty/render.rs src/ui/attached.rs src/ui/pm_pane.rs src/pty/session.rs
git commit -m "feat(pty): apply scrollback offset before reading screen"
```

---

### Task 3: Enable mouse capture at terminal setup

**Files:**
- Modify: `src/main.rs` (terminal setup + teardown, plus any panic-recovery hook)

- [ ] **Step 1: Find the terminal init/teardown**

Search `src/main.rs` for `enable_raw_mode`, `EnterAlternateScreen`. The pattern is likely:

```rust
enable_raw_mode()?;
execute!(stdout, EnterAlternateScreen)?;
// ... run loop ...
execute!(stdout, LeaveAlternateScreen)?;
disable_raw_mode()?;
```

Also look for a `set_hook`/panic-recovery block; if one exists, it usually calls `LeaveAlternateScreen` and `disable_raw_mode` on panic.

- [ ] **Step 2: Add `EnableMouseCapture` / `DisableMouseCapture`**

Update setup:

```rust
use crossterm::event::{EnableMouseCapture, DisableMouseCapture};
enable_raw_mode()?;
execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
```

Update teardown:

```rust
execute!(stdout, DisableMouseCapture, LeaveAlternateScreen)?;
disable_raw_mode()?;
```

If a panic hook exists, mirror `DisableMouseCapture` next to the `LeaveAlternateScreen` call there.

- [ ] **Step 3: Build to verify it compiles**

```
cargo build 2>&1 | tail -5
```

Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat(ui): enable terminal mouse capture for wheel events"
```

---

### Task 4: Route mouse-wheel events to the focused session

**Files:**
- Modify: `src/app.rs` (`handle_event`, new `handle_mouse`/`scroll_active`/`active_session`)

- [ ] **Step 1: Write the failing tests**

Add inside the existing `#[cfg(test)] mod tests` block in `src/app.rs`:

```rust
#[test]
fn wheel_up_scrolls_attached_workspace() {
    use crossterm::event::{MouseEvent, MouseEventKind, KeyModifiers};
    let (mut app, _tmp, ws_id) = test_app_with_attached_workspace();
    let before = app.sessions.get(ws_id).unwrap().scrollback_offset
        .load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(before, 0);
    handle_mouse(&mut app, MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 0, row: 0,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(
        app.sessions.get(ws_id).unwrap().scrollback_offset
            .load(std::sync::atomic::Ordering::Relaxed),
        3,
        "one wheel notch = 3 rows"
    );
}

#[test]
fn wheel_down_decreases_offset_saturating() {
    use crossterm::event::{MouseEvent, MouseEventKind, KeyModifiers};
    let (mut app, _tmp, ws_id) = test_app_with_attached_workspace();
    app.sessions.get(ws_id).unwrap().scroll_up(5);
    handle_mouse(&mut app, MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 0, row: 0,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(
        app.sessions.get(ws_id).unwrap().scrollback_offset
            .load(std::sync::atomic::Ordering::Relaxed),
        2
    );
}

#[test]
fn wheel_targets_pm_when_pm_attached() {
    use crossterm::event::{MouseEvent, MouseEventKind, KeyModifiers};
    let mut app = test_app_with_attached_pm();
    handle_mouse(&mut app, MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 0, row: 0,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(
        app.pm.as_ref().unwrap().scrollback_offset
            .load(std::sync::atomic::Ordering::Relaxed),
        3
    );
}

#[test]
fn wheel_targets_pm_in_dashboard_when_pm_focused() {
    use crossterm::event::{MouseEvent, MouseEventKind, KeyModifiers};
    let mut app = test_app_dashboard_with_pm_focused();
    handle_mouse(&mut app, MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 0, row: 0,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(
        app.pm.as_ref().unwrap().scrollback_offset
            .load(std::sync::atomic::Ordering::Relaxed),
        3
    );
}

#[test]
fn wheel_noop_when_dashboard_focused_no_target() {
    use crossterm::event::{MouseEvent, MouseEventKind, KeyModifiers};
    let mut app = test_app_dashboard_focused();
    // Just verify nothing panics; no targetable session means silent no-op.
    handle_mouse(&mut app, MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 0, row: 0,
        modifiers: KeyModifiers::NONE,
    });
}
```

If helpers like `test_app_with_attached_workspace` don't already exist, look at existing tests that construct an `App` with attached views (search for `View::Attached(` and `View::AttachedPm` inside the tests module). Build small inline helpers using the same construction; do not factor them out into a shared module unless three or more tests need them.

- [ ] **Step 2: Run tests to verify they fail**

```
cargo test --lib wheel_ -- --test-threads=1 2>&1 | tail -15
```

Expected: compile errors — `handle_mouse` not found.

- [ ] **Step 3: Add the helpers**

In `src/app.rs`, near `handle_event`, add:

```rust
fn handle_mouse(app: &mut App, m: crossterm::event::MouseEvent) {
    use crossterm::event::MouseEventKind;
    match m.kind {
        MouseEventKind::ScrollUp   => scroll_active(app, 3, true),
        MouseEventKind::ScrollDown => scroll_active(app, 3, false),
        _ => {}
    }
}

/// Apply a scroll delta to the session currently under focus. `up=true`
/// scrolls toward older content (higher offset).
fn scroll_active(app: &App, rows: usize, up: bool) {
    let Some(session) = active_session(app) else { return };
    if up {
        session.scroll_up(rows);
    } else {
        session.scroll_down(rows);
    }
}

/// Returns the session that should receive scroll input given the current
/// view + focus, or None when there is no targetable session.
fn active_session(app: &App) -> Option<std::sync::Arc<crate::pty::session::Session>> {
    match app.view {
        View::Attached(id) => app.sessions.get(id),
        View::AttachedPm   => app.pm.clone(),
        View::Dashboard if matches!(app.focus, crate::ui::PaneFocus::Pm) => app.pm.clone(),
        _ => None,
    }
}
```

If `app.sessions.get(id)` does not already return an `Arc<Session>`, inspect `SessionManager::get` and adjust the return type / clone behavior to match. The contract `active_session` needs is "an owned handle that survives the mutex unlock"; in practice that means `Arc<Session>`.

- [ ] **Step 4: Hook the mouse arm into `handle_event`**

Find the existing match in `handle_event` (the one that currently handles `CtEvent::Key` and `CtEvent::Resize`). Add:

```rust
CtEvent::Mouse(m) => handle_mouse(app, m),
```

- [ ] **Step 5: Run the tests to verify they pass**

```
cargo test --lib wheel_ -- --test-threads=1 2>&1 | tail -15
```

Expected: 5 passed.

- [ ] **Step 6: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): route mouse wheel to focused session scrollback"
```

---

### Task 5: Reset scrollback on keystrokes that hit the PTY

**Files:**
- Modify: `src/app.rs` (`handle_key_attached`, `handle_key_attached_pm`)

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn keystroke_to_pty_resets_scrollback() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let (mut app, _tmp, ws_id) = test_app_with_attached_workspace();
    app.sessions.get(ws_id).unwrap().scroll_up(20);
    assert!(app.sessions.get(ws_id).unwrap().is_scrolled());
    handle_key_attached(
        &mut app,
        ws_id,
        KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
    );
    assert!(
        !app.sessions.get(ws_id).unwrap().is_scrolled(),
        "any keystroke flowing to PTY must snap to live"
    );
}

#[test]
fn leader_keystroke_does_not_reset_scrollback() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let (mut app, _tmp, ws_id) = test_app_with_attached_workspace();
    app.sessions.get(ws_id).unwrap().scroll_up(20);
    // Ctrl-x (leader) — consumed by wsx, never sent to PTY.
    handle_key_attached(
        &mut app,
        ws_id,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    );
    assert!(
        app.sessions.get(ws_id).unwrap().is_scrolled(),
        "leader key is consumed by wsx; offset should be preserved"
    );
}

#[test]
fn arrow_key_resets_scrollback_and_forwards_to_pty() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let (mut app, _tmp, ws_id) = test_app_with_attached_workspace();
    app.sessions.get(ws_id).unwrap().scroll_up(20);
    // Up arrow goes to the PTY (Claude Code prompt history). It should
    // also snap scrollback to live since it's a real keystroke.
    handle_key_attached(
        &mut app,
        ws_id,
        KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
    );
    assert!(!app.sessions.get(ws_id).unwrap().is_scrolled());
}
```

- [ ] **Step 2: Run tests to verify they fail**

```
cargo test --lib keystroke_to_pty_resets_scrollback leader_keystroke_does_not_reset_scrollback arrow_key_resets -- --test-threads=1 2>&1 | tail -15
```

Expected: the `keystroke_to_pty_resets` and `arrow_key_resets` cases fail (scroll state persists); `leader_keystroke_does_not_reset` passes incidentally (we haven't changed anything yet).

- [ ] **Step 3: Add `scroll_to_live` at the two PTY-write sites**

In `handle_key_attached`, find the path that calls `session.writer.send(bytes).await` (or the equivalent channel send for the encoded keystroke). Just before it, add:

```rust
session.scroll_to_live();
let _ = session.writer.send(bytes).await;
```

Repeat in `handle_key_attached_pm` (PM session writer call).

Critically: this goes **after** the leader-pending arm has had its chance to consume the keystroke, so leader keys never reach this point and never reset scrollback.

- [ ] **Step 4: Run tests to verify they pass**

```
cargo test --lib keystroke_to_pty_resets_scrollback leader_keystroke_does_not_reset_scrollback arrow_key_resets -- --test-threads=1 2>&1 | tail -15
```

Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): reset session scrollback on any PTY-bound keystroke"
```

---

### Task 6: Final fmt + clippy + full test pass + manual smoke

**Files:** none (verification only)

- [ ] **Step 1: Format**

```
cargo fmt
```

- [ ] **Step 2: Clippy**

```
cargo clippy --all-targets -- -D warnings 2>&1 | tail -20
```

Expected: no warnings.

- [ ] **Step 3: Full test suite**

```
cargo test --lib -- --test-threads=1 2>&1 | tail -10
```

Expected: all pass. Baseline before this plan: 249. This plan adds ~12 tests; expect ~261 total.

- [ ] **Step 4: Manual smoke**

Run wsx, attach to a session with plenty of history, confirm:
- Wheel up scrolls back through the visible session content.
- Wheel down returns toward live.
- Up/Down arrows still cycle Claude Code's prompt history (no scrollback effect).
- Typing any printable character snaps the view back to live.
- `Ctrl-x u` / `Ctrl-x d` / etc. do **not** reset scrollback (leader actions preserve offset).
- Same behaviors apply when attached to the PM pane and when the dashboard split is active with PM focused.

- [ ] **Step 5: Push the branch**

```
git push -u origin <branch-name>
```
