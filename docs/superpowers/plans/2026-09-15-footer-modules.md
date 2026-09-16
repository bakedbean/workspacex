# Dashboard Footer Modules Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let users define footer/bar modules in `theme.toml` as `[module.<name>]` tables composed from fleet-wide variables, and ship a `funnel` preset that replaces the usage sparkline as the dashboard footer's default right-hand content.

**Architecture:** A static `FLEET_VARS` table (beside `SEGMENTS`) names the variables a module may use; `FleetStats` derives their values once per frame from `App`'s in-memory maps and exposes them as a `SegmentMap`. The theme loader resolves `[module.*]` tables into ordinary `SegmentConfig`s (validated against `FLEET_VARS`, stored in `BarSpecs.segments` so priority-drop works unchanged) and records their names in `BarSpecs.modules`; every bar composer inserts each module's rendered segment before `render_bar`, so `$<name>` works in any bar.

**Tech Stack:** Rust, ratatui, serde + toml (theme file), rusqlite (store). Tests are `cargo test` unit tests inside each module.

**Spec:** `docs/superpowers/specs/2026-09-15-footer-modules-design.md`

## Global Constraints

- Format grammar is unchanged: `$name` identifiers are `[A-Za-z0-9_]` (`src/ui/bar/format.rs:112`). Modules are referenced as `$<table name>`.
- Fleet count variables render **empty at zero** (so `( … )` groups drop); `workspaces` and `repos` always render a number.
- Modules carry **no click hit**.
- `$usage` stays a registered segment; only the bundled default stops placing it.
- Every module name must be disjoint from `SEGMENTS` names; every fleet variable name must be disjoint from `SEGMENTS` names and `ITEM_COLORS`. (Spec amendment: the attention-alert count is named `alerts`, not `attention`, because `attention` is a segment.)
- Verification before every commit: `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `mise exec rust@1.95.0 -- cargo fmt --all --check` (CI pins rustfmt 1.95.0; the ambient rustfmt can false-pass).
- Some `app::input` / `pty::command` PTY-timing tests flake under full `cargo test`; re-run a failing one in isolation before treating it as a regression.
- Commit messages end with `Claude-Session: https://claude.ai/code/session_01RYWyvE139mYRPJw3JCapzx`.
- Run all commands from the worktree root `/home/eben/.local/state/wsx/worktrees/workspacex/glossy-saffron`.

---

## File map

| File | Responsibility |
|---|---|
| `src/ui/bar/registry.rs` | Add `FleetVar`, `FLEET_VARS`, `fleet_var()` beside `SEGMENTS`. |
| `src/ui/bar/fleet.rs` (new) | `FleetRow`, `FleetStats::{from_rows, collect, to_vars}`, `empty()`. Pure; no I/O. |
| `src/ui/bar/mod.rs` | `pub mod fleet;` |
| `src/app/state.rs` | `App.msgs_queued: u32`. |
| `src/app/status.rs` | `App::live_workspace_count()` (extracted from `run.rs`). |
| `src/app/run.rs` | Use `live_workspace_count()`. |
| `src/app/messaging.rs` | Set `msgs_queued` in `drain_agent_messages`. |
| `src/config/theme_file.rs` | `ModuleTable`, `ThemeFile.module`, `resolve_module`, `BarSpecs.modules`, allowed-name union. |
| `src/ui/bar/providers.rs` | `module()` provider. |
| `src/ui/bar/bars.rs` | `put_modules()`; `fleet` on every inputs struct / composer. |
| `src/ui/dashboard/mod.rs`, `detail.rs`, `tests.rs` | Thread `fleet` through `DashboardInputs`, `DetailInputs`, `render_footer`. |
| `src/ui/attached/mod.rs`, `src/app/render/{attached,dashboard,mod}.rs` | Thread `fleet` into attached bars and the dashboard frame. |
| `src/ui/bar/tests.rs` | Composer tests: modules render, drop, and the new default snapshot. |
| `src/ui/bar/default_theme.toml` | `[module.funnel]`, new footer `right_format`, grammar comment. |
| `docs/examples/theme-*.toml` | Place `$funnel` in place of `$usage`. |
| `docs/book/src/configuration/themes.md` | `### Modules` section. |

---

### Task 1: `FLEET_VARS` registry

**Files:**
- Modify: `src/ui/bar/registry.rs` (after `singleton_names`, ~line 194; tests module at ~196)

**Interfaces:**
- Produces: `pub struct FleetVar { pub name: &'static str, pub doc: &'static str }`, `pub const FLEET_VARS: &[FleetVar]`, `pub fn fleet_var(name: &str) -> Option<&'static FleetVar>`, `pub fn fleet_var_names() -> Vec<&'static str>`.

- [ ] **Step 1: Write the failing tests**

Append inside the existing `mod tests` in `src/ui/bar/registry.rs`:

```rust
    #[test]
    fn fleet_vars_are_unique_snake_case_and_disjoint_from_segments() {
        let mut seen = std::collections::HashSet::new();
        for v in FLEET_VARS {
            assert!(seen.insert(v.name), "duplicate fleet var {}", v.name);
            assert!(
                v.name.chars().all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit()),
                "fleet var {} is not snake_case",
                v.name
            );
            assert!(segment_def(v.name).is_none(), "fleet var {} collides with a segment", v.name);
            assert!(!ITEM_COLORS.contains(&v.name), "fleet var {} collides with an item colour", v.name);
            assert!(!v.doc.is_empty(), "fleet var {} has no doc", v.name);
        }
    }

    #[test]
    fn fleet_var_finds_by_name_and_misses_unknown_names() {
        assert_eq!(fleet_var("mergeable").map(|v| v.name), Some("mergeable"));
        assert!(fleet_var("attention").is_none(), "attention is a segment, not a fleet var");
        assert!(fleet_var("nope").is_none());
        assert!(fleet_var_names().contains(&"workspaces"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib ui::bar::registry`
Expected: compile error — `FLEET_VARS`, `fleet_var`, `fleet_var_names` not found.

- [ ] **Step 3: Add the table**

Insert after `singleton_names()` in `src/ui/bar/registry.rs`:

```rust
/// A fleet-wide variable a `[module.<name>]` format may reference. Values
/// are derived once per frame by `crate::ui::bar::fleet::FleetStats`; the
/// loader validates module formats against this list exactly as it
/// validates a segment's `format` against its `SegmentDef::vars`.
pub struct FleetVar {
    pub name: &'static str,
    /// One line for the book's variable table and the loader's error hint.
    pub doc: &'static str,
}

/// Every fleet variable. Counts render empty at zero so a `( … )` group
/// around one drops; `workspaces` and `repos` always render a number.
pub const FLEET_VARS: &[FleetVar] = &[
    FleetVar { name: "working", doc: "workspaces whose last reported status is `working`" },
    FleetVar { name: "waiting", doc: "workspaces whose last reported status is `waiting`" },
    FleetVar { name: "blocked", doc: "workspaces whose last reported status is `blocked`" },
    FleetVar { name: "done", doc: "workspaces whose last reported status is `done`" },
    FleetVar { name: "busy", doc: "workspaces parked on background work (hook-inferred `busy`)" },
    FleetVar { name: "unreported", doc: "workspaces with no reported status" },
    FleetVar { name: "alerts", doc: "workspaces with an unacknowledged attention alert" },
    FleetVar { name: "awaiting", doc: "workspaces whose agent is awaiting an answer" },
    FleetVar { name: "stalled", doc: "workspaces whose agent has stalled" },
    FleetVar { name: "active", doc: "workspaces whose agent is actively working" },
    FleetVar { name: "idle", doc: "workspaces whose agent is idle" },
    FleetVar { name: "live_agents", doc: "workspaces with a live (thinking or waiting) primary session" },
    FleetVar { name: "pr_none", doc: "workspaces polled with no PR" },
    FleetVar { name: "pr_draft", doc: "workspaces with a draft PR" },
    FleetVar { name: "pr_open", doc: "workspaces with an open PR" },
    FleetVar { name: "pr_conflicted", doc: "workspaces with a conflicted PR" },
    FleetVar { name: "pr_merged", doc: "workspaces whose PR merged (not yet archived)" },
    FleetVar { name: "pr_closed", doc: "workspaces whose PR was closed unmerged" },
    FleetVar { name: "review_required", doc: "PRs still awaiting a review" },
    FleetVar { name: "changes_requested", doc: "PRs with changes requested" },
    FleetVar { name: "approved", doc: "PRs approved" },
    FleetVar { name: "unresolved", doc: "unresolved review threads across the fleet" },
    FleetVar { name: "mergeable", doc: "PRs that are open and approved" },
    FleetVar { name: "dirty", doc: "workspaces with modified or untracked files" },
    FleetVar { name: "msgs_queued", doc: "agent-to-agent messages not yet delivered" },
    FleetVar { name: "workspaces", doc: "total workspaces (always rendered)" },
    FleetVar { name: "repos", doc: "total repos (always rendered)" },
];

pub fn fleet_var(name: &str) -> Option<&'static FleetVar> {
    FLEET_VARS.iter().find(|v| v.name == name)
}

/// Every fleet variable name, in table order — the `allowed` list a module
/// format validates against.
pub fn fleet_var_names() -> Vec<&'static str> {
    FLEET_VARS.iter().map(|v| v.name).collect()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib ui::bar::registry`
Expected: all pass, including the two new tests.

- [ ] **Step 5: Format and commit**

```bash
mise exec rust@1.95.0 -- cargo fmt --all
git add src/ui/bar/registry.rs
git commit -m "Bar registry: declare the fleet variables modules may reference

Claude-Session: https://claude.ai/code/session_01RYWyvE139mYRPJw3JCapzx"
```

---

### Task 2: `FleetStats` — derive fleet variables from `App`

**Files:**
- Create: `src/ui/bar/fleet.rs`
- Modify: `src/ui/bar/mod.rs:8-14` (add `pub mod fleet;`)
- Modify: `src/app/state.rs` (`App` struct ~line 522; constructor ~line 51)
- Modify: `src/app/status.rs:84` (add `live_workspace_count`)
- Modify: `src/app/run.rs:290-300` (use it)
- Modify: `src/app/messaging.rs:313` (record `msgs_queued`)

**Interfaces:**
- Consumes: `FLEET_VARS` (Task 1); `crate::ui::bar::providers::{var, vars}`; `App` maps `pushed_status`, `workspace_activity`, `workspace_needs_attention`, `pr_lifecycle`, `pr_review`, `pr_unresolved`, `workspace_status`; `App::classify_status`.
- Produces:
  - `pub struct FleetRow { reported: Option<ReportedState>, activity: Option<ActivityState>, alert: bool, live: bool, lifecycle: Option<BranchLifecycle>, review: Option<ReviewDecision>, unresolved: u32, dirty: bool }`
  - `pub struct FleetStats` with `pub fn from_rows(rows: impl IntoIterator<Item = FleetRow>, repos: u32, msgs_queued: u32) -> FleetStats`, `pub fn collect(app: &App) -> FleetStats`, `pub fn to_vars(&self) -> SegmentMap`
  - `pub fn empty() -> &'static SegmentMap` (all counts absent; `workspaces`/`repos` = `0`) for tests and preview paths.
  - `App.msgs_queued: u32`, `App::live_workspace_count(&self) -> u32`.

- [ ] **Step 1: Write the failing tests**

Create `src/ui/bar/fleet.rs` with only the test module for now:

```rust
//! Fleet-wide variables for `[module.<name>]` formats: derived once per
//! frame from `App`'s in-memory maps and exposed as a `SegmentMap` keyed
//! by `registry::FLEET_VARS` names. Pure over its inputs — no I/O.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::activity::ActivityState;
    use crate::data::store::ReportedState;
    use crate::git::forge::{BranchLifecycle, ReviewDecision};
    use crate::ui::bar::registry::FLEET_VARS;

    fn row() -> FleetRow {
        FleetRow {
            reported: None,
            activity: None,
            alert: false,
            live: false,
            lifecycle: None,
            review: None,
            unresolved: 0,
            dirty: false,
        }
    }

    fn text(vars: &SegmentMap, name: &str) -> String {
        vars.get(name)
            .map(|s| s.spans.iter().map(|sp| sp.content.as_ref()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn every_fleet_var_has_an_entry_and_zero_counts_render_empty() {
        let vars = FleetStats::from_rows(Vec::new(), 0, 0).to_vars();
        for v in FLEET_VARS {
            assert!(vars.contains_key(v.name), "missing {}", v.name);
        }
        assert_eq!(text(&vars, "working"), "");
        assert_eq!(text(&vars, "mergeable"), "");
        assert_eq!(text(&vars, "workspaces"), "0");
        assert_eq!(text(&vars, "repos"), "0");
    }

    #[test]
    fn counts_each_variable_from_rows() {
        let rows = vec![
            FleetRow {
                reported: Some(ReportedState::Working),
                activity: Some(ActivityState::Active),
                live: true,
                lifecycle: Some(BranchLifecycle::PrOpen),
                review: Some(ReviewDecision::Approved),
                dirty: true,
                ..row()
            },
            FleetRow {
                reported: Some(ReportedState::Blocked),
                activity: Some(ActivityState::AwaitingAnswer),
                alert: true,
                lifecycle: Some(BranchLifecycle::PrOpen),
                review: Some(ReviewDecision::ChangesRequested),
                unresolved: 3,
                ..row()
            },
            FleetRow {
                reported: Some(ReportedState::Busy),
                activity: Some(ActivityState::Stalled),
                lifecycle: Some(BranchLifecycle::PrMerged),
                review: Some(ReviewDecision::Approved),
                ..row()
            },
            FleetRow {
                activity: Some(ActivityState::Idle),
                lifecycle: Some(BranchLifecycle::NoPr),
                unresolved: 2,
                ..row()
            },
        ];
        let vars = FleetStats::from_rows(rows, 2, 5).to_vars();
        assert_eq!(text(&vars, "working"), "1");
        assert_eq!(text(&vars, "blocked"), "1");
        assert_eq!(text(&vars, "busy"), "1");
        assert_eq!(text(&vars, "unreported"), "1");
        assert_eq!(text(&vars, "waiting"), "");
        assert_eq!(text(&vars, "alerts"), "1");
        assert_eq!(text(&vars, "awaiting"), "1");
        assert_eq!(text(&vars, "stalled"), "1");
        assert_eq!(text(&vars, "active"), "1");
        assert_eq!(text(&vars, "idle"), "1");
        assert_eq!(text(&vars, "live_agents"), "1");
        assert_eq!(text(&vars, "pr_none"), "1");
        assert_eq!(text(&vars, "pr_open"), "2");
        assert_eq!(text(&vars, "pr_merged"), "1");
        assert_eq!(text(&vars, "pr_draft"), "");
        assert_eq!(text(&vars, "approved"), "2");
        assert_eq!(text(&vars, "changes_requested"), "1");
        assert_eq!(text(&vars, "unresolved"), "5");
        assert_eq!(text(&vars, "mergeable"), "1", "open+approved only; merged+approved excluded");
        assert_eq!(text(&vars, "dirty"), "1");
        assert_eq!(text(&vars, "msgs_queued"), "5");
        assert_eq!(text(&vars, "workspaces"), "4");
        assert_eq!(text(&vars, "repos"), "2");
    }

    #[test]
    fn empty_map_has_totals_only() {
        let vars = empty();
        assert_eq!(text(vars, "workspaces"), "0");
        assert_eq!(text(vars, "working"), "");
    }

    #[test]
    fn collect_reads_the_app_maps() {
        use crate::data::store::{NewWorkspace, Store};
        let store = Store::open_in_memory().unwrap();
        let repo = store.add_repo(std::path::Path::new("/tmp/r"), "r", "x").unwrap();
        let ws = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "a",
                branch: "x/a",
                worktree_path: std::path::Path::new("/tmp/r/a"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        let mut app = crate::app::App::new(store, std::path::PathBuf::from("/tmp/wsx-test")).unwrap();
        app.pr_lifecycle.insert(ws, BranchLifecycle::PrOpen);
        app.pr_review.insert(ws, ReviewDecision::Approved);
        app.workspace_needs_attention.insert(ws);
        app.msgs_queued = 2;
        let vars = FleetStats::collect(&app).to_vars();
        assert_eq!(text(&vars, "workspaces"), "1");
        assert_eq!(text(&vars, "repos"), "1");
        assert_eq!(text(&vars, "unreported"), "1");
        assert_eq!(text(&vars, "mergeable"), "1");
        assert_eq!(text(&vars, "alerts"), "1");
        assert_eq!(text(&vars, "msgs_queued"), "2");
        assert_eq!(text(&vars, "live_agents"), "", "no session → not live");
    }
}
```

Add `pub mod fleet;` to `src/ui/bar/mod.rs` next to the other `pub mod` lines. (`insert_workspace` returns the new `WorkspaceId`; confirm with `grep -n "pub fn insert_workspace" src/data/workspace.rs` and adapt if it returns a `Workspace`.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib ui::bar::fleet`
Expected: compile errors — `FleetRow`, `FleetStats`, `empty`, `App.msgs_queued` not found.

- [ ] **Step 3: Implement `fleet.rs`**

Above the test module in `src/ui/bar/fleet.rs`:

```rust
use crate::app::activity::ActivityState;
use crate::app::App;
use crate::data::store::ReportedState;
use crate::git::forge::{BranchLifecycle, ReviewDecision};
use crate::ui::bar::providers::var;
use crate::ui::bar::segment::SegmentMap;
use std::sync::OnceLock;

/// One workspace's contribution, already looked up from `App`'s maps so
/// `FleetStats::from_rows` stays pure and testable without a store.
#[derive(Debug, Clone, Default)]
pub struct FleetRow {
    pub reported: Option<ReportedState>,
    pub activity: Option<ActivityState>,
    /// In `App::workspace_needs_attention`.
    pub alert: bool,
    /// Primary session is `Thinking` or `Waiting` — the same predicate the
    /// usage sparkline buckets count.
    pub live: bool,
    pub lifecycle: Option<BranchLifecycle>,
    pub review: Option<ReviewDecision>,
    pub unresolved: u32,
    pub dirty: bool,
}

/// Fleet-wide counts, one field per `registry::FLEET_VARS` entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FleetStats {
    pub working: u32,
    pub waiting: u32,
    pub blocked: u32,
    pub done: u32,
    pub busy: u32,
    pub unreported: u32,
    pub alerts: u32,
    pub awaiting: u32,
    pub stalled: u32,
    pub active: u32,
    pub idle: u32,
    pub live_agents: u32,
    pub pr_none: u32,
    pub pr_draft: u32,
    pub pr_open: u32,
    pub pr_conflicted: u32,
    pub pr_merged: u32,
    pub pr_closed: u32,
    pub review_required: u32,
    pub changes_requested: u32,
    pub approved: u32,
    pub unresolved: u32,
    pub mergeable: u32,
    pub dirty: u32,
    pub msgs_queued: u32,
    pub workspaces: u32,
    pub repos: u32,
}

impl FleetStats {
    pub fn from_rows(rows: impl IntoIterator<Item = FleetRow>, repos: u32, msgs_queued: u32) -> Self {
        let mut s = FleetStats {
            repos,
            msgs_queued,
            ..Default::default()
        };
        for r in rows {
            s.workspaces += 1;
            match r.reported {
                Some(ReportedState::Working) => s.working += 1,
                Some(ReportedState::Waiting) => s.waiting += 1,
                Some(ReportedState::Blocked) => s.blocked += 1,
                Some(ReportedState::Done) => s.done += 1,
                Some(ReportedState::Busy) => s.busy += 1,
                None => s.unreported += 1,
            }
            if r.alert {
                s.alerts += 1;
            }
            match r.activity {
                Some(ActivityState::AwaitingAnswer) => s.awaiting += 1,
                Some(ActivityState::Stalled) => s.stalled += 1,
                Some(ActivityState::Active) => s.active += 1,
                Some(ActivityState::Idle) => s.idle += 1,
                _ => {}
            }
            if r.live {
                s.live_agents += 1;
            }
            match r.lifecycle {
                Some(BranchLifecycle::NoPr) => s.pr_none += 1,
                Some(BranchLifecycle::PrDraft) => s.pr_draft += 1,
                Some(BranchLifecycle::PrOpen) => s.pr_open += 1,
                Some(BranchLifecycle::PrConflicted) => s.pr_conflicted += 1,
                Some(BranchLifecycle::PrMerged) => s.pr_merged += 1,
                Some(BranchLifecycle::PrClosed) => s.pr_closed += 1,
                None => {}
            }
            match r.review {
                Some(ReviewDecision::ReviewRequired) => s.review_required += 1,
                Some(ReviewDecision::ChangesRequested) => s.changes_requested += 1,
                Some(ReviewDecision::Approved) => s.approved += 1,
                None => {}
            }
            s.unresolved += r.unresolved;
            if r.lifecycle == Some(BranchLifecycle::PrOpen)
                && r.review == Some(ReviewDecision::Approved)
            {
                s.mergeable += 1;
            }
            if r.dirty {
                s.dirty += 1;
            }
        }
        s
    }

    /// Walk every workspace the dashboard lists, once per frame.
    pub fn collect(app: &App) -> Self {
        let rows = app.workspaces.iter().map(|(_, ws)| {
            let status = app.classify_status(ws);
            FleetRow {
                reported: app.pushed_status.get(&ws.id).map(|r| r.state),
                activity: app.workspace_activity.get(&ws.id).copied(),
                alert: app.workspace_needs_attention.contains(&ws.id),
                live: matches!(
                    status,
                    crate::ui::dashboard::status::Status::Thinking
                        | crate::ui::dashboard::status::Status::Waiting
                ),
                lifecycle: app.pr_lifecycle.get(&ws.id).copied(),
                review: app.pr_review.get(&ws.id).copied(),
                unresolved: app.pr_unresolved.get(&ws.id).copied().unwrap_or(0),
                dirty: app
                    .workspace_status
                    .get(&ws.id)
                    .is_some_and(|g| g.modified + g.untracked > 0),
            }
        });
        Self::from_rows(rows, app.repos.len() as u32, app.msgs_queued)
    }

    /// The variable map a module format evaluates against. Counts are
    /// empty at zero so `( … )` groups drop; totals always render.
    pub fn to_vars(&self) -> SegmentMap {
        let count = |n: u32| if n == 0 { String::new() } else { n.to_string() };
        let entries: [(&str, String); 27] = [
            ("working", count(self.working)),
            ("waiting", count(self.waiting)),
            ("blocked", count(self.blocked)),
            ("done", count(self.done)),
            ("busy", count(self.busy)),
            ("unreported", count(self.unreported)),
            ("alerts", count(self.alerts)),
            ("awaiting", count(self.awaiting)),
            ("stalled", count(self.stalled)),
            ("active", count(self.active)),
            ("idle", count(self.idle)),
            ("live_agents", count(self.live_agents)),
            ("pr_none", count(self.pr_none)),
            ("pr_draft", count(self.pr_draft)),
            ("pr_open", count(self.pr_open)),
            ("pr_conflicted", count(self.pr_conflicted)),
            ("pr_merged", count(self.pr_merged)),
            ("pr_closed", count(self.pr_closed)),
            ("review_required", count(self.review_required)),
            ("changes_requested", count(self.changes_requested)),
            ("approved", count(self.approved)),
            ("unresolved", count(self.unresolved)),
            ("mergeable", count(self.mergeable)),
            ("dirty", count(self.dirty)),
            ("msgs_queued", count(self.msgs_queued)),
            ("workspaces", self.workspaces.to_string()),
            ("repos", self.repos.to_string()),
        ];
        entries
            .into_iter()
            .map(|(k, v)| (k.to_string(), var(v)))
            .collect()
    }
}

/// A fleet with nothing in it — for tests and preview renders that have no
/// `App`. Every count is absent; `workspaces`/`repos` read `0`.
pub fn empty() -> &'static SegmentMap {
    static EMPTY: OnceLock<SegmentMap> = OnceLock::new();
    EMPTY.get_or_init(|| FleetStats::default().to_vars())
}
```

`ReportedState`, `ActivityState`, `BranchLifecycle`, `ReviewDecision` must be `Copy + PartialEq`; check with `grep -n "derive" -A1` on each and add `Copy`/`PartialEq` if missing. If `ReportedStatus.state` is not `Copy`, use `.map(|r| r.state.clone())`.

- [ ] **Step 4: Add `App.msgs_queued` and `live_workspace_count`**

In `src/app/state.rs`, next to `pub activity_history` (~line 579), add:

```rust
    /// Undelivered agent-to-agent messages, refreshed each time the mail
    /// drain runs. Read by `ui::bar::fleet` for `$msgs_queued`.
    pub msgs_queued: u32,
```

and in `App::new` next to `activity_history: VecDeque::new(),` add `msgs_queued: 0,`.

In `src/app/status.rs`, after `classify_status`, add:

```rust
    /// Workspaces whose primary session is live — `Thinking` or `Waiting`.
    /// The usage sparkline buckets and `$live_agents` share this predicate.
    pub fn live_workspace_count(&self) -> u32 {
        self.workspaces
            .iter()
            .filter(|(_rid, ws)| {
                matches!(
                    self.classify_status(ws),
                    crate::ui::dashboard::status::Status::Thinking
                        | crate::ui::dashboard::status::Status::Waiting
                )
            })
            .count() as u32
    }
```

In `src/app/run.rs:291-300`, replace the inline `let live = g.workspaces.iter().filter(...).count() as u32;` with `let live = g.live_workspace_count();`.

In `src/app/messaging.rs`, directly after the `let pending = match self.store.undelivered_messages() { … };` block in `drain_agent_messages` (~line 313-320), add:

```rust
        self.msgs_queued = pending.len() as u32;
```

(This is before the `deliverable` filter, so in-flight messages still count as queued until delivered.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib ui::bar::fleet && cargo test --lib app::messaging && cargo test --lib app::run`
Expected: all pass.

- [ ] **Step 6: Format, clippy, commit**

```bash
mise exec rust@1.95.0 -- cargo fmt --all
cargo clippy --all-targets -- -D warnings
git add src/ui/bar/fleet.rs src/ui/bar/mod.rs src/app/state.rs src/app/status.rs src/app/run.rs src/app/messaging.rs
git commit -m "Fleet stats: derive the module variables from App state once per frame

Claude-Session: https://claude.ai/code/session_01RYWyvE139mYRPJw3JCapzx"
```

---

### Task 3: Theme loader — `[module.<name>]` tables

**Files:**
- Modify: `src/config/theme_file.rs` (`ThemeFile` ~22-43, `merge_over` ~122, `BarSpecs` ~141, `resolve_segment` ~266, `resolve` ~484, tests ~578+)

**Interfaces:**
- Consumes: `registry::{fleet_var_names, segment_def, SEGMENTS}`.
- Produces:
  - `pub struct ModuleTable { format, style, priority, disabled }` (all `Option`, `deny_unknown_fields`).
  - `ThemeFile.module: BTreeMap<String, ModuleTable>` — declared **before** the flattened `segments` field.
  - `BarSpecs.modules: Vec<String>` — module names in table order; their `SegmentConfig`s are stored in `BarSpecs.segments` under the same name (so `render_bar`'s priority lookup and `bars::cfg` work unchanged).

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `src/config/theme_file.rs`:

```rust
    #[test]
    fn module_table_resolves_into_segments_and_modules() {
        let specs = ok(
            "[module.pipe]\nformat = \"([$pr_open open](fg:ok) )($mergeable ready)\"\npriority = 40\n[dashboard_footer]\nright_format = \"$pipe\"\n",
        );
        assert_eq!(specs.modules, vec!["funnel".to_string(), "pipe".to_string()]);
        assert_eq!(specs.segments["pipe"].priority, 40);
        assert_eq!(
            specs.segments["pipe"].format,
            format::parse("([$pr_open open](fg:ok) )($mergeable ready)").unwrap()
        );
        assert_eq!(specs.dashboard_footer.right_format, format::parse("$pipe").unwrap());
    }

    #[test]
    fn module_format_may_only_use_fleet_vars() {
        let e = errs("[module.pipe]\nformat = \"$label $pr_open\"\n");
        assert_eq!(e.len(), 1, "{e:?}");
        assert_eq!(e[0].location, "[module.pipe].format");
        assert!(e[0].message.contains("unknown `$label`"), "{}", e[0].message);
        assert!(e[0].message.contains("pr_open"), "hint lists fleet vars: {}", e[0].message);
    }

    #[test]
    fn module_name_may_not_collide_with_a_segment() {
        let e = errs("[module.keys]\nformat = \"$working\"\n");
        assert_eq!(e.len(), 1, "{e:?}");
        assert_eq!(e[0].location, "[module.keys]");
        assert!(e[0].message.contains("built-in segment"), "{}", e[0].message);
    }

    #[test]
    fn module_table_rejects_segment_only_keys() {
        let e = ThemeFile::parse("[module.pipe]\nseparator = \"x\"\n").unwrap_err();
        assert!(e.message.contains("separator"), "{}", e.message);
        let e = ThemeFile::parse("[module.pipe]\nstyles = [\"x\"]\n").unwrap_err();
        assert!(e.message.contains("styles"), "{}", e.message);
    }

    #[test]
    fn module_is_placeable_in_every_bar() {
        let specs = ok(
            "[module.pipe]\nformat = \"$working\"\n[attached_bottom]\nright_format = \"$pipe\"\n[dashboard_header]\nright_format = \"$pipe\"\n[dashboard_detail]\nformat = \"$pipe\"\n[attached_top]\nformat = \"$pipe\"\n",
        );
        assert_eq!(specs.attached_bottom.right_format, format::parse("$pipe").unwrap());
        assert_eq!(specs.dashboard_detail.format, format::parse("$pipe").unwrap());
    }

    #[test]
    fn bar_referencing_an_undefined_module_is_an_error_listing_modules() {
        let e = errs("[dashboard_footer]\nright_format = \"$nope\"\n");
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].message.contains("unknown `$nope`"), "{}", e[0].message);
        assert!(e[0].message.contains("funnel"), "hint lists modules: {}", e[0].message);
    }

    #[test]
    fn user_module_table_merges_per_field_over_the_default() {
        let specs = ok("[module.funnel]\npriority = 7\n");
        assert_eq!(specs.segments["funnel"].priority, 7);
        assert!(
            !specs.segments["funnel"].format.is_empty(),
            "unset format keeps the bundled default's"
        );
    }

    #[test]
    fn module_style_forms_dollar_style() {
        let specs = ok("[module.pipe]\nformat = \"[$working]($style)\"\nstyle = \"fg:ok bold\"\n");
        assert!(specs.segments.contains_key("pipe"));
    }
```

These tests assume the bundled default defines `[module.funnel]` (Task 5). Until then `specs.modules` would be `["pipe"]` and `user_module_table_merges_per_field_over_the_default` / the `funnel` hint assertions fail — that is expected; Task 5 turns them green. To keep the commit green now, add a minimal placeholder table to `src/ui/bar/default_theme.toml` in this task:

```toml
# --- modules ----------------------------------------------------------
# A [module.<name>] table is a segment you compose yourself from fleet-wide
# variables (see the book's "Modules" section) and place in any bar as
# $<name>. Only `format`, `style`, `priority`, `disabled` are accepted.

[module.funnel]
format = "$working"
```

(Task 5 replaces this format with the real preset.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib config::theme_file`
Expected: compile error — `specs.modules` has no such field; parse of `[module.pipe]` currently routes into `segments` and errors "unknown segment".

- [ ] **Step 3: Add `ModuleTable` and the `module` field**

In `src/config/theme_file.rs`, add to `ThemeFile` **immediately before** the `#[serde(flatten)] pub segments` field:

```rust
    /// User-composed modules, `[module.<name>]`. Declared before the
    /// flattened `segments` map so serde routes the `module` table here
    /// rather than treating it as a segment named `module`.
    #[serde(default)]
    pub module: BTreeMap<String, ModuleTable>,
```

After `SegmentTable`, add:

```rust
/// A `[module.<name>]` table: a segment composed from fleet variables. No
/// items, so none of the multi-item keys (`separator`, `styles`,
/// `more_format`) and no `symbol`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleTable {
    pub format: Option<String>,
    pub style: Option<String>,
    pub priority: Option<u32>,
    pub disabled: Option<bool>,
}

impl ModuleTable {
    fn merge_over(self, base: ModuleTable) -> ModuleTable {
        ModuleTable {
            format: self.format.or(base.format),
            style: self.style.or(base.style),
            priority: self.priority.or(base.priority),
            disabled: self.disabled.or(base.disabled),
        }
    }
}
```

In `ThemeFile::merge_over`, after the `for (name, tbl) in base.segments` loop:

```rust
        for (name, tbl) in base.module {
            let mine = self.module.remove(&name).unwrap_or_default();
            self.module.insert(name, mine.merge_over(tbl));
        }
```

Add to `BarSpecs`:

```rust
    /// Names of every `[module.<name>]`, in table order. Each has its
    /// `SegmentConfig` in `segments` under the same name.
    pub modules: Vec<String>,
```

- [ ] **Step 4: Add `resolve_module` and wire `resolve`**

After `resolve_segment`, add:

```rust
/// Validate one `[module.<name>]` table: its `format` may reference only
/// `registry::FLEET_VARS` (plus `$style`), and its name may not shadow a
/// built-in segment. Returns the module's `SegmentConfig`.
fn resolve_module(
    name: &str,
    tbl: &ModuleTable,
    resolver: &Resolver,
    errors: &mut Vec<ThemeError>,
) -> Option<SegmentConfig> {
    if segment_def(name).is_some() {
        errors.push(error(
            format!("[module.{name}]"),
            "name collides with a built-in segment; pick another",
        ));
        return None;
    }
    let loc = format!("[module.{name}].format");
    let nodes = parse_format(&loc, tbl.format.as_deref().unwrap_or(""), errors);
    let allowed = crate::ui::bar::registry::fleet_var_names();
    let seg_resolver = resolver.with_styles(placeholder_styles(&["style"]));
    validate(&loc, &nodes, &allowed, &seg_resolver, errors);
    let style = styled(
        &format!("[module.{name}].style"),
        tbl.style.as_deref(),
        resolver,
        errors,
    );
    Some(SegmentConfig {
        style,
        symbol: None,
        format: nodes,
        disabled: tbl.disabled.unwrap_or(false),
        priority: tbl
            .priority
            .unwrap_or(crate::ui::bar::render::DEFAULT_PRIORITY),
        separator: Vec::new(),
        more_format: Vec::new(),
        styles: Vec::new(),
    })
}
```

Import `segment_def` (already imported at line 9 — confirm). In `resolve()`, after the `for (name, tbl) in &file.segments` loop and before `let segment_names`, add:

```rust
    let mut modules = Vec::new();
    for (name, tbl) in &file.module {
        if let Some(cfg) = resolve_module(name, tbl, &resolver, &mut errors) {
            segments.insert(name.clone(), cfg);
            modules.push(name.clone());
        }
    }

    // Bars may place any segment or any module.
    let mut allowed_names: Vec<&str> = SEGMENTS.iter().map(|d| d.name).collect();
    allowed_names.extend(modules.iter().map(String::as_str));
```

Replace `let segment_names: Vec<&str> = …;` with nothing (delete it) and pass `&allowed_names` to each of the five `resolve_bar` calls. Add `modules` to the `Ok(BarSpecs { … })` literal.

The `validate` hint (`allowed: …`) now lists modules after the segments automatically — that is what `bar_referencing_an_undefined_module_is_an_error_listing_modules` checks.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib config::theme_file`
Expected: all pass. `bundled_default_parses_and_validates` still passes (the placeholder `[module.funnel]` resolves).

- [ ] **Step 6: Check the reload path compiles and the theme CLI still validates**

Run: `cargo test --lib app::theme_reload && cargo run -q -- theme check 2>&1 | head -5` (if `theme check` is not a subcommand, run `cargo run -q -- theme 2>&1 | head -3` and confirm no panic).
Expected: tests pass; no error output.

- [ ] **Step 7: Format, clippy, commit**

```bash
mise exec rust@1.95.0 -- cargo fmt --all
cargo clippy --all-targets -- -D warnings
git add src/config/theme_file.rs src/ui/bar/default_theme.toml
git commit -m "Theme loader: accept [module.<name>] tables composed from fleet variables

Claude-Session: https://claude.ai/code/session_01RYWyvE139mYRPJw3JCapzx"
```

---

### Task 4: Render modules in every bar

**Files:**
- Modify: `src/ui/bar/providers.rs` (after `usage`, ~line 522)
- Modify: `src/ui/bar/bars.rs` (`put` ~24, every inputs struct and composer)
- Modify: `src/ui/bar/tests.rs` (every `AttachedInputs { … }`, `DashboardFooterInputs { … }`, `DashboardHeaderInputs { … }` literal; new tests)
- Modify: `src/ui/dashboard/mod.rs` (`DashboardInputs` ~44, `render` ~146, `render_list_area` header call ~257, `render_footer` ~329)
- Modify: `src/ui/dashboard/detail.rs` (`DetailInputs` ~22; call at ~186; test literals ~879, ~1175, ~1232)
- Modify: `src/ui/dashboard/tests.rs` (13 `DashboardInputs { … }` literals)
- Modify: `src/ui/attached/mod.rs` (`render_panes` ~118; its call at ~157; test helper ~349 and its `render_panes` calls ~452/515/568/741)
- Modify: `src/app/render/attached.rs` (`AttachedData` ~20-45; `render_panes` calls ~335/394)
- Modify: `src/app/render/dashboard.rs` (~71 `DashboardInputs`, ~152 `DetailInputs`, ~192 `render_footer`)

**Interfaces:**
- Consumes: `BarSpecs.modules` (Task 3), `fleet::{FleetStats, empty}` (Task 2), `SegmentMap`.
- Produces:
  - `providers::module(cfg: &SegmentConfig, fleet: &SegmentMap, resolver: &Resolver) -> Option<Segment>`
  - `bars::put_modules(segments: &mut SegmentMap, specs: &BarSpecs, fleet: &SegmentMap, resolver: &Resolver)`
  - New field `pub fleet: &'a SegmentMap` on `DashboardFooterInputs`, `DashboardHeaderInputs`, `AttachedInputs`, `DashboardInputs`, `DetailInputs`.
  - `bars::dashboard_detail(specs, theme, pinned, fleet: &SegmentMap, width)`.
  - `dashboard::render_footer(f, area, activity, theme, specs, window_label, workspace_selected, notice, fleet: &SegmentMap)`.
  - `attached::render_panes(…, active_agent, fleet: &SegmentMap, theme)`.

- [ ] **Step 1: Write the failing composer tests**

Append to `src/ui/bar/tests.rs` (top-level, after the existing modules):

```rust
#[cfg(test)]
mod module_tests {
    use super::*;
    use crate::config::theme_file::{ThemeFile, resolve};
    use crate::ui::bar::fleet::{FleetRow, FleetStats};
    use crate::ui::bar::segment::SegmentMap;
    use test_util::plain;

    fn specs_with(src: &str) -> crate::config::theme_file::BarSpecs {
        resolve(ThemeFile::parse(src).unwrap(), &Theme::wsx()).unwrap()
    }

    fn fleet(working: u32, mergeable: u32) -> SegmentMap {
        let rows = (0..working).map(|_| FleetRow {
            reported: Some(crate::data::store::ReportedState::Working),
            ..Default::default()
        });
        let ready = (0..mergeable).map(|_| FleetRow {
            lifecycle: Some(crate::git::forge::BranchLifecycle::PrOpen),
            review: Some(crate::git::forge::ReviewDecision::Approved),
            ..Default::default()
        });
        FleetStats::from_rows(rows.chain(ready), 1, 0).to_vars()
    }

    #[test]
    fn module_renders_in_the_dashboard_footer_and_drops_zero_items() {
        let specs = specs_with(
            "[module.pipe]\nformat = \"([$working wrk](fg:ok)  )([$mergeable rdy](fg:merged))\"\n[dashboard_footer]\nright_format = \"$pipe\"\n",
        );
        let theme = Theme::wsx();
        let out = dashboard_footer(
            &specs,
            &theme,
            &DashboardFooterInputs {
                activity: &[],
                version: "0.1.0",
                window_label: "24h",
                workspace_selected: false,
                fleet: &fleet(3, 0),
            },
            80,
        );
        let text = plain(&out.line);
        assert!(text.ends_with("3 wrk  "), "{text:?}");
        assert!(!text.contains("rdy"), "zero mergeable drops its group: {text:?}");
        assert!(out.hits.iter().all(|h| matches!(h.hit, Hit::Key(_))), "modules carry no hit");
    }

    #[test]
    fn module_renders_in_the_attached_bottom_bar() {
        let specs = specs_with(
            "[module.pipe]\nformat = \"$working working\"\n[attached_bottom]\nright_format = \"$pipe\"\n",
        );
        let theme = Theme::wsx();
        let f = fleet(2, 0);
        let bars = attached_bars(
            &specs,
            &theme,
            AttachedInputs {
                repo: "wsx",
                name: "foo",
                version: "0.1.0",
                window_label: "24h",
                activity: &[],
                agent: None,
                attention: None,
                pinned: &[],
                procs: 0,
                diff: None,
                pr: None,
                model_tokens: None,
                agents: &[],
                active_agent: None,
                fleet: &f,
            },
            80,
            80,
        );
        assert!(plain(&bars.bottom.line).ends_with("2 working"), "{:?}", plain(&bars.bottom.line));
    }

    #[test]
    fn module_priority_drops_before_keys_when_narrow() {
        let specs = specs_with(
            "[module.pipe]\nformat = \"$working working across the whole fleet right now\"\npriority = 10\n[dashboard_footer]\nformat = \"$keys\"\nright_format = \"$pipe\"\n",
        );
        let theme = Theme::wsx();
        let f = fleet(2, 0);
        let render = |w: u16| {
            plain(
                &dashboard_footer(
                    &specs,
                    &theme,
                    &DashboardFooterInputs {
                        activity: &[],
                        version: "0.1.0",
                        window_label: "24h",
                        workspace_selected: false,
                        fleet: &f,
                    },
                    w,
                )
                .line,
            )
        };
        assert!(render(160).contains("2 working"));
        let narrow = render(60);
        assert!(!narrow.contains("2 working"), "{narrow:?}");
        assert!(narrow.contains("nav"), "keys survive: {narrow:?}");
    }

    #[test]
    fn dashboard_detail_renders_a_module() {
        let specs = specs_with("[module.pipe]\nformat = \"$working w\"\n[dashboard_detail]\nformat = \"$pipe\"\n");
        let theme = Theme::wsx();
        let out = dashboard_detail(&specs, &theme, &[], &fleet(1, 0), 40);
        assert!(plain(&out.line).starts_with("1 w"), "{:?}", plain(&out.line));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib ui::bar::tests::module_tests`
Expected: compile error — no `fleet` field on the inputs structs; `dashboard_detail` takes 4 args.

- [ ] **Step 3: Add the provider and `put_modules`**

In `src/ui/bar/providers.rs`, after `usage`:

```rust
/// A `[module.<name>]`: the user's `format` evaluated against the fleet
/// variable map. No state colour of its own and no click target.
pub fn module(cfg: &SegmentConfig, fleet: &SegmentMap, resolver: &Resolver) -> Option<Segment> {
    eval_segment(cfg, fleet, Style::default(), &[], resolver)
}
```

In `src/ui/bar/bars.rs`, after `put`:

```rust
/// Insert every `[module.<name>]` from `specs.modules`, so `$<name>` works
/// in whichever bar the theme places it. Called by every composer.
pub(super) fn put_modules(
    segments: &mut SegmentMap,
    specs: &BarSpecs,
    fleet: &SegmentMap,
    resolver: &style::Resolver<'_>,
) {
    for name in &specs.modules {
        put(segments, name, providers::module(cfg(specs, name), fleet, resolver));
    }
}
```

- [ ] **Step 4: Thread `fleet` through the composers**

In `src/ui/bar/bars.rs`:

- `DashboardFooterInputs`: add `pub fleet: &'a SegmentMap,` with doc `/// Fleet variables for `[module.*]` segments — `fleet::FleetStats::to_vars()`.`
- `dashboard_footer`: before `render_bar`, add `put_modules(&mut segments, specs, inputs.fleet, &resolver);`
- `DashboardHeaderInputs`: add `pub fleet: &'a SegmentMap,`; in `dashboard_header` add `put_modules(&mut segments, specs, inputs.fleet, &resolver);` before `render_bar`.
- `dashboard_detail`: change signature to `(specs, theme, pinned, fleet: &SegmentMap, width)` and add `put_modules(&mut segments, specs, fleet, &resolver);` before `render_bar`.
- `AttachedInputs`: add `pub fleet: &'a SegmentMap,`; in `attached_segments` add `put_modules(&mut segments, specs, inputs.fleet, resolver);` before the final `segments`.

- [ ] **Step 5: Thread `fleet` through the UI and app layers**

`src/ui/dashboard/mod.rs`:
- `DashboardInputs`: add `pub fleet: &'a crate::ui::bar::segment::SegmentMap,` (doc: `/// Fleet variables for theme modules, built once per frame.`).
- `render_footer`: add a trailing parameter `fleet: &crate::ui::bar::segment::SegmentMap` and pass `fleet` in the `DashboardFooterInputs` literal.
- `render` (the with-footer wrapper ~146): pass `inputs.fleet` to `render_footer`.
- The `DashboardHeaderInputs` literal (~259): add `fleet: inputs.fleet,`.

`src/ui/dashboard/detail.rs`:
- `DetailInputs`: add `pub fleet: &'a crate::ui::bar::segment::SegmentMap,`.
- The `dashboard_detail(...)` call (~186): pass `inputs.fleet` as the 4th argument.
- Each `DetailInputs { … }` test literal (~879, ~1175, ~1232): add `fleet: crate::ui::bar::fleet::empty(),`.

`src/ui/dashboard/tests.rs`: every `DashboardInputs { … }` literal (13): add `fleet: crate::ui::bar::fleet::empty(),`.

`src/ui/attached/mod.rs`:
- `render_panes`: add parameter `fleet: &crate::ui::bar::segment::SegmentMap,` immediately before `theme: &Theme`; pass `fleet` in the `AttachedInputs` literal (~160).
- Test helper `attached_bars_top` (~340) and the `render_panes(` calls in tests (~452, ~515, ~568, ~741): pass `crate::ui::bar::fleet::empty()`.

`src/app/render/attached.rs`:
- `AttachedData`: add `fleet: crate::ui::bar::segment::SegmentMap,`; in `inputs()` add `fleet: &self.fleet,`.
- In `gather_local` and the remote gather (~157-172, ~214-228): set `fleet: crate::ui::bar::fleet::FleetStats::collect(app).to_vars(),`.
- The two `render_panes(` calls (~335, ~394): pass `&data.fleet` (whatever the local `AttachedData` binding is named) before `&app.theme`.

`src/app/render/dashboard.rs`:
- After `let (window, activity) = usage_sparkline(app);` (~69) add `let fleet = crate::ui::bar::fleet::FleetStats::collect(app).to_vars();`.
- `DashboardInputs` literal (~71): add `fleet: &fleet,`.
- `DetailInputs` literal (~152): add `fleet: &fleet,`.
- `render_footer` call (~192): pass `&fleet` as the last argument.

`src/ui/bar/tests.rs`: every existing `AttachedInputs { … }` (7), `DashboardFooterInputs { … }` (1), `DashboardHeaderInputs { … }` (2) literal: add `fleet: crate::ui::bar::fleet::empty(),`.

Then `cargo build --all-targets 2>&1 | grep -E "^error" -A5` and fix any site the list above missed — the compiler is authoritative.

- [ ] **Step 6: Run the full test suite**

Run: `cargo test`
Expected: all pass (the `default_footer_snapshot` test still passes because the placeholder `funnel` renders empty against `fleet::empty()` and `$usage` is still in the default `right_format`).

- [ ] **Step 7: Format, clippy, commit**

```bash
mise exec rust@1.95.0 -- cargo fmt --all
cargo clippy --all-targets -- -D warnings
git add -A src
git commit -m "Bars: render [module.*] segments from fleet variables in every bar

Claude-Session: https://claude.ai/code/session_01RYWyvE139mYRPJw3JCapzx"
```

---

### Task 5: The `funnel` preset and the new default footer

**Files:**
- Modify: `src/ui/bar/default_theme.toml` (`[dashboard_footer]` ~30-35; `[usage]` ~110-116; the `[module.funnel]` placeholder from Task 3)
- Modify: `src/ui/bar/tests.rs` (`default_footer_snapshot` ~284)
- Modify: `src/config/theme_file.rs` (`bundled_default_parses_and_validates` ~591)

**Interfaces:**
- Consumes: everything above.
- Produces: the shipped `[module.funnel]` table; `[dashboard_footer].right_format = "($version  )$funnel"`.

- [ ] **Step 1: Update the tests to the new default**

In `src/ui/bar/tests.rs`, change `footer()` to accept a fleet map and rewrite `default_footer_snapshot`:

```rust
    fn footer(selected: bool, label: &str, width: u16, fleet: &crate::ui::bar::segment::SegmentMap) -> Rendered {
        let theme = Theme::wsx();
        let specs = bundled_default(&theme);
        let activity: Vec<u32> = (0..24).collect();
        dashboard_footer(
            &specs,
            &theme,
            &DashboardFooterInputs {
                activity: &activity,
                version: "0.1.0",
                window_label: label,
                workspace_selected: selected,
                fleet,
            },
            width,
        )
    }

    #[test]
    fn default_footer_snapshot() {
        let out = footer(true, "24h", 120, crate::ui::bar::fleet::empty());
        let text = plain(&out.line);
        assert!(
            text.starts_with(
                " ↑↓  nav   ↵  open   n  new   G  group   o  order   /  filter   ?  actions   q  quit"
            ),
            "{text:?}"
        );
        assert!(text.trim_end().ends_with("0.1.0"), "empty fleet: version only: {text:?}");
        assert!(!text.contains('▁'), "the sparkline is no longer in the default footer: {text:?}");
        assert_eq!(out.line.width(), 120);
        assert_eq!(
            out.hits.iter().filter(|h| matches!(h.hit, Hit::Key(_))).count(),
            8
        );
    }

    #[test]
    fn default_footer_funnel_lists_nonzero_stages() {
        use crate::ui::bar::fleet::{FleetRow, FleetStats};
        let rows = vec![
            FleetRow { reported: Some(crate::data::store::ReportedState::Working), ..Default::default() },
            FleetRow { reported: Some(crate::data::store::ReportedState::Working), ..Default::default() },
            FleetRow { reported: Some(crate::data::store::ReportedState::Blocked), ..Default::default() },
            FleetRow {
                lifecycle: Some(crate::git::forge::BranchLifecycle::PrOpen),
                review: Some(crate::git::forge::ReviewDecision::Approved),
                ..Default::default()
            },
        ];
        let fleet = FleetStats::from_rows(rows, 1, 0).to_vars();
        let text = plain(&footer(false, "24h", 140, &fleet).line);
        assert!(
            text.ends_with("0.1.0  2 working  1 blocked  1 ready"),
            "{text:?}"
        );
    }

    #[test]
    fn usage_still_renders_when_a_theme_places_it() {
        let theme = Theme::wsx();
        let specs = crate::config::theme_file::resolve(
            crate::config::theme_file::ThemeFile::parse(
                "[dashboard_footer]\nright_format = \"$usage\"\n",
            )
            .unwrap(),
            &theme,
        )
        .unwrap();
        let activity: Vec<u32> = (0..24).collect();
        let out = dashboard_footer(
            &specs,
            &theme,
            &DashboardFooterInputs {
                activity: &activity,
                version: "0.1.0",
                window_label: "24h",
                workspace_selected: false,
                fleet: crate::ui::bar::fleet::empty(),
            },
            120,
        );
        let spark = crate::ui::dashboard::sparkline::render(&activity, 24);
        assert!(plain(&out.line).ends_with(&format!("24h {spark}")));
        assert!(out.hits.iter().any(|h| matches!(h.hit, Hit::UsageGraph)));
    }
```

Fix any other caller of the old 3-arg `footer(...)` helper in that file (grep `footer(` in `src/ui/bar/tests.rs`) by appending `, crate::ui::bar::fleet::empty()`.

In `src/config/theme_file.rs` `bundled_default_parses_and_validates`, add:

```rust
        assert_eq!(specs.modules, vec!["funnel".to_string()]);
        assert_eq!(specs.segments["funnel"].priority, 60);
        assert_eq!(
            specs.dashboard_footer.right_format,
            format::parse("($version  )$funnel").unwrap()
        );
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib ui::bar::tests && cargo test --lib config::theme_file::tests::bundled_default_parses_and_validates`
Expected: `default_footer_snapshot` fails (sparkline still present), `default_footer_funnel_lists_nonzero_stages` fails, the `bundled_default` assertions fail.

- [ ] **Step 3: Write the preset**

In `src/ui/bar/default_theme.toml`, change `[dashboard_footer]`:

```toml
[dashboard_footer]
format = "$keys"
# The trailing "  " is grouped with $version so it drops along with it (not
# left as an orphaned gap) when the terminal is too narrow for both.
right_format = "($version  )$funnel"
fill = " "
```

Update the `[version]` comment to say it drops "before the funnel module". Leave `[usage]` in place; update its comment:

```toml
# Usage sparkline. Variables: $label $spark
# Not placed by default any more — put `$usage` in a bar format to bring it
# back (it keeps its click, which opens the window picker).
[usage]
format = "[$label $spark](fg:path)"
priority = 60
```

Replace the Task 3 placeholder with the real preset and a fuller grammar note:

```toml
# --- modules ----------------------------------------------------------
# A [module.<name>] table is a segment you compose yourself from fleet-wide
# variables and place in any bar as $<name>. It takes `format`, `style`,
# `priority`, `disabled` only. Counts render empty at zero, so wrap each item
# in ( ) to drop it; `$workspaces` and `$repos` always render.
#
# Variables: $working $waiting $blocked $done $busy $unreported $alerts
#   $awaiting $stalled $active $idle $live_agents $pr_none $pr_draft
#   $pr_open $pr_conflicted $pr_merged $pr_closed $review_required
#   $changes_requested $approved $unresolved $mergeable $dirty $msgs_queued
#   $workspaces $repos

# The delivery funnel: how many workspaces sit at each stage of the
# pipeline, from working through review to mergeable. Each item carries
# its own trailing gap so a missing stage takes its gap with it; the last
# has none so the module hugs the right edge.
[module.funnel]
format   = "([$working working](fg:ok)  )([$blocked blocked](fg:err)  )([$review_required review](fg:waiting)  )([$changes_requested changes](fg:warn)  )([$mergeable ready](fg:merged))"
# Same slot as $usage had: $version drops first on a narrow footer.
priority = 60
```

- [ ] **Step 4: Run the full suite**

Run: `cargo test`
Expected: all pass. If `default_footer_funnel_lists_nonzero_stages` fails on spacing, print `text` and adjust the assertion to the exact rendering — the two-space item gap is the format's literal `  ` inside each group.

- [ ] **Step 5: Look at it**

Run: `cargo run -q` in a terminal with a few workspaces (or `wsx` if installed from this worktree) and confirm the footer shows the funnel and no sparkline; resize narrow and confirm `$version` drops first, then the funnel, and the key hints survive. Quit with `q`.

- [ ] **Step 6: Format, clippy, commit**

```bash
mise exec rust@1.95.0 -- cargo fmt --all
cargo clippy --all-targets -- -D warnings
git add src/ui/bar/default_theme.toml src/ui/bar/tests.rs src/config/theme_file.rs
git commit -m "Default theme: ship the funnel module in place of the usage sparkline

Claude-Session: https://claude.ai/code/session_01RYWyvE139mYRPJw3JCapzx"
```

---

### Task 6: Example themes place `$funnel`

**Files:**
- Modify: `docs/examples/theme-jellybeans.toml:41,72`, `theme-nord.toml:44,75`, `theme-nord0.toml:51,82`, `theme-orange.toml:52,101`, `theme-rose-pine.toml:42,73`, `theme-rose-pine-moon.toml:42,73`, `theme-starship.toml:29-32,88`

**Interfaces:** none new. Each file's `[dashboard_footer].right_format` currently ends in a powerline block `[ $usage ](bg:<accent> fg:<text>)`.

- [ ] **Step 1: Add a validation test for the examples (if none exists)**

Check: `grep -rn "docs/examples" src/config/theme_file.rs`. If a test already loads every example, skip to Step 2. Otherwise append to `mod tests` in `src/config/theme_file.rs`:

```rust
    #[test]
    fn every_example_theme_resolves() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/examples");
        let mut n = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "toml") {
                load(&path, &Theme::wsx()).unwrap_or_else(|e| panic!("{}: {e:?}", path.display()));
                n += 1;
            }
        }
        assert!(n >= 7, "expected the example themes, found {n}");
    }
```

Run `cargo test --lib config::theme_file::tests::every_example_theme_resolves` — expected PASS now (examples still reference `$usage`, which remains valid).

- [ ] **Step 2: Swap `$usage` for `$funnel` in each example**

For each of the 7 files, in `[dashboard_footer].right_format` replace the literal `$usage` with `$funnel` (keep the surrounding powerline caps and colours unchanged), e.g. jellybeans line 41 becomes:

```toml
right_format = "[](fg:mid)[( $version )](bg:mid fg:fg)[](fg:green bg:mid)[ $funnel ](bg:green fg:bg)"
```

Then replace each file's `[usage]` table with a `[module.funnel]` table that keeps the funnel's item styles on the example's own palette. Read each palette (`[palette]` at the top of the file) and map: `working` → the palette's green/ok, `blocked` → red, `review` → yellow/amber, `changes` → orange/peach, `ready` → purple/magenta (whatever the example already uses for `merged` in its `[pr]` table). Example for jellybeans (adjust names to what its `[palette]` defines):

```toml
# The funnel module on the jellybeans palette; the block's bg comes from the
# bar format, so items set only fg.
[module.funnel]
format   = "([$working working](fg:bg bold)  )([$blocked blocked](fg:red)  )([$review_required review](fg:yellow)  )([$changes_requested changes](fg:orange)  )([$mergeable ready](fg:purple))"
priority = 60
```

Do not restructure anything else in these files.

- [ ] **Step 3: Verify every example still resolves**

Run: `cargo test --lib config::theme_file::tests::every_example_theme_resolves`
Expected: PASS. A failure names the file and the unknown colour — fix the palette name.

- [ ] **Step 4: Commit**

```bash
git add docs/examples src/config/theme_file.rs
git commit -m "Example themes: place the funnel module where the usage graph was

Claude-Session: https://claude.ai/code/session_01RYWyvE139mYRPJw3JCapzx"
```

---

### Task 7: Book — `### Modules`

**Files:**
- Modify: `docs/book/src/configuration/themes.md` (insert a new `### Modules` section before `### A powerline example`, ~line 392; mention modules in `### Bars` where `$usage` is described)

**Interfaces:** documentation only. The variable table must match `FLEET_VARS` in `src/ui/bar/registry.rs` name-for-name.

- [ ] **Step 1: Write the section**

Insert before `### A powerline example`:

````markdown
### Modules

A module is a segment you compose yourself. Declare it as a
`[module.<name>]` table and place it in any bar as `$<name>`:

```toml
[module.funnel]
format   = "([$working working](fg:ok)  )([$blocked blocked](fg:err)  )([$mergeable ready](fg:merged))"
priority = 60

[dashboard_footer]
right_format = "($version  )$funnel"
```

A module table takes `format`, `style` (patched over the bar style to form
`$style`), `priority`, and `disabled` — nothing else, since a module has no
items. Its `format` may reference only the fleet variables below, which
describe every workspace on the dashboard at once. A count renders empty
when it is zero, so wrap each item in a `( … )` group to drop it along with
its label and gap; `$workspaces` and `$repos` always render a number.

A module's name may not be the name of a built-in segment (`keys`, `usage`,
`pr`, …). The bundled default defines one module, `funnel`, and places it
where the usage graph used to be; set only the fields you want to change to
restyle it, or define your own and put that in the bar instead. To bring
the sparkline back, place `$usage` again:

```toml
[dashboard_footer]
right_format = "($version  )$funnel  $usage"
```

Modules carry no click target.

| Variable | Counts |
|---|---|
| `working` `waiting` `blocked` `done` | workspaces whose last `wsx status set` is that state |
| `busy` | workspaces parked on background work (hook-inferred) |
| `unreported` | workspaces with no reported status |
| `alerts` | workspaces with an unacknowledged attention alert |
| `awaiting` `stalled` `active` `idle` | workspaces by live transcript classification |
| `live_agents` | workspaces with a live (thinking or waiting) primary session |
| `pr_none` `pr_draft` `pr_open` `pr_conflicted` `pr_merged` `pr_closed` | workspaces by PR lifecycle |
| `review_required` `changes_requested` `approved` | PRs by review verdict |
| `unresolved` | unresolved review threads across the fleet |
| `mergeable` | PRs that are open and approved |
| `dirty` | workspaces with modified or untracked files |
| `msgs_queued` | agent-to-agent messages not yet delivered |
| `workspaces` `repos` | totals (always rendered) |
````

In `### Bars`, where the dashboard footer's right side is described as `$version` + `$usage`, change it to say the default right side is `$version` and the `funnel` module, with a pointer to `### Modules`.

- [ ] **Step 2: Build the book**

Run: `mdbook build docs/book 2>&1 | tail -3` (skip if `mdbook` is not installed — note that in the commit message body instead).
Expected: no warnings about the changed page.

- [ ] **Step 3: Commit**

```bash
git add docs/book/src/configuration/themes.md
git commit -m "Book: document [module.<name>] tables and the fleet variables

Claude-Session: https://claude.ai/code/session_01RYWyvE139mYRPJw3JCapzx"
```

---

## Self-review

- **Spec coverage:** config shape + validation → Task 3; fleet variables → Tasks 1–2; provider/composers/per-frame collection → Task 4; default preset + `$usage` retained → Task 5; example themes → Task 6; book → Task 7; `msgs_queued` on the refresh path → Task 2 Step 4. Spec amendments made here: `attention` → `alerts` (segment-name collision), `funnel` priority `60` (parity with `$usage` so `$version` drops first), and `FleetRow`/`from_rows` as the pure core under `collect`. The spec is updated to match in the same commit as this plan.
- **Type consistency:** `fleet: &'a SegmentMap` everywhere; `dashboard_detail(specs, theme, pinned, fleet, width)`; `render_footer(…, notice, fleet)`; `render_panes(…, active_agent, fleet, theme)`; `BarSpecs.modules: Vec<String>`; `FleetStats::from_rows(rows, repos, msgs_queued)`.
- **Placeholders:** none; every code step has its code.
