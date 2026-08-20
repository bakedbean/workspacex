# Updates Panel Workspace Filter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring the dashboard's `/` substring filter to the agent updates panel (`^x u`), so a user can narrow a long, repo-grouped workspace list by name, repo, or status text.

**Architecture:** `ordered_workspaces_for_panel` already produces the single ordered ID list that both the panel renderer and the panel key handler index into, so the filter applies there and everything downstream (row visibility, repo headers, selection indices) follows for free. Filter state lives in the `Modal::UpdatesPanel` variant alongside `sort`, so it resets on every open. Matching runs against the same status text the row renders, via a function both share.

**Tech Stack:** Rust, ratatui, crossterm, tokio (`#[tokio::test]` for key-handler tests).

## Global Constraints

- Never commit directly to `main`. This work happens on the current branch; a PR opens at the end.
- CI runs `cargo fmt --check`, `cargo clippy`, and `cargo test` as separate gates. Run `cargo fmt` before every commit — clippy passing does not mean fmt is clean.
- Column widths throughout this codebase are measured in `char`s, not display cells (`src/ui/text.rs:1`). Keep that convention.
- The updates panel is capped at 80 columns with a 1-column border on each side, so footer text must fit **78 chars**.
- Filter matching is case-insensitive substring. No fuzzy matching.
- `Some("")` and `None` are distinct filter states: `Some("")` means filter-input mode is active with an empty needle (all rows still visible), `None` means normal key handling. This mirrors `DashboardState::filter`.
- Spec: `docs/superpowers/specs/2026-08-20-updates-panel-filter-design.md`.

---

## File Structure

| File | Responsibility | Change |
| --- | --- | --- |
| `src/ui/modal/updates_panel.rs` | Panel ordering, filtering, row rendering, footer | Modify — add `PanelInputs`, `row_status_text`, `matches_filter`, filter param, footer variants |
| `src/ui/modal/mod.rs` | `Modal` enum, re-exports | Modify — add `filter` field, export `PanelInputs` |
| `src/app/input.rs` | Key handling | Modify — `panel_order`/`reselect` helpers, filter-input intercept, `/` binding |
| `src/app/render.rs` | Draws the panel | Modify — build `PanelInputs`, pass the needle |
| `src/app.rs` | Shared derived caches | Modify — add `awaiting_permission_map()` |
| `src/ui/dashboard/layout.rs` | Dashboard top chrome | Modify — echo the active needle |
| `src/ui/dashboard/mod.rs` | Dashboard render entry | Modify — pass `state.filter` to `top_chrome` |
| `src/ui/text.rs` | Shared text helpers | Modify — add `FILTER_ECHO_MAX` |
| `src/app/input_tests.rs` | Key-handler tests | Modify — new filter tests, update existing literals |

Tests live beside the code in `#[cfg(test)] mod` blocks, per this codebase's convention. `src/ui/modal/updates_panel.rs` already has three: `scroll_offset_tests`, `workspace_row_tests`, `ordering_tests`.

---

### Task 1: Bundle panel inputs and extract the row status text

Pure refactor, no behavior change. It exists because Task 2 needs to add two more parameters to functions already carrying `#[allow(clippy::too_many_arguments)]`, and needs the row's status text from outside `workspace_row`.

**Files:**
- Modify: `src/ui/modal/updates_panel.rs` (add `PanelInputs`, `row_status_text`; rewrite the signatures of `ordered_workspaces_for_panel:104` and `render_updates_panel:183`)
- Modify: `src/ui/modal/mod.rs:24` (re-export `PanelInputs`)
- Modify: `src/app.rs` (add `awaiting_permission_map`, next to `classified_statuses:1187`)
- Modify: `src/app/render.rs:674-707` (build `PanelInputs`)
- Modify: `src/app/input.rs:1614-1636` (add `panel_order` helper, use it)
- Test: `src/ui/modal/updates_panel.rs` (`workspace_row_tests`, `ordering_tests`)

**Interfaces:**
- Produces:
  - `pub struct PanelInputs<'a>` with fields `repos`, `workspaces`, `events`, `activity`, `needs_attention`, `awaiting`, `statuses`, `lifecycles`
  - `impl PanelInputs<'_> { fn status_text(&self, w: &Workspace) -> String }`
  - `fn row_status_text(w: &Workspace, events: Option<&WorkspaceEvents>, activity: Option<ActivityState>, needs_attention: bool, awaiting: Option<&(String, i64)>) -> (String, Option<i64>)`
  - `pub fn ordered_workspaces_for_panel(inputs: &PanelInputs<'_>, sort: UpdatesSort) -> Vec<WorkspaceId>`
  - `pub fn render_updates_panel(f: &mut Frame, area: Rect, inputs: &PanelInputs<'_>, selected: usize, now_ms: i64, sort: UpdatesSort, theme: &Theme)`
  - `pub fn App::awaiting_permission_map(&self) -> HashMap<WorkspaceId, (String, i64)>`
  - `fn panel_order(app: &App, sort: UpdatesSort) -> Vec<WorkspaceId>` (private to `src/app/input.rs`)

- [ ] **Step 1: Write the failing test**

Add to the `workspace_row_tests` module in `src/ui/modal/updates_panel.rs` (after `workspace_row_shows_permission_tool_in_status_text`, around line 619). This test is the guard against the extraction drifting from what the row renders:

```rust
    /// `row_status_text` is the single source for the row's status text —
    /// the renderer and (from Task 2) the filter both read it. If the
    /// extraction ever drifts from what `workspace_row` draws, the filter
    /// would fail to match text the user can plainly see.
    #[test]
    fn row_status_text_matches_what_the_row_renders() {
        let theme = Theme::ansi();
        let mut failed = fixture_workspace("gamma");
        failed.state = WorkspaceState::Failed;
        let awaiting = ("Bash".to_string(), 5_000i64);
        // (workspace, activity, needs_attention, awaiting, expected text)
        let cases: [(&Workspace, Option<ActivityState>, bool, Option<&(String, i64)>, &str); 4] = [
            (
                &fixture_workspace("alpha"),
                Some(ActivityState::Awaiting),
                true,
                Some(&awaiting),
                "awaiting permission: Bash",
            ),
            (
                &fixture_workspace("beta"),
                Some(ActivityState::Stalled),
                true,
                None,
                "stalled",
            ),
            (&failed, None, false, None, "failed"),
            (
                &fixture_workspace("delta"),
                None,
                false,
                None,
                "no session",
            ),
        ];
        for (w, activity, attention, awaiting, expected) in cases {
            let (text, _) = row_status_text(w, None, activity, attention, awaiting);
            assert_eq!(text, expected, "row_status_text for {}", w.name);
            let line = workspace_row(
                w, None, activity, attention, awaiting, false,
                Status::Idle, None, 10_000, 20, 78, &theme,
            );
            assert!(
                line_text(&line).contains(expected),
                "row for {} should render {expected:?}: {}",
                w.name,
                line_text(&line)
            );
        }
    }
```

The `cases` array borrows `fixture_workspace(..)` temporaries; bind them to locals first if the borrow checker objects:

```rust
        let (alpha, beta, delta) = (
            fixture_workspace("alpha"),
            fixture_workspace("beta"),
            fixture_workspace("delta"),
        );
```

and reference `&alpha`, `&beta`, `&delta` in the array.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib row_status_text_matches_what_the_row_renders`
Expected: FAIL to compile — `cannot find function 'row_status_text' in this scope`.

- [ ] **Step 3: Extract `row_status_text`**

In `src/ui/modal/updates_panel.rs`, insert this above `fn workspace_row` (line 331):

```rust
/// The status text a row displays, plus the timestamp its age column is
/// anchored to. Split out of `workspace_row` so the filter can match on
/// exactly the text the row shows — a row must never display text the
/// filter fails to find.
fn row_status_text(
    w: &crate::data::store::Workspace,
    events: Option<&crate::activity::events::WorkspaceEvents>,
    activity: Option<crate::ui::updates_bar::ActivityState>,
    needs_attention: bool,
    awaiting: Option<&(String, i64)>,
) -> (String, Option<i64>) {
    use crate::ui::updates_bar::ActivityState;
    if let Some((tool, ts)) = awaiting {
        return (format!("awaiting permission: {tool}"), Some(*ts));
    }
    if needs_attention {
        let label = match activity {
            Some(ActivityState::AwaitingAnswer) => "question",
            Some(ActivityState::Complete) => "complete",
            Some(ActivityState::Stalled) => "stalled",
            _ => "waiting",
        };
        return (
            label.to_string(),
            events.and_then(|e| e.latest.as_ref().map(|s| s.timestamp_ms)),
        );
    }
    if matches!(
        activity,
        Some(ActivityState::Active) | Some(ActivityState::Idle)
    ) {
        let text = events
            .and_then(|e| e.latest.as_ref().map(|s| s.display.clone()))
            .unwrap_or_else(|| "active".to_string());
        let ts = events.and_then(|e| e.latest.as_ref().map(|s| s.timestamp_ms));
        return (text, ts);
    }
    if w.state == crate::data::store::WorkspaceState::Failed {
        return ("failed".to_string(), None);
    }
    if events.and_then(|e| e.latest.as_ref()).is_some() {
        return ("resumable".to_string(), None);
    }
    ("no session".to_string(), None)
}
```

Then in `workspace_row`, delete the whole `let (status_text, age_anchor_ms) = if let Some((tool, ts)) = awaiting { ... };` block (lines 368-395) and replace it with:

```rust
    let (status_text, age_anchor_ms) =
        row_status_text(w, events, activity, needs_attention, awaiting);
```

Leave everything else in `workspace_row` alone — the `glyph` computation above it and the `let age = age_anchor_ms.map(...)` line below it are unchanged. Keep the `let failed = ...` binding: the glyph and `status_fg` still use it.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --lib row_status_text_matches_what_the_row_renders`
Expected: PASS.

- [ ] **Step 5: Add `PanelInputs`**

In `src/ui/modal/updates_panel.rs`, add near the top (after the `const` block, before `name_col_width`):

```rust
/// The borrowed caches the panel reads. Bundled so the renderer and the key
/// handler hand identical inputs to `ordered_workspaces_for_panel` without
/// two long, drift-prone argument lists. Mirrors `DashboardInputs` on the
/// dashboard side.
pub struct PanelInputs<'a> {
    pub repos: &'a [crate::data::store::Repo],
    pub workspaces: &'a [(RepoId, crate::data::store::Workspace)],
    pub events:
        &'a HashMap<crate::data::store::WorkspaceId, crate::activity::events::WorkspaceEvents>,
    pub activity:
        &'a HashMap<crate::data::store::WorkspaceId, crate::ui::updates_bar::ActivityState>,
    pub needs_attention: &'a HashSet<crate::data::store::WorkspaceId>,
    pub awaiting: &'a HashMap<crate::data::store::WorkspaceId, (String, i64)>,
    pub statuses: &'a HashMap<crate::data::store::WorkspaceId, Status>,
    pub lifecycles: &'a HashMap<crate::data::store::WorkspaceId, BranchLifecycle>,
}

impl PanelInputs<'_> {
    /// The status text a row would display for `w`. Used by the renderer's
    /// caller-side logic and, from Task 2, by the filter.
    fn status_text(&self, w: &crate::data::store::Workspace) -> String {
        row_status_text(
            w,
            self.events.get(&w.id),
            self.activity.get(&w.id).copied(),
            self.needs_attention.contains(&w.id),
            self.awaiting.get(&w.id),
        )
        .0
    }
}
```

`status_text` is unused until Task 2 — add `#[allow(dead_code)]` on it for this commit and remove that attribute in Task 2, or simply fold the `impl` block into Task 2. Prefer folding it into Task 2 to avoid a dead-code allow that outlives its reason; the struct itself is used immediately.

- [ ] **Step 6: Convert the two panel functions to `PanelInputs`**

Replace the signature of `ordered_workspaces_for_panel` (line 104) and drop its `#[allow(clippy::too_many_arguments)]`:

```rust
pub fn ordered_workspaces_for_panel(
    inputs: &PanelInputs<'_>,
    sort: UpdatesSort,
) -> Vec<crate::data::store::WorkspaceId> {
    let mut out = Vec::new();
    for repo in inputs.repos {
        let mut ws_for_repo: Vec<&crate::data::store::Workspace> = inputs
            .workspaces
            .iter()
            .filter(|(rid, _)| *rid == repo.id)
            .map(|(_, w)| w)
            .collect();
        ws_for_repo.sort_by_key(|w| {
            let default_key = sort_key(w, inputs.events, inputs.activity, inputs.needs_attention);
            let mode_rank = match sort {
                UpdatesSort::Default => (0, std::cmp::Reverse(0)),
                UpdatesSort::Status => status_rank(w, inputs.statuses),
                UpdatesSort::PrStatus => (
                    lifecycle_rank(inputs.lifecycles.get(&w.id).copied()),
                    std::cmp::Reverse(0),
                ),
            };
            (mode_rank, default_key)
        });
        out.extend(ws_for_repo.into_iter().map(|w| w.id));
    }
    out
}
```

Replace the signature of `render_updates_panel` (line 183) and drop its `#[allow(clippy::too_many_arguments)]`:

```rust
pub fn render_updates_panel(
    f: &mut Frame,
    area: Rect,
    inputs: &PanelInputs<'_>,
    selected: usize,
    now_ms: i64,
    sort: UpdatesSort,
    theme: &Theme,
) {
```

Inside its body, rewrite every reference to the old parameters as `inputs.<field>`:
- `ordered_workspaces_for_panel(repos, workspaces, ...)` → `ordered_workspaces_for_panel(inputs, sort)`
- `workspaces.iter()` (the `name_col` computation) → `inputs.workspaces.iter()`
- `for repo in repos` → `for repo in inputs.repos`
- `workspaces.iter().filter(...)` (the per-repo collect) → `inputs.workspaces.iter().filter(...)`
- `statuses.get(&w.id)` → `inputs.statuses.get(&w.id)`
- `lifecycles.get(&w.id)` → `inputs.lifecycles.get(&w.id)`
- `events.get(&w.id)`, `activity.get(&w.id)`, `needs_attention.contains(&w.id)`, `awaiting.get(&w.id)` → the `inputs.` equivalents

`workspace_row` keeps its own parameter list unchanged — it is per-row, not per-panel, and takes computed values (`is_selected`, `name_col`, `row_width`).

- [ ] **Step 7: Export `PanelInputs`**

In `src/ui/modal/mod.rs:24`, extend the re-export:

```rust
pub use updates_panel::{
    PanelInputs, UpdatesSort, ordered_workspaces_for_panel, render_updates_panel,
};
```

- [ ] **Step 8: Add `App::awaiting_permission_map`**

In `src/app.rs`, immediately after `classified_statuses` (which ends around line 1198):

```rust
    /// Permission prompts still awaiting an answer, keyed by workspace id.
    /// Shared by the updates-panel renderer and key handler so both derive
    /// row text — and the filter's match target — from identical inputs.
    pub fn awaiting_permission_map(
        &self,
    ) -> std::collections::HashMap<crate::data::store::WorkspaceId, (String, i64)> {
        self.workspaces
            .iter()
            .filter_map(|(_, w)| self.awaiting_permission(w.id).map(|a| (w.id, a)))
            .collect()
    }
```

- [ ] **Step 9: Update the renderer call site**

In `src/app/render.rs`, replace the `UpdatesPanel` arm's body (lines 674-707) with:

```rust
            crate::ui::modal::Modal::UpdatesPanel { selected, sort } => {
                let now_ms = crate::time::now_ms();
                let awaiting = app.awaiting_permission_map();
                let activity_translated: std::collections::HashMap<
                    crate::data::store::WorkspaceId,
                    crate::ui::updates_bar::ActivityState,
                > = app
                    .workspace_activity
                    .iter()
                    .map(|(k, v)| (*k, translate_activity(*v)))
                    .collect();
                let statuses = app.classified_statuses();
                let inputs = crate::ui::modal::PanelInputs {
                    repos: &app.repos,
                    workspaces: &app.workspaces,
                    events: &app.workspace_events,
                    activity: &activity_translated,
                    needs_attention: &app.workspace_needs_attention,
                    awaiting: &awaiting,
                    statuses: &statuses,
                    lifecycles: &app.pr_lifecycle,
                };
                crate::ui::modal::render_updates_panel(
                    f, area, &inputs, *selected, now_ms, *sort, &app.theme,
                );
            }
```

This deletes the inline `for (_rid, w) in &app.workspaces { ... }` loop that built `awaiting`.

- [ ] **Step 10: Add `panel_order` and use it in the key handler**

The `PanelInputs` struct borrows `app` immutably, which collides with the `app.modal = Some(...)` assignments inside the same match arm. Build it inside a helper that returns an owned `Vec`, so the borrow ends at the call. Add near the other free functions in `src/app/input.rs` (above `handle_key_modal:1291`):

```rust
/// The updates panel's ordered workspace list. Returns an owned Vec so the
/// borrow of `app` ends at the call — the caller mutates `app.modal` right
/// after. Both the key handler and `app::render` must derive row order from
/// identical inputs or the selection indices drift from the drawn rows.
fn panel_order(
    app: &App,
    sort: crate::ui::modal::UpdatesSort,
) -> Vec<crate::data::store::WorkspaceId> {
    let activity_translated: std::collections::HashMap<
        crate::data::store::WorkspaceId,
        crate::ui::updates_bar::ActivityState,
    > = app
        .workspace_activity
        .iter()
        .map(|(k, v)| (*k, crate::app::render::translate_activity(*v)))
        .collect();
    let statuses = app.classified_statuses();
    let awaiting = app.awaiting_permission_map();
    let inputs = crate::ui::modal::PanelInputs {
        repos: &app.repos,
        workspaces: &app.workspaces,
        events: &app.workspace_events,
        activity: &activity_translated,
        needs_attention: &app.workspace_needs_attention,
        awaiting: &awaiting,
        statuses: &statuses,
        lifecycles: &app.pr_lifecycle,
    };
    crate::ui::modal::ordered_workspaces_for_panel(&inputs, sort)
}
```

In the `Modal::UpdatesPanel` arm (line 1614), delete the `activity_translated` / `statuses` / `order` block (lines 1616-1636) and replace with:

```rust
        Modal::UpdatesPanel { selected, sort } => {
            let selected_now = selected;
            // Build the same ordered workspace list the renderer uses, so
            // arrow keys and Enter operate on the same indices.
            let order = panel_order(app, sort);
```

In the `'o'` handler (line 1659), replace the second `ordered_workspaces_for_panel(...)` call with `let new_order = panel_order(app, new_sort);` and delete the stale comment about reusing the pre-cycle maps.

- [ ] **Step 11: Update the panel's own tests to the new signature**

In `ordering_tests` (line 997), add `awaiting` to the `Maps` helper and rewrite `order`:

```rust
    #[derive(Default)]
    struct Maps {
        events: HashMap<WorkspaceId, crate::activity::events::WorkspaceEvents>,
        activity: HashMap<WorkspaceId, crate::ui::updates_bar::ActivityState>,
        attention: HashSet<WorkspaceId>,
        awaiting: HashMap<WorkspaceId, (String, i64)>,
        statuses: HashMap<WorkspaceId, Status>,
        lifecycles: HashMap<WorkspaceId, BranchLifecycle>,
    }

    fn order(
        repos: &[Repo],
        ws: &[(RepoId, Workspace)],
        maps: &Maps,
        sort: UpdatesSort,
    ) -> Vec<WorkspaceId> {
        ordered_workspaces_for_panel(
            &PanelInputs {
                repos,
                workspaces: ws,
                events: &maps.events,
                activity: &maps.activity,
                needs_attention: &maps.attention,
                awaiting: &maps.awaiting,
                statuses: &maps.statuses,
                lifecycles: &maps.lifecycles,
            },
            sort,
        )
    }
```

Every existing test in that module calls the `order` wrapper, so no other test bodies change.

- [ ] **Step 12: Verify the whole suite, fmt, and clippy**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: PASS, with no `too_many_arguments` allows remaining on either panel function.

Note: `click_chip_auto_spawns_session_when_missing` is a known flaky PTY-timing test. If it is the only failure, re-run it alone to confirm.

- [ ] **Step 13: Commit**

```bash
git add src/ui/modal/updates_panel.rs src/ui/modal/mod.rs src/app.rs src/app/render.rs src/app/input.rs
git commit -m "refactor: bundle updates-panel inputs and extract row status text

PanelInputs collapses two argument lists that had both grown past
clippy's threshold, and row_status_text gives the (upcoming) filter a
way to match the exact text a row renders. No behavior change."
```

---

### Task 2: Filter the ordered workspace list

**Files:**
- Modify: `src/ui/modal/updates_panel.rs` (add `matches_filter`, `PanelInputs::status_text`; add the `filter` parameter to `ordered_workspaces_for_panel` and `render_updates_panel`; new empty state)
- Modify: `src/app/input.rs` (`panel_order` gains a `filter` parameter, passes `None` for now)
- Modify: `src/app/render.rs` (passes `None` for now)
- Test: `src/ui/modal/updates_panel.rs` (`ordering_tests`, plus a new render test)

**Interfaces:**
- Consumes: `PanelInputs`, `row_status_text`, `panel_order` from Task 1.
- Produces:
  - `fn matches_filter(w: &Workspace, repo_name: &str, status_text: &str, needle: &str) -> bool`
  - `pub fn ordered_workspaces_for_panel(inputs: &PanelInputs<'_>, sort: UpdatesSort, filter: Option<&str>) -> Vec<WorkspaceId>`
  - `pub fn render_updates_panel(f, area, inputs: &PanelInputs<'_>, selected: usize, now_ms: i64, sort: UpdatesSort, filter: Option<&str>, theme: &Theme)`
  - `fn panel_order(app: &App, sort: UpdatesSort, filter: Option<&str>) -> Vec<WorkspaceId>`

- [ ] **Step 1: Write the failing tests**

Add to `ordering_tests` in `src/ui/modal/updates_panel.rs`. First extend the `order` wrapper with a filter-aware sibling (keep `order` delegating so existing tests are untouched):

```rust
    fn order_filtered(
        repos: &[Repo],
        ws: &[(RepoId, Workspace)],
        maps: &Maps,
        sort: UpdatesSort,
        filter: Option<&str>,
    ) -> Vec<WorkspaceId> {
        ordered_workspaces_for_panel(
            &PanelInputs {
                repos,
                workspaces: ws,
                events: &maps.events,
                activity: &maps.activity,
                needs_attention: &maps.attention,
                awaiting: &maps.awaiting,
                statuses: &maps.statuses,
                lifecycles: &maps.lifecycles,
            },
            sort,
            filter,
        )
    }
```

and make `order` call it with `None`:

```rust
    fn order(
        repos: &[Repo],
        ws: &[(RepoId, Workspace)],
        maps: &Maps,
        sort: UpdatesSort,
    ) -> Vec<WorkspaceId> {
        order_filtered(repos, ws, maps, sort, None)
    }
```

Then the tests:

```rust
    /// The needle matches the workspace name, case-insensitively.
    #[test]
    fn filter_matches_workspace_name() {
        let repos = vec![fixture_repo(1)];
        let ws = vec![
            fixture_ws(1, 1, "auth-refactor"),
            fixture_ws(2, 1, "billing-fix"),
        ];
        let maps = Maps::default();
        assert_eq!(
            order_filtered(&repos, &ws, &maps, UpdatesSort::Default, Some("AUTH")),
            vec![WorkspaceId(1)]
        );
    }

    /// A repo-name needle keeps every workspace in that repo — the same
    /// affordance the dashboard gives for "show me just this repo".
    #[test]
    fn filter_matches_repo_name_and_keeps_its_workspaces() {
        let repos = vec![fixture_repo(1), fixture_repo(2)];
        let ws = vec![
            fixture_ws(1, 1, "alpha"),
            fixture_ws(2, 1, "beta"),
            fixture_ws(3, 2, "gamma"),
        ];
        let maps = Maps::default();
        // fixture_repo(1) is named "repo1", fixture_repo(2) is "repo2".
        assert_eq!(
            order_filtered(&repos, &ws, &maps, UpdatesSort::Default, Some("repo1")),
            vec![WorkspaceId(1), WorkspaceId(2)]
        );
    }

    /// The needle also matches the live status text, so "permission" or
    /// "stalled" narrows to the rows that actually say that.
    #[test]
    fn filter_matches_status_text() {
        let repos = vec![fixture_repo(1)];
        let ws = vec![fixture_ws(1, 1, "alpha"), fixture_ws(2, 1, "beta")];
        let mut maps = Maps::default();
        maps.awaiting
            .insert(WorkspaceId(1), ("Bash".to_string(), 1_000));
        assert_eq!(
            order_filtered(&repos, &ws, &maps, UpdatesSort::Default, Some("permission")),
            vec![WorkspaceId(1)]
        );
        // beta has no session at all, so its status text is "no session".
        assert_eq!(
            order_filtered(&repos, &ws, &maps, UpdatesSort::Default, Some("no session")),
            vec![WorkspaceId(2)]
        );
    }

    /// `Some("")` is the "user pressed / but hasn't typed" state: every row
    /// stays visible. Only a non-empty needle narrows anything.
    #[test]
    fn empty_needle_matches_everything() {
        let repos = vec![fixture_repo(1)];
        let ws = vec![fixture_ws(1, 1, "alpha"), fixture_ws(2, 1, "beta")];
        let maps = Maps::default();
        let all = vec![WorkspaceId(1), WorkspaceId(2)];
        assert_eq!(
            order_filtered(&repos, &ws, &maps, UpdatesSort::Default, Some("")),
            all
        );
        assert_eq!(
            order_filtered(&repos, &ws, &maps, UpdatesSort::Default, None),
            all
        );
    }

    /// A needle matching nothing yields an empty order — the renderer turns
    /// that into "(no matching workspaces)".
    #[test]
    fn filter_matching_nothing_yields_empty_order() {
        let repos = vec![fixture_repo(1)];
        let ws = vec![fixture_ws(1, 1, "alpha")];
        let maps = Maps::default();
        assert!(
            order_filtered(&repos, &ws, &maps, UpdatesSort::Default, Some("zzz")).is_empty()
        );
    }

    /// Filtering narrows the list without reshuffling it: survivors keep
    /// their unfiltered relative order under every sort mode.
    #[test]
    fn filter_preserves_relative_order() {
        let repos = vec![fixture_repo(1)];
        let ws = vec![
            fixture_ws(1, 1, "keep-one"),
            fixture_ws(2, 1, "drop-me"),
            fixture_ws(3, 1, "keep-two"),
        ];
        let mut maps = Maps::default();
        maps.attention.insert(WorkspaceId(3));
        // Unfiltered, attention pulls keep-two to the front.
        let unfiltered = order(&repos, &ws, &maps, UpdatesSort::Default);
        let expected: Vec<WorkspaceId> = unfiltered
            .into_iter()
            .filter(|id| *id != WorkspaceId(2))
            .collect();
        assert_eq!(
            order_filtered(&repos, &ws, &maps, UpdatesSort::Default, Some("keep")),
            expected
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib ordering_tests`
Expected: FAIL to compile — `ordered_workspaces_for_panel` takes 2 arguments but 3 were supplied.

- [ ] **Step 3: Implement matching and the filter parameter**

Add `matches_filter` to `src/ui/modal/updates_panel.rs`, above `ordered_workspaces_for_panel`:

```rust
/// Case-insensitive substring match against the workspace name, the owning
/// repo's name, and the row's live status text. Mirrors the dashboard's
/// `matches_filter`, whose three fields are the same idea: what the row is
/// called, where it lives, and what it currently says.
fn matches_filter(
    w: &crate::data::store::Workspace,
    repo_name: &str,
    status_text: &str,
    needle: &str,
) -> bool {
    let needle = needle.to_lowercase();
    w.name.to_lowercase().contains(&needle)
        || repo_name.to_lowercase().contains(&needle)
        || status_text.to_lowercase().contains(&needle)
}
```

Add the `PanelInputs::status_text` impl block from Task 1 Step 5 now (if it was deferred).

Change `ordered_workspaces_for_panel` to take and apply the needle:

```rust
pub fn ordered_workspaces_for_panel(
    inputs: &PanelInputs<'_>,
    sort: UpdatesSort,
    filter: Option<&str>,
) -> Vec<crate::data::store::WorkspaceId> {
    // An empty buffer means "filter mode is on but nothing typed yet" —
    // every row still shows. Only a non-empty needle narrows the list.
    let needle = filter.filter(|f| !f.is_empty());
    let mut out = Vec::new();
    for repo in inputs.repos {
        let mut ws_for_repo: Vec<&crate::data::store::Workspace> = inputs
            .workspaces
            .iter()
            .filter(|(rid, _)| *rid == repo.id)
            .map(|(_, w)| w)
            .filter(|w| {
                needle
                    .map(|n| matches_filter(w, &repo.name, &inputs.status_text(w), n))
                    .unwrap_or(true)
            })
            .collect();
        // ...unchanged sort_by_key and extend...
    }
    out
}
```

- [ ] **Step 4: Thread the parameter through the renderer and callers**

`render_updates_panel` gains `filter: Option<&str>` between `sort` and `theme`, and forwards it:

```rust
    let order = ordered_workspaces_for_panel(inputs, sort, filter);
```

Its empty state distinguishes the two causes:

```rust
    // Nothing to show. Separate the two causes: an empty panel and a panel
    // whose rows the needle hid are very different situations for the user.
    if lines.is_empty() {
        let msg = if filter.map(|f| !f.is_empty()).unwrap_or(false) {
            "(no matching workspaces)"
        } else {
            "(no workspaces)"
        };
        lines.push(Line::from(Span::styled(
            msg.to_string(),
            theme.dim_style(),
        )));
    }
```

`panel_order` in `src/app/input.rs` gains the same parameter:

```rust
fn panel_order(
    app: &App,
    sort: crate::ui::modal::UpdatesSort,
    filter: Option<&str>,
) -> Vec<crate::data::store::WorkspaceId> {
```

and forwards it: `crate::ui::modal::ordered_workspaces_for_panel(&inputs, sort, filter)`.

Both key-handler call sites pass `None` for now (Task 4 wires the real value): `panel_order(app, sort, None)` and `panel_order(app, new_sort, None)`.

`src/app/render.rs` passes `None` for now:

```rust
                crate::ui::modal::render_updates_panel(
                    f, area, &inputs, *selected, now_ms, *sort, None, &app.theme,
                );
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib ordering_tests`
Expected: PASS — all six new tests plus the pre-existing ordering tests.

- [ ] **Step 6: Write the render-level test**

Add a new test module at the end of `src/ui/modal/updates_panel.rs`. It renders the panel to a `TestBackend` and asserts on the text, which is how `src/app/input_tests.rs:29` already tests rendering:

```rust
#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::data::store::{Repo, RepoId, Workspace, WorkspaceId, WorkspaceState};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    fn fixture_repo(id: i64, name: &str) -> Repo {
        Repo {
            id: RepoId(id),
            name: name.to_string(),
            path: PathBuf::from("/tmp/r"),
            branch_prefix: String::new(),
            custom_instructions: None,
            setup_script: None,
            archive_script: None,
            pinned_commands: None,
            related_repos: None,
            base_branch: None,
            detail_bar_config: None,
            created_at: 0,
            sort_order: 0,
        }
    }

    fn fixture_ws(id: i64, repo: i64, name: &str) -> (RepoId, Workspace) {
        (
            RepoId(repo),
            Workspace {
                id: WorkspaceId(id),
                repo_id: RepoId(repo),
                name: name.to_string(),
                branch: "main".to_string(),
                worktree_path: PathBuf::from("/tmp/ws"),
                state: WorkspaceState::Ready,
                setup_status: crate::data::store::SetupStatus::Ok,
                created_at: 0,
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            },
        )
    }

    /// Draw the panel and flatten the buffer to one string per row.
    fn draw(repos: &[Repo], ws: &[(RepoId, Workspace)], filter: Option<&str>) -> String {
        let theme = Theme::ansi();
        let (events, activity, attention, awaiting, statuses, lifecycles) = (
            HashMap::new(),
            HashMap::new(),
            HashSet::new(),
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
        );
        let inputs = PanelInputs {
            repos,
            workspaces: ws,
            events: &events,
            activity: &activity,
            needs_attention: &attention,
            awaiting: &awaiting,
            statuses: &statuses,
            lifecycles: &lifecycles,
        };
        let mut term = Terminal::new(TestBackend::new(80, 25)).unwrap();
        term.draw(|f| {
            render_updates_panel(
                f,
                f.area(),
                &inputs,
                0,
                10_000,
                UpdatesSort::Default,
                filter,
                &theme,
            )
        })
        .unwrap();
        let buf = term.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// A repo whose workspaces all filter out loses its header too — an
    /// empty section header is pure noise in a panel meant to be scanned.
    #[test]
    fn filtered_out_repo_draws_no_header() {
        let repos = vec![fixture_repo(1, "alpha-repo"), fixture_repo(2, "beta-repo")];
        let ws = vec![fixture_ws(1, 1, "one"), fixture_ws(2, 2, "two")];
        let rendered = draw(&repos, &ws, Some("one"));
        assert!(rendered.contains("alpha-repo"), "{rendered}");
        assert!(!rendered.contains("beta-repo"), "{rendered}");
    }

    /// The two empty states are distinguishable: a filter that hit nothing
    /// must not read as "you have no workspaces".
    #[test]
    fn empty_states_distinguish_filter_from_no_workspaces() {
        let repos = vec![fixture_repo(1, "alpha-repo")];
        let ws = vec![fixture_ws(1, 1, "one")];
        assert!(draw(&repos, &ws, Some("zzz")).contains("(no matching workspaces)"));
        assert!(draw(&repos, &[], None).contains("(no workspaces)"));
    }
}
```

- [ ] **Step 7: Run the render tests**

Run: `cargo test --lib render_tests`
Expected: PASS.

- [ ] **Step 8: Verify, fmt, clippy**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src/ui/modal/updates_panel.rs src/app/input.rs src/app/render.rs
git commit -m "feat: filter the updates panel's workspace list

Matches name, repo name, and the row's live status text. Applied in
ordered_workspaces_for_panel, so hidden rows, hidden repo headers, and
the key handler's indices all stay consistent by construction. No key
binding yet — every caller passes None."
```

---

### Task 3: Footer variants

**Files:**
- Modify: `src/ui/modal/updates_panel.rs` (`footer_text:172`, its call site)
- Modify: `src/ui/text.rs` (add `FILTER_ECHO_MAX`)
- Test: `src/ui/modal/updates_panel.rs` (`ordering_tests`, which already holds the `footer_labels_match_modes` test)

**Interfaces:**
- Produces:
  - `pub(crate) const FILTER_ECHO_MAX: usize = 24;` in `crate::ui::text`
  - `fn footer_text(sort: UpdatesSort, filter: Option<&str>) -> String`

- [ ] **Step 1: Write the failing tests**

Add to `ordering_tests` in `src/ui/modal/updates_panel.rs`, next to `footer_labels_match_modes`:

```rust
    /// The panel is capped at 80 columns with a 1-col border each side, so
    /// the footer has 78 chars to work with. Check every sort mode — the
    /// mode name is inlined, and `default` is the longest.
    #[test]
    fn footer_fits_the_panel_in_every_mode() {
        for sort in [UpdatesSort::Default, UpdatesSort::Status, UpdatesSort::PrStatus] {
            let idle = footer_text(sort, None);
            assert!(
                idle.chars().count() <= 78,
                "idle footer for {sort:?} is {} chars: {idle}",
                idle.chars().count()
            );
            let filtering = footer_text(sort, Some(&"x".repeat(60)));
            assert!(
                filtering.chars().count() <= 78,
                "filtering footer for {sort:?} is {} chars: {filtering}",
                filtering.chars().count()
            );
        }
    }

    /// Idle footer advertises the filter key; filtering footer echoes the
    /// needle and swaps `esc close` for `esc clear`, because that is what
    /// Esc does while a filter is up.
    #[test]
    fn footer_swaps_hints_when_filtering() {
        let idle = footer_text(UpdatesSort::Default, None);
        assert!(idle.contains("[/] filter"), "{idle}");
        assert!(idle.contains("[esc] close"), "{idle}");

        let filtering = footer_text(UpdatesSort::Default, Some("auth"));
        assert!(filtering.starts_with("/auth"), "{filtering}");
        assert!(filtering.contains("[esc] clear"), "{filtering}");
        assert!(!filtering.contains("[esc] close"), "{filtering}");
    }

    /// `/` with nothing typed still echoes, so the keypress has visible
    /// feedback before the first character.
    #[test]
    fn footer_echoes_empty_needle() {
        let filtering = footer_text(UpdatesSort::Default, Some(""));
        assert!(filtering.starts_with('/'), "{filtering}");
        assert!(filtering.contains("[esc] clear"), "{filtering}");
    }

    /// A long needle is truncated rather than pushing the key hints off
    /// the line.
    #[test]
    fn footer_truncates_a_long_needle() {
        let filtering = footer_text(UpdatesSort::Default, Some(&"x".repeat(60)));
        assert!(filtering.contains('…'), "{filtering}");
        assert!(filtering.contains("[↑↓] move"), "{filtering}");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib footer`
Expected: FAIL to compile — `footer_text` takes 1 argument but 2 were supplied.

- [ ] **Step 3: Implement**

In `src/ui/text.rs`, add below `truncate`:

```rust
/// Cap on an echoed filter needle in chrome (the updates-panel footer, the
/// dashboard top bar). Long enough to recognize what you typed, short
/// enough that it can't push the surrounding hints off the line.
pub(crate) const FILTER_ECHO_MAX: usize = 24;
```

In `src/ui/modal/updates_panel.rs`, replace `footer_text` (line 172):

```rust
/// Footer hint line, sized to fit the widest panel (80 cols − 2 border = 78).
/// The `↑↓` / `↵` glyphs match the dashboard footer's and buy the room the
/// `[/] filter` chip needs. While filtering, printable keys are filter text
/// rather than shortcuts, so only the hints that still work are listed.
fn footer_text(sort: UpdatesSort, filter: Option<&str>) -> String {
    match filter {
        Some(needle) => format!(
            "/{}    [esc] clear  [\u{2191}\u{2193}] move  [\u{21b5}] switch",
            crate::ui::text::truncate(needle, crate::ui::text::FILTER_ECHO_MAX)
        ),
        None => format!(
            "[\u{2191}\u{2193}] move  [\u{21b5}] switch  [v/s] split  [o] sort:{}  [/] filter  [esc] close",
            sort.footer_label()
        ),
    }
}
```

Update its call site inside `render_updates_panel`:

```rust
    f.render_widget(
        Paragraph::new(footer_text(sort, filter)).style(theme.dim_style()),
        footer_area,
    );
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib footer`
Expected: PASS. The idle footer is 77 chars in `default` mode, 76 in `status`, 72 in `pr`; the filtering footer maxes at 63.

- [ ] **Step 5: Verify, fmt, clippy**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/ui/modal/updates_panel.rs src/ui/text.rs
git commit -m "feat: advertise and echo the filter in the updates-panel footer

Switches to the dashboard footer's arrow glyphs to make room for the
new chip; the filtering variant echoes the needle and lists only the
hints that still fire while printable keys are filter text."
```

---

### Task 4: Filter state and key handling

**Files:**
- Modify: `src/ui/modal/mod.rs:72-80` (add the `filter` field)
- Modify: `src/app/input.rs:995-1000` (open with `filter: None`), `:1614+` (the `UpdatesPanel` arm)
- Modify: `src/app/render.rs` (pass the real needle)
- Test: `src/app/input_tests.rs`

**Interfaces:**
- Consumes: `panel_order(app, sort, filter)` from Task 2.
- Produces:
  - `Modal::UpdatesPanel { selected: usize, sort: UpdatesSort, filter: Option<String> }`
  - `fn reselect(selected_id: Option<WorkspaceId>, new_order: &[WorkspaceId], old_index: usize) -> usize` (private to `src/app/input.rs`)

- [ ] **Step 1: Write the failing tests**

Add to the same module in `src/app/input_tests.rs` that holds `updates_panel_o_cycles_sort_and_follows_selection` (around line 1431). The workspace fixture setup is copied rather than shared, matching the existing tests in that file:

```rust
    /// Two workspaces in one repo, both Ready. Returns their ids in
    /// insertion order (alpha, beta).
    fn seed_two_workspaces(store: &Store) -> Vec<crate::data::store::WorkspaceId> {
        use crate::data::store::{NewWorkspace, WorkspaceState};
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let mut ids = Vec::new();
        for (name, branch, path) in [
            ("alpha", "repo/alpha", "/tmp/wsx-test/alpha"),
            ("beta", "repo/beta", "/tmp/wsx-test/beta"),
        ] {
            let id = store
                .insert_workspace(&NewWorkspace {
                    repo_id,
                    name,
                    branch,
                    worktree_path: std::path::Path::new(path),
                    yolo: false,
                    agent: crate::pty::session::AgentKind::Claude,
                    shared: false,
                })
                .unwrap();
            store.set_workspace_state(id, WorkspaceState::Ready).unwrap();
            ids.push(id);
        }
        ids
    }

    fn shared_app() -> SharedApp {
        Arc::new(Mutex::new(
            App::new(
                Store::open_in_memory().unwrap(),
                PathBuf::from("/tmp/wsx-test"),
            )
            .unwrap(),
        ))
    }

    fn press(code: crossterm::event::KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// `/` arms filter mode with an empty buffer — distinct from `None`, so
    /// the footer can echo the bare `/` before any typing. Subsequent
    /// printable keys are filter text, not the j/k/o/l/v/s shortcuts.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn updates_panel_slash_arms_filter_and_captures_typing() {
        use crate::ui::modal::{Modal, UpdatesSort};
        use crossterm::event::KeyCode;
        let store = Store::open_in_memory().unwrap();
        seed_two_workspaces(&store);
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(Modal::UpdatesPanel {
            selected: 0,
            sort: UpdatesSort::Default,
            filter: None,
        });
        let shared = shared_app();

        handle_key_modal(&mut app, &shared, press(KeyCode::Char('/')))
            .await
            .unwrap();
        match app.modal {
            Some(Modal::UpdatesPanel { ref filter, .. }) => {
                assert_eq!(filter.as_deref(), Some(""), "/ arms an empty buffer");
            }
            ref other => panic!("expected UpdatesPanel; got {other:?}"),
        }

        // 'j' would move the selection outside filter mode; here it types.
        for c in ['j', 'b'] {
            handle_key_modal(&mut app, &shared, press(KeyCode::Char(c)))
                .await
                .unwrap();
        }
        match app.modal {
            Some(Modal::UpdatesPanel { ref filter, .. }) => {
                assert_eq!(filter.as_deref(), Some("jb"));
            }
            ref other => panic!("expected UpdatesPanel; got {other:?}"),
        }

        handle_key_modal(&mut app, &shared, press(KeyCode::Backspace))
            .await
            .unwrap();
        match app.modal {
            Some(Modal::UpdatesPanel { ref filter, .. }) => {
                assert_eq!(filter.as_deref(), Some("j"));
            }
            ref other => panic!("expected UpdatesPanel; got {other:?}"),
        }

        // Backspace past the start is inert, not a panel close.
        for _ in 0..3 {
            handle_key_modal(&mut app, &shared, press(KeyCode::Backspace))
                .await
                .unwrap();
        }
        match app.modal {
            Some(Modal::UpdatesPanel { ref filter, .. }) => {
                assert_eq!(filter.as_deref(), Some(""));
            }
            ref other => panic!("expected UpdatesPanel; got {other:?}"),
        }
    }

    /// Esc is two-stage: it clears an active filter first and only closes
    /// the panel once there is no filter to clear.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn updates_panel_esc_clears_filter_before_closing() {
        use crate::ui::modal::{Modal, UpdatesSort};
        use crossterm::event::KeyCode;
        let store = Store::open_in_memory().unwrap();
        seed_two_workspaces(&store);
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(Modal::UpdatesPanel {
            selected: 0,
            sort: UpdatesSort::Default,
            filter: Some("alp".to_string()),
        });
        let shared = shared_app();

        handle_key_modal(&mut app, &shared, press(KeyCode::Esc))
            .await
            .unwrap();
        match app.modal {
            Some(Modal::UpdatesPanel { ref filter, .. }) => {
                assert_eq!(filter.as_deref(), None, "first Esc clears the filter");
            }
            ref other => panic!("panel should stay open; got {other:?}"),
        }

        handle_key_modal(&mut app, &shared, press(KeyCode::Esc))
            .await
            .unwrap();
        assert!(app.modal.is_none(), "second Esc closes the panel");
    }

    /// Arrows keep navigating while filter mode is on — they are the escape
    /// hatch for j/k being filter text.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn updates_panel_arrows_navigate_while_filtering() {
        use crate::ui::modal::{Modal, UpdatesSort};
        use crossterm::event::KeyCode;
        let store = Store::open_in_memory().unwrap();
        seed_two_workspaces(&store);
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(Modal::UpdatesPanel {
            selected: 0,
            sort: UpdatesSort::Default,
            filter: Some(String::new()),
        });
        let shared = shared_app();

        handle_key_modal(&mut app, &shared, press(KeyCode::Down))
            .await
            .unwrap();
        match app.modal {
            Some(Modal::UpdatesPanel {
                selected,
                ref filter,
                ..
            }) => {
                assert_eq!(selected, 1, "Down still moves while filtering");
                assert_eq!(filter.as_deref(), Some(""), "and does not edit the buffer");
            }
            ref other => panic!("expected UpdatesPanel; got {other:?}"),
        }
    }

    /// The cursor tracks its workspace across a filter edit rather than its
    /// index, and clamps into range when the needle hides it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn updates_panel_selection_follows_workspace_across_filter_edits() {
        use crate::ui::modal::{Modal, UpdatesSort};
        use crossterm::event::KeyCode;
        let store = Store::open_in_memory().unwrap();
        seed_two_workspaces(&store);
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        // Start on beta (index 1), filter mode armed.
        app.modal = Some(Modal::UpdatesPanel {
            selected: 1,
            sort: UpdatesSort::Default,
            filter: Some(String::new()),
        });
        let shared = shared_app();

        // Typing "b" hides alpha; beta is now the only row, at index 0.
        handle_key_modal(&mut app, &shared, press(KeyCode::Char('b')))
            .await
            .unwrap();
        match app.modal {
            Some(Modal::UpdatesPanel { selected, .. }) => {
                assert_eq!(selected, 0, "cursor follows beta to its new row");
            }
            ref other => panic!("expected UpdatesPanel; got {other:?}"),
        }

        // Typing on until nothing matches: the index clamps rather than
        // pointing past the end of an empty list.
        for c in ['z', 'z'] {
            handle_key_modal(&mut app, &shared, press(KeyCode::Char(c)))
                .await
                .unwrap();
        }
        match app.modal {
            Some(Modal::UpdatesPanel { selected, .. }) => {
                assert_eq!(selected, 0, "empty result clamps to 0");
            }
            ref other => panic!("expected UpdatesPanel; got {other:?}"),
        }
    }

    /// Filter state lives in the modal variant, so reopening starts clean —
    /// a stale needle would silently hide rows on the next open.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn updates_panel_reopens_without_a_filter() {
        use crate::ui::modal::{Modal, UpdatesSort};
        use crossterm::event::{KeyCode, KeyEvent};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);
        let _ = ws_id;
        app.modal = Some(Modal::UpdatesPanel {
            selected: 0,
            sort: UpdatesSort::Default,
            filter: Some("stale".to_string()),
        });
        let shared = shared_app();
        handle_key_modal(&mut app, &shared, press(KeyCode::Esc))
            .await
            .unwrap();
        assert!(app.modal.is_none());

        // Reopen via the real leader path: Ctrl-X then 'u'.
        handle_key_attached(
            &mut app,
            &shared,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        handle_key_attached(&mut app, &shared, press(KeyCode::Char('u')))
            .await
            .unwrap();
        match app.modal {
            Some(Modal::UpdatesPanel { ref filter, .. }) => {
                assert_eq!(filter.as_deref(), None, "reopen starts unfiltered");
            }
            ref other => panic!("expected UpdatesPanel; got {other:?}"),
        }
    }
```

Mirror `updates_panel_reopens_in_default_after_close` (line ~1512) for the exact leader-key reopen sequence and the `spawn_attached_workspace` helper — copy its setup verbatim if the sketch above diverges from what that test actually does.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib updates_panel_`
Expected: FAIL to compile — `missing field 'filter' in initializer of 'Modal'`. Existing `UpdatesPanel { selected, sort }` literals elsewhere will also fail; that is expected and Step 4 fixes them.

- [ ] **Step 3: Add the field**

In `src/ui/modal/mod.rs`, extend the variant (line 72):

```rust
    UpdatesPanel {
        /// Index into the modal's ordered workspace list. Up/Down adjust
        /// it; Enter switches `app.view` to that workspace.
        selected: usize,
        /// Active sort mode; `o` cycles it. Not persisted — reset to
        /// `Default` on every open.
        sort: UpdatesSort,
        /// `None` = normal key handling. `Some(buf)` = filter-input mode,
        /// where printable keys are filter text rather than shortcuts.
        /// `Some("")` is a real state: `/` was pressed, nothing typed yet,
        /// every row still visible. Not persisted, like `sort`.
        filter: Option<String>,
    },
```

- [ ] **Step 4: Wire the key handler**

In `src/app/input.rs`, the `^x u` accelerator (line 995) opens unfiltered:

```rust
        KeyCode::Char('u') => {
            app.modal = Some(crate::ui::modal::Modal::UpdatesPanel {
                selected: 0,
                sort: crate::ui::modal::UpdatesSort::default(),
                filter: None,
            });
            Ok(())
        }
```

Add `reselect` next to `panel_order`:

```rust
/// Keep the cursor on its workspace across a re-order (a sort cycle or a
/// filter edit) rather than on its index. Falls back to clamping the old
/// index into range when the workspace is gone from the new order — which
/// is what a filter that hid it does.
fn reselect(
    selected_id: Option<crate::data::store::WorkspaceId>,
    new_order: &[crate::data::store::WorkspaceId],
    old_index: usize,
) -> usize {
    if let Some(pos) = selected_id.and_then(|id| new_order.iter().position(|w| *w == id)) {
        return pos;
    }
    old_index.min(new_order.len().saturating_sub(1))
}
```

Rewrite the head of the `Modal::UpdatesPanel` arm (line 1614):

```rust
        Modal::UpdatesPanel {
            selected,
            sort,
            filter,
        } => {
            let selected_now = selected;
            // Build the same ordered workspace list the renderer uses, so
            // arrow keys and Enter operate on the same indices.
            let order = panel_order(app, sort, filter.as_deref());
            // Filter-input mode: while the buffer is live, printable keys
            // edit it rather than firing j/k/o/l/v/s, and Esc clears the
            // filter instead of closing the panel. Arrows and Enter fall
            // through so the panel stays navigable mid-search. Mirrors the
            // dashboard's filter intercept (see `handle_key_dashboard`).
            if let Some(buf) = filter.as_ref() {
                let edited: Option<Option<String>> = match k.code {
                    KeyCode::Esc => Some(None),
                    KeyCode::Backspace => {
                        let mut b = buf.clone();
                        b.pop();
                        Some(Some(b))
                    }
                    KeyCode::Char(c)
                        if !c.is_control()
                            && !k.modifiers.contains(KeyModifiers::CONTROL)
                            && !k.modifiers.contains(KeyModifiers::ALT) =>
                    {
                        let mut b = buf.clone();
                        b.push(c);
                        Some(Some(b))
                    }
                    _ => None,
                };
                if let Some(new_filter) = edited {
                    let selected_id = order.get(selected_now).copied();
                    let new_order = panel_order(app, sort, new_filter.as_deref());
                    app.modal = Some(Modal::UpdatesPanel {
                        selected: reselect(selected_id, &new_order, selected_now),
                        sort,
                        filter: new_filter,
                    });
                    return Ok(());
                }
            }
            match k.code {
```

Then inside that `match k.code`, add the `filter` field to every `Modal::UpdatesPanel { .. }` construction:
- `Up`/`k` and `Down`/`j` arms: add `filter: filter.clone()`
- the `'o'` arm: add `filter: filter.clone()`, change its order calls to `panel_order(app, sort, filter.as_deref())` / `panel_order(app, new_sort, filter.as_deref())`, and replace its `.and_then(...).unwrap_or(0)` selection math with `reselect(selected_id, &new_order, selected_now)`

and add a new arm for `/`, placed before the catch-all `_ => {}`:

```rust
                // `/` arms filter mode. Reached only when `filter` is None —
                // an active buffer swallows printable keys above.
                KeyCode::Char('/') => {
                    app.modal = Some(Modal::UpdatesPanel {
                        selected: selected_now,
                        sort,
                        filter: Some(String::new()),
                    });
                }
```

The `Esc`, `Enter`/`l`, and `v`/`s` arms need no change: `Esc` still sets `app.modal = None` (reached only when there is no filter to clear), and the attach arms close the panel outright.

- [ ] **Step 5: Pass the needle to the renderer**

In `src/app/render.rs`, destructure the new field and forward it:

```rust
            crate::ui::modal::Modal::UpdatesPanel {
                selected,
                sort,
                filter,
            } => {
```

and at the call:

```rust
                crate::ui::modal::render_updates_panel(
                    f,
                    area,
                    &inputs,
                    *selected,
                    now_ms,
                    *sort,
                    filter.as_deref(),
                    &app.theme,
                );
```

Also check `src/app/render.rs:402`, which matches `Modal::UpdatesPanel { .. }` — the `..` means it needs no change.

- [ ] **Step 6: Update every existing `UpdatesPanel` literal in tests**

`src/app/input_tests.rs` constructs the variant at roughly lines 386, 435, 516, 638, 699, 820, 838, 1409, 1465, 1525, 1618, 1687. Add `filter: None` to each. Pattern matches that bind `{ selected, .. }` or `{ selected, sort }` need `..` added where they currently bind exhaustively — `{ selected, sort }` becomes `{ selected, sort, .. }`.

Run `cargo test --lib 2>&1 | grep -c "missing field"` to confirm none remain.

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test --lib updates_panel_`
Expected: PASS — the five new tests plus every pre-existing `updates_panel_*` test.

- [ ] **Step 8: Verify, fmt, clippy**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src/ui/modal/mod.rs src/app/input.rs src/app/render.rs src/app/input_tests.rs
git commit -m "feat: filter the updates panel with /

Printable keys become filter text while the buffer is live, arrows and
Enter keep navigating, and Esc clears before it closes. The cursor
tracks its workspace across filter edits rather than its index."
```

---

### Task 5: Echo the active filter on the dashboard

The dashboard has had a `/` filter with no visible needle: `DashboardState::filter` is read only by `matches_filter`, so rows vanish as you type with nothing on screen explaining why. Scope here is the echo only — matching rules, the footer, and the dashboard's empty state are untouched.

**Files:**
- Modify: `src/ui/dashboard/layout.rs:19-59` (`top_chrome`)
- Modify: `src/ui/dashboard/mod.rs:212-221` (call site)
- Test: `src/ui/dashboard/layout.rs` (existing test module, around line 197)

**Interfaces:**
- Consumes: `crate::ui::text::{truncate, FILTER_ECHO_MAX}` from Task 3.
- Produces: `pub fn top_chrome(group: GroupMode, repos: usize, workspaces: usize, filter: Option<&str>, width: usize, theme: &Theme) -> Line<'static>`

- [ ] **Step 1: Write the failing tests**

Add to the test module in `src/ui/dashboard/layout.rs`, next to `top_chrome_shows_app_name_and_counts`:

```rust
    /// Without the echo, `/` gives no feedback and rows disappearing from
    /// the list have no visible cause.
    #[test]
    fn top_chrome_echoes_the_active_filter() {
        let theme = Theme::wsx();
        let line = top_chrome(GroupMode::Repo, 9, 14, Some("auth"), 100, &theme);
        assert!(text(&line).contains("/auth"), "{:?}", text(&line));

        let bare = top_chrome(GroupMode::Repo, 9, 14, None, 100, &theme);
        assert!(!text(&bare).contains('/'), "{:?}", text(&bare));
    }

    /// `/` with an empty buffer still echoes, so the keypress registers
    /// before the first character is typed.
    #[test]
    fn top_chrome_echoes_an_empty_filter() {
        let theme = Theme::wsx();
        let line = top_chrome(GroupMode::Repo, 9, 14, Some(""), 100, &theme);
        assert!(text(&line).contains('/'), "{:?}", text(&line));
    }

    /// A long needle is truncated so it cannot displace the right-hand
    /// counts.
    #[test]
    fn top_chrome_truncates_a_long_filter_and_keeps_counts() {
        let theme = Theme::wsx();
        let needle = "x".repeat(80);
        let line = top_chrome(GroupMode::Repo, 9, 14, Some(&needle), 100, &theme);
        let t = text(&line);
        assert!(t.contains('…'), "{t:?}");
        assert!(t.trim_end().ends_with("9 repos · 14 workspaces"), "{t:?}");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib top_chrome`
Expected: FAIL to compile — `top_chrome` takes 5 arguments but 6 were supplied.

- [ ] **Step 3: Implement**

In `src/ui/dashboard/layout.rs`, add `filter: Option<&str>` to `top_chrome` between `workspaces` and `width`, and push the echo after the group tabs, before the `right` / `gap` computation:

```rust
pub fn top_chrome(
    group: GroupMode,
    repos: usize,
    workspaces: usize,
    filter: Option<&str>,
    width: usize,
    theme: &Theme,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = vec![
        // ...unchanged brand / group-tab spans...
    ];

    // Echo the live needle: without it, `/` looks inert and rows vanishing
    // from the list have no visible cause. Truncated so a long needle can't
    // squeeze out the right-hand counts, which floor their gap at 1.
    if let Some(needle) = filter {
        spans.push(Span::styled(
            format!(
                "  /{}",
                crate::ui::text::truncate(needle, crate::ui::text::FILTER_ECHO_MAX)
            ),
            Style::default()
                .fg(theme.warn)
                .add_modifier(Modifier::BOLD),
        ));
    }

    let right = format!("{repos} repos · {workspaces} workspaces");
    // ...unchanged gap math...
}
```

In `src/ui/dashboard/mod.rs:212`, pass the state's buffer:

```rust
    f.render_widget(
        Paragraph::new(layout::top_chrome(
            state.group_mode,
            inputs.repos.len(),
            inputs.workspaces.len(),
            state.filter.as_deref(),
            chunks[0].width as usize,
            theme,
        )),
        chunks[0],
    );
```

Update the existing `top_chrome_shows_app_name_and_counts` test to pass `None`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib top_chrome`
Expected: PASS.

- [ ] **Step 5: Verify, fmt, clippy**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/ui/dashboard/layout.rs src/ui/dashboard/mod.rs
git commit -m "fix: echo the active filter in the dashboard top chrome

DashboardState::filter was read only by matches_filter, so typing into
it silently removed rows with nothing on screen to explain why."
```

---

### Task 6: Manual test doc, verification, and PR

**Files:**
- Create: `docs/manual-tests/updates-panel-filter.md`

The keyboard behavior here (printable keys changing meaning, two-stage Esc) is the kind of thing unit tests confirm mechanically but only a human confirms *feels* right, which is what this directory is for.

- [ ] **Step 1: Write the manual test doc**

Create `docs/manual-tests/updates-panel-filter.md`, following the shape of the existing files in that directory (`# Title — manual test`, a short intent paragraph, `## Setup`, `## Scenarios` as a numbered list of action/expected pairs):

```markdown
# Updates panel workspace filter — manual test

Verifies the `/` filter in the agent updates panel: that printable keys
become filter text, that the needle matches names, repos, and status
text, that the cursor tracks its workspace as the list narrows, and that
Esc clears before it closes.

## Setup

Launch wsx with at least two repos registered, several workspaces in
each, and at least one live agent session:

```
wsx
```

Attach to any workspace, then press `Ctrl-x` followed by `u`.

## Scenarios

1. **Footer advertises the filter.** Expected: the footer reads
   `[↑↓] move  [↵] switch  [v/s] split  [o] sort:default  [/] filter
   [esc] close`, on one line, not clipped at the panel's right edge.

2. **`/` arms filter mode.** Press `/`. Expected: the footer becomes
   `/` followed by `[esc] clear  [↑↓] move  [↵] switch`. No rows have
   disappeared yet — the bare `/` is visible feedback that the key
   registered.

3. **Repo name narrows to one repo.** Type a registered repo's name.
   Expected: every other repo's section disappears, header included —
   not an empty header with no rows under it. All of the named repo's
   workspaces remain.

4. **Workspace name narrows to one row.** Clear (Esc) and type part of
   a workspace name. Expected: only matching rows remain, and the name
   column tightens to the longest surviving name.

5. **Status text is matchable.** Clear and type `no session`.
   Expected: only workspaces with no session remain. With a workspace
   sitting on a permission prompt, `permission` narrows to it.

6. **Printable keys are text, arrows still navigate.** With a filter
   active, press `j`. Expected: a `j` is appended to the needle and the
   selection does NOT move. Press `↓`. Expected: the selection moves and
   the needle is unchanged. Press Enter on a row. Expected: it attaches,
   same as without a filter.

7. **Cursor tracks its workspace.** Select a row partway down the list,
   then type a needle that keeps it but hides rows above it. Expected:
   the highlight stays on the same workspace as it moves up the list,
   rather than staying on the same screen row.

8. **No matches.** Type a needle matching nothing. Expected:
   `(no matching workspaces)` — not `(no workspaces)`, which would read
   as "you have none at all".

9. **Two-stage Esc.** Press Esc. Expected: the full list returns and the
   panel stays open. Press Esc again. Expected: the panel closes.

10. **Reopen starts clean.** Press `Ctrl-x` then `u` again. Expected: no
    filter is active and every workspace is listed.

11. **Dashboard echo.** Return to the dashboard and press `/`, then type.
    Expected: the needle appears next to the `group:` tabs in the top
    bar, so the vanishing rows have a visible cause. Esc clears it.
```

- [ ] **Step 2: Build and run**

```bash
cargo build --release
```

Note the pitfall recorded for this repo: building is not installing. Run the built binary directly, or install it, before judging behavior.

- [ ] **Step 3: Walk the doc**

Run every scenario in `docs/manual-tests/updates-panel-filter.md` against the built binary. Fix anything that does not match, then re-run `cargo test`.

- [ ] **Step 4: Commit the doc**

```bash
git add docs/manual-tests/updates-panel-filter.md
git commit -m "docs: manual test for the updates-panel filter"
```

- [ ] **Step 5: Open the PR**

```bash
git push -u origin HEAD
```

Then open a PR against `main` describing the feature, linking the spec, and noting the dashboard echo fix as an included drive-by.

---

## Self-Review

**Spec coverage:**

| Spec section | Task |
| --- | --- |
| State (`filter: Option<String>`, `Some("")` vs `None`) | Task 4 Step 3 |
| Keys table (`/`, printable, Backspace, arrows, Enter, two-stage Esc) | Task 4 Step 4 |
| CONTROL/ALT chars are not filter text | Task 4 Step 4 (the `Char(c)` guard) |
| Matching (name, repo name, status text; empty needle matches all) | Task 2 Steps 1, 3 |
| `row_status_text` extraction | Task 1 Steps 3, 4 |
| Filter applied in `ordered_workspaces_for_panel` | Task 2 Step 3 |
| `App::awaiting_permission_map` | Task 1 Step 8 |
| `PanelInputs`, both `too_many_arguments` allows removed | Task 1 Steps 5, 6 |
| Selection follows its workspace, clamps when hidden | Task 4 Step 4 (`reselect`) |
| Repo headers vanish with their rows | Task 2 Step 6 (`filtered_out_repo_draws_no_header`) |
| `(no matching workspaces)` empty state | Task 2 Steps 4, 6 |
| Footer, both variants, 78-char budget | Task 3 |
| Dashboard needle echo | Task 5 |
| Every test the spec lists | Tasks 1-5, test steps |

**Placeholder scan:** none. Every code step carries the actual code; the one judgment call (Task 1 Step 5's `#[allow(dead_code)]` versus folding the `impl` into Task 2) states a recommendation rather than leaving it open.

**Type consistency:** `PanelInputs` field names are identical in Task 1 Step 5 (definition), Step 9 (render.rs), Step 10 (`panel_order`), Step 11 (`ordering_tests`), and Task 2 Step 6 (`render_tests`). `row_status_text` keeps one signature across Tasks 1 and 2. `panel_order` gains its `filter` parameter in Task 2 Step 4 and every later call site passes it. `footer_text(sort, filter)` is consistent across Task 3's test and implementation. `FILTER_ECHO_MAX` is defined once (Task 3) and consumed in Tasks 3 and 5.
