# Robust dashboard selection anchoring

**Date:** 2026-06-11
**Status:** Implemented (PR #169)
**Issue:** [#168](https://github.com/bakedbean/workspacex/issues/168)

## Problem

On the dashboard, the **selected workspace can jump to a *different* workspace**
when another (active) workspace changes status. Reproduction:

1. A repo holds two workspaces: `A` (active, Thinking) and `B` (the one the user
   has selected, Complete).
2. `A` finishes its turn (Thinking → Complete). The repo now has **zero** active
   workspaces, so `default_fold(counts)` flips to *folded*
   (`src/ui/dashboard/sort.rs:41` — folds when
   `question + stalled + waiting + thinking == 0`).
3. The repo auto-folds → `B`'s row disappears from `visible_targets`
   (`src/ui/dashboard/mod.rs:264` only emits workspace targets for expanded
   repos).
4. The per-frame re-anchor (`src/app/render.rs:241-261`) cannot find `B`'s
   target in the rebuilt `selectable`, so it falls into the `else` branch and
   **clamps the old index** — which now points at a *neighbor* target. Selection
   silently moves onto a different workspace and the original intent is lost.

Agent-driven status reporting (#166 / PR #167) made this more frequent: the
freshness-gate can flap a workspace Complete ↔ Working, folding/unfolding the
repo repeatedly and drifting the selection each tick.

This is **only** the selection-identity bug (issue #168, proposed work item 1).
Workspace *ordering* (recency/priority re-sorts) is explicitly **out of scope**.

## Root cause

The selection model makes the **index** authoritative and *derives* the
remembered target from it:

- `dashboard.selected: usize` — index into `app.selectable`. Source of truth.
- `dashboard.selection: Option<SelectionTarget>` — set each draw to
  `selected_target()` (`src/app/render.rs:262`); used by the renderer to paint
  the highlight by matching `WorkspaceId`.
- `selected_target()` → `selectable[selected]` (`src/app.rs:452`).

When the selected target's row leaves `selectable` (fold / filter / quiet-repo),
the index can no longer represent it. Clamping the index reassigns selection to
whatever target now occupies that slot, permanently losing the user's intent.

## Goal

The selected workspace **never changes to a different workspace** because of a
reorder, fold, or filter. Selection stays anchored to its `WorkspaceId`,
restoring it when a temporarily-hidden row reappears. `visible_targets` (the nav
index) and the rendered order stay in lockstep (unchanged).

## Design

### 1. Invert the selection model

- `dashboard.selection: Option<SelectionTarget>` becomes the **durable,
  authoritative** user intent.
- `dashboard.selected: usize` becomes a **derived nav cursor**: where the
  highlight sits *when the target is visible*, and the starting point for `j`/`k`
  stepping. It is no longer the identity of the selection.
- `selected_target()` returns `dashboard.selection` directly.
- New helper `App::select_index(idx)` sets **both** fields atomically:

  ```rust
  pub(crate) fn select_index(&mut self, idx: usize) {
      self.dashboard.selected = idx;
      self.dashboard.selection = self.selectable.get(idx).copied();
  }
  ```

  Every place that changes selection *intent* via an index uses this helper so
  the two fields cannot desync.

### 2. Reconciliation as a pure function

Extract the per-frame re-anchor into a pure, unit-testable function (location:
`src/ui/dashboard/mod.rs` or a small `selection` submodule):

```rust
/// Resolve the durable selection against a freshly-rebuilt selectable list.
/// Returns the (selection, selected-index) to store.
pub fn reconcile_selection(
    old_selection: Option<SelectionTarget>,
    old_selected: usize,
    new_selectable: &[SelectionTarget],
    target_exists: impl Fn(SelectionTarget) -> bool,
) -> (Option<SelectionTarget>, usize) {
    match old_selection {
        // Visible → follow / restore to its current index.
        Some(t) if new_selectable.iter().any(|s| *s == t) => {
            let idx = new_selectable.iter().position(|s| *s == t).unwrap();
            (Some(t), idx)
        }
        // Hidden but still exists (folded repo / filter / quiet repo) → PARK:
        // keep the intent on `t`; clamp the nav cursor for safety; do NOT
        // reassign identity. Highlight is simply not drawn until `t` returns.
        Some(t) if target_exists(t) => {
            let idx = old_selected.min(new_selectable.len().saturating_sub(1));
            (Some(t), idx)
        }
        // Gone (archived/deleted) or no prior selection → fall back to whatever
        // sits at the clamped index (None when the list is empty).
        _ => {
            if new_selectable.is_empty() {
                (None, 0)
            } else {
                let idx = old_selected.min(new_selectable.len() - 1);
                (new_selectable.get(idx).copied(), idx)
            }
        }
    }
}
```

`src/app/render.rs:241-261` becomes:

```rust
let new_selectable = dashboard::visible_targets(&inputs, &app.dashboard);
// Run every frame (not only when `new_selectable` differs): `refresh()`
// rebuilds `selectable` between draws, so a deleted selected workspace can
// leave the shape unchanged here while the target no longer exists. An
// unconditional reconcile drops the gone target promptly. The call is cheap
// (a couple of scans over a small vec).
let prev_selection = app.dashboard.selection;
let prev_selected = app.dashboard.selected;
let (selection, selected) = dashboard::reconcile_selection(
    prev_selection,
    prev_selected,
    &new_selectable,
    |t| app.selection_target_exists(t),
);
app.selectable = new_selectable;
app.dashboard.selection = selection;
app.dashboard.selected = selected;
```

`app.dashboard.selection = app.selected_target();` at the old line 262 is
removed — `selection` is now authoritative, not re-derived. (`prev_selection`
/ `prev_selected` are copied into locals first so the immutable borrow of
`app` inside the `target_exists` closure doesn't conflict with the writes.)

`App::selection_target_exists(t)` checks `app.workspaces` (for `Workspace`) /
`app.repos` (for `Repo`).

### 3. Sync points

- **Nav** (`src/app/input.rs:471-492`, `Up`/`k` and `Down`/`j`): after computing
  the stepped index, call `app.select_index(idx)` instead of assigning
  `dashboard.selected`.
- **Workspace-create landing** (`src/app.rs:1425`): use `select_index`.
- **Repo-move** (`src/app/input.rs:270-278`): already sets both; route through
  `select_index` for the index, keeping the explicit `selection` assignment or
  letting the helper do it.
- **`refresh()` clamp** (`src/app.rs:423-425`): stays as a harmless bound-check;
  identity now survives refresh because `selection` is authoritative.

### 4. Renderer (unchanged behavior)

`render_by_repo` / `render_by_attention` already paint the highlight by matching
`state.selection`'s `WorkspaceId` and call `list_state.select(selected_idx)`
with `None` when no row matches. A parked (hidden) selection naturally yields no
highlight and no list scroll until the row reappears. No change needed.

## Data flow

1. User selects workspace `B` → `select_index` sets `selection = Workspace(B)`,
   `selected = idx(B)`.
2. Active workspace `A` finishes → repo auto-folds → `B` leaves `visible_targets`.
3. Next draw: `reconcile_selection` sees `B` hidden but existing → **parks**:
   `selection` stays `Workspace(B)`, no highlight drawn.
4. `A` becomes active again (or user unfolds) → repo expands → `B` reappears →
   `reconcile_selection` finds `B` → `selected = idx(B)`, highlight restored.
5. If `B` is archived → `target_exists` false → selection falls back to a
   neighbor.

## Testing

Pure-function unit tests for `reconcile_selection`:

1. **Follow while visible** — target present at a new index → `(Some(t), newIdx)`.
2. **Park while hidden-but-exists** — target absent from `new_selectable`,
   `target_exists` true → selection unchanged (same `WorkspaceId`), index clamped.
   This is the core regression guard for #168.
3. **Restore on reappear** — after a park, target back in `new_selectable` →
   index points at it again.
4. **Fallback when gone** — target absent and `target_exists` false → selection
   becomes the clamped neighbor (or `None` when empty).
5. **First selection** — `old_selection = None` → selects the clamped index.

Integration-level (existing dashboard/app test harness):

6. Selected workspace's repo auto-folds (status change drives `default_fold`) →
   `app.selected_target()` still returns the same `Workspace(id)`.
7. Repo re-expands → highlight index resolves back to that workspace.

## Out of scope

- Workspace *ordering* changes (recency/priority re-sorts, stable sort keys,
  persisted `sort_order`, manual reorder). Issue #168 work items 2 and 3.
- Keeping a repo expanded because it holds the selected workspace (considered;
  deferred — the parking behavior is sufficient for the reported bug).
- Mouse row-click selection (does not exist today).
