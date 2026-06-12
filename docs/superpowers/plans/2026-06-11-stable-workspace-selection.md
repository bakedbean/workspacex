# Robust Dashboard Selection Anchoring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop the dashboard selection from jumping to a *different* workspace when another workspace's status change auto-folds (or filters) the selected workspace's row.

**Architecture:** Invert the selection model so the remembered `SelectionTarget` is authoritative and the list index is a derived nav cursor. A pure `reconcile_selection` function resolves the durable target against the freshly-rebuilt `selectable` list each frame: it follows the target while visible, *parks* it (keeps the same `WorkspaceId`) while temporarily hidden, and only falls back to a neighbor when the target no longer exists.

**Tech Stack:** Rust, ratatui TUI, `cargo test` / `cargo clippy` / `cargo fmt`.

**Spec:** `docs/superpowers/specs/2026-06-11-stable-workspace-selection-design.md`

---

## File Structure

- `src/ui/dashboard/mod.rs` — add the pure `reconcile_selection` function + its unit tests. It already imports `crate::app::SelectionTarget` (line 17), so the function lives naturally beside `visible_targets`.
- `src/app.rs` — change `selected_target()` to return the durable `dashboard.selection`; add `select_index()` and `selection_target_exists()` helpers; use `select_index` at the create-landing site.
- `src/app/render.rs` — replace the re-anchor block (lines 241–262) with a call to `reconcile_selection`.
- `src/app/input.rs` — route nav (`j`/`k`/Up/Down) and repo-move selection through `select_index`.

---

## Task 1: Pure `reconcile_selection` function

**Files:**
- Modify: `src/ui/dashboard/mod.rs` (add function near `visible_targets`, ~line 327; add tests in the existing `#[cfg(test)] mod tests` or a new `#[cfg(test)] mod selection_tests`)

- [ ] **Step 1: Write the failing tests**

Add to `src/ui/dashboard/mod.rs` (append a new test module at the end of the file, before or after the existing `state_defaults` module):

```rust
#[cfg(test)]
mod reconcile_selection_tests {
    use super::*;
    use crate::data::store::{RepoId, WorkspaceId};

    fn ws(n: i64) -> SelectionTarget {
        SelectionTarget::Workspace(WorkspaceId(n))
    }
    fn repo(n: i64) -> SelectionTarget {
        SelectionTarget::Repo(RepoId(n))
    }

    #[test]
    fn follows_target_to_new_index_while_visible() {
        // Selected ws(2) was at index 0; after a reorder it sits at index 2.
        let new = vec![ws(1), ws(3), ws(2)];
        let (sel, idx) = reconcile_selection(Some(ws(2)), 0, &new, |_| true);
        assert_eq!(sel, Some(ws(2)), "identity preserved");
        assert_eq!(idx, 2, "index follows the target");
    }

    #[test]
    fn parks_on_same_target_when_hidden_but_exists() {
        // ws(2)'s repo auto-folded → ws(2) is gone from selectable but still
        // exists. Selection must NOT move to a neighbor.
        let new = vec![repo(1), ws(1)];
        let (sel, idx) = reconcile_selection(Some(ws(2)), 1, &new, |t| t == ws(2));
        assert_eq!(sel, Some(ws(2)), "selection parked on the same workspace");
        assert!(idx < new.len(), "nav cursor clamped in-bounds");
    }

    #[test]
    fn restores_index_when_target_reappears() {
        // After parking, the repo re-expands and ws(2) is back at index 2.
        let new = vec![repo(1), ws(1), ws(2)];
        let (sel, idx) = reconcile_selection(Some(ws(2)), 1, &new, |_| true);
        assert_eq!(sel, Some(ws(2)));
        assert_eq!(idx, 2, "highlight resolves back to the workspace");
    }

    #[test]
    fn falls_back_to_neighbor_when_target_gone() {
        // ws(2) archived: absent from selectable AND target_exists is false.
        let new = vec![repo(1), ws(1), ws(3)];
        let (sel, idx) = reconcile_selection(Some(ws(2)), 2, &new, |_| false);
        assert_eq!(idx, 2, "clamped to old index");
        assert_eq!(sel, Some(ws(3)), "selection becomes the neighbor at that slot");
    }

    #[test]
    fn empty_selectable_yields_none() {
        let new: Vec<SelectionTarget> = vec![];
        let (sel, idx) = reconcile_selection(Some(ws(2)), 5, &new, |_| false);
        assert_eq!(sel, None);
        assert_eq!(idx, 0);
    }

    #[test]
    fn none_selection_selects_clamped_index() {
        let new = vec![repo(1), ws(1)];
        let (sel, idx) = reconcile_selection(None, 5, &new, |_| true);
        assert_eq!(idx, 1, "clamped to last");
        assert_eq!(sel, Some(ws(1)));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p wsx reconcile_selection_tests 2>&1 | tail -20`
Expected: FAIL to compile — `cannot find function reconcile_selection in this scope`.

- [ ] **Step 3: Implement `reconcile_selection`**

Add to `src/ui/dashboard/mod.rs` immediately after the `visible_targets` function (after line 327):

```rust
/// Resolve the durable selection against a freshly-rebuilt `selectable` list.
/// Returns the `(selection, selected-index)` the dashboard should store.
///
/// - **Visible:** the target is still in `new_selectable` → follow it to its
///   current index (this is how selection survives reorders and restores after
///   a fold re-expands).
/// - **Hidden but existing:** the target left `new_selectable` (its repo
///   auto-folded, a filter hid it, or it dropped to QUIET REPOS) yet still
///   exists per `target_exists` → *park*: keep the same target, clamp the nav
///   cursor for safety, and do NOT reassign identity to a neighbor. The renderer
///   simply draws no highlight until the row returns.
/// - **Gone / no prior selection:** the target was archived (`target_exists`
///   false) or there was no selection → fall back to whatever sits at the
///   clamped index (`None` when the list is empty).
pub fn reconcile_selection(
    old_selection: Option<SelectionTarget>,
    old_selected: usize,
    new_selectable: &[SelectionTarget],
    target_exists: impl Fn(SelectionTarget) -> bool,
) -> (Option<SelectionTarget>, usize) {
    if let Some(t) = old_selection {
        if let Some(idx) = new_selectable.iter().position(|s| *s == t) {
            return (Some(t), idx);
        }
        if target_exists(t) {
            let idx = old_selected.min(new_selectable.len().saturating_sub(1));
            return (Some(t), idx);
        }
    }
    if new_selectable.is_empty() {
        (None, 0)
    } else {
        let idx = old_selected.min(new_selectable.len() - 1);
        (new_selectable.get(idx).copied(), idx)
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p wsx reconcile_selection_tests 2>&1 | tail -20`
Expected: PASS (6 passed).

- [ ] **Step 5: Commit**

```bash
git add src/ui/dashboard/mod.rs
git commit -m "feat(dashboard): add reconcile_selection for stable selection anchoring (#168)"
```

---

## Task 2: App selection helpers + authoritative `selected_target()`

**Files:**
- Modify: `src/app.rs` — `selected_target()` (lines 452–454); add `select_index()` and `selection_target_exists()` nearby.
- Test: add a `#[cfg(test)]` module in `src/app.rs` (or extend an existing one) — see step 1.

- [ ] **Step 1: Write the failing tests**

Find the existing in-memory App test pattern. There is one at `src/app/render.rs:960` (`App::new(store, PathBuf::from("/tmp/wsx-test"))`). Add this test module at the end of `src/app.rs`:

```rust
#[cfg(test)]
mod selection_helper_tests {
    use super::*;
    use crate::data::store::{NewWorkspace, Store};
    use std::path::PathBuf;

    fn app_with_one_workspace() -> (App, crate::data::store::WorkspaceId) {
        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "x")
            .unwrap();
        let w = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "a",
                branch: "x/a",
                worktree_path: std::path::Path::new("/tmp/r/a"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        (app, w)
    }

    #[test]
    fn select_index_sets_both_fields() {
        let (mut app, w) = app_with_one_workspace();
        // selectable after refresh: [Repo(repo), Workspace(w)].
        let idx = app
            .selectable
            .iter()
            .position(|t| *t == SelectionTarget::Workspace(w))
            .unwrap();
        app.select_index(idx);
        assert_eq!(app.dashboard.selected, idx);
        assert_eq!(app.dashboard.selection, Some(SelectionTarget::Workspace(w)));
    }

    #[test]
    fn selected_target_returns_durable_selection_not_index() {
        let (mut app, w) = app_with_one_workspace();
        app.dashboard.selection = Some(SelectionTarget::Workspace(w));
        // Deliberately desync the index to an out-of-range / different slot.
        app.dashboard.selected = 0; // Repo header
        assert_eq!(
            app.selected_target(),
            Some(SelectionTarget::Workspace(w)),
            "selected_target follows the durable selection, not the index"
        );
    }

    #[test]
    fn selection_target_exists_tracks_workspaces_and_repos() {
        let (app, w) = app_with_one_workspace();
        let repo_id = app.repos[0].id;
        assert!(app.selection_target_exists(SelectionTarget::Workspace(w)));
        assert!(app.selection_target_exists(SelectionTarget::Repo(repo_id)));
        assert!(!app.selection_target_exists(SelectionTarget::Workspace(
            crate::data::store::WorkspaceId(9999)
        )));
    }
}
```

Note: confirm `Store::add_repo`, `insert_workspace`, and `NewWorkspace` field names against `src/app/render.rs:960-977` (this plan mirrors that exact harness). If `add_repo`'s signature differs, copy it verbatim from that test.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p wsx selection_helper_tests 2>&1 | tail -20`
Expected: FAIL to compile — `no method named select_index` / `selection_target_exists`.

- [ ] **Step 3: Implement the helpers and change `selected_target()`**

In `src/app.rs`, replace the existing `selected_target` (lines 452–454):

```rust
    pub fn selected_target(&self) -> Option<SelectionTarget> {
        self.selectable.get(self.dashboard.selected).copied()
    }
```

with:

```rust
    /// The durable, authoritative selection target. Returns
    /// `dashboard.selection` rather than indexing `selectable`, so the
    /// selection survives a temporarily-hidden row (folded repo / filter /
    /// quiet repo) instead of silently following the index onto a neighbor.
    pub fn selected_target(&self) -> Option<SelectionTarget> {
        self.dashboard.selection
    }

    /// Set the selection by index into the current `selectable`, keeping the
    /// durable `selection` target and the `selected` nav cursor in sync. Use
    /// this anywhere selection *intent* changes via an index (nav, click,
    /// landing on a freshly-created workspace).
    pub(crate) fn select_index(&mut self, idx: usize) {
        self.dashboard.selected = idx;
        self.dashboard.selection = self.selectable.get(idx).copied();
    }

    /// Whether a selection target still refers to a live repo/workspace.
    /// Used by `reconcile_selection` to tell a temporarily-hidden target
    /// (park it) from a removed one (fall back to a neighbor).
    pub(crate) fn selection_target_exists(&self, t: SelectionTarget) -> bool {
        match t {
            SelectionTarget::Repo(id) => self.repos.iter().any(|r| r.id == id),
            SelectionTarget::Workspace(id) => {
                self.workspaces.iter().any(|(_, w)| w.id == id)
            }
        }
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p wsx selection_helper_tests 2>&1 | tail -20`
Expected: PASS (3 passed).

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): make selected_target durable; add select_index + selection_target_exists (#168)"
```

---

## Task 3: Wire `reconcile_selection` into the draw loop

**Files:**
- Modify: `src/app/render.rs` lines 241–262 (the re-anchor block + the `app.dashboard.selection = app.selected_target();` line).

- [ ] **Step 1: Replace the re-anchor block**

In `src/app/render.rs`, the current block reads:

```rust
            let new_selectable = dashboard::visible_targets(&inputs, &app.dashboard);
            if new_selectable != app.selectable {
                // Preserve the user's *target* across reorderings, not
                // their *index* — keep arrow nav anchored to the same
                // workspace even if the visible order shifts (e.g.
                // status change moves it up/down).
                let prev_target = app.selectable.get(app.dashboard.selected).copied();
                app.selectable = new_selectable;
                if let Some(t) = prev_target {
                    if let Some(idx) = app.selectable.iter().position(|s| *s == t) {
                        app.dashboard.selected = idx;
                    } else if !app.selectable.is_empty() {
                        app.dashboard.selected =
                            app.dashboard.selected.min(app.selectable.len() - 1);
                    } else {
                        app.dashboard.selected = 0;
                    }
                } else if !app.selectable.is_empty() {
                    app.dashboard.selected = app.dashboard.selected.min(app.selectable.len() - 1);
                }
            }
            app.dashboard.selection = app.selected_target();
```

Replace it entirely with:

```rust
            let new_selectable = dashboard::visible_targets(&inputs, &app.dashboard);
            if new_selectable != app.selectable {
                // Reconcile the durable selection against the rebuilt list.
                // A temporarily-hidden target (folded repo / filter / quiet
                // repo) is PARKED on the same WorkspaceId rather than clamped
                // onto a neighbor, and restored when its row reappears.
                let (selection, selected) = dashboard::reconcile_selection(
                    app.dashboard.selection,
                    app.dashboard.selected,
                    &new_selectable,
                    |t| app.selection_target_exists(t),
                );
                app.selectable = new_selectable;
                app.dashboard.selection = selection;
                app.dashboard.selected = selected;
            } else if app.dashboard.selection.is_none() {
                // First frame / never-selected: seed selection from the cursor.
                app.dashboard.selection = app.selectable.get(app.dashboard.selected).copied();
            }
```

- [ ] **Step 2: Build to verify it compiles**

Run: `cargo build -p wsx 2>&1 | tail -20`
Expected: builds clean (warnings OK). Note: the closure borrows `app` immutably while `app.dashboard.*` fields are read by value first — the three field reads (`app.dashboard.selection`, `app.dashboard.selected`) are `Copy` and are evaluated before the closure, so there is no borrow conflict. If the borrow checker complains, copy the two fields into locals first:
```rust
let (prev_sel, prev_idx) = (app.dashboard.selection, app.dashboard.selected);
let (selection, selected) = dashboard::reconcile_selection(prev_sel, prev_idx, &new_selectable, |t| app.selection_target_exists(t));
```

- [ ] **Step 3: Run the full dashboard + app test suites**

Run: `cargo test -p wsx 2>&1 | tail -30`
Expected: PASS. If a pre-existing test asserted the *old* clamp-to-neighbor behavior on a hidden target, update it to expect the parked target (cite the test name when doing so).

- [ ] **Step 4: Commit**

```bash
git add src/app/render.rs
git commit -m "fix(dashboard): park selection on hidden rows instead of clamping to a neighbor (#168)"
```

---

## Task 4: Route nav and create-landing through `select_index`

**Files:**
- Modify: `src/app/input.rs` lines 471–492 (Up/`k`, Down/`j`).
- Modify: `src/app.rs` lines 1422–1425 (create landing).
- Modify: `src/app/input.rs` lines 270–278 (repo-move) — optional consistency pass.

- [ ] **Step 1: Update the Up/`k` and Down/`j` handlers**

In `src/app/input.rs`, the current arms read:

```rust
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
            let max = app.selectable.len().saturating_sub(1);
            app.dashboard.selected = if app.dashboard.selected == 0 {
                max
            } else {
                app.dashboard.selected - 1
            };
            // Clear any in-flight reply draft so it can't leak to the newly
            // selected workspace (draft is tied to the workspace at the time
            // keystrokes arrived, not to wherever the cursor ends up).
            app.dashboard.reply_draft.clear();
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
            let max = app.selectable.len().saturating_sub(1);
            app.dashboard.selected = if app.dashboard.selected >= max {
                0
            } else {
                app.dashboard.selected + 1
            };
            // Clear any in-flight reply draft (same rationale as Up/k above).
            app.dashboard.reply_draft.clear();
        }
```

Replace the index assignments with `select_index` so `selection` stays in sync:

```rust
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
            let max = app.selectable.len().saturating_sub(1);
            let idx = if app.dashboard.selected == 0 {
                max
            } else {
                app.dashboard.selected - 1
            };
            app.select_index(idx);
            // Clear any in-flight reply draft so it can't leak to the newly
            // selected workspace (draft is tied to the workspace at the time
            // keystrokes arrived, not to wherever the cursor ends up).
            app.dashboard.reply_draft.clear();
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
            let max = app.selectable.len().saturating_sub(1);
            let idx = if app.dashboard.selected >= max {
                0
            } else {
                app.dashboard.selected + 1
            };
            app.select_index(idx);
            // Clear any in-flight reply draft (same rationale as Up/k above).
            app.dashboard.reply_draft.clear();
        }
```

- [ ] **Step 2: Update the create-landing site**

In `src/app.rs`, the current block (around lines 1422–1425) reads:

```rust
                if let Some(idx) = g
                    .selectable
                    .iter()
                    .position(|t| *t == SelectionTarget::Workspace(id))
                {
                    g.dashboard.selected = idx;
                }
```

Replace with:

```rust
                if let Some(idx) = g
                    .selectable
                    .iter()
                    .position(|t| *t == SelectionTarget::Workspace(id))
                {
                    g.select_index(idx);
                }
```

- [ ] **Step 3: Update the repo-move site (consistency)**

In `src/app/input.rs` lines 270–278, the current block reads:

```rust
    // Anchor the cursor to the repo we just moved.
    if let Some(idx) = app
        .selectable
        .iter()
        .position(|t| *t == SelectionTarget::Repo(rid))
    {
        app.dashboard.selected = idx;
    }
    app.dashboard.selection = Some(SelectionTarget::Repo(rid));
```

Replace with:

```rust
    // Anchor the cursor to the repo we just moved.
    if let Some(idx) = app
        .selectable
        .iter()
        .position(|t| *t == SelectionTarget::Repo(rid))
    {
        app.select_index(idx);
    }
```

- [ ] **Step 4: Run the input + app test suites**

Run: `cargo test -p wsx 2>&1 | tail -30`
Expected: PASS. The existing nav tests (`src/app/input_tests.rs`) set `app.dashboard.selected` directly and read it back — they still pass because `select_index` writes `selected`. If any test reads `selected_target()` after manually setting only `selected` (without `selection`), update it to use `select_index` or set `selection` too (cite the test name).

- [ ] **Step 5: Commit**

```bash
git add src/app/input.rs src/app.rs
git commit -m "refactor(app): route selection changes through select_index (#168)"
```

---

## Task 5: Verify, lint, format

**Files:** none (verification only).

- [ ] **Step 1: Full test suite**

Run: `cargo test -p wsx 2>&1 | tail -30`
Expected: all pass.

- [ ] **Step 2: Clippy**

Run: `cargo clippy -p wsx --all-targets 2>&1 | tail -30`
Expected: no new warnings in `src/app.rs`, `src/app/render.rs`, `src/app/input.rs`, `src/ui/dashboard/mod.rs`.

- [ ] **Step 3: Format**

Run: `cargo fmt`
Then: `git diff --stat`
Expected: only whitespace/formatting touch-ups, if any.

- [ ] **Step 4: Manual smoke (optional, documented)**

If running the TUI: select a Complete workspace `B` that shares a repo with one active workspace `A`. Let `A` finish so the repo auto-folds. Confirm: (a) selection does **not** jump to another workspace; (b) when the repo re-expands (e.g. `A` resumes or you press `l`/unfold), the highlight returns to `B`. The acceptance criterion is "selection stays anchored to the same `WorkspaceId`".

- [ ] **Step 5: Commit any fmt changes**

```bash
git add -A
git commit -m "style: cargo fmt" || echo "nothing to format"
```

---

## Self-Review notes

- **Spec coverage:** Model inversion (Task 2), pure `reconcile_selection` with all four branches (Task 1), draw-loop wiring + removal of the old re-derive line (Task 3), sync points nav/create/repo-move (Task 4), tests #1–#5 from the spec (Task 1) plus helper tests (Task 2). Spec tests #6/#7 (full fold integration) are covered in behavior by the pure-function park/restore tests plus the `selected_target` durability test; a heavier App-level fold scenario is left to the optional manual smoke (Step 5, Task 5) to avoid brittle session/status fixture setup.
- **Type consistency:** `reconcile_selection(Option<SelectionTarget>, usize, &[SelectionTarget], impl Fn(SelectionTarget) -> bool) -> (Option<SelectionTarget>, usize)` is used identically in Task 1 (def), Task 1 tests, and Task 3 (call). `select_index(usize)` and `selection_target_exists(SelectionTarget) -> bool` match between Task 2 (def) and Tasks 3–4 (calls).
- **No placeholders:** every code step shows the full before/after text.
