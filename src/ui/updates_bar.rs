//! Content selection for the attached-view top-bar workspace row.
//!
//! Pure module: takes pre-computed slices of App state and returns the
//! row's entries — every workspace but the attached one, those needing
//! attention first, the rest in dashboard order — as [`AttentionItems`].
//! Laying them out is the bar engine's job: the `attention` provider in
//! `ui::bar::providers` renders each entry through the theme's
//! `[attention]` item format and fits them to the bar.

use crate::activity::events::WorkspaceEvents;
use crate::data::store::WorkspaceId;
use crate::git::forge::BranchLifecycle;
use crate::ui::dashboard::sort::{SortMode, SortRow, order_workspaces};
use crate::ui::dashboard::status::Status;

/// Activity classification mirrors `app::ActivityState`. Kept here as a
/// re-export-friendly enum so updates_bar doesn't depend on app.rs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityState {
    /// Agent paused waiting for the user to answer a question.
    AwaitingAnswer,
    /// Agent finished a task and is awaiting acknowledgment.
    Complete,
    Awaiting,
    Active,
    Idle,
    /// Claude has stalled mid-tool-chain.
    Stalled,
    Waiting,
    Off,
}

#[derive(Debug, Clone)]
pub struct WorkspaceUpdateInfo<'a> {
    pub id: WorkspaceId,
    pub name: &'a str,
    pub repo_name: &'a str,
    pub events: Option<&'a WorkspaceEvents>,
    pub activity: ActivityState,
    pub needs_attention: bool,
    /// Cached PR lifecycle for this workspace's branch, mirroring the
    /// dashboard's `app.pr_lifecycle`. Drives the attention entry's
    /// name color so the top bar matches the dashboard's PR hues.
    pub lifecycle: Option<BranchLifecycle>,
    /// `Some((tool_name, first_seen_ms))` when a tool_use has been pending
    /// for the App's stale threshold. Caller computes via
    /// `App::awaiting_permission`.
    pub awaiting_tool: Option<(String, i64)>,
    /// The dashboard's recency signal for this workspace (seconds since
    /// last interaction, `None` = never active). Caller computes via
    /// `workspace_age_secs` so the attention line orders entries exactly
    /// like the dashboard's NEEDS ATTENTION section.
    pub ago_secs: Option<u64>,
    /// The dashboard's canonical classification for this workspace.
    /// Caller computes via `App::classify_status`; deliberately NOT
    /// derived from the legacy bell `activity`, whose classifier lacks
    /// the dashboard's PTY-active question suppression and pushed
    /// `ReportedState` handling, so the two can disagree. Drives the
    /// sort so ordering matches the dashboard rows.
    pub status: Status,
}

/// One workspace that the user should pay attention to. Carries
/// pre-computed display fields so the renderer doesn't need access to live
/// App state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttentionEntry {
    pub workspace_id: WorkspaceId,
    pub repo_name: String,
    pub name: String,
    /// Anchor epoch-ms for the "(5m)" age display. The most recent of:
    /// pending tool_use timestamp, latest event timestamp, or `now`.
    pub age_anchor_ms: i64,
    /// The workspace's dashboard status. Drives the row glyph and its
    /// color, so an entry reads exactly like its dashboard row.
    pub status: Status,
    /// PR lifecycle for this workspace, used to color the `repo/name`
    /// text with the same hues the dashboard uses (green=open,
    /// purple=merged, …). `None` (or a colorless lifecycle like NoPr)
    /// falls back to the muted `path` color.
    pub lifecycle: Option<BranchLifecycle>,
}

/// The attention entries as the bar engine takes them: every candidate,
/// unfitted, with the clock to age them by and the width the theme's
/// `[attention]` items may occupy. The `attention` provider in
/// `ui::bar::providers` renders each entry through the theme's item
/// format, folds what doesn't fit into its `more_format` tail, and records
/// one click hit per rendered entry plus one over the tail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttentionItems {
    pub entries: Vec<AttentionEntry>,
    pub now_ms: i64,
    /// Columns the rendered items may occupy — see
    /// `ui::bar::attention_width_budget`.
    pub max_width: usize,
}

/// One-char glyph for the inline status row. Mirrors the dashboard's
/// attn-marker vocabulary so users see the same icons in both surfaces.
pub fn glyph_for_activity(a: ActivityState) -> char {
    match a {
        ActivityState::AwaitingAnswer => '?',
        ActivityState::Complete => '\u{2713}', // ✓ CHECK MARK
        ActivityState::Awaiting | ActivityState::Stalled => '⚠',
        // Defensive default for the non-alertable states.
        _ => '⚠',
    }
}

impl SortRow for WorkspaceUpdateInfo<'_> {
    fn sort_status(&self) -> Status {
        self.status
    }
    fn sort_ago_secs(&self) -> Option<u64> {
        self.ago_secs
    }
    fn sort_name(&self) -> &str {
        self.name
    }
}

/// Collect every workspace except the currently-attached one, as the
/// attached view's top-bar row. Workspaces whose `needs_attention` flag is
/// set are promoted ahead of the rest; inside each group the dashboard's
/// own comparator decides under the dashboard's current sort mode, so the
/// row reads the same way the by-repo list does. Callers pass
/// `DashboardState::sort_mode` and `blocked_pin_max_age_secs`.
pub fn collect_workspace_row(
    candidates: &[WorkspaceUpdateInfo],
    attached_workspace: Option<WorkspaceId>,
    now_ms: i64,
    sort_mode: SortMode,
    pin_max_age_secs: u64,
) -> Vec<AttentionEntry> {
    let (mut flagged, mut rest): (Vec<&WorkspaceUpdateInfo>, Vec<&WorkspaceUpdateInfo>) =
        candidates
            .iter()
            .filter(|c| Some(c.id) != attached_workspace)
            .partition(|c| c.needs_attention);
    order_workspaces(&mut flagged, sort_mode, pin_max_age_secs);
    order_workspaces(&mut rest, sort_mode, pin_max_age_secs);
    flagged
        .into_iter()
        .chain(rest)
        .map(|c| {
            let age_anchor_ms = c
                .awaiting_tool
                .as_ref()
                .map(|(_, t)| *t)
                .or_else(|| {
                    c.events
                        .and_then(|e| e.latest.as_ref().map(|s| s.timestamp_ms))
                })
                .unwrap_or(now_ms);
            AttentionEntry {
                workspace_id: c.id,
                repo_name: c.repo_name.to_string(),
                name: c.name.to_string(),
                age_anchor_ms,
                status: c.status,
                lifecycle: c.lifecycle,
            }
        })
        .collect()
}

// Moved to `crate::util::time` so non-TUI callers (the macOS menubar) can use it
// without depending on ratatui widget code. Re-exported here because the
// updates bar, the updates panel, and the PM digest all reach it by this path.
pub use crate::util::time::format_age;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::events::{EventKind, EventSnapshot, WorkspaceEvents};
    use crate::data::store::WorkspaceId;
    use crate::ui::dashboard::sort::BLOCKED_PIN_MAX_AGE_DEFAULT_SECS as PIN;

    /// Test fixture: derive a canonical status from the legacy activity.
    fn status_for_activity(a: ActivityState) -> Status {
        match a {
            ActivityState::AwaitingAnswer => Status::Question,
            ActivityState::Stalled => Status::Stalled,
            ActivityState::Awaiting => Status::Question,
            ActivityState::Complete => Status::Complete,
            ActivityState::Active => Status::Thinking,
            ActivityState::Waiting => Status::Waiting,
            ActivityState::Idle | ActivityState::Off => Status::Idle,
        }
    }

    type WsOwned = (
        WorkspaceId,
        Option<WorkspaceEvents>,
        ActivityState,
        bool,
        Option<(String, i64)>,
        String,      // name
        String,      // repo_name
        Option<u64>, // ago_secs
        Status,      // canonical dashboard status
    );

    fn ws(
        id: i64,
        name: &str,
        events: Option<WorkspaceEvents>,
        activity: ActivityState,
        needs_attention: bool,
        awaiting: Option<(String, i64)>,
    ) -> WsOwned {
        ws_ago(id, name, events, activity, needs_attention, awaiting, None)
    }

    #[allow(clippy::too_many_arguments)]
    fn ws_ago(
        id: i64,
        name: &str,
        events: Option<WorkspaceEvents>,
        activity: ActivityState,
        needs_attention: bool,
        awaiting: Option<(String, i64)>,
        ago_secs: Option<u64>,
    ) -> WsOwned {
        (
            WorkspaceId(id),
            events,
            activity,
            needs_attention,
            awaiting,
            name.to_string(),
            "test-repo".to_string(),
            ago_secs,
            // Tests that need the canonical status to diverge from the
            // legacy activity construct WorkspaceUpdateInfo directly.
            status_for_activity(activity),
        )
    }

    fn snap(display: &str, timestamp_ms: i64) -> EventSnapshot {
        EventSnapshot {
            kind: EventKind::AssistantText,
            display: display.to_string(),
            timestamp_ms,
        }
    }

    fn events_with_latest(display: &str, timestamp_ms: i64) -> WorkspaceEvents {
        WorkspaceEvents {
            latest: Some(snap(display, timestamp_ms)),
            ..Default::default()
        }
    }

    fn to_candidates(rows: &[WsOwned]) -> Vec<WorkspaceUpdateInfo<'_>> {
        rows.iter()
            .map(
                |(
                    id,
                    events,
                    activity,
                    needs_attention,
                    awaiting,
                    name,
                    repo_name,
                    ago_secs,
                    status,
                )| {
                    WorkspaceUpdateInfo {
                        id: *id,
                        name: name.as_str(),
                        repo_name: repo_name.as_str(),
                        events: events.as_ref(),
                        activity: *activity,
                        needs_attention: *needs_attention,
                        lifecycle: None,
                        awaiting_tool: awaiting.clone(),
                        ago_secs: *ago_secs,
                        status: *status,
                    }
                },
            )
            .collect()
    }

    #[test]
    fn collect_workspace_row_lists_unflagged_workspaces() {
        // The row is a workspace list, not an alert list: a workspace with
        // no attention flag still gets an entry.
        let evt = events_with_latest("recent", 5_000);
        let rows = [ws(1, "busy", Some(evt), ActivityState::Idle, false, None)];
        let candidates = to_candidates(&rows);
        let entries = collect_workspace_row(&candidates, None, 10_000, SortMode::Recency, PIN);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "busy");
        assert_eq!(entries[0].status, Status::Idle);
    }

    #[test]
    fn collect_workspace_row_promotes_flagged_before_dashboard_order() {
        // Flagged workspaces come first regardless of the dashboard
        // comparator; inside each group the comparator decides (recency
        // mode here, so the fresher unflagged row precedes the older one).
        let rows = [
            ws_ago(
                1,
                "older",
                None,
                ActivityState::Idle,
                false,
                None,
                Some(600),
            ),
            ws_ago(2, "fresh", None, ActivityState::Idle, false, None, Some(5)),
            ws_ago(
                3,
                "flagged-stale",
                None,
                ActivityState::Complete,
                true,
                None,
                Some(9_000),
            ),
        ];
        let candidates = to_candidates(&rows);
        let entries = collect_workspace_row(&candidates, None, 10_000, SortMode::Recency, PIN);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["flagged-stale", "fresh", "older"]);
    }

    #[test]
    fn collect_workspace_row_sorts_most_recently_interacted_first() {
        // Same status priority — order falls to the dashboard's ago_secs
        // recency signal (smaller = more recent = first), NOT the event
        // timestamps (which here would give the opposite order).
        let older_evt = events_with_latest("stale-ws-newer-event", 9_000);
        let newer_evt = events_with_latest("fresh-ws-older-event", 1_000);
        let rows = [
            ws_ago(
                1,
                "stale",
                Some(older_evt),
                ActivityState::Awaiting,
                true,
                None,
                Some(600),
            ),
            ws_ago(
                2,
                "fresh",
                Some(newer_evt),
                ActivityState::Awaiting,
                true,
                None,
                Some(5),
            ),
        ];
        let candidates = to_candidates(&rows);
        let entries = collect_workspace_row(&candidates, None, 10_000, SortMode::Recency, PIN);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "fresh");
        assert_eq!(entries[1].name, "stale");
    }

    #[test]
    fn collect_workspace_row_status_mode_sorts_priority_before_recency() {
        // The dashboard's `sort: status` mode: Stalled (5) outranks
        // AwaitingAnswer/Question (4) outranks Waiting (3), even when
        // lower-priority entries are more recent.
        let rows = [
            ws_ago(
                1,
                "waiting",
                None,
                ActivityState::Waiting,
                true,
                None,
                Some(1),
            ),
            ws_ago(
                2,
                "question",
                None,
                ActivityState::AwaitingAnswer,
                true,
                None,
                Some(2),
            ),
            ws_ago(
                3,
                "stalled",
                None,
                ActivityState::Stalled,
                true,
                None,
                Some(300),
            ),
        ];
        let candidates = to_candidates(&rows);
        let entries = collect_workspace_row(&candidates, None, 10_000, SortMode::Status, PIN);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["stalled", "question", "waiting"]);
    }

    #[test]
    fn collect_workspace_row_recency_mode_orders_like_the_dashboard() {
        // The dashboard's default `sort: recency` mode: a freshly blocked
        // row is pinned on top, everything else sits in its recency
        // bucket, and a block older than the pin window sorts on age like
        // any other row. Status priority must NOT float a days-old
        // stalled workspace over rows that were active a minute ago.
        const DAY: u64 = 24 * 60 * 60;
        let rows = [
            ws_ago(
                1,
                "stale-stalled",
                None,
                ActivityState::Stalled,
                true,
                None,
                Some(3 * DAY),
            ),
            ws_ago(
                2,
                "fresh-waiting",
                None,
                ActivityState::Waiting,
                true,
                None,
                Some(30),
            ),
            ws_ago(
                3,
                "fresh-question",
                None,
                ActivityState::AwaitingAnswer,
                true,
                None,
                Some(60),
            ),
        ];
        let candidates = to_candidates(&rows);
        let entries = collect_workspace_row(&candidates, None, 10_000, SortMode::Recency, PIN);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["fresh-question", "fresh-waiting", "stale-stalled"]);
    }

    #[test]
    fn collect_workspace_row_sorts_by_canonical_status_not_legacy_activity() {
        // Both carry the legacy bell activity AwaitingAnswer (→ Question),
        // but the dashboard's canonical classifier downgraded ws1 to
        // Waiting (e.g. PTY-active question suppression). The sort must
        // follow the canonical status, so ws2 outranks ws1 despite ws1
        // being far more recent.
        let mk = |id: i64, name: &'static str, status: Status, ago: u64| WorkspaceUpdateInfo {
            id: WorkspaceId(id),
            name,
            repo_name: "test-repo",
            events: None,
            activity: ActivityState::AwaitingAnswer,
            needs_attention: true,
            lifecycle: None,
            awaiting_tool: None,
            ago_secs: Some(ago),
            status,
        };
        let candidates = vec![
            mk(1, "suppressed", Status::Waiting, 1),
            mk(2, "question", Status::Question, 500),
        ];
        let entries = collect_workspace_row(&candidates, None, 10_000, SortMode::Recency, PIN);
        assert_eq!(entries[0].name, "question");
        assert_eq!(entries[1].name, "suppressed");
    }

    #[test]
    fn collect_workspace_row_sorts_never_active_last() {
        let rows = [
            ws_ago(1, "never", None, ActivityState::Awaiting, true, None, None),
            ws_ago(
                2,
                "old",
                None,
                ActivityState::Awaiting,
                true,
                None,
                Some(9_999),
            ),
        ];
        let candidates = to_candidates(&rows);
        let entries = collect_workspace_row(&candidates, None, 10_000, SortMode::Recency, PIN);
        assert_eq!(entries[0].name, "old");
        assert_eq!(entries[1].name, "never");
    }

    #[test]
    fn collect_workspace_row_excludes_currently_attached() {
        let evt = events_with_latest("evt", 5_000);
        let rows = [ws(1, "self", Some(evt), ActivityState::Waiting, true, None)];
        let candidates = to_candidates(&rows);
        let entries = collect_workspace_row(
            &candidates,
            Some(WorkspaceId(1)),
            10_000,
            SortMode::Recency,
            PIN,
        );
        assert!(entries.is_empty());
    }

    #[test]
    fn collect_workspace_row_uses_awaiting_tool_timestamp_as_anchor() {
        // awaiting_tool's first-seen ts takes priority over latest event ts
        let evt = events_with_latest("old", 1_000);
        let rows = [ws(
            1,
            "ws",
            Some(evt),
            ActivityState::Awaiting,
            true,
            Some(("Bash".to_string(), 8_000)),
        )];
        let candidates = to_candidates(&rows);
        let entries = collect_workspace_row(&candidates, None, 10_000, SortMode::Recency, PIN);
        assert_eq!(entries[0].age_anchor_ms, 8_000);
    }
}
