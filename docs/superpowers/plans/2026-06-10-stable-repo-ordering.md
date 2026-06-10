# Stable, manually-ordered repos on the dashboard — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give dashboard repos a stable, user-controlled order (persisted in the DB) so adding/removing/changing workspaces never reshuffles them, with `Shift+K`/`Shift+J` to move the selected repo up/down.

**Architecture:** Add a persisted `sort_order` column to the `repos` table (DB migration v14, seeded alphabetically). Load and render repos by `sort_order` instead of the dynamic noise score. Reorder is a dashboard key action that swaps two repos' `sort_order` values and keeps the selection on the moved repo.

**Tech Stack:** Rust, ratatui (TUI), rusqlite (SQLite), crossterm (key events), tokio (async key handler).

**Spec:** `docs/superpowers/specs/2026-06-10-stable-repo-ordering-design.md`

---

## File Structure

- `src/data/store.rs` — `Repo` struct gains `sort_order: i64`; migration v14 (ALTER + alphabetical seed); `repos()` selects `sort_order` and `ORDER BY sort_order, name`; `add_repo` appends at tail; new `swap_repo_sort_order`.
- `src/ui/dashboard/by_repo.rs` — `RepoView` gains `sort_order`; `order_repos` sorts ascending by it; replace the noise-order test.
- `src/ui/dashboard/sort.rs` — remove now-unused `noise_score` + its two tests.
- `src/ui/dashboard/mod.rs` — `render_by_repo` threads `sort_order` into `RepoView`; `visible_targets` (Repo branch) sorts by `sort_order` to stay in lockstep with the renderer.
- `src/app/input.rs` — `Shift+K` repo branch → move up; new `Shift+J` arm → move down; `move_selected_repo` helper.
- Test-only `Repo {…}`/`RepoView {…}` literal sites updated to set `sort_order`.

A note on lockstep: `render_by_repo` (what the user sees) and `visible_targets` (what arrow-key navigation indexes into) **both** order repos independently today. Both must switch to `sort_order` together (Tasks 5–6) or selection will desync from the rendered rows.

---

## Task 1: Add `sort_order` to the `Repo` struct and all literal sites

This is a compile-only change: add the field and set it everywhere a `Repo` is built so the crate keeps compiling. Behavior changes come in later tasks.

**Files:**
- Modify: `src/data/store.rs:36-50` (struct), `src/data/store.rs:295-309` (`repos()` mapping — temporary literal `0`, replaced in Task 2)
- Modify (test literals): `src/ui/dashboard/tests.rs:14`, `src/agent/related.rs:80`, `src/config/chronology_source.rs:26`, `src/detail_modules/mod.rs:114`, `src/data/repo.rs:98`, `src/config/detail_bar_config.rs:222`

- [ ] **Step 1: Add the field to the struct**

In `src/data/store.rs`, add `sort_order` as the last field of `Repo`:

```rust
#[derive(Debug, Clone)]
pub struct Repo {
    pub id: RepoId,
    pub name: String,
    pub path: PathBuf,
    pub branch_prefix: String,
    pub custom_instructions: Option<String>,
    pub setup_script: Option<String>,
    pub archive_script: Option<String>,
    pub pinned_commands: Option<String>,
    pub related_repos: Option<String>,
    pub base_branch: Option<String>,
    pub detail_bar_config: Option<String>,
    pub chronology_config: Option<String>,
    pub created_at: i64,
    pub sort_order: i64,
}
```

- [ ] **Step 2: Set it in the `repos()` row mapping (temporary placeholder)**

In `src/data/store.rs`, the `repos()` closure builds a `Repo` (currently ending at `created_at: r.get(12)?,`). Add the field with a literal `0` for now (Task 2 wires it to the column):

```rust
                created_at: r.get(12)?,
                sort_order: 0,
```

- [ ] **Step 3: Set it in every test/helper `Repo {…}` literal**

Add `sort_order: 0,` as the last field in each of these literals (each is a `Repo { … }` constructor):

- `src/ui/dashboard/tests.rs:14` (in `fn fake_repo`)
- `src/agent/related.rs:80` (in `fn repo`)
- `src/config/chronology_source.rs:26` (in `fn test_repo`)
- `src/detail_modules/mod.rs:114` (the `Box::leak(Box::new(Repo { … }))`)
- `src/data/repo.rs:98` (in `fn repo`)
- `src/config/detail_bar_config.rs:222` (in `fn test_repo`)

For example, in `src/data/repo.rs`:

```rust
        Repo {
            // ...existing fields...
            chronology_config: None,
            created_at: 0,
            sort_order: 0,
        }
```

- [ ] **Step 4: Verify it compiles (this catches any literal site missed above)**

Run: `cargo build 2>&1 | tail -20`
Expected: builds cleanly. If it reports `missing field \`sort_order\` in initializer of \`...::Repo\``, add `sort_order: 0,` to the named file/line and rebuild. Repeat until clean. (This step exists specifically to surface any `Repo {…}` site not listed in Step 3.)

- [ ] **Step 5: Commit**

```bash
git add src/data/store.rs src/ui/dashboard/tests.rs src/agent/related.rs \
  src/config/chronology_source.rs src/detail_modules/mod.rs src/data/repo.rs \
  src/config/detail_bar_config.rs
git commit -m "refactor(store): add sort_order field to Repo struct"
```

---

## Task 2: Migration v14 — add the column, seed alphabetically, load by it

**Files:**
- Modify: `src/data/store.rs:239-251` (end of `migrate()`, add `if v < 14` block)
- Modify: `src/data/store.rs:286-312` (`repos()` query + mapping)
- Test: `src/data/store.rs` (`#[cfg(test)] mod tests` — add to the existing test module if present, else create one)

- [ ] **Step 1: Write the failing tests**

Add two tests to the test module in `src/data/store.rs`. The first checks that
`repos()` loads in `sort_order` order (the new `ORDER BY`); the second checks
the migration seeds *pre-existing* repos alphabetically.

Important behavior distinction: the alphabetical seed only applies to repos that
existed when the v14 migration ran. Repos added *after* migration get a
tail-appended `sort_order` (Task 3), i.e. registration order — NOT alphabetical.
So the seed test must simulate legacy (pre-v14) rows rather than calling
`add_repo`. We do that by inserting raw rows, rewinding `user_version` to 13,
and re-running the migration (the v14 column already exists, so only the seed
`UPDATE` re-runs — it is deterministic and idempotent).

```rust
    #[test]
    fn repos_load_in_sort_order() {
        let store = Store::open_in_memory().unwrap();
        // Raw insert with explicit, out-of-order sort_order values.
        store
            .conn()
            .execute(
                "INSERT INTO repos (name, path, branch_prefix, created_at, sort_order) \
                 VALUES ('b','/tmp/wsx-b','',0,2),('a','/tmp/wsx-a','',0,0),('c','/tmp/wsx-c','',0,1)",
                [],
            )
            .unwrap();
        let names: Vec<String> =
            store.repos().unwrap().into_iter().map(|r| r.name).collect();
        assert_eq!(names, vec!["a", "c", "b"], "ordered by sort_order, not name or id");
    }

    #[test]
    fn migration_seeds_existing_repos_alphabetically() {
        let store = Store::open_in_memory().unwrap();
        // Simulate legacy pre-v14 rows: all sort_order = 0, non-alphabetical id order.
        store
            .conn()
            .execute(
                "INSERT INTO repos (name, path, branch_prefix, created_at, sort_order) \
                 VALUES ('charlie','/tmp/wsx-c','',0,0),\
                        ('alpha','/tmp/wsx-a','',0,0),\
                        ('bravo','/tmp/wsx-b','',0,0)",
                [],
            )
            .unwrap();
        // Rewind to v13 and re-run migrate so the v14 seed UPDATE runs over them.
        store.conn().execute("PRAGMA user_version = 13", []).unwrap();
        store.migrate_for_test().unwrap();

        let repos = store.repos().unwrap();
        let names: Vec<&str> = repos.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "bravo", "charlie"]);
        let orders: Vec<i64> = repos.iter().map(|r| r.sort_order).collect();
        assert_eq!(orders, vec![0, 1, 2], "unique 0-based alphabetical ranks");
    }
```

> Both `Store::conn()` and `Store::migrate_for_test()` already exist as
> `pub(crate)`/`#[cfg(test)]` helpers (`src/data/store.rs:770`, `:775`) and are
> reachable from this in-crate test module.

- [ ] **Step 2: Add the migration block**

In `src/data/store.rs`, inside `migrate()`, immediately after the `if v < 13 { ... }` block and before `Ok(())`, add:

```rust
        if v < 14 {
            let has_col: i64 = self.conn.query_row(
                "SELECT count(*) FROM pragma_table_info('repos') WHERE name = 'sort_order'",
                [],
                |r| r.get(0),
            )?;
            if has_col == 0 {
                self.conn.execute(
                    "ALTER TABLE repos ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0",
                    [],
                )?;
            }
            // Seed a unique, deterministic alphabetical rank (case-insensitive,
            // id as tiebreak) for all existing repos. Idempotent: recomputes the
            // same ranks if re-run after a partial migration.
            self.conn.execute(
                "UPDATE repos SET sort_order = (\
                     SELECT COUNT(*) FROM repos r2 \
                     WHERE LOWER(r2.name) < LOWER(repos.name) \
                        OR (LOWER(r2.name) = LOWER(repos.name) AND r2.id < repos.id)\
                 )",
                [],
            )?;
            self.conn.execute("PRAGMA user_version = 14", [])?;
        }
```

- [ ] **Step 3: Load by `sort_order`**

In `src/data/store.rs`, update the `repos()` query and mapping. Change the SQL to select the column and order by it, and read it into the field (replacing the `sort_order: 0` placeholder from Task 1):

```rust
    pub fn repos(&self) -> Result<Vec<Repo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, path, branch_prefix, custom_instructions, \
                    setup_script, archive_script, pinned_commands, \
                    related_repos, base_branch, detail_bar_config, \
                    chronology_config, created_at, sort_order \
             FROM repos ORDER BY sort_order, name",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Repo {
                id: RepoId(r.get(0)?),
                name: r.get(1)?,
                path: PathBuf::from(r.get::<_, String>(2)?),
                branch_prefix: r.get(3)?,
                custom_instructions: r.get(4)?,
                setup_script: r.get(5)?,
                archive_script: r.get(6)?,
                pinned_commands: r.get(7)?,
                related_repos: r.get(8)?,
                base_branch: r.get(9)?,
                detail_bar_config: r.get(10)?,
                chronology_config: r.get(11)?,
                created_at: r.get(12)?,
                sort_order: r.get(13)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib repos_load_in_sort_order migration_seeds_existing_repos_alphabetically 2>&1 | tail -20`
Expected: PASS (both). These do not depend on Task 3.

- [ ] **Step 5: Commit**

```bash
git add src/data/store.rs
git commit -m "feat(store): persist repo sort_order, seed alphabetically (migration v14)"
```

---

## Task 3: New repos append to the end

**Files:**
- Modify: `src/data/store.rs:254-261` (`add_repo`)
- Test: `src/data/store.rs` test module

- [ ] **Step 1: Write the failing test**

Add to the test module in `src/data/store.rs`:

```rust
    #[test]
    fn add_repo_appends_to_tail_sort_order() {
        let store = Store::open_in_memory().unwrap();
        // "zeta" then "alpha": even though alpha sorts first by name, the
        // tail-append rule must give zeta=0, alpha=1 (registration order).
        store.add_repo(std::path::Path::new("/tmp/wsx-zeta"), "zeta", "").unwrap();
        store.add_repo(std::path::Path::new("/tmp/wsx-alpha2"), "alpha", "").unwrap();

        let order = |name: &str, repos: &[Repo]| {
            repos.iter().find(|r| r.name == name).unwrap().sort_order
        };
        let repos = store.repos().unwrap();
        assert_eq!(order("zeta", &repos), 0, "first registered → tail of empty list → 0");
        assert_eq!(order("alpha", &repos), 1, "second registered → appended after → 1");
    }
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cargo test --lib add_repo_appends_to_tail_sort_order 2>&1 | tail -20`
Expected: FAIL — both repos currently get `sort_order = 0` (column default), so the `alpha == 1` assertion fails.

- [ ] **Step 3: Make `add_repo` append at the tail**

In `src/data/store.rs`, change the INSERT to compute the next `sort_order` as `max + 1` (which is `0` for the first repo, since `COALESCE(MAX(...), -1) + 1`):

```rust
    pub fn add_repo(&self, path: &Path, name: &str, branch_prefix: &str) -> Result<RepoId> {
        let now = now_ms();
        self.conn.execute(
            "INSERT INTO repos (name, path, branch_prefix, created_at, sort_order) \
             VALUES (?1, ?2, ?3, ?4, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM repos))",
            rusqlite::params![name, path.to_string_lossy(), branch_prefix, now],
        )?;
        Ok(RepoId(self.conn.last_insert_rowid()))
    }
```

- [ ] **Step 4: Run both store tests**

Run: `cargo test --lib add_repo_appends_to_tail_sort_order repos_load_in_sort_order 2>&1 | tail -20`
Expected: PASS (both).

- [ ] **Step 5: Commit**

```bash
git add src/data/store.rs
git commit -m "feat(store): append new repos to end of sort order"
```

---

## Task 4: `swap_repo_sort_order` store method

**Files:**
- Modify: `src/data/store.rs` (add method near the other `set_repo_*` methods, ~line 314)
- Test: `src/data/store.rs` test module

- [ ] **Step 1: Write the failing test**

Add to the test module in `src/data/store.rs`:

```rust
    #[test]
    fn swap_repo_sort_order_swaps_two_repos() {
        let store = Store::open_in_memory().unwrap();
        let a = store.add_repo(std::path::Path::new("/tmp/wsx-a"), "aaa", "").unwrap(); // sort_order 0
        let b = store.add_repo(std::path::Path::new("/tmp/wsx-b"), "bbb", "").unwrap(); // sort_order 1

        store.swap_repo_sort_order(a, b).unwrap();

        let repos = store.repos().unwrap();
        // After swap, bbb (now 0) sorts before aaa (now 1).
        let names: Vec<&str> = repos.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["bbb", "aaa"], "swap reorders the load order");

        let so = |name: &str| repos.iter().find(|r| r.name == name).unwrap().sort_order;
        assert_eq!(so("bbb"), 0);
        assert_eq!(so("aaa"), 1);
    }
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cargo test --lib swap_repo_sort_order_swaps_two_repos 2>&1 | tail -20`
Expected: FAIL — `swap_repo_sort_order` does not exist (compile error / unresolved method).

- [ ] **Step 3: Implement the method**

In `src/data/store.rs`, add this method alongside the other `set_repo_*` methods (e.g. just after `set_repo_branch_prefix`, around line 320). It reads both values and writes each to the other's, inside a transaction for atomicity:

```rust
    /// Swap the `sort_order` of two repos. Used by the dashboard to move a
    /// repo up/down by one slot. Atomic so a crash can't leave a half-swap.
    pub fn swap_repo_sort_order(&self, a: RepoId, b: RepoId) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let so_a: i64 =
            tx.query_row("SELECT sort_order FROM repos WHERE id = ?1", [a.0], |r| r.get(0))?;
        let so_b: i64 =
            tx.query_row("SELECT sort_order FROM repos WHERE id = ?1", [b.0], |r| r.get(0))?;
        tx.execute(
            "UPDATE repos SET sort_order = ?1 WHERE id = ?2",
            rusqlite::params![so_b, a.0],
        )?;
        tx.execute(
            "UPDATE repos SET sort_order = ?1 WHERE id = ?2",
            rusqlite::params![so_a, b.0],
        )?;
        tx.commit()?;
        Ok(())
    }
```

- [ ] **Step 4: Run the test**

Run: `cargo test --lib swap_repo_sort_order_swaps_two_repos 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/data/store.rs
git commit -m "feat(store): add swap_repo_sort_order for manual reordering"
```

---

## Task 5: Order the dashboard by `sort_order` (renderer + nav, in lockstep)

Replace the noise-score ordering in both the renderer (`order_repos`) and the navigation index builder (`visible_targets`). Remove the now-unused `noise_score`.

**Files:**
- Modify: `src/ui/dashboard/by_repo.rs:13-38` (`RepoView` + `order_repos`), `:123-242` (tests)
- Modify: `src/ui/dashboard/mod.rs:356-384` (`render_by_repo` builds `RepoView`), `:226-262` (`visible_targets` Repo branch)
- Modify: `src/ui/dashboard/sort.rs:39-42` (delete `noise_score`), `:68-86` (delete its two tests)

- [ ] **Step 1: Replace the `order_repos` test (write the new failing expectation)**

In `src/ui/dashboard/by_repo.rs`, replace the existing `order_repos_puts_noisy_first_and_empty_last` test (lines 224-242) with a test that asserts ascending `sort_order` ordering regardless of activity. Also extend `make_view` (lines 128-164) to set the new field. First, in `make_view`, add `sort_order` to the returned `RepoView` (use the `id` as the order so the fixture is deterministic):

```rust
        RepoView {
            id,
            name: r.name.as_str(),
            path: r.path.clone(),
            counts,
            expanded,
            workspaces,
            sort_order: id as i64,
        }
```

Then replace the old ordering test with:

```rust
    #[test]
    fn order_repos_sorts_by_sort_order_ascending() {
        let repos = fixture::repos();
        // Build views, then assign sort_order in REVERSE of fixture order so a
        // correct ascending sort visibly reorders them (id stays the identity).
        let mut views: Vec<RepoView<'_>> = repos
            .iter()
            .enumerate()
            .map(|(i, r)| make_view(r, i as u64, true))
            .collect();
        let n = views.len() as i64;
        for (i, v) in views.iter_mut().enumerate() {
            v.sort_order = n - 1 - i as i64;
        }
        order_repos(&mut views);
        let orders: Vec<i64> = views.iter().map(|v| v.sort_order).collect();
        let mut sorted = orders.clone();
        sorted.sort();
        assert_eq!(orders, sorted, "repos must be in ascending sort_order");
        // Activity/emptiness must NOT affect order anymore.
        assert_eq!(views.first().unwrap().sort_order, 0);
        assert_eq!(views.last().unwrap().sort_order, n - 1);
    }
```

- [ ] **Step 2: Run it to confirm it fails to compile**

Run: `cargo test --lib order_repos_sorts_by_sort_order_ascending 2>&1 | tail -20`
Expected: FAIL — `RepoView` has no field `sort_order` yet (compile error).

- [ ] **Step 3: Add the field and rewrite `order_repos`**

In `src/ui/dashboard/by_repo.rs`, add the field to the struct and replace the function body. Also drop the `noise_score` import:

```rust
use crate::ui::dashboard::row::{self, RowInputs};
use crate::ui::dashboard::sort::StatusCounts;
use crate::ui::dashboard::status::Status;
```

```rust
#[derive(Debug, Clone)]
pub struct RepoView<'a> {
    pub id: u64,
    pub name: &'a str,
    /// Lossy-converted display path — `RepoView` owns the string so
    /// non-UTF8 path bytes survive the conversion (with U+FFFD
    /// substitution) instead of being dropped to an empty string.
    pub path: String,
    pub counts: StatusCounts,
    pub expanded: bool,
    /// Persisted manual order; repos render ascending by this. Stable across
    /// workspace add/remove/status changes.
    pub sort_order: i64,
    /// Already sorted by Status priority (Stalled first).
    pub workspaces: Vec<RowInputs>,
}

/// Order repos by their persisted manual `sort_order`, ascending. This is
/// stable: workspace activity never changes a repo's position.
pub fn order_repos(repos: &mut [RepoView<'_>]) {
    repos.sort_by_key(|r| r.sort_order);
}
```

- [ ] **Step 4: Thread `sort_order` through `render_by_repo`**

In `src/ui/dashboard/mod.rs`, the `RepoView { … }` built around line 374 must set the field from the source repo (`r` is a `&Repo` from `inputs.repos`):

```rust
            RepoView {
                id: repo_id_u64,
                name: &r.name,
                path: r.path.to_string_lossy().into_owned(),
                counts,
                expanded,
                sort_order: r.sort_order,
                workspaces,
            }
```

- [ ] **Step 5: Make `visible_targets` order by `sort_order` too (lockstep)**

In `src/ui/dashboard/mod.rs`, the `GroupMode::Repo` branch of `visible_targets` (lines 226-262) currently builds `Pending { repo_id, counts, workspace_ids }` and sorts by noise score. Add a `sort_order` field to `Pending`, populate it from `r.sort_order`, and replace the sort. Update the `struct Pending`:

```rust
            #[derive(Clone)]
            struct Pending {
                repo_id: crate::data::store::RepoId,
                counts: StatusCounts,
                sort_order: i64,
                workspace_ids: Vec<crate::data::store::WorkspaceId>,
            }
```

In the `.map(|r| { … })` that builds each `Pending`, add `sort_order: r.sort_order,`:

```rust
                    Pending {
                        repo_id: r.id,
                        counts,
                        sort_order: r.sort_order,
                        workspace_ids: rows.into_iter().map(|(_, id)| id).collect(),
                    }
```

Replace the `pending.sort_by(|a, b| { … noise_score … })` block (lines 252-262) with:

```rust
            // Mirror by_repo::order_repos: stable manual order, ascending.
            pending.sort_by_key(|p| p.sort_order);
```

- [ ] **Step 6: Remove the now-unused `noise_score` and its tests**

In `src/ui/dashboard/sort.rs`, delete the `noise_score` function (lines 39-42) and delete the two tests that reference it: `noise_score_question_outweighs_stalled` and `noise_score_matches_design_example_ordering` (lines 68-86).

- [ ] **Step 7: Build and run the dashboard tests**

Run: `cargo test --lib dashboard 2>&1 | tail -30`
Expected: PASS, including `order_repos_sorts_by_sort_order_ascending`. No `noise_score`-related compile errors (the function and its only two callers are all removed).

If the build complains `unused import` for anything in `by_repo.rs` or `mod.rs`, remove the dangling import. Run `cargo build 2>&1 | tail -20` to confirm clean.

- [ ] **Step 8: Commit**

```bash
git add src/ui/dashboard/by_repo.rs src/ui/dashboard/mod.rs src/ui/dashboard/sort.rs
git commit -m "feat(dashboard): order repos by persisted sort_order, not noise score"
```

---

## Task 6: Reorder keybindings — `Shift+K` (up) / `Shift+J` (down)

`Shift+K` already opens the process list for a selected **workspace** and is a no-op for a selected **repo header** (`src/app/input.rs:734`). We keep the workspace behavior and repurpose the repo branch to "move up"; add a new `Shift+J` arm for "move down". Reordering acts only when a repo header is selected.

**Files:**
- Modify: `src/app/input.rs:734-742` (the `Char('K')` arm) and add a `Char('J')` arm next to it
- Add: `move_selected_repo` helper in `src/app/input.rs` (near `toggle_focused_fold`, ~line 224)
- Test: `src/app/input_tests.rs` (in the same `mod tests` as `make_app_with_n_repos`)

- [ ] **Step 1: Write the failing tests**

Add to the test module in `src/app/input_tests.rs` (it already has `make_app_with_n_repos`, `press`, and imports `KeyModifiers`). Repos created there are `repo-0`, `repo-1`, `repo-2` with `sort_order` 0,1,2, so `app.selectable` (no workspaces) is `[Repo(id0), Repo(id1), Repo(id2)]`:

```rust
    #[tokio::test]
    async fn shift_k_moves_selected_repo_up() {
        let (mut app, ids) = make_app_with_n_repos(3);
        app.dashboard.selected = 1; // select repo-1 (Repo header)
        press(&mut app, 'K', KeyModifiers::SHIFT).await;

        let order: Vec<_> = app.repos.iter().map(|r| r.id).collect();
        assert_eq!(order, vec![ids[1], ids[0], ids[2]], "repo-1 moved above repo-0");
        assert_eq!(
            app.selected_target(),
            Some(SelectionTarget::Repo(ids[1])),
            "selection follows the moved repo"
        );
    }

    #[tokio::test]
    async fn shift_j_moves_selected_repo_down() {
        let (mut app, ids) = make_app_with_n_repos(3);
        app.dashboard.selected = 1; // select repo-1
        press(&mut app, 'J', KeyModifiers::SHIFT).await;

        let order: Vec<_> = app.repos.iter().map(|r| r.id).collect();
        assert_eq!(order, vec![ids[0], ids[2], ids[1]], "repo-1 moved below repo-2");
        assert_eq!(app.selected_target(), Some(SelectionTarget::Repo(ids[1])));
    }

    #[tokio::test]
    async fn shift_k_at_top_is_noop() {
        let (mut app, ids) = make_app_with_n_repos(3);
        app.dashboard.selected = 0; // top repo
        press(&mut app, 'K', KeyModifiers::SHIFT).await;
        let order: Vec<_> = app.repos.iter().map(|r| r.id).collect();
        assert_eq!(order, vec![ids[0], ids[1], ids[2]], "no movement at the top");
    }

    #[tokio::test]
    async fn shift_j_at_bottom_is_noop() {
        let (mut app, ids) = make_app_with_n_repos(3);
        app.dashboard.selected = 2; // bottom repo
        press(&mut app, 'J', KeyModifiers::SHIFT).await;
        let order: Vec<_> = app.repos.iter().map(|r| r.id).collect();
        assert_eq!(order, vec![ids[0], ids[1], ids[2]], "no movement at the bottom");
    }
```

> `SelectionTarget` is already in scope in this test module (used by other
> tests). If not, add `use crate::app::SelectionTarget;` to the module.

- [ ] **Step 2: Run them to confirm they fail**

Run: `cargo test --lib shift_k_moves_selected_repo_up shift_j_moves_selected_repo_down shift_k_at_top_is_noop shift_j_at_bottom_is_noop 2>&1 | tail -30`
Expected: FAIL — `Shift+K` on a repo header is currently a no-op (order unchanged), and there is no `Shift+J` arm, so all four assertions about movement fail.

- [ ] **Step 3: Add the `move_selected_repo` helper**

In `src/app/input.rs`, add this near `toggle_focused_fold` (~line 224). It only acts on a selected **repo header**:

```rust
/// Move the currently selected repo one slot up (`up = true`) or down on the
/// dashboard, persisting the new order. No-op unless a repo *header* is
/// selected, and no-op at the ends of the list. Keeps the selection anchored
/// to the moved repo so repeated presses walk it into place.
fn move_selected_repo(app: &mut App, up: bool) -> Result<()> {
    let Some(SelectionTarget::Repo(rid)) = app.selected_target() else {
        return Ok(());
    };
    let Some(pos) = app.repos.iter().position(|r| r.id == rid) else {
        return Ok(());
    };
    let neighbor = if up {
        pos.checked_sub(1)
    } else if pos + 1 < app.repos.len() {
        Some(pos + 1)
    } else {
        None
    };
    let Some(nb) = neighbor else { return Ok(()) };
    let nb_id = app.repos[nb].id;

    app.store.swap_repo_sort_order(rid, nb_id)?;
    app.refresh()?;

    // Anchor the cursor to the repo we just moved.
    if let Some(idx) = app
        .selectable
        .iter()
        .position(|t| *t == SelectionTarget::Repo(rid))
    {
        app.dashboard.selected = idx;
    }
    app.dashboard.selection = Some(SelectionTarget::Repo(rid));
    Ok(())
}
```

- [ ] **Step 4: Wire the keys**

In `src/app/input.rs`, change the `Repo` branch of the existing `Char('K')` arm (lines 734-742) from a no-op to "move up", and add a new `Char('J')` arm immediately after it. Replace:

```rust
        (KeyCode::Char('K'), _) => {
            if let Some(SelectionTarget::Workspace(id)) = app.selected_target() {
                app.modal = Some(Modal::ProcessList {
                    workspace_id: id,
                    selected: 0,
                });
            }
            // 'K' on a Repo header is intentionally a no-op.
        }
```

with:

```rust
        (KeyCode::Char('K'), _) => match app.selected_target() {
            Some(SelectionTarget::Workspace(id)) => {
                app.modal = Some(Modal::ProcessList {
                    workspace_id: id,
                    selected: 0,
                });
            }
            // Shift+K on a repo header moves it up one slot.
            Some(SelectionTarget::Repo(_)) => move_selected_repo(app, true)?,
            None => {}
        },
        (KeyCode::Char('J'), _) => {
            // Shift+J on a repo header moves it down one slot. On a workspace
            // it's a no-op (J is otherwise unbound on the dashboard).
            if let Some(SelectionTarget::Repo(_)) = app.selected_target() {
                move_selected_repo(app, false)?;
            }
        }
```

- [ ] **Step 5: Run the reorder tests**

Run: `cargo test --lib shift_k_moves_selected_repo_up shift_j_moves_selected_repo_down shift_k_at_top_is_noop shift_j_at_bottom_is_noop 2>&1 | tail -30`
Expected: PASS (all four).

- [ ] **Step 6: Run the full suite + clippy**

Run: `cargo test 2>&1 | tail -25 && cargo clippy --all-targets 2>&1 | tail -15`
Expected: all tests pass; no new clippy warnings. (Pre-existing process-list test for `Shift+K` on a workspace should still pass, confirming we didn't break it.)

- [ ] **Step 7: Commit**

```bash
git add src/app/input.rs src/app/input_tests.rs
git commit -m "feat(dashboard): move selected repo up/down with Shift+K / Shift+J"
```

---

## Task 7: Documentation — README keybindings

The README documents dashboard keybindings; add the new ones so the feature is discoverable.

**Files:**
- Modify: `README.md` (the Keybindings section)

- [ ] **Step 1: Find the keybindings table**

Run: `grep -n "Keybindings\|process list\|Shift" README.md | head`
Expected: locates the dashboard keybindings section/table.

- [ ] **Step 2: Add entries**

In the dashboard keybindings list/table, add rows describing:
- `Shift+K` — when a repo header is selected, move the repo up one slot (persisted). When a workspace is selected, it still opens the process list.
- `Shift+J` — when a repo header is selected, move the repo down one slot (persisted).

Match the surrounding table/markdown format exactly (column layout, key glyph style).

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs(readme): document Shift+K / Shift+J repo reordering"
```

---

## Final verification

- [ ] Run the whole suite once more: `cargo test 2>&1 | tail -25` — all green.
- [ ] `cargo clippy --all-targets 2>&1 | tail -15` — clean.
- [ ] Manual smoke (optional, via the `/run` skill): launch the TUI, select a repo header, press `Shift+J`/`Shift+K`, confirm the repo moves and stays put after adding/removing a workspace.
- [ ] Open a PR per the repo's workflow (feature branch already in use; do **not** push to main).
