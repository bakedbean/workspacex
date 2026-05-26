# Archive-workspace modal loading indicator — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show an animated spinner while a workspace is being archived (removed), so the TUI does not appear frozen during slow `git worktree remove` calls.

**Architecture:** Mirror the existing create-workspace spawn-and-reconcile pattern. Add a new `Modal::ArchiveRunning` variant (no fields — Esc is a deliberate no-op during archive). Spawn `workspace::archive` on a `tokio::spawn` task so the main event loop's `App` mutex is released, then reconcile the outcome via a generation counter (`pending_archive_gen`) that mirrors `pending_create_gen`.

**Tech Stack:** Rust 2021, tokio (multi-thread runtime), ratatui. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-05-26-wsx-remove-spinner-design.md`

---

## File Structure

| File | Role |
|---|---|
| `src/ui/modal.rs` | Add `Modal::ArchiveRunning` variant. Render branch displays the existing braille spinner + "Removing workspace…" text. |
| `src/workspace.rs` | Add `archive_with_app(app, repo, ws, opts) -> Result<SetupResult>` — TUI-path archive that interleaves brief app-lock acquisitions around unlocked git/script work. Required because `Store` is not `Clone`, so the spawned task cannot hold `&Store` across the long await. |
| `src/app.rs` | Add `App` fields `next_archive_gen: u64` and `pending_archive_gen: Option<u64>`. Add `App::alloc_archive_gen()` and free function `reconcile_archive_result()`. |
| `src/app/input.rs` | Rewrite the `'y'` branch of `Modal::ConfirmArchive` to spawn-and-reconcile. Add a `Modal::ArchiveRunning` match arm that ignores all keys. |
| `src/app/input_tests.rs` | Tests for the transition, the Esc no-op, and the reconcile function (success / error / staleness). |

## Conventions

- **TDD:** every behavior task starts with a failing test.
- **Commits:** one logical change per commit. Run `cargo test --workspace` before each commit.
- **No `unwrap()` on Mutex locks** in production code — match the existing `lock().await` patterns in `src/app.rs`.
- The plan mirrors the create-flow patterns at:
  - Modal variant: `src/ui/modal.rs:23` (`SetupRunning`)
  - Render branch: `src/ui/modal.rs:102-106`
  - App fields: `src/app.rs:254-255` (`next_create_gen`, `pending_create_gen`)
  - Allocator: `src/app.rs:329-335` (`alloc_create_gen`)
  - Reconcile: `src/app.rs:898-946` (`reconcile_create_result`)
  - Spawn site: `src/app/input.rs:854-881` (`Enter` in `NewWorkspace`)
  - Input no-op: `src/app/input.rs:950-958` (`SetupRunning` arm)
  - Integration tests: `src/app/input_tests.rs:2262-2372`

Reading these references first will make every task below faster.

---

## Task 1: Add `Modal::ArchiveRunning` variant and render branch

**Files:**
- Modify: `src/ui/modal.rs`
- Modify: `src/app/input.rs`

- [ ] **Step 1: Add the variant**

In `src/ui/modal.rs`, the `Modal` enum currently ends around line 42 with the `RepoSettings` variant. Add a new variant immediately after `SetupRunning` (around line 25, after its closing `},`):

```rust
    ArchiveRunning,
```

The variant carries no fields because Esc is a deliberate no-op during archive (see the spec's "Non-goals").

- [ ] **Step 2: Add the render branch**

Still in `src/ui/modal.rs`, the `match modal` block inside `render()` runs from line 76. Add a new arm immediately after the `SetupRunning` arm (currently at lines 102-106) and before the `Error` arm:

```rust
        Modal::ArchiveRunning => {
            let frame = crate::ui::dashboard::spinner::frame(tick);
            let body = format!("  {frame} Removing workspace…");
            ("archive workspace", body)
        }
```

The body has no `[esc] cancel` hint, in contrast to `SetupRunning`'s body — Esc is a no-op here.

- [ ] **Step 3: Add the input match arm (no-op)**

In `src/app/input.rs`, the `match modal` block inside `handle_key_modal` runs from line 832. The `SetupRunning` arm is at lines 950-958. Add a new arm immediately after it:

```rust
        Modal::ArchiveRunning => {
            // Archive is non-cancellable. Swallow all keys until the
            // spawned task completes and reconciles the modal.
        }
```

The arm exists only to keep the `match` exhaustive — there is no behavior.

- [ ] **Step 4: Verify it builds**

Run: `cargo build`
Expected: completes successfully with no errors. The new variant compiles and exhaustiveness is satisfied across `render()` and `handle_key_modal`.

- [ ] **Step 5: Verify tests still pass**

Run: `cargo test --workspace`
Expected: all existing tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/ui/modal.rs src/app/input.rs
git commit -m "feat(modal): add ArchiveRunning variant"
```

---

## Task 2: Add archive-generation tracking to `App`

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add the two fields to the `App` struct**

In `src/app.rs`, the `App` struct definition begins at line 100. The create-flow fields `next_create_gen: u64` and `pending_create_gen: Option<u64>` live near lines 106-110. Add two new sibling fields immediately after them:

```rust
    /// Monotonic counter handed out to in-flight workspace archive tasks.
    pub next_archive_gen: u64,
    /// Generation id of the currently in-flight workspace archive, if any.
    /// Used by the reconcile step to detect stale completions.
    pub pending_archive_gen: Option<u64>,
```

- [ ] **Step 2: Initialize the fields in `App::new`**

Still in `src/app.rs`, the `App` constructor body around line 254 initializes `next_create_gen: 0, pending_create_gen: None,`. Add the two analogous archive initializers immediately after those lines:

```rust
            next_archive_gen: 0,
            pending_archive_gen: None,
```

- [ ] **Step 3: Add the allocator method**

Still in `src/app.rs`, the `alloc_create_gen` method is defined at lines 329-335. Add an analogous method immediately after it:

```rust
    /// Allocate a fresh generation id for a new workspace-archive task.
    pub fn alloc_archive_gen(&mut self) -> u64 {
        let g = self.next_archive_gen;
        self.next_archive_gen = self.next_archive_gen.wrapping_add(1);
        self.pending_archive_gen = Some(g);
        g
    }
```

- [ ] **Step 4: Verify it builds and tests pass**

Run: `cargo test --workspace`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): add archive-generation tracking fields"
```

---

## Task 3: Add `reconcile_archive_result` with unit tests

This task introduces the reconcile function and unit-tests its three paths (Ok → close modal, Err with matching gen → Error modal, Err with mismatched gen → no modal change). TDD order: write the three tests, watch them fail to compile, then implement.

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Write the failing tests**

In `src/app.rs`, scroll to the bottom of the file. There is an existing `#[cfg(test)] mod derive_stopped_kind_tests` block starting around line 947. Add a new test module immediately after that one (still at the file's bottom):

```rust
#[cfg(test)]
mod reconcile_archive_tests {
    use super::*;
    use crate::error::Error;
    use crate::setup::SetupResult;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::Mutex;

    fn make_app() -> (App, TempDir) {
        let store = crate::store::Store::open_in_memory().unwrap();
        let tmp = TempDir::new().unwrap();
        let app = App::new(store, tmp.path().to_path_buf()).unwrap();
        (app, tmp)
    }

    #[tokio::test]
    async fn reconcile_ok_closes_archive_running_modal() {
        let (mut app, _tmp) = make_app();
        app.modal = Some(crate::ui::modal::Modal::ArchiveRunning);
        app.pending_archive_gen = Some(7);
        app.next_archive_gen = 8;
        let shared = Arc::new(Mutex::new(app));
        reconcile_archive_result(shared.clone(), 7, Ok(SetupResult::Ok)).await;
        let g = shared.lock().await;
        assert!(g.modal.is_none(), "modal should clear on Ok; got {:?}", g.modal);
        assert!(g.pending_archive_gen.is_none());
    }

    #[tokio::test]
    async fn reconcile_err_sets_error_modal() {
        let (mut app, _tmp) = make_app();
        app.modal = Some(crate::ui::modal::Modal::ArchiveRunning);
        app.pending_archive_gen = Some(7);
        app.next_archive_gen = 8;
        let shared = Arc::new(Mutex::new(app));
        reconcile_archive_result(
            shared.clone(),
            7,
            Err(Error::Setup("boom".into())),
        )
        .await;
        let g = shared.lock().await;
        match &g.modal {
            Some(crate::ui::modal::Modal::Error { message }) => {
                assert!(message.contains("boom"), "error message should contain failure detail; got {message:?}");
            }
            other => panic!("expected Modal::Error, got {other:?}"),
        }
        assert!(g.pending_archive_gen.is_none());
    }

    #[tokio::test]
    async fn reconcile_skips_modal_mutation_when_gen_mismatch() {
        let (mut app, _tmp) = make_app();
        // Simulate: a different modal is already showing (e.g. an Error
        // popped by another flow) and pending_archive_gen advanced past
        // the value our stale task carries.
        app.modal = Some(crate::ui::modal::Modal::Error {
            message: "untouched".into(),
        });
        app.pending_archive_gen = Some(99);
        app.next_archive_gen = 100;
        let shared = Arc::new(Mutex::new(app));
        reconcile_archive_result(
            shared.clone(),
            7, // stale — does not match pending_archive_gen
            Err(Error::Setup("ignored".into())),
        )
        .await;
        let g = shared.lock().await;
        match &g.modal {
            Some(crate::ui::modal::Modal::Error { message }) => {
                assert_eq!(message, "untouched", "stale reconcile must not overwrite modal");
            }
            other => panic!("expected the pre-existing Error modal to survive, got {other:?}"),
        }
        assert_eq!(g.pending_archive_gen, Some(99), "stale reconcile must not clear pending_archive_gen");
    }
}
```

- [ ] **Step 2: Confirm the tests fail to compile**

Run: `cargo test --lib reconcile_archive_tests`
Expected: build fails with `cannot find function reconcile_archive_result in this scope` (or similar). This confirms the test references the function we're about to implement.

- [ ] **Step 3: Implement `reconcile_archive_result`**

In `src/app.rs`, the existing `reconcile_create_result` is defined at lines 898-946. Add the archive equivalent immediately after it (before the `#[cfg(test)] mod derive_stopped_kind_tests` block):

```rust
/// Reconcile the outcome of a spawned `workspace::archive` task.
/// Locks the app briefly; if the modal is still `ArchiveRunning` AND the
/// generation matches ours, applies the outcome (close modal on success,
/// switch to `Modal::Error` on failure). Otherwise — user dismissed or
/// some other flow replaced the modal — leaves the modal alone but still
/// calls `refresh()` so the dashboard reflects the store mutation.
pub(crate) async fn reconcile_archive_result(
    app: SharedApp,
    my_gen: u64,
    result: Result<crate::setup::SetupResult>,
) {
    let mut g = app.lock().await;
    let is_mine = g.pending_archive_gen == Some(my_gen);
    if is_mine {
        g.pending_archive_gen = None;
    }
    match result {
        Ok(_) => {
            if is_mine && matches!(g.modal, Some(crate::ui::modal::Modal::ArchiveRunning)) {
                g.modal = None;
            }
            let _ = g.refresh();
        }
        Err(e) => {
            if is_mine && matches!(g.modal, Some(crate::ui::modal::Modal::ArchiveRunning)) {
                g.modal = Some(crate::ui::modal::Modal::Error {
                    message: e.to_string(),
                });
            }
            let _ = g.refresh();
        }
    }
}
```

Notes:
- `crate::error::Error` already has `Display` via `thiserror`, so `e.to_string()` produces a readable message (e.g. `"setup: boom"`).
- Archive does not have a `Cancelled` branch like create does, because Esc is a no-op for archive; we don't suppress error popups for cancelled cases.

- [ ] **Step 4: Verify the tests pass**

Run: `cargo test --lib reconcile_archive_tests`
Expected: all three tests pass.

- [ ] **Step 5: Verify the whole workspace still passes**

Run: `cargo test --workspace`
Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): add reconcile_archive_result"
```

---

## Task 4: Add `workspace::archive_with_app`

`Store` is not `Clone` (it owns a `rusqlite::Connection`), so the spawned tokio task cannot capture `&Store`. The create flow solves this with `create_with_app` (at `src/workspace.rs:117-244`), which takes `SharedApp` and interleaves brief lock acquisitions around the unlocked async work. We mirror that pattern for archive.

**Files:**
- Modify: `src/workspace.rs`
- Modify: `src/workspace.rs` (tests at the bottom of the file)

- [ ] **Step 1: Write a failing test for `archive_with_app`**

In `src/workspace.rs`, the existing `archive_runs_archive_script_when_set` test is at line 693. Locate the `#[cfg(test)] mod tests` block (look at line 367 for `use super::*;`). Add a new test in that module, ideally near the existing archive test:

```rust
    #[tokio::test]
    async fn archive_with_app_removes_workspace_and_worktree() {
        use std::sync::Arc;
        use tokio::sync::Mutex;
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let id = crate::repo::add(&store, repo_dir.path(), "demo", "")
            .await
            .unwrap();
        let base = TempDir::new().unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let created = create(
            &store,
            &repo,
            Some("doomed"),
            base.path(),
            false,
            crate::pty::session::AgentKind::Claude,
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        let worktree_path = created.workspace.worktree_path.clone();
        let ws_id = created.workspace.id;
        // Build a minimal App wrapping the populated store so we can pass
        // it as SharedApp.
        let app = crate::app::App::new(store, base.path().to_path_buf()).unwrap();
        let shared = Arc::new(Mutex::new(app));
        let result = archive_with_app(
            shared.clone(),
            repo.clone(),
            created.workspace.clone(),
            ArchiveOpts {
                force_branch_delete: true,
                ..Default::default()
            },
        )
        .await;
        assert!(result.is_ok(), "archive_with_app failed: {result:?}");
        // Worktree is gone from disk.
        assert!(!worktree_path.exists(), "worktree still present after archive");
        // Workspace row is gone from the store.
        let g = shared.lock().await;
        assert!(
            g.store.workspaces(repo.id).unwrap().iter().all(|w| w.id != ws_id),
            "workspace row still present after archive"
        );
    }
```

Notes:
- This test reuses the existing `init_git_repo()`, `TempDir`, and `create(...)` helpers already imported in the test module.
- The `App::new` call uses the same store the workspace was created in (we move it into the App). This is the same trick the create test pattern uses elsewhere.

- [ ] **Step 2: Confirm the test fails to compile**

Run: `cargo test --lib archive_with_app_removes_workspace_and_worktree`
Expected: build fails with `cannot find function archive_with_app in this scope`.

- [ ] **Step 3: Implement `archive_with_app`**

In `src/workspace.rs`, the existing `archive` function ends at line 278. Add the new function immediately after it (before `pub async fn discover_untracked` at line 281):

```rust
/// TUI-friendly variant of `archive` that interleaves App lock acquisition
/// with the long-running async git/script phases. Unlike `archive`, this
/// function never holds the App lock across `.await` boundaries on the
/// archive script or `git worktree remove`, so the event loop can continue
/// to tick and redraw.
pub async fn archive_with_app(
    app: crate::app::SharedApp,
    repo: Repo,
    ws: Workspace,
    opts: ArchiveOpts,
) -> Result<SetupResult> {
    // --- Phase 1 (unlocked, async): run the archive script if any. ---
    let archive_result = setup::run_archive(
        repo.archive_script.as_deref(),
        &repo.path,
        &ws.worktree_path,
        tokio_util::sync::CancellationToken::new(),
        |_| {},
    )
    .await?;

    // --- Phase 2 (unlocked, async): remove the worktree from disk. ---
    if !opts.keep_worktree && ws.worktree_path.exists() {
        git::remove_worktree(&repo.path, &ws.worktree_path).await?;
    }

    // --- Phase 3 (unlocked, async): delete the branch. Failures here
    //     are non-fatal and intentionally swallowed, matching `archive`. ---
    let _ = git::branch_delete(&repo.path, &ws.branch, opts.force_branch_delete).await;

    // --- Phase 4 (short, locked): delete the store row + clean up MCP. ---
    {
        let g = app.lock().await;
        g.store.delete_workspace(ws.id)?;
        if crate::mcp::enabled(&g.store)
            && let Err(e) = crate::mcp::remove_worktree_entry(&ws.worktree_path)
        {
            tracing::warn!(error = %e, "failed to remove worktree entry from ~/.claude.json");
        }
    }

    Ok(archive_result)
}
```

Notes:
- The body is a one-to-one decomposition of the existing `archive` function (lines 252-278) into the same phase shape `create_with_app` uses. No new behavior is introduced.
- `crate::mcp::enabled(&g.store)` takes `&Store`; `crate::mcp::remove_worktree_entry(&path)` takes a path. Both are quick; running them inside the lock is fine.
- The lock acquisition in Phase 4 is the only place we touch the mutex; the slow git work in Phases 1-3 runs entirely unlocked.

- [ ] **Step 4: Verify the test passes**

Run: `cargo test --lib archive_with_app_removes_workspace_and_worktree`
Expected: PASS.

- [ ] **Step 5: Run the full workspace test suite**

Run: `cargo test --workspace`
Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/workspace.rs
git commit -m "feat(workspace): add archive_with_app for TUI-path archive"
```

---

## Task 5: Wire spawn-and-reconcile into the `ConfirmArchive` handler

The user-visible change: pressing `y` in the archive-confirm modal transitions to the spinner immediately, with the archive running on a background task via `archive_with_app`. TDD: write the integration test that asserts the immediate transition, watch it fail, then rewrite the handler.

**Files:**
- Modify: `src/app/input.rs`
- Modify: `src/app/input_tests.rs`

- [ ] **Step 1: Write the failing integration test**

In `src/app/input_tests.rs`, scroll to a location that matches the surrounding test style. A good spot is immediately after the `esc_in_setup_running_cancels_and_closes_modal` test (around line 2372). Add this new test:

```rust
    #[tokio::test]
    async fn y_in_confirm_archive_transitions_to_archive_running_and_spawns_task() {
        use crate::ui::modal::Modal;
        use std::sync::Arc;
        use tokio::sync::Mutex;
        let store = crate::store::Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let repo_id = crate::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        // Create the workspace BEFORE wrapping the store in the App, since
        // App::new takes the store by value.
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == repo_id)
            .unwrap();
        let created = crate::workspace::create(
            &store,
            &repo,
            Some("doomed"),
            tmp.path(),
            false,
            crate::pty::session::AgentKind::Claude,
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        let ws_id = created.workspace.id;
        let app = Arc::new(Mutex::new(
            App::new(store, tmp.path().to_path_buf()).unwrap(),
        ));
        // Open the ConfirmArchive modal.
        {
            let mut g = app.lock().await;
            g.modal = Some(Modal::ConfirmArchive {
                workspace_id: ws_id,
                name: created.workspace.name.clone(),
            });
        }
        // Send 'y'.
        let evt = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('y'),
            crossterm::event::KeyModifiers::empty(),
        );
        {
            let mut g = app.lock().await;
            handle_event(&mut g, &app, CtEvent::Key(evt)).await.unwrap();
            // Immediately after 'y', modal should be ArchiveRunning.
            assert!(
                matches!(g.modal, Some(Modal::ArchiveRunning)),
                "modal should transition to ArchiveRunning immediately; got {:?}",
                g.modal
            );
            assert!(g.pending_archive_gen.is_some());
        }
        // Yield so the spawned archive task can complete.
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        // Eventually, modal should be None and the workspace should be gone.
        let g = app.lock().await;
        assert!(
            g.modal.is_none(),
            "modal should clear after archive succeeds; got {:?}",
            g.modal
        );
        assert!(g.pending_archive_gen.is_none());
        assert!(
            g.workspaces.iter().all(|(_, w)| w.id != ws_id),
            "archived workspace should be removed from app.workspaces"
        );
    }
```

Notes for this test:
- We create the workspace via the CLI-path `create(&store, ...)` BEFORE moving the store into `App::new`, because `App::new` consumes the store. After `App::new`, future store access goes through the lock.
- `init_git_repo()` and `handle_event` / `CtEvent` are already used by the surrounding tests (see `enter_in_new_workspace_modal_transitions_to_setup_running_and_spawns_task` at line 2263).
- The 1500ms sleep matches the create test's timing for slow CI runners; archive should normally finish in well under that.

- [ ] **Step 2: Confirm the test fails**

Run: `cargo test --lib y_in_confirm_archive_transitions_to_archive_running_and_spawns_task`
Expected: assertion failure at the immediate-transition check — the current handler `await`s the archive synchronously and only clears the modal afterwards, so the modal will be `None` (or `Error`) by the time we check, not `ArchiveRunning`.

- [ ] **Step 3: Rewrite the `'y'` branch of `ConfirmArchive`**

In `src/app/input.rs`, the `Modal::ConfirmArchive` arm starts at line 902. The `'y'` branch currently runs from line 903 to line 944, awaiting `crate::workspace::archive(...)` synchronously inside the handler. Replace that entire `KeyCode::Char('y') => { ... }` block with the spawn-based version:

```rust
            KeyCode::Char('y') => {
                let (repo, ws) = {
                    let ws = app
                        .workspaces
                        .iter()
                        .find(|(_, w)| w.id == workspace_id)
                        .map(|(_, w)| w.clone());
                    let repo = ws
                        .as_ref()
                        .and_then(|w| app.repos.iter().find(|r| r.id == w.repo_id).cloned());
                    match (repo, ws) {
                        (Some(r), Some(w)) => (r, w),
                        _ => {
                            app.modal = None;
                            return Ok(());
                        }
                    }
                };
                let archive_gen = app.alloc_archive_gen();
                app.modal = Some(Modal::ArchiveRunning);
                let shared_clone = shared.clone();
                let _ = name;
                tokio::spawn(async move {
                    let result = crate::workspace::archive_with_app(
                        shared_clone.clone(),
                        repo,
                        ws,
                        crate::workspace::ArchiveOpts {
                            force_branch_delete: true,
                            ..Default::default()
                        },
                    )
                    .await;
                    crate::app::reconcile_archive_result(shared_clone, archive_gen, result)
                        .await;
                });
            }
```

Notes:
- We resolve `(repo, ws)` synchronously while we hold `&mut app` (cheap clones from the in-memory vec). This is the same lookup the old handler did.
- `let _ = name;` matches the existing handler's pattern of suppressing the unused-`name` binding warning.
- The spawned task hands `shared_clone` to `archive_with_app`, which manages the lock interleave internally.

- [ ] **Step 4: Verify the test passes**

Run: `cargo test --lib y_in_confirm_archive_transitions_to_archive_running_and_spawns_task`
Expected: PASS.

- [ ] **Step 5: Run the full test suite**

Run: `cargo test --workspace`
Expected: all tests pass. Watch in particular for any test that previously relied on `ConfirmArchive`'s `'y'` being synchronous — if a test asserts post-archive state immediately after sending `y`, it now needs to wait for the spawned task. Search with: `grep -rn "ConfirmArchive" src/ --include="*.rs"`. Update any such test to await briefly (e.g. `tokio::time::sleep(Duration::from_millis(500)).await;`) before asserting.

- [ ] **Step 6: Commit**

```bash
git add src/app/input.rs src/app/input_tests.rs
git commit -m "feat(archive): spawn archive on tokio task with spinner"
```

---

## Task 6: Add the Esc-no-op test

This locks in the deliberate Esc-during-archive behavior so a future change cannot accidentally re-introduce cancellation without us noticing.

**Files:**
- Modify: `src/app/input_tests.rs`

- [ ] **Step 1: Write the test**

In `src/app/input_tests.rs`, immediately after the test added in Task 5, add:

```rust
    #[tokio::test]
    async fn esc_in_archive_running_is_noop() {
        use crate::ui::modal::Modal;
        use std::sync::Arc;
        use tokio::sync::Mutex;
        let store = crate::store::Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let repo_id = crate::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        // Give the archive a slow archive-script so it's still running
        // when we press Esc.
        store
            .set_repo_archive_script(repo_id, Some("sleep 1"))
            .unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let app = Arc::new(Mutex::new(
            App::new(store, tmp.path().to_path_buf()).unwrap(),
        ));
        let ws_id = {
            let mut g = app.lock().await;
            let repo = g.repos.iter().find(|r| r.id == repo_id).unwrap().clone();
            let base = g.worktree_base.clone();
            let created = crate::workspace::create(
                &g.store,
                &repo,
                Some("doomed"),
                &base,
                false,
                crate::pty::session::AgentKind::Claude,
                tokio_util::sync::CancellationToken::new(),
                |_| {},
            )
            .await
            .unwrap();
            g.refresh().unwrap();
            g.modal = Some(Modal::ConfirmArchive {
                workspace_id: created.workspace.id,
                name: created.workspace.name.clone(),
            });
            created.workspace.id
        };
        // Press 'y' to start archiving.
        {
            let mut g = app.lock().await;
            let y = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('y'),
                crossterm::event::KeyModifiers::empty(),
            );
            handle_event(&mut g, &app, CtEvent::Key(y)).await.unwrap();
            assert!(matches!(g.modal, Some(Modal::ArchiveRunning)));
        }
        // Yield briefly so the archive script kicks off but is still
        // running (sleep 1 gives us a 1s window).
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        // Press Esc — should be a no-op.
        {
            let mut g = app.lock().await;
            let esc = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::empty(),
            );
            handle_event(&mut g, &app, CtEvent::Key(esc)).await.unwrap();
            assert!(
                matches!(g.modal, Some(Modal::ArchiveRunning)),
                "Esc must not close ArchiveRunning; got {:?}",
                g.modal
            );
            assert!(g.pending_archive_gen.is_some());
        }
        // Wait for the archive to actually finish.
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        let g = app.lock().await;
        assert!(g.modal.is_none(), "modal should clear once archive finishes");
        assert!(
            g.workspaces.iter().all(|(_, w)| w.id != ws_id),
            "workspace should be archived"
        );
    }
```

- [ ] **Step 2: Run the test**

Run: `cargo test --lib esc_in_archive_running_is_noop`
Expected: PASS. (The no-op arm was already added in Task 1; this test just locks the contract in.)

- [ ] **Step 3: Run the full suite once more**

Run: `cargo test --workspace`
Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/app/input_tests.rs
git commit -m "test(archive): esc during ArchiveRunning is a no-op"
```

---

## Task 7: Manual smoke test

Automated tests cover state transitions; the spinner animation itself needs eyeballs.

**Files:** none (manual verification)

- [ ] **Step 1: Pick a workspace with slow removal**

Find or create a workspace whose worktree contains a slow-to-delete directory — `node_modules`, `target/`, or just `dd if=/dev/zero of=junk bs=1M count=200` to make ~200MB of dummy data that `git worktree remove` has to delete.

- [ ] **Step 2: Run the TUI**

Run: `cargo run --release`
(Or whatever the project's standard launch command is — check `README.md`.)

- [ ] **Step 3: Trigger the archive flow**

In the TUI, select the slow workspace and press `d` (or the archive shortcut — `src/app/input.rs:486` confirms `d`). Confirm with `y`.

- [ ] **Step 4: Verify spinner behavior**

Check:
- Modal switches to `"  ⠋ Removing workspace…"` (or similar braille frame) **immediately** on `y`.
- The braille glyph animates — frames cycle through `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`.
- Pressing Esc during the spinner does nothing — the modal stays up.
- On completion, the modal closes and the workspace is gone from the dashboard.

If any of those are wrong, return to the relevant task and debug.

- [ ] **Step 5: No commit (manual test)**

If everything works, you're done.

---

## Self-review checklist

Before opening a PR:

- [ ] Spec items 1-3 (success transition, Esc no-op, error reconcile) all have tests.
- [ ] Spec item 4 (gen mismatch staleness) has a unit test.
- [ ] `cargo test --workspace` passes cleanly.
- [ ] `cargo clippy --workspace -- -D warnings` is clean (project standard — check `CLAUDE.md` if unsure).
- [ ] Manual smoke test passed.
- [ ] No `unwrap()` introduced in production code on `lock().await`.
- [ ] The plan introduced **no** changes to `workspace::archive`'s signature (this is intentional — see spec's "Why no `CancellationToken` on `workspace::archive`").
