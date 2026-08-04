//! Shared synthesizers for the workspace row's status-adaptive flex
//! column and the detail bar's SESSION SUMMARY. Pure string builders
//! over `WorkspaceEvents` + `Status`; no rendering, no wall-clock reads.

use crate::activity::events::{ToolUseCounts, WorkspaceEvents};
use crate::data::store::{ReportedStatus, WorkspaceRecap};
use crate::ui::dashboard::status::Status;
use crate::ui::pm_pane::RECAP_STALE_SLACK_MS;
/// One recap segment for the renderer to width-fit. Agent-authored short
/// forms render verbatim; fallback full fields (`authored: false`) get a
/// width floor from the renderer and may expand into free column width.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecapSegment {
    pub text: String,
    pub authored: bool,
}

/// Precomputed flex-column content for one workspace row, chosen by the
/// caller from the workspace's status + events + recap + reported state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowColumn {
    /// Status word rendered first, always present: a fresh agent-pushed state
    /// (`working`/`blocked`/…) or the derived label (`asking`/`stalled`/…).
    pub token: String,
    /// Token came from a fresh push — the renderer uses the `▸ ` prefix.
    pub reported: bool,
    pub body: ColumnBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnBody {
    /// Agent-authored recap segments (goal/state/next, short forms preferred),
    /// greedy-fitted by the renderer. `stale`: activity outran `updated_at`.
    Recap {
        segments: Vec<RecapSegment>,
        stale: bool,
    },
    /// No recap — the pre-recap heuristic text (question topic, tool trace,
    /// last turn text…), already stripped of the status word the token carries.
    Fallback {
        text: String,
        emphasis: ColumnEmphasis,
    },
    Empty,
}

/// How the row renderer should color the column body. The leading prefix
/// always takes the status color; this controls the body only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnEmphasis {
    /// Default — non-attention states render dim, as the message line does today.
    Dim,
    /// `Question` — paint the body in the row's status color.
    Status,
    /// `Stalled` — paint the body in the warn color.
    Warn,
}

fn token_for(status: Status) -> &'static str {
    match status {
        Status::Question => "asking",
        Status::Complete => "done",
        other => other.label(),
    }
}

/// Build the status-adaptive flex-column content for one workspace row.
/// `now_ms` is the shared epoch-ms time base (same one `app.rs` uses), so
/// stall durations match the detail bar.
///
/// `reported` is the freshness-gated agent-pushed status from
/// `App::fresh_reported_status`; when present its state word becomes the
/// token (the pushed message text is no longer shown — the recap carries
/// detail now). `recap` is the workspace's latest `wsx recap set` digest;
/// when it has any short/full field it wins over the heuristic fallback body.
pub fn row_column(
    status: Status,
    events: Option<&WorkspaceEvents>,
    now_ms: i64,
    reported: Option<&ReportedStatus>,
    recap: Option<&WorkspaceRecap>,
) -> RowColumn {
    let (token, is_reported) = match reported {
        Some(r) => (r.state.as_str().to_string(), true),
        None => (token_for(status).to_string(), false),
    };
    let segments = recap.map(recap_segments).unwrap_or_default();
    let body = if !segments.is_empty() {
        let last_activity = events.map(|e| e.last_log_activity_ms).unwrap_or(0);
        let stale = recap
            .map(|r| last_activity > r.updated_at + RECAP_STALE_SLACK_MS)
            .unwrap_or(false);
        ColumnBody::Recap { segments, stale }
    } else {
        match fallback_text(status, events, now_ms) {
            Some((text, emphasis)) => ColumnBody::Fallback { text, emphasis },
            None => ColumnBody::Empty,
        }
    };
    RowColumn {
        token,
        reported: is_reported,
        body,
    }
}

/// Short form preferred (verbatim), full field article-stripped otherwise,
/// absent fields skipped. Order: goal, state, next. No width clipping here —
/// the renderer owns width policy (floor + expansion) via `authored`.
fn recap_segments(r: &WorkspaceRecap) -> Vec<RecapSegment> {
    [
        (&r.goal_short, &r.goal),
        (&r.state_short, &r.state),
        (&r.next_short, &r.next),
    ]
    .into_iter()
    .filter_map(|(short, full)| {
        non_empty_trimmed(short.as_deref())
            .map(|s| RecapSegment {
                text: collapse_ws(s),
                authored: true,
            })
            .or_else(|| {
                non_empty_trimmed(full.as_deref()).map(|f| RecapSegment {
                    text: terse(f),
                    authored: false,
                })
            })
    })
    .collect()
}

/// Mechanical terse-ification for a full recap field standing in for a
/// missing short form: collapse whitespace and drop articles (a/an/the).
/// Only ever applied to full fields — agent-authored short forms render
/// verbatim.
fn terse(s: &str) -> String {
    // This runs during per-frame row synthesis: build the stripped string
    // incrementally (no per-word lowercase allocation, no intermediate Vec).
    let is_article = |w: &str| {
        w.eq_ignore_ascii_case("a") || w.eq_ignore_ascii_case("an") || w.eq_ignore_ascii_case("the")
    };
    let collapsed = collapse_ws(s);
    let mut stripped = String::with_capacity(collapsed.len());
    for word in collapsed.split_whitespace() {
        if is_article(word) {
            continue;
        }
        if !stripped.is_empty() {
            stripped.push(' ');
        }
        stripped.push_str(word);
    }
    // An all-article field must not vanish — keep the raw text.
    if stripped.is_empty() {
        collapsed
    } else {
        stripped
    }
}

/// The pre-recap heuristic body, minus the status word (the token carries it):
/// the old `Question` arm's "asking: X" becomes "X", the old `Stalled` arm's
/// "stalled · 3m quiet" becomes "3m quiet", and the `{label}…` fillers vanish.
fn fallback_text(
    status: Status,
    events: Option<&WorkspaceEvents>,
    now_ms: i64,
) -> Option<(String, ColumnEmphasis)> {
    let evt = events?;
    match status {
        Status::Question => {
            let body = match evt.pending_question_tool() {
                Some("ExitPlanMode") => Some("review plan".to_string()),
                Some(_) => non_empty_trimmed(evt.pending_question_text.as_deref()).map(collapse_ws),
                None => evt
                    .pending_permission_tool(now_ms, 3_000)
                    .map(|(n, _)| format!("awaiting: {n}")),
            };
            body.map(|t| (t, ColumnEmphasis::Status))
        }
        Status::Stalled => {
            if evt.last_log_activity_ms > 0 {
                let quiet_secs =
                    now_ms.saturating_sub(evt.last_log_activity_ms).max(0) as u64 / 1000;
                Some((
                    format!("{} quiet", format_ago_short(Some(quiet_secs))),
                    ColumnEmphasis::Warn,
                ))
            } else {
                None
            }
        }
        Status::Thinking | Status::Waiting => {
            let trace = format_tool_trace(&evt.tool_use_counts);
            let live = non_empty_trimmed(evt.current_action.as_deref());
            let text = match (trace.is_empty(), live) {
                (false, Some(l)) => format!("{trace} · {l}"),
                (false, None) => trace,
                (true, Some(l)) => l.to_string(),
                (true, None) => return None,
            };
            Some((text, ColumnEmphasis::Dim))
        }
        Status::Complete => non_empty_trimmed(evt.last_completed_turn_text.as_deref())
            .or_else(|| non_empty_trimmed(evt.first_user_text.as_deref()))
            .map(|t| (collapse_ws(t), ColumnEmphasis::Dim)),
        Status::Idle => non_empty_trimmed(evt.first_user_text.as_deref())
            .map(|t| (collapse_ws(t), ColumnEmphasis::Dim)),
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
        parts.push(format!(
            "read {} {}",
            counts.read,
            plural("file", counts.read)
        ));
    }
    if counts.edit > 0 {
        parts.push(format!(
            "edited {} {}",
            counts.edit,
            plural("file", counts.edit)
        ));
    }
    if counts.write > 0 {
        parts.push(format!(
            "wrote {} {}",
            counts.write,
            plural("file", counts.write)
        ));
    }
    if counts.bash > 0 {
        parts.push(format!(
            "ran {} {}",
            counts.bash,
            plural("command", counts.bash)
        ));
    }
    if counts.other > 0 {
        parts.push(format!("+{} other actions", counts.other));
    }
    parts.join(", ")
}

/// Trim `s` and keep it only if it is non-empty after trimming. Lets the
/// `Complete`/`Idle` arms chain candidate signals with `.or_else` so a
/// blank value falls through to the next candidate.
fn non_empty_trimmed(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|t| !t.is_empty())
}

/// Collapse every run of whitespace (spaces, tabs, newlines) into a single
/// space and trim the ends. The dashboard row renders each workspace as a
/// single-line `ListItem`; an interior newline would miscount against the
/// char-based truncation and misalign the right-aligned age column. The old
/// `EventSnapshot.display` path collapsed whitespace upstream — this keeps
/// the same single-line guarantee for the raw `first_user_text` / recap text.
///
/// Builds the result incrementally rather than collecting into a `Vec` first,
/// since the row column is synthesized every frame per workspace.
fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for word in s.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
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
    use crate::data::store::ReportedState;
    use std::collections::HashMap;

    fn evt() -> WorkspaceEvents {
        WorkspaceEvents::default()
    }

    fn recap_with(
        goal: Option<&str>,
        goal_short: Option<&str>,
        state_short: Option<&str>,
        next_short: Option<&str>,
        updated_at: i64,
    ) -> WorkspaceRecap {
        WorkspaceRecap {
            goal: goal.map(String::from),
            goal_short: goal_short.map(String::from),
            state_short: state_short.map(String::from),
            next_short: next_short.map(String::from),
            updated_at,
            ..Default::default()
        }
    }

    fn reported(state: ReportedState) -> ReportedStatus {
        ReportedStatus {
            state,
            message: Some("ignored by the column now".into()),
            source: "model".into(),
            reported_at: 0,
        }
    }

    #[test]
    fn token_derives_from_status_when_no_push() {
        let c = row_column(Status::Question, Some(&evt()), 0, None, None);
        assert_eq!(c.token, "asking");
        assert!(!c.reported);
        let c = row_column(Status::Complete, Some(&evt()), 0, None, None);
        assert_eq!(c.token, "done");
    }

    #[test]
    fn fresh_push_sets_token_and_reported_flag() {
        let r = reported(ReportedState::Blocked);
        let c = row_column(Status::Waiting, Some(&evt()), 0, Some(&r), None);
        assert_eq!(c.token, "blocked");
        assert!(c.reported);
    }

    #[test]
    fn pushed_message_text_no_longer_appears() {
        let r = reported(ReportedState::Working);
        let c = row_column(Status::Waiting, None, 0, Some(&r), None);
        assert!(matches!(c.body, ColumnBody::Empty));
    }

    #[test]
    fn recap_prefers_short_forms_in_order() {
        let rc = recap_with(
            None,
            Some("Audit V2 #2835"),
            Some("3/12 done"),
            Some("fix drift"),
            0,
        );
        let c = row_column(Status::Waiting, Some(&evt()), 0, None, Some(&rc));
        match c.body {
            ColumnBody::Recap { segments, stale } => {
                let texts: Vec<&str> = segments.iter().map(|s| s.text.as_str()).collect();
                assert_eq!(texts, vec!["Audit V2 #2835", "3/12 done", "fix drift"]);
                assert!(segments.iter().all(|s| s.authored));
                assert!(!stale);
            }
            other => panic!("expected Recap, got {other:?}"),
        }
    }

    #[test]
    fn missing_short_falls_back_to_stripped_full_field() {
        let long = "Audit all V2 invoices auto-issued today for the CV-04964 amount-drift bug";
        let rc = recap_with(Some(long), None, Some("3/12 done"), None, 0);
        let c = row_column(Status::Waiting, Some(&evt()), 0, None, Some(&rc));
        match c.body {
            ColumnBody::Recap { segments, .. } => {
                assert_eq!(
                    segments.len(),
                    2,
                    "absent next is skipped, not placeholder'd"
                );
                // Articles stripped, otherwise unclipped: the renderer owns
                // width policy now, keyed off `authored`.
                assert_eq!(
                    segments[0].text,
                    "Audit all V2 invoices auto-issued today for CV-04964 amount-drift bug"
                );
                assert!(!segments[0].authored);
                assert!(segments[1].authored);
            }
            other => panic!("expected Recap, got {other:?}"),
        }
    }

    #[test]
    fn full_field_fallback_strips_articles_without_clipping() {
        let rc = recap_with(
            Some("Make the dashboard PR status column clickable from anywhere"),
            None,
            None,
            None,
            0,
        );
        let c = row_column(Status::Waiting, Some(&evt()), 0, None, Some(&rc));
        match c.body {
            ColumnBody::Recap { segments, .. } => {
                assert_eq!(
                    segments[0].text,
                    "Make dashboard PR status column clickable from anywhere"
                );
            }
            other => panic!("expected Recap, got {other:?}"),
        }
    }

    #[test]
    fn authored_short_forms_are_never_rewritten() {
        // Article stripping applies only to the mechanical full-field clip;
        // agent-authored short forms render verbatim.
        let rc = recap_with(None, Some("fix the flaky thing"), None, None, 0);
        let c = row_column(Status::Waiting, Some(&evt()), 0, None, Some(&rc));
        match c.body {
            ColumnBody::Recap { segments, .. } => {
                assert_eq!(segments[0].text, "fix the flaky thing");
                assert!(segments[0].authored);
            }
            other => panic!("expected Recap, got {other:?}"),
        }
    }

    #[test]
    fn all_article_field_falls_back_to_raw_text() {
        // A field that is nothing but articles must not vanish entirely.
        let rc = recap_with(Some("the the the"), None, None, None, 0);
        let c = row_column(Status::Waiting, Some(&evt()), 0, None, Some(&rc));
        match c.body {
            ColumnBody::Recap { segments, .. } => {
                assert_eq!(segments[0].text, "the the the");
            }
            other => panic!("expected Recap, got {other:?}"),
        }
    }

    #[test]
    fn all_empty_recap_behaves_as_no_recap() {
        let rc = recap_with(None, None, None, None, 0);
        let e = WorkspaceEvents {
            first_user_text: Some("migrate auth".into()),
            ..WorkspaceEvents::default()
        };
        let c = row_column(Status::Idle, Some(&e), 0, None, Some(&rc));
        assert!(matches!(c.body, ColumnBody::Fallback { .. }));
    }

    #[test]
    fn recap_stale_when_activity_outruns_updated_at() {
        let rc = recap_with(None, Some("g"), None, None, 1_000);
        let e = WorkspaceEvents {
            last_log_activity_ms: 1_000 + crate::ui::pm_pane::RECAP_STALE_SLACK_MS + 1,
            ..WorkspaceEvents::default()
        };
        let c = row_column(Status::Waiting, Some(&e), 0, None, Some(&rc));
        assert!(matches!(c.body, ColumnBody::Recap { stale: true, .. }));
    }

    #[test]
    fn question_fallback_drops_asking_prefix() {
        // Token already says "asking"; the fallback body is the bare topic.
        let mut e = evt();
        e.pending_tool_uses
            .insert("tu_q".into(), ("AskUserQuestion".into(), 0));
        e.pending_question_text = Some("Auth method".into());
        let c = row_column(Status::Question, Some(&e), 10_000, None, None);
        assert_eq!(c.token, "asking");
        match c.body {
            ColumnBody::Fallback { text, emphasis } => {
                assert_eq!(text, "Auth method");
                assert_eq!(emphasis, ColumnEmphasis::Status);
            }
            other => panic!("expected Fallback, got {other:?}"),
        }
    }

    #[test]
    fn question_without_topic_renders_asking_only_via_empty_body() {
        let mut e = evt();
        e.pending_tool_uses
            .insert("tu_q".into(), ("AskUserQuestion".into(), 0));
        let c = row_column(Status::Question, Some(&e), 10_000, None, None);
        assert_eq!(c.token, "asking");
        assert!(matches!(c.body, ColumnBody::Empty));
    }

    #[test]
    fn question_blank_topic_falls_back_to_empty_body() {
        // A whitespace-only topic must not render as a body; it falls
        // through to Empty like an absent topic.
        let mut e = evt();
        e.pending_tool_uses
            .insert("tu_q".into(), ("AskUserQuestion".into(), 0));
        e.pending_question_text = Some("   ".into());
        let c = row_column(Status::Question, Some(&e), 10_000, None, None);
        assert!(matches!(c.body, ColumnBody::Empty));
    }

    #[test]
    fn question_exit_plan_mode_renders_review_plan() {
        let mut e = evt();
        e.pending_tool_uses
            .insert("tu_p".into(), ("ExitPlanMode".into(), 0));
        let c = row_column(Status::Question, Some(&e), 10_000, None, None);
        match c.body {
            ColumnBody::Fallback { text, emphasis } => {
                assert_eq!(text, "review plan");
                assert_eq!(emphasis, ColumnEmphasis::Status);
            }
            other => panic!("expected Fallback, got {other:?}"),
        }
    }

    #[test]
    fn question_permission_tool_renders_awaiting_tool() {
        let mut pending = HashMap::new();
        // epoch-0 timestamp guarantees age > the 3s stale threshold.
        pending.insert("tu_b".to_string(), ("Bash".to_string(), 0_i64));
        let e = WorkspaceEvents {
            pending_tool_uses: pending,
            ..WorkspaceEvents::default()
        };
        let c = row_column(Status::Question, Some(&e), 10_000, None, None);
        match c.body {
            ColumnBody::Fallback { text, emphasis } => {
                assert_eq!(text, "awaiting: Bash");
                assert_eq!(emphasis, ColumnEmphasis::Status);
            }
            other => panic!("expected Fallback, got {other:?}"),
        }
    }

    #[test]
    fn question_with_no_pending_tool_uses_empty_body() {
        let c = row_column(Status::Question, Some(&evt()), 10_000, None, None);
        assert_eq!(c.token, "asking");
        assert!(matches!(c.body, ColumnBody::Empty));
    }

    #[test]
    fn stalled_fallback_is_quiet_detail_only() {
        let e = WorkspaceEvents {
            last_log_activity_ms: 1,
            ..WorkspaceEvents::default()
        };
        let c = row_column(Status::Stalled, Some(&e), 240_000, None, None);
        assert_eq!(c.token, "stalled");
        match c.body {
            ColumnBody::Fallback { text, emphasis } => {
                assert_eq!(text, "3m quiet");
                assert_eq!(emphasis, ColumnEmphasis::Warn);
            }
            other => panic!("expected Fallback, got {other:?}"),
        }
    }

    #[test]
    fn thinking_shows_tool_trace_dim() {
        let mut e = evt();
        e.tool_use_counts.bash = 2;
        e.tool_use_counts.edit = 3;
        let c = row_column(Status::Thinking, Some(&e), 0, None, None);
        match c.body {
            ColumnBody::Fallback { text, emphasis } => {
                assert_eq!(text, "edited 3 files, ran 2 commands");
                assert_eq!(emphasis, ColumnEmphasis::Dim);
            }
            other => panic!("expected Fallback, got {other:?}"),
        }
    }

    #[test]
    fn thinking_with_no_tools_yet_is_empty_body() {
        let c = row_column(Status::Thinking, Some(&evt()), 0, None, None);
        assert_eq!(c.token, "thinking");
        assert!(matches!(c.body, ColumnBody::Empty));
    }

    #[test]
    fn waiting_with_no_tools_yet_is_empty_body() {
        let c = row_column(Status::Waiting, Some(&evt()), 0, None, None);
        assert_eq!(c.token, "waiting");
        assert!(matches!(c.body, ColumnBody::Empty));
    }

    #[test]
    fn thinking_appends_current_action_to_trace() {
        let mut e = evt();
        e.tool_use_counts.edit = 3;
        e.current_action = Some("now column_content.rs".into());
        let c = row_column(Status::Thinking, Some(&e), 0, None, None);
        match c.body {
            ColumnBody::Fallback { text, emphasis } => {
                assert_eq!(text, "edited 3 files · now column_content.rs");
                assert_eq!(emphasis, ColumnEmphasis::Dim);
            }
            other => panic!("expected Fallback, got {other:?}"),
        }
    }

    #[test]
    fn thinking_appends_bash_command_to_trace() {
        let mut e = evt();
        e.tool_use_counts.bash = 5;
        e.current_action = Some("cargo test --lib".into());
        let c = row_column(Status::Thinking, Some(&e), 0, None, None);
        match c.body {
            ColumnBody::Fallback { text, .. } => {
                assert_eq!(text, "ran 5 commands · cargo test --lib");
            }
            other => panic!("expected Fallback, got {other:?}"),
        }
    }

    #[test]
    fn thinking_shows_action_alone_when_no_counts_yet() {
        let mut e = evt();
        e.current_action = Some("now column_content.rs".into());
        let c = row_column(Status::Thinking, Some(&e), 0, None, None);
        match c.body {
            ColumnBody::Fallback { text, .. } => {
                assert_eq!(text, "now column_content.rs");
            }
            other => panic!("expected Fallback, got {other:?}"),
        }
    }

    #[test]
    fn waiting_appends_current_action_to_trace() {
        // Waiting shares the arm with Thinking; pin the live-item behavior
        // under Waiting too so a future refactor can't split it silently.
        let mut e = evt();
        e.tool_use_counts.bash = 5;
        e.current_action = Some("cargo test --lib".into());
        let c = row_column(Status::Waiting, Some(&e), 0, None, None);
        match c.body {
            ColumnBody::Fallback { text, emphasis } => {
                assert_eq!(text, "ran 5 commands · cargo test --lib");
                assert_eq!(emphasis, ColumnEmphasis::Dim);
            }
            other => panic!("expected Fallback, got {other:?}"),
        }
    }

    #[test]
    fn complete_prefers_turn_recap() {
        let e = WorkspaceEvents {
            last_completed_turn_text: Some("split the quick-start into two".into()),
            first_user_text: Some("do the thing".into()),
            ..WorkspaceEvents::default()
        };
        let c = row_column(Status::Complete, Some(&e), 0, None, None);
        match c.body {
            ColumnBody::Fallback { text, emphasis } => {
                assert_eq!(text, "split the quick-start into two");
                assert_eq!(emphasis, ColumnEmphasis::Dim);
            }
            other => panic!("expected Fallback, got {other:?}"),
        }
    }

    #[test]
    fn complete_falls_back_to_first_user_text() {
        let e = WorkspaceEvents {
            first_user_text: Some("migrate auth".into()),
            ..WorkspaceEvents::default()
        };
        let c = row_column(Status::Complete, Some(&e), 0, None, None);
        match c.body {
            ColumnBody::Fallback { text, .. } => assert_eq!(text, "migrate auth"),
            other => panic!("expected Fallback, got {other:?}"),
        }
    }

    #[test]
    fn complete_with_nothing_is_empty_body() {
        let c = row_column(Status::Complete, Some(&evt()), 0, None, None);
        assert!(matches!(c.body, ColumnBody::Empty));
    }

    #[test]
    fn complete_whitespace_only_recap_falls_back_to_prompt() {
        // A blank/whitespace-only recap must not block the fallback to the
        // prompt (regression: an `.or()` before trimming kept the blank
        // recap and rendered the em-dash).
        let e = WorkspaceEvents {
            last_completed_turn_text: Some("   \n  ".into()),
            first_user_text: Some("migrate auth".into()),
            ..WorkspaceEvents::default()
        };
        let c = row_column(Status::Complete, Some(&e), 0, None, None);
        match c.body {
            ColumnBody::Fallback { text, .. } => assert_eq!(text, "migrate auth"),
            other => panic!("expected Fallback, got {other:?}"),
        }
    }

    #[test]
    fn idle_shows_first_user_text() {
        let e = WorkspaceEvents {
            first_user_text: Some("backfill the 003 migration".into()),
            ..WorkspaceEvents::default()
        };
        let c = row_column(Status::Idle, Some(&e), 0, None, None);
        match c.body {
            ColumnBody::Fallback { text, emphasis } => {
                assert_eq!(text, "backfill the 003 migration");
                assert_eq!(emphasis, ColumnEmphasis::Dim);
            }
            other => panic!("expected Fallback, got {other:?}"),
        }
    }

    #[test]
    fn idle_with_no_prompt_is_empty_body() {
        let c = row_column(Status::Idle, Some(&evt()), 0, None, None);
        assert!(matches!(c.body, ColumnBody::Empty));
    }

    #[test]
    fn idle_collapses_interior_newlines_to_single_line() {
        let e = WorkspaceEvents {
            first_user_text: Some("migrate auth\n\nto the new token flow".into()),
            ..WorkspaceEvents::default()
        };
        let c = row_column(Status::Idle, Some(&e), 0, None, None);
        match c.body {
            ColumnBody::Fallback { text, .. } => {
                assert_eq!(text, "migrate auth to the new token flow");
                assert!(!text.contains('\n'));
            }
            other => panic!("expected Fallback, got {other:?}"),
        }
    }

    #[test]
    fn complete_collapses_interior_whitespace_to_single_line() {
        let e = WorkspaceEvents {
            last_completed_turn_text: Some("split the quick-start\n  into two   sections".into()),
            ..WorkspaceEvents::default()
        };
        let c = row_column(Status::Complete, Some(&e), 0, None, None);
        match c.body {
            ColumnBody::Fallback { text, .. } => {
                assert_eq!(text, "split the quick-start into two sections");
                assert!(!text.contains('\n'));
            }
            other => panic!("expected Fallback, got {other:?}"),
        }
    }

    #[test]
    fn no_events_no_recap_is_token_only() {
        let c = row_column(Status::Idle, None, 0, None, None);
        assert_eq!(c.token, "idle");
        assert!(matches!(c.body, ColumnBody::Empty));
    }
}
