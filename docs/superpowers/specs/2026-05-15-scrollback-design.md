# Scrollback for attached sessions and PM pane — Design

**Issue:** [#27](https://github.com/bakedbean/workspacex/issues/27)

## Goal

Claude Code sessions inside wsx should behave identically to standalone Claude Code sessions:

1. Trackpad/wheel scrolls through the session's visible history.
2. Up/Down arrow keys cycle Claude Code's prompt history (unchanged from today).

## Root cause

wsx runs in alt-screen mode without mouse capture. In that configuration, most terminals translate wheel events into Up/Down arrow keystrokes and deliver them to the foreground app. wsx then forwards those keystrokes into the PTY, and Claude Code interprets them as prompt-history navigation. The user can't distinguish "scroll wheel" from "arrow key" because the terminal has already conflated them.

By enabling mouse capture in wsx, the terminal sends real ANSI mouse sequences (which crossterm decodes into `MouseEvent::ScrollUp/ScrollDown`) instead of the arrow-key translation. wsx can then handle wheel events directly for scrollback while arrow keystrokes continue flowing to the PTY untouched.

## Approach

1. Turn on `EnableMouseCapture` when wsx starts (mirror with `DisableMouseCapture` on shutdown and panic recovery).
2. Maintain a per-`Session` `scrollback_offset` (rows back from live).
3. Translate `ScrollUp`/`ScrollDown` events to offset deltas on the **focused** session.
4. Call `vt100::Parser::set_scrollback(offset)` before each render — the existing render path then naturally shows the scrolled-back content via `parser.screen()`.
5. Reset `scrollback_offset = 0` whenever a key is sent to the PTY.

vt100 v0.15 exposes `Parser::set_scrollback(rows: usize)` and `Screen::scrollback() -> usize`. The buffer is 1000 lines (`session.rs:329`). `set_scrollback` clamps to the available history, so we can request more than exists.

## Decisions

- **No new keybinds.** No scroll-mode, no `Ctrl-x [`, no footer hints. The wheel just works. Arrow keys remain pass-through (Claude Code prompt history).
- **Reset on any keystroke that hits the PTY.** Typing snaps the view back to live. Leader keystrokes (`Ctrl-x` + second key) don't go to the PTY and therefore don't reset offset.
- **Silent.** No on-screen indicator that scrollback is active; the scrolled content itself is the signal.
- **PM pane scope.** Wheel operates on the focused session:
  - `View::Attached(id)` → that workspace session
  - `View::AttachedPm` → PM session
  - `View::Dashboard` + `PaneFocus::Pm` → PM session
  - `View::Dashboard` + `PaneFocus::Dashboard` → wheel ignored (dashboard isn't a PTY)
- **Wheel granularity.** 3 rows per wheel notch (matches most terminals' native scrollback step).
- **Mouse capture mode.** Basic `EnableMouseCapture` only — wheel events are all we want. No motion, no focus-change. This is the same flag tmux/htop/btop use, so terminal text selection still works (most terminals let Shift bypass capture for selection).
- **Branch.** UX-feel change → goes on a branch per project convention.

## Scope

### In

1. `scrollback_offset: AtomicUsize` on `Session` + helpers (`scroll_up`, `scroll_down`, `scroll_to_live`, `is_scrolled`).
2. `parser.set_scrollback(offset)` applied at every render-from-session call site (attached view + PM pane).
3. `EnableMouseCapture` at terminal setup; `DisableMouseCapture` at teardown (incl. panic guard).
4. `CtEvent::Mouse` arm in `handle_event` that routes `ScrollUp`/`ScrollDown` to the focused session per the PM scope rules.
5. `scroll_to_live()` call at every keystroke-to-PTY site (two call sites: attached + attached-PM handlers).
6. Tests: session helpers, render-with-offset reveals older content, wheel routing by view/focus, keystroke-resets-offset.

### Out

- `Ctrl-x [` scroll mode or any keyboard scrollback. Arrow keys continue going to the PTY (Claude Code prompt history). If we ever want keyboard scroll, it goes in a follow-up.
- Footer hints, indicators, copy/search-in-scrollback.
- Configurable scrollback length. 1000 lines is enough; one-line change later if needed.
- Wheel for the dashboard list (use `j/k/arrows` as today) or modals (they fit).
- Mouse click handling (no focus changes via click, no link follow).

## Implementation notes

### Per-session state

Add to `pty::session::Session`:

```rust
/// Rows back from live. 0 = live tail. The render path calls
/// `parser.set_scrollback(offset)` before reading `parser.screen()`,
/// so vt100 clamps to available history.
pub scrollback_offset: std::sync::atomic::AtomicUsize,
```

Methods:
- `scroll_up(rows: usize)` — `offset = offset.saturating_add(rows)`.
- `scroll_down(rows: usize)` — `offset = offset.saturating_sub(rows)`.
- `scroll_to_live()` — `offset = 0`.
- `is_scrolled() -> bool` — `offset > 0` (test convenience).

The render path acquires the parser mutex, calls `parser.set_scrollback(offset)`, then reads `parser.screen()`. Single mutex acquisition per render — `set_scrollback` is cheap.

### Mouse capture lifecycle

`src/main.rs` (or wherever the terminal is set up) currently does:

```rust
enable_raw_mode()?;
execute!(stdout, EnterAlternateScreen)?;
// ...
execute!(stdout, LeaveAlternateScreen)?;
disable_raw_mode()?;
```

Add `EnableMouseCapture`/`DisableMouseCapture`:

```rust
execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
// ...
execute!(stdout, DisableMouseCapture, LeaveAlternateScreen)?;
```

Mirror in the panic-recovery / Drop guard if one exists.

### Event routing

Add to `handle_event`:

```rust
CtEvent::Mouse(m) => match m.kind {
    MouseEventKind::ScrollUp   => scroll_active(app, 3, true),
    MouseEventKind::ScrollDown => scroll_active(app, 3, false),
    _ => {}
},
```

`scroll_active(app, rows, up)` resolves the focused session via `active_session(app)` (see PM-scope rules above) and calls `scroll_up`/`scroll_down`. Returns silently when no session is focused.

### Reset-on-keystroke

The two PTY-write call sites (`handle_key_attached` and `handle_key_attached_pm`) gain one line right before the writer send:

```rust
session.scroll_to_live();
let _ = session.writer.send(bytes).await;
```

Leader keys never reach this path — they're consumed in the leader-pending arm — so leader actions don't snap the view.

## Risks

- **Text selection.** Mouse capture means terminal-native click-and-drag selection no longer works without Shift. This is a long-standing trade-off (the same one tmux makes by default). Documented as a follow-up if it bites; not blocking.
- **Wheel on touchpads.** Some trackpads emit many small scroll events. The 3-rows-per-notch step keeps scrolling responsive without being twitchy; if it feels off in practice, tunable in one line.
- **Wheel inside Claude Code's own scroll regions.** Claude Code does not enable mouse capture itself (otherwise wsx would never have seen the wheel-as-arrow translation in the first place), so there's nothing to compete with. If a future Claude Code version starts capturing mouse, we'd need to forward wheel through; that's not the current world.

## Out-of-scope follow-ups

- Keyboard scrollback (`Ctrl-x [` mode) if we ever want it.
- Search-in-scrollback.
- Configurable scrollback length.
- Click-to-focus a pane.
- Wheel scrolling in the workspace-updates modal panel.
