# New-workspace modal loading indicator — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the TUI's "new workspace" modal visibly indicate work-in-progress with an animated spinner, prevent duplicate-Enter from creating multiple workspaces, and let Esc cancel the in-flight setup script.

**Architecture:** Spawn `workspace::create` in a `tokio::spawn` task so the main event loop's `App` mutex is held only briefly at start and end. Thread a `tokio_util::sync::CancellationToken` through `workspace::create` and into `setup::run_setup` so Esc on the modal can cancel the running child process (`kill_on_drop(true)` already handles process reaping). A generation counter on `App` distinguishes a fresh create from a stale one when reconciling outcomes.

**Tech Stack:** Rust 2021, tokio (multi-thread runtime), tokio-util (new dep, for `CancellationToken`), ratatui, rusqlite.

---

## File Structure

| File | Role |
|---|---|
| `Cargo.toml` | Add `tokio-util` dependency. |
| `src/error.rs` | Add `Error::Cancelled` variant. |
| `src/store.rs` | Add `SetupStatus::Cancelled` variant + persistence. |
| `src/setup.rs` | `run_setup` gains `cancel: CancellationToken` param; the existing `select!` loop gains a cancel arm. |
| `src/workspace.rs` | Keep existing `create()` for the CLI path (unchanged). Add `create_with_app(app: SharedApp, ..., cancel)` for the TUI path that interleaves lock/unlock. Cancellation checked between phases. |
| `src/ui/modal.rs` | `Modal::SetupRunning` variant gains `cancel: CancellationToken`. Renderer shows the existing braille spinner driven by `app.tick` + status line + `(Esc to cancel)` hint. |
| `src/app.rs` | `App` gains `next_create_gen: u64` and `pending_create_gen: Option<u64>`. The `NewWorkspace::Enter` handler spawns the create task and transitions modal. The `SetupRunning::Esc` handler calls `cancel.cancel()` and closes the modal. New helper `reconcile_create_result` runs after the spawned task completes. |

## Conventions

- **TDD:** every behavior task starts with a failing test. Spec test IDs (1–8) are noted in each task.
- **Commits:** one logical change per commit. Run `cargo test --workspace` before each commit.
- **No `unwrap()` on Mutex locks** in production code — use `.expect("…")` with a brief reason, matching the existing codebase style (search `src/app.rs` for `lock().await` patterns).

---

## Task 1: Add tokio-util dependency

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add the dependency**

In `Cargo.toml`, locate the `[dependencies]` section (it contains `tokio = { version = "1", features = ["full"] }` near the top of the deps block at around line 19). Add immediately after the `tokio` line:

```toml
tokio-util = { version = "0.7", default-features = false }
```

We do not need any features — only `CancellationToken` from the root module, which is gated by no feature flag.

- [ ] **Step 2: Verify it builds**

Run: `cargo build`
Expected: completes successfully, downloads `tokio-util` if not in the lock file.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "build: add tokio-util for CancellationToken"
```

---

## Task 2: Add Error::Cancelled variant

**Files:**
- Modify: `src/error.rs`

- [ ] **Step 1: Add the variant**

In `src/error.rs`, the `Error` enum currently ends at line 19. Add a new variant immediately before the closing `}`:

```rust
    #[error("cancelled")]
    Cancelled,
```

The full enum should now look like:

```rust
#[derive(Debug, Error)]
pub enum Error {
    #[error("git: {0}")]
    Git(String),
    #[error("store: {0}")]
    Store(#[from] rusqlite::Error),
    #[error("pty: {0}")]
    Pty(String),
    #[error("setup: {0}")]
    Setup(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid input: {0}")]
    UserInput(String),
    #[error("cancelled")]
    Cancelled,
}
```

- [ ] **Step 2: Verify it builds**

Run: `cargo build`
Expected: success. No call sites match `Error::Cancelled` yet, so exhaustive matches elsewhere may complain — if any do, fix by adding `Error::Cancelled => …` arms that map to a sensible default (most likely just propagate or log). Verify there are no such matches with `grep -rn 'match.*Error\b' src/ | head`.

- [ ] **Step 3: Commit**

```bash
git add src/error.rs
git commit -m "feat(error): add Cancelled variant"
```

---

## Task 3: Add SetupStatus::Cancelled variant + persistence

**Files:**
- Modify: `src/store.rs`
- Test: `src/store.rs` (existing `mod tests`)

- [ ] **Step 1: Write the failing test**

In `src/store.rs`, scroll to the `#[cfg(test)] mod tests` block (starts around line 517). Add this test after the existing `setup_status` tests:

```rust
    #[test]
    fn setup_status_cancelled_roundtrips() {
        let store = Store::open_in_memory().unwrap();
        // Insert a repo + workspace fixture.
        let repo_id = store
            .insert_repo("demo", &PathBuf::from("/tmp/demo"), "wsx")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "alpha",
                branch: "wsx/alpha",
                worktree_path: &PathBuf::from("/tmp/demo/alpha"),
                yolo: false,
            })
            .unwrap();
        store.set_setup_status(id, SetupStatus::Cancelled).unwrap();
        let ws = store.workspaces(repo_id).unwrap();
        assert_eq!(ws[0].setup_status, SetupStatus::Cancelled);
    }
```

If your existing test fixtures use a different repo-insertion pattern, mirror that pattern instead — search `src/store.rs` for `insert_repo` to match the existing call shape.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib store::tests::setup_status_cancelled_roundtrips`
Expected: FAIL with "no variant or associated item named `Cancelled`" (compile error).

- [ ] **Step 3: Add the variant**

In `src/store.rs`, modify the `SetupStatus` enum at line 20–26:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupStatus {
    NotRun,
    Skipped,
    Ok,
    Failed,
    Cancelled,
}
```

Modify `setup_label` at line 499–506:

```rust
fn setup_label(s: &SetupStatus) -> &'static str {
    match s {
        SetupStatus::NotRun => "NotRun",
        SetupStatus::Skipped => "Skipped",
        SetupStatus::Ok => "Ok",
        SetupStatus::Failed => "Failed",
        SetupStatus::Cancelled => "Cancelled",
    }
}
```

Modify `parse_setup` at line 507–514:

```rust
fn parse_setup(s: &str) -> SetupStatus {
    match s {
        "Ok" => SetupStatus::Ok,
        "Failed" => SetupStatus::Failed,
        "Skipped" => SetupStatus::Skipped,
        "Cancelled" => SetupStatus::Cancelled,
        _ => SetupStatus::NotRun,
    }
}
```

No schema migration is needed because the column is stored as TEXT; old rows containing "NotRun"/"Ok"/etc. continue to parse correctly, and new rows can store "Cancelled".

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib store::tests::setup_status_cancelled_roundtrips`
Expected: PASS.

- [ ] **Step 5: Run the full test suite**

Run: `cargo test --workspace`
Expected: PASS. Any `match` on `SetupStatus` elsewhere will now fail to compile; the only known external site is `src/app.rs:675` (`ws.setup_status == crate::store::SetupStatus::Failed`), which is an equality check and unaffected. Verify with: `grep -rn 'SetupStatus::' src/ | grep -v store.rs | grep -v workspace.rs`.

- [ ] **Step 6: Commit**

```bash
git add src/store.rs
git commit -m "feat(store): add SetupStatus::Cancelled"
```

---

## Task 4: Add cancellation parameter to setup::run_setup (spec tests #1, #2)

**Files:**
- Modify: `src/setup.rs`
- Test: `src/setup.rs` (existing `mod tests`)

This task implements spec test #1 (`run_setup_respects_cancellation`) and spec test #2 (`run_setup_completes_before_cancel_is_ignored`).

- [ ] **Step 1: Write the failing test for cancellation respect**

In `src/setup.rs`, locate the `#[cfg(test)] mod tests` block (starts around line 134). Add at the bottom:

```rust
    #[tokio::test]
    async fn run_setup_respects_cancellation() {
        use tokio_util::sync::CancellationToken;
        let tmp = TempDir::new().unwrap();
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            cancel_clone.cancel();
        });
        let start = std::time::Instant::now();
        let result = run_setup(
            Some("sleep 10"),
            tmp.path(),
            tmp.path(),
            cancel,
            |_| {},
        )
        .await;
        let elapsed = start.elapsed();
        assert!(
            matches!(result, Err(Error::Cancelled)),
            "expected Err(Cancelled), got {result:?}"
        );
        assert!(
            elapsed < std::time::Duration::from_millis(1500),
            "expected fast cancel, took {elapsed:?}"
        );
    }
```

Note: the signature this test assumes — `run_setup(script, repo_root, worktree, cancel, on_line)` — does not yet exist. The test will fail to compile until Step 3.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib setup::tests::run_setup_respects_cancellation`
Expected: FAIL with a compile error about argument count / unknown identifier `CancellationToken`.

- [ ] **Step 3: Add the cancellation parameter to run_setup and run_archive**

In `src/setup.rs`, modify `run_setup` at line 19–29:

```rust
pub async fn run_setup<F: FnMut(SetupLine) + Send>(
    script: Option<&str>,
    repo_root: &Path,
    worktree: &Path,
    cancel: tokio_util::sync::CancellationToken,
    on_line: F,
) -> Result<SetupResult> {
    match script {
        Some(s) if !s.trim().is_empty() => {
            run_script(s, repo_root, worktree, cancel, on_line).await
        }
        _ => Ok(SetupResult::Skipped),
    }
}
```

Modify `run_archive` at line 31–41 the same way:

```rust
pub async fn run_archive<F: FnMut(SetupLine) + Send>(
    script: Option<&str>,
    repo_root: &Path,
    worktree: &Path,
    cancel: tokio_util::sync::CancellationToken,
    on_line: F,
) -> Result<SetupResult> {
    match script {
        Some(s) if !s.trim().is_empty() => {
            run_script(s, repo_root, worktree, cancel, on_line).await
        }
        _ => Ok(SetupResult::Skipped),
    }
}
```

Modify `run_script` signature at line 43–48:

```rust
async fn run_script<F: FnMut(SetupLine) + Send>(
    script: &str,
    repo_root: &Path,
    worktree: &Path,
    cancel: tokio_util::sync::CancellationToken,
    mut on_line: F,
) -> Result<SetupResult> {
```

Modify the `tokio::select!` loop inside `run_script` (currently at line 99–112). Add a third arm for cancellation. The loop currently looks like:

```rust
    loop {
        tokio::select! {
            line = out_reader.next_line() => match line {
                Ok(Some(l)) => on_line(SetupLine::Stdout(l)),
                Ok(None) => break,
                Err(e) => return Err(Error::Setup(format!("stdout read: {e}"))),
            },
            line = err_reader.next_line() => match line {
                Ok(Some(l)) => on_line(SetupLine::Stderr(l)),
                Ok(None) => break,
                Err(e) => return Err(Error::Setup(format!("stderr read: {e}"))),
            },
        }
    }
```

Replace with:

```rust
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                // Dropping `child` triggers kill_on_drop. We still return
                // before draining readers; the OS reaps the process.
                return Err(Error::Cancelled);
            }
            line = out_reader.next_line() => match line {
                Ok(Some(l)) => on_line(SetupLine::Stdout(l)),
                Ok(None) => break,
                Err(e) => return Err(Error::Setup(format!("stdout read: {e}"))),
            },
            line = err_reader.next_line() => match line {
                Ok(Some(l)) => on_line(SetupLine::Stderr(l)),
                Ok(None) => break,
                Err(e) => return Err(Error::Setup(format!("stderr read: {e}"))),
            },
        }
    }
```

`biased;` is intentional: when cancellation arrives, the cancel arm always wins over a racy line-ready event, so we don't read one more line before bailing.

- [ ] **Step 4: Update existing test fixtures that call run_setup/run_archive**

Search for existing callers of `run_setup` and `run_archive` in tests within `src/setup.rs` and elsewhere:

Run: `grep -rn 'run_setup\|run_archive' src/ tests/ 2>/dev/null`

For each existing call site (including the existing tests in `src/setup.rs`), add a `CancellationToken::new()` argument in the right position. Example:

```rust
// before:
run_setup(Some("echo hi"), tmp.path(), tmp.path(), |_| {}).await
// after:
run_setup(
    Some("echo hi"),
    tmp.path(),
    tmp.path(),
    tokio_util::sync::CancellationToken::new(),
    |_| {},
).await
```

Do not yet update `workspace::create`'s call to `run_setup` — that comes in Task 5.

- [ ] **Step 5: Run cancellation test to verify it passes**

Run: `cargo test --lib setup::tests::run_setup_respects_cancellation`
Expected: PASS.

- [ ] **Step 6: Add the "late cancel is ignored" test**

Add to the same `mod tests`:

```rust
    #[tokio::test]
    async fn run_setup_completes_before_cancel_is_ignored() {
        use tokio_util::sync::CancellationToken;
        let tmp = TempDir::new().unwrap();
        let cancel = CancellationToken::new();
        let result = run_setup(
            Some("true"),
            tmp.path(),
            tmp.path(),
            cancel.clone(),
            |_| {},
        )
        .await;
        // Cancel arrives long after run_setup has returned.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        cancel.cancel();
        assert!(matches!(result, Ok(SetupResult::Ok)), "got {result:?}");
    }
```

- [ ] **Step 7: Run the test**

Run: `cargo test --lib setup::tests::run_setup_completes_before_cancel_is_ignored`
Expected: PASS.

- [ ] **Step 8: Run the full suite**

Run: `cargo test --workspace`
Expected: PASS. If `workspace::create` callers fail to compile because `setup::run_setup` now takes one more argument, fix `src/workspace.rs` (lines 62–68 and line 100-106) by adding `tokio_util::sync::CancellationToken::new()` as the 4th argument for now — Task 5 will replace these with real tokens.

- [ ] **Step 9: Commit**

```bash
git add src/setup.rs src/workspace.rs
git commit -m "feat(setup): cancellation token wired through run_setup"
```

---

## Task 5: Add cancellation to workspace::create (spec tests #3, #4)

**Files:**
- Modify: `src/workspace.rs`, `src/cli.rs`, `src/app.rs`
- Test: `src/workspace.rs` (existing `mod tests`)

This task adds cancellation to the existing `create()` for both the CLI and TUI callers; the TUI-specific `create_with_app` arrives in Task 7. This task implements spec tests #3 (`create_returns_cancelled_when_token_cancelled_before_start`) and #4 (`create_marks_setup_status_cancelled_when_cancelled_during_setup`).

- [ ] **Step 1: Write the failing test for pre-cancelled token**

In `src/workspace.rs`, locate the `#[cfg(test)] mod tests` (starts around line 204). Add:

```rust
    #[tokio::test]
    async fn create_returns_cancelled_when_token_cancelled_before_start() {
        use tokio_util::sync::CancellationToken;
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let id = crate::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let base = TempDir::new().unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result =
            create(&store, &repo, Some("alpha"), base.path(), false, cancel, |_| {}).await;
        assert!(matches!(result, Err(Error::Cancelled)), "got {result:?}");
        let rows = store.workspaces(id).unwrap();
        assert!(
            rows.is_empty(),
            "no row should be inserted when pre-cancelled"
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib workspace::tests::create_returns_cancelled_when_token_cancelled_before_start`
Expected: FAIL — compile error about argument count on `create()`.

- [ ] **Step 3: Add the cancel parameter to workspace::create**

In `src/workspace.rs`, modify `create` at line 18–25:

```rust
pub async fn create<F: FnMut(SetupLine) + Send>(
    store: &Store,
    repo: &Repo,
    name: Option<&str>,
    worktree_base: &Path,
    yolo: bool,
    cancel: tokio_util::sync::CancellationToken,
    on_setup_line: F,
) -> Result<CreatedWorkspace> {
```

Add checkpoints inside the function body. After the `let worktree_path = …;` line (around line 36), insert:

```rust
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
```

Replace the existing `git::fetch_for_base(...).await?;` line (around line 46) with:

```rust
    git::fetch_for_base(&repo.path, base).await?;
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
```

After `store.set_workspace_state(id, WorkspaceState::Ready)?;` (around line 60), add a cancel check that marks the workspace before bailing:

```rust
    if cancel.is_cancelled() {
        store.set_setup_status(id, SetupStatus::Cancelled)?;
        return Err(Error::Cancelled);
    }
```

Replace the existing `setup::run_setup` call (around line 62) with:

```rust
    let setup_result = setup::run_setup(
        repo.setup_script.as_deref(),
        &repo.path,
        &worktree_path,
        cancel.clone(),
        on_setup_line,
    )
    .await;
```

(Note: we drop the `?` so we can map cancellation to a store update.)

Replace the existing `let status = match &setup_result { … }` block with a flow that handles cancellation specially:

```rust
    let setup_result = match setup_result {
        Ok(r) => r,
        Err(Error::Cancelled) => {
            store.set_setup_status(id, SetupStatus::Cancelled)?;
            return Err(Error::Cancelled);
        }
        Err(e) => return Err(e),
    };
    let status = match &setup_result {
        SetupResult::Ok => SetupStatus::Ok,
        SetupResult::Skipped => SetupStatus::Skipped,
        SetupResult::Failed { .. } => SetupStatus::Failed,
    };
    store.set_setup_status(id, status)?;
```

- [ ] **Step 4: Update remaining run_setup/run_archive call sites in workspace.rs**

`archive()` at line 93 calls `setup::run_archive`. Since archive does not currently take a cancel token from callers, pass a fresh token to keep the call compiling:

```rust
    let archive_result = setup::run_archive(
        repo.archive_script.as_deref(),
        &repo.path,
        &ws.worktree_path,
        tokio_util::sync::CancellationToken::new(),
        on_archive_line,
    )
    .await?;
```

- [ ] **Step 5: Update the CLI caller**

In `src/cli.rs:740`, modify the call:

```rust
            let created = crate::workspace::create(
                &store,
                &r,
                name.as_deref(),
                &worktree_base,
                yolo,
                tokio_util::sync::CancellationToken::new(),
                |_| {},
            )
            .await?;
```

- [ ] **Step 6: Update the existing TUI caller (placeholder, will be replaced in Task 7)**

In `src/app.rs:2191`, change the call to pass `tokio_util::sync::CancellationToken::new()` as the new argument. This is a temporary scaffolding — Task 7 replaces this whole handler with `create_with_app`.

```rust
                let result = crate::workspace::create(
                    &app.store,
                    &repo,
                    name.as_deref(),
                    &base,
                    yolo,
                    tokio_util::sync::CancellationToken::new(),
                    |_| {},
                )
                .await;
```

- [ ] **Step 7: Update existing tests in src/workspace.rs**

Search for existing `create(` calls in the test module (`grep -n 'create(' src/workspace.rs | head -20`). For each, add a `tokio_util::sync::CancellationToken::new()` argument in the new position (6th argument, before the closure). Example, at line 243:

```rust
        let created = create(
            &store,
            &repo,
            Some("alpha"),
            base.path(),
            false,
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
```

Repeat for every existing test that calls `create(...)`.

- [ ] **Step 8: Run the pre-cancel test**

Run: `cargo test --lib workspace::tests::create_returns_cancelled_when_token_cancelled_before_start`
Expected: PASS.

- [ ] **Step 9: Write the failing test for mid-setup cancellation**

Add to `src/workspace.rs` test module:

```rust
    #[tokio::test]
    async fn create_marks_setup_status_cancelled_when_cancelled_during_setup() {
        use tokio_util::sync::CancellationToken;
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let id = crate::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        // Configure a slow setup script via the store.
        store
            .set_repo_setup_script(id, Some("sleep 10"))
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let base = TempDir::new().unwrap();
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            cancel_clone.cancel();
        });
        let result =
            create(&store, &repo, Some("alpha"), base.path(), false, cancel, |_| {}).await;
        assert!(matches!(result, Err(Error::Cancelled)), "got {result:?}");
        let rows = store.workspaces(id).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].setup_status, SetupStatus::Cancelled);
        assert_eq!(rows[0].state, WorkspaceState::Ready);
        assert!(rows[0].worktree_path.exists(), "worktree should remain on disk");
    }
```

Note: the test references `store.set_repo_setup_script(id, Some("sleep 10"))`. If this method has a different name in your codebase, search for the equivalent: `grep -n 'setup_script\|set_repo' src/store.rs`. Use whatever setter exists, or update the repo via the same path the existing `repo::add` followed by a manual UPDATE if needed.

- [ ] **Step 10: Run test to verify it passes**

Run: `cargo test --lib workspace::tests::create_marks_setup_status_cancelled_when_cancelled_during_setup`
Expected: PASS.

- [ ] **Step 11: Run the full suite**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 12: Commit**

```bash
git add src/workspace.rs src/cli.rs src/app.rs
git commit -m "feat(workspace): cancellation token wired through create"
```

---

## Task 6: Update Modal::SetupRunning to hold cancel token + render spinner

**Files:**
- Modify: `src/ui/modal.rs`
- Modify: `src/app.rs` (the existing placeholder use of `SetupRunning` at line 2188)

- [ ] **Step 1: Modify the modal variant**

In `src/ui/modal.rs`, replace `SetupRunning` at line 22-24:

```rust
    SetupRunning {
        cancel: tokio_util::sync::CancellationToken,
    },
```

We drop `log: Vec<String>` — single-status-line design (per the spec's non-goals: no log streaming).

- [ ] **Step 2: Update the renderer**

In `src/ui/modal.rs`, replace the `SetupRunning` arm of the `render` function (currently lines 90–94):

```rust
        Modal::SetupRunning { .. } => {
            let frame = crate::ui::dashboard::spinner::frame(tick);
            let body = format!(
                "  {frame} Creating workspace…\n\n  [esc] cancel",
            );
            ("new workspace", body)
        }
```

This requires `render` to accept the `tick` value. Modify the `render` signature at line 62:

```rust
pub fn render(f: &mut Frame, area: Rect, modal: &Modal, tick: u32, theme: &Theme) {
```

- [ ] **Step 3: Update the renderer's caller**

In `src/app.rs`, find the call to `modal::render` (around line 1057 per Explore agent's report). Update to pass `app.tick`:

```rust
modal::render(f, area, other, app.tick, &app.theme);
```

- [ ] **Step 4: Update the placeholder app.rs handler**

Update `src/app.rs:2188` (the `SetupRunning` construction) to pass a token. This is still scaffolding — Task 7 rewrites this handler:

```rust
                app.modal = Some(Modal::SetupRunning {
                    cancel: tokio_util::sync::CancellationToken::new(),
                });
```

- [ ] **Step 5: Verify it builds**

Run: `cargo build`
Expected: success. Any leftover references to `SetupRunning { log: … }` should fail — search and fix: `grep -rn 'SetupRunning' src/`.

- [ ] **Step 6: Run tests**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/ui/modal.rs src/app.rs
git commit -m "feat(ui): SetupRunning modal renders spinner + holds cancel token"
```

---

## Task 7: Add `create_with_app` for the TUI path (interleaved lock/unlock)

**Files:**
- Modify: `src/workspace.rs`
- Test: `src/workspace.rs` (existing test module)

This function is the TUI-specific entry point. It takes `SharedApp` and acquires the lock only briefly between async git/setup phases, so the main event loop can continue to tick and redraw.

- [ ] **Step 1: Write the failing test**

In `src/workspace.rs` test module, add:

```rust
    #[tokio::test]
    async fn create_with_app_works_end_to_end_without_holding_lock() {
        use crate::app::App;
        use std::sync::Arc;
        use tokio::sync::Mutex;
        use tokio_util::sync::CancellationToken;
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        crate::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        let base = TempDir::new().unwrap();
        let app = Arc::new(Mutex::new(
            App::new(store, base.path().to_path_buf()).unwrap(),
        ));
        let repo = {
            let g = app.lock().await;
            g.repos[0].clone()
        };

        let cancel = CancellationToken::new();
        let created = create_with_app(
            app.clone(),
            repo,
            Some("alpha".to_string()),
            base.path().to_path_buf(),
            false,
            cancel,
        )
        .await
        .unwrap();
        assert_eq!(created.workspace.name, "alpha");
        // The lock should NOT be held at this point — we can grab it.
        let g = app.try_lock().expect("lock should be free");
        drop(g);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib workspace::tests::create_with_app_works_end_to_end_without_holding_lock`
Expected: FAIL — `create_with_app` does not exist.

- [ ] **Step 3: Implement `create_with_app`**

In `src/workspace.rs`, add this new function right after the existing `create` function (around line 86):

```rust
/// TUI-friendly variant of `create` that interleaves App lock acquisition
/// with the long-running async git/setup phases. Unlike `create`, this
/// function never holds the App lock across `.await` boundaries on git or
/// setup work, so the event loop can continue to tick and redraw.
///
/// Cancellation: same semantics as `create`. Pre-fetch and pre-insert
/// cancellation returns `Err(Cancelled)` cleanly. Cancellation during
/// setup marks the row `SetupStatus::Cancelled` and leaves the worktree
/// on disk.
pub async fn create_with_app(
    app: crate::app::SharedApp,
    repo: Repo,
    name: Option<String>,
    worktree_base: PathBuf,
    yolo: bool,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<CreatedWorkspace> {
    // --- Phase 1 (short, locked): compute names/paths, no I/O. ---
    let (final_name, branch, worktree_path) = {
        let g = app.lock().await;
        let resolved_name = match name.as_deref() {
            Some(s) if !s.trim().is_empty() => s.trim().to_string(),
            _ => crate::names::generate(),
        };
        let prefix = crate::repo::resolve_branch_prefix(&repo, &g.store)?;
        let branch = if prefix.is_empty() {
            resolved_name.clone()
        } else {
            format!("{}/{}", prefix.trim_end_matches('/'), resolved_name)
        };
        let worktree_path = worktree_base.join(&repo.name).join(&resolved_name);
        (resolved_name, branch, worktree_path)
    };

    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }

    // --- Phase 2 (unlocked, async): fetch base branch. ---
    let base = repo
        .base_branch
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    crate::git::fetch_for_base(&repo.path, base).await?;

    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }

    // --- Phase 3 (short, locked): insert workspace row. ---
    let id = {
        let g = app.lock().await;
        g.store.insert_workspace(&NewWorkspace {
            repo_id: repo.id,
            name: &final_name,
            branch: &branch,
            worktree_path: &worktree_path,
            yolo,
        })?
    };

    // --- Phase 4 (unlocked, async): create worktree. ---
    let worktree_result = crate::git::create_worktree(&repo.path, &branch, base, &worktree_path).await;
    if let Err(e) = worktree_result {
        let g = app.lock().await;
        g.store.set_workspace_state(id, WorkspaceState::Failed)?;
        return Err(e);
    }
    {
        let g = app.lock().await;
        g.store.set_workspace_state(id, WorkspaceState::Ready)?;
    }

    if cancel.is_cancelled() {
        let g = app.lock().await;
        g.store.set_setup_status(id, SetupStatus::Cancelled)?;
        return Err(Error::Cancelled);
    }

    // --- Phase 5 (unlocked, async): run setup script. ---
    let setup_result = setup::run_setup(
        repo.setup_script.as_deref(),
        &repo.path,
        &worktree_path,
        cancel.clone(),
        |_| {},
    )
    .await;
    let setup_result = match setup_result {
        Ok(r) => r,
        Err(Error::Cancelled) => {
            let g = app.lock().await;
            g.store.set_setup_status(id, SetupStatus::Cancelled)?;
            return Err(Error::Cancelled);
        }
        Err(e) => return Err(e),
    };
    let status = match &setup_result {
        SetupResult::Ok => SetupStatus::Ok,
        SetupResult::Skipped => SetupStatus::Skipped,
        SetupResult::Failed { .. } => SetupStatus::Failed,
    };

    // --- Phase 6 (short, locked): finalize. ---
    let ws = {
        let g = app.lock().await;
        g.store.set_setup_status(id, status)?;
        g.store
            .workspaces(repo.id)?
            .into_iter()
            .find(|w| w.id == id)
            .ok_or_else(|| Error::Store(rusqlite::Error::QueryReturnedNoRows))?
    };
    Ok(CreatedWorkspace {
        workspace: ws,
        setup_result,
    })
}
```

- [ ] **Step 4: Verify test passes**

Run: `cargo test --lib workspace::tests::create_with_app_works_end_to_end_without_holding_lock`
Expected: PASS.

- [ ] **Step 5: Run full suite**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/workspace.rs
git commit -m "feat(workspace): create_with_app for TUI lock interleaving"
```

---

## Task 8: Add generation counter to App

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add fields**

In `src/app.rs`, modify the `App` struct (line 194–270). Add these two fields immediately after the existing `pub modal: Option<Modal>,` field at line 198:

```rust
    /// Monotonic counter handed out to in-flight workspace creation tasks.
    pub next_create_gen: u64,
    /// Generation id of the currently in-flight workspace creation, if any.
    /// Used by the reconcile step to detect stale completions (user cancelled,
    /// new create started, etc.).
    pub pending_create_gen: Option<u64>,
```

- [ ] **Step 2: Initialize the fields**

In `App::new()` (line 273–327), add the fields to the struct literal at around line 311 (after `pm_auto_summary_sent: false,`):

```rust
            next_create_gen: 0,
            pending_create_gen: None,
```

- [ ] **Step 3: Add an allocator method**

In the `impl App` block, add a helper method near the existing `pub fn refresh` (around line 329):

```rust
    /// Allocate a fresh generation id for a new workspace-creation task.
    pub fn alloc_create_gen(&mut self) -> u64 {
        let g = self.next_create_gen;
        self.next_create_gen = self.next_create_gen.wrapping_add(1);
        self.pending_create_gen = Some(g);
        g
    }
```

- [ ] **Step 4: Verify it builds**

Run: `cargo build`
Expected: success.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): generation counter for in-flight workspace creates"
```

---

## Task 9: Wire Enter handler to spawn create task (spec test #5)

**Files:**
- Modify: `src/app.rs`
- Test: `src/app.rs` (existing test module that includes the `handle_event` pattern at line 4274)

This task implements spec test #5 (`enter_in_new_workspace_modal_transitions_to_setup_running_and_spawns_task`).

- [ ] **Step 1: Add a helper for reconciliation**

In `src/app.rs`, near `pub async fn branch_drift_poll` (around line 2514), add a new helper function:

```rust
/// Reconcile the outcome of a spawned `workspace::create_with_app` task.
/// Locks the app briefly; if the modal is still `SetupRunning` AND the
/// generation matches ours, applies the outcome (close modal on success,
/// switch to `Modal::Error` on failure). Otherwise — user dismissed or
/// started a new create — leaves the modal alone but still calls
/// `refresh()` so the dashboard reflects any state we wrote to the store.
async fn reconcile_create_result(
    app: SharedApp,
    my_gen: u64,
    result: Result<crate::workspace::CreatedWorkspace>,
) {
    let mut g = app.lock().await;
    let is_mine = g.pending_create_gen == Some(my_gen);
    if is_mine {
        g.pending_create_gen = None;
    }
    match result {
        Ok(_) => {
            if is_mine && matches!(g.modal, Some(crate::ui::modal::Modal::SetupRunning { .. })) {
                g.modal = None;
            }
            let _ = g.refresh();
        }
        Err(crate::error::Error::Cancelled) => {
            // User cancelled — modal already cleared by Esc handler. Refresh
            // so the dashboard reflects setup_status=Cancelled.
            let _ = g.refresh();
        }
        Err(e) => {
            if is_mine && matches!(g.modal, Some(crate::ui::modal::Modal::SetupRunning { .. })) {
                g.modal = Some(crate::ui::modal::Modal::Error {
                    message: e.to_string(),
                });
            }
            let _ = g.refresh();
        }
    }
}
```

Note: this function needs to be at module scope, not inside `impl App`. Place it next to other module-scope async helpers.

- [ ] **Step 2: Modify `handle_key_modal` for the Enter case**

`handle_key_modal` currently takes `app: &mut App`. To spawn a background task that calls `reconcile_create_result(app: SharedApp, ...)`, we need the `SharedApp` — but the caller (in `handle_event` at `app.rs:617-620`) only has a `MutexGuard<App>`. The cleanest fix is to thread the `SharedApp` clone through to `handle_event` and `handle_key_modal`.

Look at the call chain at line 617–620:

```rust
            maybe_evt = events.next() => {
                let Some(Ok(evt)) = maybe_evt else { break; };
                let mut g = app.lock().await;
                handle_event(&mut g, evt).await?;
            }
```

Change to pass the SharedApp too:

```rust
            maybe_evt = events.next() => {
                let Some(Ok(evt)) = maybe_evt else { break; };
                let mut g = app.lock().await;
                handle_event(&mut g, &app, evt).await?;
            }
```

Modify `handle_event` signature at line 1215:

```rust
async fn handle_event(app: &mut App, shared: &SharedApp, evt: CtEvent) -> Result<()> {
```

Pass `shared` through to `handle_key_modal`. Modify `handle_key_modal` signature at line 2165:

```rust
async fn handle_key_modal(
    app: &mut App,
    shared: &SharedApp,
    k: crossterm::event::KeyEvent,
) -> Result<()> {
```

Find the call to `handle_key_modal` inside `handle_event` (search for `handle_key_modal(` in `app.rs`). Update it to pass `shared`:

```rust
handle_key_modal(app, shared, k).await?;
```

- [ ] **Step 3: Replace the Enter handler body**

In `src/app.rs`, locate `KeyCode::Enter =>` inside `handle_key_modal`'s `NewWorkspace` arm (lines 2176–2210). Replace the entire arm body with:

```rust
            KeyCode::Enter => {
                let name = if name_buffer.trim().is_empty() {
                    None
                } else {
                    Some(name_buffer.clone())
                };
                let repo = app.repos.iter().find(|r| r.id == repo_id).unwrap().clone();
                let base = app.worktree_base.clone();
                let cancel = tokio_util::sync::CancellationToken::new();
                let gen = app.alloc_create_gen();
                app.modal = Some(Modal::SetupRunning {
                    cancel: cancel.clone(),
                });
                let shared_clone = shared.clone();
                tokio::spawn(async move {
                    let result = crate::workspace::create_with_app(
                        shared_clone.clone(),
                        repo,
                        name,
                        base,
                        yolo,
                        cancel,
                    )
                    .await;
                    reconcile_create_result(shared_clone, gen, result).await;
                });
            }
```

- [ ] **Step 4: Write the failing integration test**

Find the existing `handle_event` test fixtures (look at the test starting near line 4274). Build a similar one in the same test module:

```rust
    #[tokio::test]
    async fn enter_in_new_workspace_modal_transitions_to_setup_running_and_spawns_task() {
        use crate::store::RepoId;
        use crate::ui::modal::Modal;
        use std::sync::Arc;
        use tokio::sync::Mutex;
        // Reuse the existing test-app builder pattern from this module.
        // If a helper like `make_test_app` exists, use it; otherwise
        // construct App directly with an in-memory store.
        let store = crate::store::Store::open_in_memory().unwrap();
        let repo_dir = workspace_tests_helper_init_git_repo();
        let repo_id = crate::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let app = Arc::new(Mutex::new(
            App::new(store, tmp.path().to_path_buf()).unwrap(),
        ));
        {
            let mut g = app.lock().await;
            g.modal = Some(Modal::NewWorkspace {
                repo_id,
                name_buffer: "alpha".to_string(),
                yolo: false,
            });
        }
        // Send Enter.
        let evt = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::empty(),
        );
        {
            let mut g = app.lock().await;
            handle_event(&mut g, &app, CtEvent::Key(evt)).await.unwrap();
            // Immediately after Enter, modal should be SetupRunning.
            assert!(matches!(g.modal, Some(Modal::SetupRunning { .. })));
            assert!(g.pending_create_gen.is_some());
        }
        // Yield so the spawned task gets a chance to complete.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        // Eventually, modal should be None and a workspace should exist.
        let g = app.lock().await;
        assert!(g.modal.is_none(), "modal should clear after create succeeds");
        assert!(g.pending_create_gen.is_none());
        assert_eq!(g.workspaces.len(), 1);
        let _ = repo_id; // suppress unused warning if not referenced above
    }
```

`workspace_tests_helper_init_git_repo` is a stand-in for whatever test helper exists in `src/app.rs` for creating a git repo. Search for the closest existing helper (`grep -n 'fn init_git_repo\|fn make_test_app' src/app.rs`) and either reuse it or copy the implementation from `src/workspace.rs:209-226`.

- [ ] **Step 5: Update existing handle_event tests**

The signature change to `handle_event(app, shared, evt)` breaks any existing test that calls `handle_event(&mut app, …)`. Find them with `grep -n 'handle_event(' src/app.rs | grep -v '//'`. The existing tests use a bare `&mut App`, not a `SharedApp`. For each existing test, wrap the app in `Arc::new(Mutex::new(...))` and pass it through. Example pattern:

```rust
// before:
handle_event(&mut app, CtEvent::Paste("hello paste".into())).await
// after:
let shared = Arc::new(Mutex::new(app));
let mut g = shared.lock().await;
handle_event(&mut g, &shared, CtEvent::Paste("hello paste".into())).await
```

If a test only needs the App value back out at the end, drop the guard first: `drop(g); let app = Arc::try_unwrap(shared).unwrap().into_inner();`.

- [ ] **Step 6: Run the test**

Run: `cargo test --lib enter_in_new_workspace_modal_transitions_to_setup_running_and_spawns_task`
Expected: PASS.

- [ ] **Step 7: Run the full suite**

Run: `cargo test --workspace`
Expected: PASS, including the migrated existing tests.

- [ ] **Step 8: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): spawn workspace::create_with_app from Enter handler"
```

---

## Task 10: Wire Esc handler to cancel + ignore Enter during SetupRunning (spec tests #6, #7, #8)

**Files:**
- Modify: `src/app.rs`
- Test: `src/app.rs` (test module)

This task implements spec test #6 (`esc_in_setup_running_cancels_and_closes_modal`), spec test #7 (`enter_during_setup_running_is_a_noop`), and spec test #8 (`successful_create_after_esc_does_not_show_error_modal`).

- [ ] **Step 1: Add SetupRunning arm to handle_key_modal**

In `src/app.rs`, find the `match modal { Modal::NewWorkspace { … } => match k.code { … } …}` block. After the `NewWorkspace` arm, add (or replace if a stub exists) a `SetupRunning` arm:

```rust
        Modal::SetupRunning { cancel } => match k.code {
            KeyCode::Esc => {
                cancel.cancel();
                app.modal = None;
                app.pending_create_gen = None;
            }
            // Enter (and any other key) is intentionally ignored during creation.
            _ => {}
        },
```

- [ ] **Step 2: Write the failing test for Esc cancellation**

In the `app.rs` test module, add:

```rust
    #[tokio::test]
    async fn esc_in_setup_running_cancels_and_closes_modal() {
        use crate::ui::modal::Modal;
        use std::sync::Arc;
        use tokio::sync::Mutex;
        let store = crate::store::Store::open_in_memory().unwrap();
        let repo_dir = workspace_tests_helper_init_git_repo();
        let repo_id = crate::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        store.set_repo_setup_script(repo_id, Some("sleep 5")).unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let app = Arc::new(Mutex::new(
            App::new(store, tmp.path().to_path_buf()).unwrap(),
        ));
        // Open the modal and press Enter.
        {
            let mut g = app.lock().await;
            g.modal = Some(Modal::NewWorkspace {
                repo_id,
                name_buffer: "alpha".to_string(),
                yolo: false,
            });
            let enter = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::empty(),
            );
            handle_event(&mut g, &app, CtEvent::Key(enter)).await.unwrap();
            assert!(matches!(g.modal, Some(Modal::SetupRunning { .. })));
        }
        // Brief yield so the spawned task gets to start the setup script.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        // Press Esc.
        {
            let mut g = app.lock().await;
            let esc = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::empty(),
            );
            handle_event(&mut g, &app, CtEvent::Key(esc)).await.unwrap();
            assert!(g.modal.is_none(), "modal should close immediately on Esc");
            assert!(g.pending_create_gen.is_none());
        }
        // Wait for the spawned task to wind down.
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        let g = app.lock().await;
        assert_eq!(g.workspaces.len(), 1);
        assert_eq!(
            g.workspaces[0].1.setup_status,
            crate::store::SetupStatus::Cancelled
        );
        // Modal should still be None — the late reconcile must not pop an error.
        assert!(g.modal.is_none());
    }
```

If `set_repo_setup_script` does not exist, look for the equivalent method or set the script via a direct SQL update in the test (use whatever helper the existing `repo_settings` tests use).

- [ ] **Step 3: Run the test**

Run: `cargo test --lib esc_in_setup_running_cancels_and_closes_modal`
Expected: PASS.

- [ ] **Step 4: Write the failing test for Enter during SetupRunning**

```rust
    #[tokio::test]
    async fn enter_during_setup_running_is_a_noop() {
        use crate::ui::modal::Modal;
        use std::sync::Arc;
        use tokio::sync::Mutex;
        let store = crate::store::Store::open_in_memory().unwrap();
        let repo_dir = workspace_tests_helper_init_git_repo();
        let repo_id = crate::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        store.set_repo_setup_script(repo_id, Some("sleep 1")).unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let app = Arc::new(Mutex::new(
            App::new(store, tmp.path().to_path_buf()).unwrap(),
        ));
        {
            let mut g = app.lock().await;
            g.modal = Some(Modal::NewWorkspace {
                repo_id,
                name_buffer: "alpha".to_string(),
                yolo: false,
            });
            let enter = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::empty(),
            );
            handle_event(&mut g, &app, CtEvent::Key(enter)).await.unwrap();
            // Press Enter again — should not spawn a second create.
            handle_event(&mut g, &app, CtEvent::Key(enter)).await.unwrap();
            handle_event(&mut g, &app, CtEvent::Key(enter)).await.unwrap();
        }
        // Wait for the (single) setup to finish.
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        let g = app.lock().await;
        assert_eq!(
            g.workspaces.len(),
            1,
            "exactly one workspace should be created"
        );
    }
```

- [ ] **Step 5: Run the test**

Run: `cargo test --lib enter_during_setup_running_is_a_noop`
Expected: PASS.

- [ ] **Step 6: Write the failing test for race-condition resilience**

```rust
    #[tokio::test]
    async fn successful_create_after_esc_does_not_show_error_modal() {
        use crate::ui::modal::Modal;
        use std::sync::Arc;
        use tokio::sync::Mutex;
        let store = crate::store::Store::open_in_memory().unwrap();
        let repo_dir = workspace_tests_helper_init_git_repo();
        let repo_id = crate::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        // No setup script — create is very fast.
        let tmp = tempfile::TempDir::new().unwrap();
        let app = Arc::new(Mutex::new(
            App::new(store, tmp.path().to_path_buf()).unwrap(),
        ));
        {
            let mut g = app.lock().await;
            g.modal = Some(Modal::NewWorkspace {
                repo_id,
                name_buffer: "alpha".to_string(),
                yolo: false,
            });
            let enter = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::empty(),
            );
            handle_event(&mut g, &app, CtEvent::Key(enter)).await.unwrap();
            // Immediately Esc — race against the spawned create completing.
            let esc = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::empty(),
            );
            handle_event(&mut g, &app, CtEvent::Key(esc)).await.unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let g = app.lock().await;
        // Regardless of which side won the race, modal must not be Error.
        assert!(
            !matches!(g.modal, Some(Modal::Error { .. })),
            "Esc race should never produce an error modal, got {:?}",
            g.modal
        );
    }
```

- [ ] **Step 7: Run the test**

Run: `cargo test --lib successful_create_after_esc_does_not_show_error_modal`
Expected: PASS.

- [ ] **Step 8: Run the full suite**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): Esc cancels in-flight create; Enter ignored during SetupRunning"
```

---

## Task 11: Manual smoke test

**Files:** none

This is verification work, not code.

- [ ] **Step 1: Set up a repo with a slow setup script**

In a real repo registered with wsx, edit the setup script (via `wsx config edit` or the repo-settings modal) to `sleep 5 && echo done`.

- [ ] **Step 2: Run the TUI**

Run: `cargo run --release`

- [ ] **Step 3: Verify each behavior**

In the TUI:

1. Press `n` to open the new-workspace modal, type a name, press Enter.
2. Confirm the modal **immediately** switches to the spinner view ("Creating workspace…" with an animated braille glyph).
3. Press Enter several more times rapidly. Confirm only ONE workspace is created.
4. Open a new modal again with `n`, name it, Enter, then immediately press Esc. Confirm:
   - Modal closes within a frame (~16ms).
   - The dashboard shows the workspace row with `setup_status=Cancelled`.
   - No error modal appears.
5. Open one more modal, name it, Enter, let it run to completion. Confirm the modal closes on success and the workspace appears with `setup_status=Ok` (or `Skipped` if no setup script is configured).

- [ ] **Step 4: Document any deviations**

If any step fails to behave as described, capture the failure mode (screen recording, log output, exact reproduction steps) and open follow-up tasks before claiming completion.

---

## Self-Review Checklist (run before claiming done)

- [ ] Every spec requirement maps to a task: animated spinner (Task 6), duplicate-Enter prevention (Task 10), Esc cancellation (Task 10), `SetupStatus::Cancelled` persistence (Task 3), worktree-left-on-cancel (Task 7), no error modal after Esc race (Task 10), staleness via generation counter (Task 8 + Task 9 reconcile).
- [ ] Every test number from the spec is implemented: #1, #2 (Task 4), #3, #4 (Task 5), #5 (Task 9), #6, #7, #8 (Task 10).
- [ ] No leftover `tokio_util::sync::CancellationToken::new()` placeholders in production code (only in tests). Specifically check `src/app.rs` Enter handler post-Task-9 (should use the real `cancel` token, not a fresh one) and `src/workspace.rs:archive` (acceptable, archive doesn't expose cancellation to its callers yet).
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [ ] `cargo fmt --check` clean.
