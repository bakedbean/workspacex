# Process tracking per workspace — Design

**Issue:** [#21](https://github.com/bakedbean/workspacex/issues/21)

## Goal

Give the user at-a-glance visibility into long-running processes (dev servers, watchers, etc.) running under each workspace's worktree, with a way to inspect and kill them from inside wsx. wsx does not start or own these processes — they're whatever the user launched (via `[t]`, a separate terminal, mise, etc.).

## Approach

**Detection by cwd matching.** Periodically shell out to `lsof -d cwd -F pcn` to list every running process and its current working directory, then bucket the results by which workspace worktree path each cwd is a descendant of. Cache results, render a small `~N` annotation on the dashboard when N > 0, and offer a modal listing the processes with SIGTERM / SIGKILL controls.

wsx never spawns these processes itself. The user retains full launch control. wsx is observability + a kill hook.

## Decisions

- **Detection tool:** Shell out to `lsof -d cwd -F pcn`. No new Rust dependency. Cross-platform (Linux + macOS). If `lsof` is missing, the feature degrades gracefully — proc count stays at 0 and the modal shows "lsof not available."
- **Throttle:** scan once every 10 seconds globally (not per-workspace — lsof returns everything in one call). Cached on `App`.
- **Filter (exclude these process names):** `bash`, `zsh`, `fish`, `sh`, `dash`, `ash`, `wsx`, `claude`, `nvim`, `vim`, `emacs`, `code`, `cursor`, `tmux`, `screen`. The list is small and stable; embedded as a `const`.
- **Filter (include rule):** process's cwd is the workspace's worktree path, or a path under it.
- **Display:**
  - Dashboard row gets a small ` ~N` annotation (e.g., `~2`) styled `merged_style` (cyan), placed between the branch column and the activity word. Hidden when N=0 to keep the row clean.
  - `k` keybind on the dashboard opens `Modal::ProcessList { workspace_id, selected: usize }`. From the attached view, `Ctrl-x k` does the same.
- **Modal layout:**
  ```
  ─── Processes in <workspace-name> ──────
    PID    COMMAND         CWD
    12345  npm             /path/to/wt
    12346  node            /path/to/wt
  ─────────────────────────────────────────
  [↑/↓] move   [k] term   [K] kill   [esc] close
  ```
- **Kill controls:** `k` sends `SIGTERM`, `K` sends `SIGKILL`. Both immediately re-scan so the modal reflects the new state.
- **No processes case:** modal still opens, displays "no tracked processes" placeholder. Reachable so the user can verify the feature is working.
- **Direct to main.** Functional feature, not subjective.

## Scope

### In
1. New `src/proc.rs` module:
   - `ProcInfo { pid: i32, command: String, cwd: PathBuf }`
   - `scan() -> Result<Vec<ProcInfo>>` — runs `lsof`, parses output, returns all processes (caller does filtering).
   - `bucket_by_worktree(procs: &[ProcInfo], worktrees: &[(WorkspaceId, &Path)]) -> HashMap<WorkspaceId, Vec<ProcInfo>>` — applies filter (deny-list + cwd descendant match) and groups.
   - `kill(pid: i32, signal: Signal) -> Result<()>` — wraps `nix::sys::signal::kill` or shells out to `kill`.
2. `App` state: `pub workspace_processes: HashMap<WorkspaceId, Vec<ProcInfo>>` + `pub last_proc_scan_ms: i64`.
3. Integration into `branch_drift_poll`: every 10 s, run scan + bucket, update state.
4. Dashboard row render: `~N` annotation when N>0 (between branch column and activity word).
5. `Modal::ProcessList { workspace_id, selected: usize }` variant.
6. Modal renderer in `src/ui/modal.rs::render_process_list(...)` (similar dispatch pattern to `UpdatesPanel`).
7. `(KeyCode::Char('k'), _)` arm in `handle_key_dashboard` — opens modal for selected workspace.
8. `(KeyCode::Char('k'), _)` arm inside the `leader_pending` block of `handle_key_attached` — opens modal for attached workspace.
9. Modal key handler: `Up`/`Down` move selection, `k` SIGTERM, `K` SIGKILL, `Esc` close. After kill, immediately re-scan.
10. Dashboard footer adds `[k] procs`.
11. Attached-view footer condensed entry: `k=procs` joins the grouped list.
12. README: new "Process tracking" section + new keybind rows.

### Out
- Starting processes from wsx (still defer to `[t]` terminal / mise / user choice).
- PTY capture / live output viewing.
- Process tree / parent-child relationships (just a flat list).
- CPU / memory stats.
- Cross-user process inspection.
- Windows support.
- Persistent task definitions / named launchers.
- Auto-restart / health-check semantics.

## Implementation notes

### lsof output format
```
$ lsof -d cwd -F pcn
p1234
ccommand
n/path/to/cwd
p5678
canother
n/other/cwd
```
Each process is a block. Lines begin with a single character indicating the field: `p` = pid, `c` = command (15 chars max), `n` = name (here, the cwd path). Blocks are not separated by blank lines; the next `p` line starts a new block. Parser: line-by-line, accumulate fields into a partial struct, push on next `p` (and at EOF).

### Filtering implementation
```rust
const PROC_DENYLIST: &[&str] = &[
    "bash", "zsh", "fish", "sh", "dash", "ash",
    "wsx", "claude",
    "nvim", "vim", "emacs",
    "code", "cursor",
    "tmux", "screen",
];

fn is_descendant_or_equal(child: &Path, parent: &Path) -> bool {
    child.starts_with(parent)
}
```

### Kill implementation
Prefer `nix::sys::signal::kill(Pid::from_raw(pid), Signal::SIGTERM)` if `nix` is already a dep; otherwise shell out to `kill -TERM <pid>` / `kill -KILL <pid>`. Check Cargo.toml first.

### Modal selection semantics
`Modal::ProcessList { workspace_id, selected }` stores the workspace id and the row index. Up/Down clamp to `0..procs.len()`. Re-scan after kill keeps the same row index but clamps if the list shrunk. If the list is empty after re-scan, render "no tracked processes" and disable kill keys.

### Renderer integration
The new modal can't be rendered by `modal::render()` because it needs live App state (the process list). Follow the `UpdatesPanel` pattern: `modal::render()` early-returns for `ProcessList`, and `draw()` dispatches to a dedicated `render_process_list(f, area, workspace_name, procs, selected, theme)` function with borrowed state.

## Risks

- **Race: process exits between scan and kill.** `kill` returns `ESRCH`; we swallow the error (treat as success) and re-scan.
- **lsof not installed.** Detect on first scan failure; show "lsof not available" placeholder in modal; row annotation stays at 0. Don't repeatedly error-log on every scan.
- **False positives from cwd inheritance.** A long-lived `bash` whose cwd happens to be the worktree counts unless denylisted. The current denylist covers shells; users running an unusual shell may need to extend it (a future per-repo `proc_denylist` setting is a reasonable follow-up).
- **False negatives from process cwd changes.** A dev server that `chdir`s after start won't be detected. Most dev servers don't; acceptable for v1.
- **lsof permission issues.** On Linux, reading another user's `/proc/<pid>/cwd` requires privileges. wsx is single-user, so processes belong to the wsx user — no issue.
- **Footer width.** Adding `[k] procs` pushes the already-wide dashboard footer further. Acceptable; ratatui clips gracefully. The compact attached-view footer absorbs `k=procs` more cheaply.

## Out-of-scope follow-ups

- Per-repo `proc_denylist` / `proc_allowlist` settings for unusual setups.
- Named task launchers (`wsx config set tasks dev:"npm run dev"`) — option B from the brainstorm. Defer until users want it.
- Live process output viewer (would require PTY-capture path; option B+).
- Process group / pgrp-aware kill ("kill the whole tree").
