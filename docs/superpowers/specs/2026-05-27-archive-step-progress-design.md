# Archive-workspace modal: step-by-step progress

## Problem

The archive modal currently shows a single spinner with the label
"Removing workspace…" while `workspace::archive_with_app` runs through
four sequential phases:

1. The repo's optional archive script.
2. `git worktree remove` — the slow one when `node_modules`, `target`, or
   other large build artifacts are present. Frequently takes several
   seconds.
3. `git branch -D` on the workspace branch.
4. Registry cleanup (sqlite row + `~/.claude.json` MCP entry).

A user watching the spinner has no idea which phase is running, how much
of the work has completed, or whether the removal is making forward
progress at all. For node-heavy worktrees the spinner can sit there long
enough that the user wonders whether the TUI has hung again — exactly
the failure mode the original spinner work was meant to eliminate.

## Goals

- Surface all four phases of `archive_with_app` in the modal as a vertical
  checklist with done / in-progress / pending markers.
- When the repo has no archive script configured, render the script row
  as "(skipped)" rather than omitting it, so the modal's shape is
  consistent across repos.
- No regressions to event-loop responsiveness, cancellation behavior, or
  reconciliation semantics.

## Non-goals

- **Streaming archive-script output** into the modal. Out of scope.
- **Cancellation.** Archive remains non-interruptible (matches existing
  behavior at `src/app/input.rs:956`).
- **CLI removal path.** `workspace::archive` (the non-TUI variant) keeps
  its current callback-driven surface; no new progress reporting there.
- **Sub-phase progress.** We do not crack open `git worktree remove` to
  count files deleted, and we do not stream archive-script stdout into
  the modal. Phase-level granularity is the unit.

## Approach

Promote `Modal::ArchiveRunning` from a unit variant to a struct variant
carrying the current `ArchiveStep` and a `script_present: bool` flag.
`workspace::archive_with_app` updates the `step` field between its
existing four phases by briefly locking `SharedApp` — the same pattern
phase 4 already uses for the DB + MCP cleanup. The renderer reads the
field on each tick and draws a four-line checklist.

### Why this approach

- **Minimal surface area.** No new channels, no new background tasks, no
  new state on `App`. The single source of truth for "what's on screen"
  stays inside the `Modal` enum.
- **Matches existing patterns.** `archive_with_app` already takes
  `SharedApp` and already acquires the lock in phase 4. The new
  phase-boundary writes use the same idiom.
- **Compiler-enforced exhaustiveness.** Modeling phase as an
  `ArchiveStep` enum (rather than a `u8`) forces every render site and
  test to handle every variant if the pipeline ever grows.

### Alternatives considered

- **`on_progress` callback** mirroring `on_archive_line` in
  `archive()`. Adds a channel + a draining task with no architectural
  win — `archive_with_app` already locks `SharedApp`.
- **Side field on `App`** (`archive_step: Option<ArchiveStep>`). Splits
  modal state across two places so render/reset has to coordinate;
  rejected for the same coupling problem.

## Components

### `ArchiveStep` (new, in `src/ui/modal.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveStep {
    Script,         // Phase 1 in progress
    RemoveWorktree, // Phase 2 in progress
    DeleteBranch,   // Phase 3 in progress
    Cleanup,        // Phase 4 in progress
}
```

`ArchiveStep` represents the *currently running* phase. There is no
terminal `Done` variant — the modal is cleared by
`reconcile_archive_result` once the spawn finishes.

### `Modal::ArchiveRunning` (updated, in `src/ui/modal.rs`)

```rust
ArchiveRunning {
    step: ArchiveStep,
    /// Whether the repo has an archive script configured. Determines
    /// whether the Script row is rendered as in-progress/done or as
    /// "(skipped)".
    script_present: bool,
},
```

### Renderer (updated, in `src/ui/modal.rs::render`)

The existing `ArchiveRunning` arm becomes:

```rust
Modal::ArchiveRunning { step, script_present } => {
    let body = render_archive_steps(*step, *script_present, tick);
    ("archive workspace", body)
}
```

A new `render_archive_steps(step, script_present, tick) -> String`
helper produces the four-line body. For each step, the marker is:

| Position vs current step | `script_present` | Marker |
|--------------------------|------------------|--------|
| Script row               | false            | `—` with `(skipped)` suffix (overrides every other rule) |
| ordinal < current        | true             | `✓`    |
| ordinal == current       | true             | spinner frame |
| ordinal > current        | true             | `·`    |

The Script-row override is checked before the position rules so that a
no-script repo never shows the Script row spinning, even during the
brief window where `step == Script` and `run_archive` is returning
`Skipped`.

Modal height grows from 12 to 14 rows to fit four content lines + the
existing padding. Width stays at 60.

Example renderings (spinner frame shown as `⠋`):

```
  ✓ Running archive script
  ⠋ Removing worktree…
  · Deleting branch
  · Cleaning up registry
```

```
  — Archive script (skipped)
  ⠋ Removing worktree…
  · Deleting branch
  · Cleaning up registry
```

### Caller seed (updated, in `src/app/input.rs`)

The `ConfirmArchive` `KeyCode::Char('y')` handler at
`src/app/input.rs:906` already sets the modal before spawning the
archive task. The change: seed the new fields from data the caller
already has.

```rust
let script_present = repo.archive_script.as_deref()
    .map(|s| !s.trim().is_empty())
    .unwrap_or(false);
let archive_gen = app.alloc_archive_gen();
app.modal = Some(Modal::ArchiveRunning {
    step: ArchiveStep::Script,
    script_present,
});
// existing spawn unchanged...
```

The `script_present` test matches the early-out logic inside
`setup::run_archive` (`src/setup.rs:41`) so the modal's "(skipped)"
indicator is accurate.

### Phase-boundary updates (updated, in `src/workspace.rs::archive_with_app`)

After each phase completes, briefly lock `SharedApp` and advance
`step` — but only if the modal is still `ArchiveRunning` from this
archive (stale-modal guard, mirroring the `pending_archive_gen` pattern
in `reconcile_archive_result`).

```rust
// After Phase 1, before Phase 2:
advance_archive_step(&app, ArchiveStep::RemoveWorktree).await;

// After Phase 2, before Phase 3:
advance_archive_step(&app, ArchiveStep::DeleteBranch).await;

// After Phase 3, before Phase 4:
advance_archive_step(&app, ArchiveStep::Cleanup).await;
```

```rust
async fn advance_archive_step(app: &SharedApp, next: ArchiveStep) {
    let mut g = app.lock().await;
    if let Some(Modal::ArchiveRunning { step, .. }) = &mut g.modal {
        *step = next;
    }
    // else: a stale archive task — leave the current modal alone.
}
```

The phase-4 lock acquisition already exists; the new helper adds three
short additional acquisitions. Each holds the mutex for microseconds —
imperceptible to the render loop.

## Data flow

1. User presses `y` in `ConfirmArchive`. Handler computes
   `script_present`, sets
   `app.modal = ArchiveRunning { step: Script, script_present }`, and
   spawns `archive_with_app`.
2. `archive_with_app` runs Phase 1 (no-op when `script_present` is
   false), then advances to `step: RemoveWorktree`.
3. Runs Phase 2 (the slow one), advances to `step: DeleteBranch`.
4. Runs Phase 3, advances to `step: Cleanup`.
5. Runs Phase 4, returns.
6. `reconcile_archive_result` clears the modal on success or replaces it
   with `Modal::Error` on failure — unchanged.

## Error handling

- Phases 1 and 2 propagate `Result::Err` and exit the function early,
  leaving the modal frozen on the failing step until
  `reconcile_archive_result` replaces it with `Modal::Error`. This is
  desirable: the failing step is the last visible state.
- Phase 3's error is intentionally swallowed today; that stays.
- The stale-modal guard in `advance_archive_step` means a second archive
  task started after the first one's modal was replaced cannot bash the
  current modal state.

## Testing

- **Unit tests on `render_archive_steps`.** For each
  `(step, script_present)` combination, assert which lines render with
  which marker. Covers all 4 × 2 combinations plus a "stale tick" sanity
  check that the spinner frame varies with `tick`.
- **Modify `y_in_confirm_archive_transitions_to_archive_running_and_spawns_task`**
  (`src/app/input_tests.rs:2375`) to assert the initial modal is
  `ArchiveRunning { step: Script, script_present }` and that
  `script_present` matches the fixture repo's archive-script setting.
- **New test** that drives a slow archive (`sleep 0.5` archive script,
  mirroring the pattern in `esc_in_archive_running_is_noop` at
  `src/app/input_tests.rs:2450`) and observes the `step` field
  advancing past `Script` while the task is still running.
- **Render snapshot** (text-equality) for the "(skipped)" rendering when
  `script_present = false`.

## Scope notes

- No new dependencies.
- No change to `workspace::archive` (the non-TUI variant); its
  `on_archive_line` callback surface is untouched.
- No change to reconciliation (`reconcile_archive_result`).
- Modal sizing: width 60 (unchanged), height 12 → 14.
- The new `ArchiveStep` enum is `Copy` to avoid clone noise in
  `advance_archive_step` and renderer call sites.
