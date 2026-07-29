# macOS Menubar (SwiftBar) Indicator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A SwiftBar-hosted macOS menubar item mirroring the Linux waybar indicator: workspace count + attention color in the bar, a dropdown of per-workspace rows with PR/dirty/diff indicators, per-row submenus (Jump / Open PR / Copy path / Reveal in Finder), plus `wsx setup menubar`.

**Architecture:** SwiftBar is the long-lived host; wsx stays a short-lived data provider. New `#[cfg(target_os = "macos")] src/menubar/` module beside `src/waybar/`. The render path (`wsx menubar plugin`, polled every 10s) reads sqlite only; git/PR facts are recomputed by a detached throttled sweep (`wsx menubar refresh`). Platform-neutral pieces (row collection, IPC socket protocol, attention ranking) are extracted from the Linux-gated waybar tree into shared modules first.

**Tech Stack:** Rust (existing deps only: rusqlite, serde, futures, dirs, shlex, libc, tokio). No new crates. SwiftBar line protocol for output; `osascript`/`open`/`lsappinfo`/`defaults` CLI tools at runtime.

**Spec:** `docs/superpowers/specs/2026-07-29-macos-menubar-design.md`

## Global Constraints

- No menubar code in core modules: `src/menubar/` is `#[cfg(target_os = "macos")]`; core never imports it. Non-macOS `wsx menubar …` returns: `wsx menubar is only available on macOS (SwiftBar integration)`.
- Linux builds must stay green after the refactors: `src/waybar/` keeps compiling and its tests keep passing (CI matrix covers Linux + macOS).
- The plugin render path must not fork git/gh subprocesses; cache reads only.
- `wsx menubar plugin` never fails: any error → icon-only header line, exit 0.
- Every user-controlled string (repo, slug, branch, status message, path) is sanitized before entering a SwiftBar line: control chars → space, `|` → `¦`.
- No `clap`: extend the hand-rolled parser in `src/cli.rs` following its existing patterns (`GROUPS`, flat `CliAction`, per-group `parse_*`).
- Gate check before commit, every task: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`.
- Commit messages: conventional commits, no Co-Authored-By/Generated-with trailers (user rule).

---

### Task 1: Extract shared `workspace_rows` module from the waybar tree

Pure move/refactor — no behavior change. Everything platform-neutral that the
menubar will need leaves the `#[cfg(target_os = "linux")]` tree.

**Files:**
- Create: `src/workspace_rows.rs`
- Modify: `src/lib.rs` (add `pub mod workspace_rows;` — ungated)
- Modify: `src/waybar/entries.rs` (remove moved items, import from new module)
- Modify: `src/waybar/menu.rs` (remove `sanitize`, import from new module)
- Modify: `src/waybar/status.rs` (use shared `attention_rank`/`state_glyph`)

**Interfaces:**
- Consumes: `Store`, `ScmCacheRow`, `ReportedStatus`, `crate::git::*` (all existing).
- Produces (later tasks rely on these exact names):
  - `pub struct RowInput { pub repo_name: String, pub slug: String, pub branch: String, pub worktree_path: PathBuf, pub status: Option<ReportedStatus>, pub cache: ScmCacheRow }` — note the NEW `worktree_path` field (menubar submenu actions need it; waybar ignores it).
  - `pub async fn collect_rows_fresh(store: &Store) -> Result<Vec<RowInput>>` (today's `collect_rows`: fresh git facts + write-through)
  - `pub async fn run_refresh_prs(store: &Store) -> Result<()>`
  - `pub fn is_stale(fetched_at: Option<i64>, now: i64, throttle_secs: i64) -> bool`
  - `pub fn sanitize(s: &str) -> String`
  - `pub fn attention_rank(state: ReportedState) -> u8` (waybar's `rank`)
  - `pub fn state_glyph(state: Option<ReportedState>) -> &'static str` (waybar's `status::glyph`)
  - `pub fn unix_now() -> i64`
  - `pub const PR_REFRESH_THROTTLE_SECS: i64 = 120;`
  - `pub(crate) const GIT_FACTS_CONCURRENCY: usize = 8;`
  - `pub(crate) async fn gather_git_facts(worktree: PathBuf) -> Option<(bool, crate::git::DiffStats)>`

- [ ] **Step 1: Create `src/workspace_rows.rs`** by moving, verbatim except for visibility/renames noted, from the waybar tree:
  - From `src/waybar/entries.rs`: `PR_REFRESH_THROTTLE_SECS`, `GIT_FACTS_CONCURRENCY`, `RowInput` (add `pub worktree_path: PathBuf`), `gather_git_facts`, `collect_rows` (rename `collect_rows_fresh`; populate the new `worktree_path` field — the tuple in `metas` already carries it), `unix_now`, `run_refresh_prs`, and `needs_pr_refresh` generalized to:

```rust
/// True when `fetched_at` is missing or older than `throttle_secs`.
/// Future timestamps (clock skew) count as fresh — don't refetch.
pub fn is_stale(fetched_at: Option<i64>, now: i64, throttle_secs: i64) -> bool {
    match fetched_at {
        None => true,
        Some(t) => now.saturating_sub(t) >= throttle_secs,
    }
}
```

  - From `src/waybar/menu.rs`: `sanitize` (make it `pub`).
  - From `src/waybar/status.rs`: `rank` → `pub fn attention_rank`, `glyph` → `pub fn state_glyph`.
  - Module doc comment: `//! Platform-neutral workspace row collection shared by the Linux waybar and macOS menubar integrations.`
  - Move the `throttle_decision` test from `entries.rs` here, updated:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staleness_decision() {
        assert!(is_stale(None, 1000, 120));
        assert!(is_stale(Some(880), 1000, 120));
        assert!(!is_stale(Some(881), 1000, 120));
        assert!(!is_stale(Some(2000), 1000, 120)); // clock skew: don't refetch
    }
}
```

- [ ] **Step 2: Update the waybar tree to consume the shared module.**
  - `src/lib.rs`: add `pub mod workspace_rows;` (alphabetical position, ungated).
  - `src/waybar/entries.rs`: delete moved items; `use crate::workspace_rows::{collect_rows_fresh, is_stale, sanitize, unix_now, RowInput, PR_REFRESH_THROTTLE_SECS};` and re-point call sites (`run_menu_entries` calls `collect_rows_fresh`; the old `needs_pr_refresh(f, now)` calls in `run_refresh_prs` moved out with it). `run_refresh_prs` in entries.rs becomes a thin re-export so the CLI dispatch keeps working unchanged: `pub use crate::workspace_rows::run_refresh_prs;`.
  - `src/waybar/entries.rs` `icon_glyph`: change `crate::waybar::status::glyph(other)` → `crate::workspace_rows::state_glyph(other)`.
  - `src/waybar/menu.rs`: replace the local `sanitize` with `pub(crate) use crate::workspace_rows::sanitize;`.
  - `src/waybar/status.rs`: delete `rank`/`glyph`, `use crate::workspace_rows::{attention_rank, state_glyph};`, update call sites (`rank(st.state)` → `attention_rank(st.state)`, `glyph(...)` → `state_glyph(...)`).
  - `entries.rs` test `collect_rows_sorted_and_composed`: call `crate::workspace_rows::collect_rows_fresh` instead of `super::collect_rows`.

- [ ] **Step 3: Run the gate**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: PASS on macOS. Note: `src/waybar/` is Linux-gated, so this machine does not compile it — Linux verification happens in Step 4.

- [ ] **Step 4: Cross-check the Linux build**

Run: `rustup target add x86_64-unknown-linux-gnu && cargo check --target x86_64-unknown-linux-gnu`
Expected: PASS (compile-only; no linking). If the target's std or a C cross-compile for rusqlite is unavailable on this machine, note it in the commit body and rely on CI's Linux job — do not skip silently.

- [ ] **Step 5: Commit**

```bash
git add src/lib.rs src/workspace_rows.rs src/waybar/
git commit -m "refactor: extract platform-neutral workspace rows from waybar tree"
```

---

### Task 2: `scm_cache` v19 — `pr_url` + `git_fetched_at` columns, PR URL fetch

**Files:**
- Modify: `src/data/schema.rs` (migration v19)
- Modify: `src/data/scm_cache.rs` (row fields, upsert signatures, `clear_scm_git`)
- Modify: `src/git/forge.rs` (`PrStatus.url`, gh `--json` field)
- Modify: `src/app/background.rs:384` (call site)
- Modify: `src/workspace_rows.rs` (call sites moved in Task 1)
- Possibly: `src/commands/shared.rs` (PrStatus loses `Copy`)

**Interfaces:**
- Produces:
  - `ScmCacheRow` gains `pub pr_url: Option<String>`, `pub git_fetched_at: Option<i64>`; derives become `(Debug, Clone, PartialEq, Eq, Default)` — `Copy` is dropped (String field).
  - `Store::upsert_scm_pr(&self, id: WorkspaceId, lifecycle: BranchLifecycle, number: Option<u32>, url: Option<&str>, fetched_at: i64) -> Result<()>`
  - `Store::upsert_scm_git(&self, id: WorkspaceId, dirty: bool, additions: u32, deletions: u32, git_fetched_at: i64) -> Result<()>`
  - `Store::clear_scm_git(&self, id: WorkspaceId, git_fetched_at: i64) -> Result<()>` — NULLs dirty/additions/deletions, stamps `git_fetched_at`.
  - `PrStatus` gains `pub url: Option<String>`; derives become `(Debug, Clone, PartialEq, Eq)` — `Copy` dropped.

- [ ] **Step 1: Write failing tests** in `src/data/scm_cache.rs`'s test module:

```rust
#[test]
fn v19_columns_round_trip() {
    let (store, id) = store_with_workspace();
    store
        .upsert_scm_pr(
            id,
            BranchLifecycle::PrOpen,
            Some(12),
            Some("https://github.com/o/r/pull/12"),
            1000,
        )
        .unwrap();
    store.upsert_scm_git(id, true, 4, 2, 2000).unwrap();
    let row = store.all_scm_cache().unwrap()[&id].clone();
    assert_eq!(row.pr_url.as_deref(), Some("https://github.com/o/r/pull/12"));
    assert_eq!(row.git_fetched_at, Some(2000));
    assert_eq!(row.fetched_at, Some(1000));
}

#[test]
fn clear_scm_git_nulls_git_fields_and_stamps_time() {
    let (store, id) = store_with_workspace();
    store.upsert_scm_git(id, true, 4, 2, 1000).unwrap();
    store
        .upsert_scm_pr(id, BranchLifecycle::PrOpen, Some(7), None, 1000)
        .unwrap();
    store.clear_scm_git(id, 3000).unwrap();
    let row = store.all_scm_cache().unwrap()[&id].clone();
    assert_eq!(row.dirty, None);
    assert_eq!(row.additions, None);
    assert_eq!(row.deletions, None);
    assert_eq!(row.git_fetched_at, Some(3000));
    // PR fields untouched.
    assert_eq!(row.pr_number, Some(7));
}
```

And in `src/git/forge.rs`'s test module (find the existing `parse_gh_pr_status` tests and extend):

```rust
#[test]
fn parse_carries_pr_url() {
    let s = parse_gh_pr_status(
        r#"{"state":"OPEN","isDraft":false,"mergeable":"MERGEABLE","number":5,"url":"https://github.com/o/r/pull/5"}"#,
    )
    .unwrap();
    assert_eq!(s.url.as_deref(), Some("https://github.com/o/r/pull/5"));
    // Absent url stays None.
    let s = parse_gh_pr_status(r#"{"state":"MERGED","number":9}"#).unwrap();
    assert_eq!(s.url, None);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test scm_cache v19_columns 2>&1 | tail -5` (and the forge test)
Expected: COMPILE ERROR (wrong arity / missing fields) — that counts as the failing state for a signature change.

- [ ] **Step 3: Implement.**
  - `src/data/schema.rs`, after the `v < 18` block:

```rust
if v < 19 {
    self.add_column_if_missing("scm_cache", "pr_url", "pr_url TEXT")?;
    self.add_column_if_missing("scm_cache", "git_fetched_at", "git_fetched_at INTEGER")?;
    self.conn().execute("PRAGMA user_version = 19", [])?;
}
```

  - `src/data/scm_cache.rs`: add the two fields to `ScmCacheRow` (adjust derives), extend `all_scm_cache`'s SELECT to `… deletions, fetched_at, pr_url, git_fetched_at` and read them (`pr_url: r.get(7)?, git_fetched_at: r.get(8)?` — keep field-order match), extend `upsert_scm_pr` (SET `pr_url = excluded.pr_url`) and `upsert_scm_git` (SET `git_fetched_at = excluded.git_fetched_at`), and add:

```rust
pub fn clear_scm_git(&self, id: WorkspaceId, git_fetched_at: i64) -> Result<()> {
    self.conn().execute(
        "INSERT INTO scm_cache (workspace_id, git_fetched_at) VALUES (?1, ?2)
         ON CONFLICT(workspace_id) DO UPDATE SET
           dirty = NULL, additions = NULL, deletions = NULL,
           git_fetched_at = excluded.git_fetched_at",
        rusqlite::params![id.0, git_fetched_at],
    )?;
    Ok(())
}
```

  - `src/git/forge.rs`: add `#[serde(default)] url: Option<String>` to `GhPrView`, `pub url: Option<String>` to `PrStatus` (drop `Copy`), add `url` to the `gh pr view --json` field list, and populate it in `parse_gh_pr_status`.
  - Fix call sites: `src/app/background.rs:384` → `.upsert_scm_pr(id, status.lifecycle, status.number, status.url.as_deref(), now_ms / 1000)`; `src/workspace_rows.rs` `run_refresh_prs` → `upsert_scm_pr(ws.id, status.lifecycle, status.number, status.url.as_deref(), unix_now())`; `collect_rows_fresh` write-through → `upsert_scm_git(id, dirty, stats.added, stats.removed, unix_now())`.
  - `Copy` fallout: `cargo check` will flag them — expect `src/data/scm_cache.rs` tests indexing `all_scm_cache().unwrap()[&id]` (add `.clone()`), possibly `src/commands/shared.rs` around lines 115–130 (clone or borrow as needed). Fix mechanically; do not restructure.

- [ ] **Step 4: Run the gate**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: PASS, including the new tests.

- [ ] **Step 5: Cross-check Linux** (waybar entries call `upsert_scm_git` via the moved code — arity changed)

Run: `cargo check --target x86_64-unknown-linux-gnu`
Expected: PASS (same fallback rule as Task 1 Step 4).

- [ ] **Step 6: Commit**

```bash
git add src/data/ src/git/forge.rs src/app/background.rs src/workspace_rows.rs src/commands/
git commit -m "feat(scm-cache): pr_url and git_fetched_at columns, PR url fetch"
```

---

### Task 3: Cache-only row collection + throttled git-facts sweep

**Files:**
- Modify: `src/workspace_rows.rs`
- Test: same file, `#[cfg(test)] mod tests`

**Interfaces:**
- Produces:
  - `pub fn collect_rows_cached(store: &Store) -> Result<Vec<RowInput>>` — sync; store + scm_cache reads only, no git subprocesses, no write-through. Sorted repo-name-then-slug like `collect_rows_fresh`.
  - `pub async fn refresh_git_facts(store: &Store) -> Result<()>` — recompute git facts for workspaces whose `git_fetched_at` is stale (> `GIT_REFRESH_THROTTLE_SECS`); git failure → `clear_scm_git`.
  - `pub const GIT_REFRESH_THROTTLE_SECS: i64 = 60;`

- [ ] **Step 1: Write failing tests** in `src/workspace_rows.rs`:

```rust
#[tokio::test]
async fn cached_collect_reads_cache_without_git() {
    use crate::data::store::{NewWorkspace, Store};
    use crate::pty::session::AgentKind;

    let store = Store::open_in_memory().unwrap();
    let repo = store
        .add_repo(std::path::Path::new("/tmp/r"), "r", "x")
        .unwrap();
    let id = store
        .insert_workspace(&NewWorkspace {
            repo_id: repo,
            name: "w",
            branch: "x/w",
            // Nonexistent on purpose: cached collect must not care.
            worktree_path: &std::path::PathBuf::from("/nonexistent/r/w"),
            yolo: false,
            agent: AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    store.upsert_scm_git(id, true, 4, 2, 1000).unwrap();

    let rows = collect_rows_cached(&store).unwrap();
    assert_eq!(rows.len(), 1);
    // Cache values pass through untouched — fresh mode would have
    // suppressed them because git fails on the missing worktree.
    assert_eq!(rows[0].cache.dirty, Some(true));
    assert_eq!(rows[0].cache.additions, Some(4));
    assert_eq!(rows[0].worktree_path.display().to_string(), "/nonexistent/r/w");
}

#[tokio::test]
async fn refresh_git_facts_clears_failed_and_skips_fresh() {
    use crate::data::store::{NewWorkspace, Store};
    use crate::pty::session::AgentKind;

    let store = Store::open_in_memory().unwrap();
    let repo = store
        .add_repo(std::path::Path::new("/tmp/r"), "r", "x")
        .unwrap();
    let mut ids = vec![];
    for name in ["stale", "fresh"] {
        ids.push(
            store
                .insert_workspace(&NewWorkspace {
                    repo_id: repo,
                    name,
                    branch: &format!("x/{name}"),
                    worktree_path: &std::path::PathBuf::from(format!("/nonexistent/{name}")),
                    yolo: false,
                    agent: AgentKind::Claude,
                    shared: false,
                })
                .unwrap(),
        );
    }
    // stale: git_fetched_at long ago → swept; git fails → cleared, restamped.
    store.upsert_scm_git(ids[0], true, 4, 2, 0).unwrap();
    // fresh: stamped now → skipped, indicators survive even though the
    // worktree is equally nonexistent.
    store.upsert_scm_git(ids[1], true, 9, 9, unix_now()).unwrap();

    refresh_git_facts(&store).await.unwrap();

    let caches = store.all_scm_cache().unwrap();
    let stale = caches[&ids[0]].clone();
    assert_eq!(stale.dirty, None, "failed git must clear stale indicators");
    assert!(stale.git_fetched_at.unwrap() > 0, "sweep must restamp");
    let fresh = caches[&ids[1]].clone();
    assert_eq!(fresh.dirty, Some(true), "fresh row must not be swept");
    assert_eq!(fresh.additions, Some(9));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test workspace_rows 2>&1 | tail -5`
Expected: COMPILE ERROR — `collect_rows_cached` / `refresh_git_facts` not found.

- [ ] **Step 3: Implement** in `src/workspace_rows.rs`. Refactor so both collectors share the metadata walk:

```rust
pub const GIT_REFRESH_THROTTLE_SECS: i64 = 60;

struct WsMeta {
    id: crate::data::store::WorkspaceId,
    repo_name: String,
    slug: String,
    branch: String,
    worktree_path: PathBuf,
}

/// Workspace metadata sorted by repo name then workspace name.
fn workspace_metas(store: &Store) -> Result<Vec<WsMeta>> {
    let mut repos = crate::data::repo::list(store)?;
    repos.sort_by(|a, b| a.name.cmp(&b.name));
    let mut metas = Vec::new();
    for repo in &repos {
        let mut workspaces = store.workspaces(repo.id)?;
        workspaces.sort_by(|a, b| a.name.cmp(&b.name));
        for ws in workspaces {
            metas.push(WsMeta {
                id: ws.id,
                repo_name: repo.name.clone(),
                slug: ws.name,
                branch: ws.branch,
                worktree_path: ws.worktree_path,
            });
        }
    }
    Ok(metas)
}

/// Cache-only rows: store + scm_cache reads, zero subprocesses. The
/// menubar plugin renders from this on every poll.
pub fn collect_rows_cached(store: &Store) -> Result<Vec<RowInput>> {
    let statuses = store.all_workspace_status()?;
    let mut caches = store.all_scm_cache()?;
    Ok(workspace_metas(store)?
        .into_iter()
        .map(|m| RowInput {
            status: statuses.get(&m.id).cloned(),
            cache: caches.remove(&m.id).unwrap_or_default(),
            repo_name: m.repo_name,
            slug: m.slug,
            branch: m.branch,
            worktree_path: m.worktree_path,
        })
        .collect())
}

/// Recompute git facts for every workspace whose git_fetched_at is stale.
/// Bounded fan-out; git failure clears the row's git fields (a stale ● is
/// worse than none). Silent by contract, like run_refresh_prs.
pub async fn refresh_git_facts(store: &Store) -> Result<()> {
    let now = unix_now();
    let caches = store.all_scm_cache()?;
    let targets: Vec<WsMeta> = workspace_metas(store)?
        .into_iter()
        .filter(|m| {
            caches
                .get(&m.id)
                .is_none_or(|c| is_stale(c.git_fetched_at, now, GIT_REFRESH_THROTTLE_SECS))
        })
        .collect();
    use futures::StreamExt;
    let facts: Vec<_> = futures::stream::iter(
        targets
            .iter()
            .map(|m| gather_git_facts(m.worktree_path.clone())),
    )
    .buffered(GIT_FACTS_CONCURRENCY)
    .collect()
    .await;
    for (m, fact) in targets.into_iter().zip(facts) {
        match fact {
            Some((dirty, stats)) => {
                let _ = store.upsert_scm_git(m.id, dirty, stats.added, stats.removed, now);
            }
            None => {
                let _ = store.clear_scm_git(m.id, now);
            }
        }
    }
    Ok(())
}
```

Also rewrite `collect_rows_fresh` to build on `workspace_metas` (same zip
pattern as today, keeping its suppress-in-memory-on-git-failure semantics
and write-through). Behavior identical; the entries.rs test
`collect_rows_sorted_and_composed` still passes unchanged.

- [ ] **Step 4: Run the gate**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/workspace_rows.rs
git commit -m "feat(rows): cache-only collection and throttled git-facts sweep"
```

---

### Task 4: Un-gate the TUI IPC socket (Linux → all unix)

**Files:**
- Create: `src/tui_ipc.rs` (moved from `src/waybar/ipc.rs`, content unchanged)
- Delete: `src/waybar/ipc.rs`
- Modify: `src/lib.rs`, `src/waybar/mod.rs`, `src/waybar/jump.rs`, `src/main.rs`

**Interfaces:**
- Produces: `crate::tui_ipc::{socket_dir, socket_path_for, live_socket_candidates, parse_line, handle_line, listen}` — exact same signatures as today's `crate::waybar::ipc::*`. The TUI now listens on macOS too (jump depends on this).

- [ ] **Step 1: Move the file.** `git mv src/waybar/ipc.rs src/tui_ipc.rs`. Update its module doc: it serves both waybar and menubar jumpers. No logic changes; the tests move with it.

- [ ] **Step 2: Rewire.**
  - `src/lib.rs`: add `#[cfg(unix)]\npub mod tui_ipc;` (alphabetical).
  - `src/waybar/mod.rs`: remove `pub mod ipc;`.
  - `src/waybar/jump.rs`: `crate::waybar::ipc::live_socket_candidates()` → `crate::tui_ipc::live_socket_candidates()`.
  - `src/main.rs`: change both `#[cfg(target_os = "linux")]` blocks (spawn at ~line 105, socket cleanup at ~line 127) to `#[cfg(unix)]`, and `wsx::waybar::ipc::` → `wsx::tui_ipc::`.

- [ ] **Step 3: Run the gate.** The ipc tests (`parse_line_handles_spaces_in_repo_names`, `socket_path_shape`, `handle_line_selects_workspace`) now run on macOS — first time this code compiles here.

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test tui_ipc`
Expected: PASS. Then full `cargo test`: PASS.

- [ ] **Step 4: Cross-check Linux**

Run: `cargo check --target x86_64-unknown-linux-gnu`
Expected: PASS (same fallback rule as Task 1 Step 4).

- [ ] **Step 5: Commit**

```bash
git add src/lib.rs src/tui_ipc.rs src/waybar/ src/main.rs
git commit -m "refactor: un-gate TUI ipc socket to all unix platforms"
```

---

### Task 5: Menubar plugin document renderer + refresh entry point

The heart of the feature: pure functions that render the SwiftBar document,
plus the never-fails `print_plugin` entry and the `run_refresh` sweep.

**Files:**
- Create: `src/menubar/mod.rs`, `src/menubar/plugin.rs`, `src/menubar/refresh.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `workspace_rows::{collect_rows_cached, attention_rank, state_glyph, sanitize, RowInput}`, `ScmCacheRow`, `BranchLifecycle`, `ReportedState`.
- Produces:
  - `pub fn print_plugin(db_path: &Path)` — prints the document, exit-0 semantics, spawns the detached refresh.
  - `pub async fn run_refresh(store: &Store) -> Result<()>` in `refresh.rs`.
  - `pub(crate) fn render(repo_names: &[String], rows: &[RowInput], wsx_bin: &str) -> String` and helpers (`header_line`, `row_line`, `submenu_lines`, `esc_text`, `quote_param`) — unit-test surface.

SwiftBar protocol notes (for the implementer — the whole task hangs on these):
- Document = header line(s), then `---`, then menu lines. A line is `text | param=value param=value…`. Lines without `bash=`/`href=` render as non-clickable text.
- Submenu items are the parent line's following lines prefixed `--`.
- `sfimage=<sf-symbol-name>` puts an SF Symbol on the item; `sfcolor=#light,#dark` tints it per appearance.
- Action params: `bash="/abs/path" param1=a param2=b terminal=false` (each argv element its own paramN); `href=<url>` opens a URL.
- Values containing spaces must be double-quoted; we quote ALL `bash`/`paramN`/`href` values unconditionally.

- [ ] **Step 1: Write the failing tests** — create `src/menubar/plugin.rs` containing ONLY the test module first (functions referenced don't exist yet), `src/menubar/mod.rs` with `pub mod plugin;\npub mod refresh;`, an empty-for-now `src/menubar/refresh.rs` (just `use crate::data::store::Store; use crate::error::Result;`), and in `src/lib.rs`: `#[cfg(target_os = "macos")]\npub mod menubar;`.

```rust
#[cfg(test)]
mod plugin_tests {
    use super::*;
    use crate::data::scm_cache::ScmCacheRow;
    use crate::data::store::{ReportedState, ReportedStatus};
    use crate::git::forge::BranchLifecycle;
    use crate::workspace_rows::RowInput;

    fn status(state: ReportedState, msg: Option<&str>) -> ReportedStatus {
        ReportedStatus {
            state,
            message: msg.map(str::to_string),
            source: "test".into(),
            reported_at: 0,
        }
    }

    fn row(repo: &str, slug: &str) -> RowInput {
        RowInput {
            repo_name: repo.into(),
            slug: slug.into(),
            branch: format!("x/{slug}"),
            worktree_path: format!("/wt/{repo}/{slug}").into(),
            status: None,
            cache: ScmCacheRow::default(),
        }
    }

    #[test]
    fn header_counts_and_colors_by_worst_state() {
        // Idle: count, symbol, no color.
        assert_eq!(header_line(4, None), "4 | sfimage=arrow.triangle.branch");
        // Blocked outranks working → red pair.
        let h = header_line(2, Some(ReportedState::Blocked));
        assert!(h.starts_with("2 | sfimage=arrow.triangle.branch"), "{h}");
        assert!(h.contains("sfcolor=#c92a2a,#ff6b6b"), "{h}");
    }

    #[test]
    fn error_header_is_icon_only() {
        assert_eq!(error_header(), "| sfimage=arrow.triangle.branch");
    }

    #[test]
    fn row_line_composes_indicators() {
        let mut r = row("r", "fix-bug");
        r.status = Some(status(ReportedState::Working, None));
        r.cache = ScmCacheRow {
            pr_lifecycle: Some(BranchLifecycle::PrConflicted),
            pr_number: Some(123),
            dirty: Some(true),
            additions: Some(45),
            deletions: Some(12),
            ..Default::default()
        };
        let line = row_line(&r);
        assert!(line.starts_with("\u{21bb} fix-bug"), "{line}");
        assert!(line.contains("#123 conflict"), "{line}");
        assert!(line.contains('\u{25cf}'), "{line}");
        assert!(line.contains("+45 -12"), "{line}");
        assert!(line.ends_with("| font=SFMono-Regular size=12"), "{line}");
    }

    #[test]
    fn pr_field_rules_match_linux() {
        // Unknown and NoPr render identically: nothing.
        assert_eq!(pr_field(&ScmCacheRow::default()), "");
        let no_pr = ScmCacheRow {
            pr_lifecycle: Some(BranchLifecycle::NoPr),
            fetched_at: Some(0),
            ..Default::default()
        };
        assert_eq!(pr_field(&no_pr), "");
        // Open: bare number. Merged/draft/conflict/closed: word.
        for (l, expect) in [
            (BranchLifecycle::PrOpen, "#7"),
            (BranchLifecycle::PrDraft, "#7 draft"),
            (BranchLifecycle::PrConflicted, "#7 conflict"),
            (BranchLifecycle::PrMerged, "#7 merged"),
            (BranchLifecycle::PrClosed, "#7 closed"),
        ] {
            let c = ScmCacheRow {
                pr_lifecycle: Some(l),
                pr_number: Some(7),
                ..Default::default()
            };
            assert_eq!(pr_field(&c), expect, "{l:?}");
        }
    }

    #[test]
    fn clean_default_row_is_just_glyph_and_slug() {
        let line = row_line(&row("r", "w"));
        assert!(line.starts_with("\u{b7} w |"), "{line}");
        assert!(!line.contains('#'), "{line}");
    }

    #[test]
    fn submenu_has_jump_first_and_pr_only_when_cached() {
        let mut r = row("meals backend", "api-fix");
        r.status = Some(status(ReportedState::Blocked, Some("needs input")));
        let lines = submenu_lines(&r, "/usr/local/bin/wsx");
        // Subtitle first: branch — state: message.
        assert_eq!(lines[0], "-- x/api-fix \u{2014} blocked: needs input");
        assert_eq!(
            lines[1],
            "-- Jump | bash=\"/usr/local/bin/wsx\" param1=\"menubar\" param2=\"jump\" param3=\"meals backend\" param4=\"api-fix\" terminal=false"
        );
        assert!(!lines.iter().any(|l| l.contains("Open PR")), "{lines:?}");
        assert!(
            lines.iter().any(|l| l.starts_with("-- Copy worktree path | bash=")
                && l.contains("param2=\"copy-path\"")),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains("Reveal in Finder | bash=\"/usr/bin/open\" param1=\"-R\"")),
            "{lines:?}"
        );

        r.cache.pr_number = Some(12);
        r.cache.pr_url = Some("https://github.com/o/r/pull/12".into());
        let lines = submenu_lines(&r, "/usr/local/bin/wsx");
        assert!(
            lines
                .iter()
                .any(|l| l == "-- Open PR #12 in browser | href=\"https://github.com/o/r/pull/12\""),
            "{lines:?}"
        );
    }

    #[test]
    fn text_cannot_inject_lines_or_params() {
        // A hostile status message: newline (new menu row) and pipe (param
        // separator) must both be neutralized.
        let mut r = row("r", "w");
        r.status = Some(status(ReportedState::Working, Some("evil\n-- fake | bash=\"/bin/rm\"")));
        for line in submenu_lines(&r, "/bin/wsx") {
            assert!(!line.contains('\n'), "{line}");
        }
        let subtitle = &submenu_lines(&r, "/bin/wsx")[0];
        assert!(subtitle.contains('\u{00a6}'), "pipe not neutralized: {subtitle}");
        assert!(!subtitle.contains(" | bash"), "{subtitle}");
    }

    #[test]
    fn render_groups_by_repo_and_lists_empty_repos() {
        let rows = vec![row("alpha", "one"), row("alpha", "two"), row("beta", "b1")];
        let doc = render(
            &["alpha".into(), "beta".into(), "empty".into()],
            &rows,
            "/bin/wsx",
        );
        let lines: Vec<&str> = doc.lines().collect();
        assert_eq!(lines[0], "3 | sfimage=arrow.triangle.branch");
        assert_eq!(lines[1], "---");
        let alpha = lines.iter().position(|l| *l == "alpha").unwrap();
        let beta = lines.iter().position(|l| *l == "beta").unwrap();
        let empty = lines.iter().position(|l| *l == "empty").unwrap();
        assert!(alpha < beta && beta < empty);
        assert_eq!(lines[empty + 1], "(no workspaces)");
        // Footer.
        assert_eq!(*lines.last().unwrap(), "Refresh | refresh=true");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test menubar 2>&1 | tail -5`
Expected: COMPILE ERROR — functions not defined.

- [ ] **Step 3: Implement `src/menubar/plugin.rs`** above the tests:

```rust
//! `wsx menubar plugin`: renders the full SwiftBar document (header line,
//! separator, menu body) from cache-only workspace rows. Never fails —
//! errors degrade to an icon-only header. After printing, spawns the
//! detached `wsx menubar refresh` sweep so indicators self-heal.
//! See docs/superpowers/specs/2026-07-29-macos-menubar-design.md.

use std::path::Path;

use crate::data::scm_cache::ScmCacheRow;
use crate::data::store::{ReportedState, ReportedStatus, Store};
use crate::error::Result;
use crate::git::forge::BranchLifecycle;
use crate::workspace_rows::{attention_rank, collect_rows_cached, sanitize, state_glyph, RowInput};

const SF_SYMBOL: &str = "arrow.triangle.branch";
const ROW_FONT: &str = "font=SFMono-Regular size=12";

/// light,dark hex pairs for SwiftBar's per-appearance sfcolor.
fn sfcolor(state: ReportedState) -> &'static str {
    match state {
        ReportedState::Blocked => "#c92a2a,#ff6b6b",
        ReportedState::Done => "#2f9e44,#69db7c",
        ReportedState::Waiting => "#b08800,#ffd43b",
        ReportedState::Working | ReportedState::Busy => "#1971c2,#4dabf7",
    }
}

/// Line text sanitizer: control chars collapse (via sanitize) and the
/// protocol's text/params separator '|' becomes a broken bar, so no
/// user-controlled string can smuggle params or extra rows.
fn esc_text(s: &str) -> String {
    sanitize(s).replace('|', "\u{00a6}")
}

/// All bash=/paramN=/href= values are double-quoted; interior quotes
/// degrade to '\'' (a path with a double quote is pathological — keeping
/// the protocol unbreakable beats preserving it).
fn quote_param(s: &str) -> String {
    format!("\"{}\"", esc_text(s).replace('"', "'"))
}

pub(crate) fn error_header() -> String {
    format!("| sfimage={SF_SYMBOL}")
}

pub(crate) fn header_line(count: usize, best: Option<ReportedState>) -> String {
    match best {
        Some(state) => format!("{count} | sfimage={SF_SYMBOL} sfcolor={}", sfcolor(state)),
        None => format!("{count} | sfimage={SF_SYMBOL}"),
    }
}

pub(crate) fn pr_field(c: &ScmCacheRow) -> String {
    let word = match c.pr_lifecycle {
        Some(BranchLifecycle::PrDraft) => "draft",
        Some(BranchLifecycle::PrConflicted) => "conflict",
        Some(BranchLifecycle::PrMerged) => "merged",
        Some(BranchLifecycle::PrClosed) => "closed",
        Some(BranchLifecycle::PrOpen) => "",
        Some(BranchLifecycle::NoPr) | None => return String::new(),
    };
    match (c.pr_number, word) {
        (Some(n), "") => format!("#{n}"),
        (Some(n), w) => format!("#{n} {w}"),
        (None, "") => String::new(),
        (None, w) => w.to_string(),
    }
}

pub(crate) fn row_line(r: &RowInput) -> String {
    let mut cols = vec![format!(
        "{} {}",
        state_glyph(r.status.as_ref().map(|s| s.state)),
        esc_text(&r.slug)
    )];
    let pr = pr_field(&r.cache);
    if !pr.is_empty() {
        cols.push(pr);
    }
    if r.cache.dirty == Some(true) {
        cols.push("\u{25cf}".into());
    }
    if let (Some(a), Some(d)) = (r.cache.additions, r.cache.deletions)
        && (a > 0 || d > 0)
    {
        cols.push(format!("+{a} -{d}"));
    }
    format!("{} | {ROW_FONT}", cols.join("  "))
}

/// `branch — state: message`, the info the Linux menu shows as subtext.
fn subtitle(r: &RowInput) -> String {
    let b = r.branch.clone();
    match &r.status {
        None => b,
        Some(s) => match s.message.as_deref().filter(|m| !m.is_empty()) {
            Some(m) => format!("{b} \u{2014} {}: {}", s.state.as_str(), m),
            None => format!("{b} \u{2014} {}", s.state.as_str()),
        },
    }
}

pub(crate) fn submenu_lines(r: &RowInput, wsx_bin: &str) -> Vec<String> {
    let wt = r.worktree_path.display().to_string();
    let mut out = vec![format!("-- {}", esc_text(&subtitle(r)))];
    out.push(format!(
        "-- Jump | bash={} param1=\"menubar\" param2=\"jump\" param3={} param4={} terminal=false",
        quote_param(wsx_bin),
        quote_param(&r.repo_name),
        quote_param(&r.slug),
    ));
    if let (Some(n), Some(url)) = (r.cache.pr_number, r.cache.pr_url.as_deref()) {
        out.push(format!("-- Open PR #{n} in browser | href={}", quote_param(url)));
    }
    out.push(format!(
        "-- Copy worktree path | bash={} param1=\"menubar\" param2=\"copy-path\" param3={} param4={} terminal=false",
        quote_param(wsx_bin),
        quote_param(&r.repo_name),
        quote_param(&r.slug),
    ));
    out.push(format!(
        "-- Reveal in Finder | bash=\"/usr/bin/open\" param1=\"-R\" param2={} terminal=false",
        quote_param(&wt),
    ));
    out
}

pub(crate) fn render(repo_names: &[String], rows: &[RowInput], wsx_bin: &str) -> String {
    let best = rows
        .iter()
        .filter_map(|r| r.status.as_ref().map(|s| s.state))
        .max_by_key(|s| attention_rank(*s));
    let mut lines = vec![header_line(rows.len(), best), "---".into()];
    for repo in repo_names {
        lines.push(esc_text(repo));
        let mut any = false;
        for r in rows.iter().filter(|r| &r.repo_name == repo) {
            any = true;
            lines.push(row_line(r));
            lines.extend(submenu_lines(r, wsx_bin));
        }
        if !any {
            lines.push("(no workspaces)".into());
        }
    }
    lines.push("---".into());
    lines.push("Refresh | refresh=true".into());
    lines.join("\n")
}

fn plugin_document(store: &Store, wsx_bin: &str) -> Result<String> {
    let mut repos = crate::data::repo::list(store)?;
    repos.sort_by(|a, b| a.name.cmp(&b.name));
    let names: Vec<String> = repos.into_iter().map(|r| r.name).collect();
    let rows = collect_rows_cached(store)?;
    Ok(render(&names, &rows, wsx_bin))
}

/// Fire-and-forget `wsx menubar refresh` so indicators self-heal by a
/// later poll (same contract as the waybar PR sweep).
fn spawn_refresh(wsx_bin: &str) {
    use std::process::Stdio;
    let _ = std::process::Command::new(wsx_bin)
        .args(["menubar", "refresh"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// Never fails: SwiftBar polls this; on any error print the bare symbol
/// and exit 0 so the bar shows a quiet idle item, never an error string.
pub fn print_plugin(db_path: &Path) {
    let wsx_bin = crate::install_common::preferred_wsx_bin(dirs::home_dir());
    match Store::open(db_path).and_then(|s| plugin_document(&s, &wsx_bin)) {
        Ok(doc) => println!("{doc}"),
        Err(_) => println!("{}", error_header()),
    }
    spawn_refresh(&wsx_bin);
}
```

NOTE: `print_plugin` references `crate::install_common::preferred_wsx_bin`,
which Task 7 creates. For THIS task only, use
`std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "wsx".into())`
inline instead, with a `// Task 7 swaps this for preferred_wsx_bin` comment;
Task 7 does the swap. (The tests above don't touch `print_plugin`, so either
form passes.)

And `src/menubar/refresh.rs`:

```rust
//! `wsx menubar refresh`: detached throttled sweep refreshing git facts
//! and PR state into scm_cache. Silent by contract.

use crate::data::store::Store;
use crate::error::Result;

pub async fn run_refresh(store: &Store) -> Result<()> {
    crate::workspace_rows::refresh_git_facts(store).await?;
    crate::workspace_rows::run_refresh_prs(store).await
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test menubar`
Expected: PASS. Then full `cargo test`: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/lib.rs src/menubar/
git commit -m "feat(menubar): SwiftBar plugin document renderer and refresh sweep"
```

---

### Task 6: Menubar jump — IPC select, app focus, terminal spawn, copy-path

**Files:**
- Create: `src/menubar/jump.rs`
- Modify: `src/menubar/mod.rs` (add `pub mod jump;`)

**Interfaces:**
- Consumes: `crate::tui_ipc::live_socket_candidates` (Task 4), `Store::get_setting` (existing).
- Produces:
  - `pub fn jump(repo: &str, slug: &str, terminal_cmd: Option<&str>) -> Result<()>` — `terminal_cmd` is the raw `terminal_cmd` setting; caller (Task 8) fetches it.
  - `pub fn copy_path(store: &Store, repo: &str, slug: &str) -> Result<()>`
  - `pub(crate) fn notify(msg: &str)` — osascript notification + stderr.

- [ ] **Step 1: Write the failing tests** (bottom of the new `src/menubar/jump.rs`; start the file with just tests + `use` lines):

```rust
#[cfg(test)]
mod jump_tests {
    use super::*;

    #[test]
    fn applescript_str_escapes_quotes_and_backslashes() {
        assert_eq!(applescript_str(r#"say "hi" \now"#), r#""say \"hi\" \\now""#);
    }

    #[test]
    fn parse_bundle_id_variants() {
        assert_eq!(
            parse_bundle_id("\"CFBundleIdentifier\"=\"com.googlecode.iterm2\"\n"),
            Some("com.googlecode.iterm2".into())
        );
        assert_eq!(parse_bundle_id(""), None);
        assert_eq!(parse_bundle_id("\"CFBundleIdentifier\"=\"[ NULL ]\""), None);
        assert_eq!(parse_bundle_id("garbage without equals"), None);
    }

    #[test]
    fn ancestor_pids_walks_ps() {
        let chain = ancestor_pids(std::process::id());
        assert_eq!(chain.first(), Some(&std::process::id()));
        assert!(chain.len() >= 2, "expected self + parent, got {chain:?}");
        assert!(chain.len() <= 32);
    }

    #[test]
    fn terminal_template_requires_cmd_placeholder() {
        // With {cmd}: substituted. Without: None → caller falls through to
        // the osascript paths (a bare `open -a iTerm` can't carry a command).
        assert_eq!(
            resolve_terminal_template(Some("alacritty -e {cmd}"), "wsx --select r/s"),
            Some("alacritty -e wsx --select r/s".into())
        );
        assert_eq!(resolve_terminal_template(Some("open -a iTerm"), "x"), None);
        assert_eq!(resolve_terminal_template(None, "x"), None);
        assert_eq!(resolve_terminal_template(Some("  "), "x"), None);
    }

    #[test]
    fn copy_path_unknown_workspace_errors() {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        assert!(copy_path(&store, "nope", "missing").is_err());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test menubar::jump 2>&1 | tail -5`
Expected: COMPILE ERROR.

- [ ] **Step 3: Implement** above the tests:

```rust
//! `wsx menubar jump`: select the workspace in a running TUI over the
//! shared unix socket (focusing its terminal app), or spawn a fresh
//! terminal running `wsx --select repo/slug`. Also `copy-path`, the
//! pbcopy action for the SwiftBar submenu.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::data::store::Store;
use crate::error::{Error, Result};

pub fn jump(repo: &str, slug: &str, terminal_cmd: Option<&str>) -> Result<()> {
    for (path, pid) in crate::tui_ipc::live_socket_candidates() {
        match std::os::unix::net::UnixStream::connect(&path) {
            Ok(mut stream) => {
                if writeln!(stream, "select {repo} {slug}").is_ok() {
                    focus_app_of(pid);
                    return Ok(());
                }
            }
            Err(_) => {
                // Stale socket from a killed TUI.
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    let res = spawn_tui(repo, slug, terminal_cmd);
    if let Err(e) = &res {
        notify(&format!("jump failed: {e}"));
    }
    res
}

/// osascript notification + stderr — the macOS notify-send analogue.
pub(crate) fn notify(msg: &str) {
    eprintln!("wsx: {msg}");
    let _ = Command::new("/usr/bin/osascript")
        .args([
            "-e",
            &format!(
                "display notification {} with title \"wsx\"",
                applescript_str(msg)
            ),
        ])
        .status();
}

fn applescript_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn ppid_of(pid: u32) -> Option<u32> {
    let out = Command::new("/bin/ps")
        .args(["-o", "ppid=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// self → parent → … , capped at 32 like the Linux /proc walk.
fn ancestor_pids(pid: u32) -> Vec<u32> {
    let mut chain = vec![pid];
    let mut current = pid;
    while chain.len() < 32 {
        let Some(ppid) = ppid_of(current) else { break };
        if ppid <= 1 {
            break;
        }
        chain.push(ppid);
        current = ppid;
    }
    chain
}

/// Parse `lsappinfo info -only bundleid <pid>` output:
/// `"CFBundleIdentifier"="com.googlecode.iterm2"`. Empty / `[ NULL ]`
/// means the pid isn't an app process.
fn parse_bundle_id(s: &str) -> Option<String> {
    let (_, rhs) = s.split_once('=')?;
    let v = rhs.trim().trim_matches('"').trim();
    if v.is_empty() || v == "[ NULL ]" {
        return None;
    }
    Some(v.to_string())
}

/// Best-effort: walk the TUI's ancestor chain, find the first pid that is
/// a real app (lsappinfo knows it), activate it. Any failure → silent
/// skip; the selection already happened (mirror of the hyprctl path).
fn focus_app_of(tui_pid: u32) {
    for pid in ancestor_pids(tui_pid) {
        let Ok(out) = Command::new("/usr/bin/lsappinfo")
            .args(["info", "-only", "bundleid", &pid.to_string()])
            .output()
        else {
            return;
        };
        if let Some(bundle) = parse_bundle_id(&String::from_utf8_lossy(&out.stdout)) {
            let _ = Command::new("/usr/bin/open").args(["-b", &bundle]).status();
            return;
        }
    }
}

fn shquote(s: &str) -> String {
    shlex::try_quote(s)
        .map(|c| c.into_owned())
        .unwrap_or_else(|_| format!("'{}'", s.replace(['\'', '\0'], "")))
}

/// terminal_cmd is honored only when it carries a `{cmd}` placeholder —
/// a bare app-open command (`open -a iTerm`) cannot run a command, and
/// guessing an argv position would misfire.
fn resolve_terminal_template(configured: Option<&str>, cmd: &str) -> Option<String> {
    let t = configured?.trim();
    if t.is_empty() || !t.contains("{cmd}") {
        return None;
    }
    Some(t.replace("{cmd}", cmd))
}

fn iterm_installed() -> bool {
    std::path::Path::new("/Applications/iTerm.app").exists()
        || dirs::home_dir().is_some_and(|h| h.join("Applications/iTerm.app").exists())
}

fn spawn_detached(prog: &str, args: &[&str]) -> Result<()> {
    let mut cmd = Command::new(prog);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Own session so it outlives the SwiftBar action process (same
    // pattern as waybar::jump::spawn_tui).
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    cmd.spawn()
        .map_err(|e| Error::UserInput(format!("failed to launch '{prog}': {e}")))?;
    Ok(())
}

fn spawn_tui(repo: &str, slug: &str, terminal_cmd: Option<&str>) -> Result<()> {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("wsx"));
    let cmd = format!(
        "{} --select {}",
        shquote(&exe.display().to_string()),
        shquote(&format!("{repo}/{slug}"))
    );
    if let Some(full) = resolve_terminal_template(terminal_cmd, &cmd) {
        return spawn_detached("/bin/sh", &["-c", &full]);
    }
    if iterm_installed() {
        return spawn_detached(
            "/usr/bin/osascript",
            &[
                "-e",
                &format!(
                    "tell application \"iTerm2\" to create window with default profile command {}",
                    applescript_str(&cmd)
                ),
                "-e",
                "tell application \"iTerm2\" to activate",
            ],
        );
    }
    spawn_detached(
        "/usr/bin/osascript",
        &[
            "-e",
            &format!(
                "tell application \"Terminal\" to do script {}",
                applescript_str(&cmd)
            ),
            "-e",
            "tell application \"Terminal\" to activate",
        ],
    )
}

/// Resolve the worktree path from the store (fresh — paths can move) and
/// pipe it to pbcopy.
pub fn copy_path(store: &Store, repo: &str, slug: &str) -> Result<()> {
    for r in crate::data::repo::list(store)? {
        if r.name != repo {
            continue;
        }
        for ws in store.workspaces(r.id)? {
            if ws.name == slug {
                let mut child = Command::new("/usr/bin/pbcopy")
                    .stdin(Stdio::piped())
                    .spawn()
                    .map_err(|e| Error::UserInput(format!("pbcopy failed: {e}")))?;
                child
                    .stdin
                    .take()
                    .expect("piped stdin")
                    .write_all(ws.worktree_path.display().to_string().as_bytes())?;
                child.wait()?;
                return Ok(());
            }
        }
    }
    Err(Error::UserInput(format!("unknown workspace {repo}/{slug}")))
}
```

Add `pub mod jump;` to `src/menubar/mod.rs`.

- [ ] **Step 4: Run the gate**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/menubar/
git commit -m "feat(menubar): jump via TUI ipc with app focus, terminal spawn, copy-path"
```

---

### Task 7: `install_common` + the `wsx setup menubar` installer

**Files:**
- Create: `src/install_common.rs`, `src/menubar/install.rs`
- Modify: `src/lib.rs` (`pub(crate) mod install_common;` — ungated), `src/waybar/install.rs` (use shared helpers), `src/menubar/mod.rs` (`pub mod install;`), `src/menubar/plugin.rs` (swap in `preferred_wsx_bin` per Task 5's note)

**Interfaces:**
- Produces:
  - `pub(crate) fn install_common::write_atomic(path: &Path, content: &str) -> Result<()>` (moved verbatim from `waybar/install.rs`)
  - `pub(crate) fn install_common::preferred_wsx_bin(home: Option<PathBuf>) -> String` (moved verbatim, doc comment updated to mention both installers)
  - `pub fn menubar::install::run() -> Result<Vec<String>>` — what `wsx setup menubar` calls
  - `pub(crate) fn menubar::install::parse_plugin_dir(defaults_stdout: &str, home: Option<&Path>) -> Option<PathBuf>`
  - `pub fn menubar::install::install_into(dir: &Path, wsx_bin: &str) -> Result<Vec<String>>`

- [ ] **Step 1: Move the shared helpers.** Create `src/install_common.rs` with `write_atomic` and `preferred_wsx_bin` cut verbatim from `src/waybar/install.rs` (visibility `pub(crate)`); in `waybar/install.rs` replace them with `use crate::install_common::{preferred_wsx_bin, write_atomic};`. Move `preferred_wsx_bin`'s test along if it has one (check `install_tests`). Add `pub(crate) mod install_common;` to `src/lib.rs`. In `src/menubar/plugin.rs`, swap the `current_exe()` placeholder for `crate::install_common::preferred_wsx_bin(dirs::home_dir())` (Task 5's note).

- [ ] **Step 2: Write failing tests** in the new `src/menubar/install.rs`:

```rust
#[cfg(test)]
mod install_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parse_plugin_dir_trims_and_expands_tilde() {
        let home = Path::new("/Users/u");
        assert_eq!(
            parse_plugin_dir("/Users/u/SwiftBar\n", Some(home)),
            Some("/Users/u/SwiftBar".into())
        );
        assert_eq!(
            parse_plugin_dir("~/Library/SwiftBar\n", Some(home)),
            Some("/Users/u/Library/SwiftBar".into())
        );
        assert_eq!(parse_plugin_dir("", Some(home)), None);
        assert_eq!(parse_plugin_dir("   \n", Some(home)), None);
    }

    #[test]
    fn shim_execs_quoted_wsx_plugin() {
        let s = shim("/opt/my tools/wsx");
        assert!(s.starts_with("#!/bin/sh\n"), "{s}");
        assert!(s.contains("exec '/opt/my tools/wsx' menubar plugin"), "{s}");
    }

    #[test]
    fn install_into_writes_executable_shim_idempotently() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        install_into(dir.path(), "/usr/local/bin/wsx").unwrap();
        // Re-run: overwrite, not error (refreshes the baked path).
        install_into(dir.path(), "/usr/local/bin/wsx2").unwrap();
        let path = dir.path().join(SHIM_NAME);
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("/usr/local/bin/wsx2"), "{content}");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o755, 0o755, "shim must be executable");
    }
}
```

(Confirm `tempfile` is already a dev-dependency — `rg tempfile Cargo.toml`; the waybar install tests almost certainly use it. If not, use `std::env::temp_dir()` + manual cleanup instead of adding a dep.)

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test menubar::install 2>&1 | tail -5`
Expected: COMPILE ERROR.

- [ ] **Step 4: Implement `src/menubar/install.rs`:**

```rust
//! `wsx setup menubar` installer: writes the SwiftBar plugin shim into the
//! SwiftBar plugin directory (resolved from SwiftBar's defaults domain)
//! and asks SwiftBar to reload. Conservative like the waybar installer:
//! when the directory can't be resolved, print instructions, never guess.

use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::install_common::{preferred_wsx_bin, write_atomic};

/// Filename encodes SwiftBar's refresh interval.
pub(crate) const SHIM_NAME: &str = "wsx-menubar.10s.sh";

fn shim(wsx_bin: &str) -> String {
    let quoted = shlex::try_quote(wsx_bin)
        .map(|c| c.into_owned())
        .unwrap_or_else(|_| wsx_bin.to_string());
    format!(
        "#!/bin/sh\n# Installed by `wsx setup menubar`. Re-run it after moving wsx.\nexec {quoted} menubar plugin\n"
    )
}

/// `defaults read com.ameba.SwiftBar PluginDirectory` output → path.
/// Handles trailing newline and a leading `~`.
pub(crate) fn parse_plugin_dir(defaults_stdout: &str, home: Option<&Path>) -> Option<PathBuf> {
    let raw = defaults_stdout.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return home.map(|h| h.join(rest));
    }
    Some(PathBuf::from(raw))
}

fn plugin_dir() -> Option<PathBuf> {
    let out = std::process::Command::new("/usr/bin/defaults")
        .args(["read", "com.ameba.SwiftBar", "PluginDirectory"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_plugin_dir(
        &String::from_utf8_lossy(&out.stdout),
        dirs::home_dir().as_deref(),
    )
}

pub fn install_into(dir: &Path, wsx_bin: &str) -> Result<Vec<String>> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(SHIM_NAME);
    write_atomic(&path, &shim(wsx_bin))?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    Ok(vec![format!("wrote {}", path.display())])
}

pub fn run() -> Result<Vec<String>> {
    let wsx_bin = preferred_wsx_bin(dirs::home_dir());
    match plugin_dir() {
        Some(dir) => {
            let mut lines = install_into(&dir, &wsx_bin)?;
            // Best-effort hot reload; -g keeps SwiftBar in the background.
            let reloaded = std::process::Command::new("/usr/bin/open")
                .args(["-g", "swiftbar://refreshallplugins"])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            lines.push(if reloaded {
                "asked SwiftBar to reload plugins".into()
            } else {
                "reload SwiftBar plugins: open -g 'swiftbar://refreshallplugins'".into()
            });
            Ok(lines)
        }
        None => Ok(vec![
            "SwiftBar not configured (no PluginDirectory in its defaults domain)".into(),
            "install it: brew install swiftbar — then launch it once and pick a plugin folder".into(),
            format!("then re-run: wsx setup menubar (installs {SHIM_NAME} into that folder)"),
        ]),
    }
}
```

Add `pub mod install;` to `src/menubar/mod.rs`.

- [ ] **Step 5: Run the gate + Linux cross-check** (waybar/install.rs changed)

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo check --target x86_64-unknown-linux-gnu`
Expected: PASS (Linux check: same fallback rule as Task 1 Step 4).

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/install_common.rs src/waybar/install.rs src/menubar/
git commit -m "feat(menubar): swiftbar plugin installer, shared install helpers"
```

---

### Task 8: CLI wiring — `menubar` group, `setup menubar`, dispatch

**Files:**
- Modify: `src/cli.rs` (GROUPS, `CliAction`, `parse_setup`, new `parse_menubar`, `run_cli` dispatch, help/parse tests)

**Interfaces:**
- Consumes: `menubar::plugin::print_plugin`, `menubar::jump::{jump, copy_path}`, `menubar::refresh::run_refresh`, `menubar::install::run` (Tasks 5–7).
- Produces: `CliAction::{MenubarPlugin, MenubarJump{repo,slug}, MenubarCopyPath{repo,slug}, MenubarRefresh, SetupMenubar}` — parsed on ALL platforms (uniform help), executed only on macOS.

- [ ] **Step 1: Write the failing parse/help tests** in `src/cli.rs`'s test module (mirror `parses_waybar_commands` at cli.rs:3080 and `waybar_group_help_renders` at :3126):

```rust
#[test]
fn parses_menubar_commands() {
    assert!(matches!(
        parse(&["menubar", "plugin"]),
        Ok(CliAction::MenubarPlugin)
    ));
    match parse(&["menubar", "jump", "meals backend", "api-fix"]) {
        Ok(CliAction::MenubarJump { repo, slug }) => {
            assert_eq!(repo, "meals backend");
            assert_eq!(slug, "api-fix");
        }
        other => panic!("{other:?}"),
    }
    match parse(&["menubar", "copy-path", "r", "s"]) {
        Ok(CliAction::MenubarCopyPath { repo, slug }) => {
            assert_eq!(repo, "r");
            assert_eq!(slug, "s");
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        parse(&["menubar", "refresh"]),
        Ok(CliAction::MenubarRefresh)
    ));
    assert!(parse(&["menubar", "jump", "onlyrepo"]).is_err());
    assert!(parse(&["menubar", "bogus"]).is_err());
    assert!(parse(&["menubar"]).is_err());
}

#[test]
fn parses_setup_menubar() {
    assert!(matches!(
        parse(&["setup", "menubar"]),
        Ok(CliAction::SetupMenubar)
    ));
}

#[test]
fn menubar_group_help_renders() {
    let h = render_group_help("menubar");
    assert!(h.contains("wsx menubar —"));
    assert!(h.contains("plugin"));
    assert!(h.contains("copy-path"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test parses_menubar 2>&1 | tail -5`
Expected: COMPILE ERROR (missing variants).

- [ ] **Step 3: Implement**, mirroring the waybar precedent at every layer:
  - **GROUPS** (after the `waybar` entry at cli.rs:224–248):

```rust
GroupInfo {
    name: "menubar",
    blurb: "macOS menubar (SwiftBar) status module and workspace jumper",
    commands: &[
        CmdInfo {
            usage: "plugin",
            blurb: "Print the SwiftBar plugin document",
        },
        CmdInfo {
            usage: "jump <repo> <slug>",
            blurb: "Select the workspace in a running TUI, or launch one",
        },
        CmdInfo {
            usage: "copy-path <repo> <slug>",
            blurb: "Copy the workspace's worktree path to the clipboard",
        },
        CmdInfo {
            usage: "refresh",
            blurb: "Refresh cached git/PR indicators for all workspaces",
        },
    ],
},
```

  - **setup group** commands array (cli.rs:176–185): add `CmdInfo { usage: "menubar", blurb: "Install the SwiftBar plugin shim" },`.
  - **CliAction** (after `WaybarRefreshPrs` at :444): `SetupMenubar, MenubarPlugin, MenubarJump { repo: String, slug: String }, MenubarCopyPath { repo: String, slug: String }, MenubarRefresh,`.
  - **parse_args** group dispatch (~:600): `"menubar" => parse_menubar(&mut it).map_err(|e| tag_group(e, group)),`.
  - **parse_setup** (:1109): add `Some("menubar") => Ok(CliAction::SetupMenubar),`.
  - **parse_menubar** (after `parse_waybar`):

```rust
fn parse_menubar(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("plugin") => Ok(CliAction::MenubarPlugin),
        Some("jump") => {
            let (Some(repo), Some(slug)) = (it.next(), it.next()) else {
                return Err(Error::Usage {
                    group: None,
                    msg: "jump needs <repo> <slug>".into(),
                });
            };
            Ok(CliAction::MenubarJump { repo, slug })
        }
        Some("copy-path") => {
            let (Some(repo), Some(slug)) = (it.next(), it.next()) else {
                return Err(Error::Usage {
                    group: None,
                    msg: "copy-path needs <repo> <slug>".into(),
                });
            };
            Ok(CliAction::MenubarCopyPath { repo, slug })
        }
        Some("refresh") => Ok(CliAction::MenubarRefresh),
        other => Err(Error::Usage {
            group: None,
            msg: match other {
                Some(cmd) => format!("unknown menubar command: {cmd}"),
                None => "missing menubar command".into(),
            },
        }),
    }
}
```

  - **run_cli early blocks** (beside the WaybarStatus/SetupWaybar blocks at :1304–1323 — before the store opens, mirroring their `#[cfg]` shape exactly):

```rust
if matches!(action, CliAction::MenubarPlugin) {
    #[cfg(target_os = "macos")]
    {
        crate::menubar::plugin::print_plugin(&dirs.db_path());
        return Ok(());
    }
    #[cfg(not(target_os = "macos"))]
    return Err(menubar_macos_only());
}
if matches!(action, CliAction::SetupMenubar) {
    #[cfg(target_os = "macos")]
    {
        for line in crate::menubar::install::run()? {
            println!("{line}");
        }
        return Ok(());
    }
    #[cfg(not(target_os = "macos"))]
    return Err(menubar_macos_only());
}
```

  - **main match arms** (beside the waybar arms at :1880–1891):

```rust
#[cfg(target_os = "macos")]
CliAction::MenubarJump { repo, slug } => {
    let terminal_cmd = store.get_setting("terminal_cmd")?;
    crate::menubar::jump::jump(&repo, &slug, terminal_cmd.as_deref())?
}
#[cfg(target_os = "macos")]
CliAction::MenubarCopyPath { repo, slug } => {
    crate::menubar::jump::copy_path(&store, &repo, &slug)?
}
#[cfg(target_os = "macos")]
CliAction::MenubarRefresh => crate::menubar::refresh::run_refresh(&store).await?,
#[cfg(not(target_os = "macos"))]
CliAction::MenubarJump { .. } | CliAction::MenubarCopyPath { .. } | CliAction::MenubarRefresh => {
    return Err(menubar_macos_only());
}
```

  Extend the early-handled unreachable arm at :1892 with `| CliAction::MenubarPlugin | CliAction::SetupMenubar`.
  - **Error helper** (beside `waybar_linux_only` at :1903):

```rust
fn menubar_macos_only() -> Error {
    Error::UserInput("wsx menubar is only available on macOS (SwiftBar integration)".into())
}
```

  - Check `store.get_setting("terminal_cmd")` — cli.rs:513–514 shows `terminal_cmd` is a known settings key; find how existing dispatch reads settings (grep `get_setting` in cli.rs) and match that pattern exactly (it may go through a config helper rather than raw `get_setting`).

- [ ] **Step 4: Run the gate + smoke test**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: PASS.
Run: `cargo run -- menubar plugin | head -20`
Expected: a header line with `sfimage=`, `---`, repo sections with rows for this machine's real workspaces (this session's own repo should appear). Also `cargo run -- menubar --help` renders the group help.

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): wsx menubar group and setup menubar wiring"
```

---

### Task 9: Docs, manual test, live install verification

**Files:**
- Create: `docs/manual-tests/menubar.md`
- Modify: `README.md` (menubar section beside the waybar one at :48–65)

- [ ] **Step 1: README.** Read the waybar section (README.md:48–65) and add a sibling section after it:

```markdown
### macOS menubar (SwiftBar)

A menubar item mirroring the waybar indicator: workspace count tinted by the
most attention-worthy agent status, with a dropdown of per-workspace rows
(PR state, dirty marker, diff stats) and per-row actions — jump to the
workspace in a running `wsx` TUI (or launch one), open its PR, copy or
reveal its worktree.

    brew install swiftbar   # if you don't have it; launch once, pick a plugin folder
    wsx setup menubar

The plugin refreshes every 10s from cache; git/PR indicators are swept in
the background (≤ ~70s staleness). Jump prefers a running TUI via its unix
socket and falls back to spawning your terminal — set
`wsx config set terminal_cmd '<cmd with {cmd}>'` to control which one, or
let it use iTerm2/Terminal automatically.
```

(Adjust wording/format to match the actual waybar section's style after reading it.)

- [ ] **Step 2: Manual test doc.** Write `docs/manual-tests/menubar.md` modeled on `docs/manual-tests/waybar.md` (read it first; keep its structure):

```markdown
# Manual test: macOS menubar (SwiftBar)

Prereqs: SwiftBar installed and running, `wsx` at ~/.local/bin/wsx,
at least one repo with workspaces, `gh` authed for PR indicators.

1. `wsx setup menubar` — reports the shim path; SwiftBar shows the wsx
   item (branch symbol + workspace count) within ~10s.
2. Header color: `wsx status set blocked --message t` in some workspace →
   item turns red within a poll; `wsx status clear` → returns to default.
   Check both light and dark system appearance.
3. Menu: click the item — repos as section headers (empty repos listed
   with "(no workspaces)"), one row per workspace with status glyph and
   slug; monospace alignment; a workspace with an open PR shows #N after
   the background sweep (≤ ~2 min or next TUI poll).
4. Dirty indicator: touch a file in a worktree → ● appears within ~70s
   (poll + sweep); revert → disappears.
5. Submenu: hover a row — subtitle (branch — state: message), Jump,
   Open PR (only when cached), Copy worktree path (pbpaste to verify),
   Reveal in Finder.
6. Jump, TUI running: open `wsx` in a terminal, Jump from the menu →
   the TUI selects+attaches that workspace and the terminal app comes
   frontmost.
7. Jump, no TUI: quit all wsx TUIs, Jump → a new terminal window (iTerm2
   if installed, else Terminal; terminal_cmd template if configured)
   opens running `wsx --select repo/slug`.
8. Error path: `mv ~/.local/state/wsx/state.db{,.bak}` → item degrades to
   icon-only (no error text); restore the db.
9. Repo/slug with a space or `|` in a status message renders without
   breaking rows or params.
```

- [ ] **Step 3: Live verification (this machine).** Run `cargo build --release` (or `cargo install --path .` if that's the documented flow — check README) so `~/.local/bin/wsx` (or the current install) has the feature; run `wsx setup menubar`; walk manual-test items 1–5 at minimum, plus 6 or 7. Record any deviations — fix code or docs before committing. If SwiftBar is not installed and the user hasn't approved installing it, run `wsx setup menubar` anyway to verify the not-installed hint path, note the limitation, and flag it in the final report instead of silently skipping.

- [ ] **Step 4: Full gate one last time**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo check --target x86_64-unknown-linux-gnu`
Expected: PASS (Linux check: same fallback rule as Task 1 Step 4).

- [ ] **Step 5: Commit**

```bash
git add README.md docs/manual-tests/menubar.md
git commit -m "docs: macOS menubar section and manual test"
```

---

## Plan self-review notes

- Spec coverage: plugin renderer (Task 5), refresh sweep (Tasks 3+5), jump/focus/terminal-spawn/notify (Task 6), copy-path (Tasks 6+8), installer (Task 7), CLI isolation + non-macOS error (Task 8), scm_cache pr_url/git_fetched_at (Task 2), IPC un-gating (Task 4), entries split (Task 1), README + manual tests (Task 9). Spec's "second subtitle line at top of submenu" = `submenu_lines`' first element. Spec's TUI-write-through freshness bonus needs no work (background.rs already writes through; Task 2 only widens the call).
- The `-- ---`-style submenu separators, SwiftBar streaming, and refresh-on-open are deliberately absent (spec non-goals).
- Type consistency: `RowInput`/`ScmCacheRow` fields, `upsert_scm_pr`/`upsert_scm_git`/`clear_scm_git` arities, and `jump(repo, slug, terminal_cmd)` are each defined once (Tasks 1–3, 6) and consumed with matching signatures in Tasks 5–8.
- Known judgment calls for the implementer: exact `ReportedState::as_str()` casing in subtitles (reuse, don't re-derive); if `lsappinfo` output format differs on this OS version, fix `parse_bundle_id` against real output and extend its test.
