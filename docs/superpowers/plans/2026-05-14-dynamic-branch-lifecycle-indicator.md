# Dynamic Branch Lifecycle Indicator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the static Powerline branch glyph next to each workspace's branch name with an icon that reflects the branch's GitHub PR state (no PR, draft, open, merged, closed).

**Architecture:** A new `forge` module shells out to the `gh` CLI to fetch PR state per branch. The existing 2s `branch_drift_poll` loop in `app.rs` gains a per-workspace throttle (30s) for the PR fetch — that piggybacks on a loop already iterating every workspace, so no new task is needed. Results are cached on `App` keyed by `WorkspaceId` as `Option<BranchLifecycle>` (None means "we haven't checked yet"). The dashboard's `format_branch_label` consumes the cached lifecycle to pick a glyph; missing `gh`, network errors, or non-GitHub remotes degrade silently to the existing static glyph.

**Tech Stack:** Rust, tokio, serde_json (all already in `Cargo.toml`), `gh` CLI (external dependency, degrades gracefully when absent).

---

## File Structure

- **Create** `src/forge.rs` — `BranchLifecycle` enum, `gh` JSON parser, async `fetch_branch_lifecycle` function.
- **Modify** `src/lib.rs` — add `pub mod forge;`.
- **Modify** `src/app.rs` — add `pr_lifecycle` and `pr_last_poll_ms` cache fields on `App`; plug the fetch into the existing `branch_drift_poll` loop with a 30s throttle.
- **Modify** `src/ui/dashboard.rs` — extend `Item::Workspace` with `lifecycle: Option<crate::forge::BranchLifecycle>`; thread it from `app.rs:281` into the items vec; update `format_branch_label` to take the lifecycle and pick the glyph.

Behaviour summary:

| State | Source | Nerd-font glyph | ASCII fallback |
|---|---|---|---|
| `Unknown` (gh missing, error, not a GH remote) | fetch failed | `\u{e0a0}` (existing branch glyph) | (no annotation) |
| `NoPr` | gh: "no pull requests found" | `\u{e0a0}` | (no annotation) |
| `PrDraft` | gh: `state=OPEN`, `isDraft=true` | `\u{f407}` + ` draft` | ` (draft)` |
| `PrOpen` | gh: `state=OPEN`, `isDraft=false` | `\u{f407}` | ` (pr)` |
| `PrMerged` | gh: `state=MERGED` | `\u{f419}` | ` (merged)` |
| `PrClosed` | gh: `state=CLOSED` | `\u{f659}` | ` (closed)` |

> **Glyph note:** the codepoints above are from Nerd Fonts (oct-git-pull-request `f407`, oct-git-merge `f419`, oct-x `f659`). Treat them as the initial pick — if your installed Nerd Font shows tofu, the engineer running the plan can substitute equivalents from the same range.

---

### Task 1: Add `forge` module scaffolding with `BranchLifecycle` and parser

**Files:**
- Create: `src/forge.rs`
- Modify: `src/lib.rs`
- Test: inline `#[cfg(test)] mod tests` inside `src/forge.rs`

- [ ] **Step 1: Write the failing tests for the parser**

Create `src/forge.rs` with the type, a parser stub, and tests:

```rust
use crate::error::{Error, Result};
use serde::Deserialize;
use std::path::Path;
use tokio::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchLifecycle {
    NoPr,
    PrDraft,
    PrOpen,
    PrMerged,
    PrClosed,
}

#[derive(Debug, Deserialize)]
struct GhPrView {
    state: String,
    #[serde(rename = "isDraft", default)]
    is_draft: bool,
}

/// Parse the JSON returned by `gh pr view <branch> --json state,isDraft`.
/// Returns the lifecycle variant for a known PR, or `None` if the JSON is
/// missing or unparseable (callers treat unknown as "no info").
pub(crate) fn parse_gh_pr_view(stdout: &str) -> Option<BranchLifecycle> {
    let parsed: GhPrView = serde_json::from_str(stdout.trim()).ok()?;
    match parsed.state.as_str() {
        "OPEN" if parsed.is_draft => Some(BranchLifecycle::PrDraft),
        "OPEN" => Some(BranchLifecycle::PrOpen),
        "MERGED" => Some(BranchLifecycle::PrMerged),
        "CLOSED" => Some(BranchLifecycle::PrClosed),
        _ => None,
    }
}

/// Heuristic: `gh pr view` exits 1 with a stderr line like
/// `no pull requests found for branch "foo"` when the branch has no PR.
/// This is distinct from auth errors, network errors, or "no remote".
pub(crate) fn stderr_means_no_pr(stderr: &str) -> bool {
    stderr.contains("no pull requests found")
}

pub async fn fetch_branch_lifecycle(
    _worktree: &Path,
    _branch: &str,
) -> Result<Option<BranchLifecycle>> {
    // Implemented in Task 2.
    Err(Error::Git("not implemented".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_open_pr() {
        let json = r#"{"state":"OPEN","isDraft":false}"#;
        assert_eq!(parse_gh_pr_view(json), Some(BranchLifecycle::PrOpen));
    }

    #[test]
    fn parses_draft_pr() {
        let json = r#"{"state":"OPEN","isDraft":true}"#;
        assert_eq!(parse_gh_pr_view(json), Some(BranchLifecycle::PrDraft));
    }

    #[test]
    fn parses_merged_pr() {
        let json = r#"{"state":"MERGED","isDraft":false}"#;
        assert_eq!(parse_gh_pr_view(json), Some(BranchLifecycle::PrMerged));
    }

    #[test]
    fn parses_closed_pr() {
        let json = r#"{"state":"CLOSED","isDraft":false}"#;
        assert_eq!(parse_gh_pr_view(json), Some(BranchLifecycle::PrClosed));
    }

    #[test]
    fn parser_returns_none_for_garbage() {
        assert_eq!(parse_gh_pr_view("not json"), None);
        assert_eq!(parse_gh_pr_view(""), None);
        assert_eq!(parse_gh_pr_view(r#"{"state":"WAT"}"#), None);
    }

    #[test]
    fn stderr_no_pr_heuristic() {
        assert!(stderr_means_no_pr(
            r#"no pull requests found for branch "foo""#
        ));
        assert!(!stderr_means_no_pr("error: not authenticated"));
        assert!(!stderr_means_no_pr(""));
    }
}
```

Then add the module to `src/lib.rs` — insert `pub mod forge;` after `pub mod external;`:

```rust
pub mod app;
pub mod cli;
pub mod config;
pub mod error;
pub mod events;
pub mod pm;
pub mod external;
pub mod forge;
pub mod git;
pub mod names;
pub mod pty;
pub mod repo;
pub mod setup;
pub mod store;
pub mod ui;
pub mod workspace;
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --lib forge::`
Expected: 6 tests pass (`parses_open_pr`, `parses_draft_pr`, `parses_merged_pr`, `parses_closed_pr`, `parser_returns_none_for_garbage`, `stderr_no_pr_heuristic`).

- [ ] **Step 3: Commit**

```bash
git add src/forge.rs src/lib.rs
git commit -m "feat(forge): BranchLifecycle enum + gh pr view JSON parser"
```

---

### Task 2: Implement `fetch_branch_lifecycle` against the real `gh` CLI

**Files:**
- Modify: `src/forge.rs`
- Test: inline `#[cfg(test)] mod tests` in `src/forge.rs` (gated `#[ignore]` for the real-gh test)

- [ ] **Step 1: Write the failing test**

Append to the existing `#[cfg(test)] mod tests` in `src/forge.rs`:

```rust
    /// Sanity check that fetch handles a non-git path gracefully.
    /// Should not panic; should return Ok(None) (treated as "unknown").
    #[tokio::test]
    async fn fetch_returns_none_on_non_git_path() {
        let tmp = tempfile::TempDir::new().unwrap();
        let result = fetch_branch_lifecycle(tmp.path(), "main").await;
        assert!(matches!(result, Ok(None)), "got {result:?}");
    }
```

Run: `cargo test --lib forge::fetch_returns_none_on_non_git_path`
Expected: FAIL — current stub returns `Err`.

- [ ] **Step 2: Implement `fetch_branch_lifecycle`**

Replace the stub in `src/forge.rs` with:

```rust
pub async fn fetch_branch_lifecycle(
    worktree: &Path,
    branch: &str,
) -> Result<Option<BranchLifecycle>> {
    let out = Command::new("gh")
        .current_dir(worktree)
        .args([
            "pr",
            "view",
            branch,
            "--json",
            "state,isDraft",
        ])
        .output()
        .await;

    let out = match out {
        Ok(o) => o,
        // gh not installed, not on PATH, permission error, etc. — degrade.
        Err(_) => return Ok(None),
    };

    if out.status.success() {
        let stdout = String::from_utf8_lossy(&out.stdout);
        return Ok(parse_gh_pr_view(&stdout));
    }

    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr_means_no_pr(&stderr) {
        return Ok(Some(BranchLifecycle::NoPr));
    }

    // Auth failure, non-GitHub remote, network blip — degrade.
    Ok(None)
}
```

Note: `tempfile` is already a dev-dependency (used by `src/git.rs` tests at line 113). No `Cargo.toml` change needed.

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test --lib forge::`
Expected: all 7 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/forge.rs
git commit -m "feat(forge): fetch_branch_lifecycle shells out to gh pr view"
```

---

### Task 3: Cache PR lifecycle on `App` state

**Files:**
- Modify: `src/app.rs` (struct definition near line 85, `new()` initializer near line 120)

- [ ] **Step 1: Add the cache fields to `App`**

In `src/app.rs`, the `App` struct currently ends with (around line 85-98):

```rust
    pub workspace_status:
        std::collections::HashMap<crate::store::WorkspaceId, crate::git::WorkspaceStatus>,
    pub workspace_events:
        std::collections::HashMap<crate::store::WorkspaceId, crate::events::WorkspaceEvents>,
    /// Per-workspace tracking for attention-alert state.
    pub workspace_activity: std::collections::HashMap<crate::store::WorkspaceId, ActivityState>,
    /// Workspaces whose alert hasn't been acknowledged (cleared on attach).
    pub workspace_needs_attention: std::collections::HashSet<crate::store::WorkspaceId>,
```

Insert two new fields immediately after `workspace_status`:

```rust
    pub workspace_status:
        std::collections::HashMap<crate::store::WorkspaceId, crate::git::WorkspaceStatus>,
    /// Cached PR lifecycle per workspace. Absent key = never polled; present
    /// key = last successful poll's result.
    pub pr_lifecycle:
        std::collections::HashMap<crate::store::WorkspaceId, crate::forge::BranchLifecycle>,
    /// Last epoch-ms we attempted a PR fetch per workspace (throttle key).
    pub pr_last_poll_ms: std::collections::HashMap<crate::store::WorkspaceId, i64>,
    pub workspace_events:
        std::collections::HashMap<crate::store::WorkspaceId, crate::events::WorkspaceEvents>,
```

- [ ] **Step 2: Initialize the new fields in `App::new`**

In `App::new` (around line 108-125), the initializer currently contains:

```rust
            workspace_status: std::collections::HashMap::new(),
            workspace_events: std::collections::HashMap::new(),
```

Update to:

```rust
            workspace_status: std::collections::HashMap::new(),
            pr_lifecycle: std::collections::HashMap::new(),
            pr_last_poll_ms: std::collections::HashMap::new(),
            workspace_events: std::collections::HashMap::new(),
```

- [ ] **Step 3: Verify the project still builds**

Run: `cargo build`
Expected: clean build (the fields are not yet read).

Run: `cargo test --lib`
Expected: all existing tests still pass.

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): cache PR lifecycle + last-poll timestamp on App"
```

---

### Task 4: Poll PR lifecycle in the existing branch-drift loop with 30s throttle

**Files:**
- Modify: `src/app.rs` (the `branch_drift_poll` function, around lines 1029-1071)

- [ ] **Step 1: Add the lifecycle fetch to the poll loop**

The existing `branch_drift_poll` ticks every 2s and already iterates every workspace, fetching `current_branch` and `workspace_status`. We add a third per-workspace step that respects a 30s throttle per workspace.

After the existing `workspace_status` block (lines 1067-1071 in the current code):

```rust
            // 2) Workspace status — refresh the cache for this workspace.
            if let Ok(status) = crate::git::workspace_status(&path).await {
                let mut g = app.lock().await;
                g.workspace_status.insert(id, status);
            }
```

Insert a new block immediately after it:

```rust
            // 3) PR lifecycle — throttled to once per 30s per workspace.
            //    gh is a network call, so we don't run it every tick.
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let should_poll_pr = {
                let g = app.lock().await;
                g.pr_last_poll_ms
                    .get(&id)
                    .map(|t| now_ms.saturating_sub(*t) >= 30_000)
                    .unwrap_or(true)
            };
            if should_poll_pr {
                // Mark the attempt before awaiting the fetch, so concurrent
                // ticks don't queue up multiple gh processes.
                {
                    let mut g = app.lock().await;
                    g.pr_last_poll_ms.insert(id, now_ms);
                }
                if let Ok(Some(lifecycle)) =
                    crate::forge::fetch_branch_lifecycle(&path, &db_branch).await
                {
                    let mut g = app.lock().await;
                    g.pr_lifecycle.insert(id, lifecycle);
                }
                // Ok(None) → leave any existing cached value alone; better
                // than clobbering a previously-known state on a transient
                // network error.
            }
```

Note: `db_branch` is the third element of the tuple destructured from `snapshot` near line 1041:

```rust
        let snapshot: Vec<(WorkspaceId, std::path::PathBuf, String, String)> = {
```

so it's already in scope inside the loop body. (Re-read lines 1033-1066 if the destructuring pattern looks unfamiliar; you don't need to change it.)

- [ ] **Step 2: Renumber the existing comment**

The existing comment at the start of the JSONL block (around line 1073, currently `// 3) Tail Claude Code session JSONL …`) should become `// 4) Tail Claude Code session JSONL …` since PR lifecycle is now step 3.

- [ ] **Step 3: Verify the project still builds**

Run: `cargo build`
Expected: clean build.

Run: `cargo test --lib`
Expected: all existing tests still pass.

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): poll PR lifecycle in branch_drift_poll (30s throttle)"
```

---

### Task 5: Pipe lifecycle into the dashboard item and update `format_branch_label`

**Files:**
- Modify: `src/ui/dashboard.rs` (the `Item::Workspace` variant near line 13, the rendering at line 76, and `format_branch_label` at line 236)
- Modify: `src/app.rs` (the items push at line 275)
- Modify existing test fixtures in `src/ui/dashboard.rs` (the workspace constructor in tests near line 265)

- [ ] **Step 1: Write the failing tests for the new `format_branch_label` signature**

Currently `format_branch_label` is at `src/ui/dashboard.rs:236`:

```rust
fn format_branch_label(branch: &str, nerd: bool) -> String {
    if nerd {
        format!("\u{e0a0} {branch}")
    } else {
        branch.to_string()
    }
}
```

There are no existing tests for it. Add a `#[cfg(test)] mod label_tests` block at the bottom of `src/ui/dashboard.rs` (or extend the existing `mod tests`):

```rust
#[cfg(test)]
mod label_tests {
    use super::*;
    use crate::forge::BranchLifecycle;

    #[test]
    fn nerd_no_lifecycle_uses_branch_glyph() {
        let s = format_branch_label("feat/x", true, None);
        assert_eq!(s, "\u{e0a0} feat/x");
    }

    #[test]
    fn nerd_open_pr_uses_pr_glyph() {
        let s = format_branch_label("feat/x", true, Some(BranchLifecycle::PrOpen));
        assert_eq!(s, "\u{f407} feat/x");
    }

    #[test]
    fn nerd_draft_pr_annotates() {
        let s = format_branch_label("feat/x", true, Some(BranchLifecycle::PrDraft));
        assert_eq!(s, "\u{f407} feat/x draft");
    }

    #[test]
    fn nerd_merged_pr_uses_merge_glyph() {
        let s = format_branch_label("feat/x", true, Some(BranchLifecycle::PrMerged));
        assert_eq!(s, "\u{f419} feat/x");
    }

    #[test]
    fn nerd_closed_pr_uses_x_glyph() {
        let s = format_branch_label("feat/x", true, Some(BranchLifecycle::PrClosed));
        assert_eq!(s, "\u{f659} feat/x");
    }

    #[test]
    fn nerd_no_pr_uses_branch_glyph() {
        let s = format_branch_label("feat/x", true, Some(BranchLifecycle::NoPr));
        assert_eq!(s, "\u{e0a0} feat/x");
    }

    #[test]
    fn ascii_open_pr_appends_pr_suffix() {
        let s = format_branch_label("feat/x", false, Some(BranchLifecycle::PrOpen));
        assert_eq!(s, "feat/x (pr)");
    }

    #[test]
    fn ascii_draft_pr_appends_draft_suffix() {
        let s = format_branch_label("feat/x", false, Some(BranchLifecycle::PrDraft));
        assert_eq!(s, "feat/x (draft)");
    }

    #[test]
    fn ascii_merged_pr_appends_merged_suffix() {
        let s = format_branch_label("feat/x", false, Some(BranchLifecycle::PrMerged));
        assert_eq!(s, "feat/x (merged)");
    }

    #[test]
    fn ascii_closed_pr_appends_closed_suffix() {
        let s = format_branch_label("feat/x", false, Some(BranchLifecycle::PrClosed));
        assert_eq!(s, "feat/x (closed)");
    }

    #[test]
    fn ascii_no_pr_is_plain() {
        let s = format_branch_label("feat/x", false, Some(BranchLifecycle::NoPr));
        assert_eq!(s, "feat/x");
    }

    #[test]
    fn ascii_none_is_plain() {
        let s = format_branch_label("feat/x", false, None);
        assert_eq!(s, "feat/x");
    }
}
```

Run: `cargo test --lib label_tests`
Expected: FAIL — `format_branch_label` doesn't yet take 3 args.

- [ ] **Step 2: Update `format_branch_label` to take a lifecycle**

Replace `format_branch_label` at `src/ui/dashboard.rs:236-242` with:

```rust
fn format_branch_label(
    branch: &str,
    nerd: bool,
    lifecycle: Option<crate::forge::BranchLifecycle>,
) -> String {
    use crate::forge::BranchLifecycle::*;
    if nerd {
        let (glyph, suffix) = match lifecycle {
            None | Some(NoPr) => ("\u{e0a0}", ""),
            Some(PrOpen) => ("\u{f407}", ""),
            Some(PrDraft) => ("\u{f407}", " draft"),
            Some(PrMerged) => ("\u{f419}", ""),
            Some(PrClosed) => ("\u{f659}", ""),
        };
        format!("{glyph} {branch}{suffix}")
    } else {
        let suffix = match lifecycle {
            Some(PrOpen) => " (pr)",
            Some(PrDraft) => " (draft)",
            Some(PrMerged) => " (merged)",
            Some(PrClosed) => " (closed)",
            None | Some(NoPr) => "",
        };
        format!("{branch}{suffix}")
    }
}
```

- [ ] **Step 3: Add the lifecycle field to `Item::Workspace` and thread it through**

In `src/ui/dashboard.rs`, the `Item::Workspace` variant currently ends at line 26 with `awaiting_tool: Option<(String, i64)>`. Add a new field above it:

```rust
    Workspace {
        repo: &'a Repo,
        workspace: &'a Workspace,
        session_running: bool,
        seconds_since_activity: Option<u64>,
        has_prior_session: bool,
        status: Option<crate::git::WorkspaceStatus>,
        latest_event: Option<crate::events::EventSnapshot>,
        needs_attention: bool,
        lifecycle: Option<crate::forge::BranchLifecycle>,
        awaiting_tool: Option<(String, i64)>,
    },
```

In the matcher near line 76, destructure the new field:

```rust
            Item::Workspace {
                repo: _,
                workspace,
                session_running,
                seconds_since_activity,
                has_prior_session,
                status,
                latest_event,
                needs_attention,
                lifecycle,
                awaiting_tool,
            } => {
```

At line 118, update the call:

```rust
                let branch_label = format_branch_label(&workspace.branch, nerd_fonts, *lifecycle);
```

- [ ] **Step 4: Populate the lifecycle field when constructing the item in `app.rs`**

In `src/app.rs` around line 275:

```rust
                    items.push(dashboard::Item::Workspace {
                        repo,
                        workspace: ws,
                        session_running: running,
                        seconds_since_activity: secs,
                        has_prior_session: has_prior,
                        status: app.workspace_status.get(&ws.id).copied(),
                        latest_event: app
                            .workspace_events
                            .get(&ws.id)
                            .and_then(|e| e.latest.clone()),
                        needs_attention,
                        lifecycle: app.pr_lifecycle.get(&ws.id).copied(),
                        awaiting_tool: awaiting,
                    });
```

- [ ] **Step 5: Update any existing test fixtures that build `Item::Workspace`**

The existing tests in `src/ui/dashboard.rs` (around lines 397, 435, 572 — search for `Item::Workspace`) construct workspace items literally. Add `lifecycle: None,` to each construction. The grep target is:

Run: `grep -n "Item::Workspace" src/ui/dashboard.rs`

For each match, ensure the struct literal includes `lifecycle: None,` alongside the other fields. Example pattern to look for:

```rust
        let item = Item::Workspace {
            repo: &r,
            workspace: &ws,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: Some(st),
            latest_event: None,
            needs_attention: false,
            awaiting_tool: None,
        };
```

Becomes:

```rust
        let item = Item::Workspace {
            repo: &r,
            workspace: &ws,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: Some(st),
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
        };
```

- [ ] **Step 6: Run all tests**

Run: `cargo test --lib`
Expected: all tests pass, including the 10 new `label_tests::*`.

- [ ] **Step 7: Verify a release build**

Run: `cargo build --release`
Expected: clean build (no warnings introduced by this change).

- [ ] **Step 8: Commit**

```bash
git add src/ui/dashboard.rs src/app.rs
git commit -m "feat(ui): branch glyph reflects PR lifecycle (open/draft/merged/closed)"
```

---

### Task 6: Manual smoke test in a real repo

This is intentionally a manual step — the gh CLI's behaviour against real GitHub is hard to mock meaningfully, and the value of seeing the glyph render is high.

- [ ] **Step 1: Verify `gh` is authenticated**

Run: `gh auth status`
Expected: `Logged in to github.com as <your-user>`. If not, the user runs `gh auth login` first.

- [ ] **Step 2: Build and run wsx**

Run: `cargo build && ./target/debug/wsx`
Expected: TUI launches.

- [ ] **Step 3: Visual check**

In the dashboard, locate any workspace whose branch matches an open PR on the corresponding GitHub remote. Within ~30s of launch (or immediately on the next 2s tick after 30s have elapsed since the previous poll for that workspace), the leading glyph should change from `\u{e0a0}` to `\u{f407}` (open PR).

Repeat for at least one branch with a merged PR (`\u{f419}`) and one branch with no PR (stays `\u{e0a0}`).

- [ ] **Step 4: Degrade-gracefully check**

Quit wsx. Temporarily move `gh` out of PATH:

Run: `which gh` (note the path)
Run: `sudo mv $(which gh) /tmp/gh-backup` (or move it into a non-PATH dir without sudo, depending on install)
Run: `./target/debug/wsx`
Expected: dashboard renders normally; all branches show the static branch glyph (or no glyph in ASCII mode). No panics, no error spam in logs.

Restore: `sudo mv /tmp/gh-backup $(which-was-gh)` (use the path noted above).

- [ ] **Step 5: No commit needed for the smoke test**

If anything looked wrong in steps 3-4, open a follow-up task — don't patch on this branch unless the issue is a clear regression introduced by tasks 1-5.

---

## Self-Review

**Spec coverage:**
- "Show git symbol indicative of branch state (pull request, merged, etc)" — Task 5 renders different glyphs for `PrOpen`, `PrDraft`, `PrMerged`, `PrClosed`, `NoPr`. ✓
- "Use gh" — Task 2 shells out to `gh pr view`. ✓
- Graceful degradation when gh is missing — Task 2 returns `Ok(None)` on spawn failure; Task 5 maps `None` to the existing static glyph. ✓ Task 6 step 4 verifies this manually.

**Placeholder scan:** No TODO/TBD/fill-in entries. Every code step has the full code block.

**Type consistency:**
- `BranchLifecycle` enum variants (`NoPr`, `PrDraft`, `PrOpen`, `PrMerged`, `PrClosed`) are referenced identically in Tasks 1, 2, 5.
- `fetch_branch_lifecycle(worktree, branch) -> Result<Option<BranchLifecycle>>` signature is used identically in Tasks 1, 2, 4.
- `parse_gh_pr_view` and `stderr_means_no_pr` are private to `forge` (`pub(crate)` for parser per Task 1 test access).
- `App.pr_lifecycle` and `App.pr_last_poll_ms` defined in Task 3, read in Tasks 4 and 5.
- `Item::Workspace.lifecycle` defined in Task 5, populated in Task 5 step 4.

No mismatches.
