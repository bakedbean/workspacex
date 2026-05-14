# Project Manager Pane Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an opt-in horizontal pane below the dashboard that hosts a Claude Code "project manager" session, which inspects active workspaces (via a wsx-written `workspaces.json` plus read-only filesystem tools) and reports what each was created for, where it left off, and what's next.

**Architecture:** New `src/pm.rs` owns the dossier file and PM-spawn glue. `SpawnMode` gains a `ProjectManager` variant routed through the same `build_claude_command` + PTY plumbing used for workspace sessions. `SessionManager` tracks PM as a single optional session (separate from the workspace-keyed map). `App` gains `pm`, `pm_visible`, and `focus` fields; the dashboard view splits 60/40 when visible. Auto-summary is delivered by writing to the PM PTY after observing the activity stream settle (no new bytes for 400ms after some output).

**Tech Stack:** Rust 2024, ratatui 0.29, crossterm 0.28, portable-pty 0.9, vt100 0.15, tokio (multi-thread), serde_json (already a dep), rusqlite 0.32 (already), shlex (already).

**Spec:** `docs/superpowers/specs/2026-05-14-project-manager-pane-design.md`.

**Spec deviations (implementation-level YAGNI):**
- `workspaces.json` uses `generated_at_epoch_seconds: i64` instead of an RFC 3339 string — avoids adding a `chrono`/`time` dependency. PM can still see freshness.
- `state` field dropped from each workspace entry. We filter to `WorkspaceState::Ready` only (the actual "live" state; `Pending`/`Failed`/`Orphaned` are excluded). Presence in the JSON ⇒ Ready.

---

## File Structure

**Create:**
- `src/pm.rs` — PM module: workspaces.json schema + writer, dir initialization, system prompt + allowedTools constants, message constants, orchestrator helpers (`open_pm`, `refresh_pm`).
- `src/ui/pm_pane.rs` — Renders the PM PTY into a `Rect` with focus-aware title/footer.

**Modify:**
- `src/pty/session.rs` — Add `SpawnMode::ProjectManager`, extend `build_claude_command`, add `Session::send_text_when_settled`, extend `SessionManager` with PM accessors and have `kill_all` cover PM.
- `src/events.rs` — Make `encode_cwd` `pub` so `pm.rs` can compute `session_log_dir` paths.
- `src/ui/mod.rs` — Add `PaneFocus` enum.
- `src/app.rs` — Add `pm`, `pm_visible`, `focus` fields; integrate split layout into `draw`; route `p`/`Tab`/`Esc`/`r` in `handle_key_dashboard`; forward keystrokes to PM when focused.
- `src/cli.rs` — Add `pm_enabled` and `pm_custom_instructions` to `known_setting_key`.
- `README.md` — New "Project manager pane" section; two new settings rows; one new storage row.

---

## Task 1: `workspaces.json` writer

**Files:**
- Create: `src/pm.rs`
- Modify: `src/lib.rs:1` (add `pub mod pm;`)
- Modify: `src/events.rs:121-125` (make `encode_cwd` `pub`)

Builds the dossier writer in isolation. No PM spawn, no UI — just "given a `Store` and a path, produce the file."

- [ ] **Step 1: Add module declaration**

Edit `src/lib.rs`. Find the existing `pub mod` lines (top of file). Add:

```rust
pub mod pm;
```

- [ ] **Step 2: Make `encode_cwd` public**

In `src/events.rs`, change the signature at line 123 from:

```rust
fn encode_cwd(path: &Path) -> String {
```

to:

```rust
pub fn encode_cwd(path: &Path) -> String {
```

- [ ] **Step 3: Create skeleton `src/pm.rs`**

```rust
//! Project Manager pane: dossier file + PM Claude Code session orchestration.

use crate::error::{Error, Result};
use crate::store::{Store, WorkspaceState};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize)]
struct WorkspacesDossier {
    generated_at_epoch_seconds: i64,
    repos: Vec<RepoEntry>,
}

#[derive(Debug, Serialize)]
struct RepoEntry {
    name: String,
    path: PathBuf,
    workspaces: Vec<WorkspaceEntry>,
}

#[derive(Debug, Serialize)]
struct WorkspaceEntry {
    name: String,
    branch: String,
    worktree_path: PathBuf,
    session_log_dir: PathBuf,
    git: GitCounts,
}

#[derive(Debug, Serialize, Default)]
struct GitCounts {
    modified: usize,
    untracked: usize,
    ahead: usize,
    behind: usize,
}

/// Write the workspaces dossier file atomically (tempfile + rename).
///
/// Only `WorkspaceState::Ready` workspaces are included. Repos with no
/// Ready workspaces appear with an empty `workspaces` array.
pub fn write_workspaces_json(store: &Store, target: &Path) -> Result<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let mut repos = Vec::new();
    for repo in store.repos()? {
        let mut workspaces = Vec::new();
        for ws in store.workspaces(repo.id)? {
            if ws.state != WorkspaceState::Ready {
                continue;
            }
            let session_log_dir = compute_session_log_dir(&ws.worktree_path);
            workspaces.push(WorkspaceEntry {
                name: ws.name,
                branch: ws.branch,
                worktree_path: ws.worktree_path,
                session_log_dir,
                git: GitCounts::default(),
            });
        }
        repos.push(RepoEntry {
            name: repo.name,
            path: repo.path,
            workspaces,
        });
    }

    let dossier = WorkspacesDossier {
        generated_at_epoch_seconds: now,
        repos,
    };

    let payload = serde_json::to_string_pretty(&dossier)
        .map_err(|e| Error::Io(std::io::Error::other(format!("workspaces.json serialize: {e}"))))?;

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = target.with_extension("json.tmp");
    std::fs::write(&tmp, payload)?;
    std::fs::rename(&tmp, target)?;
    Ok(())
}

fn compute_session_log_dir(worktree: &Path) -> PathBuf {
    let abs = std::fs::canonicalize(worktree).unwrap_or_else(|_| worktree.to_path_buf());
    let encoded = crate::events::encode_cwd(&abs);
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    home.join(".claude/projects").join(encoded)
}
```

- [ ] **Step 4: Write the failing test**

Append to `src/pm.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{NewWorkspace, Store, WorkspaceState};
    use tempfile::TempDir;

    #[test]
    fn workspaces_json_includes_only_ready_filters_failed_and_pending() {
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(Path::new("/tmp/fake-repo"), "demo", "")
            .unwrap();
        let ws_ready = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "ready-one",
                branch: "demo/ready-one",
                worktree_path: Path::new("/tmp/wsx-wt/ready-one"),
            })
            .unwrap();
        store
            .set_workspace_state(ws_ready, WorkspaceState::Ready)
            .unwrap();
        let ws_failed = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "broken",
                branch: "demo/broken",
                worktree_path: Path::new("/tmp/wsx-wt/broken"),
            })
            .unwrap();
        store
            .set_workspace_state(ws_failed, WorkspaceState::Failed)
            .unwrap();

        let dir = TempDir::new().unwrap();
        let target = dir.path().join("workspaces.json");
        write_workspaces_json(&store, &target).unwrap();
        let text = std::fs::read_to_string(&target).unwrap();
        assert!(text.contains("\"name\": \"ready-one\""), "{text}");
        assert!(!text.contains("\"name\": \"broken\""), "{text}");
        assert!(text.contains("\"generated_at_epoch_seconds\""), "{text}");
        assert!(text.contains("\"workspaces\": ["), "{text}");
    }

    #[test]
    fn workspaces_json_empty_repo_shows_empty_array() {
        let store = Store::open_in_memory().unwrap();
        store
            .add_repo(Path::new("/tmp/empty-repo"), "empty", "")
            .unwrap();
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("workspaces.json");
        write_workspaces_json(&store, &target).unwrap();
        let text = std::fs::read_to_string(&target).unwrap();
        assert!(text.contains("\"name\": \"empty\""), "{text}");
        assert!(text.contains("\"workspaces\": []"), "{text}");
    }
}
```

- [ ] **Step 5: Run tests, verify pass**

Run: `cargo test --lib pm:: -- --test-threads=1`
Expected: 2 passed, 0 failed.

- [ ] **Step 6: Commit**

```bash
git add src/pm.rs src/lib.rs src/events.rs
git commit -m "feat(pm): workspaces.json dossier writer"
```

---

## Task 2: PM directory initialization

**Files:**
- Modify: `src/pm.rs` (append `init_pm_dir` + tests)
- Modify: `src/config.rs` (add `pm_dir()` to `Dirs`)

PM has a fixed cwd under the state dir, initialized as a git repo on first use.

- [ ] **Step 1: Add `pm_dir` to `Dirs`**

In `src/config.rs`, after the existing `log_dir` method (line 33), add:

```rust
    pub fn pm_dir(&self) -> PathBuf {
        self.app_dir().join("project-manager")
    }
```

- [ ] **Step 2: Write the failing test for `init_pm_dir`**

Append to the `tests` module in `src/pm.rs`:

```rust
    #[test]
    fn init_pm_dir_creates_dir_and_git_init() {
        let dir = TempDir::new().unwrap();
        let pm_root = dir.path().join("pm");
        init_pm_dir(&pm_root).unwrap();
        assert!(pm_root.is_dir());
        assert!(pm_root.join(".git").is_dir(), "expected git repo init");
        // Idempotent: second call should not error.
        init_pm_dir(&pm_root).unwrap();
    }
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --lib pm::tests::init_pm_dir_creates_dir_and_git_init -- --test-threads=1`
Expected: FAIL — `init_pm_dir` does not exist.

- [ ] **Step 4: Implement `init_pm_dir`**

Append to `src/pm.rs` (before the `#[cfg(test)]` line):

```rust
/// Initialize the PM working directory. Creates `dir` if needed and
/// runs `git init` inside it so Claude Code is happy. Idempotent — if
/// the directory already contains a git repo, this is a no-op.
pub fn init_pm_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    if dir.join(".git").is_dir() {
        return Ok(());
    }
    let status = std::process::Command::new("git")
        .arg("init")
        .arg("-q")
        .current_dir(dir)
        .status()
        .map_err(|e| Error::Io(std::io::Error::other(format!("git init pm dir: {e}"))))?;
    if !status.success() {
        return Err(Error::Git(format!(
            "git init failed in {} (exit {:?})",
            dir.display(),
            status.code()
        )));
    }
    Ok(())
}
```

- [ ] **Step 5: Run tests, verify pass**

Run: `cargo test --lib pm:: -- --test-threads=1`
Expected: 3 passed, 0 failed.

- [ ] **Step 6: Commit**

```bash
git add src/pm.rs src/config.rs
git commit -m "feat(pm): init_pm_dir creates pm cwd with git init"
```

---

## Task 3: PM system prompt + allowedTools

**Files:**
- Modify: `src/pm.rs` (append constants + helper)

The strings PM needs at spawn time. Pure functions, fully testable.

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `src/pm.rs`:

```rust
    #[test]
    fn system_prompt_contains_distinctive_phrases() {
        let p = pm_system_prompt(None);
        assert!(p.contains("project manager"), "{p}");
        assert!(p.contains("./workspaces.json"), "{p}");
        assert!(p.contains("session_log_dir"), "{p}");
    }

    #[test]
    fn system_prompt_appends_custom_instructions() {
        let p = pm_system_prompt(Some("Be extra terse."));
        assert!(p.contains("project manager"), "{p}");
        assert!(p.ends_with("Be extra terse."), "{p}");
        assert!(p.contains("\n\nBe extra terse."), "{p}");
    }

    #[test]
    fn allowed_tools_is_read_only_set() {
        let tools = pm_allowed_tools();
        assert!(tools.contains("Read"));
        assert!(tools.contains("Bash(git status:*)"));
        assert!(tools.contains("Bash(git log:*)"));
        assert!(tools.contains("Bash(git diff:*)"));
        assert!(tools.contains("Bash(cat:*)"));
        assert!(tools.contains("Bash(ls:*)"));
        assert!(!tools.contains("Write"));
        assert!(!tools.contains("Edit"));
        // Catch any inadvertent broad bash variant.
        assert!(!tools.split(',').any(|t| t.trim() == "Bash(*)"), "{tools}");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib pm:: -- --test-threads=1`
Expected: 3 fails — symbols undefined.

- [ ] **Step 3: Implement the constants and helper**

Append to `src/pm.rs` (before the `#[cfg(test)]` line):

```rust
const PM_SYSTEM_PROMPT: &str = "\
You are a project manager for a developer running multiple parallel coding \
workspaces under wsx. Each workspace is a git worktree with its own Claude \
Code session. Your job: when asked, inspect their active workspaces and \
report (1) what each was created for, (2) where it left off, (3) what's \
next to close it out.\n\
\n\
Where to find information:\n\
  - ./workspaces.json lists all active workspaces with: name, branch,\n\
    worktree_path, session_log_dir, git counts.\n\
  - For the original prompt: read the FIRST user message in the earliest\n\
    *.jsonl under session_log_dir.\n\
  - For recent activity: read the LAST several entries in the most recent\n\
    *.jsonl under session_log_dir.\n\
  - For code state: cd to worktree_path; use git status / log / diff.\n\
\n\
Constraints:\n\
  - Read-only. You cannot modify workspaces.\n\
  - Be concise — the developer is glancing at a small pane. Default to a\n\
    per-workspace block:\n\
        <name>: <one-line status>\n\
          - Created for: <one-line>\n\
          - Last activity: <one-line>\n\
          - Next: <one-line>\n\
  - If you're uncertain about \"next\", say so; don't fabricate.\n\
  - workspaces.json refreshes when the developer asks. Trust its contents\n\
    over stale memory.";

/// Build the PM system prompt, optionally appending custom instructions.
pub fn pm_system_prompt(custom: Option<&str>) -> String {
    match custom.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        None => PM_SYSTEM_PROMPT.to_string(),
        Some(extra) => format!("{PM_SYSTEM_PROMPT}\n\n{extra}"),
    }
}

/// Comma-separated read-only tool allowlist for the PM session.
pub fn pm_allowed_tools() -> &'static str {
    "Read,Bash(git status:*),Bash(git log:*),Bash(git diff:*),Bash(git branch:*),Bash(cat:*),Bash(ls:*)"
}

/// The initial user message wsx sends to PM after a Fresh spawn.
pub const PM_AUTO_SUMMARY_MESSAGE: &str =
    "Give me a status summary of all active workspaces per your instructions.";

/// The user message wsx sends to PM on `r` refresh.
pub const PM_REFRESH_MESSAGE: &str =
    "Refresh: workspaces.json has been updated. Re-summarize the current state of all workspaces.";
```

- [ ] **Step 4: Run tests, verify pass**

Run: `cargo test --lib pm:: -- --test-threads=1`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add src/pm.rs
git commit -m "feat(pm): system prompt, allowed tools, auto-message constants"
```

---

## Task 4: `SpawnMode::ProjectManager` variant

**Files:**
- Modify: `src/pty/session.rs:134-142` (extend `SpawnMode`)
- Modify: `src/pty/session.rs:153-211` (extend `build_claude_command`)

Wire PM into the existing claude command builder so it picks up the system prompt, allowedTools, and `--continue` flag in a consistent way.

- [ ] **Step 1: Write the failing test**

In `src/pty/session.rs`, find the existing `#[cfg(test)] mod tests` (around line 374). Add this test alongside the existing ones:

```rust
    #[test]
    fn project_manager_mode_adds_allowed_tools_and_system_prompt() {
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/x/workspaces.json"),
            custom_instructions: None,
            resume: false,
        };
        let cmd = build_claude_command(&cwd, &mode);
        // CommandBuilder doesn't expose args directly; build the OS command
        // and inspect via the std debug form.
        let dbg = format!("{cmd:?}");
        assert!(dbg.contains("--allowedTools"), "{dbg}");
        assert!(dbg.contains("Read"), "{dbg}");
        assert!(dbg.contains("Bash(git status:*)"), "{dbg}");
        assert!(dbg.contains("--append-system-prompt"), "{dbg}");
        assert!(dbg.contains("project manager"), "{dbg}");
        assert!(!dbg.contains("--continue"), "should be Fresh-style: {dbg}");
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[test]
    fn project_manager_mode_resume_adds_continue() {
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/x/workspaces.json"),
            custom_instructions: None,
            resume: true,
        };
        let cmd = build_claude_command(&cwd, &mode);
        let dbg = format!("{cmd:?}");
        assert!(dbg.contains("--continue"), "{dbg}");
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib pty::session::tests::project_manager_mode -- --test-threads=1`
Expected: FAIL — `SpawnMode::ProjectManager` variant doesn't exist.

- [ ] **Step 3: Extend `SpawnMode`**

In `src/pty/session.rs`, change the `SpawnMode` enum (line 134):

```rust
#[derive(Debug, Clone)]
pub enum SpawnMode {
    /// Brand-new session. Apply rename system prompt if context provided.
    Fresh {
        rename_ctx: Option<RenameContext>,
        custom_instructions: Option<String>,
    },
    /// Resume the most recent prior session in this worktree via `--continue`.
    Continue { custom_instructions: Option<String> },
    /// Spawn the project-manager session. Embeds the PM system prompt and
    /// a read-only tool allowlist. When `resume` is true, also passes
    /// `--continue` to pick up PM's prior conversation.
    ProjectManager {
        workspaces_json_path: std::path::PathBuf,
        custom_instructions: Option<String>,
        resume: bool,
    },
}
```

- [ ] **Step 4: Extend `build_claude_command`**

Replace the body of `build_claude_command` (lines 153-211) with:

```rust
pub fn build_claude_command(cwd: &Path, mode: &SpawnMode) -> CommandBuilder {
    let bin = std::env::var("WSX_CLAUDE_BIN").unwrap_or_else(|_| "claude".to_string());
    let mut cmd = CommandBuilder::new(bin);
    cmd.cwd(cwd);
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }

    let (rename_prompt, custom, allow_git_branch, allow_tools_override, add_continue) = match mode {
        SpawnMode::Continue {
            custom_instructions,
        } => (None, custom_instructions.clone(), false, None, true),
        SpawnMode::Fresh {
            rename_ctx,
            custom_instructions,
        } => {
            let rename_mode =
                std::env::var("WSX_RENAME_MODE").unwrap_or_else(|_| "claude".to_string());
            let (rp, allow) = if let Some(ctx) = rename_ctx {
                if rename_mode == "claude" {
                    (
                        Some(render_rename_system_prompt(
                            &ctx.current_branch,
                            &ctx.branch_prefix,
                        )),
                        true,
                    )
                } else {
                    (None, false)
                }
            } else {
                (None, false)
            };
            (rp, custom_instructions.clone(), allow, None, false)
        }
        SpawnMode::ProjectManager {
            workspaces_json_path: _,
            custom_instructions,
            resume,
        } => (
            Some(crate::pm::pm_system_prompt(custom_instructions.as_deref())),
            None,
            false,
            Some(crate::pm::pm_allowed_tools().to_string()),
            *resume,
        ),
    };

    if add_continue {
        cmd.arg("--continue");
    }

    if let Some(tools) = allow_tools_override {
        cmd.arg("--allowedTools");
        cmd.arg(tools);
    } else if allow_git_branch {
        cmd.arg("--allowedTools");
        cmd.arg("Bash(git branch:*)");
    }

    let combined = match (rename_prompt, custom) {
        (None, None) => None,
        (Some(r), None) => Some(r),
        (None, Some(c)) => Some(c),
        (Some(r), Some(c)) => Some(format!("{r}\n\n{c}")),
    };

    if let Some(prompt) = combined {
        cmd.arg("--append-system-prompt");
        cmd.arg(prompt);
    }

    cmd
}
```

- [ ] **Step 5: Run tests, verify pass**

Run: `cargo test --lib pty::session::tests -- --test-threads=1`
Expected: All existing tests still pass + 2 new tests pass.

Run: `cargo test --lib -- --test-threads=1`
Expected: All lib tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/pty/session.rs
git commit -m "feat(pty): SpawnMode::ProjectManager with read-only tools + system prompt"
```

---

## Task 5: `Session::send_text_when_settled`

**Files:**
- Modify: `src/pty/session.rs` (add method on `Session`)

The settle-and-write helper used for auto-summary and refresh. Watches `activity_ms` for a quiet window, then writes to the PTY.

- [ ] **Step 1: Write the failing test**

Append to the tests module in `src/pty/session.rs`:

```rust
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn send_text_when_settled_writes_after_quiet_window() {
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let cwd = PathBuf::from(".");
        let s = spawn_session(
            &cwd,
            80,
            24,
            SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
            },
        )
        .unwrap();
        // Prime cat with some output so activity_ms is populated, then let it settle.
        s.writer.send(b"prime\n".to_vec()).await.unwrap();
        // The helper waits for the quiet window, then writes the payload.
        // With cat, the payload echoes back into the screen buffer.
        s.send_text_when_settled("AUTO_MSG", 200, 3_000).await;
        // Allow cat to echo.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let screen = s.parser.lock().unwrap().screen().contents();
        assert!(screen.contains("AUTO_MSG"), "screen contents: {screen:?}");
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn send_text_when_settled_times_out_when_no_output() {
        unsafe {
            // /bin/sh -c 'sleep 5' produces no output for the duration.
            std::env::set_var("WSX_CLAUDE_BIN", "/bin/sleep");
        }
        let cwd = PathBuf::from(".");
        // sleep wants a duration arg; portable-pty's CommandBuilder takes
        // additional args via subsequent `arg` calls inside build_claude_command,
        // but our builder doesn't pass extra args. Use sh instead:
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/bin/sh");
        }
        let s = spawn_session(
            &cwd,
            80,
            24,
            SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
            },
        )
        .unwrap();
        // sh with no -c will read from stdin; it produces no spontaneous output.
        // After 500ms timeout the helper should give up silently.
        let start = std::time::Instant::now();
        s.send_text_when_settled("NEVER_SENT", 200, 500).await;
        let elapsed = start.elapsed();
        assert!(elapsed >= Duration::from_millis(450), "{elapsed:?}");
        assert!(elapsed < Duration::from_millis(1500), "{elapsed:?}");
        s.kill();
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib pty::session::tests::send_text_when_settled -- --test-threads=1`
Expected: FAIL — `send_text_when_settled` method does not exist.

- [ ] **Step 3: Implement the method**

In `src/pty/session.rs`, inside `impl Session` (alongside `resize`, `kill`, etc., around line 65), add:

```rust
    /// Write `text` (with a trailing `\r`) to the PTY after the activity
    /// stream has been quiet for `quiet_ms` milliseconds following some
    /// output. If the overall window of `timeout_ms` elapses without ever
    /// seeing both output AND a quiet window, log a warning and return
    /// without writing.
    ///
    /// Used to gate the PM auto-summary message on claude having finished
    /// rendering its banner + input prompt.
    pub async fn send_text_when_settled(&self, text: &str, quiet_ms: u64, timeout_ms: u64) {
        use std::sync::atomic::Ordering;
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(timeout_ms);
        let quiet = std::time::Duration::from_millis(quiet_ms);
        loop {
            if start.elapsed() >= timeout {
                tracing::warn!(
                    text = %text,
                    "send_text_when_settled: timed out waiting for PTY to settle"
                );
                return;
            }
            let last = self.activity_ms.load(Ordering::Relaxed);
            if last > 0 {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                let since_last = now_ms.saturating_sub(last);
                if since_last >= quiet_ms {
                    let mut payload = text.as_bytes().to_vec();
                    payload.push(b'\r');
                    let _ = self.writer.send(payload).await;
                    return;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let _ = quiet; // silence unused (clippy) — quiet_ms is used directly.
        }
    }
```

- [ ] **Step 4: Run tests, verify pass**

Run: `cargo test --lib pty::session::tests::send_text_when_settled -- --test-threads=1`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add src/pty/session.rs
git commit -m "feat(pty): Session::send_text_when_settled for gated auto-messages"
```

---

## Task 6: `SessionManager` PM lifecycle

**Files:**
- Modify: `src/pty/session.rs` (extend `SessionManager` + `kill_all`)

PM is tracked separately from the workspace-keyed map so we don't need a sentinel `WorkspaceId`.

- [ ] **Step 1: Write the failing test**

Append to the tests module in `src/pty/session.rs`:

```rust
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn session_manager_pm_spawn_get_kill() {
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let cwd = PathBuf::from(".");
        let mut mgr = SessionManager::new();
        assert!(mgr.pm().is_none());
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/wsx-test-pm/workspaces.json"),
            custom_instructions: None,
            resume: false,
        };
        let s = mgr.spawn_pm(&cwd, 80, 24, mode).unwrap();
        assert!(mgr.pm().is_some());
        // Second spawn while running is a no-op (returns existing).
        let mode2 = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/wsx-test-pm/workspaces.json"),
            custom_instructions: None,
            resume: false,
        };
        let s2 = mgr.spawn_pm(&cwd, 80, 24, mode2).unwrap();
        assert!(Arc::ptr_eq(&s, &s2));
        // kill_all also kills PM.
        mgr.kill_all();
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(matches!(
            *s.status.read().unwrap(),
            SessionStatus::Exited { .. }
        ));
        assert!(mgr.pm().is_none(), "kill_all should clear pm slot");
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib pty::session::tests::session_manager_pm_spawn_get_kill -- --test-threads=1`
Expected: FAIL — `spawn_pm` and `pm` methods don't exist.

- [ ] **Step 3: Extend `SessionManager`**

In `src/pty/session.rs`, find the `SessionManager` struct (around line 330). Replace it with:

```rust
pub struct SessionManager {
    sessions: HashMap<WorkspaceId, Arc<Session>>,
    pm: Option<Arc<Session>>,
}
```

Replace the `impl SessionManager::new`:

```rust
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            pm: None,
        }
    }
```

Add new methods inside `impl SessionManager` (after `get`):

```rust
    pub fn spawn_pm(
        &mut self,
        cwd: &Path,
        cols: u16,
        rows: u16,
        mode: SpawnMode,
    ) -> Result<Arc<Session>> {
        if let Some(existing) = &self.pm {
            if matches!(*existing.status.read().unwrap(), SessionStatus::Running { .. }) {
                return Ok(existing.clone());
            }
        }
        let session = Arc::new(spawn_session(cwd, cols, rows, mode)?);
        self.pm = Some(session.clone());
        Ok(session)
    }

    pub fn pm(&self) -> Option<Arc<Session>> {
        self.pm.clone()
    }
```

Replace the existing `kill_all`:

```rust
    pub fn kill_all(&mut self) {
        for s in self.sessions.values() {
            s.kill();
        }
        self.sessions.clear();
        if let Some(pm) = &self.pm {
            pm.kill();
        }
        self.pm = None;
    }
```

- [ ] **Step 4: Run tests, verify pass**

Run: `cargo test --lib pty::session::tests -- --test-threads=1`
Expected: All session tests pass including the new one.

- [ ] **Step 5: Commit**

```bash
git add src/pty/session.rs
git commit -m "feat(pty): SessionManager tracks pm separately; kill_all covers it"
```

---

## Task 7: `PaneFocus` enum + `App` fields

**Files:**
- Modify: `src/ui/mod.rs` (add `PaneFocus`)
- Modify: `src/app.rs:48-104` (add fields + initialization)

State-only change. Fields default to "PM not visible, focus on Dashboard."

- [ ] **Step 1: Write the failing test**

In `src/app.rs`, find the existing `#[cfg(test)] mod tests` (or add one at the bottom if absent). Add:

```rust
#[cfg(test)]
mod pm_state_tests {
    use super::*;
    use crate::store::Store;
    use std::path::PathBuf;

    #[test]
    fn app_initializes_pm_state_off() {
        let store = Store::open_in_memory().unwrap();
        let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        assert!(app.pm.is_none());
        assert!(!app.pm_visible);
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib app::pm_state_tests -- --test-threads=1`
Expected: FAIL — `App::pm`, `App::pm_visible`, `App::focus`, `PaneFocus` don't exist.

- [ ] **Step 3: Add `PaneFocus`**

In `src/ui/mod.rs`, append:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneFocus {
    Dashboard,
    ProjectManager,
}
```

- [ ] **Step 4: Add fields to `App`**

In `src/app.rs`, inside the `App` struct (around line 48), add three new fields (place them after `theme`):

```rust
    pub pm: Option<std::sync::Arc<crate::pty::session::Session>>,
    pub pm_visible: bool,
    pub focus: crate::ui::PaneFocus,
```

In `App::new` (around line 76), add three corresponding initializers inside the struct literal:

```rust
            pm: None,
            pm_visible: false,
            focus: crate::ui::PaneFocus::Dashboard,
```

- [ ] **Step 5: Run tests, verify pass**

Run: `cargo test --lib -- --test-threads=1`
Expected: All lib tests pass including the new one.

- [ ] **Step 6: Commit**

```bash
git add src/ui/mod.rs src/app.rs
git commit -m "feat(app): PaneFocus + pm/pm_visible/focus App fields"
```

---

## Task 8: `pm_pane::render` + dashboard split layout

**Files:**
- Create: `src/ui/pm_pane.rs`
- Modify: `src/ui/mod.rs` (declare module)
- Modify: `src/app.rs` `draw()` function (split when visible)

Renders the PM PTY into a `Rect` with a focus-aware title. When `pm_visible == false`, the dashboard occupies the full area exactly as today.

- [ ] **Step 1: Write the failing test**

Append to the `pm_state_tests` module in `src/app.rs`:

```rust
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn dashboard_renders_full_area_when_pm_hidden() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        assert!(!app.pm_visible);
        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw_for_test(f, &mut app)).unwrap();
        // No PM divider line should be present.
        let buf = term.backend().buffer();
        let rendered = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!rendered.contains("Project Manager"), "{rendered}");
    }

    #[test]
    fn dashboard_renders_split_with_pm_title_when_visible_even_without_session() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.pm_visible = true; // No session yet — the pane shows a placeholder.
        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw_for_test(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        let rendered = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("Project Manager"),
            "expected pane title in rendered buffer:\n{rendered}"
        );
        assert!(
            rendered.contains("Tab to focus"),
            "expected unfocused hint:\n{rendered}"
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib app::pm_state_tests::dashboard_renders -- --test-threads=1`
Expected: FAIL — second test fails (no PM render path yet).

- [ ] **Step 3: Create `src/ui/pm_pane.rs`**

```rust
//! Project Manager pane: renders PM PTY into a sub-rect with focus-aware title.

use crate::pty::session::Session;
use crate::pty::render::render_screen;
use crate::ui::PaneFocus;
use crate::ui::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use std::sync::Arc;

/// Render the PM pane into `area`. When `session` is `None` (pane was
/// just opened and spawn is in flight), a single placeholder line is
/// shown.
pub fn render(
    f: &mut Frame,
    area: Rect,
    session: Option<&Arc<Session>>,
    focus: PaneFocus,
    theme: &Theme,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    let title_area = chunks[0];
    let term_area = chunks[1];

    let title = match focus {
        PaneFocus::ProjectManager => "── Project Manager [Tab/Esc back · r refresh] ──",
        PaneFocus::Dashboard => "── Project Manager [Tab to focus] ──",
    };
    f.render_widget(
        Paragraph::new(title).style(theme.dim_style()),
        title_area,
    );

    match session {
        Some(s) => {
            let parser = s.parser.lock().unwrap();
            let screen = parser.screen();
            render_screen(screen, f.buffer_mut(), term_area);
            if matches!(focus, PaneFocus::ProjectManager) && !screen.hide_cursor() {
                let (cy, cx) = screen.cursor_position();
                f.set_cursor_position((term_area.x + cx, term_area.y + cy));
            }
        }
        None => {
            f.render_widget(
                Paragraph::new("starting project manager…").style(theme.dim_style()),
                term_area,
            );
        }
    }
}

/// Recompute PTY dimensions after a terminal resize.
pub fn resize_session(session: &Arc<Session>, area: Rect) {
    // Subtract 1 row for the title bar.
    let _ = session.resize(area.width, area.height.saturating_sub(1));
}
```

- [ ] **Step 4: Declare the module**

In `src/ui/mod.rs`, add at the top alongside the other `pub mod` lines:

```rust
pub mod pm_pane;
```

- [ ] **Step 5: Integrate the split in `draw()`**

In `src/app.rs`, find `View::Dashboard => {` (around line 198). The body builds `items` and calls `dashboard::render(f, area, ...)`. Replace the `area` argument passed to `dashboard::render` with a split-aware `dashboard_area`. At the top of the `View::Dashboard` arm (before the `let notifications_on = ...` line) insert:

```rust
            let (dashboard_area, pm_area) = if app.pm_visible {
                let chunks = ratatui::layout::Layout::default()
                    .direction(ratatui::layout::Direction::Vertical)
                    .constraints([
                        ratatui::layout::Constraint::Percentage(60),
                        ratatui::layout::Constraint::Percentage(40),
                    ])
                    .split(area);
                (chunks[0], Some(chunks[1]))
            } else {
                (area, None)
            };
```

Then change the existing `dashboard::render(f, area, ...)` call so it uses `dashboard_area` instead of `area` (both occurrences in this arm — the one at the bottom of the arm).

After the `dashboard::render(...)` call, add:

```rust
            if let Some(pm_area) = pm_area {
                if let Some(session) = app.pm.as_ref() {
                    crate::ui::pm_pane::resize_session(session, pm_area);
                }
                crate::ui::pm_pane::render(f, pm_area, app.pm.as_ref(), app.focus, &app.theme);
            }
```

- [ ] **Step 6: Run tests, verify pass**

Run: `cargo test --lib app::pm_state_tests -- --test-threads=1`
Expected: All four tests pass.

Run: `cargo test --workspace -- --test-threads=1`
Expected: All tests still pass (smoke + branch_drift unaffected).

- [ ] **Step 7: Commit**

```bash
git add src/ui/pm_pane.rs src/ui/mod.rs src/app.rs
git commit -m "feat(ui): pm_pane module + 60/40 split when PM visible"
```

---

## Task 9: `open_pm` orchestrator + `p` keybind open path

**Files:**
- Modify: `src/pm.rs` (add `open_pm` async fn)
- Modify: `src/app.rs` (handle `p` in `handle_key_dashboard`)

`open_pm` does the orchestration: ensures PM dir exists, writes `workspaces.json`, decides Fresh vs Continue, spawns. No auto-message yet (Task 11 layers that on).

- [ ] **Step 1: Write the failing test**

In `src/pm.rs`, append to the tests module:

```rust
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn open_pm_spawns_session_and_writes_workspaces_json() {
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let dir = TempDir::new().unwrap();
        let pm_root = dir.path().join("pm");
        let store = Store::open_in_memory().unwrap();
        store.add_repo(Path::new("/tmp/r"), "r", "").unwrap();
        let mut mgr = crate::pty::session::SessionManager::new();
        open_pm(&mut mgr, &store, &pm_root, None).await.unwrap();
        assert!(mgr.pm().is_some(), "expected pm session");
        assert!(pm_root.join("workspaces.json").exists());
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib pm::tests::open_pm_spawns_session_and_writes_workspaces_json -- --test-threads=1`
Expected: FAIL — `open_pm` does not exist.

- [ ] **Step 3: Implement `open_pm`**

Append to `src/pm.rs` (before the `#[cfg(test)]` line):

```rust
/// Open or resume the PM session. Initializes the PM directory, refreshes
/// `workspaces.json`, spawns the PM PTY (Fresh or Continue depending on
/// whether claude has a prior session for the PM cwd), and stores it on
/// the manager. Caller decides whether to send the auto-summary message.
pub async fn open_pm(
    mgr: &mut crate::pty::session::SessionManager,
    store: &Store,
    pm_dir: &Path,
    custom_instructions: Option<String>,
) -> Result<()> {
    init_pm_dir(pm_dir)?;
    let workspaces_json = pm_dir.join("workspaces.json");
    write_workspaces_json(store, &workspaces_json)?;
    let resume = crate::pty::session::has_prior_session(pm_dir);
    let mode = crate::pty::session::SpawnMode::ProjectManager {
        workspaces_json_path: workspaces_json,
        custom_instructions,
        resume,
    };
    mgr.spawn_pm(pm_dir, 80, 24, mode)?;
    Ok(())
}
```

- [ ] **Step 4: Wire `p` into the dashboard handler**

In `src/app.rs`, in `handle_key_dashboard` (around line 365), add a new match arm BEFORE the trailing `_ => {}`:

```rust
        (KeyCode::Char('p'), _) => {
            if pm_enabled(&app.store) {
                if app.pm_visible {
                    // Hide pane; session stays alive.
                    app.pm_visible = false;
                    app.focus = crate::ui::PaneFocus::Dashboard;
                } else {
                    // Open pane. Spawn if not yet spawned this run.
                    let dirs = crate::config::Dirs::discover();
                    let pm_dir = dirs.pm_dir();
                    let custom = app
                        .store
                        .get_setting("pm_custom_instructions")
                        .ok()
                        .flatten();
                    if let Err(e) =
                        crate::pm::open_pm(&mut app.sessions, &app.store, &pm_dir, custom).await
                    {
                        app.modal = Some(Modal::Error {
                            message: e.to_string(),
                        });
                        return Ok(());
                    }
                    app.pm = app.sessions.pm();
                    app.pm_visible = true;
                }
            }
        }
```

Also add the helper near the top of `src/app.rs` (alongside `nerd_fonts_enabled` / `notifications_enabled`):

```rust
fn pm_enabled(store: &Store) -> bool {
    match store.get_setting("pm_enabled").ok().flatten() {
        None => true,
        Some(v) => !matches!(
            v.trim().to_lowercase().as_str(),
            "false" | "0" | "off" | "no"
        ),
    }
}
```

(Find the existing `fn nerd_fonts_enabled` to see the exact pattern; place this immediately after it.)

- [ ] **Step 5: Run tests, verify pass**

Run: `cargo test --lib -- --test-threads=1`
Expected: All lib tests pass (including the new `open_pm` test).

- [ ] **Step 6: Commit**

```bash
git add src/pm.rs src/app.rs
git commit -m "feat(pm): open_pm orchestrator + p keybind open/hide"
```

---

## Task 10: Tab/Esc focus toggle + PM-focused key forwarding

**Files:**
- Modify: `src/app.rs` (extend `handle_key_dashboard`)

When the pane is visible and focus is on PM, most keystrokes forward to the PM PTY. Tab and Esc swap focus back. The `p` keybind is unchanged — hides the pane regardless of focus.

- [ ] **Step 1: Write the failing test**

Append to the `pm_state_tests` module in `src/app.rs`:

```rust
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tab_swaps_focus_when_pm_visible() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.pm_visible = true;
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .await
            .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::ProjectManager));
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .await
            .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn esc_returns_focus_to_dashboard() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.pm_visible = true;
        app.focus = crate::ui::PaneFocus::ProjectManager;
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .await
            .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tab_no_op_when_pm_hidden() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        assert!(!app.pm_visible);
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .await
            .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib app::pm_state_tests::tab -- --test-threads=1`
Expected: FAIL — Tab/Esc not yet handled.

- [ ] **Step 3: Implement focus routing + PM key forwarding**

In `src/app.rs`, modify `handle_key_dashboard` (line 365). Wrap the body so PM-focused keys forward instead of running the dashboard handler:

Replace the function signature line + opening brace:

```rust
async fn handle_key_dashboard(app: &mut App, k: crossterm::event::KeyEvent) -> Result<()> {
```

…and prepend the following before the existing `match (k.code, k.modifiers) {`:

```rust
    // PM pane focus handling. When PM is focused, most keystrokes forward
    // to its PTY. Tab/Esc swap back to dashboard; `p` and `r` are still
    // handled here.
    if app.pm_visible && matches!(app.focus, crate::ui::PaneFocus::ProjectManager) {
        match (k.code, k.modifiers) {
            (KeyCode::Tab, _) | (KeyCode::Esc, _) => {
                app.focus = crate::ui::PaneFocus::Dashboard;
                return Ok(());
            }
            (KeyCode::Char('p'), _) | (KeyCode::Char('r'), _) => {
                // Fall through to the main match below.
            }
            _ => {
                if let Some(session) = app.pm.as_ref() {
                    if let Some(bytes) = encode_key_for_pty(&k) {
                        let _ = session.writer.send(bytes).await;
                    }
                }
                return Ok(());
            }
        }
    }
    // Tab when focus is on Dashboard and PM is visible: swap to PM.
    if app.pm_visible
        && matches!(app.focus, crate::ui::PaneFocus::Dashboard)
        && k.code == KeyCode::Tab
    {
        app.focus = crate::ui::PaneFocus::ProjectManager;
        return Ok(());
    }
```

Add the helper `encode_key_for_pty` near the top of `src/app.rs` (after the existing `fn classify_activity`):

```rust
fn encode_key_for_pty(k: &crossterm::event::KeyEvent) -> Option<Vec<u8>> {
    use crossterm::event::{KeyCode, KeyModifiers};
    match (k.code, k.modifiers) {
        (KeyCode::Char(c), m) if m == KeyModifiers::NONE || m == KeyModifiers::SHIFT => {
            Some(c.to_string().into_bytes())
        }
        (KeyCode::Char(c), m) if m.contains(KeyModifiers::CONTROL) => {
            let upper = c.to_ascii_uppercase();
            if ('@'..='_').contains(&upper) {
                Some(vec![(upper as u8) - b'@'])
            } else {
                None
            }
        }
        (KeyCode::Enter, _) => Some(b"\r".to_vec()),
        (KeyCode::Backspace, _) => Some(vec![0x7f]),
        (KeyCode::Up, _) => Some(b"\x1b[A".to_vec()),
        (KeyCode::Down, _) => Some(b"\x1b[B".to_vec()),
        (KeyCode::Right, _) => Some(b"\x1b[C".to_vec()),
        (KeyCode::Left, _) => Some(b"\x1b[D".to_vec()),
        (KeyCode::Tab, _) => Some(b"\t".to_vec()),
        _ => None,
    }
}
```

(Note: this duplicates a small amount of logic from the existing attached-view key handler. The two paths have slightly different ignore-list needs, so keep them separate rather than abstract.)

- [ ] **Step 4: Run tests, verify pass**

Run: `cargo test --lib app::pm_state_tests -- --test-threads=1`
Expected: All `pm_state_tests` pass.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): Tab/Esc focus toggle + PM key forwarding"
```

---

## Task 11: Auto-summary on first Fresh open

**Files:**
- Modify: `src/pm.rs` (add `open_pm_with_auto_summary`)
- Modify: `src/app.rs` (call the auto-summary path on first open per run)

The auto-summary message is sent only when (a) it's the first time the pane has been opened in this wsx run AND (b) the spawn mode chosen was Fresh (no prior session). Subsequent opens within the same run skip it; `--continue` resumes never auto-send.

- [ ] **Step 1: Write the failing test**

In `src/pm.rs`, append to the tests module:

```rust
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn open_pm_with_auto_summary_writes_message_after_settle() {
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let dir = TempDir::new().unwrap();
        let pm_root = dir.path().join("pm");
        let store = Store::open_in_memory().unwrap();
        store.add_repo(Path::new("/tmp/r"), "r", "").unwrap();
        let mut mgr = crate::pty::session::SessionManager::new();
        // Prime activity so the settle gate has something to wait on. We
        // call open_pm first, then poke the PTY with a byte to populate
        // activity_ms, then dispatch the auto-summary helper.
        open_pm_with_auto_summary(&mut mgr, &store, &pm_root, None)
            .await
            .unwrap();
        let session = mgr.pm().expect("pm session");
        session.writer.send(b"x\n".to_vec()).await.unwrap();
        // Let send_text_when_settled in the bg task observe quiet + write.
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        let screen = session.parser.lock().unwrap().screen().contents();
        assert!(
            screen.contains("status summary"),
            "expected auto-summary echoed by cat. screen: {screen:?}"
        );
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib pm::tests::open_pm_with_auto_summary -- --test-threads=1`
Expected: FAIL — symbol undefined.

- [ ] **Step 3: Implement `open_pm_with_auto_summary`**

Append to `src/pm.rs` (before `#[cfg(test)]`):

```rust
/// Like `open_pm` but also spawns a background task that delivers the
/// auto-summary message after the PTY settles. Only call this on the
/// FIRST open per wsx run AND only when the resulting spawn mode is
/// Fresh — `--continue` resumes should not auto-send.
pub async fn open_pm_with_auto_summary(
    mgr: &mut crate::pty::session::SessionManager,
    store: &Store,
    pm_dir: &Path,
    custom_instructions: Option<String>,
) -> Result<()> {
    let was_resume = crate::pty::session::has_prior_session(pm_dir);
    open_pm(mgr, store, pm_dir, custom_instructions).await?;
    if was_resume {
        return Ok(());
    }
    if let Some(session) = mgr.pm() {
        let session = session.clone();
        tokio::spawn(async move {
            session
                .send_text_when_settled(PM_AUTO_SUMMARY_MESSAGE, 400, 5_000)
                .await;
        });
    }
    Ok(())
}
```

- [ ] **Step 4: Wire `App` to use the auto-summary path on first open**

In `src/app.rs`, modify the `p` open arm added in Task 9 so it uses `open_pm_with_auto_summary` instead of `open_pm`, but only on the very first open per process. Add a new field on `App` first.

In the `App` struct (line 48 area), add after the `focus` field:

```rust
    pub pm_auto_summary_sent: bool,
```

In `App::new`, initialize:

```rust
            pm_auto_summary_sent: false,
```

In `handle_key_dashboard`, replace the `open_pm` call from Task 9 with:

```rust
                    let result = if app.pm_auto_summary_sent {
                        crate::pm::open_pm(&mut app.sessions, &app.store, &pm_dir, custom).await
                    } else {
                        crate::pm::open_pm_with_auto_summary(
                            &mut app.sessions,
                            &app.store,
                            &pm_dir,
                            custom,
                        )
                        .await
                    };
                    if let Err(e) = result {
                        app.modal = Some(Modal::Error {
                            message: e.to_string(),
                        });
                        return Ok(());
                    }
                    app.pm_auto_summary_sent = true;
                    app.pm = app.sessions.pm();
                    app.pm_visible = true;
```

- [ ] **Step 5: Run tests, verify pass**

Run: `cargo test --lib pm::tests -- --test-threads=1`
Expected: All `pm` tests pass.

Run: `cargo test --lib -- --test-threads=1`
Expected: All lib tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/pm.rs src/app.rs
git commit -m "feat(pm): auto-summary message on first Fresh open"
```

---

## Task 12: `r` refresh

**Files:**
- Modify: `src/pm.rs` (add `refresh_pm`)
- Modify: `src/app.rs` (handle `r` when PM focused)

`r` while PM is focused rewrites `workspaces.json` and sends the refresh message.

- [ ] **Step 1: Write the failing test**

In `src/pm.rs`, append to the tests module:

```rust
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn refresh_pm_rewrites_json_and_sends_message() {
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let dir = TempDir::new().unwrap();
        let pm_root = dir.path().join("pm");
        let store = Store::open_in_memory().unwrap();
        store.add_repo(Path::new("/tmp/r"), "r", "").unwrap();
        let mut mgr = crate::pty::session::SessionManager::new();
        open_pm(&mut mgr, &store, &pm_root, None).await.unwrap();
        let first_meta = std::fs::metadata(pm_root.join("workspaces.json"))
            .unwrap()
            .modified()
            .unwrap();
        // Wait so mtime advances on rewrite.
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        let session = mgr.pm().unwrap();
        session.writer.send(b"prime\n".to_vec()).await.unwrap();
        refresh_pm(&mut mgr, &store, &pm_root).await.unwrap();
        // Allow settle + echo.
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        let second_meta = std::fs::metadata(pm_root.join("workspaces.json"))
            .unwrap()
            .modified()
            .unwrap();
        assert!(second_meta > first_meta, "expected mtime advance");
        let screen = session.parser.lock().unwrap().screen().contents();
        assert!(
            screen.contains("Refresh"),
            "expected refresh echo. screen: {screen:?}"
        );
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib pm::tests::refresh_pm -- --test-threads=1`
Expected: FAIL — `refresh_pm` not defined.

- [ ] **Step 3: Implement `refresh_pm`**

Append to `src/pm.rs` (before `#[cfg(test)]`):

```rust
/// Refresh PM state: rewrite `workspaces.json` and send the refresh
/// user message to PM's PTY (after settle).
pub async fn refresh_pm(
    mgr: &mut crate::pty::session::SessionManager,
    store: &Store,
    pm_dir: &Path,
) -> Result<()> {
    write_workspaces_json(store, &pm_dir.join("workspaces.json"))?;
    if let Some(session) = mgr.pm() {
        let s = session.clone();
        tokio::spawn(async move {
            s.send_text_when_settled(PM_REFRESH_MESSAGE, 400, 5_000)
                .await;
        });
    }
    Ok(())
}
```

- [ ] **Step 4: Wire `r` in dashboard handler**

In `src/app.rs`, inside the existing PM-focused early-return block from Task 10 (where Tab/Esc are caught), the `'r'` falls through to the main match. Add a new arm in the main match for `'r'`:

In `handle_key_dashboard`'s `match (k.code, k.modifiers) {`, add (alongside `'p'`):

```rust
        (KeyCode::Char('r'), _)
            if app.pm_visible && matches!(app.focus, crate::ui::PaneFocus::ProjectManager) =>
        {
            let dirs = crate::config::Dirs::discover();
            let pm_dir = dirs.pm_dir();
            if let Err(e) = crate::pm::refresh_pm(&mut app.sessions, &app.store, &pm_dir).await {
                app.modal = Some(Modal::Error {
                    message: e.to_string(),
                });
            }
        }
```

- [ ] **Step 5: Run tests, verify pass**

Run: `cargo test --lib pm::tests -- --test-threads=1`
Expected: All `pm` tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/pm.rs src/app.rs
git commit -m "feat(pm): r refresh rewrites json + sends refresh message"
```

---

## Task 13: Settings keys (`pm_enabled`, `pm_custom_instructions`)

**Files:**
- Modify: `src/cli.rs:62-73` (extend `known_setting_key`)
- Modify: `src/cli.rs` test module (extend acceptance test)

The `pm_enabled` gating is already used by Task 9 via `pm_enabled(store)`; this task adds the CLI surface so the user can set/unset the keys.

- [ ] **Step 1: Write the failing test**

In `src/cli.rs`, find the existing `rejects_unknown_setting_key` test (line 346 area). Add this alongside:

```rust
    #[test]
    fn accepts_pm_enabled_and_pm_custom_instructions() {
        // Sanity-check the allowlist directly.
        assert!(known_setting_key("pm_enabled"));
        assert!(known_setting_key("pm_custom_instructions"));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib cli::tests::accepts_pm -- --test-threads=1`
Expected: FAIL — `pm_enabled` not recognized.

- [ ] **Step 3: Extend `known_setting_key`**

In `src/cli.rs`, replace the body of `known_setting_key` (line 62):

```rust
fn known_setting_key(k: &str) -> bool {
    matches!(
        k,
        "branch_prefix"
            | "custom_instructions"
            | "nerd_fonts"
            | "editor_cmd"
            | "terminal_cmd"
            | "notifications"
            | "theme"
            | "pm_enabled"
            | "pm_custom_instructions"
    )
}
```

- [ ] **Step 4: Run tests, verify pass**

Run: `cargo test --lib cli::tests -- --test-threads=1`
Expected: All CLI tests pass.

Run: `cargo test --workspace -- --test-threads=1`
Expected: All tests pass across lib, branch_drift, smoke.

Run: `cargo clippy --all-targets -- -D warnings`
Expected: No warnings.

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): pm_enabled + pm_custom_instructions settings keys"
```

---

## Task 14: README + final commit closing issue #8

**Files:**
- Modify: `README.md`

Documentation. Final commit includes the `Closes #8` trailer.

- [ ] **Step 1: Add the new settings rows**

In `README.md`, find the existing settings table (the one with `branch_prefix`, `custom_instructions`, `nerd_fonts`, `editor_cmd`, etc.). Add two new rows immediately after the `theme` row:

```markdown
| `pm_enabled` | Enable the Project Manager pane (`p` keybind). Default ON; set to `off` / `false` / `0` / `no` to disable. |
| `pm_custom_instructions` | Free-text appended to the project manager's system prompt. Same `@file` / empty-clears semantics as `custom_instructions`. |
```

- [ ] **Step 2: Add the new dashboard keybinding rows**

In `README.md`, find the "Dashboard" keybinding table. Add the following rows after the existing `[q] Quit` row:

```markdown
| `p` | Toggle the Project Manager pane (no-op when `pm_enabled` is off) |
| `Tab` | Swap focus between dashboard and the PM pane (when visible) |
| `r` (when PM focused) | Refresh `workspaces.json` and ask PM to re-summarize |
```

- [ ] **Step 3: Add the "Project manager pane" section**

In `README.md`, insert a new section between "Auto-rename modes" and "Environment variables":

```markdown
## Project manager pane

Press `p` on the dashboard to open a horizontal pane below the workspace list
hosting a dedicated Claude Code "project manager" session. PM's job is to
answer three questions about each of your active workspaces:

- What was this workspace created for?
- Where have things been left off?
- What's next to close it out?

`Tab` swaps focus into the PM pane (keystrokes then go to PM, like the
attached view). `Tab` or `Esc` swaps focus back to the dashboard. `r`
(while PM is focused) refreshes `workspaces.json` and asks PM to
re-summarize.

PM lives at `$XDG_STATE_HOME/wsx/project-manager/` and persists across wsx
restarts via Claude Code's `--continue`. On the first `p` of a wsx run with
no prior PM session, wsx auto-sends a status-summary request. On subsequent
runs (resuming via `--continue`), wsx stays silent — type your own
question or press `r` for a fresh summary.

PM only sees workspaces wsx knows about (registered repos and their `Ready`
workspaces). It gets read-only access:

- `Read` for inspecting files (including `~/.claude/projects/.../<session>.jsonl`).
- Narrow `Bash` for `git status` / `log` / `diff` / `branch`, `cat`, `ls`.

Disable the feature entirely with `wsx config set pm_enabled off`.
Customize PM's behavior with `wsx config set pm_custom_instructions @./pm.md`.
```

- [ ] **Step 4: Add the storage row**

In `README.md`, find the "Storage and configuration files" table. Add this row before the `~/.claude/projects/...` row:

```markdown
| `$XDG_STATE_HOME/wsx/project-manager/` | PM Claude Code session cwd; contains `workspaces.json` and PM's own git init. Auto-created on first `p`. |
```

- [ ] **Step 5: Final build + test sweep**

Run: `cargo test --workspace -- --test-threads=1`
Expected: All tests pass.

Run: `cargo clippy --all-targets -- -D warnings`
Expected: No warnings.

Run: `cargo build --release`
Expected: Succeeds.

- [ ] **Step 6: Final commit**

```bash
git add README.md
git commit -m "$(cat <<'EOF'
feat(pm): project manager pane (#8)

Opt-in horizontal pane below the dashboard hosting a Claude Code session
with a project-manager system prompt and read-only filesystem tools.

- `p` toggles the pane; `Tab`/`Esc` swap focus; `r` refreshes.
- PM cwd at $XDG_STATE_HOME/wsx/project-manager/ persists across wsx
  restarts via --continue.
- Hybrid info flow: wsx writes a terse workspaces.json (paths, branches,
  git counts, session-log dirs); PM mines JSONLs / git on demand.
- Auto-summary on first Fresh open; --continue resumes stay silent.
- Settings: pm_enabled (default on), pm_custom_instructions.

Closes #8
EOF
)"
```

---

## Self-Review

**1. Spec coverage:**

| Spec section | Covered by |
|---|---|
| New `pm.rs` module | Task 1 |
| Persistent home + git init | Task 2 |
| `SpawnMode::ProjectManager` + build_claude_command extension | Task 4 |
| `--allowedTools` narrow list | Task 3 (constant) + Task 4 (wiring) |
| `Session::send_text_when_settled` settle gate | Task 5 |
| `SessionManager` PM tracking + `kill_all` extension | Task 6 |
| `App.pm` / `pm_visible` / `focus` fields | Task 7 |
| `workspaces.json` schema + atomic write | Task 1 |
| PM system prompt + custom-instructions append | Task 3 |
| 60/40 split layout | Task 8 |
| Focus state machine (Tab/Esc) | Task 10 |
| `p` keybind open/close | Task 9 |
| Auto-summary on first Fresh open only | Task 11 |
| `r` refresh | Task 12 |
| `pm_enabled` toggle | Task 9 (gating) + Task 13 (CLI) |
| `pm_custom_instructions` | Task 3 (prompt) + Task 13 (CLI) + Task 9 (load) |
| README updates | Task 14 |

No gaps.

**2. Placeholder scan:** No "TODO", "TBD", "etc.", "handle edge cases", or "similar to Task N" in any task.

**3. Type consistency:**
- `SpawnMode::ProjectManager { workspaces_json_path, custom_instructions, resume }` — consistent across Tasks 4, 6, 9.
- `Session::send_text_when_settled(&self, text: &str, quiet_ms: u64, timeout_ms: u64)` — used identically in Tasks 5, 11, 12.
- `pm_system_prompt(custom: Option<&str>) -> String` — declared in Task 3, used in Task 4.
- `open_pm` / `open_pm_with_auto_summary` / `refresh_pm` — async fns with consistent `&mut SessionManager, &Store, &Path` signature; the with-auto-summary variant adds `Option<String>` for custom instructions.
- `PaneFocus` — defined in Task 7, used in Tasks 8, 10, 12.
- `App.pm_auto_summary_sent` — added in Task 11, no other consumers.

All type names and signatures are consistent.
