# Workspace layout persistence

Today, splits in the attached view are ephemeral: parking back to the
dashboard requires closing each pane with `Ctrl-x d` until the last one
detaches. Re-entering a workspace always opens a fresh single pane.

This change lets the user park a multi-pane attached view, return to the
dashboard, and pick up exactly where they left off — including across
wsx restarts.

## Goals

- A new chord, `Ctrl-x Esc`, that returns to the dashboard while
  remembering the current split layout.
- Per-workspace layout storage: each workspace owns at most one
  remembered layout (keyed on the workspace that anchors it).
- Auto-restore on dashboard `Enter`: opening a workspace with a saved
  layout reconstructs the panes and spawns any sessions that aren't
  already live.
- Stale-leaf tolerance: archived workspaces drop out of restored
  layouts; layouts whose anchor is archived are deleted by cascade.
- A subtle dashboard indicator showing which workspaces have a saved
  multi-pane layout.

## Non-goals

- Cross-anchor layout sharing (each anchor's layout is independent).
- Layout undo / history.
- Lazy session spawning.
- Customizable restore behavior (modifier keys, opt-out).

## Architecture overview

Three localized changes:

1. **`src/store.rs`** — new `workspace_layouts` table + getters/setters,
   guarded by a new migration (`user_version = 10`). No effect on
   existing code paths.
2. **`src/app.rs`** — two touch points:
   - new `Ctrl-x Esc` chord in `handle_key_attached`,
   - replace the unconditional `AttachedState::single(id)` in the
     dashboard-Enter-on-Workspace branch with a `restore_attached_state`
     helper.
3. **`src/ui/split.rs`** — add `serde` derives on `SplitTree` and
   `SplitDirection`, plus a `prune` method that drops leaves matching a
   predicate and collapses singleton splits (mirrors the existing
   collapse logic in `SplitTree::close`). The existing private
   `first_leaf_path` helper is promoted to a `pub fn` (or a
   `SplitTree::first_leaf_path(&self) -> FocusPath` method) so restore
   can recompute focus after pruning.

No new modules, no new threads, no new events. Restore reuses
`app.sessions.spawn` — the same spawn used today from dashboard Enter.

## Data model

### Schema (migration v10)

```sql
CREATE TABLE IF NOT EXISTS workspace_layouts (
  anchor_workspace_id INTEGER PRIMARY KEY
    REFERENCES workspaces(id) ON DELETE CASCADE,
  tree_json TEXT NOT NULL,
  focus_json TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);
```

- `PRIMARY KEY` enforces at-most-one layout per anchor.
- `ON DELETE CASCADE` is the entire archive-cleanup story: when
  `Store::delete_workspace` runs, the matching layout row disappears
  automatically.
- `tree_json` / `focus_json` are TEXT (not BLOB) so they're greppable
  and inspectable via `sqlite3`.

### Serialization

`SplitTree` and `SplitDirection` derive `Serialize, Deserialize`.
`WorkspaceId` gets `#[serde(transparent)]` so it serializes as a bare
integer rather than `{"0": 12}`. Resulting JSON shapes:

```json
{"Leaf": 12}
{"Split": {"direction": "Vertical", "children": [{"Leaf": 12}, {"Leaf": 17}]}}
```

`FocusPath` (`Vec<usize>`) serializes as `[0, 1]`.

### Store API additions

```rust
impl Store {
    pub fn set_workspace_layout(
        &self,
        anchor: WorkspaceId,
        tree: &SplitTree,
        focus: &[usize],
    ) -> Result<()>;                    // INSERT OR REPLACE

    pub fn get_workspace_layout(
        &self,
        anchor: WorkspaceId,
    ) -> Result<Option<(SplitTree, FocusPath)>>;
    // Returns None on missing row OR on JSON parse failure (with warning
    // log, deleting the corrupt row).

    pub fn delete_workspace_layout(&self, anchor: WorkspaceId) -> Result<()>;

    pub fn list_multi_pane_layout_anchors(&self) -> Result<Vec<WorkspaceId>>;
    // Reads all rows, deserializes each tree, returns anchors whose
    // tree has more than one leaf. Used to populate the dashboard
    // indicator cache.
}
```

`Store` already depends on domain types from outside `store.rs` (e.g.
`WorkspaceState`, `SetupStatus`); adding `SplitTree` follows that
pattern.

## Save flow (`Ctrl-x Esc`)

Triggered in `handle_key_attached` inside the `if app.leader_pending`
arm:

```rust
KeyCode::Esc => {
    if let View::Attached(state) = &app.view {
        save_layout(app, state);
    }
    app.view = View::Dashboard;
    return Ok(());
}
```

Anchor resolution: the anchor is the **first leaf in tree order**.

```rust
fn save_layout(app: &App, state: &AttachedState) {
    let Some(anchor) = state.leaves().first().copied() else {
        return;
    };
    if let Err(e) = app.store.set_workspace_layout(
        anchor,
        &state.tree,
        &state.focus,
    ) {
        tracing::warn!(?e, "failed to save workspace layout");
    }
    app.refresh_layout_indicator_cache();
}
```

This anchor rule is correct because `SplitTree::split`
(`src/ui/split.rs:188-225`) always inserts new leaves *after* the
focused one. The first leaf in tree order is invariably the workspace
the user originally entered from the dashboard.

Single-pane park: `leaves()` returns `[A]`; the saved tree is
`Leaf(A)`. Restore degenerates to single-pane on next entry, identical
to the no-saved-layout case. Harmless, no special-casing.

`Ctrl-x d` and Esc detach paths do **not** save. Any prior saved
layout for that anchor is left untouched until either the next
`Ctrl-x Esc` rewrites it or the anchor is archived (CASCADE drops it).

Save failures log and continue — failing to save must not trap the
user in the attached view.

## Restore flow (dashboard `Enter` on Workspace)

Touchpoint: `src/app.rs:1525-1534`, the `SelectionTarget::Workspace`
branch. Replace the trailing `app.view = View::Attached(AttachedState::single(id));`
with:

```rust
app.view = restore_attached_state(app, id)?;
```

Where:

```rust
fn restore_attached_state(app: &mut App, anchor: WorkspaceId) -> Result<View> {
    let Some((mut tree, mut focus)) = app.store.get_workspace_layout(anchor)? else {
        return Ok(View::Attached(AttachedState::single(anchor)));
    };

    // Build the set of valid leaves: workspace must still exist in app.workspaces.
    let valid: HashSet<WorkspaceId> = app.workspaces.values().map(|w| w.id).collect();
    let pruned = tree.prune(|id| valid.contains(&id));

    match pruned {
        PruneOutcome::Empty => {
            // Anchor itself was missing or everything was pruned. Drop
            // the now-meaningless row and fall back to single-pane on
            // the anchor (which the outer code already spawned).
            let _ = app.store.delete_workspace_layout(anchor);
            app.refresh_layout_indicator_cache();
            Ok(View::Attached(AttachedState::single(anchor)))
        }
        PruneOutcome::Kept => {
            if tree.leaf_at(&focus).is_none() {
                focus = first_leaf_path(&tree);
            }
            // Spawn any session that isn't live yet (anchor was spawned
            // by the caller above).
            for leaf_id in tree.leaves() {
                if leaf_id == anchor || app.sessions.get(leaf_id).is_some() {
                    continue;
                }
                spawn_session_for(app, leaf_id);
            }
            Ok(View::Attached(AttachedState { tree, focus }))
        }
    }
}
```

### `SplitTree::prune`

New method in `src/ui/split.rs`:

```rust
pub enum PruneOutcome {
    Empty,
    Kept,
}

impl SplitTree {
    pub fn prune<F: Fn(WorkspaceId) -> bool>(&mut self, keep: F) -> PruneOutcome;
}
```

Implementation: depth-first. For each `Split`, prune children
recursively, drop ones that came back `Empty`, collapse to the sole
remaining child if only one survives, return `Empty` if zero survive.
For each `Leaf`, return `Empty` iff `keep(id)` is false. Mirrors the
collapse logic in `SplitTree::close` (`src/ui/split.rs:231-265`) and
preserves the same invariant: no 1-child splits.

### Spawn loop

Each `spawn` is a few ms (fork + tty alloc). A 4-pane restore is
~30ms, dominated by PTY setup. Acceptable. Parallelizing is YAGNI.

Spawn failures for side panes log and continue — the rest of the
layout still restores. The anchor's spawn happens before
`restore_attached_state` and uses `?`, matching today's behavior.

## Dashboard indicator (codicon `split_horizontal`)

A subtle badge appears next to workspaces whose saved layout has more
than one leaf, when nerd fonts are enabled.

### Placement

A trailing badge after the workspace name, parallel to how
`setup_failed` already renders ` ⚙!` (`src/ui/dashboard/row.rs:122-135`).
Same mechanism: shrink the name's pad width to reserve 2 cells, then
render ` ` followed by the codicon after the name span.

If both `setup_failed` and `has_multi_pane_layout` are true the badges
stack: name → ` ⚙!` → ` `.

### Glyph

`nf-cod-split_horizontal` = **U+EBB0**, styled with `theme.dim_style()`.

### Gating

Only rendered when `inputs.nerd_fonts == true`, matching the
branch-lifecycle glyph convention (`row.rs:138-148`). No Unicode
fallback — split layouts are an advanced feature.

### When the badge shows

Only when the saved layout has **more than one leaf**. A single-pane
saved layout is functionally identical to never-having-saved.

### Data plumbing

- Add `pub has_multi_pane_layout: bool` to `RowInputs`.
- Add `app.workspaces_with_multi_pane_layouts: HashSet<WorkspaceId>` to
  `App`, populated by `Store::list_multi_pane_layout_anchors`.
- Refresh the cache:
  - on startup, in `App::new` after `Store::open`,
  - after every `Ctrl-x Esc` save (`save_layout` calls
    `app.refresh_layout_indicator_cache()`),
  - after restore prunes a layout down to empty
    (`restore_attached_state` does the same),
  - after workspace archive (callers of `Store::delete_workspace`).

The refresh is one indexed query plus a JSON parse per row. Cheap
enough to re-run after each potentially-changing event without
per-event diff logic.

### Row rendering

```rust
let layout_badge_width = if inputs.has_multi_pane_layout && inputs.nerd_fonts { 2 } else { 0 };
let name_target = name_width
    .saturating_sub(if inputs.setup_failed { 3 } else { 0 })
    .saturating_sub(layout_badge_width)
    .max(1);
// existing name + setup_failed badge rendering ...
if inputs.has_multi_pane_layout && inputs.nerd_fonts {
    spans.push(Span::styled(" \u{ebb0}".to_string(), theme.dim_style()));
}
```

## Edge cases & lifecycle

- **Anchor archived while parked.** `ON DELETE CASCADE` drops the
  layout row when `Store::delete_workspace` runs.
- **Side-pane workspace archived while parked.** The layout row
  survives; the dangling `WorkspaceId` is filtered out by `prune` on
  the next restore.
- **All side panes archived.** `prune` collapses the tree to a single
  anchor leaf; restore returns `AttachedState::single(anchor)`. The
  row remains; next `Ctrl-x Esc` rewrites it.
- **Anchor not in `app.workspaces` at restore.** The spawn at the call
  site fails first, so `restore_attached_state` isn't reached.
  Belt-and-braces: `prune` would return `Empty` and the existing
  missing-session check would bounce to dashboard.
- **Workspace appears in multiple anchors' layouts.** Allowed. Each
  anchor's row is independent.
- **Anchor flip when first leaf closes.** Closing the original entry
  pane via `Ctrl-x d` shifts the first leaf. The next `Ctrl-x Esc`
  saves under the new first leaf; the previous anchor's row is left
  untouched (the user can re-enter the old anchor later and pick up
  its layout).
- **Layout JSON corruption.** `get_workspace_layout` catches
  `serde_json::Error`, logs a warning, deletes the row, returns
  `None`. Caller falls back to single-pane.
- **Migration on existing DBs.** Migration v10 is purely additive (a
  new table), following the existing user_version-gated pattern in
  `src/store.rs:93-204`. Reopening an older DB upgrades cleanly.
- **Users who never use `Ctrl-x Esc`.** No layouts ever get written.
  `get_workspace_layout` always returns `None`. `restore_attached_state`
  always returns `AttachedState::single`. Zero behavior change.

## Testing

### Unit — `src/ui/split.rs`

- `prune_removes_dropped_leaves_and_collapses_singletons` — start with
  `(A|B|C)`, prune `B`, expect `(A|C)`.
- `prune_collapses_nested_singleton` — start with `(A | (B/C))`, prune
  `C`, expect `(A|B)` not `(A | (B))`.
- `prune_returns_empty_when_no_leaves_survive` — prune everything,
  expect `PruneOutcome::Empty`.
- `prune_preserves_invariant_no_one_child_splits` — build a random
  tree, prune a random subset, assert every `Split` has ≥2 children.
- `splittree_serde_round_trip` — `to_string` → `from_str` for a
  representative nested tree, assert structural equality.

### Unit — `src/store.rs`

- `set_then_get_workspace_layout_round_trips`.
- `archiving_workspace_cascades_to_layout` — insert anchor + layout,
  `delete_workspace`, expect `get_workspace_layout` returns `None`.
- `get_workspace_layout_returns_none_on_corrupted_json` — manually
  `UPDATE` to invalid JSON, read, expect `None` and that the row is
  deleted.
- `set_workspace_layout_replaces_existing` — write twice, second wins.
- `list_multi_pane_layout_anchors_returns_only_multi_leaf_layouts` —
  insert one single-leaf and one two-leaf layout, expect only the
  second's anchor.

### Integration — `src/app.rs`

- `ctrl_x_esc_saves_layout_and_returns_to_dashboard` — 2-pane attached
  view, send `Ctrl-x Esc`, assert `view == Dashboard` and store has a
  row keyed by the first leaf.
- `dashboard_enter_restores_saved_layout` — write a layout via store,
  simulate dashboard Enter on the anchor, assert `View::Attached` with
  the right leaves in the right shape.
- `dashboard_enter_falls_back_to_single_pane_when_no_layout` — pristine
  workspace, Enter, assert single-pane (regression guard for existing
  behavior).
- `restore_prunes_archived_side_panes` — write `(A|B|C)`, archive `B`,
  Enter on `A`, assert restored layout is `(A|C)`.
- `ctrl_x_d_does_not_modify_saved_layout` — write a layout, enter,
  close a pane with `Ctrl-x d`, return to dashboard via last-pane
  close, assert the stored layout is unchanged.
- `dashboard_cache_refreshes_after_save_layout` — drive `Ctrl-x Esc`,
  assert `app.workspaces_with_multi_pane_layouts` contains the anchor.
- `dashboard_cache_refreshes_after_archive` — set up a layout, archive
  the anchor, assert the set no longer contains it.

### Unit — `src/ui/dashboard/row.rs`

- `multi_pane_layout_appends_codicon_when_nerd_fonts` — flags true,
  assert `\u{ebb0}` present.
- `multi_pane_layout_skipped_without_nerd_fonts` — `nerd_fonts = false`,
  assert glyph absent.
- `layout_and_setup_failed_both_render` — both flags true, assert both
  `⚙!` and `\u{ebb0}` present.
- `name_shrinks_to_accommodate_layout_badge` — long name with flag
  true, assert name truncated to fit the badge.

### Manual smoke

Add a `docs/manual-tests/workspace-layout-persistence.md` mirroring the
existing `attention-detection.md` so future contributors know the
manual path:

- Park a 3-pane layout, quit wsx (`q`), restart, enter the anchor —
  layout restores with all three sessions live.
- Park a layout, archive a side pane via CLI, re-enter — layout is
  pruned correctly.
- Park a layout, archive the anchor, confirm the row is gone (sqlite
  inspection).

## File touch list

- `src/store.rs` — migration v10, 3 new public methods, ~80 lines + tests.
- `src/ui/split.rs` — serde derives, `prune` method + `PruneOutcome` enum,
  ~50 lines + tests.
- `src/app.rs` — `restore_attached_state` helper, `save_layout` helper,
  `refresh_layout_indicator_cache`, new `Ctrl-x Esc` chord arm,
  modification to dashboard-Enter branch, new `App` field, ~80 lines
  + tests.
- `src/ui/dashboard/row.rs` — new `RowInputs.has_multi_pane_layout`
  field, name-width math, badge render, ~15 lines + tests.
- `src/ui/dashboard/{by_attention,by_repo,fixture}.rs` — populate the
  new `RowInputs` field from the cache.
- `docs/manual-tests/workspace-layout-persistence.md` — new file.
