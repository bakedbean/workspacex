# Filtering workspaces in the updates panel

## Problem

The dashboard has a `/` filter that narrows the workspace list to a
case-insensitive substring match. The agent updates panel (`^x u`) lists the
same workspaces, grouped by repo, but has no way to narrow them — on a machine
with a few dozen workspaces across many repos the panel is a long scroll even
though the user usually knows the name of the one they want.

Add the dashboard's filter to the updates panel.

A secondary gap surfaced while reading the dashboard code: the dashboard never
echoes the needle the user typed. `DashboardState::filter` is read only by
`matches_filter` (`src/ui/dashboard/mod.rs:306`, `:503`, `:593`); nothing renders
it. Rows disappear as you type with no visible indication of why. The PM pane
does echo its needle (`src/ui/pm_pane.rs:190`). This spec fixes the dashboard
echo too, since "the same way as the dashboard" would otherwise inherit the bug.

## Current shape

`ordered_workspaces_for_panel` (`src/ui/modal/updates_panel.rs:104`) computes the
ordered list of workspace IDs the panel shows: grouped by repo in App's repo
order, sorted within each repo by the active `UpdatesSort`, tie-broken by
`(attention, failed, activity_rank, recency)`.

Both consumers index into that one list:

- `render_updates_panel` builds `pos_of: WorkspaceId -> index`, uses it to gate
  which rows draw and which repo headers appear, and to decide which row is
  selected.
- The `Modal::UpdatesPanel` key handler (`src/app/input.rs:1614`) rebuilds the
  same order to map `selected` back to a workspace id for Enter / `v` / `s`.

That makes `ordered_workspaces_for_panel` the single place a filter needs to
hook in: dropping an id from it hides the row *and* the repo header (the
renderer already `continue`s past repos whose workspaces are all absent) *and*
keeps the key handler's indices aligned with what is drawn.

## Design

### State

`Modal::UpdatesPanel` gains a third field:

```rust
UpdatesPanel {
    selected: usize,
    sort: UpdatesSort,
    /// `None` = not filtering; `Some(buf)` = filter mode active. Echoed even
    /// while empty so the `/` press has visible feedback before any typing.
    /// Not persisted — reset on every open, like `sort`.
    filter: Option<String>,
}
```

`Some("")` and `None` are distinct: the former means the user pressed `/` and
the panel is in filter-input mode with an empty needle (all rows still visible);
the latter means normal key handling. This mirrors `DashboardState::filter`.

### Keys

While `filter.is_some()`, printable chars are filter text rather than shortcuts,
exactly as the dashboard does at `src/app/input.rs:525`:

| key | `filter.is_none()` | `filter.is_some()` |
| --- | --- | --- |
| `/` | enter filter mode (`Some(String::new())`) | appends `/` to the buffer |
| printable char | `j`/`k` move, `o` sort, `l` switch, `v`/`s` split | appends to the buffer |
| Backspace | inert | pops a char |
| `↑` / `↓` | move | move |
| `Enter` | switch | switch |
| `Esc` | close the panel | clear the filter (`None`); panel stays open |

Two-stage Esc: the first press drops the filter, the second closes the panel.

Chars carrying CONTROL or ALT are not filter text — they fall through to the
existing handlers, matching the dashboard's guard.

### Matching

New in `updates_panel.rs`:

```rust
fn matches_filter(
    w: &Workspace,
    repo_name: &str,
    status_text: &str,
    needle: &str,
) -> bool
```

Case-insensitive substring against three fields:

1. the workspace name (the panel's name column — the dashboard's analogue is the
   branch, which is what its name column shows),
2. the owning repo's name, so typing a repo name keeps all of its workspaces,
3. the row's live status text — `awaiting permission: Bash`, `question`,
   `complete`, `stalled`, `waiting`, `failed`, `resumable`, `no session`, or the
   latest activity line.

An empty needle matches everything.

The status text is currently computed inline inside `workspace_row`
(`src/ui/modal/updates_panel.rs:368`), which the filter cannot reach. Extract
it:

```rust
fn row_status_text(
    w: &Workspace,
    events: Option<&WorkspaceEvents>,
    activity: Option<ActivityState>,
    needs_attention: bool,
    awaiting: Option<&(String, i64)>,
) -> (String, Option<i64>)   // (text, age anchor)
```

`workspace_row` calls it for rendering; `matches_filter`'s caller calls it for
matching. One source, so the two cannot drift — a row can never display text
that the filter fails to match.

### Where the filter applies

`ordered_workspaces_for_panel` applies the needle while collecting each repo's
workspaces, before sorting. It needs two things it does not have today: the
needle, and the `awaiting` map (an input to `row_status_text`).

`awaiting` is built today only in `render.rs:680`; the key handler does not have
it. Add `App::awaiting_permission_map()` returning
`HashMap<WorkspaceId, (String, i64)>` so both call sites build it identically,
and have `render.rs` use it in place of its inline loop.

### Signatures

`ordered_workspaces_for_panel` is already at 8 args under
`#[allow(clippy::too_many_arguments)]`; the needle and `awaiting` push it to 10.
`render_updates_panel` is at 14 under the same allow.

Bundle the borrowed inputs both share into one struct, the same shape
`DashboardInputs` (`src/ui/dashboard/mod.rs`) already has on the dashboard side:

```rust
pub struct PanelInputs<'a> {
    pub repos: &'a [Repo],
    pub workspaces: &'a [(RepoId, Workspace)],
    pub events: &'a HashMap<WorkspaceId, WorkspaceEvents>,
    pub activity: &'a HashMap<WorkspaceId, ActivityState>,
    pub needs_attention: &'a HashSet<WorkspaceId>,
    pub awaiting: &'a HashMap<WorkspaceId, (String, i64)>,
    pub statuses: &'a HashMap<WorkspaceId, Status>,
    pub lifecycles: &'a HashMap<WorkspaceId, BranchLifecycle>,
}
```

Both functions drop to roughly four parameters and both
`#[allow(clippy::too_many_arguments)]` come off:

```rust
pub fn ordered_workspaces_for_panel(
    inputs: &PanelInputs<'_>,
    sort: UpdatesSort,
    filter: Option<&str>,
) -> Vec<WorkspaceId>

pub fn render_updates_panel(
    f: &mut Frame,
    area: Rect,
    inputs: &PanelInputs<'_>,
    selected: usize,
    now_ms: i64,
    sort: UpdatesSort,
    filter: Option<&str>,
    theme: &Theme,
)
```

`workspace_row` keeps its own parameter list — it is per-row, not per-panel, and
takes computed values (`is_selected`, `name_col`, `row_width`) rather than the
shared maps.

### Selection

The selection follows its workspace rather than its index. After every filter
edit (and after Esc clears the filter), recompute the order with the new needle,
find the previously selected workspace id in it, and use that position; if the
filter hid it, clamp `selected` into the new range (`min(selected, len - 1)`, or
0 when the list is empty). This is the trick the `o` sort handler already uses
at `src/app/input.rs:1659`.

### Rendering

`render_updates_panel` passes the needle to `ordered_workspaces_for_panel`, and
the existing `pos_of` gating does the rest: filtered-out rows do not draw, and
repos whose workspaces all filtered out lose their headers via the existing
`if ws_for_repo.is_empty() { continue; }`.

The empty state distinguishes the two causes, matching the PM pane
(`src/ui/pm_pane.rs:172`):

- no workspaces at all → `(no workspaces)` (unchanged)
- non-empty filter matched nothing → `(no matching workspaces)`

### Footer

The footer must fit the panel's inner width: the panel is capped at 80 columns
and the border takes 2, so 78.

Not filtering (77 chars with the longest sort label, `default`):

```
[↑↓] move  [↵] switch  [v/s] split  [o] sort:default  [/] filter  [esc] close
```

The current line is 72 chars using `[↑/↓]` and `[enter/l]`. Adding a `[/] filter`
chip needs 12 more, which overflows; switching to the `↑↓` and `↵` glyphs the
dashboard footer already uses (`src/ui/dashboard/layout.rs:119-121`) buys back
enough room and makes the two footers consistent. `Enter` and `l` both still
switch, and `k`/`j` both still move — only the hint text gets shorter.

Filtering:

```
/needle    [esc] clear  [↑↓] move  [↵] switch
```

The needle is truncated (`crate::ui::text::truncate`) so a long one cannot push
the hints off the line. `v`/`s`/`o` are omitted from this variant because they
are filter text while filtering, not shortcuts.

### Dashboard echo

`top_chrome` (`src/ui/dashboard/layout.rs:19`) gains `filter: Option<&str>` and
renders `/needle` after the group tabs when it is `Some`, truncated so it cannot
squeeze out the right-hand `N repos · N workspaces` block (which already floors
its gap at 1). `render_without_footer` passes `state.filter.as_deref()`.

Scope is the echo only. The dashboard's matching rules, its footer, and its
missing "no matching workspaces" empty state are untouched.

## Testing

Key handler (`src/app/input_tests.rs`):

- `/` sets `filter` to `Some("")` and leaves the panel open
- typing appends; Backspace pops; Backspace on an empty buffer is inert
- `j` moves the selection when not filtering, and appends `j` when filtering
- `↑`/`↓` still move, and Enter still switches, while filtering
- Esc with an active filter clears it and keeps the panel open; a second Esc
  closes the panel
- the selection follows its workspace across a filter edit that reorders the
  list, and clamps into range when the filter hides it
- opening the panel starts with `filter: None` (no inheritance from a prior open)

Panel (`src/ui/modal/updates_panel.rs`):

- `matches_filter` hits on each of workspace name, repo name, and status text,
  case-insensitively, and misses on a needle present in none of them
- an empty needle matches every workspace
- `ordered_workspaces_for_panel` returns only matching ids and keeps the
  unfiltered relative order among them
- a repo whose workspaces all filter out draws no header
- `(no matching workspaces)` renders for a non-empty needle with no hits;
  `(no workspaces)` still renders when there are none at all
- both footer variants fit 78 columns, for every `UpdatesSort` mode
- `row_status_text` returns the same text the row displays (guards the
  extraction against drift)

Dashboard (`src/ui/dashboard/layout.rs`):

- `top_chrome` renders `/needle` when a filter is active and omits it otherwise
- a long needle does not displace the right-hand counts

## Commits

1. `refactor`: introduce `PanelInputs`, extract `row_status_text`, add
   `App::awaiting_permission_map()`. No behavior change.
2. `feat`: filter workspaces in the updates panel with `/`.
3. `feat`: echo the active filter needle in the dashboard top chrome.

## Out of scope

- Persisting the filter across panel opens (`sort` does not persist either).
- Fuzzy or token-based matching. Substring only, matching the dashboard.
- Filtering the agents panel (`^x a`) or the process list.
- The dashboard's own missing "no matching workspaces" empty state.
