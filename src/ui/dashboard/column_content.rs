//! Shared synthesizers for the workspace row's status-adaptive flex
//! column and the detail bar's SESSION SUMMARY. Pure string builders
//! over `WorkspaceEvents` + `Status`; no rendering, no wall-clock reads.

use crate::activity::events::{ToolUseCounts, WorkspaceEvents};
use crate::ui::dashboard::status::Status;

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

fn plural(noun: &str, n: u32) -> String {
    if n == 1 {
        noun.to_string()
    } else {
        format!("{noun}s")
    }
}
