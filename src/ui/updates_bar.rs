//! Content selection for the attached-view "other workspaces" status row.
//!
//! Pure module: takes pre-computed slices of App state, returns an inline
//! list of attention-needing workspaces. The caller (typically
//! `attached::render`) handles drawing. The activity-fallback path that
//! previously surfaced "most recent event" was removed — issue #18 makes
//! the status row exclusively about workspaces that need user action.

use crate::events::WorkspaceEvents;
use crate::store::WorkspaceId;

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
    /// `Some((tool_name, first_seen_ms))` when a tool_use has been pending
    /// for the App's stale threshold. Caller computes via
    /// `App::awaiting_permission`.
    pub awaiting_tool: Option<(String, i64)>,
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
    /// The activity state that triggered this entry. Drives the
    /// status-row glyph (?/✓/⚠) so the user can tell at a glance
    /// whether a workspace is waiting for an answer, finished a
    /// task, or hit a permission prompt.
    pub activity: ActivityState,
}

/// One-char glyph for the inline status row. Mirrors the dashboard's
/// attn-marker vocabulary so users see the same icons in both surfaces.
fn glyph_for_activity(a: ActivityState) -> char {
    match a {
        ActivityState::AwaitingAnswer => '?',
        ActivityState::Complete => '\u{2713}', // ✓ CHECK MARK
        ActivityState::Awaiting | ActivityState::Stalled => '⚠',
        // Defensive default — non-alertable states shouldn't appear
        // in the status row (collect_attention filters by
        // needs_attention) but be safe.
        _ => '⚠',
    }
}

/// Collect every workspace whose `needs_attention` flag is set, excluding
/// the currently-attached one. Sorted by most-recent-first (newest
/// alerts surface at the front of the inline list).
pub fn collect_attention(
    candidates: &[WorkspaceUpdateInfo],
    attached_workspace: Option<WorkspaceId>,
    now_ms: i64,
) -> Vec<AttentionEntry> {
    let mut out: Vec<AttentionEntry> = candidates
        .iter()
        .filter(|c| c.needs_attention && Some(c.id) != attached_workspace)
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
                activity: c.activity,
            }
        })
        .collect();
    // Most recent first.
    out.sort_by_key(|e| -e.age_anchor_ms);
    out
}

/// Render the inline status-row line:
/// `repo/foo (5m) │ repo/bar (1h) │ repo/baz (15m)`
///
/// When the natural concatenation exceeds `max_width`, drop entries from
/// the right and append `… +N more`. Returns `None` when `entries` is
/// empty so the caller can collapse the status area entirely.
pub fn format_attention_line(
    entries: &[AttentionEntry],
    now_ms: i64,
    max_width: usize,
) -> Option<String> {
    if entries.is_empty() {
        return None;
    }
    let parts: Vec<String> = entries
        .iter()
        .map(|e| {
            let age = format_age(now_ms.saturating_sub(e.age_anchor_ms));
            let g = glyph_for_activity(e.activity);
            format!("{} {}/{} ({})", g, e.repo_name, e.name, age)
        })
        .collect();
    let sep = " │ ";
    // Greedy fit: include as many full entries as fit, then summarize the
    // remainder with "… +N more".
    let mut included = 0usize;
    let mut total = 0usize;
    for (i, p) in parts.iter().enumerate() {
        let sep_w = if i == 0 { 0 } else { sep.chars().count() };
        let candidate = total + sep_w + p.chars().count();
        if candidate > max_width {
            break;
        }
        total = candidate;
        included += 1;
    }
    if included == 0 {
        // Even the first entry doesn't fit — show it truncated so we never
        // render an empty bar when there ARE pending alerts.
        let mut truncated: String = parts[0].chars().take(max_width.saturating_sub(1)).collect();
        truncated.push('…');
        return Some(truncated);
    }
    let mut out = parts[..included].join(sep);
    let remaining = parts.len() - included;
    if remaining > 0 {
        let suffix = format!(" … +{remaining} more");
        let suffix_w = suffix.chars().count();
        // Trim included entries from the tail until the suffix fits.
        while included > 0 && out.chars().count() + suffix_w > max_width {
            included -= 1;
            out = parts[..included].join(sep);
        }
        out.push_str(&suffix);
    }
    Some(out)
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

    fn to_candidates(rows: &[WsOwned]) -> Vec<WorkspaceUpdateInfo<'_>> {
        rows.iter()
            .map(
                |(id, events, activity, needs_attention, awaiting, name, repo_name)| {
                    WorkspaceUpdateInfo {
                        id: *id,
                        name: name.as_str(),
                        repo_name: repo_name.as_str(),
                        events: events.as_ref(),
                        activity: *activity,
                        needs_attention: *needs_attention,
                        awaiting_tool: awaiting.clone(),
                    }
                },
            )
            .collect()
    }

    #[test]
    fn collect_attention_returns_empty_when_none_need_attention() {
        let evt = events_with_latest("recent", 5_000);
        let rows = [ws(1, "busy", Some(evt), ActivityState::Idle, false, None)];
        let candidates = to_candidates(&rows);
        let entries = collect_attention(&candidates, None, 10_000);
        assert!(entries.is_empty());
    }

    #[test]
    fn collect_attention_sorts_newest_first() {
        let older = events_with_latest("older", 1_000);
        let newer = events_with_latest("newer", 8_000);
        let rows = [
            ws(1, "older", Some(older), ActivityState::Waiting, true, None),
            ws(2, "newer", Some(newer), ActivityState::Awaiting, true, None),
        ];
        let candidates = to_candidates(&rows);
        let entries = collect_attention(&candidates, None, 10_000);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "newer");
        assert_eq!(entries[1].name, "older");
    }

    #[test]
    fn collect_attention_excludes_currently_attached() {
        let evt = events_with_latest("evt", 5_000);
        let rows = [ws(1, "self", Some(evt), ActivityState::Waiting, true, None)];
        let candidates = to_candidates(&rows);
        let entries = collect_attention(&candidates, Some(WorkspaceId(1)), 10_000);
        assert!(entries.is_empty());
    }

    #[test]
    fn collect_attention_uses_awaiting_tool_timestamp_as_anchor() {
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
        let entries = collect_attention(&candidates, None, 10_000);
        assert_eq!(entries[0].age_anchor_ms, 8_000);
    }

    #[test]
    fn format_attention_line_returns_none_when_empty() {
        assert!(format_attention_line(&[], 0, 80).is_none());
    }

    #[test]
    fn format_attention_line_joins_with_separator() {
        let entries = vec![
            AttentionEntry {
                workspace_id: WorkspaceId(1),
                repo_name: "a".into(),
                name: "x".into(),
                age_anchor_ms: 9_000, // 1s before now
                activity: ActivityState::Awaiting,
            },
            AttentionEntry {
                workspace_id: WorkspaceId(2),
                repo_name: "b".into(),
                name: "y".into(),
                age_anchor_ms: 5_000, // 5s before now
                activity: ActivityState::Awaiting,
            },
        ];
        let line = format_attention_line(&entries, 10_000, 80).expect("line");
        assert_eq!(line, "⚠ a/x (1s) │ ⚠ b/y (5s)");
    }

    #[test]
    fn format_attention_line_overflow_adds_plus_more_suffix() {
        let entries: Vec<AttentionEntry> = (0i64..5)
            .map(|i| AttentionEntry {
                workspace_id: WorkspaceId(i),
                repo_name: format!("repo{i}"),
                name: format!("ws{i}"),
                age_anchor_ms: 10_000 - i * 1000,
                activity: ActivityState::Awaiting,
            })
            .collect();
        // Width tight enough to fit 2 entries + "… +3 more".
        let line = format_attention_line(&entries, 10_000, 35).expect("line");
        assert!(line.contains("… +"), "expected overflow marker: {line}");
        assert!(line.ends_with("more"), "{line}");
        assert!(
            line.chars().count() <= 35,
            "got {} chars: {line}",
            line.chars().count()
        );
    }

    #[test]
    fn format_attention_line_extreme_overflow_truncates_first_entry() {
        // Even one entry doesn't fit — make sure we still render *something*
        // rather than returning an empty bar.
        let entries = vec![AttentionEntry {
            workspace_id: WorkspaceId(1),
            repo_name: "extremely-long-repo-name".into(),
            name: "workspace-name".into(),
            age_anchor_ms: 9_000,
            activity: ActivityState::Awaiting,
        }];
        let line = format_attention_line(&entries, 10_000, 10).expect("line");
        assert!(line.ends_with('…'), "expected ellipsis truncation: {line}");
        assert!(line.chars().count() <= 10);
    }

    #[test]
    fn format_attention_line_uses_question_glyph_for_awaiting_answer() {
        let entries = vec![AttentionEntry {
            workspace_id: WorkspaceId(1),
            repo_name: "demo".into(),
            name: "alpha".into(),
            age_anchor_ms: 0,
            activity: ActivityState::AwaitingAnswer,
        }];
        let line = format_attention_line(&entries, 5_000, 80).expect("line");
        assert!(line.starts_with("? demo/alpha"), "got: {line}");
    }

    #[test]
    fn format_attention_line_uses_check_glyph_for_complete() {
        let entries = vec![AttentionEntry {
            workspace_id: WorkspaceId(1),
            repo_name: "demo".into(),
            name: "alpha".into(),
            age_anchor_ms: 0,
            activity: ActivityState::Complete,
        }];
        let line = format_attention_line(&entries, 5_000, 80).expect("line");
        assert!(line.starts_with("\u{2713} demo/alpha"), "got: {line}");
    }

    #[test]
    fn format_attention_line_uses_warning_glyph_for_awaiting_permission() {
        let entries = vec![AttentionEntry {
            workspace_id: WorkspaceId(1),
            repo_name: "demo".into(),
            name: "alpha".into(),
            age_anchor_ms: 0,
            activity: ActivityState::Awaiting,
        }];
        let line = format_attention_line(&entries, 5_000, 80).expect("line");
        assert!(line.starts_with("⚠ demo/alpha"), "got: {line}");
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
