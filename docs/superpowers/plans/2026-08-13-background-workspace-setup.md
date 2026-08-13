# Background Workspace Setup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make workspace create and archive run in the background so the dashboard is usable immediately instead of blocked behind a modal for ~20 seconds at each end.

**Architecture:** The async work already exists and is already correct — `create_with_app` and `archive_with_app` are detached `tokio` tasks that never hold the App lock across an `.await`. The blocker is ownership: `SharedProgress` and the `CancellationToken` live inside `Modal::SetupRunning`, so closing the modal drops the only handle to the running work. This plan hoists those handles into an `App`-owned `in_flight` registry, makes the modal a pure viewer, moves the DB row insert ahead of the `git fetch` so the row can appear instantly, and adds dashboard badges for the in-flight and failed lifecycle states.

**Tech Stack:** Rust (edition 2024), tokio, ratatui, rusqlite, tokio-util `CancellationToken`.

**Spec:** `docs/superpowers/specs/2026-08-13-background-workspace-setup-design.md`

## Global Constraints

- Rust edition 2024; MSRV 1.85 (`rust-toolchain.toml`, `Cargo.toml`).
- CI gates every commit — all four must pass before you commit:
  - `cargo fmt --all --check`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo test --all-targets --all-features`
  - `cargo test --doc --all-features`
- Tests live in inline `#[cfg(test)] mod tests` blocks in the file under test. This repo has no `tests/` convention for unit work; follow the file you are editing.
- **Never add `Co-Authored-By` or "Generated with Claude Code" trailers to commits.** Plain conventional-commit messages only.
- Never hold the `App` lock across an `.await`. Every existing async path in `src/data/workspace.rs` is structured in "phases" for this reason; preserve that structure.
- `SetupProgress` uses `std::sync::Mutex`, not tokio's, deliberately — both the writer (`on_line` callback) and the reader (`render`) are synchronous. Do not convert it.

---

### Task 1: Persist `SetupStatus::Running` and sweep stale rows at startup

Adds the variant that records "setup is running" on disk, plus the startup sweep that resolves rows orphaned by a crash. Nothing user-visible changes yet — the modal still blocks — but the crash-evidence trail now exists for later tasks to build on.

**Files:**
- Modify: `src/data/store.rs:26-33` (`SetupStatus` enum), `:393-410` (`setup_label` / `parse_setup`), `:313-321` (add `sweep_stale_running` beside `sweep_stale_pending`)
- Modify: `src/data/workspace.rs:108` and `:288` (write `Running` before the setup phase in both `create` and `create_with_app`)
- Modify: `src/app.rs:703-706` (call the new sweep next to the existing one)
- Test: inline `mod tests` in `src/data/store.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `SetupStatus::Running` — new enum variant, persisted as the string `"Running"`.
  - `Store::sweep_stale_running(&self) -> Result<usize>` — flips every `setup_status = 'Running'` row to `'Cancelled'`, returns the number of rows changed.

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block at the bottom of `src/data/store.rs`:

```rust
#[test]
fn running_setup_status_round_trips() {
    assert_eq!(setup_label(&SetupStatus::Running), "Running");
    assert_eq!(parse_setup("Running"), SetupStatus::Running);
}

#[test]
fn sweep_stale_running_resolves_only_running_rows() {
    let store = Store::open_in_memory().unwrap();
    // Raw insert, matching how the other tests in this module make repos
    // (there is no `insert_repo`; `repo::add` is async and wants a real
    // git repo on disk, which this test does not need).
    store
        .conn()
        .execute(
            "INSERT INTO repos (name, path, branch_prefix, created_at) \
             VALUES ('demo','/tmp/wsx-demo','wsx',0)",
            [],
        )
        .unwrap();
    let repo = store.repos().unwrap().into_iter().next().unwrap().id;

    let mut ids = Vec::new();
    for (name, status) in [
        ("a", SetupStatus::Running),
        ("b", SetupStatus::Ok),
        ("c", SetupStatus::Failed),
        ("d", SetupStatus::NotRun),
    ] {
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name,
                branch: &format!("wsx/{name}"),
                worktree_path: &std::path::PathBuf::from(format!("/tmp/demo/{name}")),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        store.set_setup_status(id, status).unwrap();
        ids.push(id);
    }

    assert_eq!(store.sweep_stale_running().unwrap(), 1, "only the Running row");

    let after: Vec<SetupStatus> = ids
        .iter()
        .map(|id| store.workspace_by_id(*id).unwrap().unwrap().setup_status)
        .collect();
    assert_eq!(
        after,
        vec![
            SetupStatus::Cancelled,
            SetupStatus::Ok,
            SetupStatus::Failed,
            SetupStatus::NotRun,
        ],
        "Running becomes Cancelled; every other status is untouched"
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --all-features sweep_stale_running -- --nocapture`
Expected: FAIL to compile — `no variant named 'Running' found for enum 'SetupStatus'`.

- [ ] **Step 3: Add the enum variant and its persistence mapping**

In `src/data/store.rs`, extend the enum (around `:26`):

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupStatus {
    NotRun,
    Skipped,
    Ok,
    Failed,
    Cancelled,
    /// The setup script is running right now, in THIS process. Persisted
    /// only so a crashed process leaves evidence for `sweep_stale_running`
    /// to resolve at the next startup — it is never the source for a
    /// dashboard badge, because it cannot distinguish live work from the
    /// remains of a killed process. `App::in_flight` is that source.
    Running,
}
```

Extend both mapping functions (around `:393-410`):

```rust
fn setup_label(s: &SetupStatus) -> &'static str {
    match s {
        SetupStatus::NotRun => "NotRun",
        SetupStatus::Skipped => "Skipped",
        SetupStatus::Ok => "Ok",
        SetupStatus::Failed => "Failed",
        SetupStatus::Cancelled => "Cancelled",
        SetupStatus::Running => "Running",
    }
}
fn parse_setup(s: &str) -> SetupStatus {
    match s {
        "Ok" => SetupStatus::Ok,
        "Failed" => SetupStatus::Failed,
        "Skipped" => SetupStatus::Skipped,
        "Cancelled" => SetupStatus::Cancelled,
        "Running" => SetupStatus::Running,
        _ => SetupStatus::NotRun,
    }
}
```

Adding a variant to a non-`#[non_exhaustive]` enum breaks every exhaustive `match` on it. Compile after this step and fix each arm the compiler names; treat `Running` the same as `Cancelled` at every site except where a later task says otherwise.

- [ ] **Step 4: Add the sweep**

In `src/data/store.rs`, directly below `sweep_stale_pending` (`:313`):

```rust
/// Resolve setup rows stranded by a crashed process. Unlike
/// `sweep_stale_pending` this takes no age cutoff: `Running` is written
/// only by a live in-process task, so any row still carrying it when we
/// start up belongs to a process that is already gone. Runs once at
/// startup, before the first draw, so the dashboard never renders a
/// spinner for work that died.
pub fn sweep_stale_running(&self) -> Result<usize> {
    let n = self.conn.execute(
        "UPDATE workspaces SET setup_status = 'Cancelled' WHERE setup_status = 'Running'",
        [],
    )?;
    Ok(n)
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --all-features -- sweep_stale_running running_setup_status`
Expected: PASS (2 tests).

- [ ] **Step 6: Write `Running` before the setup phase**

In `src/data/workspace.rs`, in `create`, immediately before the `setup::run_setup(` call at `:108`:

```rust
store.set_setup_status(id, SetupStatus::Running)?;
```

In `create_with_app`, inside the existing Phase 5 block, right after `p.set_phase(SetupPhase::RunningSetup)` (`:288-290`) and before `run_setup_logged`. Take the App lock briefly and release it before the `.await` — do not hold it across the setup run:

```rust
{
    let g = app.lock().await;
    g.store.set_setup_status(id, SetupStatus::Running)?;
}
```

- [ ] **Step 7: Call the sweep at startup**

In `src/app.rs`, beside the existing sweep at `:703-706`:

```rust
// Sweep stale Pending rows from previous runs.
let _ = app
    .store
    .sweep_stale_pending(std::time::Duration::from_secs(300));
// Resolve setup rows stranded by a crashed process (see sweep_stale_running).
let _ = app.store.sweep_stale_running();
```

- [ ] **Step 8: Run the full suite**

Run: `cargo test --all-targets --all-features`
Expected: PASS. The existing assertions at `src/data/workspace.rs:614`, `:709`, `:956` still hold — `Running` is always overwritten by a terminal status before `create` returns.

- [ ] **Step 9: Commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets --all-features -- -D warnings
git add src/data/store.rs src/data/workspace.rs src/app.rs
git commit -m "feat(setup): persist a Running setup status and sweep stale rows

Records SetupStatus::Running for the duration of the setup script so a
crashed process leaves evidence on disk. At startup any row still marked
Running belongs to a process that is gone, so it is resolved to the
existing Cancelled status. Groundwork for backgrounding create: without
it, a killed wsx would leave a row spinning forever."
```

---

### Task 2: Hoist progress and cancellation into an App-owned registry

Pure refactor. The modal still opens on create and still behaves identically — but it now reads its handles from `App` instead of owning them. This is the change that makes every later task possible.

**Files:**
- Create: `src/data/in_flight.rs`
- Modify: `src/data/mod.rs` (register the module)
- Modify: `src/app.rs:388-405` (add the `in_flight` field), constructor near `:560`, `reconcile_create_result` at `:2452`
- Modify: `src/app/input.rs:1260-1291` (register on spawn)
- Modify: `src/ui/modal/mod.rs:72-76` (`SetupRunning` variant), `:301-320` (its renderer)
- Test: inline `mod tests` in `src/data/in_flight.rs`

**Interfaces:**
- Consumes: `SetupStatus::Running` (Task 1) — not used directly here.
- Produces:
  - `crate::data::in_flight::{InFlight, InFlightKind}`
  - `InFlightKind::{Create, Archive}` — `Debug, Clone, Copy, PartialEq, Eq`
  - `InFlight { kind, progress: SharedProgress, cancel: CancellationToken, started: Instant }`, all fields `pub`
  - `InFlight::create(progress, cancel) -> InFlight` and `InFlight::archive(progress, cancel) -> InFlight`
  - `App.in_flight: HashMap<WorkspaceId, InFlight>` — public field
  - `Modal::SetupProgress { workspace_id: WorkspaceId }` — replaces `Modal::SetupRunning`

- [ ] **Step 1: Write the failing test**

Create `src/data/in_flight.rs` with the test block only, so it fails on the missing type:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_archive_constructors_set_their_kind() {
        let c = InFlight::create(
            crate::data::progress::SetupProgress::shared(),
            tokio_util::sync::CancellationToken::new(),
        );
        assert_eq!(c.kind, InFlightKind::Create);

        let a = InFlight::archive(
            crate::data::progress::SetupProgress::shared(),
            tokio_util::sync::CancellationToken::new(),
        );
        assert_eq!(a.kind, InFlightKind::Archive);
    }

    #[test]
    fn cancel_handle_is_shared_with_the_caller() {
        let token = tokio_util::sync::CancellationToken::new();
        let f = InFlight::create(
            crate::data::progress::SetupProgress::shared(),
            token.clone(),
        );
        assert!(!f.cancel.is_cancelled());
        token.cancel();
        assert!(
            f.cancel.is_cancelled(),
            "the registry must hold a live handle, not a detached copy"
        );
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test --all-features in_flight`
Expected: FAIL — `file not found for module 'in_flight'` (until Step 3 registers it), then `cannot find type 'InFlight'`.

- [ ] **Step 3: Write the module**

Prepend to `src/data/in_flight.rs`:

```rust
//! Registry of background workspace work owned by `App`.
//!
//! Create and archive both run as detached tokio tasks. Their progress sink
//! and cancellation token used to live inside `Modal::SetupRunning`, which
//! meant closing the modal dropped the only handle to the running work —
//! the reason Esc cancelled instead of backgrounding. `App` owns them now
//! and the modal borrows a view, so the modal can be opened and closed
//! freely while the work continues.
//!
//! This registry — not the persisted `SetupStatus::Running` — is the source
//! of truth for the dashboard's in-flight badges. It lives in this process,
//! so an entry's presence proves a task is genuinely alive.

use crate::data::progress::SharedProgress;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

/// Which lifecycle operation is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InFlightKind {
    Create,
    /// Archive is not cancellable — the token is carried for uniformity and
    /// is never fired. See `workspace::archive_with_app`.
    Archive,
}

/// One in-flight lifecycle operation against a workspace.
#[derive(Debug, Clone)]
pub struct InFlight {
    pub kind: InFlightKind,
    pub progress: SharedProgress,
    pub cancel: CancellationToken,
    pub started: Instant,
}

impl InFlight {
    pub fn create(progress: SharedProgress, cancel: CancellationToken) -> Self {
        Self { kind: InFlightKind::Create, progress, cancel, started: Instant::now() }
    }

    pub fn archive(progress: SharedProgress, cancel: CancellationToken) -> Self {
        Self { kind: InFlightKind::Archive, progress, cancel, started: Instant::now() }
    }
}
```

Register it in `src/data/mod.rs` alongside the other `pub mod` lines:

```rust
pub mod in_flight;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --all-features in_flight`
Expected: PASS (2 tests).

- [ ] **Step 5: Add the field to `App`**

In `src/app.rs`, beside `pending_create_gen` (`:394-398`):

```rust
/// Background create/archive work, keyed by the workspace it targets.
/// Sole source of truth for the dashboard's in-flight badges. An entry is
/// inserted when the task is spawned and removed by the reconciler on
/// every exit path — success, failure, and cancellation alike.
pub in_flight: std::collections::HashMap<
    crate::data::store::WorkspaceId,
    crate::data::in_flight::InFlight,
>,
```

Initialise it to `std::collections::HashMap::new()` in the `App` constructor, near the `shared_detached` initialisation at `src/app.rs:560`.

- [ ] **Step 6: Change the modal to a viewer**

In `src/ui/modal/mod.rs`, replace the `SetupRunning` variant (`:72-76`):

```rust
SetupProgress {
    workspace_id: crate::data::store::WorkspaceId,
},
```

Its renderer (`:301`) can no longer read `progress` and `started` from the variant, so they must be passed in. Add a parameter to `pub fn render` (`src/ui/modal/mod.rs:208`), whose current signature is:

```rust
pub fn render(f: &mut Frame, area: Rect, modal: &Modal, tick: u32, theme: &Theme)
```

Add `in_flight: &std::collections::HashMap<crate::data::store::WorkspaceId, crate::data::in_flight::InFlight>` after `modal`, and update every call site the compiler names (the dashboard render path passes `&app.in_flight`).

Render the arm as a match, **not** a `let ... else { return ... }` — the arm sits inside a `match` whose value is a `(&str, String)` tuple, so a `return` there would return from `render` itself, which yields `()`:

```rust
Modal::SetupProgress { workspace_id } => match in_flight.get(workspace_id) {
    // The task finished while the viewer was open. Say so rather than
    // rendering a stale tail; the reconciler has already dropped the entry.
    None => (
        "workspace setup",
        "  setup finished.\n\n  [esc] close".to_string(),
    ),
    Some(f) => {
        let frame = crate::ui::dashboard::spinner::frame(tick);
        let (phase_label, tail) = match f.progress.lock() {
            Ok(p) => (p.phase().label(), p.recent(6)),
            Err(_) => ("Working", Vec::new()),
        };
        let secs = f.started.elapsed().as_secs();
        let elapsed = format!("{:02}:{:02}", secs / 60, secs % 60);
        let mut body = format!("  {frame} {phase_label}…   ({elapsed})\n\n");
        if tail.is_empty() {
            body.push_str("  (waiting for output…)\n");
        } else {
            for line in &tail {
                body.push_str(&format!("  {}\n", truncate_to(line, 54)));
            }
        }
        body.push_str("\n  [esc] close");
        ("workspace setup", body)
    }
},
```

`render_to_text` (`src/ui/modal/mod.rs:482`) takes only a `&Modal`; give it a second parameter and pass an empty map from the tests that do not care.

Note the footer changed from `[esc] cancel` to `[esc] close` — Esc no longer cancels. Update the existing modal tests at `src/ui/modal/mod.rs:508` and `:590` to construct `SetupProgress { workspace_id }` and pass an `in_flight` map; assert on `"[esc] close"`.

- [ ] **Step 7: Register the entry on spawn**

`create_with_app` does not know the workspace id until its Phase 3 insert, so the input handler cannot register the entry itself. Give the create task a callback that registers as soon as the id exists.

In `src/data/workspace.rs`, in `create_with_app`'s Phase 3 block (`:242-257`), after `add_primary_agent` and while the lock is already held:

```rust
g.in_flight.insert(
    ws_id,
    crate::data::in_flight::InFlight::create(progress.clone(), cancel.clone()),
);
```

`progress` and `cancel` are already parameters of `create_with_app`, so nothing new needs threading through.

In `src/app/input.rs`, delete the `app.modal = Some(Modal::SetupRunning {...})` assignment at `:1271-1275`, replacing it with `app.modal = None;`. Leave the `tokio::spawn` below it untouched.

The old modal cannot be kept during this task: it was keyed to nothing, but its replacement is keyed by workspace id, and no id exists until Phase 3. Rather than invent a placeholder key, create becomes backgrounded here — one task earlier than the spec's commit ordering implied. See "Notes for the executor" at the end of this plan.

- [ ] **Step 8: Remove the entry in the reconciler**

In `src/app.rs`, in `reconcile_create_result` (`:2452`), after taking the lock and before the match:

```rust
if let Some((id, _)) = new_ws {
    g.in_flight.remove(&id);
}
```

The failure and cancellation paths have no `new_ws`, so also clear any entry whose task is finished. The simplest correct rule, given one create at a time is enforced by `pending_create_gen`: on any non-`Ok` result, remove every `InFlightKind::Create` entry:

```rust
g.in_flight
    .retain(|_, f| f.kind != crate::data::in_flight::InFlightKind::Create);
```

Apply that in the `Err(_)` arms only; the `Ok` arm removes by id above.

- [ ] **Step 9: Run the full suite**

Run: `cargo test --all-targets --all-features`
Expected: PASS.

- [ ] **Step 10: Commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets --all-features -- -D warnings
git add src/data/in_flight.rs src/data/mod.rs src/app.rs src/app/input.rs src/ui/modal/mod.rs src/data/workspace.rs
git commit -m "refactor(setup): move create progress and cancellation onto App

Modal::SetupRunning owned the SharedProgress and CancellationToken, so
dismissing it dropped the only handle to the running work — which is why
Esc cancelled rather than backgrounded. App now owns an in_flight registry
keyed by workspace id and the modal borrows a view, becoming
Modal::SetupProgress. Esc closes the viewer instead of cancelling."
```

---

### Task 3: Insert the workspace row before the fetch

Reverses a deliberate ordering decision so the row can appear on the dashboard the instant you press Enter, rather than one network round-trip later.

**Files:**
- Modify: `src/data/workspace.rs:66-101` (`create`), `:222-279` (`create_with_app`)
- Test: inline `mod tests` in `src/data/workspace.rs`

**Interfaces:**
- Consumes: `WorkspaceState::Failed` (existing).
- Produces: no signature changes. Behavioural contract: a fetch failure now leaves a persisted row in `WorkspaceState::Failed` instead of leaving no row at all.

- [ ] **Step 1: Write the failing test**

Add to `#[cfg(test)] mod tests` in `src/data/workspace.rs`, using the existing `init_git_repo` helper:

```rust
#[tokio::test]
async fn fetch_failure_leaves_a_failed_row_not_no_row() {
    let store = Store::open_in_memory().unwrap();
    let repo_dir = init_git_repo();
    // A remote that resolves by name but cannot be fetched from, so
    // `fetch_for_base` gets past its remote-name check and then fails.
    assert!(
        std::process::Command::new("git")
            .current_dir(repo_dir.path())
            .args(["remote", "add", "origin", "/nonexistent/bare.git"])
            .status()
            .unwrap()
            .success()
    );
    let id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
        .await
        .unwrap();
    store.set_repo_base_branch(id, Some("origin/main")).unwrap();
    let repo = store.repos().unwrap().into_iter().find(|r| r.id == id).unwrap();
    let base = TempDir::new().unwrap();

    let err = create(
        &store,
        &repo,
        Some("alpha"),
        base.path(),
        false,
        false,
        crate::pty::session::AgentKind::Claude,
        tokio_util::sync::CancellationToken::new(),
        |_| {},
    )
    .await;
    assert!(err.is_err(), "fetch against a bogus remote must fail");

    let rows = store.workspaces(repo.id).unwrap();
    assert_eq!(rows.len(), 1, "the row must survive so the failure is visible");
    assert_eq!(rows[0].name, "alpha");
    assert_eq!(rows[0].state, WorkspaceState::Failed);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test --all-features fetch_failure_leaves_a_failed_row -- --nocapture`
Expected: FAIL — `assertion 'left == right' failed: left: 0, right: 1`. The current order fetches first, so no row is ever inserted.

- [ ] **Step 3: Reorder `create`**

In `src/data/workspace.rs`, move the `insert_workspace` + `add_primary_agent` block (`:79-90`) to sit *before* `git::fetch_for_base` (`:74`), and wrap the fetch so a failure marks the row. Replace the comment at `:72-73`, which now argues for the opposite of what the code does:

```rust
    // Insert the row BEFORE any slow I/O so the dashboard can show it the
    // instant creation starts — that immediacy is the whole point of
    // backgrounding. This reverses the original order, which fetched first
    // to avoid leaving an orphan row behind a failed fetch. With a Failed
    // state that the dashboard now badges, that "orphan" is a visible,
    // actionable row rather than a silent one.
    let id = store.insert_workspace(&NewWorkspace {
        repo_id: repo.id,
        name: &name,
        branch: &branch,
        worktree_path: &worktree_path,
        yolo,
        agent,
        shared,
    })?;
    // Seed the primary agent instance so the roster is authoritative from birth.
    store.add_primary_agent(id, agent, crate::data::store::now_ms())?;

    if let Err(e) = git::fetch_for_base(&repo.path, base).await {
        store.set_workspace_state(id, WorkspaceState::Failed)?;
        return Err(e);
    }
    if cancel.is_cancelled() {
        store.set_workspace_state(id, WorkspaceState::Failed)?;
        return Err(Error::Cancelled);
    }
```

The pre-existing `if cancel.is_cancelled()` guard at `:92-95` is now redundant with the one above — delete the duplicate.

- [ ] **Step 4: Reorder `create_with_app` the same way**

Move Phase 3 (`:242-257`, the insert, `add_primary_agent`, and the `in_flight` registration added in Task 2) above Phase 2 (`:227-235`, the fetch). Renumber the phase comments. Wrap the fetch:

```rust
    // --- Phase 3 (unlocked, async): fetch base branch. ---
    if let Ok(mut p) = progress.lock() {
        p.set_phase(SetupPhase::Fetching);
    }
    if let Err(e) = crate::git::fetch_for_base(&repo.path, base).await {
        let g = app.lock().await;
        g.store.set_workspace_state(id, WorkspaceState::Failed)?;
        return Err(e);
    }
```

`base` is computed at `:230-234` from `repo`, which is owned by the function — move that binding above the insert so it is in scope.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --all-features -- fetch_failure_leaves_a_failed_row`
Expected: PASS.

- [ ] **Step 6: Run the full suite**

Run: `cargo test --all-targets --all-features`
Expected: PASS. Watch `src/data/workspace.rs:614` and `:1221` in particular — they assert on rows after create and are the most likely to notice an ordering change.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets --all-features -- -D warnings
git add src/data/workspace.rs
git commit -m "feat(create): insert the workspace row before fetching

Creation fetched the base branch before inserting the row, so that a
failed fetch left no orphan behind. Backgrounding inverts that trade: the
row has to exist for the dashboard to show it immediately. A fetch failure
now marks the row Failed, which the dashboard badges — more discoverable
than the transient error modal it replaces, not less."
```

---

### Task 4: Serialize git operations per repo

Backgrounding makes concurrent creates in one repo possible for the first time. `git worktree add` and `git worktree remove` both mutate `.git/worktrees/` and repo-level refs.

**Files:**
- Create: `src/data/repo_lock.rs`
- Modify: `src/data/mod.rs`
- Modify: `src/data/workspace.rs` (`create`, `create_with_app`, `archive`, `archive_with_app`)
- Test: inline `mod tests` in `src/data/repo_lock.rs`, plus a concurrency test in `src/data/workspace.rs`

**Interfaces:**
- Consumes: `RepoId` (`src/data/store.rs:8`, `pub struct RepoId(pub i64)`).
- Produces: `crate::data::repo_lock::for_repo(id: RepoId) -> Arc<tokio::sync::Mutex<()>>` — same `Arc` for the same id, distinct `Arc`s for distinct ids.

- [ ] **Step 1: Write the failing tests**

Create `src/data/repo_lock.rs` with the test block only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::store::RepoId;

    #[test]
    fn same_repo_shares_one_lock() {
        let a = for_repo(RepoId(7));
        let b = for_repo(RepoId(7));
        assert!(std::sync::Arc::ptr_eq(&a, &b), "same repo must share a lock");
    }

    #[test]
    fn different_repos_get_different_locks() {
        let a = for_repo(RepoId(101));
        let b = for_repo(RepoId(102));
        assert!(
            !std::sync::Arc::ptr_eq(&a, &b),
            "unrelated repos must not serialize against each other"
        );
    }

    #[tokio::test]
    async fn lock_is_mutually_exclusive() {
        let l = for_repo(RepoId(303));
        let held = l.lock().await;
        assert!(l.try_lock().is_err(), "second acquisition must block");
        drop(held);
        assert!(l.try_lock().is_ok());
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --all-features repo_lock`
Expected: FAIL — `cannot find function 'for_repo'`.

- [ ] **Step 3: Write the module**

Prepend to `src/data/repo_lock.rs`:

```rust
//! Per-repository serialization of git operations.
//!
//! `git worktree add` and `git worktree remove` both mutate `.git/worktrees/`
//! and repo-level refs, so two of them racing in the same repo can corrupt
//! that admin state. Before backgrounding this could not happen — the create
//! and archive modals made concurrent operations impossible. Now three
//! creates can start within a few seconds, so the git phases take a lock.
//!
//! Scope is deliberately narrow: only the git calls are guarded. Setup
//! scripts stay fully parallel — they are the slow part (~18s on ssk-web)
//! and each touches only its own worktree.
//!
//! Process-local by design. A `wsx workspace create` CLI invocation is a
//! separate process and is not covered; git's own locking is the backstop
//! there, and the CLI creates one workspace at a time regardless.

use crate::data::store::RepoId;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

static LOCKS: LazyLock<Mutex<HashMap<i64, Arc<tokio::sync::Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The lock for `id`, creating it on first use. Entries are never evicted:
/// one empty mutex per repo ever registered is negligible, and eviction
/// would risk handing out a fresh lock while another task holds the old one.
pub fn for_repo(id: RepoId) -> Arc<tokio::sync::Mutex<()>> {
    let mut locks = LOCKS.lock().unwrap();
    locks
        .entry(id.0)
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}
```

Register in `src/data/mod.rs`:

```rust
pub mod repo_lock;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --all-features repo_lock`
Expected: PASS (3 tests).

- [ ] **Step 5: Guard the git phases**

In `src/data/workspace.rs`, wrap `git::create_worktree` in `create`:

```rust
    let worktree_result = {
        let lock = crate::data::repo_lock::for_repo(repo.id);
        let _guard = lock.lock().await;
        git::create_worktree(&repo.path, &branch, base, &worktree_path).await
    };
    if let Err(e) = worktree_result {
        store.set_workspace_state(id, WorkspaceState::Failed)?;
        return Err(e);
    }
```

Apply the same wrapping to:
- `create_with_app`'s worktree phase (the `crate::git::create_worktree` call)
- `archive`'s `git::remove_worktree` call
- `archive_with_app`'s `git::remove_worktree` call

Scope the guard to the git call alone in each case. Never hold it across `run_setup`, and never hold the App lock at the same time.

- [ ] **Step 6: Add the concurrency regression test**

Add to `#[cfg(test)] mod tests` in `src/data/workspace.rs`:

```rust
// A guard, not a reproduction: unserialized concurrent `git worktree add`
// calls usually succeed, so this does not reliably fail without the lock.
// It exists so a future change that breaks concurrent creation outright is
// caught, and to document that N-at-once is now a supported flow.
#[tokio::test]
async fn concurrent_creates_in_one_repo_all_succeed() {
    let store = Store::open_in_memory().unwrap();
    let repo_dir = init_git_repo();
    let id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
        .await
        .unwrap();
    let repo = store.repos().unwrap().into_iter().find(|r| r.id == id).unwrap();
    let base = TempDir::new().unwrap();

    // `Store` is not Sync, so drive the concurrency through the git layer
    // directly — that is what the lock guards.
    let mut handles = Vec::new();
    for name in ["alpha", "beta", "gamma"] {
        let repo_path = repo.path.clone();
        let branch = format!("wsx/{name}");
        let path = base.path().join("demo").join(name);
        let repo_id = repo.id;
        handles.push(tokio::spawn(async move {
            let lock = crate::data::repo_lock::for_repo(repo_id);
            let _guard = lock.lock().await;
            crate::git::create_worktree(&repo_path, &branch, None, &path).await
        }));
    }
    for h in handles {
        h.await.unwrap().expect("every concurrent worktree add must succeed");
    }
    for name in ["alpha", "beta", "gamma"] {
        assert!(base.path().join("demo").join(name).join(".git").exists());
    }
}
```

- [ ] **Step 7: Run the full suite**

Run: `cargo test --all-targets --all-features`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets --all-features -- -D warnings
git add src/data/repo_lock.rs src/data/mod.rs src/data/workspace.rs
git commit -m "feat(git): serialize worktree operations per repository

The create and archive modals used to make concurrent lifecycle operations
impossible. Backgrounding removes that accidental protection, so three
creates can now start within seconds, and worktree add/remove both mutate
.git/worktrees and repo-level refs. A per-repo mutex guards the git calls
only; setup scripts, the slow part, stay fully parallel."
```

---

### Task 5: Dashboard badges and the on-demand progress viewer

The user-visible half. Rows gain a lifecycle badge, and the progress viewer becomes something you open deliberately rather than something that traps you.

**Files:**
- Modify: `src/ui/dashboard/row.rs:88-118` (`RowInputs`), `:259` (width), `:291-293` (spans)
- Modify: `src/app/render.rs:837` (derivation)
- Modify: `src/app/input.rs:1404` (`WorkspaceActions` keys), `:1386` (drop the old `SetupRunning` arm)
- Modify: `src/ui/modal/mod.rs:351` (actions card text)
- Test: inline `mod tests` in `src/ui/dashboard/row.rs`

**Interfaces:**
- Consumes: `App.in_flight` (Task 2), `InFlightKind` (Task 2), `SetupStatus::Running` (Task 1).
- Produces:
  - `crate::ui::dashboard::row::LifecycleBadge` — `Debug, Clone, Copy, PartialEq, Eq`, variants `Provisioning`, `Archiving`, `SetupFailed`, `SetupCancelled`, `NoWorktree`.
  - `LifecycleBadge::glyph(self, tick: usize) -> String`
  - `LifecycleBadge::width(self) -> usize` — display columns including the leading space.
  - `crate::app::render::lifecycle_badge_for(state, setup_status, in_flight) -> Option<LifecycleBadge>` — free function, `pub(crate)`, so it is testable without an `App`.
  - `RowInputs.badge: Option<LifecycleBadge>` — **replaces** `RowInputs.setup_failed: bool`.

- [ ] **Step 1: Write the failing tests**

Add to `#[cfg(test)] mod tests` in `src/ui/dashboard/row.rs`:

```rust
#[test]
fn lifecycle_badge_derivation_table() {
    use crate::app::render::lifecycle_badge_for;
    use crate::data::in_flight::InFlightKind;
    use crate::data::store::{SetupStatus, WorkspaceState};

    let cases = [
        // (state, setup_status, in_flight, expected)
        (WorkspaceState::Ready, SetupStatus::Running, Some(InFlightKind::Create), Some(LifecycleBadge::Provisioning)),
        (WorkspaceState::Ready, SetupStatus::Ok, Some(InFlightKind::Archive), Some(LifecycleBadge::Archiving)),
        (WorkspaceState::Ready, SetupStatus::Failed, None, Some(LifecycleBadge::SetupFailed)),
        (WorkspaceState::Ready, SetupStatus::Cancelled, None, Some(LifecycleBadge::SetupCancelled)),
        (WorkspaceState::Failed, SetupStatus::NotRun, None, Some(LifecycleBadge::NoWorktree)),
        (WorkspaceState::Ready, SetupStatus::Ok, None, None),
        (WorkspaceState::Ready, SetupStatus::Skipped, None, None),
        // The persisted Running status alone never badges: without a live
        // registry entry it is the residue of a crash, already swept.
        (WorkspaceState::Ready, SetupStatus::Running, None, None),
        // In-flight always wins over the persisted status.
        (WorkspaceState::Failed, SetupStatus::Failed, Some(InFlightKind::Archive), Some(LifecycleBadge::Archiving)),
    ];
    for (state, setup, kind, expected) in cases {
        assert_eq!(
            lifecycle_badge_for(&state, &setup, kind),
            expected,
            "state={state:?} setup={setup:?} in_flight={kind:?}"
        );
    }
}

#[test]
fn in_flight_badges_animate_and_terminal_ones_do_not() {
    let a = LifecycleBadge::Provisioning.glyph(0);
    let b = LifecycleBadge::Provisioning.glyph(1);
    assert_ne!(a, b, "provisioning must animate");
    assert_eq!(
        LifecycleBadge::SetupFailed.glyph(0),
        LifecycleBadge::SetupFailed.glyph(1),
        "terminal badges must be static"
    );
    assert_eq!(LifecycleBadge::SetupFailed.glyph(0), " ⚙!");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --all-features -- lifecycle_badge in_flight_badges`
Expected: FAIL — `cannot find type 'LifecycleBadge'`.

- [ ] **Step 3: Add the badge type**

In `src/ui/dashboard/row.rs`, above `RowInputs`:

```rust
/// A workspace lifecycle badge, rendered immediately after the branch name.
/// Distinct from the agent-status glyph in column 3: that one tracks whether
/// the *agent* is live, this one whether the *workspace* is ready. Both can
/// animate at once — you can attach to a workspace while its setup runs —
/// and conflating them would lose that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleBadge {
    /// Create in flight: fetching, checking out, or running setup.
    Provisioning,
    /// Archive in flight: script, worktree removal, branch delete.
    Archiving,
    SetupFailed,
    SetupCancelled,
    /// The worktree was never created (a failed fetch or checkout).
    NoWorktree,
}

impl LifecycleBadge {
    /// Rendered text, including the leading space that separates it from the
    /// branch name. `tick` drives the spinner on the in-flight variants, and
    /// is `u32` to match `spinner::frame` (`src/ui/dashboard/spinner.rs:11`).
    pub fn glyph(self, tick: u32) -> String {
        match self {
            LifecycleBadge::Provisioning => format!(" {}⚙", spinner::frame(tick)),
            LifecycleBadge::Archiving => format!(" {}⌫", spinner::frame(tick)),
            LifecycleBadge::SetupFailed => " ⚙!".to_string(),
            LifecycleBadge::SetupCancelled => " ⚙?".to_string(),
            LifecycleBadge::NoWorktree => " ✗".to_string(),
        }
    }

    /// Display columns this badge consumes. Every variant is a space plus
    /// two cells, except `NoWorktree`, which is a space plus one.
    pub fn width(self) -> usize {
        match self {
            LifecycleBadge::NoWorktree => 2,
            _ => 3,
        }
    }
}
```

- [ ] **Step 4: Add the derivation function**

In `src/app/render.rs`, as a free `pub(crate) fn` near `build_row_inputs`:

```rust
/// Derive a row's lifecycle badge. A live `in_flight` entry always wins:
/// it proves work is running right now, whereas the persisted statuses
/// describe how the last attempt ended.
pub(crate) fn lifecycle_badge_for(
    state: &crate::data::store::WorkspaceState,
    setup_status: &crate::data::store::SetupStatus,
    in_flight: Option<crate::data::in_flight::InFlightKind>,
) -> Option<crate::ui::dashboard::row::LifecycleBadge> {
    use crate::data::in_flight::InFlightKind;
    use crate::data::store::{SetupStatus, WorkspaceState};
    use crate::ui::dashboard::row::LifecycleBadge;

    match in_flight {
        Some(InFlightKind::Create) => return Some(LifecycleBadge::Provisioning),
        Some(InFlightKind::Archive) => return Some(LifecycleBadge::Archiving),
        None => {}
    }
    match (state, setup_status) {
        (WorkspaceState::Failed, _) => Some(LifecycleBadge::NoWorktree),
        (_, SetupStatus::Failed) => Some(LifecycleBadge::SetupFailed),
        (_, SetupStatus::Cancelled) => Some(LifecycleBadge::SetupCancelled),
        // A persisted `Running` with no registry entry is crash residue that
        // `sweep_stale_running` already resolved before the first draw.
        _ => None,
    }
}
```

Replace the `setup_failed` line at `src/app/render.rs:837`:

```rust
let badge = lifecycle_badge_for(&ws.state, &ws.setup_status, app.in_flight.get(&ws.id).map(|f| f.kind));
```

and the `setup_failed,` field in the `RowInputs` literal with `badge,`.

- [ ] **Step 5: Render the badge**

In `src/ui/dashboard/row.rs`, replace `RowInputs.setup_failed: bool` with:

```rust
/// Workspace lifecycle badge, rendered after the branch name. `None` for a
/// healthy, idle workspace.
pub badge: Option<LifecycleBadge>,
```

Replace the width reservation at `:259`:

```rust
let setup_badge_width = inputs.badge.map(|b| b.width()).unwrap_or(0);
```

and the span emission at `:291-293`:

```rust
if let Some(b) = inputs.badge {
    let style = match b {
        LifecycleBadge::Provisioning | LifecycleBadge::Archiving => theme.dim_style(),
        _ => theme.err_style(),
    };
    spans.push(Span::styled(b.glyph(tick), style));
}
```

Fix every `RowInputs` construction the compiler flags — the render tests at `src/app/render.rs:1222` onward build these literals.

**Do not touch `Status::priority`** (`src/ui/dashboard/status.rs:21-31`). It drives within-repo sort order, and a provisioning row is not asking for attention — sorting it high would shuffle rows under the cursor mid-create. `reconcile_create_result` already selects the new workspace and unfolds its repo (`src/app.rs:2473-2489`), which is the intended emphasis.

- [ ] **Step 6: Wire the viewer and cancel into the actions card**

In `src/app/input.rs`, delete the `Modal::SetupRunning { cancel, .. }` arm at `:1386` — that modal no longer exists.

In the `Modal::WorkspaceActions` arm (`:1404`), add two keys before the catch-all `_ => {}`:

```rust
// Open the progress viewer for a workspace with work in flight.
KeyCode::Char('o') => {
    if let Some(SelectionTarget::Workspace(ws_id)) = app.selected_target()
        && app.in_flight.contains_key(&ws_id)
    {
        app.modal = Some(Modal::SetupProgress { workspace_id: ws_id });
    }
}
// Cancel an in-flight CREATE. Archive is not cancellable.
KeyCode::Char('x') => {
    if let Some(SelectionTarget::Workspace(ws_id)) = app.selected_target()
        && let Some(f) = app.in_flight.get(&ws_id)
        && f.kind == crate::data::in_flight::InFlightKind::Create
    {
        f.cancel.cancel();
        app.modal = None;
    }
}
```

Add a `Modal::SetupProgress` key arm so Esc closes without cancelling:

```rust
Modal::SetupProgress { .. } => {
    if matches!(k.code, KeyCode::Esc | KeyCode::Enter) {
        app.modal = None;
    }
}
```

Update the actions card text at `src/ui/modal/mod.rs:351`:

```rust
Modal::WorkspaceActions => (
    "workspace actions",
    "These apply to the selected workspace:\n\n  \
     e   edit        t   term\n  \
     v   diff        g   lazygit\n  \
     c   chronox     r   rename\n  \
     o   setup log   x   cancel setup\n\n  \
     ?/Esc  close"
        .to_string(),
),
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test --all-targets --all-features`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets --all-features -- -D warnings
git add src/ui/dashboard/row.rs src/app/render.rs src/app/input.rs src/ui/modal/mod.rs
git commit -m "feat(dashboard): badge workspace lifecycle state on the row

Replaces the setup_failed boolean with a LifecycleBadge covering
provisioning, archiving, failed and cancelled setup, and a missing
worktree. The in-flight registry always wins over the persisted status: it
proves work is running now, while the statuses describe how the last
attempt ended.

The badge animates in its own column rather than reusing the agent-status
spinner — a workspace can be attached while its setup runs, and the two
spinners mean different things. Opening the setup log and cancelling a
create move onto the workspace actions card (o and x)."
```

---

### Task 6: Background the archive, guard attach, and confirm quit

**Files:**
- Modify: `src/data/workspace.rs:391-396` (`advance_archive_step`), `:403-454` (`archive_with_app`)
- Modify: `src/app/input.rs:1319-1361` (archive spawn), `:571` (quit key), `:1395` (drop the `ArchiveRunning` arm)
- Modify: `src/app.rs:2393` (`attach_workspace`), `:2513` (`reconcile_archive_result`)
- Modify: `src/ui/modal/mod.rs` (drop `ArchiveRunning`, add `ConfirmQuit`)
- Test: inline `mod tests` in `src/app.rs`

**Interfaces:**
- Consumes: `InFlight::archive` (Task 2), `LifecycleBadge::Archiving` (Task 5), `repo_lock::for_repo` (Task 4).
- Produces: `Modal::ConfirmQuit { creates: usize, archives: usize }`.

- [ ] **Step 1: Write the failing test**

Add to `#[cfg(test)] mod tests` in `src/app.rs`:

```rust
#[test]
fn attach_refuses_a_workspace_being_archived() {
    let mut app = test_app();
    let ws = app.test_workspace("doomed");
    app.in_flight.insert(
        ws,
        crate::data::in_flight::InFlight::archive(
            crate::data::progress::SetupProgress::shared(),
            tokio_util::sync::CancellationToken::new(),
        ),
    );
    // Archive kills the workspace's tmux sessions first, precisely so a live
    // agent cannot dirty the worktree during teardown. Attaching would
    // respawn one into a directory that is being deleted.
    attach_workspace(&mut app, ws).unwrap();
    assert!(
        !matches!(app.view, View::Attached(_)),
        "attach must be refused while an archive is in flight"
    );
}

#[test]
fn attach_allows_a_workspace_that_is_only_provisioning() {
    let mut app = test_app();
    let ws = app.test_workspace("fresh");
    app.in_flight.insert(
        ws,
        crate::data::in_flight::InFlight::create(
            crate::data::progress::SetupProgress::shared(),
            tokio_util::sync::CancellationToken::new(),
        ),
    );
    // Dropping in while setup runs is the entire point of this feature.
    // Only archive is unsafe. (`assert!(!x)`, not `assert!(x == false)` —
    // clippy's `bool_assert_comparison` is denied by CI.)
    assert!(
        !attach_is_blocked(&app, ws),
        "provisioning must not block attach"
    );
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --all-features -- attach_refuses attach_allows`
Expected: FAIL — `cannot find function 'attach_is_blocked'`.

- [ ] **Step 3: Add the attach guard**

In `src/app.rs`, above `attach_workspace` (`:2393`):

```rust
/// Whether attaching to `ws_id` must be refused. Only a live archive
/// blocks: its first act is killing the workspace's tmux sessions so a
/// live agent cannot dirty the worktree during teardown, and attaching
/// would respawn one into a directory that is being deleted. A create in
/// flight never blocks — working in a workspace while its setup runs is
/// the point of backgrounding.
pub(crate) fn attach_is_blocked(app: &App, ws_id: crate::data::store::WorkspaceId) -> bool {
    app.in_flight
        .get(&ws_id)
        .is_some_and(|f| f.kind == crate::data::in_flight::InFlightKind::Archive)
}
```

and as the first statement of `attach_workspace`:

```rust
if attach_is_blocked(app, ws_id) {
    return Ok(());
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --all-features -- attach_refuses attach_allows`
Expected: PASS.

- [ ] **Step 5: Register the archive in the registry**

In `src/app/input.rs`, in the `Modal::ConfirmArchive` `y` handler (`:1319`), replace the `app.modal = Some(Modal::ArchiveRunning {...})` assignment at `:1343-1346` with a registry insert and a dismissal:

```rust
let progress = crate::data::progress::SetupProgress::shared();
app.in_flight.insert(
    ws.id,
    crate::data::in_flight::InFlight::archive(
        progress.clone(),
        tokio_util::sync::CancellationToken::new(),
    ),
);
app.modal = None;
```

`script_present` is no longer needed — delete the binding at `:1338-1342`.

In `src/data/workspace.rs`, replace `advance_archive_step` (`:391-396`) so it writes a phase line into the registry's progress sink instead of mutating a modal:

```rust
/// Record archive progress for the row badge and the progress viewer.
/// Replaces the old modal-step mutation: with the archive backgrounded,
/// there is no modal to advance.
async fn note_archive_step(app: &crate::app::SharedApp, ws_id: WorkspaceId, label: &str) {
    let g = app.lock().await;
    if let Some(f) = g.in_flight.get(&ws_id)
        && let Ok(mut p) = f.progress.lock()
    {
        p.push_line(label);
    }
}
```

Update its three call sites in `archive_with_app` (`:427`, `:434`, `:440`) to
`note_archive_step(&app, ws.id, "removing worktree").await` and so on.

- [ ] **Step 6: Clear the registry entry in the archive reconciler**

In `reconcile_archive_result` (`src/app.rs:2513`), after taking the lock, remove every archive entry — one archive is in flight at a time, enforced by `pending_archive_gen`:

```rust
g.in_flight
    .retain(|_, f| f.kind != crate::data::in_flight::InFlightKind::Archive);
```

Delete the `Modal::ArchiveRunning` variant from `src/ui/modal/mod.rs`, its renderer arm, the `ArchiveStep` enum, `src/ui/modal/archive.rs`, and the input arm at `src/app/input.rs:1395`. Follow the compiler.

- [ ] **Step 7: Add the quit confirmation**

Add to `src/ui/modal/mod.rs`:

```rust
ConfirmQuit {
    creates: usize,
    archives: usize,
},
```

with a renderer arm:

```rust
Modal::ConfirmQuit { creates, archives } => {
    let mut what = Vec::new();
    if *creates > 0 {
        what.push(format!("{creates} setup(s)"));
    }
    if *archives > 0 {
        what.push(format!("{archives} archive(s)"));
    }
    (
        "work in progress",
        format!(
            "{} still running.\n\n\
             Quitting stops them: setups are cancelled, and an archive is\n\
             left part-done (archiving again finishes it).\n\n\
             [y] quit anyway   [n]/[esc] stay",
            what.join(" and ")
        ),
    )
}
```

In `src/app/input.rs`, replace the quit binding at `:571`:

```rust
(KeyCode::Char('q'), _) => {
    if app.in_flight.is_empty() {
        app.quit = true;
    } else {
        let creates = app
            .in_flight
            .values()
            .filter(|f| f.kind == crate::data::in_flight::InFlightKind::Create)
            .count();
        app.modal = Some(Modal::ConfirmQuit {
            creates,
            archives: app.in_flight.len() - creates,
        });
    }
}
```

and add its key arm:

```rust
Modal::ConfirmQuit { .. } => match k.code {
    KeyCode::Char('y') => {
        // Cancel creates on the way out so their rows land on Cancelled
        // rather than waiting for the next startup sweep to resolve them.
        // Archive has no cancellation and is simply abandoned; it is
        // self-healing, since remove_worktree falls back to remove_dir_all
        // once git no longer recognises the path.
        for f in app.in_flight.values() {
            if f.kind == crate::data::in_flight::InFlightKind::Create {
                f.cancel.cancel();
            }
        }
        app.quit = true;
    }
    KeyCode::Char('n') | KeyCode::Esc => app.modal = None,
    _ => {}
},
```

- [ ] **Step 8: Run the full suite**

Run: `cargo test --all-targets --all-features && cargo test --doc --all-features`
Expected: PASS.

- [ ] **Step 9: Manual verification**

This is the first point where the feature is observable, so verify it by hand before committing:

```bash
cargo build --release
```

Then, against the `ssk-web` repo: create a workspace and confirm the dashboard returns immediately with a spinning `⚙` badge on the new row; press `?` then `o` to watch the setup log and Esc to close it without cancelling; confirm the badge clears after ~20s; archive a workspace and confirm the row shows `⌫` and then disappears; press `q` during both and confirm the prompt appears.

- [ ] **Step 10: Commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets --all-features -- -D warnings
git add -A
git commit -m "feat(archive): run archive in the background behind a row badge

Archive drops its blocking modal and joins the in-flight registry: the row
stays visible with an archiving badge and disappears when the store row is
deleted, leaving somewhere for a removal failure to surface.

Attach is refused for a workspace being archived. Archive kills the
workspace's tmux sessions first so a live agent cannot dirty the worktree
during teardown, and attaching would respawn one into a directory that is
being deleted. A create in flight never blocks attach — working while
setup runs is the point.

Quitting with work in flight now confirms, cancelling creates on the way
out. It cannot catch a kill -9, which is what sweep_stale_running covers."
```

---

## Notes for the executor

**Task 2 Step 7 deviates from the spec's commit ordering.** The spec described commit 2 as a pure refactor with the modal still opening, and the user-visible flip landing in commit 5. That is not achievable: `create_with_app` does not know the workspace id until its Phase 3 insert, so there is no key to register the modal against at spawn time. Rather than invent a placeholder key, Task 2 stops opening the create modal, which means create is silently backgrounded one task earlier than planned. Tasks 3 and 4 are therefore usable-but-unpolished (no badge yet), and Task 5 restores full visibility. If you would rather not ship an intermediate state where a create gives no feedback at all, do Tasks 2 and 5 back to back before pausing for review.

**Explicitly out of scope** — the spec rules these out; do not add them opportunistically:

- Any change to CLI `wsx workspace create`. It stays blocking, because its caller runs `wsx agent send` next and needs a real worktree, and there is no dashboard to badge.
- A re-run-setup command. If setup fails you enter the worktree and run it by hand, exactly as today.
- Telling the agent that setup is running. An attached agent may run tests against half-installed dependencies; pacing that is the operator's call.

**`SetupStatus::Running` is not the badge source.** It is written to disk purely so a crashed process leaves evidence for `sweep_stale_running`. The badge reads `App.in_flight`, which is in-process and therefore proves liveness. Task 5's derivation table asserts this explicitly — `(Ready, Running, None) => None`. Do not "simplify" the table by badging off the persisted status.
