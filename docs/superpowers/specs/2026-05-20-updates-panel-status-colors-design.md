# Updates panel — dashboard-aligned status colors

## Goal

The workspace updates panel (opened with `Ctrl+X u` from the agent chat, aka
`Modal::UpdatesPanel`) currently renders every row as a single flat string.
Make each row carry the same color signals the V5 dashboard already uses, so
the user can scan the modal and the dashboard with the same mental model when
deciding which workspace to jump to.

## Coloring rules

Per row, top-to-bottom in the row's spans:

| Segment              | Source                                                    | Style                                      |
|----------------------|-----------------------------------------------------------|--------------------------------------------|
| Leading indent       | —                                                         | default                                    |
| Status glyph + space | `App::classify_status(ws)` (the canonical 6-state Status) | `theme.status_style(status)`               |
| Workspace name       | `app.pr_lifecycle.get(ws.id)` (`BranchLifecycle`)         | `theme.lifecycle_style(lifecycle)` + bold  |
| Status text + age    | same `status`                                             | `theme.status_style(status)`               |

`lifecycle_style` mapping (identical to today's `dashboard/row.rs:213`):

| Lifecycle         | Style              |
|-------------------|--------------------|
| `PrOpen`          | `ok` (green)       |
| `PrConflicted`    | `warn` (yellow)    |
| `PrMerged`        | `merged` (purple)  |
| `PrClosed`        | `err` (red)        |
| `NoPr` / `PrDraft`/ `None` | default (bold only) |

### Failed workspaces

When `ws.state == WorkspaceState::Failed`, the glyph (`✕`) and status text
(`failed`) use `theme.err_style()` directly. Lifecycle still wins on the name —
a failed workspace can still have a merged PR.

### Selection

Switch from `theme.selected_style()` (sets both fg and bg, which erases the new
per-span colors) to `theme.selected_bg_style()` applied via `Line::style(..)`.
ratatui composes line-level style as the base under per-span styles, so:
- per-span foregrounds win → status / lifecycle colors stay visible
- the selected row's background still highlights the selection

Matches the dashboard's `List::highlight_style(theme.selected_bg_style())` in
`dashboard/mod.rs:108`.

## Refactor: `Theme::lifecycle_style`

Today `lifecycle_style(lc, theme) -> Option<Style>` lives as a free function in
`dashboard/row.rs`. Move it to `Theme::lifecycle_style(Option<BranchLifecycle>) -> Option<Style>`
keeping the `Option` return so each caller can pick its own fallback:
- dashboard branch column → `unwrap_or_else(|| theme.dim_style())` (today's behavior)
- modal name span → `unwrap_or_default()` then add `Modifier::BOLD`

Both renderers call the same helper for the mapping itself. No behavior
change for the dashboard.

Rationale: `Theme::status_style(Status)` already exists for exactly this
reason. Putting both lookups on `Theme` keeps the modal and dashboard from
drifting on what "PR merged" or "stalled" looks like, and lets a future theme
override one color without touching renderers.

## Signature changes

```rust
// src/ui/modal.rs
pub fn render_updates_panel(
    f: &mut Frame,
    area: Rect,
    repos: &[Repo],
    workspaces: &[(RepoId, Workspace)],
    events: &HashMap<WorkspaceId, WorkspaceEvents>,
    activity: &HashMap<WorkspaceId, ActivityState>,
    needs_attention: &HashSet<WorkspaceId>,
    awaiting: &HashMap<WorkspaceId, (String, i64)>,
    statuses: &HashMap<WorkspaceId, Status>,        // NEW
    lifecycles: &HashMap<WorkspaceId, BranchLifecycle>, // NEW (= &app.pr_lifecycle)
    selected: usize,
    now_ms: i64,
    theme: &Theme,
);

fn workspace_row<'a>(
    w: &'a Workspace,
    events: Option<&'a WorkspaceEvents>,
    activity: Option<ActivityState>,
    needs_attention: bool,
    awaiting: Option<&'a (String, i64)>,
    is_selected: bool,
    status: Status,                                 // NEW
    lifecycle: Option<BranchLifecycle>,             // NEW
    now_ms: i64,
    theme: &Theme,
) -> Line<'a>;
```

Call site in `app.rs` (the block around line 977 that builds the modal data)
gains a single pre-pass:

```rust
let statuses: HashMap<WorkspaceId, Status> = app.workspaces
    .iter()
    .map(|(_, w)| (w.id, app.classify_status(w)))
    .collect();
// pass &statuses and &app.pr_lifecycle into render_updates_panel
```

`classify_status` does a small amount of work per workspace (a few
HashMap lookups, an atomic load, a SystemTime::now), so building this map fresh
each render tick is fine — it's the same cost the dashboard already pays.

## Out of scope

- Sort order. The panel keeps `(attention, failed, activity_rank, recency)`.
- Layout. No new columns, no branch text, no glyph changes.
- Other modals (process list, repo settings, new workspace).

## Tests

Extend `workspace_row_tests` in `src/ui/modal.rs`:

1. Status → glyph color. Build a row with `Status::Question` and assert the
   glyph span has `fg == theme.question`. Repeat for Complete, Stalled,
   Thinking, Idle.
2. Failed override. `WorkspaceState::Failed` + any status → glyph + text spans
   have `fg == theme.err`.
3. Lifecycle → name color. `Some(PrOpen)` → name span `fg == theme.ok`;
   `PrConflicted` → warn; `PrMerged` → merged; `PrClosed` → err;
   `None` / `NoPr` / `PrDraft` → no fg (default).
4. Selection preserves spans. With `is_selected = true`, the line-level style
   has the selected bg but spans keep their own fg.

Existing assertions on glyph characters and status text content stay valid —
the body string is unchanged, only its spans are split.

## Risks / non-risks

- **Risk: visual regression on themes that share status and lifecycle colors.**
  In the `wsx` theme, `complete == ok` (both green) and `stalled == err` (both
  red), so a workspace at `Status::Complete` with `PrOpen` will have a
  green name and green text — readable but homogeneous. Acceptable.
- **Non-risk: extra allocations.** Building one `HashMap<WorkspaceId, Status>`
  per render tick is the same scale as the existing `activity_translated` map.
  No measurable cost.
