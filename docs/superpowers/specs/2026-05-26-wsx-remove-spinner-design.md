# Archive-workspace modal: loading indicator

## Problem

Archiving a workspace can take many seconds, especially when the worktree
contains heavy build artifacts (`node_modules`, `target`, etc.) that
`git worktree remove` must delete. The current handler at
`src/app/input.rs:921` calls `workspace::archive(...).await` *synchronously*
while the app's `tokio::Mutex` is still held by the input loop. The main
event loop cannot redraw or process input during this window.

The user-visible effect: the `ConfirmArchive` modal appears frozen after
pressing `y`. There is no feedback that anything is happening, and the user
may assume the TUI has hung.

The create flow already solved the analogous problem at
`src/app/input.rs:854` by spawning the long-running work on a `tokio` task
and switching the modal to `Modal::SetupRunning { cancel }` so the renderer
can animate a spinner. This spec mirrors that pattern for archive.

## Goals

- The modal visibly indicates work-in-progress immediately after pressing
  `y`, with the same braille spinner used during workspace creation.
- The main event loop continues to tick and redraw while archive runs.
- On completion, the modal closes (success) or shows `Modal::Error`
  (failure) within one tick.

## Non-goals

- **Cancellation.** Esc during the spinner is a deliberate no-op. The slow
  step is `git worktree remove`, and interrupting it mid-flight tends to
  leave a partial worktree that requires `--force` to clean up on the next
  attempt. Per discussion, we accept a non-interruptible spinner.
- **Streaming archive-script output** into the modal. Out of scope.
- **CLI removal path** (`wsx workspace archive`). This is a TUI-only change.

## Approach

Spawn `workspace::archive` in a `tokio::spawn` task so the main event
loop's mutex is held only briefly at the beginning (to transition the modal)
and at the end (to reconcile the result). Introduce a new
`Modal::ArchiveRunning` variant rendered with the existing spinner. Use a
generation counter (`pending_archive_gen`) — analogous to the create
flow's `pending_create_gen` — so a stale completion can't clobber an
unrelated modal.

### Why a separate `ArchiveRunning` variant (not reusing `SetupRunning`)

`SetupRunning` carries a `CancellationToken` because create is cancellable.
Archive is not cancellable in this design, so the variant has no data. The
spinner label also differs ("Removing workspace…" vs "Creating workspace…").
Two narrow variants read more clearly than one general-purpose variant
with a `kind` field.

### Why no `CancellationToken` on `workspace::archive`

The existing `archive` function at `src/workspace.rs:252` constructs a
fresh token internally and passes it to `setup::run_archive`. Since this
spec does not introduce cancellation, the signature does not need to
change. The internal-token-per-call pattern stays.

## Architecture

### Affected modules

| File | Change |
|---|---|
| `src/ui/modal.rs` | Add `Modal::ArchiveRunning` variant (no fields). Add a render branch that displays `"  {frame} Removing workspace…"` with title `"archive workspace"`. |
| `src/app.rs` | Add `App` fields `next_archive_gen: u64` and `pending_archive_gen: Option<u64>`. Add `App::alloc_archive_gen(&mut self) -> u64`. Add free function `reconcile_archive_result(app: SharedApp, my_gen: u64, result: Result<crate::workspace::SetupResult>)`. |
| `src/app/input.rs` | Rewrite the `'y'` branch of `Modal::ConfirmArchive` (currently lines ~902–944). Allocate `archive_gen`, clone `repo`/`ws`/`shared`, set `app.modal = Some(Modal::ArchiveRunning)`, `tokio::spawn` the archive + reconcile. Add a `Modal::ArchiveRunning` match arm that ignores all keys. |
| `src/app/input_tests.rs` | Add tests for the new transition, success reconcile, and failure reconcile (see Testing). |

### Runtime flow on `y` in `ConfirmArchive`

1. Resolve `(repo, ws)` from `app.workspaces` / `app.repos` (already done in
   the current handler; preserved).
2. `let archive_gen = app.alloc_archive_gen();`
3. `let shared_clone = shared.clone();`
4. `app.modal = Some(Modal::ArchiveRunning);`
5. `tokio::spawn(async move { let result = crate::workspace::archive(...).await; reconcile_archive_result(shared_clone, archive_gen, result).await; });`
6. Return from the key handler. The lock is released. The main loop's
   `select!` continues; the tick fires; the next draw renders
   `ArchiveRunning` with an animated spinner.

### Runtime flow on archive completion

The spawned task locks `shared_clone`, then:

- `Ok(_)`: if `pending_archive_gen == Some(my_gen)` and the modal is still
  `Modal::ArchiveRunning`, set `modal = None` and clear
  `pending_archive_gen`. Call `app.refresh()` so the dashboard reflects the
  deleted workspace.
- `Err(e)`: same staleness check; if matched, switch modal to
  `Modal::Error { message: e.to_string() }`. Always call `app.refresh()`.

### Runtime flow on Esc in `ArchiveRunning`

No-op. The match arm exists for exhaustiveness but consumes the key event
without action. The user must wait for the operation to complete.

### Staleness check

`ArchiveRunning` swallows all input, so the user cannot dismiss or
transition the modal mid-flight. The staleness check is therefore
defensive rather than strictly required, but it is cheap and mirrors the
create flow's reconcile pattern. It handles:

1. Some external event handler (outside the input loop) replacing the
   modal during the await — e.g., a future code path that surfaces an
   Error modal from a background task.
2. A future change that allows concurrent archives. Each carries its own
   generation id and reconciles independently.
3. App teardown during archive. The `tokio::spawn` task is dropped; no
   reconcile happens.

`pending_archive_gen` / `next_archive_gen` are independent counters from
the create pair so a create and an archive can be in flight simultaneously
without ID collision.

## Race conditions

1. **Workspace deleted from elsewhere before archive completes.** Archive
   writes to the store via `delete_workspace`; if the row is already gone,
   the store call returns `Ok` (idempotent delete). No special handling
   needed.
2. **App quits during archive.** Runtime drop cancels the spawned task
   mid-`await`. Worktree may be left partially removed on disk. Behavior
   matches the create case for git subprocess interruption — not pretty,
   but consistent with what users get today during an unclean shutdown.
3. **Modal::Error already showing when archive completes.** Reconcile's
   staleness check sees modal != `ArchiveRunning`, leaves the Error modal
   alone, still calls `refresh()`.
4. **Spinner stops animating.** The spawned task holds the lock only
   briefly at the end. `app.tick` continues to increment during the await,
   so the spinner animates throughout.

## Testing

Mirror the existing create-flow tests at `src/app/input_tests.rs:2291` and
nearby:

### Integration tests in `src/app/input_tests.rs`

1. **`y_in_confirm_archive_transitions_to_archive_running_and_spawns_task`** —
   Set up a workspace; open `ConfirmArchive`; press `y`. Assert modal
   becomes `Modal::ArchiveRunning` immediately; assert
   `pending_archive_gen.is_some()`. Await briefly. Assert modal becomes
   `None` and the workspace row is gone from `app.workspaces`.

2. **`esc_in_archive_running_is_noop`** — Open `ConfirmArchive`, press `y`,
   immediately press Esc. Assert modal is still `Modal::ArchiveRunning`
   and `pending_archive_gen` is still `Some(_)`. Await briefly; assert
   the archive still completes normally.

3. **`reconcile_archive_result_with_err_sets_error_modal`** — Unit test
   `reconcile_archive_result` directly. Build a `SharedApp` with
   `Modal::ArchiveRunning` and `pending_archive_gen = Some(7)`. Call
   `reconcile_archive_result(app, 7, Err(Error::Setup("boom".into())))`.
   Assert modal becomes `Modal::Error { message }` containing "boom" and
   `pending_archive_gen` is cleared. (Direct unit test rather than e2e
   because the realistic failure paths — `git::remove_worktree` failing,
   `store.delete_workspace` failing — are awkward to inject; the contract
   we care about is reconcile's branching.)

4. **`reconcile_archive_result_skips_modal_mutation_when_gen_mismatch`** —
   Build `App` with `Modal::Error { .. }` and `pending_archive_gen = Some(2)`.
   Call `reconcile_archive_result(app, 99, Err(...))`. Assert modal is
   unchanged. (Defensive race coverage matching create's pattern.)

### Manual smoke test

1. Create a workspace in a repo whose worktree contains a slow-to-delete
   directory (`node_modules`, `target`, etc.).
2. From the dashboard, press the archive shortcut to open `ConfirmArchive`.
3. Press `y`. Verify:
   - Modal switches to "Removing workspace…" with an animated spinner
     immediately.
   - Spinner animates while `git worktree remove` works.
   - Pressing Esc during the spinner does nothing.
   - On completion, modal closes and the workspace is gone from the
     dashboard.

## TDD ordering

Per `superpowers:test-driven-development`: write tests 1 and 3 first to
capture the happy-path transition and the error-routing contract.
Implement the new modal variant, generation counter, reconcile function,
and the spawn-based handler to make them pass. Add tests 2 and 4 once
the happy path is green.

## Open questions

None.
