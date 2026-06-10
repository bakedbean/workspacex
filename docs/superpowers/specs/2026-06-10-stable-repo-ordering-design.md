# Stable, manually-ordered repos on the dashboard

**Date:** 2026-06-10
**Status:** Approved (pending implementation)

## Problem

On the dashboard's by-repo view, repos jump around in ordering as workspaces
are added or removed. The order is computed every frame by a dynamic "noise
score" that aggregates each repo's workspace statuses
(`src/ui/dashboard/by_repo.rs::order_repos` → `src/ui/dashboard/sort.rs::noise_score`).
Any change to a repo's workspaces — adding one, removing one, or a status
transition — changes its score and reshuffles its position. The result is an
unpredictable, constantly-shifting layout.

## Goal

Repos appear in a **stable, predictable, user-controlled order**. Adding,
removing, or changing the status of a workspace never moves a repo. The user
controls the order directly and it persists across sessions.

## Decisions (locked)

- **Ordering scheme:** manual / user-defined, persisted.
- **Reorder keys (TUI):** `Shift+K` moves the selected repo up, `Shift+J` moves
  it down. Mirrors the existing `j`/`k` navigation.
- **Initial order:** on first migration (and as the seed for any repo never
  manually moved), repos are ordered **alphabetically by name**.
- **Scope:** TUI only for now. No CLI command (`wsx repo reorder` can come
  later if wanted).

## Design

### 1. Data model — `src/data/store.rs`

- **Migration v14.** Following the existing versioned-migration pattern
  (`migrate()`, guarded by `PRAGMA user_version` and a
  `pragma_table_info('repos')` column-existence check for idempotency):

  ```sql
  ALTER TABLE repos ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;
  ```

  Then a one-time seed assigning each existing repo a unique alphabetical rank
  (case-insensitive, `id` as tiebreak for determinism):

  ```sql
  UPDATE repos SET sort_order = (
      SELECT COUNT(*) FROM repos r2
      WHERE LOWER(r2.name) < LOWER(repos.name)
         OR (LOWER(r2.name) = LOWER(repos.name) AND r2.id < repos.id)
  );
  ```

  Bump `PRAGMA user_version = 14`.

- **`Repo` struct.** Add `pub sort_order: i64`. Update the `repos()` query to
  select the column and `ORDER BY sort_order, name`.

- **`RepoAdd` / repo registration.** New repos receive
  `sort_order = COALESCE(MAX(sort_order), -1) + 1` so they are appended to the
  end and never disturb existing positions. This also guarantees `sort_order`
  values stay unique.

- **New store method.** `swap_repo_sort_order(a: RepoId, b: RepoId)` swaps the
  two rows' `sort_order` values inside a transaction (atomic; preserves
  uniqueness). This is the only mutation the reorder feature needs.

### 2. Ordering — `src/ui/dashboard/by_repo.rs` + `src/ui/dashboard/sort.rs`

- Thread `sort_order` onto `RepoView` (built in `src/ui/dashboard/mod.rs`).
- `order_repos()` becomes a plain ascending sort by `sort_order`. The
  empty-repos-to-tail rule and all `noise_score()`-based ordering are removed.
- Remove `noise_score()` if it becomes unused after this change (and update or
  remove its tests).
- **Kept unchanged:** `default_fold()` still uses status counts. Busy repos
  continue to auto-expand to surface what needs attention — they simply no
  longer *move*. This preserves the attention signal while making the order
  static.

### 3. Reorder action — `src/app/input.rs`

- New arms in `handle_key_dashboard`:
  - `(KeyCode::Char('K'), _)` → `move_selected_repo(app, Direction::Up)`
  - `(KeyCode::Char('J'), _)` → `move_selected_repo(app, Direction::Down)`

  (Uppercase `K`/`J` are currently unbound; lowercase `j`/`k` remain
  navigation. This follows the existing pattern of matching uppercase chars
  directly, as `fold_all_repos` does for `M`/`m`.)

- `move_selected_repo` logic:
  1. Resolve the current selection to a repo: a `Repo` selection → itself; a
     `Workspace` selection → its parent `repo_id`.
  2. Find that repo's index in the `sort_order`-ordered repo list and its
     neighbor in the requested direction.
  3. `swap_repo_sort_order(repo, neighbor)`; reload `app.repos`.
  4. **Keep the selection on the moved repo** so repeated `K`/`J` presses walk
     it into place (re-resolve the selectable index by `SelectionTarget`).

- **Guards:**
  - Only active in by-repo group mode (`GroupMode`). In by-workspace mode there
    are no repo groupings to reorder → no-op.
  - No-op at the top (Up) or bottom (Down) of the list, and when fewer than two
    repos exist.

### 4. Data flow

1. Startup / refresh: `store.repos()` returns repos ordered by `sort_order`.
2. Render: `RepoView`s carry `sort_order`; `order_repos()` sorts ascending by
   it. `visible_targets()` rebuilds the selectable vector in that order.
3. User presses `Shift+K`/`Shift+J`: `swap_repo_sort_order` persists the swap,
   `app.repos` reloads, selection stays on the moved repo. Next draw reflects
   the new order.
4. Workspace add/remove/status change: only affects fold/expansion, never
   `sort_order` → repo positions are unchanged.

### 5. Testing

- **Migration:** existing repos receive unique alphabetical `sort_order`; the
  migration is idempotent (safe to re-run).
- **Registration:** `RepoAdd` appends new repos to the tail (max + 1).
- **Store:** `swap_repo_sort_order` swaps the two values and is atomic.
- **Ordering:** `order_repos` sorts purely by `sort_order` ascending
  (replaces the existing noise-score ordering tests).
- **Reorder action:** `move_selected_repo` up/down moves the repo and preserves
  selection; no-ops at the ends, with <2 repos, and in by-workspace mode.

## Out of scope

- CLI reorder command (`wsx repo reorder ...`).
- Drag-and-drop / mouse reordering.
- Per-repo pinning or grouping beyond a single linear order.
