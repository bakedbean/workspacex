# Workspace Updates Panel Design

**Issue:** [bakedbean/workspacex#11](https://github.com/bakedbean/workspacex/issues/11)
**Date:** 2026-05-14

## Goal

When the user is attached to a workspace (full-screen claude session) or
the project manager (`View::AttachedPm`), surface activity from OTHER
workspaces without forcing them to detach back to the dashboard.

Two affordances:

1. A conditional status row at the bottom of the attached view that
   shows one line summarizing the most attention-worthy update from
   another workspace.
2. A floating updates panel (`Ctrl-a u`) that lists all workspaces with
   their current state and most recent activity, grouped by repo.

## Non-goals (v1)

- A new background poller. All required data is already collected by
  the existing `events::tail_workspace_events` (2s poll) and the
  activity classifier in `draw`. This feature is pure rendering +
  modal lifecycle.
- Showing the updates panel from the dashboard view. The dashboard
  already lists every workspace with state, activity, sub-line, and
  attention markers; a floating panel there would be redundant.
- Soft/non-blocking modal. The panel uses the existing hard-modal
  pattern (keystrokes go to the modal handler; claude is briefly
  paused-from-keystrokes while the panel is up).
- Symmetric toggle on `Ctrl-a u`. Open with the chord, dismiss with
  Esc (matching all existing modals).

## Architecture

### New module

`src/ui/updates_bar.rs` — pure function over slices of `App` state.
Kept pure (no `&App` dependency) so it's trivially testable in isolation:

```rust
pub struct UpdatesRow {
    pub glyph: char,          // '⚠' or '●'
    pub kind: UpdatesRowKind,
    pub text: String,         // rendered status-summary text including age
}

pub enum UpdatesRowKind {
    Attention,
    Activity,
}

pub struct WorkspaceUpdateInfo<'a> {
    pub id: WorkspaceId,
    pub name: &'a str,
    pub events: Option<&'a WorkspaceEvents>,
    pub activity: ActivityState,
    pub needs_attention: bool,
    pub awaiting_tool: Option<(String, i64)>,  // pre-computed by caller via App::awaiting_permission
}

pub fn select_row(
    attached_workspace: Option<WorkspaceId>,
    candidates: &[WorkspaceUpdateInfo],
    now_ms: i64,
) -> Option<UpdatesRow>;
```

Returns `None` when nothing should be shown (status row collapses to 0
rows). Returns `Some(UpdatesRow { ... })` when there's something to
surface. The function does NOT render — it picks content. `attached.rs`
builds the `candidates` slice from `App` state (it already has access)
and calls `select_row`, then renders.

The caller pre-computes `awaiting_tool` via `App::awaiting_permission`
so `updates_bar` doesn't need to know about `App`. This keeps the
module a leaf with zero non-test dependencies on `App`.

### Modal variant

`src/ui/modal.rs` gains:

```rust
pub enum Modal {
    NewWorkspace { ... },
    ConfirmArchive { ... },
    SetupRunning { ... },
    Error { ... },
    UpdatesPanel,   // no payload — reads live App state at render time
}
```

The existing `pub fn render(f, area, &Modal, &Theme)` matches `Modal::UpdatesPanel`
and delegates to a new function so the borrow surface is clean:

```rust
pub fn render_updates_panel(
    f: &mut Frame,
    area: Rect,
    repos: &[Repo],
    workspaces: &[(RepoId, Workspace)],
    events: &HashMap<WorkspaceId, WorkspaceEvents>,
    activity: &HashMap<WorkspaceId, ActivityState>,
    needs_attention: &HashSet<WorkspaceId>,
    theme: &Theme,
);
```

The `Modal::render` arm for `UpdatesPanel` is a no-op (or routes to a
"this shouldn't happen — call render_updates_panel" panic guard).
`draw()` in `app.rs` chooses the right function based on the variant.

### Render integration

In `app.rs::draw`, the existing modal block changes from:

```rust
if let Some(m) = &app.modal {
    modal::render(f, area, m, &app.theme);
}
```

to:

```rust
if let Some(m) = &app.modal {
    match m {
        Modal::UpdatesPanel => {
            modal::render_updates_panel(
                f, area,
                &app.repos,
                &app.workspaces,
                &app.workspace_events,
                &app.workspace_activity,
                &app.workspace_needs_attention,
                &app.theme,
            );
        }
        other => modal::render(f, area, other, &app.theme),
    }
}
```

## Status row (attached + attached-pm)

The existing `attached::render` uses:

```rust
Layout::default()
    .direction(Direction::Vertical)
    .constraints([Constraint::Min(1), Constraint::Length(1)])
    .split(area);
```

Becomes:

```rust
let updates_row = updates_bar::select_row(...);
let status_height = if updates_row.is_some() && app.modal.is_none() { 1 } else { 0 };
Layout::default()
    .direction(Direction::Vertical)
    .constraints([
        Constraint::Min(1),                  // PTY term
        Constraint::Length(status_height),   // optional status row
        Constraint::Length(1),               // existing footer
    ])
    .split(area);
```

When `status_height == 0`, the layout still has three chunks but the
middle one has zero height and the PTY gets that row.

**Suppress while panel is open:** if `app.modal == Some(Modal::UpdatesPanel)`,
the status row is hidden (the panel already surfaces all the info, the
row would be redundant and visually noisy under the modal). Other
modal variants don't suppress.

### Status row content selection

`updates_bar::select_row` returns:

1. **Attention priority:** Among candidates with `needs_attention == true`
   (excluding `attached_workspace`), pick the one with the most recent
   activity (by `events.latest.timestamp_ms` if present, falling back
   to insertion order). Return:
   - `glyph: '⚠'`
   - `kind: UpdatesRowKind::Attention`
   - `text: format!("{name} {state-summary} ({age})")`
   - Where `state-summary` is:
     - `awaiting permission: <tool>` if `awaiting_tool.is_some()`
     - `waiting` otherwise
   - Where `age` is the human time since the event/transition derived from
     `now_ms - awaiting_tool.1` (or the latest event's timestamp if there's
     no pending tool_use), formatted as `<n>s` for <60, `<n>m` for <60m,
     `<n>h` otherwise.

2. **Recent event fallback:** Among OTHER workspaces with a `latest` event
   newer than 60 seconds, pick the most recent. Return:
   - Glyph: `●`
   - Text: `<name>: <event-text> (<age>)`
   - Where `event-text` is the summarized line from `WorkspaceEvents::latest`
   (e.g., ``ran `cargo test` ``, `using Read`, `<assistant text>`).
   - Truncated to fit terminal width minus glyph+name+age overhead.

3. **Nothing to show:** Return `None`.

### Status row rendering

`updates_bar::render(f, area, row, theme)`:
- Renders the glyph in the appropriate theme color (warn for `⚠`, ok for `●`).
- Renders the rest of the text in `theme.dim_style()`.
- Truncates with ellipsis if the rendered width exceeds `area.width`.

### Footer hint update

The existing footer text becomes:

```
 <label>   [Ctrl-a d] detach   [Ctrl-a u] updates   [Ctrl-a a] send Ctrl-a
```

The hint is always shown; the chord works regardless of whether the
status row is currently visible.

## Floating updates panel

### Modal opening

In `handle_key_attached` and `handle_key_attached_pm`, the existing
Ctrl-a chord handler gains a new arm:

```rust
if app.ctrl_a_pending {
    app.ctrl_a_pending = false;
    match k.code {
        KeyCode::Char('d') => { /* detach */ }
        KeyCode::Char('a') => { /* send Ctrl-a */ }
        KeyCode::Char('u') => {
            app.modal = Some(Modal::UpdatesPanel);
            return Ok(());
        }
        _ => return Ok(()),
    }
}
```

### Modal dismissal

`handle_key_modal` gains an arm:

```rust
Modal::UpdatesPanel => match k.code {
    KeyCode::Esc => app.modal = None,
    _ => {} // swallow other keys
}
```

Other modal variants are unchanged.

### Panel layout

Centered window sized as a fraction of the screen:
- Width: `min(area.width.saturating_sub(4), 80)` (target ~80 columns
  or whatever fits with 2-column margins).
- Height: `min(area.height.saturating_sub(4), 25)` (target ~25 rows or
  whatever fits with 2-row margins).

Block with `Borders::ALL`, title `" Workspace updates "`, dim border style.

### Panel content

Body is built from the live `App` state passed in:

1. Group workspaces by repo, preserving repo display order.
2. For each repo, emit a header line: the repo name in `theme.header_style()`.
3. For each workspace in that repo, emit a row:
   - Glyph (one of `⚠ ● ↻ ○ ✕`):
     - `⚠` if in `needs_attention`
     - `●` if activity state is `Active` or `Idle`
     - `↻` if has a prior session but isn't running
     - `○` if no session ever
     - `✕` if state == `Failed`
   - Workspace name (padded to align)
   - Status text:
     - If `⚠`: `awaiting permission: <tool>` or `waiting`
     - If `●`: latest event summary
     - If `↻`: `resumable`
     - If `○`: `no session`
     - If `✕`: `failed`
   - Age in parens if applicable (e.g., `(12s)`)
4. Sort within each repo:
   - `needs_attention` workspaces first (most recent age first)
   - Then active/idle workspaces (most recent activity first)
   - Then resumable, then off, then failed.
5. Empty repos (no workspaces) appear with their header and a single
   dim "(no workspaces)" line.

Footer line inside the block: `[esc] close` in `theme.dim_style()`.

### Live updates

The panel re-renders on every render tick alongside the rest of the
UI (`draw()` runs at every event including `Tick`). Ages count up,
attention flags appear/clear in real time as the underlying state
changes. No special invalidation needed.

## Edge cases

- **Attached to a workspace that no longer exists in `App.workspaces`**:
  the status row simply excludes "self" by ID lookup — if the ID isn't
  in the list, no exclusion happens (all other workspaces remain
  candidates). Not a regression.
- **No workspaces at all (fresh install)**: `select_row` returns `None`;
  panel shows just the repo headers (or `(no repos)` if there are also
  no repos).
- **Modal opens during attached view but PM session is the focus
  context (impossible in current code paths but defensive)**: panel
  renders normally; modal dismissal returns to the attached view via
  the existing modal-clear path.
- **Status row content longer than terminal width**: truncate the
  middle (`<name>: <event-text-truncated-with-ellipsis> (<age>)`) so
  the age is always visible.

## Tests

All run with `--test-threads=1` per existing convention.

1. **`updates_bar::select_row` returns None when no other workspaces have
   activity or attention.** Empty events + empty attention + currently
   attached to the only workspace → `None`.

2. **Attention priority over events.** Set up: workspace A is in
   `needs_attention`, workspace B has a recent event newer than A's.
   Attached to neither. Expect `select_row` to return A's row with the
   `⚠` glyph.

3. **Most recent event wins when no attention.** Two workspaces with
   events at different timestamps; no attention. Expect the newest-event
   workspace's row.

4. **Currently-attached workspace is excluded.** Workspace A is in
   `needs_attention` and is the attached one. Expect either `None` (if
   no others) or the next-best workspace's row.

5. **Events older than 60s are ignored.** Workspace B has an event 90s
   old. Expect `None` (B is too stale to surface).

6. **`Ctrl-a u` opens the panel from attached view.** Send Ctrl-a then
   `u` to `handle_key_attached`. Expect `app.modal == Some(Modal::UpdatesPanel)`.

7. **`Ctrl-a u` opens the panel from attached-pm view.** Same test but
   via `handle_key_attached_pm`.

8. **Esc closes the panel.** Set `app.modal = Some(Modal::UpdatesPanel)`,
   call `handle_key_modal` with Esc. Expect `app.modal == None`.

9. **Other keys are swallowed by the panel modal.** Set modal, send
   `KeyCode::Char('q')`. Expect `app.modal` is still `Some(UpdatesPanel)`
   AND `app.quit == false` (the `q` shouldn't reach the dashboard quit
   path).

10. **Panel renders all repos with their workspaces, grouped.** Fixture
    with two repos and three workspaces total; render to `TestBackend`;
    assert each repo name and workspace name appears in the buffer.

11. **Status row layout: present when content available.** Render
    attached view with one OTHER workspace that has a recent event;
    assert the rendered buffer has the event text on the second-to-last
    row (above the footer).

12. **Status row layout: absent when no content.** Render attached view
    with no other activity; assert the rendered buffer's last row IS
    the footer (no blank/empty row above it).

## README changes

### Attached workspace keybindings table

Add one row:

```
| `Ctrl-a u` | Open the floating updates panel (shows other workspaces' state) |
```

### New section after "Dashboard status indicators"

```
## Workspace updates panel

When you're attached to a workspace (full-screen claude session) or the
project manager pane is expanded full-screen, wsx still tracks the other
workspaces in the background. Two affordances surface that:

- A single-row status indicator above the footer, shown only when another
  workspace needs attention or has produced output in the last 60 seconds.
  Format: `⚠ <name> awaiting permission: <tool> (<age>)` for attention,
  `● <name>: <event> (<age>)` for activity. The row collapses to nothing
  when there's nothing to surface, giving claude the row back.

- A floating panel via `Ctrl-a u` listing ALL workspaces grouped by repo,
  with their current state and latest event. Press `Esc` to close. The
  panel re-renders live, so ages count up and attention flags appear/clear
  in real time.
```
