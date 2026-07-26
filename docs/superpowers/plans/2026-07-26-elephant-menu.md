# Elephant Lua Workspace Menu Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upgrade the waybar workspace picker from a plain `walker --dmenu` list to a rich walker/elephant menu (icon + two-line rows with branch, PR state, dirty/diff indicators), with automatic fallback to the existing dmenu pipe.

**Architecture:** A new `scm_cache` sqlite table caches PR state (written by the TUI's existing poll and by a detached sweep) while git-local facts are computed fresh at menu open. A new `wsx waybar menu-entries --json` subcommand emits display-ready entries; a ~40-line Lua shim installed to `~/.config/elephant/menus/wsx.lua` feeds them to walker via elephant's `menus` provider. `wsx waybar menu` auto-detects the Lua menu and falls back to the dmenu pipe.

**Tech Stack:** Rust (tokio, rusqlite, serde_json, shlex, futures — all existing deps), Lua (gopher-lua dialect hosted by elephant), walker/elephant on Omarchy.

**Spec:** `docs/superpowers/specs/2026-07-26-elephant-menu-design.md` — read it first.

## Global Constraints

- Never commit to `main`; work stays on the current feature branch.
- wsx migrations re-run **every** startup (`SCHEMA_V1` resets `user_version` to 1). Every schema block must be idempotent (`CREATE TABLE IF NOT EXISTS`, `add_column_if_missing`).
- The `waybar` module is Linux-gated: `src/lib.rs:18` has `#[cfg(target_os = "linux")] pub mod waybar;`. CLI dispatch arms for waybar actions carry the same cfg, with a `waybar_linux_only()` error arm for other platforms.
- CI gates are separate: `cargo fmt --check`, `cargo clippy`, `cargo test` must each pass.
- Unknown vs none is semantic: NULL cache columns mean "never computed" and render as **no** indicator, never as "no PR" / "clean".
- `menu-entries` must never make a `gh`/network call; `refresh-prs` must never print to stdout.
- Menu display strings pass through `sanitize()` (control chars → space) — status messages and repo names are user/agent-controlled text.
- Shell-command strings (`action`) are built with `shlex::try_quote` — repo names may contain spaces.

---

### Task 1: `scm_cache` table + store accessors

**Files:**
- Modify: `src/data/schema.rs` (migrate() tail at ~line 129-133; consts at end)
- Create: `src/data/scm_cache.rs`
- Modify: `src/data/mod.rs` (module list)
- Modify: `src/data/store.rs` (`remove_workspace` cascade, ~line 188-203)

**Interfaces:**
- Consumes: `Store`, `WorkspaceId` (`src/data/store.rs`), `BranchLifecycle` (`src/git/forge.rs`).
- Produces (used by Tasks 3 & 5):
  - `pub struct ScmCacheRow { pr_lifecycle: Option<BranchLifecycle>, pr_number: Option<u32>, dirty: Option<bool>, additions: Option<u32>, deletions: Option<u32>, fetched_at: Option<i64> }` (all fields `pub`, derives `Debug, Clone, Copy, PartialEq, Eq, Default`)
  - `Store::upsert_scm_pr(&self, id: WorkspaceId, lifecycle: BranchLifecycle, number: Option<u32>, fetched_at: i64) -> Result<()>`
  - `Store::upsert_scm_git(&self, id: WorkspaceId, dirty: bool, additions: u32, deletions: u32) -> Result<()>`
  - `Store::all_scm_cache(&self) -> Result<HashMap<WorkspaceId, ScmCacheRow>>`

- [ ] **Step 1: Write the failing test**

Create `src/data/scm_cache.rs` with only the test module for now:

```rust
#[cfg(test)]
mod scm_cache_tests {
    use crate::data::store::{NewWorkspace, Store};
    use crate::git::forge::BranchLifecycle;
    use crate::pty::session::AgentKind;
    use std::path::Path;

    fn store_with_workspace() -> (Store, crate::data::store::WorkspaceId) {
        let store = Store::open_in_memory().unwrap();
        let repo = store.add_repo(Path::new("/tmp/r"), "r", "x").unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "w",
                branch: "x/w",
                worktree_path: &std::path::PathBuf::from("/tmp/r/w"),
                yolo: false,
                agent: AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        (store, id)
    }

    #[test]
    fn upserts_merge_into_one_row() {
        let (store, id) = store_with_workspace();
        assert!(store.all_scm_cache().unwrap().is_empty());

        store.upsert_scm_git(id, true, 4, 2).unwrap();
        store
            .upsert_scm_pr(id, BranchLifecycle::PrOpen, Some(12), 1000)
            .unwrap();

        let row = store.all_scm_cache().unwrap()[&id];
        assert_eq!(row.dirty, Some(true));
        assert_eq!(row.additions, Some(4));
        assert_eq!(row.deletions, Some(2));
        assert_eq!(row.pr_lifecycle, Some(BranchLifecycle::PrOpen));
        assert_eq!(row.pr_number, Some(12));
        assert_eq!(row.fetched_at, Some(1000));

        // A git-only update must not clobber PR fields, and vice versa.
        store.upsert_scm_git(id, false, 0, 0).unwrap();
        let row = store.all_scm_cache().unwrap()[&id];
        assert_eq!(row.pr_lifecycle, Some(BranchLifecycle::PrOpen));
        assert_eq!(row.dirty, Some(false));
    }

    #[test]
    fn pr_number_none_clears_stored_number() {
        let (store, id) = store_with_workspace();
        store
            .upsert_scm_pr(id, BranchLifecycle::PrOpen, Some(7), 1000)
            .unwrap();
        store
            .upsert_scm_pr(id, BranchLifecycle::NoPr, None, 2000)
            .unwrap();
        let row = store.all_scm_cache().unwrap()[&id];
        assert_eq!(row.pr_lifecycle, Some(BranchLifecycle::NoPr));
        assert_eq!(row.pr_number, None);
    }

    #[test]
    fn lifecycle_round_trips_through_text() {
        for l in [
            BranchLifecycle::NoPr,
            BranchLifecycle::PrDraft,
            BranchLifecycle::PrOpen,
            BranchLifecycle::PrConflicted,
            BranchLifecycle::PrMerged,
            BranchLifecycle::PrClosed,
        ] {
            assert_eq!(
                super::lifecycle_from_str(super::lifecycle_to_str(l)),
                Some(l),
                "{l:?}"
            );
        }
        assert_eq!(super::lifecycle_from_str("garbage"), None);
    }

    #[test]
    fn remove_workspace_deletes_cache_row() {
        let (store, id) = store_with_workspace();
        store.upsert_scm_git(id, true, 1, 1).unwrap();
        store.remove_workspace(id).unwrap();
        assert!(store.all_scm_cache().unwrap().is_empty());
    }
}
```

Register the module in `src/data/mod.rs` — it has a public struct, so it goes with the public modules (below `pub mod agents;` etc.):

```rust
pub mod scm_cache;
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test scm_cache 2>&1 | tail -20`
Expected: compile FAILURE — `lifecycle_to_str`, `upsert_scm_git`, etc. not found.

- [ ] **Step 3: Implement schema + module**

In `src/data/schema.rs`, after the `if v < 17 { ... }` block (line ~129-132) and before `Ok(())`:

```rust
        if v < 18 {
            self.conn().execute_batch(SCHEMA_V18_SCM_CACHE)?;
            self.conn().execute("PRAGMA user_version = 18", [])?;
        }
```

At the end of the file, with the other schema consts:

```rust
const SCHEMA_V18_SCM_CACHE: &str = "
CREATE TABLE IF NOT EXISTS scm_cache (
    workspace_id INTEGER PRIMARY KEY REFERENCES workspaces(id),
    pr_lifecycle TEXT,
    pr_number    INTEGER,
    dirty        INTEGER,
    additions    INTEGER,
    deletions    INTEGER,
    fetched_at   INTEGER
);
";
```

Top of `src/data/scm_cache.rs` (above the test module):

```rust
//! `scm_cache` accessors: per-workspace git/PR indicators for the
//! waybar/walker workspace menu. NULL columns mean "never computed" and
//! render as no indicator — distinct from a known "no PR" / "clean".
//! PR fields are written by the TUI's background poll and the
//! `wsx waybar refresh-prs` sweep; git fields by `wsx waybar menu-entries`.

use std::collections::HashMap;

use crate::data::store::{Store, WorkspaceId};
use crate::error::Result;
use crate::git::forge::BranchLifecycle;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScmCacheRow {
    pub pr_lifecycle: Option<BranchLifecycle>,
    pub pr_number: Option<u32>,
    pub dirty: Option<bool>,
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
    pub fetched_at: Option<i64>,
}

pub(crate) fn lifecycle_to_str(l: BranchLifecycle) -> &'static str {
    match l {
        BranchLifecycle::NoPr => "no_pr",
        BranchLifecycle::PrDraft => "draft",
        BranchLifecycle::PrOpen => "open",
        BranchLifecycle::PrConflicted => "conflicted",
        BranchLifecycle::PrMerged => "merged",
        BranchLifecycle::PrClosed => "closed",
    }
}

pub(crate) fn lifecycle_from_str(s: &str) -> Option<BranchLifecycle> {
    match s {
        "no_pr" => Some(BranchLifecycle::NoPr),
        "draft" => Some(BranchLifecycle::PrDraft),
        "open" => Some(BranchLifecycle::PrOpen),
        "conflicted" => Some(BranchLifecycle::PrConflicted),
        "merged" => Some(BranchLifecycle::PrMerged),
        "closed" => Some(BranchLifecycle::PrClosed),
        _ => None,
    }
}

impl Store {
    pub fn upsert_scm_pr(
        &self,
        id: WorkspaceId,
        lifecycle: BranchLifecycle,
        number: Option<u32>,
        fetched_at: i64,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT INTO scm_cache (workspace_id, pr_lifecycle, pr_number, fetched_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(workspace_id) DO UPDATE SET
               pr_lifecycle = excluded.pr_lifecycle,
               pr_number    = excluded.pr_number,
               fetched_at   = excluded.fetched_at",
            rusqlite::params![id.0, lifecycle_to_str(lifecycle), number, fetched_at],
        )?;
        Ok(())
    }

    pub fn upsert_scm_git(
        &self,
        id: WorkspaceId,
        dirty: bool,
        additions: u32,
        deletions: u32,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT INTO scm_cache (workspace_id, dirty, additions, deletions)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(workspace_id) DO UPDATE SET
               dirty     = excluded.dirty,
               additions = excluded.additions,
               deletions = excluded.deletions",
            rusqlite::params![id.0, dirty as i64, additions, deletions],
        )?;
        Ok(())
    }

    pub fn all_scm_cache(&self) -> Result<HashMap<WorkspaceId, ScmCacheRow>> {
        let mut stmt = self.conn().prepare(
            "SELECT workspace_id, pr_lifecycle, pr_number, dirty, additions, deletions, fetched_at
             FROM scm_cache",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                WorkspaceId(r.get(0)?),
                ScmCacheRow {
                    pr_lifecycle: r
                        .get::<_, Option<String>>(1)?
                        .as_deref()
                        .and_then(lifecycle_from_str),
                    pr_number: r.get(2)?,
                    dirty: r.get::<_, Option<i64>>(3)?.map(|v| v != 0),
                    additions: r.get(4)?,
                    deletions: r.get(5)?,
                    fetched_at: r.get(6)?,
                },
            ))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (id, cache) = row?;
            map.insert(id, cache);
        }
        Ok(map)
    }
}
```

In `src/data/store.rs::remove_workspace`, add the cache delete alongside the
existing manual cascade (before the `DELETE FROM workspaces` statement):

```rust
        self.conn
            .execute("DELETE FROM scm_cache WHERE workspace_id = ?1", [id.0])?;
```

Access the connection via `self.conn()` exactly as `src/data/status.rs` does
(store.rs itself uses the `self.conn` field; both work within the crate —
mirror `status.rs` since this module is its closest sibling).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test scm_cache 2>&1 | tail -5`
Expected: 4 passed.
Also run: `cargo test data:: 2>&1 | tail -5` — no regressions in other store tests.

- [ ] **Step 5: Commit**

```bash
git add src/data/schema.rs src/data/scm_cache.rs src/data/mod.rs src/data/store.rs
git commit -m "feat(waybar): scm_cache table caching PR + git indicators per workspace"
```

---

### Task 2: Row composition (pure functions) in `waybar/entries.rs`

**Files:**
- Create: `src/waybar/entries.rs`
- Modify: `src/waybar/mod.rs` (add `pub mod entries;`)
- Modify: `src/waybar/menu.rs:9` (`fn sanitize` → `pub(crate) fn sanitize`)
- Modify: `src/waybar/status.rs:33` (`fn glyph` → `pub(crate) fn glyph`)

**Interfaces:**
- Consumes: `ScmCacheRow` (Task 1), `ReportedStatus`/`ReportedState::as_str()` (`src/data/store.rs`), `BranchLifecycle`, `menu::sanitize`, `status::glyph`.
- Produces (used by Task 3):
  - `pub struct MenuEntry { text: String, subtext: String, icon: String, action: String }` (fields `pub`, derives `serde::Serialize, Debug, PartialEq`)
  - `pub(crate) fn compose_text(repo: &str, slug: &str, row: &ScmCacheRow) -> String`
  - `pub(crate) fn compose_subtext(branch: &str, status: Option<&ReportedStatus>) -> String`
  - `pub(crate) fn action_cmd(wsx_bin: &str, repo: &str, slug: &str) -> String`
  - `pub(crate) fn needs_pr_refresh(fetched_at: Option<i64>, now: i64) -> bool`
  - `pub const PR_REFRESH_THROTTLE_SECS: i64 = 120;`

- [ ] **Step 1: Write the failing tests**

Create `src/waybar/entries.rs`:

```rust
//! Rich walker/elephant menu entries: `wsx waybar menu-entries --json`
//! emits display-ready rows; `wsx waybar refresh-prs` sweeps the PR cache.
//! See docs/superpowers/specs/2026-07-26-elephant-menu-design.md.

use crate::data::scm_cache::ScmCacheRow;
use crate::data::store::ReportedStatus;
use crate::git::forge::BranchLifecycle;
use crate::waybar::menu::sanitize;

/// Skip `gh` for a workspace whose PR state was fetched more recently than
/// this. Matches the spirit of the TUI's 30s in-memory throttle but is more
/// conservative: menu opens are burstier than TUI ticks.
pub const PR_REFRESH_THROTTLE_SECS: i64 = 120;

const GLYPH_BRANCH: &str = "\u{e0a0}"; // powerline branch
const GLYPH_PR: &str = "\u{f407}"; // nf-oct-git_pull_request
const GLYPH_MERGED: &str = "\u{f419}"; // nf-oct-git_merge
const GLYPH_DIRTY: &str = "\u{25cf}"; // ●

#[derive(serde::Serialize, Debug, PartialEq)]
pub struct MenuEntry {
    pub text: String,
    pub subtext: String,
    pub icon: String,
    pub action: String,
}

pub(crate) fn needs_pr_refresh(fetched_at: Option<i64>, now: i64) -> bool {
    match fetched_at {
        None => true,
        Some(t) => now.saturating_sub(t) >= PR_REFRESH_THROTTLE_SECS,
    }
}

/// PR indicator, or None when there is no PR or the state was never fetched
/// (deliberately identical renderings — an unknown must not claim "no PR",
/// and "no PR" earns no glyph).
fn pr_segment(row: &ScmCacheRow) -> Option<String> {
    let lifecycle = row.pr_lifecycle?;
    let (glyph, suffix) = match lifecycle {
        BranchLifecycle::NoPr => return None,
        BranchLifecycle::PrOpen => (GLYPH_PR, None),
        BranchLifecycle::PrDraft => (GLYPH_PR, Some("draft")),
        BranchLifecycle::PrConflicted => (GLYPH_PR, Some("conflict")),
        BranchLifecycle::PrMerged => (GLYPH_MERGED, None),
        BranchLifecycle::PrClosed => (GLYPH_PR, Some("closed")),
    };
    let mut parts = vec![glyph.to_string()];
    if let Some(n) = row.pr_number {
        parts.push(format!("#{n}"));
    }
    if let Some(s) = suffix {
        parts.push(s.to_string());
    }
    Some(parts.join(" "))
}

pub(crate) fn compose_text(repo: &str, slug: &str, row: &ScmCacheRow) -> String {
    let mut parts = vec![format!("{}/{}", sanitize(repo), sanitize(slug))];
    if let Some(pr) = pr_segment(row) {
        parts.push(pr);
    }
    if row.dirty == Some(true) {
        parts.push(GLYPH_DIRTY.to_string());
    }
    if let (Some(a), Some(d)) = (row.additions, row.deletions) {
        if a + d > 0 {
            parts.push(format!("+{a} \u{2212}{d}"));
        }
    }
    parts.join("  ")
}

pub(crate) fn compose_subtext(branch: &str, status: Option<&ReportedStatus>) -> String {
    let b = format!("{GLYPH_BRANCH} {}", sanitize(branch));
    let Some(s) = status else {
        return b;
    };
    match s.message.as_deref().filter(|m| !m.is_empty()) {
        Some(m) => format!("{b} \u{2014} {}: {}", s.state.as_str(), sanitize(m)),
        None => format!("{b} \u{2014} {}", s.state.as_str()),
    }
}

fn quote(s: &str) -> String {
    shlex::try_quote(s)
        .map(|c| c.into_owned())
        // Only fails on interior NUL, which cannot survive sqlite TEXT
        // anyway; drop the offending byte rather than emit an unquoted arg.
        .unwrap_or_else(|_| format!("'{}'", s.replace(['\'', '\0'], "")))
}

pub(crate) fn action_cmd(wsx_bin: &str, repo: &str, slug: &str) -> String {
    format!(
        "{} waybar jump {} {}",
        quote(wsx_bin),
        quote(repo),
        quote(slug)
    )
}

#[cfg(test)]
mod entry_tests {
    use super::*;
    use crate::data::store::{ReportedState, ReportedStatus};

    fn status(state: ReportedState, msg: Option<&str>) -> ReportedStatus {
        ReportedStatus {
            state,
            message: msg.map(str::to_string),
            source: "test".into(),
            reported_at: 0,
        }
    }

    #[test]
    fn text_plain_when_cache_empty() {
        assert_eq!(
            compose_text("r", "w", &ScmCacheRow::default()),
            "r/w".to_string()
        );
    }

    #[test]
    fn text_with_all_indicators() {
        let row = ScmCacheRow {
            pr_lifecycle: Some(BranchLifecycle::PrOpen),
            pr_number: Some(123),
            dirty: Some(true),
            additions: Some(45),
            deletions: Some(12),
            fetched_at: Some(0),
        };
        let text = compose_text("workspacex", "fix-bug", &row);
        assert!(text.starts_with("workspacex/fix-bug"), "{text}");
        assert!(text.contains("#123"), "{text}");
        assert!(text.contains('\u{25cf}'), "{text}");
        assert!(text.contains("+45 \u{2212}12"), "{text}");
    }

    #[test]
    fn no_pr_and_unknown_render_identically() {
        let unknown = ScmCacheRow::default();
        let no_pr = ScmCacheRow {
            pr_lifecycle: Some(BranchLifecycle::NoPr),
            fetched_at: Some(0),
            ..Default::default()
        };
        assert_eq!(
            compose_text("r", "w", &unknown),
            compose_text("r", "w", &no_pr)
        );
    }

    #[test]
    fn draft_conflict_closed_carry_labels() {
        for (l, label) in [
            (BranchLifecycle::PrDraft, "draft"),
            (BranchLifecycle::PrConflicted, "conflict"),
            (BranchLifecycle::PrClosed, "closed"),
        ] {
            let row = ScmCacheRow {
                pr_lifecycle: Some(l),
                pr_number: Some(7),
                ..Default::default()
            };
            let text = compose_text("r", "w", &row);
            assert!(text.contains(label), "{l:?}: {text}");
            assert!(text.contains("#7"), "{l:?}: {text}");
        }
    }

    #[test]
    fn merged_uses_merge_glyph_without_label() {
        let row = ScmCacheRow {
            pr_lifecycle: Some(BranchLifecycle::PrMerged),
            pr_number: Some(9),
            ..Default::default()
        };
        let text = compose_text("r", "w", &row);
        assert!(text.contains('\u{f419}'), "{text}");
        assert!(!text.contains("merged"), "{text}");
    }

    #[test]
    fn clean_zero_diff_shows_no_noise() {
        let row = ScmCacheRow {
            dirty: Some(false),
            additions: Some(0),
            deletions: Some(0),
            ..Default::default()
        };
        assert_eq!(compose_text("r", "w", &row), "r/w");
    }

    #[test]
    fn subtext_variants() {
        assert_eq!(
            compose_subtext("x/w", None),
            "\u{e0a0} x/w".to_string()
        );
        assert_eq!(
            compose_subtext("x/w", Some(&status(ReportedState::Working, Some("fixing")))),
            "\u{e0a0} x/w \u{2014} working: fixing".to_string()
        );
        assert_eq!(
            compose_subtext("x/w", Some(&status(ReportedState::Done, None))),
            "\u{e0a0} x/w \u{2014} done".to_string()
        );
    }

    #[test]
    fn subtext_sanitizes_newlines() {
        let s = compose_subtext("x/w", Some(&status(ReportedState::Working, Some("a\nb"))));
        assert!(!s.contains('\n'), "{s}");
    }

    #[test]
    fn action_cmd_quotes_spacey_repo() {
        let cmd = action_cmd("/usr/bin/wsx", "meals backend", "api-fix");
        assert_eq!(cmd, "/usr/bin/wsx waybar jump 'meals backend' api-fix");
    }

    #[test]
    fn throttle_decision() {
        assert!(needs_pr_refresh(None, 1000));
        assert!(needs_pr_refresh(Some(880), 1000));
        assert!(!needs_pr_refresh(Some(881), 1000));
        assert!(!needs_pr_refresh(Some(2000), 1000)); // clock skew: don't refetch
    }

    #[test]
    fn menu_entry_serializes_with_lowercase_keys() {
        let e = MenuEntry {
            text: "t".into(),
            subtext: "s".into(),
            icon: "i".into(),
            action: "a".into(),
        };
        let v = serde_json::to_value([e]).unwrap();
        assert_eq!(v[0]["text"], "t");
        assert_eq!(v[0]["subtext"], "s");
        assert_eq!(v[0]["icon"], "i");
        assert_eq!(v[0]["action"], "a");
    }
}
```

Add to `src/waybar/mod.rs`:

```rust
pub mod entries;
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test entry_tests 2>&1 | tail -20`
Expected: compile FAILURE — `sanitize` is private, `glyph` untouched yet (used in Task 3, but flip both visibilities now).

- [ ] **Step 3: Make the two helpers visible**

In `src/waybar/menu.rs` line 9: `fn sanitize(` → `pub(crate) fn sanitize(`.
In `src/waybar/status.rs` line 33: `fn glyph(` → `pub(crate) fn glyph(`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test entry_tests 2>&1 | tail -5`
Expected: 11 passed. (If `throttle_decision` disagrees with the boundary, the
implementation is wrong, not the test: `now - t >= 120` refreshes.)

- [ ] **Step 5: Commit**

```bash
git add src/waybar/entries.rs src/waybar/mod.rs src/waybar/menu.rs src/waybar/status.rs
git commit -m "feat(waybar): menu entry composition for the elephant workspace picker"
```

---

### Task 3: `menu-entries` + `refresh-prs` runtime

**Files:**
- Modify: `src/waybar/entries.rs` (append; composition from Task 2 stays put)

**Interfaces:**
- Consumes: Task 1 store methods; Task 2 composers; `crate::git::{workspace_status, resolve_base_branch, workspace_diff_stats, DiffStats}`; `crate::git::forge::fetch_pr_status`; `crate::data::repo::list`; `Store::workspaces`, `Store::all_workspace_status`.
- Produces (used by Task 4):
  - `pub async fn run_menu_entries(store: &Store) -> Result<()>` — prints the JSON array to stdout, spawns the detached sweep.
  - `pub async fn run_refresh_prs(store: &Store) -> Result<()>` — silent sweep.

- [ ] **Step 1: Write the failing test**

Append to the test module in `src/waybar/entries.rs`:

```rust
    #[tokio::test]
    async fn collect_rows_sorted_and_composed() {
        use crate::data::store::{NewWorkspace, Store};
        use crate::pty::session::AgentKind;

        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "x")
            .unwrap();
        let mut ids = vec![];
        for name in ["zeta", "alpha"] {
            ids.push(
                store
                    .insert_workspace(&NewWorkspace {
                        repo_id: repo,
                        name,
                        branch: &format!("x/{name}"),
                        worktree_path: &std::path::PathBuf::from(format!(
                            "/nonexistent/r/{name}"
                        )),
                        yolo: false,
                        agent: AgentKind::Claude,
                        shared: false,
                    })
                    .unwrap(),
            );
        }
        store
            .upsert_scm_pr(ids[0], crate::git::forge::BranchLifecycle::PrOpen, Some(5), 0)
            .unwrap();

        let rows = super::collect_rows(&store).await.unwrap();
        let entries = super::build_entries(&rows, "/bin/wsx");

        assert_eq!(entries.len(), 2);
        // Sorted by workspace name within repo.
        assert!(entries[0].text.starts_with("r/alpha"), "{:?}", entries[0]);
        assert!(entries[1].text.starts_with("r/zeta"), "{:?}", entries[1]);
        // zeta carries the cached PR indicator even though its worktree is
        // missing (git facts degrade to absent, PR comes from cache).
        assert!(entries[1].text.contains("#5"), "{:?}", entries[1]);
        // Branch always present in subtext.
        assert!(entries[0].subtext.contains("x/alpha"), "{:?}", entries[0]);
        assert_eq!(entries[0].action, "/bin/wsx waybar jump r alpha");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test collect_rows_sorted 2>&1 | tail -10`
Expected: compile FAILURE — `collect_rows` / `build_entries` not found.

- [ ] **Step 3: Implement**

Append to `src/waybar/entries.rs` (above the test module). Add imports at the
top of the file: `use crate::data::store::Store;`, `use crate::error::Result;`,
`use std::path::PathBuf;`.

```rust
pub(crate) struct RowInput {
    pub repo_name: String,
    pub slug: String,
    pub branch: String,
    pub status: Option<ReportedStatus>,
    pub cache: ScmCacheRow,
}

pub(crate) fn build_entries(rows: &[RowInput], wsx_bin: &str) -> Vec<MenuEntry> {
    rows.iter()
        .map(|r| MenuEntry {
            text: compose_text(&r.repo_name, &r.slug, &r.cache),
            subtext: compose_subtext(&r.branch, r.status.as_ref()),
            icon: crate::waybar::status::glyph(r.status.as_ref().map(|s| s.state)).to_string(),
            action: action_cmd(wsx_bin, &r.repo_name, &r.slug),
        })
        .collect()
}

/// Git facts for one worktree, or None if git fails (missing worktree, not a
/// repo, …) — the row then renders without dirty/diff indicators.
async fn gather_git_facts(worktree: PathBuf) -> Option<(bool, crate::git::DiffStats)> {
    let st = crate::git::workspace_status(&worktree).await.ok()?;
    let dirty = st.modified > 0 || st.untracked > 0;
    let base = crate::git::resolve_base_branch(&worktree).await;
    let stats = crate::git::workspace_diff_stats(&worktree, &base)
        .await
        .unwrap_or(crate::git::DiffStats {
            added: 0,
            removed: 0,
        });
    Some((dirty, stats))
}

/// Rows sorted by repo name then workspace name (parity with the dmenu
/// picker). Git-local facts are gathered concurrently across workspaces and
/// written through to `scm_cache`; PR fields are read from cache only.
async fn collect_rows(store: &Store) -> Result<Vec<RowInput>> {
    let statuses = store.all_workspace_status()?;
    let mut caches = store.all_scm_cache()?;
    let mut repos = crate::data::repo::list(store)?;
    repos.sort_by(|a, b| a.name.cmp(&b.name));

    let mut metas = Vec::new();
    for repo in &repos {
        let mut workspaces = store.workspaces(repo.id)?;
        workspaces.sort_by(|a, b| a.name.cmp(&b.name));
        for ws in workspaces {
            metas.push((ws.id, repo.name.clone(), ws.name, ws.branch, ws.worktree_path));
        }
    }

    let facts = futures::future::join_all(
        metas
            .iter()
            .map(|(_, _, _, _, worktree)| gather_git_facts(worktree.clone())),
    )
    .await;

    let mut rows = Vec::with_capacity(metas.len());
    for ((id, repo_name, slug, branch, _), fact) in metas.into_iter().zip(facts) {
        let mut cache = caches.remove(&id).unwrap_or_default();
        if let Some((dirty, stats)) = fact {
            cache.dirty = Some(dirty);
            cache.additions = Some(stats.added);
            cache.deletions = Some(stats.removed);
            let _ = store.upsert_scm_git(id, dirty, stats.added, stats.removed);
        }
        rows.push(RowInput {
            repo_name,
            slug,
            branch,
            status: statuses.get(&id).cloned(),
            cache,
        });
    }
    Ok(rows)
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Fire-and-forget `wsx waybar refresh-prs` so PR data self-heals by the
/// next menu open even when the TUI is not running.
fn spawn_pr_sweep(wsx_bin: &str) {
    use std::process::Stdio;
    let _ = std::process::Command::new(wsx_bin)
        .args(["waybar", "refresh-prs"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

pub async fn run_menu_entries(store: &Store) -> Result<()> {
    let rows = collect_rows(store).await?;
    let wsx_bin = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "wsx".into());
    let entries = build_entries(&rows, &wsx_bin);
    // Serialization of plain strings cannot fail.
    println!("{}", serde_json::to_string(&entries).expect("serialize entries"));
    spawn_pr_sweep(&wsx_bin);
    Ok(())
}

/// Sequentially refresh PR state for every workspace outside the throttle
/// window. Silent by contract: improves the cache or does nothing.
pub async fn run_refresh_prs(store: &Store) -> Result<()> {
    let caches = store.all_scm_cache()?;
    for repo in crate::data::repo::list(store)? {
        for ws in store.workspaces(repo.id)? {
            let fetched = caches.get(&ws.id).and_then(|c| c.fetched_at);
            if !needs_pr_refresh(fetched, unix_now()) {
                continue;
            }
            if let Ok(Some(status)) =
                crate::git::forge::fetch_pr_status(&ws.worktree_path, &ws.branch).await
            {
                let _ = store.upsert_scm_pr(ws.id, status.lifecycle, status.number, unix_now());
            }
            // Err / Ok(None): leave cached state alone (transient failure
            // must not clobber a known lifecycle).
        }
    }
    Ok(())
}
```

Also add `use crate::data::store::ReportedStatus;` if not already imported from Task 2.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test collect_rows_sorted 2>&1 | tail -5` → 1 passed.
Run: `cargo test waybar 2>&1 | tail -5` → all waybar tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/waybar/entries.rs
git commit -m "feat(waybar): menu-entries JSON emitter and refresh-prs cache sweep"
```

---

### Task 4: CLI wiring for the two subcommands

**Files:**
- Modify: `src/cli.rs` — `CliAction` enum (~line 428-432), `parse_waybar` (~line 1113), dispatch in `run_cli` (~line 1859-1868), waybar `GroupInfo` commands (~line 224-241)

**Interfaces:**
- Consumes: `run_menu_entries` / `run_refresh_prs` (Task 3).
- Produces: `wsx waybar menu-entries` and `wsx waybar refresh-prs` on the CLI; `CliAction::WaybarMenuEntries`, `CliAction::WaybarRefreshPrs` variants.

- [ ] **Step 1: Write the failing test**

`src/cli.rs` has existing parse tests (search `mod` + `parse` near the bottom; follow their construction pattern for `Args`). Add, mirroring how the existing waybar parse cases are tested (if no waybar parse test exists, add this to the existing cli test module using the same helper the neighboring tests use to build `Args` from strings):

```rust
    #[test]
    fn parses_waybar_menu_entries_and_refresh_prs() {
        assert!(matches!(
            parse(&["wsx", "waybar", "menu-entries"]),
            Ok(CliAction::WaybarMenuEntries)
        ));
        assert!(matches!(
            parse(&["wsx", "waybar", "refresh-prs"]),
            Ok(CliAction::WaybarRefreshPrs)
        ));
    }
```

(`parse(&[...])` here stands for whatever entry the existing tests call — reuse it verbatim from a neighboring test such as the one covering `waybar jump`. Do not invent a new harness.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test parses_waybar_menu_entries 2>&1 | tail -10`
Expected: compile FAILURE — variants don't exist.

- [ ] **Step 3: Implement**

Enum (next to the existing waybar variants at ~line 428):

```rust
    WaybarMenuEntries,
    WaybarRefreshPrs,
```

`parse_waybar` — add two arms above the `other =>` arm:

```rust
        Some("menu-entries") => Ok(CliAction::WaybarMenuEntries),
        Some("refresh-prs") => Ok(CliAction::WaybarRefreshPrs),
```

Dispatch in `run_cli` — extend the existing linux-gated block:

```rust
        #[cfg(target_os = "linux")]
        CliAction::WaybarMenuEntries => crate::waybar::entries::run_menu_entries(&store).await?,
        #[cfg(target_os = "linux")]
        CliAction::WaybarRefreshPrs => crate::waybar::entries::run_refresh_prs(&store).await?,
```

and widen the non-linux arm:

```rust
        #[cfg(not(target_os = "linux"))]
        CliAction::WaybarMenu
        | CliAction::WaybarJump { .. }
        | CliAction::WaybarMenuEntries
        | CliAction::WaybarRefreshPrs => return Err(waybar_linux_only()),
```

Help text — append to the waybar `GroupInfo` commands array (~line 224):

```rust
            CmdInfo {
                usage: "menu-entries",
                blurb: "Print walker/elephant menu entries as JSON",
            },
            CmdInfo {
                usage: "refresh-prs",
                blurb: "Refresh the cached PR state for all workspaces",
            },
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test parses_waybar 2>&1 | tail -5` → passes.
Run: `cargo build 2>&1 | tail -3` → clean build (dispatch compiles).

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): wsx waybar menu-entries / refresh-prs subcommands"
```

---

### Task 5: TUI write-through of PR state

**Files:**
- Modify: `src/app/background.rs` (~line 367-380, inside the `should_poll_pr` block)

**Interfaces:**
- Consumes: `Store::upsert_scm_pr` (Task 1); existing `now_ms` (u64 milliseconds) and `status: PrStatus` locals.
- Produces: warm `scm_cache` PR fields whenever the TUI runs.

- [ ] **Step 1: Implement (no new unit test)**

This is a two-line write-through inside a background loop that has no existing
test harness; correctness is covered by Task 1's store tests plus the manual
checklist (Task 8). In the block where the in-memory maps are updated:

```rust
                if let Ok(Some(status)) =
                    crate::git::forge::fetch_pr_status(&path, &db_branch).await
                {
                    let mut g = app.lock().await;
                    g.pr_lifecycle.insert(id, status.lifecycle);
                    match status.number {
                        Some(n) => {
                            g.pr_number.insert(id, n);
                        }
                        None => {
                            g.pr_number.remove(&id);
                        }
                    }
                    // Write-through so `wsx waybar menu-entries` (a separate
                    // short-lived process) sees PR state without calling gh.
                    let _ = g
                        .store
                        .upsert_scm_pr(id, status.lifecycle, status.number, (now_ms / 1000) as i64);
                }
```

Only the `// Write-through …` comment and the `upsert_scm_pr` statement are
new; the surrounding lines exist already at `src/app/background.rs:367-380`.

- [ ] **Step 2: Verify build + existing tests**

Run: `cargo test app:: 2>&1 | tail -5`
Expected: existing app tests pass, no new failures.

- [ ] **Step 3: Commit**

```bash
git add src/app/background.rs
git commit -m "feat(tui): write PR poll results through to scm_cache"
```

---

### Task 6: Lua shim asset + installer

**Files:**
- Create: `src/waybar/assets/wsx.lua`
- Modify: `src/waybar/install.rs` (new const + fn; wire into `run()` at ~line 169-178)

**Interfaces:**
- Consumes: nothing new.
- Produces (used by Task 7's detection and by `wsx setup waybar`):
  - `~/.config/elephant/menus/wsx.lua` on disk after setup
  - `pub fn install_elephant_menu_into(config_root: &Path, wsx_bin: &str) -> Result<String>`

- [ ] **Step 1: Write the failing test**

Append to `install_tests` in `src/waybar/install.rs`:

```rust
    #[test]
    fn elephant_menu_installs_with_quoted_binary_path() {
        let tmp = tempfile::tempdir().unwrap();
        let line =
            install_elephant_menu_into(tmp.path(), "/opt/my tools/wsx").unwrap();
        let lua_path = tmp.path().join("elephant/menus/wsx.lua");
        assert!(lua_path.exists(), "{line}");
        let lua = std::fs::read_to_string(&lua_path).unwrap();
        assert!(lua.contains("'/opt/my tools/wsx'"), "{lua}");
        assert!(lua.contains("waybar menu-entries --json"), "{lua}");
        assert!(lua.contains("function GetEntries()"), "{lua}");
        assert!(!lua.contains("__WSX_BIN__"), "{lua}");
        // Re-install overwrites without error (setup is re-runnable).
        install_elephant_menu_into(tmp.path(), "/usr/bin/wsx").unwrap();
        let lua = std::fs::read_to_string(&lua_path).unwrap();
        assert!(lua.contains("/usr/bin/wsx"), "{lua}");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test elephant_menu_installs 2>&1 | tail -10`
Expected: compile FAILURE — `install_elephant_menu_into` not found.

- [ ] **Step 3: Create the asset and installer**

Create `src/waybar/assets/wsx.lua` exactly:

```lua
-- wsx workspace picker for elephant/walker.
-- Installed by `wsx setup waybar`; edits are overwritten on re-run.
-- Launch with: walker -m menus:wsx
Name = "wsx"
NamePretty = "wsx Workspaces"
HideFromProviderlist = true

-- Shell-quoted absolute path, substituted at install time. Long-string
-- brackets so the single-quoted substitution lands verbatim.
local WSX = [[__WSX_BIN__]]

function GetEntries()
  local entries = {}
  local handle = io.popen(WSX .. " waybar menu-entries --json 2>/dev/null")
  if not handle then
    return entries
  end
  local out = handle:read("*a")
  handle:close()
  if not out or out == "" then
    return entries
  end
  local decoded = jsonDecode(out)
  if type(decoded) ~= "table" then
    return entries
  end
  for _, e in ipairs(decoded) do
    if type(e) == "table" and e.text and e.action then
      table.insert(entries, {
        Text = e.text,
        Subtext = e.subtext,
        Icon = e.icon,
        Actions = { activate = e.action },
      })
    end
  end
  return entries
end
```

In `src/waybar/install.rs`, next to the other embedded assets (line ~14-17):

```rust
/// The elephant menu definition, embedded at compile time.
const MENU_LUA: &str = include_str!("assets/wsx.lua");
```

New function (near `install_into`):

```rust
/// Write the elephant menu definition under `config_root` (normally
/// `~/.config`), substituting the shell-quoted wsx binary path. Creating the
/// directory is harmless when elephant isn't installed — the menu only
/// activates once `walker` is detected on PATH (see waybar::menu).
pub fn install_elephant_menu_into(config_root: &Path, wsx_bin: &str) -> Result<String> {
    let dir = config_root.join("elephant/menus");
    std::fs::create_dir_all(&dir)?;
    let quoted = shlex::try_quote(wsx_bin)
        .map(|c| c.into_owned())
        .unwrap_or_else(|_| wsx_bin.to_string());
    let path = dir.join("wsx.lua");
    std::fs::write(&path, MENU_LUA.replace("__WSX_BIN__", &quoted))?;
    Ok(format!("installed elephant menu: {}", path.display()))
}
```

Wire into `run()` (which currently returns `install_into(&waybar_dir, epoch)`):

```rust
pub fn run() -> Result<Vec<String>> {
    let config_root = dirs::config_dir()
        .ok_or_else(|| Error::UserInput("could not resolve ~/.config".into()))?;
    let waybar_dir = config_root.join("waybar");
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut lines = install_into(&waybar_dir, epoch)?;
    let wsx_bin = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "wsx".into());
    match install_elephant_menu_into(&config_root, &wsx_bin) {
        Ok(line) => lines.push(line),
        Err(e) => lines.push(format!("elephant menu skipped: {e}")),
    }
    Ok(lines)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test install 2>&1 | tail -5`
Expected: new test + existing install tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/waybar/assets/wsx.lua src/waybar/install.rs
git commit -m "feat(waybar): install elephant menu definition via wsx setup waybar"
```

---

### Task 7: Launcher detection in `run_menu`

**Files:**
- Modify: `src/waybar/menu.rs`

**Interfaces:**
- Consumes: the installed Lua file path (`~/.config/elephant/menus/wsx.lua`), `WSX_WAYBAR_MENU` env, existing pipe logic.
- Produces: `run_menu(store)` keeps its signature (`src/cli.rs:1861` untouched) but now launches `walker -m menus:wsx` when available.

- [ ] **Step 1: Write the failing tests**

Add to `menu_tests` in `src/waybar/menu.rs`:

```rust
    #[test]
    fn detect_prefers_env_then_elephant_then_dmenu() {
        let env_cmd = Some(vec!["wofi".to_string(), "--dmenu".to_string()]);
        assert!(matches!(
            detect_menu_mode(env_cmd, true, true),
            MenuMode::Pipe(ref c) if c[0] == "wofi"
        ));
        assert!(matches!(
            detect_menu_mode(None, true, true),
            MenuMode::Elephant
        ));
        // Missing lua or missing walker → dmenu pipe default.
        for (lua, walker) in [(false, true), (true, false), (false, false)] {
            assert!(matches!(
                detect_menu_mode(None, lua, walker),
                MenuMode::Pipe(ref c) if c == &["walker".to_string(), "--dmenu".to_string()]
            ));
        }
    }

    #[test]
    fn find_in_path_scans_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("walker"), "").unwrap();
        let path_var = format!("/nonexistent:{}", tmp.path().display());
        assert!(find_in_path("walker", &path_var));
        assert!(!find_in_path("walker", "/nonexistent"));
        assert!(!find_in_path("walker", ""));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test menu_tests 2>&1 | tail -10`
Expected: compile FAILURE — `MenuMode`, `detect_menu_mode`, `find_in_path` not found.

- [ ] **Step 3: Implement**

In `src/waybar/menu.rs`, replace `menu_command()` with an Option-returning
env reader plus mode detection (update the existing
`menu_command_env_override` test to call `env_menu_command()` and assert
`None` when unset):

```rust
pub(crate) fn env_menu_command() -> Option<Vec<String>> {
    std::env::var("WSX_WAYBAR_MENU")
        .ok()
        .and_then(|v| shlex::split(&v))
        .filter(|v| !v.is_empty())
}

#[derive(Debug)]
pub(crate) enum MenuMode {
    /// Pipe lines to a dmenu-style command and parse the selection.
    Pipe(Vec<String>),
    /// Launch walker against the installed elephant `menus:wsx` provider;
    /// selection and jump are handled by the entry's action, not stdout.
    Elephant,
}

pub(crate) fn detect_menu_mode(
    env_cmd: Option<Vec<String>>,
    lua_installed: bool,
    walker_on_path: bool,
) -> MenuMode {
    if let Some(cmd) = env_cmd {
        return MenuMode::Pipe(cmd);
    }
    if lua_installed && walker_on_path {
        return MenuMode::Elephant;
    }
    MenuMode::Pipe(vec!["walker".into(), "--dmenu".into()])
}

pub(crate) fn find_in_path(name: &str, path_var: &str) -> bool {
    std::env::split_paths(path_var).any(|d| !d.as_os_str().is_empty() && d.join(name).is_file())
}
```

Restructure `run_menu`: the current body (lines building `cmd`, spawning,
piping, parsing, jumping) moves verbatim into
`fn run_pipe_menu(store: &Store, cmd: Vec<String>) -> Result<()>`, with
`let cmd = menu_command();` deleted (the `cmd` parameter replaces it). Then:

```rust
pub fn run_menu(store: &Store) -> Result<()> {
    let lua_installed = dirs::config_dir()
        .map(|d| d.join("elephant/menus/wsx.lua").exists())
        .unwrap_or(false);
    let walker_ok = find_in_path("walker", &std::env::var("PATH").unwrap_or_default());
    match detect_menu_mode(env_menu_command(), lua_installed, walker_ok) {
        MenuMode::Elephant => {
            match Command::new("walker").args(["-m", "menus:wsx"]).status() {
                // Any exit status counts as handled: walker returns non-zero
                // on dismissal too, and falling back would double-open.
                Ok(_) => Ok(()),
                // Spawn failure (walker vanished between check and exec):
                // degrade silently to the dmenu pipe.
                Err(_) => run_pipe_menu(store, vec!["walker".into(), "--dmenu".into()]),
            }
        }
        MenuMode::Pipe(cmd) => run_pipe_menu(store, cmd),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test menu 2>&1 | tail -5`
Expected: new + existing menu tests pass (including the updated env-override test).

- [ ] **Step 5: Commit**

```bash
git add src/waybar/menu.rs
git commit -m "feat(waybar): auto-detect elephant menu with dmenu fallback"
```

---

### Task 8: Manual test doc + full gates

**Files:**
- Modify: `docs/manual-tests/waybar.md` (append section)

- [ ] **Step 1: Append the manual checklist**

```markdown
## Elephant menu (walker rich picker)

Prereqs: omarchy host with walker + elephant running; `wsx` installed to
`~/.local/bin` (rebuild ≠ install — copy the binary).

- [ ] `wsx setup waybar` prints `installed elephant menu: …/elephant/menus/wsx.lua`
- [ ] `wsx waybar menu-entries --json | jq .` shows one object per workspace
      with text/subtext/icon/action; repo/slug sorted; branch in subtext
- [ ] Click the waybar module: walker opens the rich menu (two-line rows,
      status glyph icon), not the plain dmenu list
- [ ] Rows show dirty ● and +N −N immediately after touching a worktree
- [ ] With a PR open on a branch: PR indicator appears (immediately if the
      TUI is running; otherwise by the second menu open, after the sweep)
- [ ] Enter on a row jumps/attaches the workspace (same as dmenu behavior)
- [ ] Esc closes without jumping; no fallback dmenu double-opens
- [ ] `WSX_WAYBAR_MENU="wofi --dmenu" wsx waybar menu` still uses the pipe
- [ ] `mv ~/.config/elephant/menus/wsx.lua{,.off} && wsx waybar menu` falls
      back to the plain dmenu pipe (restore the file after)
```

- [ ] **Step 2: Run all gates**

Run: `cargo fmt --check && cargo clippy --all-targets 2>&1 | tail -3 && cargo test 2>&1 | tail -3`
Expected: all three clean. (`click_chip_auto_spawns_session_when_missing` is a
known flaky PTY-timing test — rerun once if it alone fails.)

- [ ] **Step 3: Commit**

```bash
git add docs/manual-tests/waybar.md
git commit -m "docs: manual test checklist for the elephant workspace menu"
```
