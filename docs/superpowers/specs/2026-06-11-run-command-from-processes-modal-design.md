# Run a background command from the processes modal

## Problem

The dashboard's processes modal (`Modal::ProcessList`) is purely observational: it
lists processes discovered in a workspace's worktree (via `lsof`/`ps`) and lets the
user SIGTERM/SIGKILL them. There is no way to *start* anything. To run a dev server
(or any one-off command) in a workspace, the user must open a separate terminal,
`cd` to the worktree path, and type the command by hand.

We want to start that command directly from the processes modal, in the workspace's
worktree directory, without leaving wsx.

## Goal

From the processes modal, let the user type a shell command and launch it as a
background process whose working directory is the workspace's worktree. The command's
output is captured to a log file. Because the process runs with `cwd = worktree_path`,
it is automatically discovered by the existing process scan and appears in the same
modal, where the existing `[K]` kill verb manages its lifecycle.

## Non-goals

- No live/streamed output inside the TUI. Output goes to a log file; the user tails it
  themselves. (An in-modal log viewer is a possible future extension, explicitly out of
  scope for v1.)
- No persisted registry of launched commands (PID → log file mapping). The notice line
  shows the log path at launch time; we do not track it afterward.
- No command history or saved/favorite commands.
- No injection into the agent PTY session — this runs a real shell command, not agent
  input.

## Design

### 1. Interaction (UX)

Within `Modal::ProcessList`, the modal has two modes:

- **List mode** (current behavior): arrow keys navigate, `k`/`K` kill, `esc` closes.
- **Input mode** (new): entered by pressing **`r`** (run). An input line replaces the
  footer. The user types a command; **Enter** launches it, **Esc** cancels back to list
  mode.

After a launch, the modal stays open and:

1. Shows a one-line **notice** below the list: `▶ started → <log path>` on success, or a
   distinctly-styled error line on failure.
2. Calls `rescan_processes(app)` so the newly spawned process appears in the list (if it
   has started and chdir'd by scan time; otherwise it appears on the next periodic poll).

Footers:

- List mode: `[↑/↓] move   [r] run   [k] term   [K] kill   [esc] close`
- Input mode: an input line, e.g. `run: <buffer>▌   [enter] launch   [esc] cancel`

Note: `r` is unused by the ProcessList modal today. The modal deliberately does **not**
alias `j`/`k` to navigation (because `k`/`K` are the kill verbs), so adding `r` does not
collide with anything.

### 2. Modal state

Extend the variant (mirroring how `Modal::NewWorkspace` carries a `name_buffer`):

```rust
Modal::ProcessList {
    workspace_id: WorkspaceId,
    selected: usize,
    input: Option<String>,   // None = list mode; Some = typing a command
    notice: Option<String>,  // last launch result / error, shown under the list
}
```

`input.is_some()` is the single mode flag — no separate enum. `notice` is set on launch
(success or failure) and persists until the next launch or modal close. All sites that
construct `Modal::ProcessList` (open, navigate, kill-then-rebuild) must initialize the
two new fields — open sets both to `None`; navigation/kill rebuilds preserve them.

### 3. Spawn mechanics

New function in `src/commands/external.rs`:

```rust
/// Launch `command` as a detached background process with cwd = `worktree`,
/// running it through `sh -c` so shell features (pipes, &&, env vars) work.
/// stdout and stderr are redirected to `log_path` (created/truncated); stdin is null.
pub fn spawn_background_command(
    worktree: &Path,
    command: &str,
    log_path: &Path,
) -> Result<()>
```

Implementation reuses the existing detached-spawn pattern (`spawn_parts` / `detach_io`),
swapping `Stdio::null()` for the opened log file on stdout/stderr:

- Program/argv: `["sh", "-c", command]`.
- `current_dir(worktree)`.
- `stdin(Stdio::null())`.
- Open `log_path` for write (create + truncate), clone the handle, set it as both
  `stdout` and `stderr`.
- `spawn()`; map I/O errors to `Error::UserInput` consistent with the surrounding helpers.

Process lifecycle: the spawned process is a child of the wsx session. It lives as long as
wsx runs and is cleaned up when wsx exits (no orphaned dev servers). This is a deliberate
v1 choice — not a new-session/`setsid` detach.

### 4. Logging

Log file path:

```rust
Dirs::discover().log_dir().join(format!("ws{}-{}.log", workspace_id.0, epoch_ms))
```

- `log_dir()` already exists (`~/.local/state/wsx/logs`) and is created at startup by
  `Dirs::ensure()`. The launch path should still tolerate the dir being absent (create if
  needed) before opening the log file.
- `epoch_ms` is the wall-clock millisecond timestamp at launch, making the filename unique
  per launch.
- The full path is placed into `notice` so the user can `tail` it.

### 5. Wiring (`src/app/input.rs`, ProcessList match arm)

Within the existing `Modal::ProcessList` handler:

- **List mode** (`input` is `None`):
  - `r` → set `input = Some(String::new())`, leave the rest unchanged.
  - existing `↑/↓`, `k`, `K`, `esc` behavior unchanged.
- **Input mode** (`input` is `Some`):
  - printable char → push to buffer.
  - Backspace → pop from buffer.
  - Esc → `input = None` (back to list mode; `notice` unchanged).
  - Enter → if buffer trimmed is empty, no-op (stay in input mode). Otherwise:
    1. Resolve `worktree_path` for `workspace_id` from `app.workspaces`.
    2. Build `log_path` (see §4).
    3. Call `spawn_background_command(worktree, &command, &log_path)`.
    4. Set `notice` to `▶ started → <log_path>` on `Ok`, or an error string on `Err`.
    5. Set `input = None`.
    6. `rescan_processes(app).await`.

Worktree lookup mirrors the kill path / `rescan_processes`, which already iterate
`app.workspaces` by `WorkspaceId`. If the workspace can't be found (shouldn't happen while
the modal is open), set an error notice and do not spawn.

### 6. Rendering (`src/ui/modal.rs::render_process_list`)

- When `input` is `Some(buf)`: render an input line (`run: {buf}▌`) in place of the list
  footer, with a short hint (`[enter] launch  [esc] cancel`).
- When `input` is `None`: render the list footer including the new `[r] run` hint.
- When `notice` is `Some(text)`: render it as a line below the process list — success
  styled dim/neutral, error styled distinctly (e.g. the theme's error/warn color). The
  spec leaves exact styling to match existing modal conventions.

### 7. Error handling

- Empty/whitespace-only command on Enter → no-op, remain in input mode.
- Shell-spawn failure or log-file-open failure → `notice` shows the error, modal returns
  to list mode (`input = None`). No panic, no crash.
- Workspace not found for `workspace_id` → error notice, no spawn.

### 8. Testing

- **Pure unit tests:**
  - Log-path builder: given a `WorkspaceId` and a timestamp, produces the expected
    `log_dir()/wsN-<ts>.log` path.
  - `sh -c` argv construction: the command is wrapped as `["sh", "-c", command]`
    (verifying the command string is passed as a single argument, not re-split).
- **Modal state-transition tests:**
  - `r` in list mode sets `input = Some("")`.
  - A char in input mode appends; Backspace pops.
  - Esc in input mode returns to list mode (`input = None`).
  - Enter with an empty/whitespace buffer is a no-op (stays in input mode).
- **Not unit-tested:** actual process spawning, consistent with how `external.rs` already
  avoids spawning real processes in its tests. The spawn helper's pure pieces (argv,
  path) are covered above; the `Command` wiring is exercised manually.

## Known nuance

For a command like `npm run dev`, the process surfaced in the modal will be the
underlying `node`/`vite`, not the `sh`/`npm` wrappers (which may be on the process-scan
denylist). That underlying process is the correct one to kill, so this is the desired
behavior, but the listed PID/command may differ from the literal string the user typed.

## Files touched

- `src/ui/modal.rs` — `Modal::ProcessList` variant fields; `render_process_list` input
  line, footer, and notice rendering.
- `src/app/input.rs` — ProcessList key handling for `r` and input mode; all
  `Modal::ProcessList { .. }` construction sites updated for the new fields.
- `src/commands/external.rs` — `spawn_background_command` and its supporting log-file
  spawn path; unit tests for argv construction.
- (Log-path helper) — small pure function, located alongside the launch logic or in
  `external.rs`, with a unit test.
