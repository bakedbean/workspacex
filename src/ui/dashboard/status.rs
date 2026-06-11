//! Canonical 6-state status vocabulary used by every view, gutter, and
//! status-strip cell. Maps the existing classifier inputs from `app.rs`
//! into a single enum so column renderers don't depend on classifier
//! internals.

use crate::app::StoppedKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    Question,
    Stalled,
    Waiting,
    Thinking,
    Complete,
    Idle,
}

impl Status {
    /// Sort key. Higher = more urgent; used for within-repo /
    /// within-section workspace ordering.
    pub fn priority(self) -> u8 {
        match self {
            Status::Stalled => 5,
            Status::Question => 4,
            Status::Waiting => 3,
            Status::Thinking => 2,
            Status::Complete => 1,
            Status::Idle => 0,
        }
    }

    /// Static glyph for this status. Live states (`Thinking`, `Waiting`)
    /// use this only when the renderer cannot animate; otherwise the
    /// spinner replaces it.
    pub fn glyph(self) -> char {
        match self {
            Status::Question => '?',
            Status::Stalled => '!',
            Status::Waiting => '…',
            Status::Thinking => '⠋',
            Status::Complete => '✓',
            Status::Idle => '·',
        }
    }

    /// Human-readable label used in the status strip and section headers.
    pub fn label(self) -> &'static str {
        match self {
            Status::Question => "question",
            Status::Stalled => "stalled",
            Status::Waiting => "waiting",
            Status::Thinking => "thinking",
            Status::Complete => "complete",
            Status::Idle => "idle",
        }
    }

    /// Live states animate the spinner in place of `glyph()`.
    pub fn is_live(self) -> bool {
        matches!(self, Status::Thinking | Status::Waiting)
    }

    /// Reduce the existing classifier inputs into a canonical `Status`.
    /// Matches the mapping table in the V5 design spec.
    // 8 inputs: each is an independent classifier signal; bundling them into a
    // struct buys little and ripples to ~19 call sites. Matches house style.
    #[allow(clippy::too_many_arguments)]
    pub fn classify(
        awaiting_tool: bool,
        stopped_kind: Option<StoppedKind>,
        stalled: bool,
        seconds_since_activity: Option<u64>,
        session_running: bool,
        user_has_prompted: bool,
        has_prior_session: bool,
        reported: Option<crate::data::store::ReportedState>,
    ) -> Self {
        use crate::data::store::ReportedState;
        // The `awaiting_tool` heuristic flags any non-question tool_use
        // pending >3s as a permission prompt — but if the PTY is still
        // streaming output the agent is clearly mid-work, so suppress the
        // false-positive `?`. Catches long `Bash` runs and any other
        // tool whose process pipes activity to the parent PTY.
        let pty_active = matches!(seconds_since_activity, Some(s) if s < 2);
        if awaiting_tool && !pty_active {
            return Status::Question;
        }

        // Tier 1/2 push: the agent (or a hook) explicitly told us it is blocked
        // or done. Terminal user-facing states; trust them over the JSONL
        // stopped_kind heuristic.
        match reported {
            Some(ReportedState::Blocked) => return Status::Question,
            Some(ReportedState::Done) => return Status::Complete,
            _ => {}
        }

        match stopped_kind {
            Some(StoppedKind::AwaitingAnswer) => return Status::Question,
            Some(StoppedKind::Complete) => return Status::Complete,
            None => {}
        }
        // `stalled` is a purely JSONL-derived signal: it fires after 60s of
        // no log growth. But a long thinking/streaming phase — most commonly
        // digesting a freshly-loaded skill body, whose `tool_use` left
        // `last_stop_reason = ToolUse` with no pending tool — writes nothing
        // to the JSONL while the terminal streams the spinner sub-second. An
        // active PTY proves the agent is alive, so suppress the false-positive
        // `!` and let the running path paint the Thinking spinner. Same
        // reasoning as the `awaiting_tool && !pty_active` guard above: a
        // genuine stall is quiet on BOTH channels.
        // Runs BEFORE the pushed live-states below, so a stuck "working" push
        // (JSONL quiet 60s, PTY idle) still self-heals to Stalled.
        if stalled && !pty_active {
            return Status::Stalled;
        }

        // Tier 1/2 push: live states. Below the stall guard by design.
        match reported {
            Some(ReportedState::Working) => return Status::Thinking,
            Some(ReportedState::Waiting) => return Status::Waiting,
            _ => {}
        }

        if session_running {
            if !user_has_prompted {
                // Session is live but no user prompt is recorded yet — the
                // agent is idle at its welcome screen (or its events haven't
                // been tailed yet), so nothing has happened. Show Idle (static
                // `·`) rather than the spinner. The `first_user_text` signal
                // lags the log tail (~2s), so the very first turn may read Idle
                // briefly before flipping to the spinner.
                return Status::Idle;
            }
            match seconds_since_activity {
                Some(s) if s < 30 => Status::Thinking,
                Some(_) => Status::Waiting,
                None => Status::Thinking,
            }
        } else {
            // No live session — `has_prior_session` distinguishes
            // "resumable" from "off" today; both collapse to Idle in V5.
            let _ = has_prior_session;
            Status::Idle
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::store::ReportedState;

    fn s(secs: u64) -> Option<u64> {
        Some(secs)
    }

    // Helper mirroring the new signature with sensible "live session, just active"
    // defaults so each test only sets what it exercises.
    fn classify_reported(reported: Option<ReportedState>, stalled: bool) -> Status {
        Status::classify(
            false,   // awaiting_tool
            None,    // stopped_kind
            stalled, // stalled
            s(5),    // seconds_since_activity (live)
            true,    // session_running
            true,    // user_has_prompted
            false,   // has_prior_session
            reported, // reported state
        )
    }

    #[test]
    fn reported_blocked_maps_to_question() {
        assert_eq!(
            classify_reported(Some(ReportedState::Blocked), false),
            Status::Question
        );
    }

    #[test]
    fn reported_done_maps_to_complete() {
        assert_eq!(
            classify_reported(Some(ReportedState::Done), false),
            Status::Complete
        );
    }

    #[test]
    fn reported_working_does_not_override_stall() {
        let st = Status::classify(
            false,
            None,
            /*stalled*/ true,
            s(120),
            true,
            true,
            false,
            Some(ReportedState::Working),
        );
        assert_eq!(st, Status::Stalled);
    }

    #[test]
    fn reported_working_maps_to_thinking_when_not_stalled() {
        assert_eq!(
            classify_reported(Some(ReportedState::Working), false),
            Status::Thinking
        );
    }

    #[test]
    fn reported_waiting_maps_to_waiting() {
        assert_eq!(
            classify_reported(Some(ReportedState::Waiting), false),
            Status::Waiting
        );
    }

    #[test]
    fn no_reported_state_falls_back_to_heuristic() {
        assert_eq!(classify_reported(None, false), Status::Thinking);
    }

    #[test]
    fn priority_ordering_matches_spec() {
        assert!(Status::Stalled.priority() > Status::Question.priority());
        assert!(Status::Question.priority() > Status::Waiting.priority());
        assert!(Status::Waiting.priority() > Status::Thinking.priority());
        assert!(Status::Thinking.priority() > Status::Complete.priority());
        assert!(Status::Complete.priority() > Status::Idle.priority());
    }

    #[test]
    fn glyphs_match_design_tokens() {
        assert_eq!(Status::Question.glyph(), '?');
        assert_eq!(Status::Stalled.glyph(), '!');
        assert_eq!(Status::Waiting.glyph(), '…');
        assert_eq!(Status::Thinking.glyph(), '⠋');
        assert_eq!(Status::Complete.glyph(), '✓');
        assert_eq!(Status::Idle.glyph(), '·');
    }

    #[test]
    fn live_states_are_thinking_and_waiting() {
        assert!(Status::Thinking.is_live());
        assert!(Status::Waiting.is_live());
        assert!(!Status::Question.is_live());
        assert!(!Status::Stalled.is_live());
        assert!(!Status::Complete.is_live());
        assert!(!Status::Idle.is_live());
    }

    #[test]
    fn awaiting_tool_outranks_everything_when_pty_quiet() {
        // PTY has been idle for 5s — strong signal that the pending tool
        // really is parked on a permission prompt rather than running.
        assert_eq!(
            Status::classify(
                true,
                Some(StoppedKind::Complete),
                true,
                s(5),
                true,
                true,
                true,
                None
            ),
            Status::Question
        );
    }

    #[test]
    fn awaiting_tool_suppressed_when_pty_active() {
        // Pending tool_use is >3s old, but the PTY is actively streaming
        // output (e.g. an Agent subagent or a long Bash run). Don't paint
        // the false-positive `?` — show the live thinking spinner instead.
        assert_eq!(
            Status::classify(true, None, false, s(0), true, true, false, None),
            Status::Thinking
        );
        assert_eq!(
            Status::classify(true, None, false, s(1), true, true, false, None),
            Status::Thinking
        );
    }

    #[test]
    fn awaiting_tool_not_suppressed_when_pty_unknown() {
        // `None` means we have no PTY recency data (e.g. session just
        // attached, no output yet). That's not positive evidence the
        // agent is working, so the permission heuristic must still fire.
        assert_eq!(
            Status::classify(true, None, false, None, true, true, false, None),
            Status::Question
        );
    }

    #[test]
    fn awaiting_answer_not_suppressed_by_active_pty() {
        // An explicit AskUserQuestion / trailing-? must still surface as
        // Question even if the PTY is briefly active — the agent has
        // genuinely stopped its turn.
        assert_eq!(
            Status::classify(
                false,
                Some(StoppedKind::AwaitingAnswer),
                false,
                s(0),
                true,
                true,
                true,
                None
            ),
            Status::Question
        );
    }

    #[test]
    fn awaiting_answer_maps_to_question() {
        assert_eq!(
            Status::classify(
                false,
                Some(StoppedKind::AwaitingAnswer),
                false,
                s(1),
                true,
                true,
                true,
                None
            ),
            Status::Question
        );
    }

    #[test]
    fn stopped_complete_maps_to_complete() {
        assert_eq!(
            Status::classify(
                false,
                Some(StoppedKind::Complete),
                false,
                s(1),
                true,
                true,
                true,
                None
            ),
            Status::Complete
        );
    }

    #[test]
    fn stalled_outranks_running_recency() {
        // With the PTY quiet (30s since last output) AND the JSONL stalled,
        // the workspace really is stuck — stalled outranks the Waiting
        // recency path.
        assert_eq!(
            Status::classify(false, None, true, s(30), true, true, true, None),
            Status::Stalled
        );
    }

    #[test]
    fn stalled_suppressed_when_pty_active() {
        // The JSONL stall heuristic fires after 60s of no log growth, but a
        // long thinking/streaming phase (e.g. digesting a freshly-loaded
        // skill body) writes nothing to the JSONL while the terminal streams
        // the spinner sub-second. An active PTY proves the agent is alive, so
        // the stall must be suppressed — mirroring the `awaiting_tool`
        // suppression. Show the live Thinking spinner instead of `!`.
        assert_eq!(
            Status::classify(false, None, true, s(0), true, true, true, None),
            Status::Thinking
        );
        assert_eq!(
            Status::classify(false, None, true, s(1), true, true, true, None),
            Status::Thinking
        );
    }

    #[test]
    fn running_under_30s_is_thinking() {
        assert_eq!(
            Status::classify(false, None, false, s(0), true, true, false, None),
            Status::Thinking
        );
        assert_eq!(
            Status::classify(false, None, false, s(29), true, true, false, None),
            Status::Thinking
        );
    }

    #[test]
    fn running_over_30s_is_waiting() {
        assert_eq!(
            Status::classify(false, None, false, s(30), true, true, false, None),
            Status::Waiting
        );
        assert_eq!(
            Status::classify(false, None, false, s(3600), true, true, false, None),
            Status::Waiting
        );
    }

    #[test]
    fn no_session_maps_to_idle_regardless_of_prior() {
        assert_eq!(
            Status::classify(false, None, false, None, false, false, true, None),
            Status::Idle
        );
        assert_eq!(
            Status::classify(false, None, false, None, false, false, false, None),
            Status::Idle
        );
    }

    #[test]
    fn running_but_never_prompted_is_idle() {
        // `s(0)` here proves the never-prompted gate beats the recency path:
        // recent activity would otherwise classify as Thinking, but with no
        // recorded user prompt the session must read Idle, not the spinner.
        assert_eq!(
            Status::classify(false, None, false, s(0), true, false, false, None),
            Status::Idle
        );
    }

    #[test]
    fn running_but_never_prompted_is_idle_when_pty_unknown() {
        assert_eq!(
            Status::classify(false, None, false, None, true, false, false, None),
            Status::Idle
        );
    }

    #[test]
    fn never_prompted_does_not_override_higher_priority_states() {
        // The not-prompted gate sits below the early returns, so a stall,
        // permission prompt, or completion still wins even with prompted=false.
        // Stall uses a PTY-quiet recency (30s) — an active PTY would suppress
        // the stall (see `stalled_suppressed_when_pty_active`).
        assert_eq!(
            Status::classify(false, None, true, s(30), true, false, false, None),
            Status::Stalled
        );
        assert_eq!(
            Status::classify(true, None, false, s(5), true, false, false, None),
            Status::Question
        );
        assert_eq!(
            Status::classify(
                false,
                Some(StoppedKind::Complete),
                false,
                None,
                true,
                false,
                false,
                None
            ),
            Status::Complete
        );
    }

    #[test]
    fn running_and_prompted_still_thinking_when_recent() {
        // Regression: once the user has prompted, recent activity is Thinking.
        assert_eq!(
            Status::classify(false, None, false, s(0), true, true, false, None),
            Status::Thinking
        );
    }
}
