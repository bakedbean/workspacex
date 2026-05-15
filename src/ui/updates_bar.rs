//! Content selection for the attached-view "other workspaces" status row.
//!
//! Pure module: takes pre-computed slices of App state, returns Option<UpdatesRow>.
//! The caller (typically `attached::render`) handles drawing.

use crate::events::WorkspaceEvents;
use crate::store::WorkspaceId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdatesRowKind {
    Attention,
    Activity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatesRow {
    pub glyph: char,
    pub kind: UpdatesRowKind,
    pub text: String,
}

/// Activity classification mirrors `app::ActivityState`. Kept here as a
/// re-export-friendly enum so updates_bar doesn't depend on app.rs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityState {
    Active,
    Idle,
    Waiting,
    Awaiting,
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
    /// `Some((tool_name, first_seen_ms))` when a tool_use has been pending
    /// for the App's stale threshold. Caller computes via
    /// `App::awaiting_permission`.
    pub awaiting_tool: Option<(String, i64)>,
}

const RECENT_EVENT_MS: i64 = 60_000;

pub fn select_row(
    attached_workspace: Option<WorkspaceId>,
    candidates: &[WorkspaceUpdateInfo],
    now_ms: i64,
) -> Option<UpdatesRow> {
    // Attention priority: among candidates with needs_attention == true,
    // excluding the attached workspace, pick the most recently active.
    let mut attention: Vec<&WorkspaceUpdateInfo> = candidates
        .iter()
        .filter(|c| c.needs_attention && Some(c.id) != attached_workspace)
        .collect();
    attention.sort_by_key(|c| {
        // Sort by most-recent first. Prefer awaiting_tool timestamp (when
        // pending) else latest event timestamp else 0.
        let ts = c
            .awaiting_tool
            .as_ref()
            .map(|(_, t)| *t)
            .or_else(|| c.events.and_then(|e| e.latest.as_ref().map(|s| s.timestamp_ms)))
            .unwrap_or(0);
        -ts
    });
    if let Some(c) = attention.first() {
        let (state_summary, age_anchor_ms) = match &c.awaiting_tool {
            Some((tool, ts)) => (format!("awaiting permission: {tool}"), *ts),
            None => {
                let ts = c
                    .events
                    .and_then(|e| e.latest.as_ref().map(|s| s.timestamp_ms))
                    .unwrap_or(now_ms);
                ("waiting".to_string(), ts)
            }
        };
        let age = format_age(now_ms.saturating_sub(age_anchor_ms));
        return Some(UpdatesRow {
            glyph: '⚠',
            kind: UpdatesRowKind::Attention,
            text: format!(
                "{}/{} {} ({})",
                c.repo_name, c.name, state_summary, age
            ),
        });
    }

    // Recent event fallback: among candidates (excluding attached) with a
    // latest event newer than RECENT_EVENT_MS, pick the most recent.
    let mut events: Vec<(&WorkspaceUpdateInfo, &crate::events::EventSnapshot)> = candidates
        .iter()
        .filter(|c| Some(c.id) != attached_workspace)
        .filter_map(|c| c.events?.latest.as_ref().map(|e| (c, e)))
        .filter(|(_, e)| now_ms.saturating_sub(e.timestamp_ms) <= RECENT_EVENT_MS)
        .collect();
    events.sort_by_key(|(_, e)| -e.timestamp_ms);
    if let Some((c, e)) = events.first() {
        let age = format_age(now_ms.saturating_sub(e.timestamp_ms));
        return Some(UpdatesRow {
            glyph: '●',
            kind: UpdatesRowKind::Activity,
            text: format!("{}/{}: {} ({})", c.repo_name, c.name, e.display, age),
        });
    }

    None
}

/// Format a millisecond delta as `<n>s` for <60s, `<n>m` for <60m, `<n>h` otherwise.
pub fn format_age(delta_ms: i64) -> String {
    let secs = (delta_ms / 1000).max(0);
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h", secs / 3600)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{EventKind, EventSnapshot, WorkspaceEvents};
    use crate::store::WorkspaceId;

    type WsOwned = (
        WorkspaceId,
        Option<WorkspaceEvents>,
        ActivityState,
        bool,
        Option<(String, i64)>,
        String, // name
        String, // repo_name
    );

    fn ws(
        id: i64,
        name: &str,
        events: Option<WorkspaceEvents>,
        activity: ActivityState,
        needs_attention: bool,
        awaiting: Option<(String, i64)>,
    ) -> WsOwned {
        (
            WorkspaceId(id),
            events,
            activity,
            needs_attention,
            awaiting,
            name.to_string(),
            "test-repo".to_string(),
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

    #[test]
    fn select_row_returns_none_when_no_other_activity_or_attention() {
        let row = select_row(None, &[], 0);
        assert!(row.is_none());
    }

    #[test]
    fn select_row_attention_wins_over_recent_event() {
        let attention = events_with_latest("attention-evt", 5_000);
        let recent = events_with_latest("recent-evt", 9_000);
        let candidates_owned = [
            ws(1, "blocked", Some(attention), ActivityState::Waiting, true, None),
            ws(2, "busy", Some(recent), ActivityState::Idle, false, None),
        ];
        let candidates: Vec<WorkspaceUpdateInfo> = candidates_owned
            .iter()
            .map(|(id, events, activity, needs_attention, awaiting, name, repo_name)| WorkspaceUpdateInfo {
                id: *id,
                name: name.as_str(),
                repo_name: repo_name.as_str(),
                events: events.as_ref(),
                activity: *activity,
                needs_attention: *needs_attention,
                awaiting_tool: awaiting.clone(),
            })
            .collect();
        let row = select_row(None, &candidates, 10_000).expect("row");
        assert_eq!(row.kind, UpdatesRowKind::Attention);
        assert_eq!(row.glyph, '⚠');
        assert!(
            row.text.contains("test-repo/blocked"),
            "expected repo/workspace prefix: {}",
            row.text
        );
        assert!(row.text.contains("waiting"), "{}", row.text);
    }

    #[test]
    fn select_row_falls_back_to_most_recent_event() {
        let older = events_with_latest("older-evt", 1_000);
        let newer = events_with_latest("newer-evt", 8_000);
        let candidates_owned = [
            ws(1, "older", Some(older), ActivityState::Idle, false, None),
            ws(2, "newer", Some(newer), ActivityState::Active, false, None),
        ];
        let candidates: Vec<WorkspaceUpdateInfo> = candidates_owned
            .iter()
            .map(|(id, events, activity, needs_attention, awaiting, name, repo_name)| WorkspaceUpdateInfo {
                id: *id,
                name: name.as_str(),
                repo_name: repo_name.as_str(),
                events: events.as_ref(),
                activity: *activity,
                needs_attention: *needs_attention,
                awaiting_tool: awaiting.clone(),
            })
            .collect();
        let row = select_row(None, &candidates, 10_000).expect("row");
        assert_eq!(row.kind, UpdatesRowKind::Activity);
        assert_eq!(row.glyph, '●');
        assert!(
            row.text.contains("test-repo/newer:"),
            "expected repo/workspace prefix: {}",
            row.text
        );
        assert!(row.text.contains("newer-evt"), "{}", row.text);
    }

    #[test]
    fn select_row_excludes_currently_attached() {
        let evt = events_with_latest("evt", 5_000);
        let candidates_owned = [ws(1, "self", Some(evt), ActivityState::Idle, false, None)];
        let candidates: Vec<WorkspaceUpdateInfo> = candidates_owned
            .iter()
            .map(|(id, events, activity, needs_attention, awaiting, name, repo_name)| WorkspaceUpdateInfo {
                id: *id,
                name: name.as_str(),
                repo_name: repo_name.as_str(),
                events: events.as_ref(),
                activity: *activity,
                needs_attention: *needs_attention,
                awaiting_tool: awaiting.clone(),
            })
            .collect();
        let row = select_row(Some(WorkspaceId(1)), &candidates, 10_000);
        assert!(row.is_none());
    }

    #[test]
    fn select_row_ignores_stale_events() {
        // event at t=0, now=120_000 ms → 120s old, > 60s threshold.
        let stale = events_with_latest("stale-evt", 0);
        let candidates_owned = [ws(1, "old", Some(stale), ActivityState::Idle, false, None)];
        let candidates: Vec<WorkspaceUpdateInfo> = candidates_owned
            .iter()
            .map(|(id, events, activity, needs_attention, awaiting, name, repo_name)| WorkspaceUpdateInfo {
                id: *id,
                name: name.as_str(),
                repo_name: repo_name.as_str(),
                events: events.as_ref(),
                activity: *activity,
                needs_attention: *needs_attention,
                awaiting_tool: awaiting.clone(),
            })
            .collect();
        let row = select_row(None, &candidates, 120_000);
        assert!(row.is_none());
    }

    #[test]
    fn select_row_awaiting_tool_renders_tool_name() {
        let candidates_owned = [ws(
            1,
            "ws",
            None,
            ActivityState::Awaiting,
            true,
            Some(("Bash".to_string(), 8_000)),
        )];
        let candidates: Vec<WorkspaceUpdateInfo> = candidates_owned
            .iter()
            .map(|(id, events, activity, needs_attention, awaiting, name, repo_name)| WorkspaceUpdateInfo {
                id: *id,
                name: name.as_str(),
                repo_name: repo_name.as_str(),
                events: events.as_ref(),
                activity: *activity,
                needs_attention: *needs_attention,
                awaiting_tool: awaiting.clone(),
            })
            .collect();
        let row = select_row(None, &candidates, 10_000).expect("row");
        assert!(
            row.text.contains("awaiting permission: Bash"),
            "{}",
            row.text
        );
        assert!(row.text.contains("(2s)"), "{}", row.text);
    }

    #[test]
    fn format_age_thresholds() {
        assert_eq!(format_age(0), "0s");
        assert_eq!(format_age(59_999), "59s");
        assert_eq!(format_age(60_000), "1m");
        assert_eq!(format_age(3_599_000), "59m");
        assert_eq!(format_age(3_600_000), "1h");
        assert_eq!(format_age(-500), "0s"); // negative delta clamps
    }
}
