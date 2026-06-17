# New-workspace setup feedback

## Problem

Creating a workspace can take a long time, dominated by the repo's setup
script. For example, the `ssk` repo's setup script is:

```
mise trust && mise install && cp ~/ssk/ssk-web/.env ./.env && \
  mise exec -- pnpm install && mise exec -- pnpm exec prisma generate
```

While this runs, the new-workspace modal (`Modal::SetupRunning`) shows only a
braille spinner and the static text "Creating workspace…". The user has no idea
which step is running or whether anything is progressing — it can look hung.

The output is *already captured* line-by-line in `setup::run_script` and handed
to an `on_line` callback, but `create_with_app` passes `|_| {}` and discards
every line. This design connects that output to the modal.

## Goal

Replace the bare spinner with:

1. A **coarse phase label** in the header (`Fetching base…` / `Creating
   worktree…` / `Running setup…`), so the pre-script phases — which produce no
   tailable output — never look idle.
2. A **live tail** of the setup script's stdout/stderr (last few lines,
   scrolling as new lines arrive). To make in-place progress bars (pnpm, mise)
   visible, output is segmented on `\r` *and* `\n`, not `\n` alone.
3. An **elapsed timer** `(MM:SS)` so the user can see it is still moving.

Esc still cancels; all other behavior is unchanged.

## Non-goals (YAGNI)

- **Surfacing a failed setup script.** Today a failing setup script still
  creates the workspace (row marked `SetupStatus::Failed`) and the modal closes
  silently. Surfacing that failure is a separate concern, out of scope here.
- No persistence of setup output after the modal closes, no scrollback, no
  separate log pane.

## Architecture

A **data-layer progress sink** that the data layer writes and the UI layer
reads. `create_with_app` stays UI-agnostic — it depends on the progress type,
not on `ui::modal`. The sink rides the same `input.rs` → `create_with_app` path
that the existing `CancellationToken` already travels.

### New module: `src/data/progress.rs`

```rust
pub enum SetupPhase {
    Fetching,
    CreatingWorktree,
    RunningSetup,
}

pub struct SetupProgress {
    phase: SetupPhase,
    lines: VecDeque<String>,   // ring buffer, bounded (cap ~64)
}

pub type SharedProgress = std::sync::Arc<std::sync::Mutex<SetupProgress>>;
```

API:

- `SetupProgress::shared() -> SharedProgress` — constructor for a new handle,
  starting in the `Fetching` phase.
- `set_phase(&self, p: SetupPhase)`.
- `push_line(&mut self, raw: &str)` — strip ANSI/control sequences, push, and
  evict the oldest if over the cap.
- Read accessors for the renderer: `phase()` and `recent(n) -> Vec<&str>` (or an
  equivalent that returns the last `n` lines).

**Why `std::sync::Mutex`, not `tokio::sync::Mutex`:** the producer is
`run_setup`'s `on_line` callback, a *synchronous* `FnMut` called inline from the
reader loop; the consumer is `render()`, also synchronous. Neither can `.await`.
A std mutex is locked, mutated, and released in microseconds on both sides, and
is never held across an `.await`, so it cannot deadlock with the app's tokio
`Mutex<App>`.

**ANSI stripping:** `-ilc` runs an interactive login shell, so tools may emit
color even with piped stdout. `push_line` strips escape sequences via the
`strip-ansi-escapes` crate (`strip_str`) before storing the line, then trims
trailing whitespace. Add the dependency with `cargo add strip-ansi-escapes`.

### Output segmentation: split on `\r` and `\n` (`src/data/setup.rs`)

`run_script` currently reads each pipe with `BufReader::…lines()`, which splits
on `\n` only — so a tool that redraws a progress bar in place with carriage
returns (pnpm, mise) emits nothing tailable until its final newline. Replace the
two `Lines` readers with a small **segmenter** that yields a segment on either
`\r` or `\n`:

- A `SegmentReader<R: AsyncRead + Unpin>` wrapping the pipe, exposing
  `async fn next_segment(&mut self) -> io::Result<Option<String>>`. It reads
  bytes into an internal buffer and flushes the accumulated bytes (as
  `String::from_utf8_lossy`) whenever it hits `\r` or `\n`. A `\r\n` pair yields
  a single segment (the trailing `\n` after a just-emitted `\r` does not produce
  an empty segment). EOF flushes any trailing partial segment.
- The `tokio::select!` loop is unchanged in shape — `out_reader.next_line()`
  becomes `out_reader.next_segment()`, preserving the biased cancellation arm
  and the stdout/stderr interleave. The post-loop drain switches the same way.
- `run_script` is shared by `run_setup` and `run_archive`, so both gain
  finer-grained output. The `SetupLine::Stdout/Stderr` callback contract is
  unchanged; only the cadence and delimiters of the segments differ. Empty
  segments (e.g. a blank line) are skipped so the callback is not spammed.

Unit-tested directly against `SegmentReader` with byte inputs containing `\n`,
`\r`, `\r\n`, and a trailing unterminated segment.

### Wiring (mirrors the existing cancel-token threading)

1. **`src/app/input.rs`** — `NewWorkspace` Enter handler:
   - Construct `let progress = SetupProgress::shared();`.
   - Build `Modal::SetupRunning { cancel, progress: progress.clone(), started:
     Instant::now() }`.
   - Pass `progress` as a new argument to `create_with_app`.

2. **`src/data/workspace.rs`** — `create_with_app` gains one parameter
   `progress: SharedProgress` and:
   - calls `set_phase(Fetching)` before `fetch_for_base` (phase 2),
   - calls `set_phase(CreatingWorktree)` before `create_worktree` (phase 4),
   - calls `set_phase(RunningSetup)` before `run_setup` (phase 5),
   - replaces the `|_| {}` callback with one that locks `progress` and calls
     `push_line` for each `SetupLine::Stdout`/`Stderr` (both merged into the one
     tail, no prefix — many tools log progress to stderr).

### Modal & render

- `src/ui/modal/mod.rs`:
  ```rust
  SetupRunning {
      cancel: tokio_util::sync::CancellationToken,
      progress: crate::data::progress::SharedProgress,
      started: std::time::Instant,
  }
  ```
  `SharedProgress` is `Arc` (Clone) and `Instant` is Copy, so the existing
  `#[derive(Debug, Clone)]` on `Modal` still holds.

- `render()` for `SetupRunning`:
  - lock `progress`, map the phase to a header word, format
    `started.elapsed()` as `(MM:SS)`, render the spinner frame;
  - render the last N tail lines that fit the modal body (modal is 60×14;
    budget ~6 rows for the tail after header + spacer + `[esc] cancel`
    footer), each truncated to the inner width with an ellipsis;
  - footer `[esc] cancel` unchanged.

- `src/app/input.rs` `SetupRunning` key handler and `app.rs`
  `reconcile_create_result` are **unchanged** beyond the added struct fields —
  Esc still cancels, completion closes the modal, git errors still route to
  `Modal::Error`.

## Data flow

```
input.rs (Enter)
  ├─ progress = SetupProgress::shared()
  ├─ Modal::SetupRunning { cancel, progress.clone(), started }
  └─ spawn create_with_app(.., progress, cancel)
        ├─ set_phase(Fetching)        → fetch_for_base
        ├─ set_phase(CreatingWorktree)→ create_worktree
        └─ set_phase(RunningSetup)    → run_setup(on_line = push_line)
                                              │ (each stdout/stderr line)
                                              ▼
                                    SetupProgress (std Mutex)
                                              ▲
render() ── locks progress, draws phase + tail + timer ──┘  (every frame)
```

## Error handling

- Unchanged. Git failures during fetch/worktree still return `Err` and
  `reconcile_create_result` shows `Modal::Error`. A failed setup script still
  completes with `SetupStatus::Failed` (see non-goals).
- The std mutex uses `lock()`; on the (practically impossible) poison case the
  renderer falls back to showing no tail lines rather than panicking.

## Testing

- **`setup.rs` segmenter tests:** `SegmentReader` over byte inputs splits on
  `\n`, `\r`, and `\r\n` (the pair yields one segment); a trailing unterminated
  segment is flushed at EOF; empty segments are skipped.
- **`progress.rs` unit tests:** ring-buffer eviction at cap; `set_phase`
  transitions; `push_line` strips ANSI escapes and trims trailing whitespace.
- **`render()` tests** (extend the existing `TestBackend` pattern in
  `modal/mod.rs`): given a `SetupProgress` seeded with a phase and several
  lines, the rendered buffer contains the phase word and the most recent lines;
  an over-wide line is truncated to the inner width.

## Commits

1. `feat(data): segment setup output on CR and LF` — replace the `Lines`
   readers in `run_script` with `SegmentReader`; segmenter unit tests. Benefits
   both setup and archive; no behavior change visible to users yet.
2. `feat(data): add SetupProgress sink for workspace creation` — `progress.rs`
   module + `strip-ansi-escapes` dependency + unit tests.
3. `feat(data): report create phases and setup output to progress sink` —
   `create_with_app` gains the `progress` param and sets phases + pushes lines;
   the `input.rs` Enter handler constructs `progress` and passes it in. The
   `Modal::SetupRunning` struct is **not** changed yet (no progress field), so
   the sink is written but not read — the modal still shows the old spinner.
   Each commit compiles and passes tests.
4. `feat(dashboard): show phase + live setup output in new-workspace modal` —
   add the `progress` + `started` fields to `Modal::SetupRunning`, populate them
   in the `input.rs` handler, render the phase/tail/timer, add render tests.
