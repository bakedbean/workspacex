# Workspace Series Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Record handoff chains as numbered workspace series that outlive archiving, and show `part N · <feature>` plus the earlier parts' PRs in the repo focus view.

**Architecture:** Two new SQLite tables (`series`, `series_parts`) with no FK to `workspaces`, owned by a new `src/data/series.rs`. `wsx workspace create --continues <slug>` joins a series. Store-level hooks keep part rows current on rename, PR write-through, archive and repo removal. The dashboard's PR poll also settles archived parts whose PR is still open. The focus view reads series from `App` and draws one `part` prose line.

**Tech Stack:** Rust, rusqlite, ratatui, tokio; `gh` CLI for PR state.

**Spec:** `docs/superpowers/specs/2026-11-17-workspace-series-design.md`

## Global Constraints

- Schema goes to v31. `SCHEMA_VERSION` in `src/data/schema.rs` = 31; the new step is `if v < 31 { execute_batch(SCHEMA_V31_SERIES); PRAGMA user_version = 31 }`.
- `series_parts` and `series` have **no** FK to `workspaces`; part rows must survive `Store::delete_workspace`.
- Nothing new is read from GitHub except `gh pr view <number>` for unsettled archived parts.
- A series is a sequence: a new part always gets `MAX(part) + 1`.
- New series start with `title = NULL`; display falls back to part 1's slug.
- Lifecycle strings in `series_parts.pr_lifecycle` use `scm_cache::lifecycle_to_str` / `lifecycle_from_str`.
- Glyphs and colors for PRs on the part line come from `crate::ui::theme::lifecycle_chip` and `Theme::lifecycle_style` (merged is `⏺` in the merged color, not the spec sketch's `✓`).
- Comments terse (2–4 lines). No Claude attribution in commits.
- No stacked PRs in this repo: all tasks land on this branch, one PR.

## Review Focus

1. **Parent workspace renamed after joining**: `series show` and the part line show the new slug and branch, never the old ones. Covered in Task 2 (`rename_updates_part_slug_and_branch`).
2. **`--continues` naming the new workspace's own slug, or a workspace in another repo**: create fails before any worktree is made. Covered in Task 3 (`continues_unknown_or_other_repo_fails_before_worktree`).
3. **Archiving with no dashboard running, PR merged since last poll**: the part stays unsettled until the poll settles it, and then it reads merged. Covered in Task 4 (`archived_open_part_settles_to_merged`).
4. **Very long title or many earlier parts on a narrow terminal**: the part line stays one line. The title is cut with `…`, and the oldest PRs collapse into `+k`. Covered in Task 5 (`part_line_fits_one_line_when_crowded`).
5. **Series whose part 1 is archived and has no title**: the fallback title is part 1's stored slug, not empty. Covered in Task 5 (`part_line_falls_back_to_part_one_slug`).

---

### Task 1: Series schema and core store API

**Files:**
- Create: `src/data/series.rs`
- Modify: `src/data/mod.rs` (add `pub mod series;`)
- Modify: `src/data/schema.rs` (const `SCHEMA_V31_SERIES`, step, `SCHEMA_VERSION = 31`)
- Test: `src/data/series.rs` (`#[cfg(test)] mod tests`, helper modelled on `store_with_workspace` in `src/data/scm_cache.rs:269`)

**Interfaces:**
- Produces (all in `crate::data::series`):
  - `pub struct SeriesId(pub i64)` (Copy, Eq, Hash, Debug)
  - `pub struct SeriesPart { pub series_id: SeriesId, pub part: u32, pub workspace_id: WorkspaceId, pub slug: String, pub branch: String, pub pr_number: Option<u32>, pub pr_lifecycle: Option<BranchLifecycle>, pub archived_at: Option<i64> }`
  - `pub struct Series { pub id: SeriesId, pub repo_id: RepoId, pub title: Option<String>, pub parts: Vec<SeriesPart> }` (parts sorted by `part` ascending)
  - `impl Series { pub fn display_title(&self) -> &str }`: the title, or else part 1's slug.
  - `pub struct SeriesJoin { pub series_id: SeriesId, pub part: u32, pub started: bool, pub title: Option<String> }`
  - `impl Store`:
    - `pub fn continue_series(&self, parent: WorkspaceId, child: WorkspaceId) -> Result<SeriesJoin>`
    - `pub fn series_for_workspace(&self, id: WorkspaceId) -> Result<Option<Series>>`
    - `pub fn all_series(&self) -> Result<Vec<Series>>`
    - `pub fn set_series_title(&self, id: SeriesId, title: Option<&str>) -> Result<()>`

SQL is the spec's `series` / `series_parts` DDL verbatim, plus `CREATE INDEX IF NOT EXISTS idx_series_parts_ws ON series_parts(workspace_id)` (the UNIQUE already indexes it; skip if redundant).

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn continuing_a_loose_workspace_starts_a_series() {
    let (store, repo) = store_with_repo();
    let a = ws(&store, repo, "a"); let b = ws(&store, repo, "b");
    let j = store.continue_series(a, b).unwrap();
    assert!(j.started); assert_eq!(j.part, 2); assert_eq!(j.title, None);
    let s = store.series_for_workspace(a).unwrap().unwrap();
    assert_eq!(s.parts.iter().map(|p| (p.part, p.slug.as_str())).collect::<Vec<_>>(),
               vec![(1, "a"), (2, "b")]);
    assert_eq!(s.display_title(), "a");
}

#[test]
fn continuing_a_part_appends_and_branches_in_creation_order() {
    // a→b, then b→c and b→d: c is part 3, d is part 4; j.started == false
}

#[test]
fn title_set_and_cleared() {
    // set_series_title(Some("Focus view")) → display_title "Focus view";
    // set_series_title(None) → falls back to part 1 slug
}

#[test]
fn migration_reaches_v31_and_is_idempotent() {
    // Store::open_in_memory(); PRAGMA user_version == 31; tables exist
}
```

`continue_series` with an unknown parent or child id returns `Err` (add `continue_series_unknown_workspace_errs`).

- [ ] **Step 2: Run tests, verify they fail**

Run: `cargo test --lib data::series`
Expected: compile error / FAIL (module missing).

- [ ] **Step 3: Implement schema step and `series.rs`**

`continue_series` runs in one transaction (`self.conn().unchecked_transaction()` or the pattern the store already uses). It reads both workspaces' `repo_id`, `name` and `branch` from `workspaces`. If the parent has no part row, it inserts a `series` row (title NULL) and parent part 1. It then inserts the child at `MAX(part)+1`. Part rows store `slug = workspaces.name` and `branch = workspaces.branch`. `pr_*` and `archived_at` start NULL. `all_series` makes one pass over `series` LEFT JOIN `series_parts`, ordered by `series.id, part`.

- [ ] **Step 4: Run tests, verify they pass**

Run: `cargo test --lib data::series && cargo test --lib data::schema`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/data/series.rs src/data/mod.rs src/data/schema.rs
git commit -m "feat(data): series tables and continue_series store API"
```

---

### Task 2: Keep part rows current (rename, PR write-through, archive, repo removal)

**Files:**
- Modify: `src/data/store.rs:223-260` (`delete_workspace`, `rename_workspace`, `set_workspace_branch`)
- Modify: `src/data/scm_cache.rs:77` (`upsert_scm_pr`)
- Modify: `src/data/repo.rs:82` (`remove_repo`)
- Modify: `src/data/series.rs` (archived-part queries)
- Test: `src/data/series.rs` tests

**Interfaces:**
- Consumes: Task 1 types.
- Produces (in `crate::data::series`):
  - `pub struct UnsettledPart { pub series_id: SeriesId, pub part: u32, pub repo_path: PathBuf, pub pr_number: u32 }`
  - `impl Store { pub fn unsettled_archived_parts(&self) -> Result<Vec<UnsettledPart>>; pub fn set_part_pr(&self, series: SeriesId, part: u32, status: &PrStatus) -> Result<()> }`

Behavior:
- `rename_workspace` / `set_workspace_branch` also `UPDATE series_parts SET slug/branch = ? WHERE workspace_id = ?`.
- `upsert_scm_pr` also `UPDATE series_parts SET pr_number = ?, pr_lifecycle = ? WHERE workspace_id = ?`. It's a no-op outside a series.
- `delete_workspace`, **before** deleting `scm_cache`: `UPDATE series_parts SET archived_at = now_ms()/1000, pr_number = COALESCE((SELECT pr_number FROM scm_cache WHERE workspace_id = ?1), pr_number), pr_lifecycle = COALESCE((SELECT pr_lifecycle …), pr_lifecycle) WHERE workspace_id = ?1`. Both archive paths (`workspace.rs:560`, `:658`) go through here.
- `remove_repo`: `DELETE FROM series_parts WHERE series_id IN (SELECT id FROM series WHERE repo_id = ?1)`, then `DELETE FROM series WHERE repo_id = ?1`.
- `unsettled_archived_parts`: `archived_at IS NOT NULL AND pr_number IS NOT NULL AND (pr_lifecycle IS NULL OR pr_lifecycle NOT IN ('merged','closed'))`, using the actual strings `lifecycle_to_str` yields for `PrMerged` / `PrClosed`, with `repo_path` joined from `repos.path`.
- `set_part_pr` writes `pr_number` (if `Some`) and `pr_lifecycle`.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn rename_updates_part_slug_and_branch() {
    // continue_series(a,b); rename_workspace(a,"a2"); set_workspace_branch(a,"x/a2")
    // series_for_workspace(b).parts[0] == ("a2","x/a2")
}
#[test] fn pr_write_through_updates_live_part() {
    // upsert_scm_pr(b, open #12) → part 2 pr_number 12, lifecycle PrOpen
}
#[test] fn archive_snapshots_pr_and_keeps_part() {
    // upsert_scm_pr(a, merged #10); delete_workspace(a)
    // series_for_workspace(b).parts[0]: archived_at.is_some(), pr_number 10, PrMerged
}
#[test] fn archived_unsettled_parts_are_listed_until_settled() {
    // a archived with open #10 → listed with repo_path; set_part_pr(merged) → not listed
}
#[test] fn failed_workspace_archive_marks_part_without_pr() {
    // continue_series(a,b); delete_workspace(b) with no scm_cache row
    // part 2 archived_at.is_some(), pr_number None; not in unsettled list
}
#[test] fn remove_repo_deletes_its_series() { /* all_series() empty afterwards */ }
```

- [ ] **Step 2: Run, verify fail**: `cargo test --lib data::series` → FAIL on the new tests.
- [ ] **Step 3: Implement the hooks and queries listed above.**
- [ ] **Step 4: Run, verify pass**: `cargo test --lib data::` → PASS (the existing scm_cache, store and repo tests stay green).
- [ ] **Step 5: Commit**

```bash
git add src/data/
git commit -m "feat(data): keep series parts current through rename, PR writes, archive"
```

---

### Task 3: CLI: `--continues`, `wsx series show`, `wsx series title`

**Files:**
- Modify: `src/cli/action.rs` (`WorkspaceCreate { …, continues: Option<String> }`; new `SeriesShow { workspace: Option<String>, json: bool }`, `SeriesTitle { title: String, workspace: Option<String> }`)
- Modify: `src/cli/parse/workspace.rs:43-70` (`--continues` like `--prompt`; usage string gains `[--continues <slug>]`)
- Create: `src/cli/parse/series.rs`, `parse_series(it) -> Result<CliAction>`. Bare `series` or `series show` → show, using `parse_show_flags(…, "series")` as `parse_recap` does. `series title <text> [--workspace <repo>/<slug>]`.
- Modify: `src/cli/parse/mod.rs:152` (`"series" => parse_series`)
- Modify: `src/cli/groups.rs` (a `series` group after `recap`, with `[show] [--workspace <repo>/<slug>] [--json]` and `title <text> [--workspace <repo>/<slug>]`)
- Modify: `src/cli/run.rs:502` (create) and add handlers next to `RecapShow` (`run.rs:1239`)
- Create: `src/commands/series.rs`, `render_series(&Series) -> String` plus a `serde::Serialize` view `SeriesView { title: Option<String>, display_title: String, parts: Vec<PartView{part, slug, branch, pr_number, pr_state: Option<&'static str>, archived: bool}> }`
- Test: `src/cli/tests.rs`, `src/commands/series.rs`

**Interfaces:**
- Consumes: `Store::continue_series`, `series_for_workspace`, `set_series_title`, `Series::display_title` (Task 1).

Create flow in `run.rs`:
1. Before calling `workspace::create`, if `continues` is set, resolve the parent with `lookup_workspace(&store, &r, slug)` (as `WorkspacePath` does at `run.rs:675`) and fail with `Error::Usage { msg: "--continues: no workspace <repo>/<slug>" }` if it's missing. The lookup is scoped to `r`, so a parent in another repo can't be named.
2. After create succeeds, call `store.continue_series(parent.id, created.workspace.id)` and print via `say!`:
   - `started == false`: `continues {repo}/{parent} as part {N} of "{display_title}"`
   - `started == true`: `started a series: {parent} is part 1, {new} is part 2 (no title yet; set one with wsx series title)`

`series show` text, when the workspace is in a series:
```text
Focus view declutter
  1  focus-view-declutter  bakedbean/focus-view-declutter  #466 merged  archived
  2  focus-diff-bars       bakedbean/focus-diff-bars       #469 draft
```
Outside a series it prints `not part of a series` and exits 0. `series title ""` → `set_series_title(None)`. A workspace outside a series → `Error::Usage("not part of a series")`.

- [ ] **Step 1: Write failing parse tests** in `src/cli/tests.rs`: `parse(&["workspace","create","r","--continues","a"])` yields `continues == Some("a")`. `--continues` with no value is `Err`. `parse(&["series"])` and `parse(&["series","show","--json"])` → `SeriesShow`. `parse(&["series","title","Focus view"])` → `SeriesTitle { title: "Focus view", workspace: None }`.
- [ ] **Step 2: Write failing tests for `render_series`**: part order, the `archived` mark, a part without a PR showing no PR column text, and the header using `display_title`.
- [ ] **Step 3: Write failing create test** `continues_unknown_or_other_repo_fails_before_worktree`. Use the harness the existing `WorkspaceCreate` run tests use (search `src/cli/tests.rs` / `src/commands` for a `run_action` test with a temp git repo; if none exists, test the resolve helper you extract, `resolve_continues_parent(store, repo, slug) -> Result<Workspace>`, and assert no worktree dir was created).
- [ ] **Step 4: Run, verify fail**: `cargo test --lib cli:: commands::series` → FAIL.
- [ ] **Step 5: Implement** the parse, action, help group, handlers and render described above.
- [ ] **Step 6: Run, verify pass**: `cargo test --lib cli:: commands::` → PASS. Also `cargo run -- help series` shows the group.
- [ ] **Step 7: Commit**

```bash
git add src/cli src/commands
git commit -m "feat(cli): workspace create --continues and wsx series show/title"
```

---

### Task 4: Settle archived parts in the PR poll

**Files:**
- Modify: `src/app/background.rs` (`pr_poll` at ~1153; new `settle_archived_parts`)
- Modify: `src/app/state.rs` (`App` gains `series_settle_last_ms: HashMap<(SeriesId, u32), i64>`, default empty, and `series: Vec<Series>`, default empty, which Task 5 reads)
- Test: `src/app/background.rs` (`pr_commit_tests` module or a sibling `series_settle_tests`)

**Interfaces:**
- Consumes: `Store::unsettled_archived_parts`, `Store::set_part_pr` (Task 2).
- Produces: `async fn settle_archived_parts<F, Fut>(app: &SharedApp, fetch_pr: &F, now_ms: i64)`, with the same `F`/`Fut` bounds as `pr_poll`.

Once per `pr_poll` tick, after the live fetches: take the unsettled list under the lock and keep parts whose `series_settle_last_ms` entry is missing or at least `PR_POLL_INTERVAL_MS` old. Stamp them, drop the lock, and call `fetch_pr(part.repo_path.clone(), part.pr_number.to_string())` for each (`gh pr view <number>` accepts a number where a branch goes), `PR_POLL_CONCURRENCY` at a time. On `Ok(Some(status))`, lock and call `set_part_pr`. On error or `None`, leave the part alone. Skip parts whose `repo_path` doesn't exist. After any write, reload `g.series = g.store.all_series().unwrap_or_default()` under the lock: in-process writes don't bump `data_version`, so `refresh()` won't pick them up (CLI writes do, via `App`'s `data_version` check at `state.rs:225`).

- [ ] **Step 1: Write failing tests** using a fetch closure keyed by the "branch" string, as in the existing poll tests:
  - `archived_open_part_settles_to_merged`: the store has part 1 archived with open #10. The fetch answers `"10"` → merged. After one `settle_archived_parts`, `unsettled_archived_parts()` is empty and the part is `PrMerged`.
  - `settled_parts_are_not_fetched`: a merged archived part, and the fetch closure panics if it's called.
  - `settle_is_throttled`: two calls within `PR_POLL_INTERVAL_MS` → the fetch count is 1.
  - `failed_fetch_leaves_snapshot`: the fetch returns `Err` → the part is still `PrOpen`.
- [ ] **Step 2: Run, verify fail**: `cargo test --lib app::background` → FAIL.
- [ ] **Step 3: Implement** `settle_archived_parts` and call it from `pr_poll`.
- [ ] **Step 4: Run, verify pass**: `cargo test --lib app::` → PASS.
- [ ] **Step 5: Commit**

```bash
git add src/app/background.rs src/app/state.rs
git commit -m "feat(dashboard): settle archived series parts' PR state in the poll"
```

---

### Task 5: Focus view `part` line

**Files:**
- Modify: `src/app/state.rs` (`App.series: Vec<Series>`, loaded in `refresh()` next to `recaps` at ~392: `self.series = self.store.all_series().unwrap_or_default()`)
- Modify: `src/ui/dashboard/tall.rs` (`WorkspaceDetail.series: Option<SeriesSlice>`, `SeriesSlice`, `series_slice()`, `part_line()`, insertion in `row_layout` before the prose loop at ~687)
- Modify: `src/app/render/dashboard.rs:425` (fill `series`)
- Test: `src/ui/dashboard/tall.rs` tests

**Interfaces:**
- Consumes: `Series`, `SeriesPart` (Task 1).
- Produces:
  - `pub struct PriorPart { pub pr: Option<(u32, Option<BranchLifecycle>)>, pub slug: String }`
  - `pub struct SeriesSlice { pub part: u32, pub title: String, pub prior: Vec<PriorPart>, pub newest_part: u32 }` (Default, Clone, Debug)
  - `pub fn series_slice(series: &Series, ws: WorkspaceId, live_pr: impl Fn(WorkspaceId) -> Option<(u32, BranchLifecycle)>) -> Option<SeriesSlice>`: `None` when `ws` isn't in `series`. Prior parts are those with `part <` this one, in ascending order. A live part (`archived_at None`) uses `live_pr`, falling back to the stored snapshot. An archived part uses the snapshot.
  - `fn part_line(s: &SeriesSlice, width: usize, theme: &Theme) -> Vec<Span<'static>>`

`render/dashboard.rs` finds the series containing `ws.id` in `app.series`. Build a `HashMap<WorkspaceId, usize>` once per frame for the focused repo only, and call `series_slice` with `|id| app.pr_number.get(&id).zip(app.pr_lifecycle.get(&id)).map(|(n, l)| (*n, *l))`.

Line layout (one line, never wraps), with `width` = the prose text width (`cols.prose - LABEL_WIDTH`):
- label `part` in the existing `{label:<LABEL_WIDTH$}` dim style, then `"{part} · "` in the default style, then the title in `theme.section_style`, or the bold style the section headers use in `src/ui/dashboard/mod.rs:619`.
- then two spaces and each prior part: `#{n} {glyph}` with `lifecycle_chip(lc).0` in `theme.lifecycle_style(Some(lc))` (dim when `None`), or the slug, dimmed, when it has no PR. Parts are separated by two spaces.
- then, if `newest_part > part`, a dim `  → part {newest_part}`.
- Fitting, in this order: keep `→ part M` whole. Drop prior parts from the oldest, replacing them with a dim `+k ` before the rest, until everything fits or none are left. Then cut the title with `…`.

- [ ] **Step 1: Write failing tests** with `layout(&tall, &AgentGlyphs::default(), 120)` and `text()` as in `initial_prompt_leads_the_detail_lines`:
  - `part_line_leads_the_prose`: a detail with `series` and a recap goal. `texts[1]` contains `part`, `4 · Focus view`, `#466` and `#468`, and `texts[2]` contains `goal`.
  - `part_line_absent_outside_a_series`: no line contains `part `.
  - `part_line_falls_back_to_part_one_slug`: a `series_slice` built from a `Series` with `title: None` and part 1 archived, slug `focus-view-declutter` → the title is that slug.
  - `part_line_shows_newer_part`: part 1 of 3 → ends with `→ part 3` and has no prior PRs.
  - `part_line_fits_one_line_when_crowded`: 20 prior parts, a 200-char title, width 60 → exactly one `part` line. It contains `+`, ends with the newest prior PR, and the title ends with `…`.
  - `series_slice_prefers_live_pr`: a live part with snapshot open #12 and live merged #12 → prior shows `PrMerged`.
- [ ] **Step 2: Run, verify fail**: `cargo test --lib ui::dashboard::tall` → FAIL.
- [ ] **Step 3: Implement** the `App` fields and refresh, `SeriesSlice`/`series_slice`/`part_line`, the insertion in `row_layout`, and the fill in `render/dashboard.rs`. `refresh()` reloads `series` every time it runs (it's cheap, as `recaps` is). Task 4 must add `series: Vec::new()` to `App::new`'s initializer if Task 5 hasn't landed yet. Whichever task adds the field first owns it.
- [ ] **Step 4: Run, verify pass**: `cargo test --lib` → PASS. Then `cargo clippy --all-targets -- -D warnings` → clean.
- [ ] **Step 5: Commit**

```bash
git add src/app src/ui/dashboard/tall.rs
git commit -m "feat(focus): show a workspace's series part and earlier PRs"
```

---

### Task 6: Handoff skill and docs

**Files:**
- Modify: `skills/handoff/SKILL.md` (step 5 command; new step 5b; step 6 WHY hint; the example block gains `--continues tsv-list-output` and a `wsx series title` line)
- Modify: `docs/book/src/dashboard/repo-focus.md` (`part` under **The prose**, and in the example block)
- Modify: `docs/book/src/cli-reference/workspace.md` (`--continues`), and add a `series` section where `recap` is documented (`grep -rln "recap set" docs/book/src/cli-reference`)
- Modify: `skills/wsx/SKILL.md` only if it documents `workspace create` flags (add `--continues`)
- Test: `src/agent/skill.rs` tests if any assert on skill content (`cargo test --lib agent::skill`)

Skill step 5b, the copy the spec fixes in substance: if the create report says `started a series` (or `wsx series show` prints no title), pick a 2–5 word name for the *feature* the chain is building, drawn from this session. Run `wsx series title --workspace <repo>/<current-slug> "<name>"` and tell the user the title you chose so they can rename it.

- [ ] **Step 1: Edit the skill and docs as listed.**
- [ ] **Step 2: Verify**: run `cargo test --lib agent::skill` → PASS. `mdbook build docs/book`, if mdbook is installed → no broken links.
- [ ] **Step 3: Manual smoke test** in a scratch repo with `WSX_BIN=target/debug/wsx`, per the memory note on sandboxes. Check the create report line for `wsx workspace create <repo> --name a2 --continues a1`, the `wsx series show` output, `wsx series title "Demo"`, and that the focus view shows `part 2 · Demo`.
- [ ] **Step 4: Commit**

```bash
git add skills docs
git commit -m "docs: handoff passes --continues and names the series; document series"
```
