//! Fleet-wide variables for `[module.<name>]` formats: derived once per
//! frame from `App`'s in-memory maps and exposed as a `SegmentMap` keyed
//! by `registry::FLEET_VARS` names. Pure over its inputs — no I/O.

use crate::app::App;
use crate::app::activity::ActivityState;
use crate::data::store::ReportedState;
use crate::git::forge::{BranchLifecycle, ReviewDecision};
use crate::pty::session::AgentKind;
use crate::ui::bar::providers::var;
use crate::ui::bar::segment::SegmentMap;
use crate::ui::text::abbreviate_tokens;
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
    /// Latest reported prompt-side context size of every agent instance in
    /// the workspace (primary and peers) whose transcript is still cached,
    /// with its kind. "Latest reported", not "live": the cache outlives the
    /// session until the tail loop prunes it, and lags a roster change until
    /// the next tail — the same eventual consistency every events reader has.
    pub context_tokens: Vec<(AgentKind, u64)>,
}

/// Fleet-wide counts, one field per `registry::FLEET_VARS` entry other
/// than the label variables (`registry::fleet_label_names`), which are the
/// theme's.
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
    /// Σ latest `context_tokens` per agent kind, indexed by position in
    /// `AgentKind::ALL`.
    pub tokens_by_kind: [u64; AgentKind::ALL.len()],
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
            // Transcripts are external input; a malformed count must not
            // panic the collector, so saturate rather than overflow.
            for (kind, n) in r.context_tokens {
                let slot = &mut s.tokens_by_kind[kind_index(kind)];
                *slot = slot.saturating_add(n);
            }
        }
        s
    }

    /// Σ `tokens_by_kind` — the fleet-wide context fill.
    pub fn tokens(&self) -> u64 {
        self.tokens_by_kind
            .iter()
            .fold(0u64, |acc, n| acc.saturating_add(*n))
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
            context_tokens: context_tokens(app, ws),
        });
        Self::from_rows(rows, app.repos.len() as u32, app.msgs_queued)
    }

    /// The variable map a module format evaluates against. Counts are
    /// empty at zero so `( … )` groups drop; totals always render. Token
    /// sums render abbreviated (`77k`, `1.2M`) and, like counts, empty at
    /// zero so a kind with no live context drops out of the module.
    pub fn to_vars(&self) -> SegmentMap {
        let count = |n: u32| if n == 0 { String::new() } else { n.to_string() };
        let tokens = |n: u64| {
            if n == 0 {
                String::new()
            } else {
                abbreviate_tokens(n)
            }
        };
        let per_kind = AgentKind::ALL.iter().map(|kind| {
            (
                format!("tokens_{}", kind.display_name()),
                tokens(self.tokens_by_kind[kind_index(*kind)]),
            )
        });
        let entries: [(&str, String); 28] = [
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
            ("tokens_total", tokens(self.tokens())),
        ];
        entries
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .chain(per_kind)
            .map(|(k, v)| (k, var(v)))
            .collect()
    }
}

/// Position of `kind` in `AgentKind::ALL` — the `tokens_by_kind` slot.
fn kind_index(kind: AgentKind) -> usize {
    AgentKind::ALL
        .iter()
        .position(|k| *k == kind)
        .expect("AgentKind::ALL lists every variant")
}

/// The workspace's primary transcript (`workspace_events`, kind from the
/// workspace) plus every peer's (`agent_events`, kind from the roster).
fn context_tokens(app: &App, ws: &crate::data::store::Workspace) -> Vec<(AgentKind, u64)> {
    let primary = app
        .workspace_events
        .get(&ws.id)
        .and_then(|e| e.context_tokens)
        .map(|n| (ws.agent, n));
    let peers = app
        .agent_roster
        .get(&ws.id)
        .into_iter()
        .flatten()
        .filter(|i| !i.is_primary)
        .filter_map(|i| {
            app.agent_events
                .get(&i.id)
                .and_then(|e| e.context_tokens)
                .map(|n| (i.agent, n))
        });
    primary.into_iter().chain(peers).collect()
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
            context_tokens: Vec::new(),
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
        // The label variables are the theme's, not the fleet's:
        // `providers::module_vars` adds them from `[agent_bar.symbols]`.
        let labels: Vec<&str> = crate::ui::bar::registry::fleet_label_names().collect();
        for v in FLEET_VARS.iter().filter(|v| !labels.contains(&v.name)) {
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
    fn sums_context_tokens_per_agent_kind_and_in_total() {
        use crate::pty::session::AgentKind;
        let rows = vec![
            FleetRow {
                context_tokens: vec![(AgentKind::Claude, 1_000_000), (AgentKind::Codex, 300_000)],
                ..row()
            },
            FleetRow {
                context_tokens: vec![(AgentKind::Claude, 250_000)],
                ..row()
            },
            FleetRow {
                context_tokens: vec![(AgentKind::Codex, 40_000)],
                ..row()
            },
        ];
        let vars = FleetStats::from_rows(rows, 1, 0).to_vars();
        assert_eq!(text(&vars, "tokens_claude"), "1.2M");
        assert_eq!(text(&vars, "tokens_codex"), "340k");
        assert_eq!(text(&vars, "tokens_omp"), "", "no omp instance → empty");
        assert_eq!(text(&vars, "tokens_pi"), "");
        assert_eq!(text(&vars, "tokens_hermes"), "");
        assert_eq!(
            text(&vars, "tokens_total"),
            "1.6M",
            "fleet total across kinds"
        );
    }

    #[test]
    fn collect_sums_primary_and_peer_context_tokens_by_kind() {
        use crate::activity::events::WorkspaceEvents;
        use crate::data::store::{NewWorkspace, Store};
        use crate::pty::session::AgentKind;
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
                agent: AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        let peer = store.add_workspace_agent(ws, AgentKind::Codex).unwrap();
        let mut app =
            crate::app::App::new(store, std::path::PathBuf::from("/tmp/wsx-test")).unwrap();
        app.agent_roster = app.store.all_workspace_agents().unwrap();
        app.workspace_events.insert(
            ws,
            WorkspaceEvents {
                context_tokens: Some(77_000),
                ..Default::default()
            },
        );
        app.agent_events.insert(
            peer.id,
            WorkspaceEvents {
                context_tokens: Some(5_000),
                ..Default::default()
            },
        );
        let vars = FleetStats::collect(&app).to_vars();
        assert_eq!(
            text(&vars, "tokens_claude"),
            "77k",
            "primary, kind from the workspace"
        );
        assert_eq!(
            text(&vars, "tokens_codex"),
            "5k",
            "peer, kind from the roster"
        );
        assert_eq!(text(&vars, "tokens_total"), "82k");
    }

    #[test]
    fn token_sums_saturate_instead_of_overflowing() {
        use crate::pty::session::AgentKind;
        let rows = vec![
            FleetRow {
                context_tokens: vec![(AgentKind::Claude, u64::MAX), (AgentKind::Codex, 1)],
                ..row()
            },
            FleetRow {
                context_tokens: vec![(AgentKind::Claude, 1)],
                ..row()
            },
        ];
        let stats = FleetStats::from_rows(rows, 1, 0);
        assert_eq!(stats.tokens_by_kind[0], u64::MAX, "per-kind sum saturates");
        assert_eq!(stats.tokens(), u64::MAX, "total saturates");
    }

    /// Each retained instance counts exactly once: same-kind peers add up,
    /// a stray `agent_events` entry under the primary's own instance id is
    /// ignored (the primary is read from `workspace_events`), an entry for
    /// an instance no longer in the roster contributes nothing, and a
    /// transcript reset (`context_tokens: None`) drops that instance.
    #[test]
    fn collect_counts_each_rostered_instance_once() {
        use crate::activity::events::WorkspaceEvents;
        use crate::data::store::{NewWorkspace, Store};
        use crate::pty::session::AgentKind;
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
                agent: AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        let primary = store
            .add_primary_agent(ws, AgentKind::Claude, 1)
            .unwrap()
            .id;
        let peer_a = store.add_workspace_agent(ws, AgentKind::Codex).unwrap();
        let peer_b = store.add_workspace_agent(ws, AgentKind::Codex).unwrap();
        let orphan = store.add_workspace_agent(ws, AgentKind::Omp).unwrap();
        store.remove_workspace_agent(orphan.id).unwrap();
        let mut app =
            crate::app::App::new(store, std::path::PathBuf::from("/tmp/wsx-test")).unwrap();
        app.agent_roster = app.store.all_workspace_agents().unwrap();
        let evt = |n: Option<u64>| WorkspaceEvents {
            context_tokens: n,
            ..Default::default()
        };
        // Primary reset mid-session: nothing reported yet.
        app.workspace_events.insert(ws, evt(None));
        // Stray entry under the primary's instance id must not resurrect it.
        app.agent_events.insert(primary, evt(Some(1_000_000)));
        app.agent_events.insert(peer_a.id, evt(Some(5_000)));
        app.agent_events.insert(peer_b.id, evt(Some(6_000)));
        // Removed from the roster; its cached events are not yet pruned.
        app.agent_events.insert(orphan.id, evt(Some(9_000_000)));
        let vars = FleetStats::collect(&app).to_vars();
        assert_eq!(text(&vars, "tokens_claude"), "", "reset primary drops out");
        assert_eq!(
            text(&vars, "tokens_codex"),
            "11k",
            "both same-kind peers count"
        );
        assert_eq!(text(&vars, "tokens_omp"), "", "unrostered instance ignored");
        assert_eq!(text(&vars, "tokens_total"), "11k");
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
