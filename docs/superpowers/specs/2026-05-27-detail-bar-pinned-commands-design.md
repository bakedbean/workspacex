# Detail bar: pinned commands for the inline reply

## Problem

When a workspace is selected on the dashboard, the detail bar surfaces a
1-line inline reply input (`┃ Reply to agent ┃ <draft>`) that lets the user
send a message to the agent without attaching to the workspace. Today, the
only way to dispatch a *pinned command* (a slash-command shortcut like
`/pull-request` or `/feedback`) is to attach to the workspace and use the
chip row above the attached-view footer.

The user wants pinned commands available from the dashboard's detail bar
too, with the same behavior as the attached view: clickable chips, plus a
keyboard shortcut.

## Goals

- A 1-row chip strip appears between the detail bar's body and its reply
  input row, rendered identically to the attached view's chip row.
- Clicking any chip dispatches its command to the selected workspace's PTY
  (suffixed with `\r`), whether or not the reply input is focused.
- Pressing `Ctrl-X` followed by `1`..`9` dispatches the corresponding
  chip's command, whether or not the reply input is focused. Pressing
  `Ctrl-X` followed by any other key clears the leader without firing.
- The chip row appears only when pinned commands are configured. With no
  pinned config, the detail bar looks exactly as it does today.
- Per-repo `repos.pinned_commands` continues to override the global
  `pinned_commands` setting (existing `pinned::resolve` semantics — no
  new resolution rules).

## Non-goals

- **PM pane chip activation.** `View::AttachedPm` already passes an empty
  pinned slice ("PM pane is out of scope for pinned commands per spec").
  No change here.
- **A new `DetailBarConfig` toggle** for enabling chips. The chip row is
  driven by the presence of `pinned_commands` config, same as the attached
  view; users without pinned commands see the same detail bar they see
  today.
- **Chip styling divergence.** The detail-bar chip row reuses
  `attached::render_chip_row` verbatim. If the attached view's chips evolve
  (hover state, theming), the detail bar tracks along.
- **Advertising the chord in the dashboard footer.** The chip row's visible
  number prefixes plus parity with the attached view are enough for v1.
  Easy follow-up if it proves confusing.
- **Streaming the draft alongside the chip command.** Per design, firing a
  chip discards any in-flight draft (matches the attached view's
  "commands are atomic" semantics).

## Approach

Reuse the attached view's chip layout/render functions verbatim. Wire the
chip row into the detail bar layout when pinned is non-empty, and reuse
the existing `app.chip_rects` / `app.pinned_commands_cache` plumbing so
the mouse handler at `src/app/input.rs:1238` works without modification.
Add a `Ctrl-X` leader handler on the dashboard view paralleling the one
that already exists on the attached view (`src/app/input.rs:740-743`).

### Layout

The detail bar's row stack is conditional on pinned:

- Empty pinned (today's layout, 5 rows):
  - header · rule · body · rule · reply
- Non-empty pinned (6 rows):
  - header · rule · body · rule · **chips** · reply

The new chip row consumes a 1-row slot inserted between the bottom rule
and the reply input row. The chip row carries chips + the existing
trailing `─` filler (so the bottom rule above + the filler form a visual
cap on the body). The reply input row sits flush below the chip row with
no separator.

The chip slot is added as a new top-level layout constraint
(`Constraint::Length(1)`) alongside the existing header / rule / body /
rule / reply constraints. Because the body's constraint is
`Constraint::Min(1)`, ratatui absorbs the extra row from the body
allotment when total height is constrained — the body shrinks by 1
when chips render. `DetailBarConfig::minimum_height()` is **not**
changed; when the bar can't fit at very small heights, the existing
height-gate (`area.height < inputs.config.minimum_height()`) already
elides the entire bar.

### Shared chip renderer

`attached::layout_chip_row` is already `pub`. We raise
`attached::render_chip_row` from private to `pub(crate)` so
`dashboard::detail::render` can call it directly. No signature change,
no behavior change.

### Pinned resolution

In `src/app/render.rs::draw`'s dashboard branch, just before constructing
`DetailInputs`, resolve `pinned` for the selected workspace's repo the
same way the attached branch already does at lines 354-365:

```rust
let global_pinned = app.store.get_setting("pinned_commands").ok().flatten();
let repo_pinned = /* selected workspace's repo's pinned_commands */;
let pinned = crate::pinned::resolve(global_pinned.as_deref(), repo_pinned.as_deref());
```

Pass the slice into `DetailInputs`. After `detail::render(...)` returns,
stash its returned chip rects + the resolved `pinned` into
`app.chip_rects` and `app.pinned_commands_cache` so the existing mouse
handler and the new keyboard chord can find them.

The frame-top clear at `render.rs:16-17` is preserved: caches reset every
frame and only repopulate when a chip row actually rendered. If the
config changed between frames and produced an empty list, the caches stay
empty and the next click / chord is a no-op.

### Input wiring

**Mouse.** The existing chip branch in `handle_mouse` at
`src/app/input.rs:1238-1257` already iterates `app.chip_rects` and looks
up `app.pinned_commands_cache[idx]`. Its only problem: it writes to
`active_session(app)`, which returns `None` on `View::Dashboard`. We add
a small helper:

```rust
fn chip_target_session(app: &App) -> Option<Arc<Session>> {
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

The mouse chip branch swaps `active_session` for `chip_target_session`.
Other mouse paths (scroll, paste) are unchanged.

**Keyboard.** Reuse the existing `app.leader_pending` flag. The attached
view's arming code at `src/app/input.rs:740-743` already sets the flag
on `Ctrl-X`; we add a parallel arming branch in the dashboard handler.

Two arming sites are needed because the dashboard reply-input handler
(`handle_detail_bar_reply_key`) intercepts keystrokes before they reach
the dashboard dispatcher when `PaneFocus::DetailBarReply` is active:

1. **In `handle_detail_bar_reply_key`** (top of function): on
   `(Ctrl-X, CONTROL)`, set `leader_pending = true` and return `true`
   (consumed). On any key while `leader_pending` is already set, return
   `false` (yield to dashboard dispatcher so the chord completes).
2. **In the main dashboard key dispatcher**, before the `match (k.code, k.modifiers)`:
   - If `k.code == LEADER_KEY && CONTROL`, set `leader_pending = true` and
     return.
   - Else if `leader_pending` is set, clear it. If the key is `Char('1'..='9')`
     and the digit indexes a valid entry in `chip_rects`, fire the chip; on
     any other follow-up, just swallow it.

This ordering means a `Ctrl-X` typed while the reply input is focused
arms the leader without inserting `^X` into the draft; the *next* key
finishes the chord.

**Firing a chip.** Extract a small helper `fire_chip(app, idx)` shared
by the mouse and keyboard paths:

```rust
async fn fire_chip(app: &mut App, idx: usize) {
    if idx >= app.chip_rects.len() {
        return; // requested chip is not visible (clamped by width)
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

The `idx >= chip_rects.len()` guard enforces "keyboard fires only
visible chips" — matching the mouse, which can only click rects that
were laid out.

**View-change safety.** `leader_pending` is reset on every view
transition between dashboard and attached (cheap; covered by an input
test) so an armed Ctrl-X doesn't leak across views.

### Components & file-level changes

`src/ui/attached.rs`
: Promote `render_chip_row` from private to `pub(crate)`. No behavior change.

`src/ui/dashboard/detail.rs`
: Add `pub pinned: &'a [crate::pinned::PinnedCommand]` to `DetailInputs`.
: Change `render`'s return type from `()` to `Vec<ratatui::layout::Rect>`.
: Insert a `Constraint::Length(1)` chip slot into the layout iff
  `!inputs.pinned.is_empty()`; render it via `attached::render_chip_row`
  and return the rects. The reply-row cursor positioning keys off the
  reply chunk's `y`, so it stays correct under both layouts.

`src/app/render.rs`
: Dashboard branch: resolve `pinned` for the selected workspace's repo,
  pass to `DetailInputs`, capture the returned `Vec<Rect>`, write into
  `app.chip_rects` and `app.pinned_commands_cache`.

`src/app/input.rs`
: New `chip_target_session` helper.
: New `fire_chip` helper.
: `handle_mouse` chip branch swaps `active_session` for `chip_target_session`
  and calls `fire_chip`.
: New `Ctrl-X` arming + chord completion at the top of the dashboard key
  dispatcher.
: `handle_detail_bar_reply_key` gains a top-of-function short-circuit
  for `Ctrl-X` (arm) and for any key while armed (yield).
: View transitions clear `leader_pending`.

No changes:
- `src/pinned.rs` — already does what we need.
- `src/app.rs` — `chip_rects` and `pinned_commands_cache` fields already exist.
- `src/detail_bar_config.rs` — `minimum_height` is unchanged.

## Data flow

1. `draw()` clears `chip_rects` and `pinned_commands_cache` at frame start.
2. Dashboard branch resolves `pinned` for the selected workspace.
3. `detail::render`:
   - Empty pinned → 5-row layout, returns `Vec::new()`.
   - Non-empty → 6-row layout; calls `attached::render_chip_row` for the
     chip slot; returns its rects.
4. Caller stashes `chip_rects` + `pinned_commands_cache` into `app`.

Activation (mouse):
1. `handle_mouse(Down(Left))` finds the chip index whose rect contains the click.
2. Calls `fire_chip(app, idx)`.

Activation (keyboard):
1. User presses `Ctrl-X` (anywhere on dashboard view, reply focused or not).
   `leader_pending = true`.
2. Next key:
   - `1..9` → `fire_chip(app, idx)` and clear leader.
   - anything else → clear leader, no-op.

## Error handling

- **No workspace selected when a chip fires.** `chip_target_session` returns
  `None`; the dispatch is a no-op. (Shouldn't happen since chips only render
  when a workspace is selected, but defensive.)
- **Selection changed between draw and chip fire.** `chip_target_session`
  resolves at fire time against the current selection, so the chip
  dispatches to whichever workspace is selected now. Matches user
  intuition.
- **Reply input focused at fire time.** Draft is cleared and focus
  returns to dashboard (consistent with Esc behavior in
  `handle_detail_bar_reply_key`).
- **Pinned config becomes empty between frames.** Next draw renders no
  chip row; caches stay empty; mouse / chord are no-ops.
- **Detail bar too short to render.** `detail::render` early-returns on
  the existing height gate; chip caches don't populate; mouse / chord
  no-op.
- **Terminal narrow enough that some chips don't fit.** `layout_chip_row`
  already drops chips from the end. `fire_chip` guards `idx <
  chip_rects.len()` so requesting a clipped-off chip via the chord is a
  no-op.
- **Ctrl-X then Tab / Esc / arrow.** Leader clears, no chip fires.
- **View transition while armed.** `leader_pending` is reset on dashboard
  ↔ attached transitions.

## Testing strategy

`src/ui/dashboard/detail.rs` unit tests:

- Empty pinned → layout still 5 rows; existing tests unaffected.
- Non-empty pinned → chip row rendered between body's bottom rule and
  reply; chip rects returned with correct `y`.
- Narrow width drops trailing chips (mirrors
  `chip_row_drops_trailing_chips_when_too_narrow` in attached.rs).
- Empty-pinned baseline: rendered text identical to today's
  `full_render_paints_header_body_and_reply_row`.

`src/app/input_tests.rs` integration tests:

- From `View::Dashboard` with a workspace selected: `Ctrl-X` then `'1'`
  fires `pinned_commands_cache[0]` to that workspace's session writer.
- Same chord while `PaneFocus::DetailBarReply`: clears the draft, fires
  the chip, returns to dashboard focus.
- Mouse click on `app.chip_rects[1]` fires `pinned_commands_cache[1]`
  to the selected workspace's session.
- `Ctrl-X` then `'a'` clears leader without firing.
- `Ctrl-X` then `'4'` when only 3 chips visible is a no-op.
- Dashboard → Attached view transition with armed leader: leader clears.

## Acceptance criteria

1. With `pinned_commands` configured globally (or on the selected
   workspace's repo), the workspace detail bar shows a 1-row chip strip
   above the reply input, styled identically to the attached view.
2. Without pinned commands, the detail bar looks exactly as it does
   today (no extra row, no extra chrome).
3. Clicking any chip dispatches its command to the selected workspace's
   PTY (suffixed with `\r`), regardless of whether the reply input is
   focused.
4. Pressing `Ctrl-X` followed by `1..9` from the dashboard view
   dispatches the corresponding chip's command to the selected
   workspace's PTY. The chord works whether reply input is focused or
   not. Reply draft is cleared on fire.
5. The chord doesn't fire when no workspace is selected, when the bar
   isn't visible, when the digit exceeds the number of *visible* chips,
   or after view transitions clear the leader.
6. Per-repo pinned override takes precedence over the global setting
   (existing `pinned::resolve` semantics).
