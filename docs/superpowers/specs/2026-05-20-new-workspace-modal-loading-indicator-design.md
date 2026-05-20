# New-workspace modal: loading indicator and cancellation

## Problem

Creating a new workspace can take many seconds, especially when the repo's
setup script does heavy work (installing dependencies, building caches). The
current modal at `src/app.rs:2176-2210` calls `workspace::create().await`
*synchronously* — that is, the call awaits while the app's `tokio::Mutex` is
still locked. The main event loop cannot redraw or process input during this
window.

The user-visible effect:

1. The modal appears frozen after pressing Enter. There is no feedback that
   anything is happening.
2. Subsequent Enter keystrokes queue in crossterm's input buffer and are
   processed *after* the freeze ends. Depending on what is on screen at that
   point, the user can end up creating multiple workspaces unintentionally.

A previous author already laid groundwork for a fix: the modal has a
`SetupRunning` variant with a log buffer, and the variant *would* render
correctly if the event loop were able to draw it. A comment at
`src/app.rs:2177-2180` explicitly defers the asynchronous version, citing
borrow-checker friction.

## Goals

- The modal visibly indicates work-in-progress immediately on Enter, with an
  animated spinner.
- Holding Enter or repeatedly pressing Enter cannot create duplicate
  workspaces.
- Pressing Esc during creation cancels the in-flight setup script and closes
  the modal within one frame.

## Non-goals

- Streaming setup-script stdout into the modal. The single-status-line design
  is intentional; rich log streaming is a separate, larger feature.
- Rolling back the worktree on cancellation. The user wanted out of the
  modal, not necessarily out of having a workspace.
- Cancelling `git fetch` or `git worktree add` mid-flight. These are external
  subprocesses we cannot interrupt without a larger refactor; they are
  typically fast enough that this does not matter in practice.

## Approach

Spawn workspace creation in a `tokio::spawn` task so the main event loop's
mutex is held only briefly at the beginning (to transition the modal) and at
the end (to reconcile the result). Pass a `tokio_util::sync::CancellationToken`
into `workspace::create` and through to `setup::run_setup`; the modal's
`SetupRunning` variant holds a clone of the same token so the Esc handler can
cancel it.

### Why `CancellationToken`

`tokio-util`'s `CancellationToken` is the idiomatic primitive for this case:
cheap to clone, idempotent on cancel, supports both polling
(`is_cancelled()`) and awaiting (`cancelled().await`), and integrates
naturally with `tokio::select!`. An alternative using `Arc<tokio::sync::Notify>`
avoids the new dependency but is less ergonomic — `Notify` does not expose a
sync `is_cancelled()` check, and one-shot identity semantics require extra
bookkeeping. The dependency footprint of `tokio-util` is small and is the
standard Rust answer here.

### Why no worktree rollback

If the user presses Esc after the worktree was already created on disk, we
mark the workspace as `setup_status=Cancelled` and leave the files in place.
The dashboard already surfaces the row; the user can archive it, re-run
setup, or just use the branch. Rolling back would require `git worktree
remove --force`, which is destructive, and would complicate the create
function with phase-aware cleanup logic. YAGNI.

## Architecture

### Affected modules

| File | Change |
|---|---|
| `Cargo.toml` | Add `tokio-util = { version = "0.7", default-features = false }`. `CancellationToken` lives in the unconditional `sync` module, so no features are required. |
| `src/error.rs` | Add `Error::Cancelled` variant. |
| `src/store/...` | Add `SetupStatus::Cancelled` variant. Persisted as a new string discriminant. |
| `src/setup.rs` | `run_setup` gains `cancel: CancellationToken` parameter. Existing `tokio::select!` loop grows a third arm that returns `Err(Error::Cancelled)` on `cancel.cancelled()`. `kill_on_drop(true)` (already set at `src/setup.rs:72`) reaps the child. |
| `src/workspace.rs` | `create` gains `cancel: CancellationToken` parameter (the CLI path). A new `create_with_app` function provides the TUI path — same phase ordering but interleaves brief `app.lock().await` cycles between async git/setup work. Token checked between phases (before fetch, before `insert_workspace`, before `git::create_worktree`, before `run_setup`). On cancel mid-flight, the workspace's `setup_status` is set to `Cancelled` if a row exists; function returns `Err(Cancelled)`. |
| `src/ui/modal.rs` | `Modal::SetupRunning` variant gains `cancel: CancellationToken` field. Renderer shows the existing braille spinner driven by `app.tick` next to a status line, plus a `(Esc to cancel)` hint. |
| `src/app.rs` | `App` gains `next_create_gen: u64` and `pending_create_gen: Option<u64>`. `handle_key_modal` for `NewWorkspace::Enter` allocates a generation, transitions to `SetupRunning`, and spawns the create task. `handle_key_modal` for `SetupRunning::Esc` calls `cancel.cancel()`, sets `app.modal = None`, clears `pending_create_gen`. The spawned task, on completion, locks the app, checks the generation, reconciles. |

### Runtime flow on Enter

1. Build `cancel = CancellationToken::new()`.
2. Clone `store`, `repo`, `base`, `name`, `yolo`, and `cancel.clone()` into
   the spawn closure.
3. Set `app.modal = Some(Modal::SetupRunning { cancel: cancel.clone() })`.
4. `tokio::spawn(async move { let result = workspace::create_with_app(app, ..., cancel).await; reconcile(app, my_token, result).await })`.
5. Return from the key handler. The lock is released. The main loop's
   `select!` continues; the tick fires; the next draw renders `SetupRunning`
   with an animated spinner.

### Runtime flow on Esc during `SetupRunning`

1. Clone `cancel` out of the modal.
2. Set `app.modal = None`.
3. Call `cancel.cancel()`.
4. The spawned task observes cancellation at its next checkpoint or via the
   `select!` in `run_setup`. `create` returns `Err(Cancelled)`. The task
   writes `SetupStatus::Cancelled` to the store if applicable. On reconcile,
   it sees `app.modal != SetupRunning { cancel: my_token }` and does not
   mutate the modal. It does call `app.refresh()` so the dashboard reflects
   the new row state.

### Staleness check

`CancellationToken` does not expose value equality, so the reconcile step
distinguishes "my creation" from "someone else's creation" via a generation
id. `App` gains a monotonic counter `next_create_gen: u64` and a field
`pending_create_gen: Option<u64>`. On Enter, the handler picks a fresh `gen`,
sets `pending_create_gen = Some(gen)`, and moves `gen` into the spawned task.
On completion, the task locks the app and applies the outcome only if
`app.pending_create_gen == Some(my_gen)`; otherwise it just updates the
store and exits. The Esc handler clears `pending_create_gen = None` along
with the modal.

This handles all three staleness sources: user cancelled and reopened a new
modal, user cancelled and went elsewhere, a stale task from a previous
session-state cycle racing a fresh one.

## Cancellation semantics

| Phase | State at cancel time | Action |
|---|---|---|
| Before `git::fetch_for_base` | Nothing persistent | Return `Err(Cancelled)` immediately. |
| During `git::fetch_for_base` | Network in flight | Let fetch finish; check token at next checkpoint. |
| Before `insert_workspace` | Possibly partial fetch | Return `Err(Cancelled)`. No store row. |
| Before `git::create_worktree` | Store row `state=Pending` | Mark `state=Failed`; return `Err(Cancelled)`. |
| Before `run_setup` | Worktree exists, `state=Ready` | Mark `setup_status=Cancelled`; return `Err(Cancelled)`. Worktree left on disk. |
| During `run_setup` | Child process running | `select!` cancel arm; child reaped via `kill_on_drop`. Mark `setup_status=Cancelled`. |

## Race conditions

1. **Esc just as `create` succeeds.** Task sees modal mismatch on reconcile,
   skips modal mutation, still calls `app.refresh()`. Workspace exists,
   modal stays closed.
2. **Esc just as `create` fails.** Same as above; no error popup shown for
   abandoned work. Failure is visible in the dashboard.
3. **Second `NewWorkspace` modal opened during a first create.** Each create
   has its own generation id; the late completion of the first task sees a
   mismatched `pending_create_gen` and skips modal mutation.
4. **App quitting while a create is in flight.** Runtime drop cancels the
   spawned task. `kill_on_drop` reaps the child. Store state is left
   wherever the last completed step landed it (matches current crash
   behavior).
5. **Spinner stops animating.** The spawned task holds the lock only at the
   end. Tick continues to fire and increment `app.tick` during creation.
6. **Cancel during git subprocess.** External git CLI cannot be interrupted;
   user perceives cancel as instant (modal closes), background work tidies
   up at next checkpoint.
7. **Double cancel.** `CancellationToken::cancel()` is idempotent. Modal is
   already `None`. No-op.

## Testing

### Unit — `src/setup.rs`

1. `run_setup_respects_cancellation`: script sleeps 10s; cancel after 100ms;
   assert return within ~200ms with `Err(Cancelled)`. Verifies `kill_on_drop`
   reaping.
2. `run_setup_completes_before_cancel_is_ignored`: script exits in 50ms;
   cancel 200ms later; assert result is the script's normal exit, not
   `Cancelled`.

### Unit — `src/workspace.rs`

3. `create_returns_cancelled_when_token_cancelled_before_start`:
   pre-cancelled token; `create()` returns `Err(Cancelled)` immediately; no
   store row.
4. `create_marks_setup_status_cancelled_when_cancelled_during_setup`: fake
   setup script that sleeps; cancel mid-flight; assert row exists with
   `state=Ready`, `setup_status=Cancelled`, worktree path exists.

### Integration — `src/app.rs` (following the pattern at line 4274)

5. `enter_in_new_workspace_modal_transitions_to_setup_running_and_spawns_task`:
   press Enter; assert modal is `SetupRunning` immediately; await briefly;
   assert modal becomes `None` and workspace appears.
6. `esc_in_setup_running_cancels_and_closes_modal`: setup script sleeps;
   press Enter; assert `SetupRunning`; press Esc; assert modal is `None`
   immediately; await briefly; assert `setup_status=Cancelled`.
7. `enter_during_setup_running_is_a_noop`: press Enter, then Enter again
   during `SetupRunning`; assert exactly one workspace created.
8. `successful_create_after_esc_does_not_show_error_modal`: press Enter,
   immediately press Esc, await; assert `app.modal == None`.

### Manual smoke test

Run the TUI against a repo with a setup script of `sleep 5 && echo done`.
Press `n`, name the workspace, press Enter. Verify:

- Modal switches to spinner view immediately.
- Spinner animates.
- Repeated Enter does nothing.
- Esc closes the modal within one frame.
- Dashboard shows `Pending → Ready → Cancelled` (or `Ok` if allowed to
  finish).

## TDD ordering

Per `superpowers:test-driven-development`: write tests 1, 3, 5, 6 first to
capture the contract. Implement cancellation plumbing to make them pass.
Add tests 2, 4, 7, 8 as the race-condition surface area is exercised.

## Open questions

None. All decisions are made in the sections above.
