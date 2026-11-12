//! Content selection for the attached-view top-bar workspace row.
//!
//! Pure module: takes pre-computed slices of App state and returns the
//! row's entries — every workspace but the attached one and those the
//! dashboard's hide rules keep out, most recently active first — as
//! [`AttentionItems`].
//! Laying them out is the bar engine's job: the `attention` provider in
//! `ui::bar::providers` renders each entry through the theme's
//! `[attention]` item format and fits them to the bar.

use crate::activity::events::WorkspaceEvents;
use crate::data::store::WorkspaceId;
use crate::git::forge::BranchLifecycle;
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
    /// `workspace_age_secs`; the row orders strictly by it.
    pub ago_secs: Option<u64>,
    /// The dashboard's canonical classification for this workspace.
    /// Caller computes via `App::classify_status`; deliberately NOT
    /// derived from the legacy bell `activity`, whose classifier lacks
    /// the dashboard's PTY-active question suppression and pushed
    /// `ReportedState` handling, so the two can disagree. Drives the
    /// entry's glyph and color, not its position.
    pub status: Status,
    /// Kept out of the row by the repo's hide rules. Caller computes via
    /// `App::hidden_from_top_bar`.
    pub hidden: bool,
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

/// Collect every workspace except the currently-attached and hidden ones,
/// as the attached view's top-bar row, most recently active first.
///
/// The order is strict recency by exact `ago_secs` — deliberately not the
/// dashboard's comparator. The dashboard buckets ages and sorts by name
/// inside a bucket to keep its long list still, and both it and the old
/// attention-first rule could park a days-old workspace ahead of one that
/// is working right now. Status shows through each entry's glyph and color
/// instead of its position. Never-active workspaces go last; the name
/// breaks ties so the order is total.
pub fn collect_workspace_row(
    candidates: &[WorkspaceUpdateInfo],
    attached_workspace: Option<WorkspaceId>,
    now_ms: i64,
) -> Vec<AttentionEntry> {
    let mut rows: Vec<&WorkspaceUpdateInfo> = candidates
        .iter()
        .filter(|c| Some(c.id) != attached_workspace && !c.hidden)
        .collect();
    rows.sort_by(|a, b| {
        let key = |c: &WorkspaceUpdateInfo| (c.ago_secs.is_none(), c.ago_secs);
        key(a).cmp(&key(b)).then_with(|| a.name.cmp(b.name))
    });
    rows.into_iter()
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

    /// A candidate with no events, PR or pending tool; tests override the
    /// fields they exercise with struct-update syntax.
    fn cand(
        id: i64,
        name: &'static str,
        status: Status,
        ago: Option<u64>,
    ) -> WorkspaceUpdateInfo<'static> {
        WorkspaceUpdateInfo {
            id: WorkspaceId(id),
            name,
            repo_name: "test-repo",
            events: None,
            activity: ActivityState::Idle,
            lifecycle: None,
            awaiting_tool: None,
            ago_secs: ago,
            status,
            hidden: false,
        }
    }

    fn events_with_latest(display: &str, timestamp_ms: i64) -> WorkspaceEvents {
        WorkspaceEvents {
            latest: Some(EventSnapshot {
                kind: EventKind::AssistantText,
                display: display.to_string(),
                timestamp_ms,
            }),
            ..Default::default()
        }
    }

    fn names(entries: &[AttentionEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn lists_idle_workspaces() {
        // The row is a workspace list, not an alert list: an idle
        // workspace still gets an entry.
        let candidates = [cand(1, "busy", Status::Idle, Some(5))];
        let entries = collect_workspace_row(&candidates, None, 10_000);
        assert_eq!(names(&entries), ["busy"]);
        assert_eq!(entries[0].status, Status::Idle);
    }

    #[test]
    fn orders_by_exact_recency_newest_first() {
        // Exact age, not the dashboard's buckets: 3m and 9m share the
        // dashboard's "under 10m" bucket, where the name would decide.
        let candidates = [
            cand(1, "aaa-nine-min", Status::Idle, Some(9 * 60)),
            cand(2, "zzz-three-min", Status::Idle, Some(3 * 60)),
            cand(3, "mmm-now", Status::Thinking, Some(1)),
        ];
        let entries = collect_workspace_row(&candidates, None, 10_000);
        assert_eq!(
            names(&entries),
            ["mmm-now", "zzz-three-min", "aaa-nine-min"]
        );
    }

    #[test]
    fn status_does_not_outrank_recency() {
        // Neither a question nor a stall jumps the queue: a workspace
        // blocked an hour ago sits behind one active a few seconds ago.
        let candidates = [
            cand(1, "question-1h", Status::Question, Some(3600)),
            cand(2, "stalled-10m", Status::Stalled, Some(600)),
            cand(3, "thinking-now", Status::Thinking, Some(2)),
        ];
        let entries = collect_workspace_row(&candidates, None, 10_000);
        assert_eq!(
            names(&entries),
            ["thinking-now", "stalled-10m", "question-1h"]
        );
    }

    #[test]
    fn ignores_event_timestamps_for_order() {
        // Order follows ago_secs (the dashboard's recency signal), not the
        // latest event timestamp, which here would give the opposite order.
        let older_evt = events_with_latest("stale-ws-newer-event", 9_000);
        let newer_evt = events_with_latest("fresh-ws-older-event", 1_000);
        let candidates = [
            WorkspaceUpdateInfo {
                events: Some(&older_evt),
                ..cand(1, "stale", Status::Question, Some(600))
            },
            WorkspaceUpdateInfo {
                events: Some(&newer_evt),
                ..cand(2, "fresh", Status::Question, Some(5))
            },
        ];
        let entries = collect_workspace_row(&candidates, None, 10_000);
        assert_eq!(names(&entries), ["fresh", "stale"]);
    }

    #[test]
    fn breaks_ties_by_name() {
        let candidates = [
            cand(1, "beta", Status::Idle, Some(60)),
            cand(2, "alpha", Status::Idle, Some(60)),
        ];
        let entries = collect_workspace_row(&candidates, None, 10_000);
        assert_eq!(names(&entries), ["alpha", "beta"]);
    }

    #[test]
    fn sorts_never_active_last() {
        let candidates = [
            cand(1, "never", Status::Question, None),
            cand(2, "old", Status::Idle, Some(9_999_999)),
        ];
        let entries = collect_workspace_row(&candidates, None, 10_000);
        assert_eq!(names(&entries), ["old", "never"]);
    }

    #[test]
    fn excludes_currently_attached() {
        let candidates = [cand(1, "self", Status::Waiting, Some(5))];
        let entries = collect_workspace_row(&candidates, Some(WorkspaceId(1)), 10_000);
        assert!(entries.is_empty());
    }

    #[test]
    fn excludes_hidden() {
        let candidates = [
            WorkspaceUpdateInfo {
                hidden: true,
                ..cand(1, "hidden", Status::Question, Some(1))
            },
            cand(2, "shown", Status::Idle, Some(500)),
        ];
        let entries = collect_workspace_row(&candidates, None, 10_000);
        assert_eq!(names(&entries), ["shown"]);
    }

    #[test]
    fn uses_awaiting_tool_timestamp_as_anchor() {
        // awaiting_tool's first-seen ts takes priority over latest event ts
        let evt = events_with_latest("old", 1_000);
        let candidates = [WorkspaceUpdateInfo {
            events: Some(&evt),
            awaiting_tool: Some(("Bash".to_string(), 8_000)),
            ..cand(1, "ws", Status::Question, Some(2))
        }];
        let entries = collect_workspace_row(&candidates, None, 10_000);
        assert_eq!(entries[0].age_anchor_ms, 8_000);
    }

    #[test]
    fn anchors_age_to_now_without_events() {
        let candidates = [cand(1, "ws", Status::Idle, None)];
        let entries = collect_workspace_row(&candidates, None, 10_000);
        assert_eq!(entries[0].age_anchor_ms, 10_000);
    }
}
