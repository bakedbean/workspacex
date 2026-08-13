# Backgrounding workspace create and archive

## Problem

Creating and archiving a workspace in a setup-intensive repo blocks the
whole TUI for about twenty seconds at each end. Measured on `ssk-web`:

| Phase | Time |
| --- | --- |
| `git fetch` for the base branch | ~1s |
| `git worktree add` (4,930 tracked files) | ~1-2s |
| Setup script (`mise` + `pnpm install` + `prisma generate`) | 17-19s |
| Archive: worktree removal (119,600 files / 1.6G `node_modules`) | ~18s |

The setup-script figure is the median of 120 logged runs under
`~/.local/state/wsx/logs/setup-ssk-web-*.log`, and it barely varies.

Making the underlying work faster is not available. `pnpm` already
hardlinks from its global store, so the install has little fat to cut, and
the obvious alternative — cloning `node_modules` with APFS copy-on-write
(`cp -Rc`) — measured 36s against pnpm's 11.4s. It is three times slower,
not faster. The win has to come from not making the user wait.

Fortunately the concurrency work is already done. Both `create_with_app`
(`src/data/workspace.rs:198`) and `archive_with_app` (`:403`) already run
as detached `tokio` tasks, and both are written to never hold the App lock
across an `.await` — their phase comments say so explicitly. The event loop
keeps ticking and redrawing throughout. Nothing is blocked except the UI's
willingness to accept input: `Modal::SetupRunning` swallows every key but
Esc (`src/app/input.rs:1386`), and `Modal::ArchiveRunning` swallows all of
them (`:1395`).

So the blocking is a modal convention, not a technical constraint, and the
change is one of state tracking and rendering rather than concurrency.

The structural obstacle is ownership. `SharedProgress` and the
`CancellationToken` live *inside* `Modal::SetupRunning`
(`src/ui/modal/mod.rs:72-76`), so dismissing the modal drops the only
handle to the running work. That is why Esc cancels rather than
backgrounds: there is nowhere for the work to live once the modal closes.

## Decision

Create and archive both run in the background by default. The dashboard
gains a badge vocabulary for in-flight and failed lifecycle states, and the
progress modal becomes a viewer you can open and close at will.

### Ownership

`App` owns a registry of in-flight work; the modal borrows a view of it.

```rust
enum InFlightKind { Create, Archive }

struct InFlight {
    kind: InFlightKind,
    progress: SharedProgress,
    cancel: CancellationToken,
    started: Instant,
}

// on App
in_flight: HashMap<WorkspaceId, InFlight>,
```

`Modal::SetupRunning { cancel, progress, started }` becomes
`Modal::SetupProgress { workspace_id }`, reading from the registry. Esc
closes the viewer and does not cancel. Cancelling moves to its own entry in
the workspace-actions modal (`?`), alongside the entry that opens the
viewer — discarding eighteen seconds of work should not be one keystroke
away from "get this off my screen," which is exactly today's failure mode.
Only creates are cancellable; archive has no cancellation today
(`src/data/workspace.rs:365` passes a token nothing ever fires) and gains
none here. The `SetupProgress` ring buffer
(`src/data/progress.rs:34`) and the per-workspace log file
(`src/data/setup_log.rs`) both persist, so a closed and reopened viewer
loses nothing.

### Row insertion moves before the fetch

`create` currently fetches before inserting the workspace row, with a
comment stating the reason (`src/data/workspace.rs:72-73`):

> Fetch before inserting the workspace row so a fetch failure (network
> down, bad remote ref) doesn't leave an orphan Pending row.

That reasoning held while a failure had a modal to report into. It stops
holding here: the promise is that the row appears immediately, and the row
cannot appear before it exists. The order becomes insert → fetch →
worktree → setup, and a fetch failure marks the row
`WorkspaceState::Failed` and badges it. The orphan the old comment feared
becomes a visible, actionable row — strictly more discoverable than a
transient error modal, not less.

### Two independent status axes

`WorkspaceState` tracks whether the worktree exists; `SetupStatus` tracks
whether the dependencies installed. They stay separate because they fail
separately: a workspace can have a real worktree with failed setup (usable,
degraded) or no worktree at all (unusable).

`WorkspaceState::Pending` is written on every insert
(`src/data/store.rs:192`) but read by no UI code today, so a half-created
workspace renders as an ordinary idle row. Backgrounding makes that
misreporting load-bearing, so `Pending` finally has to mean something.

`SetupStatus` gains a persisted `Running` variant, written when the setup
script starts and replaced on completion.

The two sources are not redundant, and their roles must not blur. The
in-flight registry is the sole source for the spinner badges: it is
in-process, so its presence proves a task is genuinely alive. The
persisted `Running` status exists only so a *crashed* process leaves
evidence on disk for the startup sweep to find. A row is therefore never
badged as provisioning on the strength of the stored status alone — by the
time the dashboard first draws, the sweep has already resolved every
orphaned `Running` row to `Cancelled`.

### Per-repo git serialization

Backgrounding creates a hazard that does not exist today: with the modal
gone, three `ssk-web` workspaces can be started within five seconds.
`git worktree add -b` writes to `.git/worktrees/` and creates a branch in
the shared repo, and `git worktree remove` prunes that same admin
directory.

A per-repo `tokio::Mutex` guards the git phases only. Setup scripts stay
fully parallel — they are the eighteen-second part, and each touches only
its own worktree.

### Badges

`RowInputs.setup_failed: bool` (`src/ui/dashboard/row.rs:88-118`), derived
by a single line at `src/app/render.rs:837`, becomes an enum derived by a
match over `(ws.state, ws.setup_status, in_flight.get(&ws.id))`:

| Condition | Badge |
| --- | --- |
| Create in flight | spinner + `⚙` |
| Archive in flight | spinner + `⌫` |
| `setup_status == Failed` | `⚙!` (exists today, `row.rs:291`) |
| `setup_status == Cancelled` | `⚙?` |
| `state == Failed` (no worktree) | `✗` |

The badge column already reserves width and emits spans for `⚙!`
(`row.rs:259`, `:291-293`), so this extends a pattern rather than inventing
one.

The animation belongs in the badge column, not the glyph column. The glyph
at `row.rs:193-197` already spins on `inputs.status.is_live()`, which
tracks the *agent's* liveness. Since a workspace can now be attached while
its setup runs, both can be live simultaneously and they mean different
things: "the agent is thinking" versus "dependencies are installing." One
spinner would conflate them. Both are driven by the same `tick`
(`src/ui/dashboard/spinner.rs:12`).

`Status::priority` (`src/ui/dashboard/status.rs:21-31`) is left alone. It
drives within-repo sort order, and a provisioning row is not asking for
attention; sorting it high would shuffle rows under the cursor.
`reconcile_create_result` already selects the new workspace and unfolds its
repo (`src/app.rs:2473-2489`), which is the right amount of emphasis.

### Archive

Archive uses the same registry under `InFlightKind::Archive`.
`advance_archive_step` (`src/data/workspace.rs:391`) writes a phase into
the registry instead of mutating the modal. The row stays visible with an
archiving badge and vanishes when `delete_workspace` lands and `refresh()`
runs, leaving somewhere for a removal failure to surface.

One guard is mandatory. Archive's first act is killing the workspace's tmux
sessions (`src/data/workspace.rs:364`), specifically so a live agent cannot
dirty the worktree during teardown. Without a guard, attaching mid-archive
would respawn a session into a directory that is actively being deleted, so
`attach_workspace` (`src/app.rs:2393`) needs an early return for any
workspace with an archive in flight.

### Interruption

Background work dies with the process. Prevention is the primary mechanism:
quitting with `!in_flight.is_empty()` prompts for confirmation and lists
what is running. Confirming quits immediately without waiting — the prompt
informs the decision, it does not stall on the work. In-flight creates are
cancelled on the way out so their rows land on `Cancelled` rather than
relying on the sweep; archives, having no cancellation, are simply
abandoned. An abandoned archive is self-healing: it leaves a partially
deleted worktree and a live row, and archiving again finishes the job,
since `remove_worktree` (`src/git/mod.rs:402`) falls back to
`remove_dir_all` when git no longer recognizes the path.

A quit guard cannot catch `kill -9`, a panic, or a closed terminal, and in
those cases a row persists as `SetupStatus::Running` with no task behind
it — the dashboard would spin a badge indefinitely for work that died days
ago. A permanent misreport is worse than the wait this change removes. So
at startup, any row still marked `Running` is flipped to the existing
`Cancelled` status, reusing the shape of `sweep_stale_pending`
(`src/data/store.rs:313`, called from `src/app.rs:703`). `Cancelled`
already means setup did not finish and already earns a badge, so this needs
no new enum variant.

### Out of scope

- **CLI `wsx workspace create` stays blocking.** Its caller is a script or
  a handoff agent that runs `wsx agent send` next and needs the worktree
  real, and there is no dashboard to badge. No change.
- **No re-run-setup command.** If setup fails today you enter the worktree
  and run it by hand, and that remains true. Whether backgrounding makes
  failed setups common enough to warrant recovery tooling is a question to
  answer with experience, not up front.
- **No agent awareness of setup state.** An attached agent may run tests
  against half-installed dependencies. Pacing that is the operator's call;
  the badge already says setup is running.

## Implementation

Six commits. The first four are behavior-preserving; the user-visible flip
lands in the last two.

1. **`SetupStatus::Running` and the stale sweep.** Add the variant and its
   persistence mapping (`setup_label`/`parse_setup`,
   `src/data/store.rs:393-410`). Write it at the start of the setup phase.
   Add the startup sweep flipping stale `Running` to `Cancelled`.
2. **Hoist progress and cancellation into `App.in_flight`.** Pure
   refactor: the modal still opens and behaves identically, but reads its
   handles from the registry rather than owning them.
3. **Reorder insert before fetch.** Covered by a test asserting that a
   fetch failure leaves a `Failed` row rather than no row — the exact
   inversion of the comment at `src/data/workspace.rs:72`.
4. **Per-repo git mutex,** with a test that two concurrent creates in one
   repo both succeed.
5. **Badges and `Modal::SetupProgress`.** Create stops opening a modal;
   the viewer opens from the workspace-actions modal (`?`) rather than
   claiming a new top-level key.
6. **Archive backgrounding,** the attach guard, and the quit confirmation.

### Testing

The repository tests this area inline and thoroughly — `src/data/workspace.rs:614`
onward asserts on `state` and `setup_status` after create, and
`src/app/render.rs:1222` onward runs full render passes via the
`test_workspace` and `test_spawn_session` helpers. Both patterns extend
directly:

- Badge derivation is a pure table test over
  `(state, setup_status, in_flight)`.
- Registry lifecycle: an entry appears on spawn and is removed by each
  reconciler, including the cancelled and failed paths.
- Fetch failure leaves a `Failed` row (commit 3).
- Two concurrent creates in one repo both succeed (commit 4).
- `attach_workspace` refuses a workspace with an archive in flight.
- The startup sweep flips `Running` to `Cancelled` and leaves every other
  status untouched.
