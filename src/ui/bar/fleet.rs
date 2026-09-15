//! Fleet-wide variables for `[module.<name>]` formats: derived once per
//! frame from `App`'s in-memory maps and exposed as a `SegmentMap` keyed
//! by `registry::FLEET_VARS` names. Pure over its inputs — no I/O.

use crate::app::App;
use crate::app::activity::ActivityState;
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
    pub fn from_rows(
        rows: impl IntoIterator<Item = FleetRow>,
        repos: u32,
        msgs_queued: u32,
    ) -> Self {
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
        let rows = app.workspaces.iter().map(|(_, ws)| FleetRow {
            reported: app.fresh_reported_status(ws.id).map(|r| r.state),
            activity: app.workspace_activity.get(&ws.id).copied(),
            alert: app.workspace_needs_attention.contains(&ws.id),
            live: app.is_live(ws),
            lifecycle: app.pr_lifecycle.get(&ws.id).copied(),
            review: app.pr_review.get(&ws.id).copied(),
            unresolved: app.pr_unresolved.get(&ws.id).copied().unwrap_or(0),
            dirty: app
                .workspace_status
                .get(&ws.id)
                .is_some_and(|g| g.modified + g.untracked > 0),
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
            FleetRow {
                reported: Some(ReportedState::Done),
                lifecycle: Some(BranchLifecycle::PrConflicted),
                review: Some(ReviewDecision::ReviewRequired),
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
        assert_eq!(
            text(&vars, "mergeable"),
            "1",
            "open+approved only; merged+approved excluded"
        );
        assert_eq!(text(&vars, "dirty"), "1");
        assert_eq!(text(&vars, "msgs_queued"), "5");
        assert_eq!(text(&vars, "workspaces"), "5");
        assert_eq!(text(&vars, "repos"), "2");
        assert_eq!(text(&vars, "done"), "1");
        assert_eq!(text(&vars, "pr_conflicted"), "1");
        assert_eq!(text(&vars, "review_required"), "1");
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
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "x")
            .unwrap();
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
        let mut app =
            crate::app::App::new(store, std::path::PathBuf::from("/tmp/wsx-test")).unwrap();
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

    #[test]
    fn collect_ignores_a_stale_reported_status() {
        use crate::activity::events::WorkspaceEvents;
        use crate::data::store::{NewWorkspace, ReportedStatus, Store};
        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "x")
            .unwrap();
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
        let mut app =
            crate::app::App::new(store, std::path::PathBuf::from("/tmp/wsx-test")).unwrap();
        // Pushed status is old (reported_at = 0); the transcript has grown
        // past it (last_log_activity_ms = 1000), so `fresh_reported_status`
        // drops it — unlike `Busy`, `Working` is gated on freshness.
        app.pushed_status.insert(
            ws,
            ReportedStatus {
                state: ReportedState::Working,
                message: None,
                source: "test".into(),
                reported_at: 0,
            },
        );
        app.workspace_events.insert(
            ws,
            WorkspaceEvents {
                last_log_activity_ms: 1000,
                ..Default::default()
            },
        );
        let vars = FleetStats::collect(&app).to_vars();
        assert_eq!(text(&vars, "working"), "");
        assert_eq!(text(&vars, "unreported"), "1");
    }
}
