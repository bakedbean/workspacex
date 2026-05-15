# Process tracking per workspace — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Detect long-running processes whose cwd is inside any workspace's worktree, surface a `~N` annotation on the dashboard, and offer a modal listing them with SIGTERM/SIGKILL controls. wsx does not own these processes; it observes them and provides a kill hook.

**Architecture:** New `src/proc.rs` module wraps `lsof -d cwd -F pcn`. Results bucket by workspace via cwd-descendant matching, filtered by a static deny-list of shells/editors. `App` caches results; `branch_drift_poll` refreshes every 10 s. Dashboard row gets an inline ` ~N` between branch column and activity word. New `Modal::ProcessList` variant follows the `UpdatesPanel` dispatch pattern. Keybinds: `k` on dashboard + `Ctrl-x k` in attached view. Direct to main.

**Tech Stack:** Rust, tokio (async shell-out via `tokio::process::Command`), crossterm (keybinds), ratatui (modal). No new crate dependencies; kill is `Command::new("kill").args(["-TERM", "<pid>"])`.

**Spec:** `docs/superpowers/specs/2026-05-15-process-tracking-design.md`

---

## File Structure

- `src/proc.rs` — **NEW**. `ProcInfo` struct, `scan()`, `parse_lsof_output()`, `bucket_by_worktree()`, `kill_pid()`, `PROC_DENYLIST` const. Pure functions where possible; only `scan()` and `kill_pid()` shell out.
- `src/lib.rs` — declare `pub mod proc;`.
- `src/app.rs` — extend `App` with `workspace_processes` + `last_proc_scan_ms`; integrate scan into `branch_drift_poll`; new key arms for `k` on dashboard and `Ctrl-x k` in attached view; new modal-handler routing.
- `src/ui/modal.rs` — `Modal::ProcessList { workspace_id, selected }` variant; `render()` early-return for the new variant; new `render_process_list()` function (follows `render_updates_panel` pattern); modal key handler dispatches `Up`/`Down`/`k`/`K`/`Esc`.
- `src/ui/dashboard.rs` — extend `workspace_main_row` to accept the process count and emit ` ~N` between branch column and activity word; footer string updated.
- `src/ui/attached.rs` — compact footer gains `k=procs`.
- `README.md` — new "Process tracking" section; keybind tables updated.

---

### Task 1: `src/proc.rs` — scan, parse, bucket, kill

**Files:**
- Create: `src/proc.rs`
- Modify: `src/lib.rs` (declare module)

- [ ] **Step 1: Write failing tests in a fresh `src/proc.rs`**

Create `src/proc.rs` with the following test scaffold (these reference types/functions that don't exist yet):

```rust
//! Per-workspace process detection.
//!
//! wsx never spawns these processes; it observes the system via `lsof`
//! and offers a kill hook. See `docs/superpowers/specs/2026-05-15-process-tracking-design.md`.

use crate::error::{Error, Result};
use crate::store::WorkspaceId;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: i32,
    pub command: String,
    pub cwd: PathBuf,
}

/// Process names that should never count as user processes for a
/// workspace, even when their cwd matches. Covers shells (which host
/// the interesting children but aren't themselves interesting),
/// wsx-spawned things (claude), and editors launched via `[e]`.
pub const PROC_DENYLIST: &[&str] = &[
    "bash", "zsh", "fish", "sh", "dash", "ash",
    "wsx", "claude",
    "nvim", "vim", "emacs",
    "code", "cursor",
    "tmux", "screen",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lsof_output_handles_three_processes() {
        let raw = "p1234\ncnpm\nn/home/u/wt/a\np5678\ncnode\nn/home/u/wt/a\np9012\ncbash\nn/home/u/wt/b\n";
        let procs = parse_lsof_output(raw);
        assert_eq!(procs.len(), 3);
        assert_eq!(procs[0].pid, 1234);
        assert_eq!(procs[0].command, "npm");
        assert_eq!(procs[0].cwd, PathBuf::from("/home/u/wt/a"));
        assert_eq!(procs[2].command, "bash");
    }

    #[test]
    fn parse_lsof_output_handles_empty() {
        assert!(parse_lsof_output("").is_empty());
    }

    #[test]
    fn parse_lsof_output_skips_block_missing_pid() {
        // A block with c and n but no p is dropped (malformed).
        let raw = "cstray\nn/tmp\np1\ncgood\nn/x\n";
        let procs = parse_lsof_output(raw);
        assert_eq!(procs.len(), 1);
        assert_eq!(procs[0].pid, 1);
    }

    #[test]
    fn bucket_groups_by_descendant_match() {
        let procs = vec![
            ProcInfo { pid: 1, command: "npm".into(), cwd: PathBuf::from("/wt/a") },
            ProcInfo { pid: 2, command: "node".into(), cwd: PathBuf::from("/wt/a/sub/dir") },
            ProcInfo { pid: 3, command: "pytest".into(), cwd: PathBuf::from("/wt/b") },
            ProcInfo { pid: 4, command: "elsewhere".into(), cwd: PathBuf::from("/other") },
        ];
        let worktrees: Vec<(WorkspaceId, &Path)> = vec![
            (WorkspaceId(10), Path::new("/wt/a")),
            (WorkspaceId(20), Path::new("/wt/b")),
        ];
        let bucketed = bucket_by_worktree(&procs, &worktrees);
        assert_eq!(bucketed.get(&WorkspaceId(10)).unwrap().len(), 2);
        assert_eq!(bucketed.get(&WorkspaceId(20)).unwrap().len(), 1);
        assert!(!bucketed.contains_key(&WorkspaceId(30)));
    }

    #[test]
    fn bucket_filters_out_denylist_commands() {
        let procs = vec![
            ProcInfo { pid: 1, command: "bash".into(), cwd: PathBuf::from("/wt/a") },
            ProcInfo { pid: 2, command: "npm".into(), cwd: PathBuf::from("/wt/a") },
            ProcInfo { pid: 3, command: "claude".into(), cwd: PathBuf::from("/wt/a") },
            ProcInfo { pid: 4, command: "nvim".into(), cwd: PathBuf::from("/wt/a") },
        ];
        let worktrees = vec![(WorkspaceId(10), Path::new("/wt/a") as &Path)];
        let bucketed = bucket_by_worktree(&procs, &worktrees);
        let list = bucketed.get(&WorkspaceId(10)).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].command, "npm");
    }

    #[test]
    fn bucket_excludes_non_matching_cwd() {
        let procs = vec![ProcInfo {
            pid: 1,
            command: "npm".into(),
            cwd: PathBuf::from("/somewhere/else"),
        }];
        let worktrees = vec![(WorkspaceId(10), Path::new("/wt/a") as &Path)];
        let bucketed = bucket_by_worktree(&procs, &worktrees);
        assert!(bucketed.get(&WorkspaceId(10)).is_none_or(|v| v.is_empty()));
    }
}
```

Also add `pub mod proc;` to `src/lib.rs` next to the other `pub mod` declarations.

- [ ] **Step 2: Run tests; confirm compile failure**

```
cargo test --lib proc:: 2>&1 | tail -15
```
Expected: undefined `parse_lsof_output`, `bucket_by_worktree`. (And `kill_pid`/`scan` aren't tested unit-style — they're integration-tested implicitly via the modal in Task 5; if you'd like, add a smoke test that `scan()` errors gracefully when `lsof` is missing — see step 3 below.)

- [ ] **Step 3: Implement the module body**

Add inside `src/proc.rs` (above the `#[cfg(test)] mod tests`):

```rust
/// Parse `lsof -d cwd -F pcn` output into a list of `ProcInfo`.
///
/// Each process is a block of lines beginning with single-char field
/// indicators: `p` (pid), `c` (command), `n` (cwd path). Blocks are
/// not separated by blank lines — the next `p` starts a new block.
pub fn parse_lsof_output(raw: &str) -> Vec<ProcInfo> {
    let mut out = Vec::new();
    let mut pid: Option<i32> = None;
    let mut command: Option<String> = None;
    let mut cwd: Option<String> = None;

    let mut flush = |pid: &mut Option<i32>, command: &mut Option<String>, cwd: &mut Option<String>, out: &mut Vec<ProcInfo>| {
        if let (Some(p), Some(c), Some(n)) = (pid.take(), command.take(), cwd.take()) {
            out.push(ProcInfo { pid: p, command: c, cwd: PathBuf::from(n) });
        } else {
            *pid = None;
            *command = None;
            *cwd = None;
        }
    };

    for line in raw.lines() {
        let Some((tag, rest)) = line.split_at_checked(1) else {
            continue;
        };
        match tag {
            "p" => {
                // Starting a new block — flush the previous one.
                flush(&mut pid, &mut command, &mut cwd, &mut out);
                pid = rest.parse::<i32>().ok();
            }
            "c" => command = Some(rest.to_string()),
            "n" => cwd = Some(rest.to_string()),
            _ => {}
        }
    }
    flush(&mut pid, &mut command, &mut cwd, &mut out);
    out
}

/// Bucket processes by which workspace's worktree their cwd falls under,
/// applying the deny-list filter on command name.
pub fn bucket_by_worktree(
    procs: &[ProcInfo],
    worktrees: &[(WorkspaceId, &Path)],
) -> HashMap<WorkspaceId, Vec<ProcInfo>> {
    let mut out: HashMap<WorkspaceId, Vec<ProcInfo>> = HashMap::new();
    for p in procs {
        if PROC_DENYLIST.contains(&p.command.as_str()) {
            continue;
        }
        for (id, wt) in worktrees {
            if p.cwd.starts_with(wt) {
                out.entry(*id).or_default().push(p.clone());
                break;
            }
        }
    }
    out
}

/// Run `lsof -d cwd -F pcn` and return the parsed process list.
/// Returns an empty list (not an error) when `lsof` is missing or
/// fails, so the rest of the dashboard keeps working.
pub async fn scan() -> Vec<ProcInfo> {
    let output = tokio::process::Command::new("lsof")
        .args(["-d", "cwd", "-F", "pcn"])
        .output()
        .await;
    match output {
        Ok(o) if o.status.success() || !o.stdout.is_empty() => {
            // lsof exits 1 when some processes can't be inspected; the
            // stdout it does produce is still valid. Only treat fully
            // empty + nonzero as "missing/broken."
            parse_lsof_output(&String::from_utf8_lossy(&o.stdout))
        }
        _ => Vec::new(),
    }
}

/// Send a signal to a process. `signal` is the `kill -<signal>` arg
/// ("TERM" or "KILL"). Silently swallows ESRCH (process already gone).
pub async fn kill_pid(pid: i32, signal: &str) -> Result<()> {
    let status = tokio::process::Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .status()
        .await
        .map_err(|e| Error::UserInput(format!("spawn kill: {e}")))?;
    // kill exit code 1 with "No such process" is fine — process is gone.
    let _ = status;
    Ok(())
}
```

- [ ] **Step 4: Run tests; expect pass**

```
cargo test --lib proc:: 2>&1 | tail -10
```
Expected: 5 tests passing.

- [ ] **Step 5: Run fmt + clippy**

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```
Both must pass.

- [ ] **Step 6: Commit**

```
git add src/proc.rs src/lib.rs
git commit -m "feat(proc): lsof-based process detection scaffolding

Adds the proc module with ProcInfo, scan, parse_lsof_output,
bucket_by_worktree, and kill_pid. Pure functions are unit-tested
against fixture lsof output. The async scan and kill shell out
to lsof / kill respectively — no new crate deps."
```

---

### Task 2: `App` state + `branch_drift_poll` integration

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add `App` fields**

In the `App` struct (search for `pub workspace_needs_attention`):

```rust
/// Processes detected per workspace (cwd inside the workspace's
/// worktree). Refreshed every ~10s by branch_drift_poll.
pub workspace_processes: std::collections::HashMap<crate::store::WorkspaceId, Vec<crate::proc::ProcInfo>>,
/// Epoch-ms of last completed `proc::scan` — throttle source.
pub last_proc_scan_ms: i64,
```

Initialize both in `App::new`:
```rust
workspace_processes: std::collections::HashMap::new(),
last_proc_scan_ms: 0,
```

- [ ] **Step 2: Wire scan into `branch_drift_poll`**

Find `branch_drift_poll` (search `pub async fn branch_drift_poll`). After the for-loop that handles per-workspace branch drift / status / PR / events ends (right before the final `}` of the outer `loop`), insert:

```rust
        // 5) Per-workspace process scan. Throttled to once per 10 s globally —
        //    lsof returns everything in a single call, so we don't pay per-workspace.
        let should_scan = {
            let g = app.lock().await;
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            now_ms.saturating_sub(g.last_proc_scan_ms) >= 10_000
        };
        if should_scan {
            let procs = crate::proc::scan().await;
            let worktrees: Vec<(crate::store::WorkspaceId, std::path::PathBuf)> = {
                let g = app.lock().await;
                g.workspaces
                    .iter()
                    .map(|(_, w)| (w.id, w.worktree_path.clone()))
                    .collect()
            };
            let worktree_refs: Vec<(crate::store::WorkspaceId, &std::path::Path)> = worktrees
                .iter()
                .map(|(id, path)| (*id, path.as_path()))
                .collect();
            let bucketed = crate::proc::bucket_by_worktree(&procs, &worktree_refs);
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let mut g = app.lock().await;
            g.workspace_processes = bucketed;
            g.last_proc_scan_ms = now_ms;
        }
```

- [ ] **Step 3: Build + run a representative test**

```
cargo build --message-format=short 2>&1 | tail -8
cargo test --lib -- --test-threads=1 2>&1 | tail -5
```
Expected: build clean, all existing tests pass (no new test added in this task — the state plumbing is exercised by Task 5's keybind tests).

- [ ] **Step 4: Commit**

```
git add src/app.rs
git commit -m "feat(app): scan + cache per-workspace processes every 10s

Adds workspace_processes + last_proc_scan_ms to App state and
wires proc::scan / proc::bucket_by_worktree into branch_drift_poll
with a 10-second throttle. No UI yet — Task 3 surfaces the count
on the dashboard."
```

---

### Task 3: Dashboard inline `~N` annotation

**Files:**
- Modify: `src/ui/dashboard.rs`

- [ ] **Step 1: Find the workspace render call site**

Open `src/ui/dashboard.rs` and search for `workspace_main_row(`. The call site (around line 130) currently passes `workspace`, `session_running`, `seconds_since_activity`, etc. We're adding one more argument: `proc_count: usize`.

- [ ] **Step 2: Write a failing test**

Add to the `#[cfg(test)] mod tests` block in `src/ui/dashboard.rs`:

```rust
#[test]
fn workspace_row_shows_proc_count_when_nonzero() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let w = workspace(1, 1, "ws", "wsx/ws");
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: false,
            seconds_since_activity: None,
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped: false,
            proc_count: 3,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let text = dump(&term, 120, 8);
    let row = text.lines().find(|l| l.contains("ws")).expect("row");
    assert!(row.contains("~3"), "expected `~3` proc count in row: {row}");
}

#[test]
fn workspace_row_hides_proc_count_when_zero() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let w = workspace(1, 1, "quiet", "wsx/quiet");
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: false,
            seconds_since_activity: None,
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let text = dump(&term, 120, 8);
    let row = text.lines().find(|l| l.contains("quiet")).expect("row");
    assert!(!row.contains("~"), "did not expect `~` count when proc_count=0: {row}");
}
```

- [ ] **Step 3: Run to confirm failures**

```
cargo test --lib dashboard:: 2>&1 | tail -15
```
Expected: compile errors — `Item::Workspace` doesn't have a `proc_count` field.

- [ ] **Step 4: Add `proc_count` to `Item::Workspace`**

Find the `Item` enum at the top of `src/ui/dashboard.rs`. Add a `proc_count: usize` field to the `Workspace` variant. Then update all five callers in `src/app.rs` (search for `dashboard::Item::Workspace`) — pass `proc_count: app.workspace_processes.get(&ws.id).map(|v| v.len()).unwrap_or(0)`.

There's one production call site (in `draw()`); the other callers are test fixtures inside `src/ui/dashboard.rs` itself. Update the in-file test fixtures to pass `proc_count: 0` so the existing tests still compile.

- [ ] **Step 5: Wire the count into `workspace_main_row`**

Update `workspace_main_row`'s signature to accept `proc_count: usize`. Inside the function, after the branch column span is pushed but before the gap calculation, insert:

```rust
    if proc_count > 0 {
        spans.push(Span::styled(
            format!(" ~{proc_count}"),
            theme.merged_style(),
        ));
    }
```

(The `merged_style` is cyan — distinct from activity colors.)

Then update the call site (around line 130 in the same file) to pass `proc_count` through.

- [ ] **Step 6: Run tests; expect pass**

```
cargo fmt
cargo test --lib dashboard:: 2>&1 | tail -10
```
Expected: both new tests pass; existing dashboard tests still pass.

- [ ] **Step 7: Run clippy + full test suite**

```
cargo clippy --all-targets -- -D warnings 2>&1 | tail -3
cargo test --lib -- --test-threads=1 2>&1 | tail -5
```
Both must be green.

- [ ] **Step 8: Commit**

```
git add src/ui/dashboard.rs src/app.rs
git commit -m "feat(ui): inline ~N process count on dashboard rows

Workspaces with detected processes (count > 0) show a small
\`~N\` annotation in merged-style between the branch column and
the activity word. Zero-count rows are unchanged."
```

---

### Task 4: `Modal::ProcessList` variant + renderer

**Files:**
- Modify: `src/ui/modal.rs`
- Modify: `src/app.rs` (for the `draw` dispatch)

- [ ] **Step 1: Add the variant**

In `src/ui/modal.rs`, extend the `Modal` enum:

```rust
ProcessList {
    workspace_id: crate::store::WorkspaceId,
    selected: usize,
},
```

- [ ] **Step 2: Early-return in `render()`**

The existing `pub fn render(...)` already early-returns for `UpdatesPanel` because that variant needs live App state. Add the same guard for `ProcessList`:

```rust
if matches!(modal, Modal::UpdatesPanel { .. } | Modal::ProcessList { .. }) {
    return;
}
```

And update the exhaustive match arms inside `render()` to include `Modal::ProcessList { .. } => unreachable!(...)` for completeness.

- [ ] **Step 3: Add `render_process_list` function**

Below `render_updates_panel`, add:

```rust
/// Render the floating process-list modal. Reads live App state via
/// borrowed slices so the modal updates on every render tick.
pub fn render_process_list(
    f: &mut Frame,
    area: Rect,
    workspace_name: &str,
    procs: &[crate::proc::ProcInfo],
    selected: usize,
    theme: &Theme,
) {
    let w = area.width.clamp(20, 80);
    let h = area.height.clamp(8, 25);
    let rect = centered(area, w, h);
    f.render_widget(Clear, rect);

    let title = format!(" Processes — {workspace_name} ");
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .style(theme.dim_style());
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let body_area = chunks[0];
    let footer_area = chunks[1];

    if procs.is_empty() {
        f.render_widget(
            Paragraph::new("(no tracked processes)").style(theme.dim_style()),
            body_area,
        );
    } else {
        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(Span::styled(
            format!("  {:<7} {:<20} {}", "PID", "COMMAND", "CWD"),
            theme.header_style(),
        )));
        for (i, p) in procs.iter().enumerate() {
            let body = format!(
                "  {:<7} {:<20} {}",
                p.pid,
                truncate(&p.command, 20),
                p.cwd.display()
            );
            if i == selected {
                lines.push(Line::from(Span::styled(body, theme.selected_style())));
            } else {
                lines.push(Line::from(body));
            }
        }
        f.render_widget(Paragraph::new(lines), body_area);
    }
    f.render_widget(
        Paragraph::new("[\u{2191}/\u{2193}] move   [k] term   [K] kill   [esc] close")
            .style(theme.dim_style()),
        footer_area,
    );
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('\u{2026}');
        out
    }
}
```

- [ ] **Step 4: Dispatch from `draw()` in `app.rs`**

Find the section in `src/app.rs::draw` that calls `render_updates_panel`. Add a sibling dispatch for `ProcessList`:

```rust
} else if let Some(crate::ui::modal::Modal::ProcessList {
    workspace_id,
    selected,
}) = &app.modal
{
    let workspace_name = app
        .workspaces
        .iter()
        .find(|(_, w)| w.id == *workspace_id)
        .map(|(_, w)| w.name.clone())
        .unwrap_or_default();
    let procs = app
        .workspace_processes
        .get(workspace_id)
        .cloned()
        .unwrap_or_default();
    crate::ui::modal::render_process_list(
        f, area, &workspace_name, &procs, *selected, &app.theme,
    );
}
```

The exact placement: locate the existing `Modal::UpdatesPanel` dispatch (search for `render_updates_panel`) and add the new branch right after it.

- [ ] **Step 5: Verify build**

```
cargo build --message-format=short 2>&1 | tail -8
```
Expected: clean.

- [ ] **Step 6: Commit**

```
git add src/ui/modal.rs src/app.rs
git commit -m "feat(ui): Modal::ProcessList variant + renderer

Adds the variant and dedicated render_process_list following the
UpdatesPanel pattern (early-return in render(), borrowed-slice
dispatch from draw()). Empty-state shows '(no tracked processes)'.
Modal isn't reachable yet — Task 5 wires the keybinds."
```

---

### Task 5: Keybinds — `k` on dashboard + `Ctrl-x k` attached + modal handler

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add `k` arm in `handle_key_dashboard`**

After the existing `(KeyCode::Char('v'), _)` arm, insert:

```rust
        (KeyCode::Char('k'), _) => {
            if let Some(SelectionTarget::Workspace(id)) = app.selected_target() {
                app.modal = Some(Modal::ProcessList {
                    workspace_id: id,
                    selected: 0,
                });
            }
            // 'k' on a Repo header is intentionally a no-op.
        }
```

- [ ] **Step 2: Add `k` arm in the `leader_pending` block of `handle_key_attached`**

Inside `handle_key_attached`'s `if app.leader_pending` block, alongside `e`/`t`/`v`, insert:

```rust
            KeyCode::Char('k') => {
                app.modal = Some(Modal::ProcessList {
                    workspace_id: id,
                    selected: 0,
                });
                return Ok(());
            }
```

- [ ] **Step 3: Handle modal keys in `handle_key_modal`**

In `handle_key_modal`, after the existing `Modal::UpdatesPanel` arm (search for `Modal::UpdatesPanel`), add a `Modal::ProcessList` arm:

```rust
        Modal::ProcessList {
            workspace_id,
            mut selected,
        } => {
            let procs = app
                .workspace_processes
                .get(&workspace_id)
                .cloned()
                .unwrap_or_default();
            match k.code {
                KeyCode::Esc => {
                    app.modal = None;
                }
                KeyCode::Up => {
                    selected = selected.saturating_sub(1);
                    app.modal = Some(Modal::ProcessList {
                        workspace_id,
                        selected,
                    });
                }
                KeyCode::Down => {
                    if !procs.is_empty() {
                        selected = (selected + 1).min(procs.len() - 1);
                    }
                    app.modal = Some(Modal::ProcessList {
                        workspace_id,
                        selected,
                    });
                }
                KeyCode::Char('k') => {
                    if let Some(p) = procs.get(selected) {
                        let _ = crate::proc::kill_pid(p.pid, "TERM").await;
                        rescan_processes(app).await;
                    }
                }
                KeyCode::Char('K') => {
                    if let Some(p) = procs.get(selected) {
                        let _ = crate::proc::kill_pid(p.pid, "KILL").await;
                        rescan_processes(app).await;
                    }
                }
                _ => {}
            }
        }
```

- [ ] **Step 4: Add the `rescan_processes` helper**

Near `build_spawn_info` (or anywhere in `src/app.rs` that fits), add a small helper:

```rust
/// Immediately re-run `proc::scan` and re-bucket. Used after a kill
/// so the modal reflects the new state without waiting for the
/// next 10s poll tick.
async fn rescan_processes(app: &mut App) {
    let procs = crate::proc::scan().await;
    let worktrees: Vec<(crate::store::WorkspaceId, std::path::PathBuf)> = app
        .workspaces
        .iter()
        .map(|(_, w)| (w.id, w.worktree_path.clone()))
        .collect();
    let worktree_refs: Vec<(crate::store::WorkspaceId, &std::path::Path)> = worktrees
        .iter()
        .map(|(id, path)| (*id, path.as_path()))
        .collect();
    app.workspace_processes = crate::proc::bucket_by_worktree(&procs, &worktree_refs);
    app.last_proc_scan_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    // Clamp the modal's `selected` index after the list size changes.
    if let Some(Modal::ProcessList {
        workspace_id,
        selected,
    }) = &mut app.modal
    {
        let len = app
            .workspace_processes
            .get(workspace_id)
            .map(|v| v.len())
            .unwrap_or(0);
        if len == 0 {
            *selected = 0;
        } else {
            *selected = (*selected).min(len - 1);
        }
    }
}
```

(Note the borrow gymnastics — the `if let` reborrow of `app.modal` needs care; alternative: snapshot the workspace_id, drop the borrow, then mutate. Adjust if rustc complains.)

- [ ] **Step 5: Build + run tests**

```
cargo fmt
cargo build --message-format=short 2>&1 | tail -8
cargo test --lib -- --test-threads=1 2>&1 | tail -5
```
Expected: clean. All 229+ tests pass.

- [ ] **Step 6: Commit**

```
git add src/app.rs
git commit -m "feat(ui): k opens process modal; k/K inside kill TERM/KILL

Dashboard 'k' on a selected workspace opens Modal::ProcessList.
From attached view, Ctrl-x k does the same. Inside the modal:
arrow keys navigate, 'k' sends SIGTERM, 'K' sends SIGKILL,
'esc' closes. Each kill immediately re-runs proc::scan so the
list reflects the new state before the next 10s poll tick."
```

---

### Task 6: Footers + README + verify + commit spec/plan + push

**Files:**
- Modify: `src/ui/dashboard.rs` (footer string)
- Modify: `src/ui/attached.rs` (compact footer)
- Modify: `README.md`
- New (commit only): the spec and plan docs

- [ ] **Step 1: Dashboard footer**

Find the dashboard footer Paragraph (currently includes `[v] diff   [d] archive`). Insert `[k] procs` between `[v] diff` and `[d] archive`:

```rust
"[↑/↓] move   [enter] attach   [n] new   [N] new (YOLO)   [e] edit   [t] terminal   [v] diff   [k] procs   [d] archive   [q] quit"
```

- [ ] **Step 2: Attached view footer**

Find `src/ui/attached.rs:48`:

```rust
format!(" {label}   [Ctrl-x] d=detach u=updates e=edit t=term v=diff x=send-Ctrl-x ");
```

Insert `k=procs` between `v=diff` and `x=send-Ctrl-x`:

```rust
format!(" {label}   [Ctrl-x] d=detach u=updates e=edit t=term v=diff k=procs x=send-Ctrl-x ");
```

- [ ] **Step 3: README — Dashboard keybinds table**

In the Dashboard keybinds table (search for `| `t` | Open the selected workspace in a terminal`), add after the `[v]` row:

```
| `k` | Show processes running under the selected workspace's worktree (no-op on repo header) |
```

- [ ] **Step 4: README — Attached workspace keybinds table**

Add a row to the attached-view keybinds table after the `Ctrl-x v` row:

```
| `Ctrl-x k` | Show processes running under the attached workspace's worktree |
```

- [ ] **Step 5: README — new "Process tracking" section**

After the "Remote access" section (or wherever fits the flow), add:

```markdown
## Process tracking

`[k]` on the dashboard (or `Ctrl-x k` while attached) shows long-running
processes whose current working directory is inside the selected
workspace's worktree — dev servers, watchers, anything you started in
that worktree from a terminal. Workspaces with detected processes show
a `~N` count between the branch and activity columns on the dashboard.

The modal lists each process's PID, command, and full cwd:

```
─── Processes — fix-bug ──────
  PID    COMMAND          CWD
  12345  npm              /home/user/wt/fix-bug
  12389  pytest           /home/user/wt/fix-bug/tests
─────────────────────────────
[↑/↓] move   [k] term   [K] kill   [esc] close
```

`k` sends `SIGTERM` to the highlighted process; `K` sends `SIGKILL`.
After either, wsx immediately re-scans so the list reflects the new
state.

**Notes:**

- Detection runs once every 10 seconds in the background via `lsof -d cwd`.
- Shells and editors (bash, zsh, nvim, code, etc.) are filtered out so the
  count surfaces what's interesting — your dev server, not the terminal
  hosting it.
- wsx never starts these processes itself. Launch them however you
  like (the `[t]` terminal keybind is one option). The feature is
  observability plus a kill hook, not lifecycle management.
- Requires `lsof` to be installed (standard on most Linux/macOS setups).
  If it's missing, the count stays at 0 and the modal shows "(no tracked
  processes)" — no errors.
```

- [ ] **Step 6: Verify**

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --lib -- --test-threads=1
```
All three must pass.

- [ ] **Step 7: Commit README**

```
git add src/ui/dashboard.rs src/ui/attached.rs README.md
git commit -m "$(cat <<'EOF'
docs: process tracking + footer hints

Adds the Process tracking section to the README, plus the new
[k] / Ctrl-x k entries in the dashboard and attached-view footers
and keybinds tables.

Closes #21.
EOF
)"
```

- [ ] **Step 8: Commit the spec and plan**

```
git add docs/superpowers/specs/2026-05-15-process-tracking-design.md docs/superpowers/plans/2026-05-15-process-tracking.md
git commit -m "docs: spec + plan for process tracking feature"
```

- [ ] **Step 9: Push and confirm issue closes**

```
git push origin main
sleep 3
gh issue view 21 --json state,closedAt,stateReason
```
Expected: `"state":"CLOSED"` and `"stateReason":"COMPLETED"` thanks to the `Closes #21` trailer.

---

## Self-review checklist

- [x] `lsof` output parser handles malformed blocks (missing `p`/`c`/`n`) gracefully
- [x] `bucket_by_worktree` applies both filters (deny-list AND cwd-descendant match)
- [x] `scan` returns empty (not error) when `lsof` is missing, so the dashboard keeps working
- [x] `kill_pid` swallows ESRCH (already-exited process)
- [x] App state has the `workspace_processes` cache + the 10s throttle stamp
- [x] `branch_drift_poll` runs ONE global lsof per cycle (not per-workspace)
- [x] Dashboard `~N` only renders when N>0; styled `merged_style` to differentiate
- [x] `Modal::ProcessList` follows the `UpdatesPanel` dispatch pattern (early-return in `render`, live-state dispatch in `draw`)
- [x] Modal keys: Up/Down navigate; `k`/`K` kill; Esc closes
- [x] After a kill, `rescan_processes` re-runs `proc::scan` synchronously so the modal updates immediately
- [x] After re-scan, `selected` is clamped to the new list length (no out-of-bounds)
- [x] Dashboard footer + attached-view footer both updated
- [x] README has both a keybinds-table entry AND a dedicated "Process tracking" section
- [x] No placeholders, no TBDs
- [x] No backward-compat hacks (direct to main, no migration concerns)
