# Updates-Modal Sort Modes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user cycle the workspace-updates modal's within-repo ordering between the default sort, workspace-status urgency, and PR-lifecycle order with the `o` key.

**Architecture:** A new `UpdatesSort` enum lives in `src/ui/modal/updates_panel.rs` next to the shared ordering function `ordered_workspaces_for_panel`, which gains `statuses`/`lifecycles`/`sort` parameters. The mode is carried in the `Modal::UpdatesPanel` variant (resets to default on every open). Renderer and key handler keep calling the one shared ordering function so rows and key indices never diverge.

**Tech Stack:** Rust, ratatui, crossterm. Tests via `cargo test`.

## Global Constraints

- Repo grouping is unchanged in every mode; sorts apply within each repo section only.
- Sort mode is NOT persisted — every modal open starts in `UpdatesSort::Default`.
- Status-mode order: Failed → Stalled → Question → Waiting → Thinking → Complete → Idle (reuses `Status::priority()`, failed outranks all).
- PrStatus-mode order: Conflicted → Open → Draft → Merged → Closed → NoPr → unknown (no lifecycle data).
- Both modes tie-break with the existing default key `(attention, failed, activity_rank, recency)`.
- CI gates: `cargo fmt --check`, `cargo clippy`, `cargo test` must all pass before each commit.

---

### Task 1: `UpdatesSort` enum and sort-aware ordering

**Files:**
- Modify: `src/ui/modal/updates_panel.rs` (enum + `ordered_workspaces_for_panel` + `render_updates_panel` signature)
- Modify: `src/ui/modal/mod.rs:27` (re-export)
- Modify: `src/app/render.rs:738-752` (caller, passes placeholder `UpdatesSort::Default`)
- Modify: `src/app/input.rs:1445-1451` (caller, passes placeholder `UpdatesSort::Default`)
- Test: `src/ui/modal/updates_panel.rs` (new `ordering_tests` module)

**Interfaces:**
- Consumes: existing `Status::priority()` (`src/ui/dashboard/status.rs`), `BranchLifecycle` (`src/git/forge.rs`), existing private `sort_key` in `updates_panel.rs`.
- Produces (later tasks rely on these exact names):
  - `pub enum UpdatesSort { Default, Status, PrStatus }` with `pub fn cycle(self) -> Self` and `pub fn footer_label(self) -> &'static str` (returns `"default"` / `"status"` / `"pr"`), re-exported from `crate::ui::modal`.
  - `ordered_workspaces_for_panel(repos, workspaces, events, activity, needs_attention, statuses: &HashMap<WorkspaceId, Status>, lifecycles: &HashMap<WorkspaceId, BranchLifecycle>, sort: UpdatesSort) -> Vec<WorkspaceId>` — three new trailing params.
  - `render_updates_panel(..)` gains a trailing `sort: UpdatesSort` param inserted before `theme` (after `now_ms`).

- [ ] **Step 1: Write the failing tests**

Add a new test module at the end of `src/ui/modal/updates_panel.rs`:

```rust
#[cfg(test)]
mod ordering_tests {
    use super::*;
    use crate::data::store::{Repo, RepoId, Workspace, WorkspaceId, WorkspaceState};
    use crate::git::forge::BranchLifecycle;
    use std::path::PathBuf;

    fn fixture_repo(id: u64) -> Repo {
        Repo {
            id: RepoId(id),
            name: format!("repo{id}"),
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
        }
    }

    fn fixture_ws(id: u64, repo: u64, name: &str) -> (RepoId, Workspace) {
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

    /// Bundles the three signal maps the ordering function reads, so each
    /// test only fills in what it exercises.
    #[derive(Default)]
    struct Maps {
        events: HashMap<WorkspaceId, crate::activity::events::WorkspaceEvents>,
        activity: HashMap<WorkspaceId, crate::ui::updates_bar::ActivityState>,
        attention: HashSet<WorkspaceId>,
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
            repos,
            ws,
            &maps.events,
            &maps.activity,
            &maps.attention,
            &maps.statuses,
            &maps.lifecycles,
            sort,
        )
    }

    #[test]
    fn cycle_walks_default_status_pr_and_back() {
        assert_eq!(UpdatesSort::Default.cycle(), UpdatesSort::Status);
        assert_eq!(UpdatesSort::Status.cycle(), UpdatesSort::PrStatus);
        assert_eq!(UpdatesSort::PrStatus.cycle(), UpdatesSort::Default);
    }

    #[test]
    fn footer_labels_match_modes() {
        assert_eq!(UpdatesSort::Default.footer_label(), "default");
        assert_eq!(UpdatesSort::Status.footer_label(), "status");
        assert_eq!(UpdatesSort::PrStatus.footer_label(), "pr");
    }

    /// Status mode: failed workspaces outrank everything, then statuses by
    /// descending urgency (Status::priority), Idle last.
    #[test]
    fn status_sort_ranks_failed_then_urgency() {
        let repos = vec![fixture_repo(1)];
        let mut ws = vec![
            fixture_ws(1, 1, "idle"),
            fixture_ws(2, 1, "stalled"),
            fixture_ws(3, 1, "failed"),
            fixture_ws(4, 1, "question"),
        ];
        ws[2].1.state = WorkspaceState::Failed;
        let mut maps = Maps::default();
        maps.statuses.insert(WorkspaceId(1), Status::Idle);
        maps.statuses.insert(WorkspaceId(2), Status::Stalled);
        maps.statuses.insert(WorkspaceId(4), Status::Question);
        let got = order(&repos, &ws, &maps, UpdatesSort::Status);
        assert_eq!(
            got,
            vec![WorkspaceId(3), WorkspaceId(2), WorkspaceId(4), WorkspaceId(1)],
            "failed → stalled → question → idle"
        );
    }

    /// A workspace missing from the statuses map ranks as Idle (last).
    #[test]
    fn status_sort_treats_missing_status_as_idle() {
        let repos = vec![fixture_repo(1)];
        let ws = vec![fixture_ws(1, 1, "unknown"), fixture_ws(2, 1, "complete")];
        let mut maps = Maps::default();
        maps.statuses.insert(WorkspaceId(2), Status::Complete);
        let got = order(&repos, &ws, &maps, UpdatesSort::Status);
        assert_eq!(got, vec![WorkspaceId(2), WorkspaceId(1)]);
    }

    /// PrStatus mode: actionable first — Conflicted → Open → Draft →
    /// Merged → Closed → NoPr → unknown (absent from the map).
    #[test]
    fn pr_sort_ranks_actionable_first() {
        use BranchLifecycle::*;
        let repos = vec![fixture_repo(1)];
        let ws = vec![
            fixture_ws(1, 1, "unknown"),
            fixture_ws(2, 1, "nopr"),
            fixture_ws(3, 1, "closed"),
            fixture_ws(4, 1, "merged"),
            fixture_ws(5, 1, "draft"),
            fixture_ws(6, 1, "open"),
            fixture_ws(7, 1, "conflicted"),
        ];
        let mut maps = Maps::default();
        maps.lifecycles.insert(WorkspaceId(2), NoPr);
        maps.lifecycles.insert(WorkspaceId(3), PrClosed);
        maps.lifecycles.insert(WorkspaceId(4), PrMerged);
        maps.lifecycles.insert(WorkspaceId(5), PrDraft);
        maps.lifecycles.insert(WorkspaceId(6), PrOpen);
        maps.lifecycles.insert(WorkspaceId(7), PrConflicted);
        let got = order(&repos, &ws, &maps, UpdatesSort::PrStatus);
        assert_eq!(
            got,
            vec![
                WorkspaceId(7),
                WorkspaceId(6),
                WorkspaceId(5),
                WorkspaceId(4),
                WorkspaceId(3),
                WorkspaceId(2),
                WorkspaceId(1),
            ]
        );
    }

    /// Ties within a mode fall back to the default key — here two PrOpen
    /// workspaces where one needs attention: attention wins the tie.
    #[test]
    fn mode_ties_fall_back_to_default_key() {
        use BranchLifecycle::*;
        let repos = vec![fixture_repo(1)];
        let ws = vec![fixture_ws(1, 1, "calm"), fixture_ws(2, 1, "alert")];
        let mut maps = Maps::default();
        maps.lifecycles.insert(WorkspaceId(1), PrOpen);
        maps.lifecycles.insert(WorkspaceId(2), PrOpen);
        maps.attention.insert(WorkspaceId(2));
        let got = order(&repos, &ws, &maps, UpdatesSort::PrStatus);
        assert_eq!(got, vec![WorkspaceId(2), WorkspaceId(1)]);
    }

    /// Default mode ignores the new maps entirely — a merged PR must not
    /// reorder anything when sort is Default.
    #[test]
    fn default_sort_ignores_status_and_lifecycle_maps() {
        use BranchLifecycle::*;
        let repos = vec![fixture_repo(1)];
        let ws = vec![fixture_ws(1, 1, "first"), fixture_ws(2, 1, "merged")];
        let mut maps = Maps::default();
        maps.lifecycles.insert(WorkspaceId(2), PrConflicted);
        maps.statuses.insert(WorkspaceId(2), Status::Stalled);
        let got = order(&repos, &ws, &maps, UpdatesSort::Default);
        assert_eq!(
            got,
            vec![WorkspaceId(1), WorkspaceId(2)],
            "default keys are equal; stable sort keeps input order"
        );
    }

    /// Sorting never crosses repo boundaries: a conflicted PR in repo 2
    /// stays under repo 2's header even though it outranks repo 1's rows.
    #[test]
    fn sorts_stay_within_repo_groups() {
        use BranchLifecycle::*;
        let repos = vec![fixture_repo(1), fixture_repo(2)];
        let ws = vec![
            fixture_ws(1, 1, "r1-open"),
            fixture_ws(2, 2, "r2-conflicted"),
        ];
        let mut maps = Maps::default();
        maps.lifecycles.insert(WorkspaceId(1), PrOpen);
        maps.lifecycles.insert(WorkspaceId(2), PrConflicted);
        let got = order(&repos, &ws, &maps, UpdatesSort::PrStatus);
        assert_eq!(
            got,
            vec![WorkspaceId(1), WorkspaceId(2)],
            "repo 1's workspaces list before repo 2's regardless of rank"
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail to compile**

Run: `cargo test --lib ordering_tests 2>&1 | tail -20`
Expected: compile error — `UpdatesSort` not found / `ordered_workspaces_for_panel` takes 5 args, not 8.

- [ ] **Step 3: Implement the enum and the sort-aware ordering**

In `src/ui/modal/updates_panel.rs`, after the `name_col_width` fn, add:

```rust
/// User-cyclable sort mode for the updates panel. Carried in the modal
/// variant, so it resets to `Default` every time the panel opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdatesSort {
    /// Today's ordering: (attention, failed, activity_rank, recency).
    #[default]
    Default,
    /// Workspace-status urgency via `Status::priority()`; failed first.
    Status,
    /// PR lifecycle, actionable first: conflicted → open → draft →
    /// merged → closed → no PR → unknown.
    PrStatus,
}

impl UpdatesSort {
    /// Next mode in the `o`-key cycle.
    pub fn cycle(self) -> Self {
        match self {
            Self::Default => Self::Status,
            Self::Status => Self::PrStatus,
            Self::PrStatus => Self::Default,
        }
    }

    /// Short mode name shown in the footer hint.
    pub fn footer_label(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Status => "status",
            Self::PrStatus => "pr",
        }
    }
}

/// Status-mode rank: lower sorts first. Failed outranks every status —
/// it's the loudest signal — then descending `Status::priority()`.
fn status_rank(
    w: &crate::data::store::Workspace,
    statuses: &HashMap<crate::data::store::WorkspaceId, Status>,
) -> u8 {
    if w.state == crate::data::store::WorkspaceState::Failed {
        return 0;
    }
    let urgency = statuses.get(&w.id).copied().unwrap_or(Status::Idle).priority();
    // priority() is 0..=5 with higher = more urgent; invert so Stalled(5)
    // ranks 1 (right after failed) and Idle(0) ranks 6 (last).
    6 - urgency
}

/// PrStatus-mode rank: actionable lifecycles first, unknown last.
fn lifecycle_rank(lifecycle: Option<BranchLifecycle>) -> u8 {
    match lifecycle {
        Some(BranchLifecycle::PrConflicted) => 0,
        Some(BranchLifecycle::PrOpen) => 1,
        Some(BranchLifecycle::PrDraft) => 2,
        Some(BranchLifecycle::PrMerged) => 3,
        Some(BranchLifecycle::PrClosed) => 4,
        Some(BranchLifecycle::NoPr) => 5,
        None => 6,
    }
}
```

Extend `ordered_workspaces_for_panel` — new params and per-mode key (the doc comment's "sorted within each repo by (attention, failed, activity_rank, recency)" sentence becomes "sorted within each repo by the active `UpdatesSort` mode, tie-broken by (attention, failed, activity_rank, recency)"):

```rust
#[allow(clippy::too_many_arguments)]
pub fn ordered_workspaces_for_panel(
    repos: &[crate::data::store::Repo],
    workspaces: &[(RepoId, crate::data::store::Workspace)],
    events: &HashMap<crate::data::store::WorkspaceId, crate::activity::events::WorkspaceEvents>,
    activity: &HashMap<crate::data::store::WorkspaceId, crate::ui::updates_bar::ActivityState>,
    needs_attention: &HashSet<crate::data::store::WorkspaceId>,
    statuses: &HashMap<crate::data::store::WorkspaceId, Status>,
    lifecycles: &HashMap<crate::data::store::WorkspaceId, BranchLifecycle>,
    sort: UpdatesSort,
) -> Vec<crate::data::store::WorkspaceId> {
    let mut out = Vec::new();
    for repo in repos {
        let mut ws_for_repo: Vec<&crate::data::store::Workspace> = workspaces
            .iter()
            .filter(|(rid, _)| *rid == repo.id)
            .map(|(_, w)| w)
            .collect();
        ws_for_repo.sort_by_key(|w| {
            let default_key = sort_key(w, events, activity, needs_attention);
            let mode_rank = match sort {
                UpdatesSort::Default => 0,
                UpdatesSort::Status => status_rank(w, statuses),
                UpdatesSort::PrStatus => lifecycle_rank(lifecycles.get(&w.id).copied()),
            };
            (mode_rank, default_key)
        });
        out.extend(ws_for_repo.into_iter().map(|w| w.id));
    }
    out
}
```

Add a `sort: UpdatesSort` parameter to `render_updates_panel` (insert between `now_ms: i64` and `theme: &Theme`) and forward the three new args at its internal call site:

```rust
    let order = ordered_workspaces_for_panel(
        repos,
        workspaces,
        events,
        activity,
        needs_attention,
        statuses,
        lifecycles,
        sort,
    );
```

- [ ] **Step 4: Update the re-export and both callers (placeholder sort)**

`src/ui/modal/mod.rs:27`:

```rust
pub use updates_panel::{UpdatesSort, ordered_workspaces_for_panel, render_updates_panel};
```

`src/app/render.rs` (inside the `Modal::UpdatesPanel { selected }` arm, ~line 738): add the new args to `render_updates_panel` — `&statuses` and `&app.pr_lifecycle` are already passed; add `crate::ui::modal::UpdatesSort::Default` between `now_ms` and `&app.theme`. (Task 2 replaces this placeholder with the modal's field.)

`src/app/input.rs` (~line 1445, the `Modal::UpdatesPanel` arm): the arm must now build the statuses map before calling the ordering fn, mirroring render.rs:

```rust
            let statuses: std::collections::HashMap<
                crate::data::store::WorkspaceId,
                crate::ui::dashboard::status::Status,
            > = app
                .workspaces
                .iter()
                .map(|(_, w)| (w.id, app.classify_status(w)))
                .collect();
            let order = crate::ui::modal::ordered_workspaces_for_panel(
                &app.repos,
                &app.workspaces,
                &app.workspace_events,
                &activity_translated,
                &app.workspace_needs_attention,
                &statuses,
                &app.pr_lifecycle,
                crate::ui::modal::UpdatesSort::Default,
            );
```

- [ ] **Step 5: Run the test suite**

Run: `cargo test --lib 2>&1 | tail -5`
Expected: PASS (all new ordering_tests green, no regressions).

- [ ] **Step 6: Lint, format, commit**

```bash
cargo fmt && cargo fmt --check && cargo clippy --all-targets 2>&1 | tail -3
git add src/ui/modal/updates_panel.rs src/ui/modal/mod.rs src/app/render.rs src/app/input.rs
git commit -m "feat(tui): sort-aware ordering for the workspace-updates panel"
```

---

### Task 2: `o` key cycles the sort; footer shows the mode

**Files:**
- Modify: `src/ui/modal/mod.rs:87-91` (`Modal::UpdatesPanel` gains `sort` field)
- Modify: `src/app/input.rs:943` (open site), `src/app/input.rs:1433-1464` (arm destructure, Up/Down rebuilds, new `o` arm, placeholder removal)
- Modify: `src/app/render.rs:711` (destructure + pass real sort)
- Modify: `src/ui/modal/updates_panel.rs` (dynamic footer)
- Test: `src/app/input_tests.rs` (update ~15 existing constructors; add cycle + selection tests)

**Interfaces:**
- Consumes (from Task 1): `crate::ui::modal::UpdatesSort` with `cycle()`, `footer_label()`, `Default` impl; 8-arg `ordered_workspaces_for_panel`; `render_updates_panel` with `sort` before `theme`.
- Produces: `Modal::UpdatesPanel { selected: usize, sort: crate::ui::modal::UpdatesSort }` — the variant every constructor and matcher must now use.

- [ ] **Step 1: Write the failing tests**

In `src/app/input_tests.rs`, next to the existing `updates_panel_modal_*` tests:

```rust
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn updates_panel_o_cycles_sort_and_follows_selection() {
        use crate::data::store::{NewWorkspace, Store, WorkspaceState};
        use crate::git::forge::BranchLifecycle;
        use crate::ui::modal::UpdatesSort;
        let store = Store::open_in_memory().unwrap();
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
            store
                .set_workspace_state(id, WorkspaceState::Ready)
                .unwrap();
            ids.push(id);
        }
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        // beta has an open PR, alpha none — under PrStatus beta sorts first,
        // flipping the two rows relative to Default/Status order.
        app.pr_lifecycle.insert(ids[1], BranchLifecycle::PrOpen);
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel {
            selected: 0, // alpha
            sort: UpdatesSort::Default,
        });
        let shared = Arc::new(Mutex::new(
            App::new(
                Store::open_in_memory().unwrap(),
                PathBuf::from("/tmp/wsx-test"),
            )
            .unwrap(),
        ));
        let press_o = KeyEvent::new(
            crossterm::event::KeyCode::Char('o'),
            KeyModifiers::NONE,
        );

        // Default → Status: both workspaces are Idle, order unchanged,
        // selection stays on alpha at index 0.
        handle_key_modal(&mut app, &shared, press_o).await.unwrap();
        match app.modal {
            Some(crate::ui::modal::Modal::UpdatesPanel { selected, sort }) => {
                assert_eq!(sort, UpdatesSort::Status);
                assert_eq!(selected, 0, "selection stays on alpha");
            }
            ref other => panic!("unexpected modal state: {other:?}"),
        }

        // Status → PrStatus: beta (open PR) jumps to index 0; the cursor
        // must follow alpha to index 1 rather than staying on row 0.
        handle_key_modal(&mut app, &shared, press_o).await.unwrap();
        match app.modal {
            Some(crate::ui::modal::Modal::UpdatesPanel { selected, sort }) => {
                assert_eq!(sort, UpdatesSort::PrStatus);
                assert_eq!(selected, 1, "cursor follows alpha to its new row");
            }
            ref other => panic!("unexpected modal state: {other:?}"),
        }

        // PrStatus → Default: back to the original order and back to row 0.
        handle_key_modal(&mut app, &shared, press_o).await.unwrap();
        match app.modal {
            Some(crate::ui::modal::Modal::UpdatesPanel { selected, sort }) => {
                assert_eq!(sort, UpdatesSort::Default);
                assert_eq!(selected, 0, "cursor follows alpha back to row 0");
            }
            ref other => panic!("unexpected modal state: {other:?}"),
        }
    }
```

Also update every existing `Modal::UpdatesPanel { selected: 0 }` constructor in `src/app/input_tests.rs` (lines ~386, 432, 510, 629, 687, 805, 1387, 1449, 1515, 1560 — grep for them) to:

```rust
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel {
            selected: 0,
            sort: crate::ui::modal::UpdatesSort::Default,
        });
```

and every match pattern `Some(crate::ui::modal::Modal::UpdatesPanel { selected })` to `Some(crate::ui::modal::Modal::UpdatesPanel { selected, .. })` (lines ~448, 462, 476, 528, 543).

And in `src/ui/modal/updates_panel.rs`, add to the `ordering_tests` module from Task 1 a footer test — the footer becomes a pure helper `footer_text` so it's testable without a Frame:

```rust
    #[test]
    fn footer_shows_active_sort_mode_and_fits_panel() {
        for (sort, label) in [
            (UpdatesSort::Default, "sort:default"),
            (UpdatesSort::Status, "sort:status"),
            (UpdatesSort::PrStatus, "sort:pr"),
        ] {
            let f = footer_text(sort);
            assert!(f.contains(label), "footer {f:?} must contain {label:?}");
            assert!(f.contains("[o]"), "footer must advertise the o key");
            assert!(
                f.chars().count() <= 78,
                "footer must fit the widest panel (80 - 2 border): {f:?}"
            );
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail to compile**

Run: `cargo test --lib 2>&1 | tail -20`
Expected: compile errors — variant `UpdatesPanel` has no field `sort`; `footer_text` not found.

- [ ] **Step 3: Extend the modal variant**

`src/ui/modal/mod.rs:87-91`:

```rust
    UpdatesPanel {
        /// Index into the modal's ordered workspace list. Up/Down adjust
        /// it; Enter switches `app.view` to that workspace.
        selected: usize,
        /// Active sort mode; `o` cycles it. Not persisted — reset to
        /// `Default` on every open.
        sort: UpdatesSort,
    },
```

- [ ] **Step 4: Wire the open site, key-handler arm, and renderer**

`src/app/input.rs:943` (leader-`u` open):

```rust
            app.modal = Some(crate::ui::modal::Modal::UpdatesPanel {
                selected: 0,
                sort: crate::ui::modal::UpdatesSort::default(),
            });
```

`src/app/input.rs:1433` arm — destructure both fields and use the live sort in the ordering call (replacing Task 1's placeholder):

```rust
        Modal::UpdatesPanel { selected, sort } => {
```

and in the `ordered_workspaces_for_panel` call, replace `crate::ui::modal::UpdatesSort::Default,` with `sort,`.

Up/Down arms preserve the mode:

```rust
                KeyCode::Up | KeyCode::Char('k') => {
                    let new_sel = selected_now.saturating_sub(1);
                    app.modal = Some(Modal::UpdatesPanel {
                        selected: new_sel,
                        sort,
                    });
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let max = order.len().saturating_sub(1);
                    let new_sel = (selected_now + 1).min(max);
                    app.modal = Some(Modal::UpdatesPanel {
                        selected: new_sel,
                        sort,
                    });
                }
```

New `o` arm (place after the Down arm, before Enter): cycle the mode and re-point `selected` at the same workspace's new index:

```rust
                // 'o' (order) cycles the sort mode. The cursor follows the
                // selected workspace to its new row rather than staying on
                // the same index.
                KeyCode::Char('o') => {
                    let selected_id = order.get(selected_now).copied();
                    let new_sort = sort.cycle();
                    let new_order = crate::ui::modal::ordered_workspaces_for_panel(
                        &app.repos,
                        &app.workspaces,
                        &app.workspace_events,
                        &activity_translated,
                        &app.workspace_needs_attention,
                        &statuses,
                        &app.pr_lifecycle,
                        new_sort,
                    );
                    let new_sel = selected_id
                        .and_then(|id| new_order.iter().position(|w| *w == id))
                        .unwrap_or(0);
                    app.modal = Some(Modal::UpdatesPanel {
                        selected: new_sel,
                        sort: new_sort,
                    });
                }
```

`src/app/render.rs:711`: destructure `Modal::UpdatesPanel { selected, sort }` and replace the Task 1 placeholder in the `render_updates_panel` call with `*sort` (positioned between `now_ms` and `&app.theme`).

- [ ] **Step 5: Dynamic footer in the renderer**

In `src/ui/modal/updates_panel.rs`, add next to `name_col_width`:

```rust
/// Footer hint line. `v`/`s` collapse into one `[v/s] split` chip so the
/// line still fits the widest panel (80 cols − 2 border = 78) with the
/// sort mode shown.
fn footer_text(sort: UpdatesSort) -> String {
    format!(
        "[\u{2191}/\u{2193}] move  [enter/l] switch  [v/s] split  [o] sort:{}  [esc] close",
        sort.footer_label()
    )
}
```

and replace the static footer `Paragraph` at the end of `render_updates_panel`:

```rust
    f.render_widget(
        Paragraph::new(footer_text(sort)).style(theme.dim_style()),
        footer_area,
    );
```

- [ ] **Step 6: Run the test suite**

Run: `cargo test --lib 2>&1 | tail -5`
Expected: PASS — new cycle/selection/footer tests green, all existing `updates_panel_*` tests still green. (`click_chip_auto_spawns_session_when_missing` is a known flaky PTY-timing test; rerun if it alone fails.)

- [ ] **Step 7: Lint, format, commit**

```bash
cargo fmt && cargo fmt --check && cargo clippy --all-targets 2>&1 | tail -3
git add src/ui/modal/mod.rs src/ui/modal/updates_panel.rs src/app/input.rs src/app/render.rs src/app/input_tests.rs
git commit -m "feat(tui): 'o' cycles updates-panel sort by status or PR state"
```
