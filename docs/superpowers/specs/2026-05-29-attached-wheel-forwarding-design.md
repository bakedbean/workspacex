# Attached view: forward the wheel to mouse-aware agents, with a Shift+wheel escape hatch

## Problem

In the attached view, an agent (e.g. Claude Code) runs inside a PTY that wsx
renders via the `fnug-vt100` parser. wsx enables `EnableMouseCapture` globally
(`src/main.rs:72`) and never toggles it per-view, so the outer terminal reports
every mouse event to wsx and wsx never forwards mouse events to the inner agent.

The only wheel handling in the attached view is `scroll_active`
(`src/app/input.rs:1473-1475`), which bumps the session's own scrollback offset
(`src/pty/session.rs:173-191`) and is applied at render time via
`parser.set_scrollback(offset)` (`src/ui/attached.rs:115-122`).

This breaks whenever the agent is on the **alternate screen** — i.e. drawing its
full-screen interactive UI with its input box. The alternate screen has no
scrollback buffer, so `scroll_up` increments the offset but `set_scrollback`
reveals nothing: the wheel is consumed and the view never moves. From the user's
seat the input area "captures the wheel and holds it hostage," and the agent's
conversation history is unreachable. It is intermittent because it depends on
whether the agent is currently on its alt-screen UI versus streaming plainly to
the primary screen (where wsx scrollback works — proven by the passing
`scrollback_offset_reveals_older_content_via_set_scrollback` test).

This was confirmed empirically: feeding primary-screen history, switching to the
alternate screen (`ESC[?1049h`), then `scroll_up(150)` shows only the alt-screen
content; the primary history is not revealed.

## Goals

- When the inner agent has **mouse reporting enabled**, a plain wheel scroll in
  the attached view is forwarded to the agent's PTY as a mouse event, so the
  agent scrolls its own view (including its full-screen conversation). This is
  the standard tmux/wezterm behavior.
- When the agent does **not** have mouse reporting enabled, a plain wheel scroll
  drives wsx's own scrollback exactly as it does today (no behavior change).
- **Shift+wheel** is an escape hatch: it always drives wsx's own scrollback,
  regardless of the agent's mouse mode, so the user can reclaim the wheel for
  wsx's captured buffer. Verified deliverable on both target terminals
  (Alacritty and iTerm2 forward Shift+wheel to the application with the SHIFT
  modifier intact).
- Forwarded wheel events carry **pane-relative, 1-based** coordinates and target
  the pane under the cursor.

## Non-goals

- **Dashboard wheel routing is untouched.** The detail-bar container routing
  (`container_under_cursor` / `adjust_detail_scroll`, gated on
  `View::Dashboard`) keeps its current behavior.
- **Wheel only.** Clicks, drags, and motion are not forwarded; left-click chip
  handling stays exactly as today. Only `ScrollUp`/`ScrollDown` are affected.
- **No "reveal the saved primary screen" behavior for the hatch.** While the
  agent is on its alt screen, wsx's scrollback is genuinely empty, so Shift+wheel
  surfaces little there. The full-screen conversation is recovered via the
  *plain* (forwarded) wheel. Teaching the hatch to temporarily display the saved
  primary screen + its scrollback is a possible follow-up, explicitly out of
  scope here.
- **No change to global mouse capture.** `EnableMouseCapture` stays on for the
  whole session; we route in software rather than toggling capture per view.

## Behavior summary

Attached views only (`View::Attached` and `View::AttachedPm`):

| Gesture          | Agent mouse mode ON                              | Agent mouse mode OFF        |
|------------------|--------------------------------------------------|-----------------------------|
| Plain wheel      | Encode + write to the agent PTY (agent scrolls)  | wsx scrollback (unchanged)  |
| Shift+wheel      | wsx scrollback (escape hatch)                    | wsx scrollback              |

"Mouse mode ON" means the session's parser reports
`screen().mouse_protocol_mode() != MouseProtocolMode::None`.

## Components

### 1. `Session::wheel_report_bytes` (`src/pty/session.rs`)

```rust
/// Encode a wheel event for the inner program if it has mouse reporting
/// enabled. Returns None when mouse mode is off (caller should fall back to
/// wsx scrollback). `col`/`row` are 1-based, pane-relative cell coordinates.
pub fn wheel_report_bytes(&self, up: bool, col: u16, row: u16) -> Option<Vec<u8>>
```

- Locks `self.parser`, reads `screen().mouse_protocol_mode()`. `None` → return
  `None`.
- Reads `screen().mouse_protocol_encoding()` to choose the byte format:
  - **Sgr**: `ESC [ < Cb ; col ; row M`, where `Cb` = 64 (wheel up) or 65 (wheel
    down). (`M` = press; wheel events are press-only.)
  - **Default / Utf8 (X10)**: `ESC [ M` followed by three bytes `32 + Cb`,
    `32 + col`, `32 + row`, with `Cb` = 64 / 65. Clamp `col`/`row` to 223 so the
    byte stays in range. (Utf8 is treated as X10 here; agents in practice request
    SGR.)
- No modifier bits are set — forwarding only happens for the plain (non-Shift)
  wheel, so the forwarded event is always unmodified.
- Keeps every `vt100` type inside `Session`; the caller in `input.rs` deals only
  in `Option<Vec<u8>>`.

### 2. Attached pane-rect capture

The attached view does not currently store pane geometry (unlike the dashboard's
`detail_container_rects`). Add an `App` field recording, per visible pane, its
session handle and on-screen content rect:

```rust
pub attached_pane_rects: Vec<(std::sync::Arc<crate::pty::session::Session>, ratatui::layout::Rect)>,
```

- Storing the `Arc<Session>` directly (rather than a `WorkspaceId`) sidesteps the
  fact that `View::AttachedPm`'s pane is the PM session, which lives in `app.pm`
  rather than `app.sessions` — workspace panes and the PM pane are recorded
  uniformly with no sentinel id or enum.
- Cleared at the top of every frame in `draw()` (alongside `chip_rects` /
  `detail_container_rects`).
- Populated in the `View::Attached` / `View::AttachedPm` render path where each
  pane's `term_area` is already computed (`src/ui/attached.rs`). The recorded
  rect is the pane's terminal content area (the region `render_screen` draws
  into), so cursor-to-cell translation is exact.

### 3. `handle_mouse` routing (`src/app/input.rs`)

`scroll_active` is left untouched (it keeps targeting the focused session's
scrollback). The only new branch sits in front of the existing scroll arms: for
a plain (non-Shift) `ScrollUp` / `ScrollDown` over a pane whose agent has mouse
reporting on, forward instead of scrolling.

```text
if scroll kind and attached view:
    if NOT Shift:
        if Some((session, rect)) = pane under (col, row) in attached_pane_rects:
            (rel_col, rel_row) = 1-based coords relative to rect
            if Some(bytes) = session.wheel_report_bytes(up, rel_col, rel_row):
                session.writer.send(bytes)        // forward to agent
                return
    // Shift held, OR no pane under cursor, OR mouse mode off:
    fall through to the existing scroll_active arm (wsx scrollback, focused)
```

- **Shift+wheel** never enters the forward branch → always falls through to
  `scroll_active` → wsx scrollback. This is the escape hatch.
- **Plain wheel, mouse mode off** → `wheel_report_bytes` returns `None` → falls
  through to `scroll_active`. Unchanged from today.
- **Plain wheel over chrome** (chip/footer/status rows, no pane underneath) →
  falls through to `scroll_active` on the focused session. Unchanged from today.
- Only **plain wheel over a mouse-aware pane** changes behavior: it forwards.

## Data flow

```
crossterm MouseEvent (absolute col/row, modifiers)
  -> handle_mouse (attached view, scroll kind)
     -> NOT Shift AND pane under cursor?
          yes -> session.wheel_report_bytes(up, rel_col, rel_row)
                   Some(bytes) -> session.writer.send(bytes) -> PTY -> agent scrolls -> return
                   None        -> (fall through)
          no  -> (fall through)
     -> scroll_active (focused session scrollback offset++)
render: parser.set_scrollback(offset) -> screen() -> drawn
```

## Error handling / edge cases

- **Writer send failure**: ignored with `let _ =`, matching every other
  `session.writer.send` call site.
- **Cursor outside all panes**: fall back to the focused session (no-op if there
  is no focused session, exactly as `active_session` already handles).
- **Mouse mode toggles mid-session**: read fresh from the parser on every event,
  so behavior tracks the agent's current mode with no caching.
- **Alt screen + Shift+wheel**: scrollback is empty; the hatch is a no-op there
  by nature (documented, not an error).

## Testing

Unit tests in `src/pty/session.rs` (using `spawn_for_test`, feeding DECSET
sequences into the parser):

- `wheel_report_bytes` returns `None` when no mouse mode is set.
- Returns SGR bytes `ESC[<64;C;RM` / `ESC[<65;C;RM` after `ESC[?1006h` +
  `ESC[?1000h`, for up/down, with given coords.
- Returns X10 bytes `ESC[M` + offset triplet after `ESC[?1000h` only (default
  encoding), with 223 clamping.

Tests in `src/app/input_tests.rs` (constructing `View::Attached` / `View::AttachedPm`
with a session whose parser has mouse mode enabled). Forwarding has no directly
observable effect in-test: the session writer's receiver is owned by the PTY pump
task and can't be drained without a dedicated test hook, so forwarding is asserted
*indirectly* via the differential effect on the wsx scrollback offset. `scroll_active`
is the only path that moves the offset, so an unchanged offset proves the event was
forwarded rather than scrolled locally.

- Plain wheel + mouse mode ON → scrollback offset unchanged (forwarded).
- Plain wheel + mouse mode OFF → scrollback offset increases by 3 (fell through).
- Shift+wheel + mouse mode ON → scrollback offset increases by 3 (escape hatch).
- Plain wheel-down + mouse mode ON, pre-scrolled to offset 5 → offset stays 5 (a
  fall-through scroll-down would drop it to 2, so this proves forwarding for the
  down direction too).
- Plain wheel over chrome (no pane under cursor) → scrollback offset increases by 3.
- Plain wheel + mouse mode ON in `View::AttachedPm` → PM session offset unchanged.

## Commit plan

1. `feat(pty): Session::wheel_report_bytes mouse encoder` + unit tests.
2. `feat(attached): capture pane rects for wheel hit-testing`.
3. `feat(input): forward wheel to mouse-aware agent; Shift+wheel scrollback hatch`
   + integration tests.
