# Workspace layout persistence — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let users park a multi-pane attached view back to the dashboard with `Ctrl-x Esc`, persist the split tree per anchor workspace in SQLite, auto-restore on dashboard `Enter`, and show a codicon `split_horizontal` badge on dashboard rows whose saved layout has more than one pane.

**Architecture:** Add a `workspace_layouts` table keyed by anchor `WorkspaceId` (CASCADE on workspace delete). Reuse the existing pure-data `SplitTree` (serialized as JSON) and add a `prune` method that mirrors the existing collapse logic in `SplitTree::close` to handle stale leaves. New chord in `handle_key_attached` saves; replacement helper in the dashboard-Enter branch restores. Dashboard indicator is driven by a `HashSet<WorkspaceId>` cache on `App`, recomputed in `App::refresh()` (which is already called on every external/internal change).

**Tech Stack:** Rust, rusqlite, serde, serde_json, ratatui, crossterm. Existing wsx codebase.

**Spec:** `docs/superpowers/specs/2026-05-23-workspace-layout-persistence-design.md`.

---

## File touch summary

- **Create:** `docs/manual-tests/workspace-layout-persistence.md`
- **Modify:** `src/store.rs` — migration v10, 4 new methods, `WorkspaceId` becomes `#[serde(transparent)]` Serialize/Deserialize.
- **Modify:** `src/ui/split.rs` — serde derives on `SplitTree` and `SplitDirection`, new `prune` method + `PruneOutcome` enum, `first_leaf_path` promoted to pub.
- **Modify:** `src/app.rs` — new `App::workspaces_with_multi_pane_layouts` field, recompute in `refresh()`, new `Ctrl-x Esc` chord arm, new `save_layout` + `restore_attached_state` helpers, dashboard-Enter branch updated.
- **Modify:** `src/ui/dashboard/row.rs` — `RowInputs.has_multi_pane_layout` field, badge render, name-width math.
- **Modify:** `src/ui/dashboard/by_attention.rs`, `src/ui/dashboard/by_repo.rs`, `src/ui/dashboard/tests.rs`, `src/app.rs:727` — pass new `RowInputs` field at every construction site.

---

## Task 1: Add serde derives to `WorkspaceId`, `SplitTree`, `SplitDirection`

**Files:**
- Modify: `src/store.rs` (around line 11 — `WorkspaceId` definition)
- Modify: `src/ui/split.rs` (lines 9-34 — `SplitDirection`, `SplitTree`)
- Test: `src/ui/split.rs` (tests module at bottom)

- [ ] **Step 1: Write the failing round-trip test**

Add at the end of the `tests` module in `src/ui/split.rs`:

```rust
#[test]
fn splittree_serde_round_trip_preserves_nested_structure() {
    let mut tree = SplitTree::Leaf(WorkspaceId(1));
    tree.split(&[], SplitDirection::Vertical, WorkspaceId(2)).unwrap();
    tree.split(&[1], SplitDirection::Horizontal, WorkspaceId(3)).unwrap();
    let json = serde_json::to_string(&tree).expect("serialize");
    let back: SplitTree = serde_json::from_str(&json).expect("deserialize");
    // Structural equality via layout output (same leaves in same order in
    // same rects).
    let a = tree.layout(Rect::new(0, 0, 80, 24));
    let b = back.layout(Rect::new(0, 0, 80, 24));
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(b.iter()) {
        assert_eq!(x.0, y.0, "leaf id");
        assert_eq!(x.1, y.1, "focus path");
        assert_eq!(x.2, y.2, "rect");
    }
}

#[test]
fn workspaceid_serializes_as_bare_integer() {
    let id = crate::store::WorkspaceId(42);
    assert_eq!(serde_json::to_string(&id).unwrap(), "42");
    let back: crate::store::WorkspaceId = serde_json::from_str("42").unwrap();
    assert_eq!(back, id);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p wsx --lib ui::split::tests::splittree_serde_round_trip_preserves_nested_structure ui::split::tests::workspaceid_serializes_as_bare_integer`
Expected: FAIL with "the trait `Serialize` is not implemented" or similar.

- [ ] **Step 3: Add `#[derive(Serialize, Deserialize)]` plus `#[serde(transparent)]` to `WorkspaceId`**

In `src/store.rs`, change the `WorkspaceId` definition (around line 11):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct WorkspaceId(pub i64);
```

(Keep all existing derives; only add the serde ones.)

- [ ] **Step 4: Add `#[derive(Serialize, Deserialize)]` to `SplitDirection` and `SplitTree`**

In `src/ui/split.rs`, update the enum definitions (lines 9-17, 27-34):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SplitDirection {
    Vertical,
    Horizontal,
}
```

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SplitTree {
    Leaf(WorkspaceId),
    Split {
        direction: SplitDirection,
        children: Vec<SplitTree>,
    },
}
```

(Leave the rest of the file untouched.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p wsx --lib ui::split::tests::splittree_serde_round_trip_preserves_nested_structure ui::split::tests::workspaceid_serializes_as_bare_integer`
Expected: PASS.

- [ ] **Step 6: Run the full test suite to catch regressions**

Run: `cargo test -p wsx --lib`
Expected: all existing tests still pass.

- [ ] **Step 7: Commit**

```bash
git add src/store.rs src/ui/split.rs
git commit -m "feat(layout): add serde derives to SplitTree, SplitDirection, WorkspaceId"
```

---

## Task 2: Add `SplitTree::prune` and `PruneOutcome`

**Files:**
- Modify: `src/ui/split.rs` (add `PruneOutcome` enum next to `CloseOutcome`; add `prune` method on `SplitTree`)
- Test: `src/ui/split.rs` (tests module)

- [ ] **Step 1: Write the failing tests**

Add at the end of the `tests` module in `src/ui/split.rs`:

```rust
#[test]
fn prune_removes_dropped_leaves_and_collapses_singletons() {
    // (A | B | C), prune B → (A | C)
    let mut tree = SplitTree::Leaf(wid(1));
    tree.split(&[], SplitDirection::Vertical, wid(2));
    tree.split(&[1], SplitDirection::Vertical, wid(3));
    let outcome = tree.prune(|id| id != wid(2));
    assert!(matches!(outcome, PruneOutcome::Kept));
    assert_eq!(tree.leaves(), vec![wid(1), wid(3)]);
}

#[test]
fn prune_collapses_nested_singleton() {
    // (A | (B / C)) — prune C — expect (A | B), with the nested split
    // collapsed (no 1-child Split allowed).
    let mut tree = SplitTree::Leaf(wid(1));
    tree.split(&[], SplitDirection::Vertical, wid(2));
    tree.split(&[1], SplitDirection::Horizontal, wid(3));
    let outcome = tree.prune(|id| id != wid(3));
    assert!(matches!(outcome, PruneOutcome::Kept));
    assert_eq!(tree.leaves(), vec![wid(1), wid(2)]);
    // Walk: every Split must have ≥ 2 children.
    fn no_singleton_splits(t: &SplitTree) {
        if let SplitTree::Split { children, .. } = t {
            assert!(children.len() >= 2, "found singleton split");
            for c in children { no_singleton_splits(c); }
        }
    }
    no_singleton_splits(&tree);
}

#[test]
fn prune_returns_empty_when_no_leaves_survive() {
    let mut tree = SplitTree::Leaf(wid(1));
    tree.split(&[], SplitDirection::Vertical, wid(2));
    let outcome = tree.prune(|_| false);
    assert!(matches!(outcome, PruneOutcome::Empty));
}

#[test]
fn prune_keeps_leaf_when_predicate_true() {
    let mut tree = SplitTree::Leaf(wid(1));
    let outcome = tree.prune(|_| true);
    assert!(matches!(outcome, PruneOutcome::Kept));
    assert_eq!(tree.leaves(), vec![wid(1)]);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p wsx --lib ui::split::tests::prune`
Expected: FAIL with "no method named `prune`" / "cannot find `PruneOutcome`".

- [ ] **Step 3: Implement `PruneOutcome` and `prune`**

Add to `src/ui/split.rs`, right after the existing `CloseOutcome` enum (around line 46):

```rust
/// What `prune` produced.
pub enum PruneOutcome {
    /// At least one leaf survived; the tree is still well-formed (no
    /// 1-child Splits).
    Kept,
    /// No leaf survived; caller should treat this tree as gone.
    Empty,
}
```

Add a method on `SplitTree` (place it next to `close`, around line 265):

```rust
/// Drop every leaf whose `keep(id)` returns false. After pruning,
/// any `Split` that ends up with a single child is collapsed into
/// that child (matches the invariant maintained by `close`).
pub fn prune<F: Fn(WorkspaceId) -> bool>(&mut self, keep: &F) -> PruneOutcome {
    match self {
        SplitTree::Leaf(id) => {
            if keep(*id) {
                PruneOutcome::Kept
            } else {
                PruneOutcome::Empty
            }
        }
        SplitTree::Split { children, .. } => {
            // Recurse and drop empty children in-place.
            let mut i = 0;
            while i < children.len() {
                match children[i].prune(keep) {
                    PruneOutcome::Kept => i += 1,
                    PruneOutcome::Empty => {
                        children.remove(i);
                    }
                }
            }
            if children.is_empty() {
                PruneOutcome::Empty
            } else if children.len() == 1 {
                // Collapse single-child split into the child.
                let only = children.remove(0);
                *self = only;
                PruneOutcome::Kept
            } else {
                PruneOutcome::Kept
            }
        }
    }
}
```

Note the `&F` parameter: borrowing the predicate (rather than moving it) makes the recursive call type-check cleanly without `Fn + Copy` constraints.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p wsx --lib ui::split::tests::prune`
Expected: all 4 prune tests PASS.

- [ ] **Step 5: Run the full split-module tests to catch invariant regressions**

Run: `cargo test -p wsx --lib ui::split`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add src/ui/split.rs
git commit -m "feat(layout): add SplitTree::prune to drop stale leaves and collapse singleton splits"
```

---

## Task 3: Promote `first_leaf_path` to a public method

**Files:**
- Modify: `src/ui/split.rs` (around line 362 — the free `first_leaf_path` helper)

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/ui/split.rs`:

```rust
#[test]
fn first_leaf_path_returns_empty_for_leaf_root() {
    let tree = SplitTree::Leaf(wid(1));
    assert_eq!(tree.first_leaf_path(), Vec::<usize>::new());
}

#[test]
fn first_leaf_path_walks_to_leftmost_leaf_of_nested_splits() {
    // (A | (B / C)) — first leaf path is [0] (A).
    let mut tree = SplitTree::Leaf(wid(1));
    tree.split(&[], SplitDirection::Vertical, wid(2));
    tree.split(&[1], SplitDirection::Horizontal, wid(3));
    assert_eq!(tree.first_leaf_path(), vec![0]);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p wsx --lib ui::split::tests::first_leaf_path`
Expected: FAIL with "no method named `first_leaf_path`".

- [ ] **Step 3: Add the public method on `SplitTree`**

Add to `impl SplitTree` (next to `leaves`, around line 144):

```rust
/// Path from the root to the first (leftmost, depth-first) leaf.
/// For a `Leaf` root this returns an empty path.
pub fn first_leaf_path(&self) -> FocusPath {
    let mut out = Vec::new();
    let mut node = self;
    loop {
        match node {
            SplitTree::Leaf(_) => return out,
            SplitTree::Split { children, .. } => {
                if children.is_empty() {
                    return out;
                }
                out.push(0);
                node = &children[0];
            }
        }
    }
}
```

(Leave the existing private `first_leaf_path` free function in place — it is still used by `SplitTree::close`. The two implementations are independent and the duplication is small.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p wsx --lib ui::split::tests::first_leaf_path`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ui/split.rs
git commit -m "feat(layout): expose SplitTree::first_leaf_path for restore use"
```

---

## Task 4: Migration v10 + `workspace_layouts` table

**Files:**
- Modify: `src/store.rs` (migration block around lines 93-204; schema constants at bottom of file)
- Test: `src/store.rs` (tests module)

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/store.rs` (find it via `#[cfg(test)] mod tests`):

```rust
#[test]
fn migration_v10_creates_workspace_layouts_table() {
    let store = Store::open_in_memory().unwrap();
    let v: i64 = store
        .conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert!(v >= 10, "user_version should be at least 10, got {v}");
    let count: i64 = store
        .conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='workspace_layouts'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "workspace_layouts table should exist");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p wsx --lib store::tests::migration_v10_creates_workspace_layouts_table`
Expected: FAIL with `user_version should be at least 10, got 9` (or table not found).

- [ ] **Step 3: Add the schema constant**

At the bottom of `src/store.rs`, add (place after the existing `SCHEMA_V8_ACTIVITY_BUCKETS` constant):

```rust
const SCHEMA_V10_WORKSPACE_LAYOUTS: &str = "
CREATE TABLE IF NOT EXISTS workspace_layouts (
    anchor_workspace_id INTEGER PRIMARY KEY
        REFERENCES workspaces(id) ON DELETE CASCADE,
    tree_json TEXT NOT NULL,
    focus_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
";
```

- [ ] **Step 4: Add the migration step**

In `Store::migrate` in `src/store.rs`, add a new block after the existing `if v < 9 { … }` block (around line 202):

```rust
if v < 10 {
    self.conn.execute_batch(SCHEMA_V10_WORKSPACE_LAYOUTS)?;
    self.conn.execute("PRAGMA user_version = 10", [])?;
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p wsx --lib store::tests::migration_v10_creates_workspace_layouts_table`
Expected: PASS.

- [ ] **Step 6: Run the existing idempotency test to confirm we didn't break replay**

Run: `cargo test -p wsx --lib store::tests::open_in_memory_runs_migrations_idempotently`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/store.rs
git commit -m "feat(layout): add migration v10 creating workspace_layouts table"
```

---

## Task 5: `Store::set_workspace_layout` / `get_workspace_layout` / `delete_workspace_layout`

**Files:**
- Modify: `src/store.rs` (new methods in the `impl Store` block; new imports at top)
- Test: `src/store.rs` (tests module)

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/store.rs`:

```rust
#[test]
fn set_then_get_workspace_layout_round_trips() {
    use crate::ui::split::{SplitDirection, SplitTree};
    let store = Store::open_in_memory().unwrap();
    let repo = store.add_repo(std::path::Path::new("/r"), "r", "x").unwrap();
    let id = store
        .insert_workspace(&NewWorkspace {
            repo_id: repo,
            name: "a",
            branch: "x/a",
            worktree_path: std::path::Path::new("/r/a"),
            state: WorkspaceState::Live,
            yolo: false,
            agent: "claude",
        })
        .unwrap();
    let mut tree = SplitTree::Leaf(id);
    tree.split(&[], SplitDirection::Vertical, id);
    let focus = vec![1];
    store.set_workspace_layout(id, &tree, &focus).unwrap();
    let got = store.get_workspace_layout(id).unwrap().expect("layout present");
    assert_eq!(got.0.leaves().len(), 2);
    assert_eq!(got.1, focus);
}

#[test]
fn get_workspace_layout_returns_none_when_absent() {
    let store = Store::open_in_memory().unwrap();
    assert!(store.get_workspace_layout(WorkspaceId(999)).unwrap().is_none());
}

#[test]
fn archiving_workspace_cascades_to_layout_row() {
    use crate::ui::split::SplitTree;
    let store = Store::open_in_memory().unwrap();
    let repo = store.add_repo(std::path::Path::new("/r"), "r", "x").unwrap();
    let id = store
        .insert_workspace(&NewWorkspace {
            repo_id: repo,
            name: "a",
            branch: "x/a",
            worktree_path: std::path::Path::new("/r/a"),
            state: WorkspaceState::Live,
            yolo: false,
            agent: "claude",
        })
        .unwrap();
    store
        .set_workspace_layout(id, &SplitTree::Leaf(id), &[])
        .unwrap();
    store.delete_workspace(id).unwrap();
    assert!(store.get_workspace_layout(id).unwrap().is_none());
}

#[test]
fn set_workspace_layout_replaces_existing() {
    use crate::ui::split::{SplitDirection, SplitTree};
    let store = Store::open_in_memory().unwrap();
    let repo = store.add_repo(std::path::Path::new("/r"), "r", "x").unwrap();
    let id = store
        .insert_workspace(&NewWorkspace {
            repo_id: repo,
            name: "a",
            branch: "x/a",
            worktree_path: std::path::Path::new("/r/a"),
            state: WorkspaceState::Live,
            yolo: false,
            agent: "claude",
        })
        .unwrap();
    let single = SplitTree::Leaf(id);
    let mut pair = SplitTree::Leaf(id);
    pair.split(&[], SplitDirection::Vertical, id);
    store.set_workspace_layout(id, &single, &[]).unwrap();
    store.set_workspace_layout(id, &pair, &[1]).unwrap();
    let got = store.get_workspace_layout(id).unwrap().unwrap();
    assert_eq!(got.0.leaves().len(), 2, "second write wins");
}

#[test]
fn get_workspace_layout_returns_none_on_corrupted_json_and_deletes_row() {
    let store = Store::open_in_memory().unwrap();
    let repo = store.add_repo(std::path::Path::new("/r"), "r", "x").unwrap();
    let id = store
        .insert_workspace(&NewWorkspace {
            repo_id: repo,
            name: "a",
            branch: "x/a",
            worktree_path: std::path::Path::new("/r/a"),
            state: WorkspaceState::Live,
            yolo: false,
            agent: "claude",
        })
        .unwrap();
    // Insert garbage directly.
    store
        .conn
        .execute(
            "INSERT INTO workspace_layouts (anchor_workspace_id, tree_json, focus_json, updated_at)
             VALUES (?1, 'not-json', '[]', 0)",
            [id.0],
        )
        .unwrap();
    assert!(store.get_workspace_layout(id).unwrap().is_none());
    // Row should have been deleted on read.
    let count: i64 = store
        .conn
        .query_row(
            "SELECT count(*) FROM workspace_layouts WHERE anchor_workspace_id = ?1",
            [id.0],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "corrupt row deleted on read");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p wsx --lib store::tests::set_then_get_workspace_layout_round_trips store::tests::get_workspace_layout_returns_none_when_absent store::tests::archiving_workspace_cascades_to_layout_row store::tests::set_workspace_layout_replaces_existing store::tests::get_workspace_layout_returns_none_on_corrupted_json_and_deletes_row`
Expected: FAIL with "no method named `set_workspace_layout`" etc.

- [ ] **Step 3: Implement the three methods**

In `src/store.rs`, inside `impl Store` (next to existing workspace methods, around line 419), add:

```rust
pub fn set_workspace_layout(
    &self,
    anchor: WorkspaceId,
    tree: &crate::ui::split::SplitTree,
    focus: &[usize],
) -> Result<()> {
    // SplitTree and Vec<usize> have no exotic Serialize impls (no maps
    // with non-string keys, no floats, no custom serializers) — calling
    // `to_string` on them is infallible in practice. `expect` makes that
    // explicit; we don't add a new Error variant just for an
    // impossible branch.
    let tree_json = serde_json::to_string(tree).expect("SplitTree serialize is infallible");
    let focus_json = serde_json::to_string(focus).expect("FocusPath serialize is infallible");
    self.conn.execute(
        "INSERT OR REPLACE INTO workspace_layouts
            (anchor_workspace_id, tree_json, focus_json, updated_at)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![anchor.0, tree_json, focus_json, now_ms()],
    )?;
    Ok(())
}

pub fn get_workspace_layout(
    &self,
    anchor: WorkspaceId,
) -> Result<Option<(crate::ui::split::SplitTree, Vec<usize>)>> {
    let row: Option<(String, String)> = self
        .conn
        .query_row(
            "SELECT tree_json, focus_json FROM workspace_layouts WHERE anchor_workspace_id = ?1",
            [anchor.0],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((tree_json, focus_json)) = row else {
        return Ok(None);
    };
    match (
        serde_json::from_str::<crate::ui::split::SplitTree>(&tree_json),
        serde_json::from_str::<Vec<usize>>(&focus_json),
    ) {
        (Ok(tree), Ok(focus)) => Ok(Some((tree, focus))),
        _ => {
            tracing::warn!(
                ?anchor,
                "workspace_layouts row failed to parse; deleting corrupt entry"
            );
            self.conn.execute(
                "DELETE FROM workspace_layouts WHERE anchor_workspace_id = ?1",
                [anchor.0],
            )?;
            Ok(None)
        }
    }
}

pub fn delete_workspace_layout(&self, anchor: WorkspaceId) -> Result<()> {
    self.conn.execute(
        "DELETE FROM workspace_layouts WHERE anchor_workspace_id = ?1",
        [anchor.0],
    )?;
    Ok(())
}
```

If `rusqlite::OptionalExtension` isn't already imported in this file, add `use rusqlite::OptionalExtension;` at the top of `src/store.rs` alongside the existing imports. Check with `grep -n "^use rusqlite" src/store.rs`; if absent, add it.

The error type (`crate::error::Error`, see `src/error.rs`) has `Store(rusqlite::Error)` and `Io(std::io::Error)` From impls — those are what `?` falls into for the `conn.execute` and `query_row` calls. Serialization is handled with `.expect` per the rationale above. No new Error variant is needed.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p wsx --lib store::tests::set_then_get_workspace_layout_round_trips store::tests::get_workspace_layout_returns_none_when_absent store::tests::archiving_workspace_cascades_to_layout_row store::tests::set_workspace_layout_replaces_existing store::tests::get_workspace_layout_returns_none_on_corrupted_json_and_deletes_row`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add src/store.rs
git commit -m "feat(layout): add Store CRUD for workspace_layouts with corruption-safe reads"
```

---

## Task 6: `Store::list_multi_pane_layout_anchors`

**Files:**
- Modify: `src/store.rs`
- Test: `src/store.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module:

```rust
#[test]
fn list_multi_pane_layout_anchors_returns_only_multi_leaf_layouts() {
    use crate::ui::split::{SplitDirection, SplitTree};
    let store = Store::open_in_memory().unwrap();
    let repo = store.add_repo(std::path::Path::new("/r"), "r", "x").unwrap();
    let a = store
        .insert_workspace(&NewWorkspace {
            repo_id: repo,
            name: "a",
            branch: "x/a",
            worktree_path: std::path::Path::new("/r/a"),
            state: WorkspaceState::Live,
            yolo: false,
            agent: "claude",
        })
        .unwrap();
    let b = store
        .insert_workspace(&NewWorkspace {
            repo_id: repo,
            name: "b",
            branch: "x/b",
            worktree_path: std::path::Path::new("/r/b"),
            state: WorkspaceState::Live,
            yolo: false,
            agent: "claude",
        })
        .unwrap();
    // a: single-leaf layout (should NOT appear).
    store
        .set_workspace_layout(a, &SplitTree::Leaf(a), &[])
        .unwrap();
    // b: two-leaf layout (should appear).
    let mut pair = SplitTree::Leaf(b);
    pair.split(&[], SplitDirection::Vertical, a);
    store.set_workspace_layout(b, &pair, &[1]).unwrap();
    let got = store.list_multi_pane_layout_anchors().unwrap();
    assert_eq!(got, vec![b], "only multi-pane anchors returned");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p wsx --lib store::tests::list_multi_pane_layout_anchors_returns_only_multi_leaf_layouts`
Expected: FAIL with "no method named `list_multi_pane_layout_anchors`".

- [ ] **Step 3: Implement the method**

Add to `impl Store` in `src/store.rs`:

```rust
pub fn list_multi_pane_layout_anchors(&self) -> Result<Vec<WorkspaceId>> {
    let mut stmt = self
        .conn
        .prepare("SELECT anchor_workspace_id, tree_json FROM workspace_layouts ORDER BY anchor_workspace_id")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = Vec::new();
    for row in rows {
        let (anchor, tree_json) = row?;
        if let Ok(tree) = serde_json::from_str::<crate::ui::split::SplitTree>(&tree_json) {
            if tree.leaves().len() > 1 {
                out.push(WorkspaceId(anchor));
            }
        }
    }
    Ok(out)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p wsx --lib store::tests::list_multi_pane_layout_anchors_returns_only_multi_leaf_layouts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/store.rs
git commit -m "feat(layout): add Store::list_multi_pane_layout_anchors"
```

---

## Task 7: Add `App::workspaces_with_multi_pane_layouts` and refresh hook

**Files:**
- Modify: `src/app.rs` (App struct definition, `App::new`, `App::refresh`)

- [ ] **Step 1: Write the failing test**

Find the `#[cfg(test)] mod tests` block at the bottom of `src/app.rs` and add:

```rust
#[tokio::test]
async fn app_refresh_populates_layout_indicator_cache_from_store() {
    use crate::ui::split::{SplitDirection, SplitTree};
    let tmp = tempfile::tempdir().unwrap();
    let store = crate::store::Store::open_in_memory().unwrap();
    let repo = store
        .add_repo(tmp.path(), "r", "x")
        .unwrap();
    let a = store
        .insert_workspace(&crate::store::NewWorkspace {
            repo_id: repo,
            name: "a",
            branch: "x/a",
            worktree_path: tmp.path(),
            state: crate::store::WorkspaceState::Live,
            yolo: false,
            agent: "claude",
        })
        .unwrap();
    let mut pair = SplitTree::Leaf(a);
    pair.split(&[], SplitDirection::Vertical, a);
    store.set_workspace_layout(a, &pair, &[1]).unwrap();
    let mut app = App::new(store, tmp.path().to_path_buf()).unwrap();
    assert!(
        app.workspaces_with_multi_pane_layouts.contains(&a),
        "cache should contain anchor with multi-pane layout"
    );
    // Single-pane layouts should NOT be in the cache.
    app.store
        .set_workspace_layout(a, &SplitTree::Leaf(a), &[])
        .unwrap();
    app.refresh().unwrap();
    assert!(!app.workspaces_with_multi_pane_layouts.contains(&a));
}
```

(If `tempfile` isn't already imported at the test scope, the existing tests show the import pattern — see other `#[tokio::test]` cases in `src/app.rs`.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p wsx --lib app::tests::app_refresh_populates_layout_indicator_cache_from_store`
Expected: FAIL with "no field `workspaces_with_multi_pane_layouts`".

- [ ] **Step 3: Add the field on the `App` struct**

Locate the `App` struct (around line 280 in `src/app.rs`). Add a new field:

```rust
/// Anchors whose saved layout has more than one pane. Used by the
/// dashboard to render the split-layout indicator. Recomputed by
/// `App::refresh`.
pub workspaces_with_multi_pane_layouts: std::collections::HashSet<crate::store::WorkspaceId>,
```

- [ ] **Step 4: Initialize the field in `App::new`**

In `App::new` (around line 291), add the field to the struct literal:

```rust
workspaces_with_multi_pane_layouts: std::collections::HashSet::new(),
```

Place it next to other `HashSet`/`HashMap` initializations (e.g., near `workspace_needs_attention`).

- [ ] **Step 5: Recompute in `App::refresh`**

At the end of `App::refresh` (just before `Ok(())` on line 388), add:

```rust
self.workspaces_with_multi_pane_layouts = self
    .store
    .list_multi_pane_layout_anchors()
    .unwrap_or_default()
    .into_iter()
    .collect();
```

(Tolerant of store error — the cache will simply be empty if the query fails. Dashboard renders no badges in that case, which is acceptable.)

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test -p wsx --lib app::tests::app_refresh_populates_layout_indicator_cache_from_store`
Expected: PASS.

- [ ] **Step 7: Run the full app-test suite to catch regressions**

Run: `cargo test -p wsx --lib app::tests`
Expected: all PASS.

- [ ] **Step 8: Commit**

```bash
git add src/app.rs
git commit -m "feat(layout): populate workspaces_with_multi_pane_layouts cache in App::refresh"
```

---

## Task 8: Add `Ctrl-x Esc` chord and `save_layout` helper

**Files:**
- Modify: `src/app.rs` (`handle_key_attached` leader-pending arm; new `save_layout` helper)

- [ ] **Step 1: Write the failing test**

In the `tests` module of `src/app.rs`, find an existing helper like `press_key` or look at `esc_returns_focus_to_dashboard` (around line 3120) for the test-setup pattern. Add a new test:

```rust
#[tokio::test]
async fn ctrl_x_esc_saves_layout_and_returns_to_dashboard() {
    use crate::ui::split::{AttachedState, SplitDirection};
    // Reuse the existing two-workspace setup helper if one exists in
    // this test module; otherwise, use the same scaffolding as
    // `updates_panel_v_splits_attached_view_vertically` (line 3473).
    let (mut app, first_id, second_id) = setup_two_workspaces_with_sessions().await;
    let mut state = AttachedState::single(first_id);
    state.split(SplitDirection::Vertical, second_id);
    app.view = crate::ui::View::Attached(state);
    // Drive the chord: Ctrl-x, then Esc.
    handle_key_attached(
        &mut app,
        first_id,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();
    handle_key_attached(
        &mut app,
        first_id,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(matches!(app.view, crate::ui::View::Dashboard));
    let saved = app.store.get_workspace_layout(first_id).unwrap();
    assert!(saved.is_some(), "layout should be saved under first leaf");
    let (tree, _) = saved.unwrap();
    assert_eq!(tree.leaves(), vec![first_id, second_id]);
    assert!(app.workspaces_with_multi_pane_layouts.contains(&first_id));
}
```

If the test module doesn't already have a `setup_two_workspaces_with_sessions` helper, model the setup on the existing test `updates_panel_v_splits_attached_view_vertically` at `src/app.rs:3473` — it already creates two workspaces and spawns sessions.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p wsx --lib app::tests::ctrl_x_esc_saves_layout_and_returns_to_dashboard`
Expected: FAIL — likely with "no layout saved" because `Ctrl-x Esc` does nothing yet.

- [ ] **Step 3: Add the `save_layout` helper**

In `src/app.rs`, add a free function (place near `handle_key_attached`):

```rust
fn save_layout(app: &mut App, state: &crate::ui::AttachedState) {
    let Some(anchor) = state.leaves().first().copied() else {
        return;
    };
    if let Err(e) = app.store.set_workspace_layout(anchor, &state.tree, &state.focus) {
        tracing::warn!(error = %e, "failed to save workspace layout");
    }
    // Recompute the dashboard indicator cache so the badge updates
    // immediately when the user returns to the dashboard.
    let _ = app.refresh();
}
```

- [ ] **Step 4: Add the `Ctrl-x Esc` arm in `handle_key_attached`**

In `handle_key_attached` (around line 1985 — start of the `if app.leader_pending { app.leader_pending = false; match k.code { … } }` block), add a new arm. Place it after the `KeyCode::Char('d')` arm (which ends around line 2006):

```rust
KeyCode::Esc => {
    if let View::Attached(state) = &app.view {
        save_layout_for(app, state.clone());
    }
    app.view = View::Dashboard;
    return Ok(());
}
```

Implementation note on the borrow: `save_layout` needs `&mut app` while reading from `app.view`. Two options: (a) clone the `AttachedState` (cheap — just two `Vec`s and a small tree), or (b) take the tree/focus by value before borrowing app mutably. The plan uses (a) via `state.clone()`. Rename the helper to `save_layout_for(app: &mut App, state: crate::ui::AttachedState)` to take by value:

```rust
fn save_layout_for(app: &mut App, state: crate::ui::AttachedState) {
    let Some(anchor) = state.leaves().first().copied() else {
        return;
    };
    if let Err(e) = app.store.set_workspace_layout(anchor, &state.tree, &state.focus) {
        tracing::warn!(error = %e, "failed to save workspace layout");
    }
    let _ = app.refresh();
}
```

(Use this signature instead of the one in Step 3 — update the helper accordingly.)

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p wsx --lib app::tests::ctrl_x_esc_saves_layout_and_returns_to_dashboard`
Expected: PASS.

- [ ] **Step 6: Run the broader attached-view test suite to catch regressions**

Run: `cargo test -p wsx --lib app::tests`
Expected: all PASS (existing `Ctrl-x d` behavior unchanged; existing `esc_returns_focus_to_dashboard` still passes).

- [ ] **Step 7: Commit**

```bash
git add src/app.rs
git commit -m "feat(layout): add Ctrl-x Esc chord to park layout before returning to dashboard"
```

---

## Task 9: Add `restore_attached_state` and wire into dashboard `Enter`

**Files:**
- Modify: `src/app.rs` (around `src/app.rs:1525-1534`, the `SelectionTarget::Workspace` branch of dashboard Enter)

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/app.rs`:

```rust
#[tokio::test]
async fn dashboard_enter_restores_saved_layout() {
    use crate::ui::split::{SplitDirection, SplitTree};
    let (mut app, first_id, second_id) = setup_two_workspaces_with_sessions().await;
    // Save a (first | second) layout under first_id.
    let mut tree = SplitTree::Leaf(first_id);
    tree.split(&[], SplitDirection::Vertical, second_id);
    app.store.set_workspace_layout(first_id, &tree, &[1]).unwrap();
    app.refresh().unwrap();
    // Navigate to first_id and press Enter from the dashboard.
    select_workspace(&mut app, first_id);
    press_key_dashboard(&mut app, KeyCode::Enter).await;
    match &app.view {
        crate::ui::View::Attached(s) => {
            assert_eq!(s.leaves(), vec![first_id, second_id]);
            assert_eq!(s.focus, vec![1]);
        }
        _ => panic!("expected attached view with restored layout"),
    }
}

#[tokio::test]
async fn dashboard_enter_falls_back_to_single_pane_when_no_layout() {
    let (mut app, first_id, _second_id) = setup_two_workspaces_with_sessions().await;
    select_workspace(&mut app, first_id);
    press_key_dashboard(&mut app, KeyCode::Enter).await;
    match &app.view {
        crate::ui::View::Attached(s) => {
            assert_eq!(s.leaves(), vec![first_id]);
        }
        _ => panic!("expected single-pane attached view"),
    }
}

#[tokio::test]
async fn restore_prunes_archived_side_panes() {
    use crate::ui::split::{SplitDirection, SplitTree};
    let (mut app, first_id, second_id) = setup_two_workspaces_with_sessions().await;
    let mut tree = SplitTree::Leaf(first_id);
    tree.split(&[], SplitDirection::Vertical, second_id);
    app.store.set_workspace_layout(first_id, &tree, &[1]).unwrap();
    // Archive second_id (delete from store). Refresh so app.workspaces drops it.
    app.store.delete_workspace(second_id).unwrap();
    app.refresh().unwrap();
    select_workspace(&mut app, first_id);
    press_key_dashboard(&mut app, KeyCode::Enter).await;
    match &app.view {
        crate::ui::View::Attached(s) => {
            assert_eq!(s.leaves(), vec![first_id], "side pane pruned");
        }
        _ => panic!("expected attached view with pruned layout"),
    }
}

#[tokio::test]
async fn ctrl_x_d_does_not_modify_saved_layout() {
    use crate::ui::split::{AttachedState, SplitDirection, SplitTree};
    let (mut app, first_id, second_id) = setup_two_workspaces_with_sessions().await;
    // Pre-save a (first | second) layout.
    let mut tree = SplitTree::Leaf(first_id);
    tree.split(&[], SplitDirection::Vertical, second_id);
    app.store.set_workspace_layout(first_id, &tree, &[1]).unwrap();
    // Build the same attached state in memory.
    let mut state = AttachedState::single(first_id);
    state.split(SplitDirection::Vertical, second_id);
    app.view = crate::ui::View::Attached(state);
    // Close second pane with Ctrl-x d.
    handle_key_attached(&mut app, second_id, KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)).await.unwrap();
    handle_key_attached(&mut app, second_id, KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)).await.unwrap();
    // Close last pane → dashboard.
    handle_key_attached(&mut app, first_id, KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)).await.unwrap();
    handle_key_attached(&mut app, first_id, KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)).await.unwrap();
    assert!(matches!(app.view, crate::ui::View::Dashboard));
    // The stored layout is unchanged.
    let (saved, _) = app.store.get_workspace_layout(first_id).unwrap().unwrap();
    assert_eq!(saved.leaves(), vec![first_id, second_id]);
}
```

If `select_workspace` and `press_key_dashboard` helpers don't already exist in the test module, copy the pattern from existing tests like `updates_panel_v_splits_attached_view_vertically` (line 3473) — they walk the dashboard's `selected` index and dispatch via `handle_key_dashboard`. Add them as local helpers near the top of the test module:

```rust
fn select_workspace(app: &mut App, id: crate::store::WorkspaceId) {
    let idx = app.selectable.iter().position(|t| matches!(t, SelectionTarget::Workspace(w) if *w == id))
        .expect("workspace in selectable list");
    app.dashboard.selected = idx;
}

async fn press_key_dashboard(app: &mut App, code: KeyCode) {
    handle_key_dashboard(app, KeyEvent::new(code, KeyModifiers::NONE)).await.unwrap();
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p wsx --lib app::tests::dashboard_enter_restores_saved_layout app::tests::dashboard_enter_falls_back_to_single_pane_when_no_layout app::tests::restore_prunes_archived_side_panes app::tests::ctrl_x_d_does_not_modify_saved_layout`
Expected: FAIL — restore tests fail because the dashboard always builds `AttachedState::single`.

- [ ] **Step 3: Implement `restore_attached_state`**

In `src/app.rs`, add a free function (place near the dashboard Enter handler):

```rust
fn restore_attached_state(app: &mut App, anchor: crate::store::WorkspaceId) -> crate::ui::AttachedState {
    let Some((mut tree, mut focus)) = app.store.get_workspace_layout(anchor).ok().flatten() else {
        return crate::ui::AttachedState::single(anchor);
    };
    let valid: std::collections::HashSet<_> = app.workspaces.iter().map(|(_, w)| w.id).collect();
    use crate::ui::split::PruneOutcome;
    let outcome = tree.prune(&|id| valid.contains(&id));
    match outcome {
        PruneOutcome::Empty => {
            let _ = app.store.delete_workspace_layout(anchor);
            let _ = app.refresh();
            crate::ui::AttachedState::single(anchor)
        }
        PruneOutcome::Kept => {
            if tree.leaf_at(&focus).is_none() {
                focus = tree.first_leaf_path();
            }
            // Spawn any missing sessions for the side panes. Anchor's
            // session was already spawned by the caller. Skip on failure
            // and continue with the remaining panes — partial restore
            // is better than no restore.
            for leaf_id in tree.leaves() {
                if leaf_id == anchor || app.sessions.get(leaf_id).is_some() {
                    continue;
                }
                if let Some((sid, sp, mode, repo_path, agent)) = build_spawn_info(app, leaf_id) {
                    maybe_mirror_mcp(app, &repo_path, &sp);
                    let remote = crate::remote_control::RemoteOpts::from_store(&app.store);
                    let _ = app.sessions.spawn(sid, &sp, 80, 24, mode, remote, agent);
                }
            }
            crate::ui::AttachedState { tree, focus }
        }
    }
}
```

- [ ] **Step 4: Wire it into the dashboard Enter handler**

In `handle_key_dashboard`, find the `Some(SelectionTarget::Workspace(id))` branch (around line 1525-1534):

```rust
Some(SelectionTarget::Workspace(id)) => {
    app.workspace_needs_attention.remove(&id);
    if let Some((id, path, mode, repo_path, agent)) = build_spawn_info(app, id) {
        maybe_mirror_mcp(app, &repo_path, &path);
        let remote = crate::remote_control::RemoteOpts::from_store(&app.store);
        let _ = app.sessions.spawn(id, &path, 80, 24, mode, remote, agent)?;
        app.view = View::Attached(AttachedState::single(id));
    }
}
```

Replace the last line so it becomes:

```rust
Some(SelectionTarget::Workspace(id)) => {
    app.workspace_needs_attention.remove(&id);
    if let Some((id, path, mode, repo_path, agent)) = build_spawn_info(app, id) {
        maybe_mirror_mcp(app, &repo_path, &path);
        let remote = crate::remote_control::RemoteOpts::from_store(&app.store);
        let _ = app.sessions.spawn(id, &path, 80, 24, mode, remote, agent)?;
        let restored = restore_attached_state(app, id);
        app.view = View::Attached(restored);
    }
}
```

- [ ] **Step 5: Wire the same restore into the updates-panel attach paths**

Search for the other call sites that build `AttachedState::single`:

Run: `grep -n "AttachedState::single" src/app.rs`

The non-test sites are around lines 2407 and 2453 (updates-panel `v`/`s` and `Enter` paths). For consistency with the design ("auto-restore on Enter"), update them too — replace each `View::Attached(AttachedState::single(id))` with:

```rust
let restored = restore_attached_state(app, id);
app.view = View::Attached(restored);
```

Leave the `app.view = View::Attached(AttachedState::single(...))` calls inside `#[cfg(test)]` test bodies untouched — those are explicit constructions of test fixtures.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p wsx --lib app::tests::dashboard_enter_restores_saved_layout app::tests::dashboard_enter_falls_back_to_single_pane_when_no_layout app::tests::restore_prunes_archived_side_panes app::tests::ctrl_x_d_does_not_modify_saved_layout`
Expected: all PASS.

- [ ] **Step 7: Run the full app-test suite to catch regressions**

Run: `cargo test -p wsx --lib app::tests`
Expected: all PASS.

- [ ] **Step 8: Commit**

```bash
git add src/app.rs
git commit -m "feat(layout): restore saved layouts on dashboard Enter and updates-panel attach"
```

---

## Task 10: Add `RowInputs.has_multi_pane_layout` and render the codicon badge

**Files:**
- Modify: `src/ui/dashboard/row.rs` (`RowInputs` struct, `render` function, tests)

- [ ] **Step 1: Write the failing tests**

In the `tests` module at the bottom of `src/ui/dashboard/row.rs`, add:

```rust
#[test]
fn multi_pane_layout_appends_codicon_when_nerd_fonts() {
    let theme = Theme::wsx();
    let mut inputs = base();
    inputs.nerd_fonts = true;
    inputs.has_multi_pane_layout = true;
    let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
    let text = line_text(&line);
    assert!(
        text.contains("\u{ebb0}"),
        "split_horizontal codicon present: {text:?}"
    );
}

#[test]
fn multi_pane_layout_skipped_without_nerd_fonts() {
    let theme = Theme::wsx();
    let mut inputs = base();
    inputs.nerd_fonts = false;
    inputs.has_multi_pane_layout = true;
    let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
    let text = line_text(&line);
    assert!(
        !text.contains("\u{ebb0}"),
        "codicon should not render without nerd fonts: {text:?}"
    );
}

#[test]
fn layout_and_setup_failed_badges_both_render() {
    let theme = Theme::wsx();
    let mut inputs = base();
    inputs.nerd_fonts = true;
    inputs.has_multi_pane_layout = true;
    inputs.setup_failed = true;
    let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
    let text = line_text(&line);
    assert!(text.contains("⚙!"), "setup badge present: {text:?}");
    assert!(text.contains("\u{ebb0}"), "layout badge present: {text:?}");
}

#[test]
fn name_shrinks_to_accommodate_layout_badge() {
    let theme = Theme::wsx();
    let mut inputs = base();
    inputs.nerd_fonts = true;
    inputs.has_multi_pane_layout = true;
    inputs.name = "this-is-a-pretty-long-workspace-name-indeed".into();
    let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
    let text = line_text(&line);
    // Both: the codicon is present, and the name was truncated.
    assert!(text.contains("\u{ebb0}"));
    assert!(text.contains("…"), "long name truncated to fit: {text:?}");
}
```

Also update the `base()` helper at the top of the tests module (around line 285) — add the new field:

```rust
has_multi_pane_layout: false,
```

(Place it next to `setup_failed`.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p wsx --lib ui::dashboard::row::tests::multi_pane_layout`
Expected: FAIL with "no field `has_multi_pane_layout`".

- [ ] **Step 3: Add the field to `RowInputs`**

In `src/ui/dashboard/row.rs`, around line 67-81 (the `RowInputs` struct), add a field:

```rust
pub has_multi_pane_layout: bool,
```

Place it next to `setup_failed` for symmetry.

- [ ] **Step 4: Update the `render` function — name-width math + badge render**

Locate the existing setup_failed handling in `render` (lines 122-135). Replace the name-target/render block with:

```rust
let layout_badge_width = if inputs.has_multi_pane_layout && inputs.nerd_fonts {
    2 // " " + codicon
} else {
    0
};
let setup_badge_width = if inputs.setup_failed { 3 } else { 0 };
let name_target = name_width
    .saturating_sub(setup_badge_width)
    .saturating_sub(layout_badge_width)
    .max(1);
let name_padded = truncate_pad(&inputs.name, name_target);
let mut name_style = Style::default().add_modifier(Modifier::BOLD);
if inputs.yolo {
    name_style = name_style.fg(theme.warn);
}
spans.push(Span::styled(name_padded, name_style));
if inputs.setup_failed {
    spans.push(Span::styled(" ⚙!".to_string(), theme.err_style()));
}
if inputs.has_multi_pane_layout && inputs.nerd_fonts {
    spans.push(Span::styled(" \u{ebb0}".to_string(), theme.dim_style()));
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p wsx --lib ui::dashboard::row::tests`
Expected: all PASS (existing setup_failed tests still pass; new ones pass too).

- [ ] **Step 6: Commit**

```bash
git add src/ui/dashboard/row.rs
git commit -m "feat(layout): add split_horizontal codicon badge for workspaces with saved multi-pane layouts"
```

---

## Task 11: Populate `has_multi_pane_layout` at every `RowInputs` construction site

**Files:**
- Modify: `src/app.rs` (around line 727)
- Modify: `src/ui/dashboard/by_attention.rs` (lines 251, 406)
- Modify: `src/ui/dashboard/by_repo.rs` (line 133)
- Modify: `src/ui/dashboard/tests.rs` (line 41)

- [ ] **Step 1: Update the main row-build site in `app.rs`**

In `src/app.rs` around line 727, locate the `RowInputs` literal. Add the new field by reading from `app.workspaces_with_multi_pane_layouts`. Since the literal sits inside a loop iterating workspaces, just check membership:

```rust
let row = crate::ui::dashboard::row::RowInputs {
    status,
    name: ws.name.clone(),
    // ... existing fields ...
    nerd_fonts,
    workspace_id: ws.id,
    has_multi_pane_layout: app.workspaces_with_multi_pane_layouts.contains(&ws.id),
};
```

Place the new field at the end of the literal (after `workspace_id`).

- [ ] **Step 2: Update test-fixture construction sites**

All other `RowInputs` literal sites are inside `#[cfg(test)] mod tests` blocks (confirmed by inspection: `by_attention.rs:251` is in `make_rows`, `by_attention.rs:406` is in another test helper, `by_repo.rs:133` is in `make_view`, `tests.rs:41` is a dashboard-tests fixture). The user-facing render path constructs `RowInputs` only in `app.rs:727`. So the test fixtures can all use the literal default.

For each of these files, add `has_multi_pane_layout: false,` to the `RowInputs` struct literal:

- `src/ui/dashboard/by_attention.rs:251` (inside `make_rows`)
- `src/ui/dashboard/by_attention.rs:406` (inside the other test helper)
- `src/ui/dashboard/by_repo.rs:133` (inside `make_view`)
- `src/ui/dashboard/tests.rs:41` (inside the dashboard tests)

Place the new field after `workspace_id` for consistency with the user-facing site.

- [ ] **Step 3: Run the full lib test suite to verify compile + behavior**

Run: `cargo test -p wsx --lib`
Expected: all PASS. The compiler will catch any missed RowInputs construction site (struct literals must include all fields).

- [ ] **Step 4: Commit**

```bash
git add src/app.rs src/ui/dashboard/by_attention.rs src/ui/dashboard/by_repo.rs src/ui/dashboard/tests.rs
git commit -m "feat(layout): plumb has_multi_pane_layout through dashboard row builders"
```

---

## Task 12: Add the manual-test doc

**Files:**
- Create: `docs/manual-tests/workspace-layout-persistence.md`

- [ ] **Step 1: Create the manual-test doc**

Look at `docs/manual-tests/attention-detection.md` as the structural template. Write `docs/manual-tests/workspace-layout-persistence.md` with the following content:

```markdown
# Manual tests — workspace layout persistence

Layouts are persisted in SQLite under the `workspace_layouts` table.
These checks confirm the save / restore / prune flow end-to-end with a
real PTY.

## Test 1 — basic park & restore

1. Open wsx. Enter workspace A from the dashboard.
2. Split vertically into workspace B (use the updates panel `v` or
   any existing split entry point).
3. Press `Ctrl-x Esc` to park the layout.
4. Confirm you are back on the dashboard and that workspace A's row
   shows the split_horizontal codicon (right of the name; only visible
   with nerd fonts).
5. Press `Enter` on workspace A.
6. Expect: the (A | B) layout restores with both PTYs live.

## Test 2 — restore across wsx restart

1. From Test 1, press `Ctrl-x Esc` again.
2. Quit wsx (`q`).
3. Restart wsx.
4. Enter workspace A.
5. Expect: same (A | B) layout restored. Sessions are fresh (claude
   restarts) but the split shape is identical.

## Test 3 — pruning a side pane

1. Park a (A | B) layout under anchor A.
2. From a separate terminal, archive workspace B with
   `wsx workspace archive <repo> <B-slug>`.
3. Wait for wsx to pick up the external change (~1s — `data_version`
   poll).
4. Enter workspace A from the dashboard.
5. Expect: single-pane view of A. The side pane was pruned.

## Test 4 — anchor cascade

1. Park any layout under anchor A.
2. Archive workspace A.
3. Inspect the DB:
   `sqlite3 ~/.local/state/wsx/wsx.db 'SELECT * FROM workspace_layouts'`
4. Expect: no row for the archived workspace (CASCADE handled it).
```

- [ ] **Step 2: Commit**

```bash
git add docs/manual-tests/workspace-layout-persistence.md
git commit -m "docs: add manual-test guide for workspace layout persistence"
```

---

## Task 13: Final integration check

- [ ] **Step 1: Build the release binary to confirm no warnings or errors**

Run: `cargo build -p wsx --release`
Expected: clean build with no warnings about unused fields or unreachable arms.

- [ ] **Step 2: Run the full test suite once more**

Run: `cargo test -p wsx`
Expected: all PASS.

- [ ] **Step 3: Run clippy to catch lint regressions**

Run: `cargo clippy -p wsx --lib --tests -- -D warnings`
Expected: clean.

- [ ] **Step 4: Drive Test 1 from the manual-test doc against the local build**

Follow `docs/manual-tests/workspace-layout-persistence.md` Test 1 with the freshly-built `wsx` binary. Confirm the codicon appears on the dashboard row after parking, and that re-entering restores the split.

- [ ] **Step 5: Commit any cleanup**

If any clippy / build fixes were needed, commit them:

```bash
git add -A
git commit -m "chore(layout): clean up build warnings"
```

(Skip this step if nothing needs to commit.)

---

## Self-review notes

- **Spec coverage:** §1 architecture → Tasks 1-3, 7-9. §2 data model → Tasks 1, 4, 5. §3 save flow → Task 8. §4 restore flow → Tasks 2, 3, 9. §5 edge cases → covered in Tasks 5, 6, 9 (CASCADE, corruption, prune). §6 testing → tests embedded in every task. §7 dashboard indicator → Tasks 6, 7, 10, 11. Manual smoke → Task 12.
- **Type consistency:** `PruneOutcome` is a unit variant pair (`Kept` / `Empty`) used identically in `prune` and `restore_attached_state`. `SplitTree::first_leaf_path` is a method on `&SplitTree` returning `FocusPath`. `save_layout_for` takes `&mut App, AttachedState` (by value) consistently with the chord handler. `restore_attached_state` takes `&mut App, WorkspaceId` and returns `AttachedState`.
- **No placeholders:** every code step shows full Rust source; every test step shows the test body.
