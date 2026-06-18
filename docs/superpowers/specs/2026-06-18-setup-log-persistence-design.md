# Persist workspace-creation setup logs

## Problem

When creating a new workspace, the `Modal::SetupRunning` modal streams the
repo's `setup_script` output live (last 6 lines, see
[`2026-06-17-new-workspace-setup-feedback-design.md`]). This works well for a
successful run, but when the script **fails** the output is lost:

- A non-zero setup-script exit is **not** an `Err`. `create_with_app` returns
  `Ok(CreatedWorkspace { setup_result: Failed { exit_code } })`
  (`workspace.rs:264`), with `SetupStatus::Failed` persisted.
- In `reconcile_create_result` (`app.rs:1430`) that `Ok(_)` lands in the
  success branch → `g.modal = None`. **The modal silently closes.**
- The captured output lives only in the `SharedProgress` ring buffer owned by
  the modal (`Arc<Mutex<SetupProgress>>`, cap 64 lines). When the modal is
  dropped, the buffer is dropped. The user is left with a `Failed` badge in the
  dashboard and no way to see what went wrong.

The prior feature called this out as a non-goal; this spec closes the gap.

## Goal

Persist each workspace-creation's setup output to a predictable file so the
user can read it after the fact. The failed workspace already shows a `Failed`
badge in the dashboard; the user proactively opens the log when they need it.

## Non-goals

- **No modal/UX change.** The modal still auto-closes on completion (success or
  failure). No held-open failed state, no extra keypress, no in-TUI pager — the
  preferred behavior is to not make the user perform an extra action unless we
  can confidently identify and present an error inline, which we are explicitly
  deferring.
- **No detail-bar or CLI-help surfacing.** Discovery is purely proactive,
  documented in the README.
- **No change to the archive path.** `run_archive` is untouched.
- **No history.** One file per workspace, overwritten each run (latest only).

## Design

### Log location

Reuse the existing `Dirs::log_dir()` (`~/.local/state/wsx/logs/`, the same
directory as `wsx.log` and background-command logs). One file per workspace,
stable name, truncated on each run:

```
~/.local/state/wsx/logs/setup-<repo>-<name>.log
```

`<repo>` and `<name>` are filesystem-sanitized (any char outside
`[A-Za-z0-9._-]` replaced with `-`). The name is stable so the file is trivially
locatable: "the setup log for workspace `foo` in repo `myrepo` is always
`setup-myrepo-foo.log`." Browsable with `ls`.

### Log contents — captured phase

Only the **setup-script phase (Phase 5)** is captured. The earlier fetch and
worktree phases already surface failures via a held-open `Modal::Error`
(`workspace.rs:184,219`), so they are not the lost data — and they run before
`id`/`worktree` exist anyway. Both `id` and `worktree_path` are available by the
time Phase 5 runs.

File layout:

```
=== setup: myrepo/foo ===
worktree: /home/eben/.../myrepo/foo
started:  1718722921 (unix seconds)

<streamed output, arrival order, ANSI stripped; stderr lines prefixed "! ">
...
ERR_PNPM_NO_MATCHING_VERSION  No matching version found for foo@^9

=== FAILED (exit 1) ===
```

- Header: repo/name, worktree path, start time. `crate::time` exposes only epoch
  helpers (no date library is a dependency), so the timestamp is epoch seconds
  with an explicit `(unix seconds)` label; the file's own mtime gives the
  human-readable time.
- Body: each `SetupLine` written in arrival order. ANSI escapes stripped (reuse
  `strip-ansi-escapes`, already a dependency). `Stderr` lines are prefixed
  `! ` so a reader can tell stdout from stderr. Blank lines after trimming are
  skipped (matching the on-screen buffer).
- Footer: the outcome — `=== OK ===`, `=== FAILED (exit N) ===`, or
  `=== SKIPPED ===` (the last is unreachable in practice because the log is only
  opened when a setup script exists; see below).

### New module: `src/data/setup_log.rs`

A small, focused, filesystem-free-testable module:

```rust
/// `<log_dir>/setup-<repo>-<name>.log`, with repo/name sanitized.
pub fn setup_log_path(log_dir: &Path, repo: &str, name: &str) -> PathBuf;

/// Best-effort: open (truncating) the log file and write the header. Returns
/// None if the file can't be created. `write_header` is a private helper.
pub fn create(log_dir: &Path, repo: &str, name: &str,
              worktree: &Path, started_secs: u64) -> Option<BufWriter<File>>;

/// Pure formatting over any Writer — unit-testable with a Vec<u8>.
pub fn write_line(w: &mut impl Write, line: &SetupLine) -> io::Result<()>;
pub fn write_footer(w: &mut impl Write, result: &SetupResult) -> io::Result<()>;
```

`SetupLine` / `SetupResult` are re-used from `data::setup`. The formatting
functions take `&mut impl Write`, so tests drive them with a `Vec<u8>` and
assert on the bytes; production drives them with a `BufWriter<File>`.

### Wiring in `create_with_app` (`src/data/workspace.rs`)

Localized to the Phase 5 block (around `workspace.rs:235-263`), gated on
`repo.setup_script` being present:

1. Before `run_setup`: build the path via `setup_log_path(&Dirs::discover().log_dir(), &repo.name, &final_name)`, `create_dir_all` the parent, open a `BufWriter<File>` (truncating), and `write_header`. **All best-effort** — see below.
2. The existing `on_line` closure (`workspace.rs:245`) already receives every
   `SetupLine` and is `FnMut(SetupLine) + Send` (`setup.rs:92`). It captures the
   `BufWriter` by move (in addition to the existing `progress` clone) and calls
   `write_line` for each line. No interior mutability needed.
3. After `run_setup` returns: `write_footer` with the `SetupResult`, then drop
   the writer (flushing).

`setup.rs` / `run_setup` are **not** modified. The progress-sink behavior is
unchanged; the file write is an additional consumer of the same line stream.

### Failure tolerance

Log I/O is strictly best-effort. Opening the file, every `write_*` call, and the
final flush are all `let _ = …`. If the log directory can't be created or a
write fails, workspace creation proceeds **exactly as today**. Logging must
never break or slow the create flow. When `repo.setup_script` is `None`/blank,
no file is opened at all (nothing to capture).

## Data flow

```
create_with_app (Phase 5)
  ├─ setup_log_path() ─ open BufWriter<File> (best-effort) ─ write_header()
  ├─ run_setup(on_line: move |line| {
  │      progress.lock().push_line(text)   // existing — drives the modal
  │      setup_log::write_line(&mut buf, &line)   // new — drives the file
  │  })
  └─ write_footer(&mut buf, &setup_result) ─ drop(buf)   // flush
```

## Testing

- **`setup_log.rs` unit tests** (no filesystem):
  - `setup_log_path` builds `setup-<repo>-<name>.log` under the given dir; repo
    and name with slashes / spaces / unsafe chars are sanitized to `-`.
  - `write_header` emits the repo/name, worktree, and `(unix seconds)` line.
  - `write_line` strips ANSI escapes; prefixes `! ` for `Stderr`; skips blank
    lines; passes `Stdout` through unprefixed.
  - `write_footer` renders `OK`, `FAILED (exit N)`, and `SKIPPED`.
- **`workspace.rs` integration** (extend existing create tests that build a real
  git repo + worktree): after a create whose `setup_script` writes a known
  marker to stdout and exits non-zero, the file at `setup_log_path(...)` exists
  and contains the header, the marker line, and `FAILED (exit N)` footer. A
  create with no setup script writes no file.
- Best-effort: a create with an unwritable log dir still succeeds (the existing
  create assertions hold; the log file is simply absent).

## Documentation

Add a short note to the README (fits the OSS-prep effort): setup output for each
workspace creation is logged to
`~/.local/state/wsx/logs/setup-<repo>-<name>.log`; check it if a new workspace
shows a failed setup. No CLI-help or detail-bar surfacing.

## Commits

1. `feat(data): add setup_log module for setup-output persistence` — new
   `src/data/setup_log.rs` (`setup_log_path` + `write_header`/`write_line`/
   `write_footer`) with filesystem-free unit tests. Wired into nothing yet.
2. `feat(data): persist setup output to a log file during workspace creation` —
   `create_with_app` opens the logger (best-effort), writes the header, tees
   each line in the `on_line` closure, and writes the footer; integration test.
3. `docs: document the workspace setup log location` — README note.
```
