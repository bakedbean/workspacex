# Editor-Agent Context Digest Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `wsx context show` / `wsx context write`, which render a markdown digest of the current workspace (recap, status, peers, recent commits, primary agent's last message, instructions for an editor-hosted agent), plus a doctrine clause telling wsx agents an editor-hosted agent may share the worktree, and docs with a neovim + magenta.nvim snippet.

**Architecture:** A new `src/commands/context.rs` owns a `ContextDigest` struct, an async `gather` that fills it from the sqlite store, git, and the agent transcript, and a pure `render` that produces markdown. The CLI wires two subcommands to it following the existing `recap` pattern. Transcript access reuses the per-agent-kind locate/tail dispatch currently inlined in the TUI tail loop, extracted into `src/activity/mod.rs`.

**Tech Stack:** Rust 2024, tokio, rusqlite (via `Store`), `sessionx` crate for transcript parsing, `tempfile` for tests, mdBook docs.

**Spec:** `docs/superpowers/specs/2026-09-02-editor-agent-context-design.md`

## Global Constraints

- Both subcommands resolve the workspace with `resolve_current_workspace` (env var, else cwd). No `--workspace` flag, no other flags; extra args are a usage error.
- `show` and `write` produce identical bytes. `write` path is `<state>/wsx/context/<repo name>/<workspace name>.md`, written via temp file + rename, and prints only the absolute path.
- Missing recap/status/transcript and any git failure render as an omitted section or `-`; never a non-zero exit. Non-zero exit only when the workspace can't be resolved or the file can't be written.
- Last assistant text is truncated to 2000 chars on a char boundary with `… [truncated]` appended.
- Recent commits capped at 20 lines.
- The wsx side must stay editor-agnostic: no mention of neovim or magenta in Rust code or in the instructions block. Docs may name them.
- Do not add `Co-Authored-By` or "Generated with" trailers to commits (user preference).
- Run `cargo fmt` before every commit; `cargo clippy --all-targets -- -D warnings` must stay clean.
- After each commit run `wsx recap set --state "<what landed>" --state-short "<≤24 chars>" --next "<next task>" --next-short "<≤24 chars>"`.

---

## File map

| File | Responsibility |
|---|---|
| `src/activity/mod.rs` | New `locate_session_file_for` / `tail_session_for` dispatch on `AgentKind` |
| `src/app/background.rs:38-89` | Replace two inline `match ws_agent` blocks with the new helpers |
| `src/git/mod.rs` | New `log_oneline(worktree, base, limit)` |
| `src/config/mod.rs` | New `Dirs::context_dir()` |
| `src/commands/context.rs` (new) | `ContextDigest`, `gather`, `render`, `digest_path`, `write_atomic`, `format_age` |
| `src/commands/mod.rs` | `pub mod context;` |
| `src/cli/action.rs` | `CliAction::ContextShow`, `CliAction::ContextWrite` |
| `src/cli/parse/reporting.rs` | `parse_context` |
| `src/cli/parse/mod.rs` | dispatch `"context"` |
| `src/cli/groups.rs` | registry entry |
| `src/cli/run.rs` | two match arms |
| `src/cli/tests.rs` | parser tests + registry list |
| `src/agent/doctrine.rs` | `CLAUSE_EXTERNAL_EDITOR` + test |
| `skills/wsx/SKILL.md` | "External editor agents" section |
| `docs/book/src/integrations/editor-agent-context.md` (new) | Docs page + nvim snippet |
| `docs/book/src/SUMMARY.md`, `integrations/index.md`, `integrations/editor-terminal-diff.md`, `reference/storage-and-config-files.md` | Links and the new path |

---

### Task 1: Extract per-agent locate/tail helpers from the tail loop

**Files:**
- Modify: `src/activity/mod.rs`
- Modify: `src/app/background.rs:38-89`

**Interfaces:**
- Produces:
  - `pub fn locate_session_file_for(kind: crate::pty::session::AgentKind, worktree: &std::path::Path) -> Option<std::path::PathBuf>`
  - `pub fn tail_session_for(kind: crate::pty::session::AgentKind, path: &std::path::Path, offset: u64) -> crate::error::Result<crate::activity::events::TailUpdate>`

- [ ] **Step 1: Write the failing test**

Append to `src/activity/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::session::AgentKind;
    use std::path::Path;

    /// A worktree nobody has ever opened an agent in has no session file
    /// for any kind. Exercises every dispatch arm without a fixture.
    #[test]
    fn locate_returns_none_for_unknown_worktree_for_every_kind() {
        let dir = tempfile::TempDir::new().unwrap();
        for kind in AgentKind::ALL {
            assert!(
                locate_session_file_for(kind, dir.path()).is_none(),
                "{kind:?} should find nothing"
            );
        }
    }

    /// Tailing a nonexistent path must surface an error, not panic, for
    /// every kind (the tail loop treats Err as "skip this tick").
    #[test]
    fn tail_missing_file_is_err_for_every_kind() {
        let missing = Path::new("/nonexistent/wsx-test/session.jsonl");
        for kind in AgentKind::ALL {
            assert!(
                tail_session_for(kind, missing, 0).is_err(),
                "{kind:?} should error on a missing file"
            );
        }
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib activity::tests -- --nocapture`
Expected: compile error, `locate_session_file_for` not found.

- [ ] **Step 3: Add the helpers**

In `src/activity/mod.rs`, after `pub mod proc;`:

```rust
use crate::pty::session::AgentKind;
use std::path::{Path, PathBuf};

/// Find the current session transcript for `worktree`, dispatching on the
/// agent kind's on-disk layout. `None` when no session has been recorded.
pub fn locate_session_file_for(kind: AgentKind, worktree: &Path) -> Option<PathBuf> {
    match kind {
        AgentKind::Claude => events::locate_session_file(worktree),
        AgentKind::Pi => pi_events::locate_session_file(worktree),
        AgentKind::Hermes => hermes_events::locate_session_file(worktree),
        AgentKind::Codex => codex_events::locate_session_file(worktree),
        AgentKind::Omp => omp_events::locate_session_file(worktree),
    }
}

/// Tail `path` from byte `offset` with the parser matching the agent kind.
/// Pass `offset = 0` to read the whole transcript.
pub fn tail_session_for(
    kind: AgentKind,
    path: &Path,
    offset: u64,
) -> crate::error::Result<events::TailUpdate> {
    match kind {
        AgentKind::Claude => events::tail_session(path, offset).map_err(Into::into),
        AgentKind::Pi => pi_events::tail_session(path, offset).map_err(Into::into),
        AgentKind::Hermes => hermes_events::tail_session(path, offset),
        AgentKind::Codex => codex_events::tail_session(path, offset).map_err(Into::into),
        AgentKind::Omp => omp_events::tail_session(path, offset).map_err(Into::into),
    }
}
```

Note: `hermes_events::tail_session` already returns `crate::error::Result`, so it has no `map_err`. If the compiler reports a mismatch for Hermes's `locate_session_file` return type, keep whatever the existing `background.rs` arm does for that kind.

- [ ] **Step 4: Replace the inline matches in `background.rs`**

In `src/app/background.rs`, replace the block starting `let current_file = match ws_agent {` (through its closing `};`) with:

```rust
    let current_file = crate::activity::locate_session_file_for(ws_agent, &worktree_path);
```

Replace the block starting `let tail_result = match ws_agent {` (through its closing `};`) with:

```rust
    let tail_result = crate::activity::tail_session_for(ws_agent, &file, tail_from);
```

- [ ] **Step 5: Run tests and clippy**

Run: `cargo test --lib activity::tests && cargo clippy --all-targets -- -D warnings`
Expected: both tests pass; clippy clean.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add src/activity/mod.rs src/app/background.rs
git commit -m "refactor(activity): extract per-agent locate/tail helpers from background tail loop"
```

---

### Task 2: `git::log_oneline`

**Files:**
- Modify: `src/git/mod.rs`

**Interfaces:**
- Produces: `pub async fn log_oneline(worktree: &Path, base: &str, limit: usize) -> Result<Vec<String>>` — each element is one `"<short sha> <subject>"` line, newest first, no trailing newline.

- [ ] **Step 1: Write the failing tests**

Inside the existing `#[cfg(test)] mod tests` in `src/git/mod.rs` (it already has `init_repo()`), add:

```rust
    #[tokio::test]
    async fn log_oneline_lists_commits_ahead_of_base_newest_first() {
        let dir = init_repo();
        let run = |args: &[&str]| {
            let status = StdCmd::new("git")
                .current_dir(dir.path())
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {:?} failed", args);
        };
        run(&["checkout", "-q", "-b", "feature"]);
        run(&["commit", "--allow-empty", "-q", "-m", "first change"]);
        run(&["commit", "--allow-empty", "-q", "-m", "second change"]);

        let lines = log_oneline(dir.path(), "main", 20).await.unwrap();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with(" second change"), "{lines:?}");
        assert!(lines[1].ends_with(" first change"), "{lines:?}");
        // "<sha> <subject>": sha is 7+ hex chars followed by a space.
        let sha = lines[0].split(' ').next().unwrap();
        assert!(sha.len() >= 7 && sha.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn log_oneline_respects_limit() {
        let dir = init_repo();
        let run = |args: &[&str]| {
            let status = StdCmd::new("git")
                .current_dir(dir.path())
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {:?} failed", args);
        };
        run(&["checkout", "-q", "-b", "feature"]);
        for i in 0..3 {
            run(&["commit", "--allow-empty", "-q", "-m", &format!("c{i}")]);
        }
        let lines = log_oneline(dir.path(), "main", 2).await.unwrap();
        assert_eq!(lines.len(), 2);
    }

    #[tokio::test]
    async fn log_oneline_is_empty_when_at_base() {
        let dir = init_repo();
        let lines = log_oneline(dir.path(), "main", 20).await.unwrap();
        assert!(lines.is_empty());
    }

    #[tokio::test]
    async fn log_oneline_errors_on_unknown_base() {
        let dir = init_repo();
        assert!(log_oneline(dir.path(), "no-such-branch", 20).await.is_err());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib git::tests::log_oneline`
Expected: compile error, `log_oneline` not found.

- [ ] **Step 3: Implement**

In `src/git/mod.rs`, after `workspace_status`:

```rust
/// `git log --oneline <base>..HEAD`, newest first, at most `limit` lines.
/// Each line is `"<short sha> <subject>"`. Empty when HEAD is at `base`.
/// Errors propagate (unknown base, not a repo) so callers can decide
/// whether to omit the section.
pub async fn log_oneline(worktree: &Path, base: &str, limit: usize) -> Result<Vec<String>> {
    let limit = limit.to_string();
    let range = format!("{base}..HEAD");
    let out = run(
        worktree,
        &["log", "--oneline", "--no-decorate", "-n", &limit, &range],
    )
    .await?;
    Ok(out
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib git::tests::log_oneline`
Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add src/git/mod.rs
git commit -m "feat(git): add log_oneline helper"
```

---

### Task 3: `ContextDigest` render (pure) + `Dirs::context_dir`

**Files:**
- Create: `src/commands/context.rs`
- Modify: `src/commands/mod.rs`
- Modify: `src/config/mod.rs:36-44`

**Interfaces:**
- Produces:
  - `pub struct ContextDigest { … }` (fields below)
  - `pub fn render(d: &ContextDigest) -> String`
  - `pub fn format_age(now_ms: i64, then_ms: i64) -> String`
  - `pub fn digest_path(dirs: &Dirs, repo_name: &str, workspace_name: &str) -> PathBuf`
  - `pub fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()>`
  - `pub const MAX_LAST_MESSAGE_CHARS: usize = 2000;`
  - `Dirs::context_dir(&self) -> PathBuf` (= `app_dir()/context`)

- [ ] **Step 1: Register the module and add `context_dir`**

`src/commands/mod.rs`: add `pub mod context;` (alphabetically first) and extend the module doc comment with `` `context` renders the workspace digest for `wsx context show|write`; ``.

`src/config/mod.rs`, after `log_dir`:

```rust
    /// Per-workspace context digests written by `wsx context write`:
    /// `<app_dir>/context/<repo>/<workspace>.md`.
    pub fn context_dir(&self) -> PathBuf {
        self.app_dir().join("context")
    }
```

- [ ] **Step 2: Write the failing render tests**

Create `src/commands/context.rs` with the struct, stub functions, and tests:

```rust
//! The workspace context digest behind `wsx context show|write`.
//!
//! An editor-hosted agent (one that runs inside the user's editor rather
//! than in a wsx-spawned PTY) has no way to learn what this workspace is
//! for, what its primary agent last reported, or who its peers are. This
//! module renders all of that as one markdown file the editor can hand to
//! its agent as context. `gather` reads the store, git, and the primary
//! agent's transcript; `render` is pure so the format is unit-tested
//! without any of those.

use crate::config::Dirs;
use crate::data::agents::AgentInstance;
use crate::data::store::{ReportedStatus, Store, Workspace, WorkspaceRecap};
use crate::error::Result;
use std::path::{Path, PathBuf};

/// Hard cap on the primary agent's last message, in chars.
pub const MAX_LAST_MESSAGE_CHARS: usize = 2000;
/// Hard cap on the recent-commits list.
pub const MAX_COMMITS: usize = 20;

#[derive(Debug, Clone)]
pub struct ContextDigest {
    pub repo_name: String,
    pub workspace_name: String,
    pub branch: String,
    pub base_ref: String,
    pub worktree: PathBuf,
    /// Primary first, then creation order (the store's ordering).
    pub agents: Vec<AgentInstance>,
    pub status: Option<ReportedStatus>,
    pub recap: Option<WorkspaceRecap>,
    /// `"<sha> <subject>"` lines, newest first, already capped.
    pub commits: Vec<String>,
    /// `None` when git status could not be read.
    pub uncommitted: Option<crate::git::WorkspaceStatus>,
    /// Untruncated; `render` applies the cap.
    pub last_assistant_text: Option<String>,
    /// Injected so age strings are deterministic in tests.
    pub now_ms: i64,
}

pub fn render(d: &ContextDigest) -> String {
    todo!()
}

pub fn format_age(now_ms: i64, then_ms: i64) -> String {
    todo!()
}

pub fn digest_path(dirs: &Dirs, repo_name: &str, workspace_name: &str) -> PathBuf {
    todo!()
}

pub fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::store::{AgentInstanceId, ReportedState, WorkspaceId};
    use crate::pty::session::AgentKind;

    fn agent(kind: AgentKind, ordinal: i64, primary: bool) -> AgentInstance {
        AgentInstance {
            id: AgentInstanceId(ordinal),
            workspace_id: WorkspaceId(1),
            agent: kind,
            ordinal,
            is_primary: primary,
            session_ref: None,
            created_at: 0,
        }
    }

    fn full() -> ContextDigest {
        ContextDigest {
            repo_name: "workspacex".into(),
            workspace_name: "magenta-nvim-context".into(),
            branch: "bakedbean/magenta-nvim-context".into(),
            base_ref: "origin/main".into(),
            worktree: PathBuf::from("/tmp/wt"),
            agents: vec![
                agent(AgentKind::Claude, 1, true),
                agent(AgentKind::Claude, 2, false),
            ],
            status: Some(ReportedStatus {
                state: ReportedState::Working,
                message: Some("writing the spec".into()),
                source: "claude".into(),
                reported_at: 1_000_000 - 4 * 60 * 1000,
            }),
            recap: Some(WorkspaceRecap {
                goal: Some("Add wsx context".into()),
                state: Some("spec approved".into()),
                next: Some("write plan".into()),
                goal_short: Some("ctx".into()),
                state_short: None,
                next_short: None,
                updated_at: 0,
            }),
            commits: vec![
                "ff2a9ad fix(input): swallow Ctrl-D".into(),
                "801794a fix(status): stop Idle".into(),
            ],
            uncommitted: Some(crate::git::WorkspaceStatus {
                modified: 3,
                untracked: 1,
                ahead: 0,
                behind: 0,
            }),
            last_assistant_text: Some("I finished the spec.".into()),
            now_ms: 1_000_000,
        }
    }

    fn minimal() -> ContextDigest {
        ContextDigest {
            repo_name: "r".into(),
            workspace_name: "w".into(),
            branch: "b/w".into(),
            base_ref: "main".into(),
            worktree: PathBuf::from("/tmp/wt"),
            agents: vec![],
            status: None,
            recap: None,
            commits: vec![],
            uncommitted: None,
            last_assistant_text: None,
            now_ms: 0,
        }
    }

    #[test]
    fn full_digest_renders_every_section_in_order() {
        let out = render(&full());
        let idx = |needle: &str| out.find(needle).unwrap_or_else(|| panic!("missing {needle:?}\n{out}"));
        assert!(out.starts_with("# wsx workspace: workspacex/magenta-nvim-context\n"));
        assert!(out.contains("- branch: bakedbean/magenta-nvim-context (base: origin/main)\n"));
        assert!(out.contains("- worktree: /tmp/wt\n"));
        assert!(out.contains("- agents: claude (primary), claude#2\n"));
        assert!(out.contains("- status: working — \"writing the spec\" (claude, 4m ago)\n"));
        assert!(out.contains("## Recap\n\n- goal: Add wsx context\n- state: spec approved\n- next: write plan\n"));
        assert!(out.contains("## Recent commits (origin/main..HEAD)\n\n- ff2a9ad fix(input): swallow Ctrl-D\n- 801794a fix(status): stop Idle\n\nUncommitted: 3 modified, 1 untracked\n"));
        assert!(out.contains("## Primary agent's last message\n\nI finished the spec.\n"));
        assert!(out.contains("## External instructions\n"));
        assert!(out.contains("`wsx agent send claude \"<one-paragraph summary>\"`"));
        assert!(idx("## Recap") < idx("## Recent commits"));
        assert!(idx("## Recent commits") < idx("## Primary agent's last message"));
        assert!(idx("## Primary agent's last message") < idx("## External instructions"));
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn minimal_digest_omits_optional_sections_and_uses_dashes() {
        let out = render(&minimal());
        assert!(out.contains("- agents: -\n"));
        assert!(out.contains("- status: -\n"));
        assert!(!out.contains("## Recap"));
        assert!(!out.contains("## Recent commits"));
        assert!(!out.contains("Uncommitted:"));
        assert!(!out.contains("## Primary agent's last message"));
        assert!(out.contains("## External instructions\n"));
        // No primary → the generic `primary` address, which `wsx agent send` accepts.
        assert!(out.contains("`wsx agent send primary \"<one-paragraph summary>\"`"));
    }

    #[test]
    fn recap_missing_fields_render_dashes() {
        let mut d = minimal();
        d.recap = Some(WorkspaceRecap {
            goal: Some("g".into()),
            ..Default::default()
        });
        let out = render(&d);
        assert!(out.contains("- goal: g\n- state: -\n- next: -\n"));
    }

    #[test]
    fn status_without_message_omits_quote() {
        let mut d = minimal();
        d.status = Some(ReportedStatus {
            state: ReportedState::Blocked,
            message: None,
            source: "hook".into(),
            reported_at: 0,
        });
        d.now_ms = 2 * 3600 * 1000;
        assert!(render(&d).contains("- status: blocked (hook, 2h ago)\n"));
    }

    #[test]
    fn clean_tree_says_clean_and_git_failure_omits_line() {
        let mut d = full();
        d.uncommitted = Some(crate::git::WorkspaceStatus::default());
        assert!(render(&d).contains("Uncommitted: clean\n"));
        d.uncommitted = None;
        assert!(!render(&d).contains("Uncommitted:"));
    }

    #[test]
    fn commits_section_omitted_when_empty_even_with_uncommitted() {
        let mut d = full();
        d.commits.clear();
        let out = render(&d);
        assert!(!out.contains("## Recent commits"));
        assert!(!out.contains("Uncommitted:"));
    }

    #[test]
    fn last_message_truncates_on_char_boundary() {
        let mut d = full();
        // 1999 ASCII chars + one 4-byte char + more: the cut must land on a
        // char boundary, keep exactly MAX chars, and append the marker.
        let mut text = "a".repeat(MAX_LAST_MESSAGE_CHARS - 1);
        text.push('😀');
        text.push_str(&"b".repeat(50));
        d.last_assistant_text = Some(text);
        let out = render(&d);
        let section = out
            .split("## Primary agent's last message\n\n")
            .nth(1)
            .unwrap()
            .split("\n\n## External instructions")
            .next()
            .unwrap();
        assert!(section.ends_with("… [truncated]"), "{section}");
        let body = section.trim_end_matches("… [truncated]");
        assert_eq!(body.chars().count(), MAX_LAST_MESSAGE_CHARS);
        assert!(body.ends_with('😀'));
    }

    #[test]
    fn last_message_at_cap_is_not_truncated() {
        let mut d = full();
        d.last_assistant_text = Some("x".repeat(MAX_LAST_MESSAGE_CHARS));
        assert!(!render(&d).contains("[truncated]"));
    }

    #[test]
    fn instructions_never_name_an_editor() {
        let out = render(&full()).to_lowercase();
        assert!(!out.contains("neovim") && !out.contains("nvim") && !out.contains("magenta"));
    }

    #[test]
    fn format_age_buckets() {
        assert_eq!(format_age(10_000, 0), "10s ago");
        assert_eq!(format_age(4 * 60_000, 0), "4m ago");
        assert_eq!(format_age(2 * 3_600_000, 0), "2h ago");
        assert_eq!(format_age(3 * 86_400_000, 0), "3d ago");
        assert_eq!(format_age(0, 5_000), "0s ago"); // clock skew clamps to zero
    }

    #[test]
    fn digest_path_nests_repo_then_workspace() {
        let dir = tempfile::TempDir::new().unwrap();
        let dirs = Dirs::for_test(dir.path());
        assert_eq!(
            digest_path(&dirs, "workspacex", "cozy-primrose"),
            dir.path().join("wsx/context/workspacex/cozy-primrose.md")
        );
    }

    #[test]
    fn write_atomic_creates_parents_and_leaves_no_temp_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let target = dir.path().join("a/b/c.md");
        write_atomic(&target, "hello\n").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "hello\n");
        let names: Vec<_> = std::fs::read_dir(target.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, vec!["c.md"]);
        // Overwrite works too.
        write_atomic(&target, "again\n").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "again\n");
    }
}
```

Note: `AgentInstance` fields are `id`, `workspace_id`, `agent`, `ordinal`, `is_primary`, `session_ref`, `created_at` (see `src/data/agents.rs`). `AgentInstanceId` and `WorkspaceId` are newtype tuples exported from `src/data/store.rs`; if `AgentInstanceId` lives elsewhere, adjust the import to match the `use` line at the top of `src/data/agents.rs`.

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib commands::context::tests`
Expected: panics on `todo!()` (or compile errors if a field name differs — fix the test import, not the struct).

- [ ] **Step 4: Implement `render`, `format_age`, `digest_path`, `write_atomic`**

Replace the four `todo!()` bodies:

```rust
const INSTRUCTIONS_HEADER: &str = "## External instructions";

fn instructions(primary_label: &str) -> String {
    format!(
        "You are an editor-hosted agent working inside a wsx-managed worktree. The \
agents listed above share this branch and this working tree with you, and one \
of them (the primary) owns this workspace's status and recap.\n\
\n\
- Before editing, run `git status` and `git diff`; the primary agent may have \
changed files since this digest was written.\n\
- Keep edits small and scoped. Do not create branches, rename the workspace, or \
run `wsx status set` / `wsx recap set`; those belong to the primary agent.\n\
- When you finish a change, or when you need a decision the primary agent \
should make, report it with:\n  \
`wsx agent send {primary_label} \"<one-paragraph summary>\"`\n  \
Run it from this worktree; the workspace is resolved from cwd.\n\
- This file is regenerated by `wsx context write`; do not edit it.\n"
    )
}

/// Coarse age: `Ns ago` / `Nm ago` / `Nh ago` / `Nd ago`. Negative
/// differences (clock skew) clamp to zero.
pub fn format_age(now_ms: i64, then_ms: i64) -> String {
    let secs = (now_ms - then_ms).max(0) / 1000;
    match secs {
        s if s < 60 => format!("{s}s ago"),
        s if s < 3_600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3_600),
        s => format!("{}d ago", s / 86_400),
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push_str("… [truncated]");
    out
}

pub fn render(d: &ContextDigest) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# wsx workspace: {}/{}\n\n",
        d.repo_name, d.workspace_name
    ));
    out.push_str(&format!("- branch: {} (base: {})\n", d.branch, d.base_ref));
    out.push_str(&format!("- worktree: {}\n", d.worktree.display()));

    let agents = if d.agents.is_empty() {
        "-".to_string()
    } else {
        d.agents
            .iter()
            .map(|a| {
                if a.is_primary {
                    format!("{} (primary)", a.label())
                } else {
                    a.label()
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    out.push_str(&format!("- agents: {agents}\n"));

    let status = match &d.status {
        None => "-".to_string(),
        Some(s) => {
            let age = format_age(d.now_ms, s.reported_at);
            match s.message.as_deref().filter(|m| !m.trim().is_empty()) {
                Some(m) => format!("{} — \"{}\" ({}, {})", s.state.as_str(), m, s.source, age),
                None => format!("{} ({}, {})", s.state.as_str(), s.source, age),
            }
        }
    };
    out.push_str(&format!("- status: {status}\n"));

    if let Some(r) = &d.recap {
        let f = |v: &Option<String>| v.as_deref().unwrap_or("-").to_string();
        out.push_str("\n## Recap\n\n");
        out.push_str(&format!("- goal: {}\n", f(&r.goal)));
        out.push_str(&format!("- state: {}\n", f(&r.state)));
        out.push_str(&format!("- next: {}\n", f(&r.next)));
    }

    if !d.commits.is_empty() {
        out.push_str(&format!("\n## Recent commits ({}..HEAD)\n\n", d.base_ref));
        for line in d.commits.iter().take(MAX_COMMITS) {
            out.push_str(&format!("- {line}\n"));
        }
        if let Some(u) = &d.uncommitted {
            let summary = if u.modified == 0 && u.untracked == 0 {
                "clean".to_string()
            } else {
                format!("{} modified, {} untracked", u.modified, u.untracked)
            };
            out.push_str(&format!("\nUncommitted: {summary}\n"));
        }
    }

    if let Some(text) = d
        .last_assistant_text
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        out.push_str("\n## Primary agent's last message\n\n");
        out.push_str(&truncate_chars(text, MAX_LAST_MESSAGE_CHARS));
        out.push('\n');
    }

    let primary_label = d
        .agents
        .iter()
        .find(|a| a.is_primary)
        .map(|a| a.label())
        .unwrap_or_else(|| "primary".to_string());
    out.push_str(&format!("\n{INSTRUCTIONS_HEADER}\n\n"));
    out.push_str(&instructions(&primary_label));
    out
}

pub fn digest_path(dirs: &Dirs, repo_name: &str, workspace_name: &str) -> PathBuf {
    dirs.context_dir()
        .join(repo_name)
        .join(format!("{workspace_name}.md"))
}

/// Write via a sibling temp file + rename so a concurrent reader never
/// observes a partial digest.
pub fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "digest path has no parent")
    })?;
    std::fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("digest.md");
    let tmp = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, contents)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}
```

The `Uncommitted` line intentionally lives inside the commits section (the spec's example shows it there, and a tree with no commits ahead of base is the cold-start case where it adds little).

- [ ] **Step 5: Run tests**

Run: `cargo test --lib commands::context::tests`
Expected: all 12 pass. If `full_digest_renders_every_section_in_order` fails on the status line, check the em-dash and quote characters match exactly (`—` U+2014, ASCII `"`).

- [ ] **Step 6: Clippy and commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
git add src/commands/context.rs src/commands/mod.rs src/config/mod.rs
git commit -m "feat(context): add ContextDigest render with tests"
```

---

### Task 4: `gather` — fill the digest from store, git, and transcript

**Files:**
- Modify: `src/commands/context.rs`

**Interfaces:**
- Consumes: `crate::activity::locate_session_file_for`, `crate::activity::tail_session_for` (Task 1); `crate::git::log_oneline` (Task 2); `crate::git::{resolve_base_branch, workspace_status}`; `Store::{repos, workspace_agents, workspace_status, workspace_recap}`.
- Produces: `pub async fn gather(store: &Store, ws: &Workspace) -> Result<ContextDigest>`

- [ ] **Step 1: Write the failing integration test**

Append inside `mod tests` in `src/commands/context.rs`:

```rust
    /// Build a real repo with one commit on `main`, then a feature branch
    /// one commit ahead, and register it as a workspace with a recap, a
    /// status, and a primary agent. `gather` must reflect all of it and
    /// tolerate the missing transcript.
    #[tokio::test]
    async fn gather_reads_store_git_and_tolerates_missing_transcript() {
        use crate::data::store::{NewWorkspace, Store};
        use std::process::Command;

        let repo = tempfile::TempDir::new().unwrap();
        let git = |args: &[&str]| {
            let st = Command::new("git")
                .current_dir(repo.path())
                .args(args)
                .status()
                .unwrap();
            assert!(st.success(), "git {args:?}");
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.email", "t@example.com"]);
        git(&["config", "user.name", "T"]);
        git(&["commit", "--allow-empty", "-q", "-m", "init"]);
        git(&["checkout", "-q", "-b", "feat/x"]);
        git(&["commit", "--allow-empty", "-q", "-m", "do the thing"]);
        std::fs::write(repo.path().join("scratch.txt"), "x").unwrap();

        let store = Store::open_in_memory().unwrap();
        let repo_id = store.add_repo(repo.path(), "myrepo", "feat").unwrap();
        let ws_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "x",
                branch: "feat/x",
                worktree_path: repo.path(),
                yolo: false,
                agent: AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        store.add_primary_agent(ws_id, AgentKind::Claude, 0).unwrap();
        store.add_workspace_agent(ws_id, AgentKind::Codex).unwrap();
        store
            .set_workspace_recap(ws_id, Some("ship x"), None, Some("tests"), None, None, None)
            .unwrap();
        store
            .set_workspace_status(ws_id, ReportedState::Working, Some("on it"), "claude")
            .unwrap();
        let ws = store.workspace_by_id(ws_id).unwrap().unwrap();

        let d = gather(&store, &ws).await.unwrap();
        assert_eq!(d.repo_name, "myrepo");
        assert_eq!(d.workspace_name, "x");
        assert_eq!(d.branch, "feat/x");
        assert_eq!(d.base_ref, "main"); // no origin → fallback
        assert_eq!(d.worktree, repo.path());
        let labels: Vec<_> = d.agents.iter().map(|a| a.label()).collect();
        assert_eq!(labels, vec!["claude", "codex"]);
        assert!(d.agents[0].is_primary);
        assert_eq!(d.status.as_ref().unwrap().message.as_deref(), Some("on it"));
        assert_eq!(d.recap.as_ref().unwrap().goal.as_deref(), Some("ship x"));
        assert_eq!(d.commits.len(), 1);
        assert!(d.commits[0].ends_with(" do the thing"));
        assert_eq!(d.uncommitted.unwrap().untracked, 1);
        assert!(d.last_assistant_text.is_none());
        assert!(d.now_ms > 0);
    }

    /// A worktree that no longer exists on disk must still produce a
    /// digest from the store alone.
    #[tokio::test]
    async fn gather_survives_missing_worktree() {
        use crate::data::store::{NewWorkspace, Store};
        let store = Store::open_in_memory().unwrap();
        let repo_id = store.add_repo(Path::new("/tmp/nope"), "r", "").unwrap();
        let ws_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "w",
                branch: "b/w",
                worktree_path: Path::new("/nonexistent/wsx-test/wt"),
                yolo: false,
                agent: AgentKind::Pi,
                shared: false,
            })
            .unwrap();
        let ws = store.workspace_by_id(ws_id).unwrap().unwrap();
        let d = gather(&store, &ws).await.unwrap();
        assert!(d.commits.is_empty());
        assert!(d.uncommitted.is_none());
        assert!(d.last_assistant_text.is_none());
        assert!(d.agents.is_empty());
        assert!(d.status.is_none());
        assert!(d.recap.is_none());
        let out = render(&d);
        assert!(out.contains("- agents: -\n"));
    }
```

Check `set_workspace_recap`'s parameter order in `src/data/recap.rs:16` before running; the six `Option<&str>` args are goal, state, next, goal_short, state_short, next_short.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib commands::context::tests::gather`
Expected: compile error, `gather` not found.

- [ ] **Step 3: Implement `gather`**

Add to `src/commands/context.rs` above `render`:

```rust
/// Fill a digest for `ws`. Store reads propagate errors (the DB is the
/// one thing we cannot render without); git and transcript failures
/// degrade to `None` / empty so the digest never fails on a dirty or
/// missing worktree.
pub async fn gather(store: &Store, ws: &Workspace) -> Result<ContextDigest> {
    let repo_name = store
        .repos()?
        .into_iter()
        .find(|r| r.id == ws.repo_id)
        .map(|r| r.name)
        .unwrap_or_else(|| "?".to_string());
    let agents = store.workspace_agents(ws.id)?;
    let status = store.workspace_status(ws.id)?;
    let recap = store.workspace_recap(ws.id)?;

    let worktree = ws.worktree_path.clone();
    let worktree_exists = worktree.is_dir();

    let base_ref = if worktree_exists {
        crate::git::resolve_base_branch(&worktree).await
    } else {
        "main".to_string()
    };
    let commits = if worktree_exists {
        crate::git::log_oneline(&worktree, &base_ref, MAX_COMMITS)
            .await
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let uncommitted = if worktree_exists {
        crate::git::workspace_status(&worktree).await.ok()
    } else {
        None
    };

    let primary_kind = agents
        .iter()
        .find(|a| a.is_primary)
        .map(|a| a.agent)
        .unwrap_or(ws.agent);
    let last_assistant_text = if worktree_exists {
        last_assistant_text(primary_kind, &worktree)
    } else {
        None
    };

    Ok(ContextDigest {
        repo_name,
        workspace_name: ws.name.clone(),
        branch: ws.branch.clone(),
        base_ref,
        worktree,
        agents,
        status,
        recap,
        commits,
        uncommitted,
        last_assistant_text,
        now_ms: crate::data::store::now_ms(),
    })
}

/// Whole-transcript scan for the primary agent's most recent assistant
/// text. Any failure (no session, parse error) is `None`; the digest is
/// advisory and must not fail because a log is mid-write.
fn last_assistant_text(kind: crate::pty::session::AgentKind, worktree: &Path) -> Option<String> {
    let file = crate::activity::locate_session_file_for(kind, worktree)?;
    match crate::activity::tail_session_for(kind, &file, 0) {
        Ok(update) => update.last_assistant_text,
        Err(e) => {
            tracing::debug!(?file, error = %e, "context digest: transcript tail failed");
            None
        }
    }
}
```

`now_ms` in `src/data/store.rs:389` is `pub(crate)`, so the call above compiles. `tracing` is already a dependency (see `src/config/mod.rs` log dir comments); if the `debug!` macro isn't imported, use the fully-qualified `tracing::debug!` as written.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib commands::context`
Expected: all pass (14 tests).

- [ ] **Step 5: Clippy and commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
git add src/commands/context.rs
git commit -m "feat(context): gather digest from store, git, and agent transcript"
```

---

### Task 5: CLI wiring — `wsx context show|write`

**Files:**
- Modify: `src/cli/action.rs:186-188`
- Modify: `src/cli/parse/reporting.rs` (append)
- Modify: `src/cli/parse/mod.rs:103`
- Modify: `src/cli/groups.rs:213-231`
- Modify: `src/cli/run.rs:732-745`
- Modify: `src/cli/tests.rs:1465-1478` and append parser tests

**Interfaces:**
- Consumes: `crate::commands::context::{gather, render, digest_path, write_atomic}` (Tasks 3–4); `resolve_current_workspace` (`src/cli/resolve.rs`).
- Produces: `CliAction::ContextShow`, `CliAction::ContextWrite`; `parse_context`.

- [ ] **Step 1: Write the failing parser tests**

In `src/cli/tests.rs`, add `"context"` to the `dispatched` array in `registry_matches_dispatched_groups` (after `"recap"`), and append:

```rust
#[test]
fn parses_context_show_and_write() {
    assert!(matches!(
        parse(&["context", "show"]).unwrap(),
        CliAction::ContextShow
    ));
    assert!(matches!(
        parse(&["context", "write"]).unwrap(),
        CliAction::ContextWrite
    ));
}

#[test]
fn context_rejects_missing_unknown_and_trailing_args() {
    assert!(parse(&["context"]).is_err());
    assert!(parse(&["context", "bogus"]).is_err());
    assert!(parse(&["context", "show", "extra"]).is_err());
    assert!(parse(&["context", "write", "--workspace", "r/s"]).is_err());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib cli::tests::parses_context cli::tests::context_rejects cli::tests::registry_matches`
Expected: compile error on `CliAction::ContextShow`.

- [ ] **Step 3: Add the actions**

`src/cli/action.rs`, after `RecapClear,`:

```rust
    /// `wsx context show` — print the workspace context digest.
    ContextShow,
    /// `wsx context write` — write the digest under the state dir, print its path.
    ContextWrite,
```

- [ ] **Step 4: Add the parser**

Append to `src/cli/parse/reporting.rs`:

```rust
pub(in crate::cli) fn parse_context(it: &mut Args) -> Result<CliAction> {
    let action = match it.next().as_deref() {
        Some("show") => CliAction::ContextShow,
        Some("write") => CliAction::ContextWrite,
        other => {
            return Err(Error::Usage {
                group: None,
                msg: format!(
                    "unknown context subcommand: {} (usage: wsx context <show|write>)",
                    other.unwrap_or("(none)")
                ),
            });
        }
    };
    if let Some(extra) = it.next() {
        return Err(Error::Usage {
            group: None,
            msg: format!("unexpected argument: {extra} (usage: wsx context <show|write>)"),
        });
    }
    Ok(action)
}
```

Update the file's module doc comment first line to mention `wsx context` alongside status and recap.

`src/cli/parse/mod.rs`: extend the `use reporting::{parse_recap, parse_status};` line to include `parse_context`, and add after the `"recap"` arm:

```rust
        "context" => parse_context(&mut it).map_err(|e| tag_group(e, group)),
```

- [ ] **Step 5: Register the group**

`src/cli/groups.rs`, after the `recap` `GroupInfo`:

```rust
    GroupInfo {
        name: "context",
        blurb: "Workspace context digest for editor-hosted agents",
        commands: &[
            CmdInfo {
                usage: "show",
                blurb: "Print the digest (branch, agents, status, recap, recent commits, \
                        primary agent's last message, instructions for an external agent)",
            },
            CmdInfo {
                usage: "write",
                blurb: "Write the digest to <state>/wsx/context/<repo>/<workspace>.md and print the path",
            },
        ],
    },
```

- [ ] **Step 6: Dispatch**

`src/cli/run.rs`, after the `CliAction::RecapClear` arm:

```rust
        CliAction::ContextShow => {
            let ws = resolve_current_workspace(&store)?;
            let digest = crate::commands::context::gather(&store, &ws).await?;
            print!("{}", crate::commands::context::render(&digest));
        }
        CliAction::ContextWrite => {
            let ws = resolve_current_workspace(&store)?;
            let digest = crate::commands::context::gather(&store, &ws).await?;
            let path = crate::commands::context::digest_path(
                dirs,
                &digest.repo_name,
                &digest.workspace_name,
            );
            crate::commands::context::write_atomic(&path, &crate::commands::context::render(&digest))?;
            println!("{}", path.display());
        }
```

- [ ] **Step 7: Run the CLI tests and the whole suite**

Run: `cargo test --lib cli:: && cargo test`
Expected: the two new tests and `registry_matches_dispatched_groups` pass; full suite green. If a help-rendering snapshot test fails because the group list changed, update the expected text to include the `context` group in the same position as the registry (after `recap`).

- [ ] **Step 8: Manual check from this worktree**

Run:
```bash
cargo run -q -- context show | head -20
cargo run -q -- context write
cat "$(cargo run -q -- context write)" | diff - <(cargo run -q -- context show) && echo identical
```
Expected: a digest for `workspacex/magenta-nvim-context` with the `## Recap` you set earlier, a commits list, your own last message under "Primary agent's last message", and `identical`.

- [ ] **Step 9: Clippy and commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
git add src/cli
git commit -m "feat(cli): add wsx context show/write"
```

---

### Task 6: Doctrine clause + skill section

**Files:**
- Modify: `src/agent/doctrine.rs:68-120`
- Modify: `skills/wsx/SKILL.md:198-229`

**Interfaces:**
- Produces: `CLAUSE_EXTERNAL_EDITOR` appended to the clause list in `process_doctrine`.

- [ ] **Step 1: Write the failing test**

In `src/agent/doctrine.rs` `mod tests`, add:

```rust
    #[test]
    fn doctrine_warns_about_editor_hosted_agents() {
        for agent in AgentKind::ALL {
            let d = process_doctrine(agent).to_lowercase();
            assert!(
                d.contains("editor-hosted agent"),
                "{agent:?} must be told about editor-hosted agents: {d}"
            );
            assert!(
                d.contains("wsx context write"),
                "{agent:?} must be told where the digest comes from: {d}"
            );
            assert!(
                d.contains("git status"),
                "{agent:?} must be told to re-check the tree: {d}"
            );
        }
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib agent::doctrine::tests::doctrine_warns_about_editor_hosted_agents`
Expected: FAIL, assertion on "editor-hosted agent".

- [ ] **Step 3: Add the clause**

After `CLAUSE_RECAP` in `src/agent/doctrine.rs`:

```rust
const CLAUSE_EXTERNAL_EDITOR: &str = "- An editor-hosted agent (one running inside the \
    user's editor rather than in a wsx session) may work in this worktree alongside \
    you. It reads a digest of your recap and status produced by `wsx context write`, \
    shares this branch and working tree, and reports back to you with `wsx agent \
    send` — its messages arrive with a bare `[message]` banner and no sender label. \
    Before assuming the tree matches your last edit, check `git status` and `git \
    diff`, and treat those messages as the user's own follow-up instructions.";
```

Add `CLAUSE_EXTERNAL_EDITOR,` as the last element of the `clauses` array in `process_doctrine`.

- [ ] **Step 4: Run doctrine tests**

Run: `cargo test --lib agent::doctrine`
Expected: all pass, including `doctrine_names_no_third_party_skill_bundle`.

- [ ] **Step 5: Add the skill section**

In `skills/wsx/SKILL.md`, insert before `## When NOT to use`:

```markdown
## External editor agents

The user may open this worktree in an editor whose own AI agent (for example
magenta.nvim in neovim) shares your branch and working tree. That agent
reads a digest produced by `wsx context write` — your recap, status, peers,
recent commits, and your last message — so it already knows what you are
doing. It does not set status or recap; it reports back with
`wsx agent send <your label> "<summary>"`, which reaches you as a bare
`[message]` banner with no sender label.

Treat those messages as the user's follow-up instructions, and run
`git status` / `git diff` before assuming the tree matches your last edit.
You can inspect the same digest yourself with `wsx context show`.
```

Also add `show` / `write` to the "CLI surface" block near the top of the skill, after the `wsx recap` lines:

```
wsx context show                            # markdown digest of this workspace (for editor-hosted agents)
wsx context write                           # same, written under the state dir; prints the path
```

The embedded skill is installed by `wsx setup install-skill` (`src/agent/skill.rs`); if a test there asserts on the skill's byte length or a heading list, update it.

- [ ] **Step 6: Run the full suite and commit**

```bash
cargo fmt
cargo test
cargo clippy --all-targets -- -D warnings
git add src/agent/doctrine.rs skills/wsx/SKILL.md
git commit -m "feat(doctrine): tell agents an editor-hosted agent may share the worktree"
```

---

### Task 7: Docs page, links, storage reference, and the user's nvim snippet

**Files:**
- Create: `docs/book/src/integrations/editor-agent-context.md`
- Modify: `docs/book/src/SUMMARY.md:27-35`
- Modify: `docs/book/src/integrations/index.md`
- Modify: `docs/book/src/integrations/editor-terminal-diff.md` (append a cross-link)
- Modify: `docs/book/src/reference/storage-and-config-files.md` (table row)
- Modify (outside repo): `~/.config/nvim/lua/polish.lua`

- [ ] **Step 1: Write the docs page**

Create `docs/book/src/integrations/editor-agent-context.md`:

````markdown
# Editor-hosted agent context

When you open a workspace's worktree in your editor and use an AI agent that
lives there (magenta.nvim, Cursor, a VS Code extension), that agent has no
idea what wsx knows: the workspace's goal, the status its primary agent last
reported, which peer agents exist, or what the primary agent was just doing.

`wsx context` closes that gap with one markdown file the editor can hand to
its agent as context.

```
wsx context show     # print the digest
wsx context write    # write it to $XDG_STATE_HOME/wsx/context/<repo>/<workspace>.md and print the path
```

Both resolve the workspace from the current directory (or `WSX_WORKSPACE_ID`
when set), so they work from any shell or editor opened inside the worktree.
`write` replaces the file atomically; a reader never sees a partial digest.

## What the digest contains

In order:

- repo/workspace name, branch and base ref, worktree path
- attached agents, primary marked `(primary)`
- the last pushed status (`working — "message" (source, 4m ago)`)
- the recap (goal / state / next)
- `git log --oneline <base>..HEAD` (up to 20) and an uncommitted-changes line
- the primary agent's last assistant message, from its session transcript
  (Claude Code, Pi, Hermes, Codex, and oh-my-pi are all supported), capped
  at 2000 characters
- an **External instructions** block

Sections with no data are omitted. Git and transcript problems never fail
the command; only an unresolvable workspace or an unwritable file does.

## External instructions

The digest ends with this block, addressed to the editor-hosted agent:

> You are an editor-hosted agent working inside a wsx-managed worktree. The
> agents listed above share this branch and this working tree with you, and
> one of them (the primary) owns this workspace's status and recap.
>
> - Before editing, run `git status` and `git diff`; the primary agent may
>   have changed files since this digest was written.
> - Keep edits small and scoped. Do not create branches, rename the
>   workspace, or run `wsx status set` / `wsx recap set`; those belong to
>   the primary agent.
> - When you finish a change, or when you need a decision the primary agent
>   should make, report it with:
>   `wsx agent send <primary label> "<one-paragraph summary>"`
>   Run it from this worktree; the workspace is resolved from cwd.
> - This file is regenerated by `wsx context write`; do not edit it.

`wsx agent send` from an editor shell carries no `WSX_AGENT_INSTANCE_ID`,
so the message reaches the primary agent with a bare `[message]` banner.

## The other direction

wsx-spawned agents get a matching doctrine clause (see
[Coding agents](../configuration/coding-agents.md)): an editor-hosted agent
may share the worktree, it reads this digest, and its messages arrive
unlabelled. They are told to re-check `git status` before assuming the tree
is theirs. The bundled wsx skill repeats the same guidance.

## neovim + magenta.nvim

magenta.nvim re-reads every context file before each request and ignores
repeat additions of the same path, so a file that wsx keeps fresh is live
context. Add this to your neovim config:

```lua
-- wsx: keep the workspace context digest fresh and hand it to magenta.nvim
local worktrees = vim.fn.expand("~/.local/state/wsx/worktrees/")
local added = false

local function magenta_sidebar_visible()
  for _, win in ipairs(vim.api.nvim_list_wins()) do
    local name = vim.api.nvim_buf_get_name(vim.api.nvim_win_get_buf(win))
    if name:find("Magenta Input", 1, true) then return true end
  end
  return false
end

local function wsx_context()
  if not vim.startswith(vim.fn.getcwd(), worktrees) then return end
  vim.system({ "wsx", "context", "write" }, { text = true }, function(out)
    if out.code ~= 0 then return end
    local path = vim.trim(out.stdout)
    if added or path == "" then return end
    vim.schedule(function()
      if magenta_sidebar_visible() then
        vim.cmd("Magenta context-files " .. vim.fn.fnameescape(path))
        added = true
      end
    end)
  end)
end

vim.api.nvim_create_autocmd({ "VimEnter", "FocusGained" }, { callback = wsx_context })
vim.api.nvim_create_user_command("WsxContext", function()
  added = false
  wsx_context()
end, {})
```

Behaviour:

- The file is rewritten on every `VimEnter` and `FocusGained` — one sqlite
  read, two git commands, one transcript scan.
- It is added to magenta once, and only when the sidebar is already open,
  because `:Magenta context-files` force-opens the sidebar otherwise.
- `:WsxContext` forces a rewrite and re-add.
- Later rewrites reach the agent without further action, since magenta
  diffs tracked files before each request.

If `$XDG_STATE_HOME` is set, change the `worktrees` path to match.

## Other editors

Any editor agent that can read a file on disk can use the digest: run
`wsx context write` on focus (or on a timer) and point the agent at the
printed path. The External instructions block is the contract; nothing in
the file is editor-specific.
````

- [ ] **Step 2: Link it**

`docs/book/src/SUMMARY.md`: after the `Editor, terminal, and diff integration` line add

```markdown
  - [Editor-hosted agent context](integrations/editor-agent-context.md)
```

`docs/book/src/integrations/index.md`: change the sentence to

```markdown
Editor/terminal/diff hooks, the context digest for editor-hosted agents, remote access and control, MCP inheritance, and the bundled agent skill.
```

`docs/book/src/integrations/editor-terminal-diff.md`: append at the end

```markdown
## Giving your editor's agent wsx context

If the editor you open has its own AI agent, see
[Editor-hosted agent context](editor-agent-context.md) for `wsx context
write`, which renders the workspace's recap, status, peers, and the primary
agent's last message into a file that agent can read.
```

`docs/book/src/reference/storage-and-config-files.md`: add a row after the `worktrees` row:

```markdown
| `$XDG_STATE_HOME/wsx/context/<repo>/<workspace>.md`  | Workspace context digest written by `wsx context write` for editor-hosted agents                        |
```

Check `docs/book/src/configuration/coding-agents.md` for a list of doctrine clauses; if it enumerates them, add one bullet: "an editor-hosted agent may share the worktree; re-check `git status` and treat its unlabelled messages as the user's instructions."

- [ ] **Step 3: Build the book if mdbook is installed**

Run: `command -v mdbook && (cd docs/book && mdbook build) || echo "mdbook not installed; skipped"`
Expected: builds with no missing-link warnings, or the skip message.

- [ ] **Step 4: Commit**

```bash
git add docs/book/src
git commit -m "docs: editor-hosted agent context page, nvim snippet, storage reference"
```

- [ ] **Step 5: Install the snippet in the user's nvim config**

Append the Lua block from Step 1 (from `-- wsx: keep the workspace context digest fresh` through the `WsxContext` command) to `~/.config/nvim/lua/polish.lua`. This file is outside the repo; do not commit it. Verify:

Run: `nvim --headless "+lua print(vim.fn.exists(':WsxContext'))" +q 2>&1 | tail -1`
Expected: `2` (command exists).

- [ ] **Step 6: End-to-end manual verification**

From this worktree, with the wsx dashboard running:

1. `wsx context show` prints the digest with this workspace's recap.
2. Open nvim from the dashboard (`e`). Toggle the magenta sidebar, then `:WsxContext`. Confirm the digest path appears in magenta's context list.
3. Ask the magenta agent "what is this workspace's goal?" and confirm it answers from the recap.
4. In the agent's session, confirm `wsx agent send claude "test from editor"` arrives as a `[message]` banner.

Record the outcome of each step in the final report to the user.

- [ ] **Step 7: Update wsx status**

```bash
wsx status set done --message "wsx context show/write, doctrine clause, docs and nvim snippet landed"
wsx recap set --state "all 7 tasks committed; manual e2e verified" --state-short "done, e2e verified" --next "open PR" --next-short "open PR"
```
