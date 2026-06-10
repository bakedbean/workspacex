//! Shared synthesizers for the workspace row's status-adaptive flex
//! column and the detail bar's SESSION SUMMARY. Pure string builders
//! over `WorkspaceEvents` + `Status`; no rendering, no wall-clock reads.

use crate::activity::events::{ToolUseCounts, WorkspaceEvents};
use crate::ui::dashboard::status::Status;

/// Precomputed flex-column content for one workspace row, chosen by the
/// caller from the workspace's status + events. `None` renders as the
/// em-dash placeholder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowColumn {
    pub text: String,
    pub emphasis: ColumnEmphasis,
}

/// How the row renderer should color the column body. The leading `└ `
/// prefix always takes the status color; this controls the body only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnEmphasis {
    /// Default — non-attention states render dim, as the message line does today.
    Dim,
    /// `Question` — paint the body in the row's status color.
    Status,
    /// `Stalled` — paint the body in the warn color.
    Warn,
}

/// Build the status-adaptive flex-column content for one workspace row.
/// `now_ms` is the shared epoch-ms time base (same one `app.rs` uses), so
/// stall durations match the detail bar. Returns `None` when there is no
/// meaningful content (the caller renders the em-dash).
pub fn row_column(status: Status, events: Option<&WorkspaceEvents>, now_ms: i64) -> Option<RowColumn> {
    let evt = events?;
    match status {
        Status::Question => {
            // Unlike `format_state_line` (which renders "question · <tool>"
            // as a status detail), the row column shows just the tool name as
            // the whole body — so the lookup is duplicated here on purpose.
            let body = evt
                .pending_question_tool()
                .map(|n| n.to_string())
                .or_else(|| evt.pending_permission_tool(now_ms, 3_000).map(|(n, _)| n))
                .unwrap_or_else(|| "question".to_string());
            Some(RowColumn { text: body, emphasis: ColumnEmphasis::Status })
        }
        Status::Stalled => Some(RowColumn {
            text: format_state_line(status, evt, now_ms),
            emphasis: ColumnEmphasis::Warn,
        }),
        Status::Thinking | Status::Waiting => {
            let trace = format_tool_trace(&evt.tool_use_counts);
            let text = if trace.is_empty() {
                format!("{}…", status.label())
            } else {
                trace
            };
            Some(RowColumn { text, emphasis: ColumnEmphasis::Dim })
        }
        Status::Complete => {
            let body = evt
                .last_completed_turn_text
                .as_deref()
                .or(evt.first_user_text.as_deref())
                .map(str::trim)
                .filter(|s| !s.is_empty())?;
            Some(RowColumn { text: collapse_ws(body), emphasis: ColumnEmphasis::Dim })
        }
        Status::Idle => {
            let body = evt
                .first_user_text
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())?;
            Some(RowColumn { text: collapse_ws(body), emphasis: ColumnEmphasis::Dim })
        }
    }
}

/// Canonical status label, optionally enriched with a why-detail drawn
/// from evt fields — pending question/permission tool for `Question`,
/// quiet duration for `Stalled`. Other states use the bare label.
pub(crate) fn format_state_line(status: Status, evt: &WorkspaceEvents, now_ms: i64) -> String {
    let base = status.label();
    let detail: Option<String> = match status {
        Status::Question => evt
            .pending_question_tool()
            .map(|n| n.to_string())
            .or_else(|| {
                evt.pending_permission_tool(now_ms, 3_000)
                    .map(|(name, _)| name)
            }),
        Status::Stalled => {
            if evt.last_log_activity_ms > 0 {
                let quiet_secs =
                    now_ms.saturating_sub(evt.last_log_activity_ms).max(0) as u64 / 1000;
                Some(format!("{} quiet", format_ago_short(Some(quiet_secs))))
            } else {
                None
            }
        }
        Status::Waiting | Status::Thinking | Status::Complete | Status::Idle => None,
    };
    match detail {
        Some(d) => format!("{base} · {d}"),
        None => base.to_string(),
    }
}

pub(crate) fn format_ago_short(secs: Option<u64>) -> String {
    match secs {
        None => "—".to_string(),
        Some(s) if s < 60 => format!("{s}s"),
        Some(s) if s < 3600 => format!("{}m", s / 60),
        Some(s) => format!("{}h", s / 3600),
    }
}

pub(crate) fn format_tool_trace(counts: &ToolUseCounts) -> String {
    let mut parts: Vec<String> = Vec::new();
    if counts.read > 0 {
        parts.push(format!("read {} {}", counts.read, plural("file", counts.read)));
    }
    if counts.edit > 0 {
        parts.push(format!("edited {} {}", counts.edit, plural("file", counts.edit)));
    }
    if counts.write > 0 {
        parts.push(format!("wrote {} {}", counts.write, plural("file", counts.write)));
    }
    if counts.bash > 0 {
        parts.push(format!("ran {} {}", counts.bash, plural("command", counts.bash)));
    }
    if counts.other > 0 {
        parts.push(format!("+{} other actions", counts.other));
    }
    parts.join(", ")
}

/// Collapse every run of whitespace (spaces, tabs, newlines) into a single
/// space and trim the ends. The dashboard row renders each workspace as a
/// single-line `ListItem`; an interior newline would miscount against the
/// char-based truncation and misalign the right-aligned age column. The old
/// `EventSnapshot.display` path collapsed whitespace upstream — this keeps
/// the same single-line guarantee for the raw `first_user_text` / recap text.
fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn plural(noun: &str, n: u32) -> String {
    if n == 1 {
        noun.to_string()
    } else {
        format!("{noun}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::events::WorkspaceEvents;
    use std::collections::HashMap;

    fn evt() -> WorkspaceEvents {
        WorkspaceEvents::default()
    }

    #[test]
    fn none_events_yields_none() {
        assert!(row_column(Status::Idle, None, 0).is_none());
    }

    #[test]
    fn question_surfaces_pending_question_tool_with_status_emphasis() {
        let mut e = evt();
        e.pending_tool_uses
            .insert("tu_q".into(), ("AskUserQuestion".into(), 0));
        let c = row_column(Status::Question, Some(&e), 10_000).unwrap();
        assert_eq!(c.text, "AskUserQuestion");
        assert_eq!(c.emphasis, ColumnEmphasis::Status);
    }

    #[test]
    fn question_surfaces_exit_plan_mode_tool() {
        let mut e = evt();
        e.pending_tool_uses
            .insert("tu_p".into(), ("ExitPlanMode".into(), 0));
        let c = row_column(Status::Question, Some(&e), 10_000).unwrap();
        assert_eq!(c.text, "ExitPlanMode");
        assert_eq!(c.emphasis, ColumnEmphasis::Status);
    }

    #[test]
    fn question_falls_back_to_permission_tool() {
        let mut pending = HashMap::new();
        // epoch-0 timestamp guarantees age > the 3s stale threshold.
        pending.insert("tu_b".to_string(), ("Bash".to_string(), 0_i64));
        let e = WorkspaceEvents {
            pending_tool_uses: pending,
            ..WorkspaceEvents::default()
        };
        let c = row_column(Status::Question, Some(&e), 10_000).unwrap();
        assert_eq!(c.text, "Bash");
        assert_eq!(c.emphasis, ColumnEmphasis::Status);
    }

    #[test]
    fn question_with_no_pending_tool_uses_bare_label() {
        let c = row_column(Status::Question, Some(&evt()), 10_000).unwrap();
        assert_eq!(c.text, "question");
        assert_eq!(c.emphasis, ColumnEmphasis::Status);
    }

    #[test]
    fn stalled_shows_quiet_duration_with_warn_emphasis() {
        let e = WorkspaceEvents {
            last_log_activity_ms: 1,
            ..WorkspaceEvents::default()
        };
        // now_ms = 240_000, last_log_activity_ms = 1 → (240_000-1)/1000 = 239s → "3m quiet"
        let c = row_column(Status::Stalled, Some(&e), 240_000).unwrap();
        assert_eq!(c.text, "stalled · 3m quiet");
        assert_eq!(c.emphasis, ColumnEmphasis::Warn);
    }

    #[test]
    fn thinking_shows_tool_trace_dim() {
        let mut e = evt();
        e.tool_use_counts.bash = 2;
        e.tool_use_counts.edit = 3;
        let c = row_column(Status::Thinking, Some(&e), 0).unwrap();
        assert_eq!(c.text, "edited 3 files, ran 2 commands");
        assert_eq!(c.emphasis, ColumnEmphasis::Dim);
    }

    #[test]
    fn thinking_with_no_tools_yet_shows_ellipsis_label() {
        let c = row_column(Status::Thinking, Some(&evt()), 0).unwrap();
        assert_eq!(c.text, "thinking…");
    }

    #[test]
    fn waiting_with_no_tools_yet_shows_ellipsis_label() {
        let c = row_column(Status::Waiting, Some(&evt()), 0).unwrap();
        assert_eq!(c.text, "waiting…");
    }

    #[test]
    fn complete_prefers_turn_recap() {
        let e = WorkspaceEvents {
            last_completed_turn_text: Some("split the quick-start into two".into()),
            first_user_text: Some("do the thing".into()),
            ..WorkspaceEvents::default()
        };
        let c = row_column(Status::Complete, Some(&e), 0).unwrap();
        assert_eq!(c.text, "split the quick-start into two");
        assert_eq!(c.emphasis, ColumnEmphasis::Dim);
    }

    #[test]
    fn complete_falls_back_to_first_user_text() {
        let e = WorkspaceEvents {
            first_user_text: Some("migrate auth".into()),
            ..WorkspaceEvents::default()
        };
        let c = row_column(Status::Complete, Some(&e), 0).unwrap();
        assert_eq!(c.text, "migrate auth");
    }

    #[test]
    fn complete_with_nothing_is_none() {
        assert!(row_column(Status::Complete, Some(&evt()), 0).is_none());
    }

    #[test]
    fn idle_shows_first_user_text() {
        let e = WorkspaceEvents {
            first_user_text: Some("backfill the 003 migration".into()),
            ..WorkspaceEvents::default()
        };
        let c = row_column(Status::Idle, Some(&e), 0).unwrap();
        assert_eq!(c.text, "backfill the 003 migration");
        assert_eq!(c.emphasis, ColumnEmphasis::Dim);
    }

    #[test]
    fn idle_with_no_prompt_is_none() {
        assert!(row_column(Status::Idle, Some(&evt()), 0).is_none());
    }

    #[test]
    fn idle_collapses_interior_newlines_to_single_line() {
        let e = WorkspaceEvents {
            first_user_text: Some("migrate auth\n\nto the new token flow".into()),
            ..WorkspaceEvents::default()
        };
        let c = row_column(Status::Idle, Some(&e), 0).unwrap();
        assert_eq!(c.text, "migrate auth to the new token flow");
        assert!(!c.text.contains('\n'));
    }

    #[test]
    fn complete_collapses_interior_whitespace_to_single_line() {
        let e = WorkspaceEvents {
            last_completed_turn_text: Some("split the quick-start\n  into two   sections".into()),
            ..WorkspaceEvents::default()
        };
        let c = row_column(Status::Complete, Some(&e), 0).unwrap();
        assert_eq!(c.text, "split the quick-start into two sections");
        assert!(!c.text.contains('\n'));
    }
}
