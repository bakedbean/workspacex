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
    /// Sort key. Higher = more urgent; used by both repo noise scoring
    /// and within-section ordering.
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
    pub fn classify(
        awaiting_tool: bool,
        stopped_kind: Option<StoppedKind>,
        stalled: bool,
        seconds_since_activity: Option<u64>,
        session_running: bool,
        user_has_prompted: bool,
        has_prior_session: bool,
    ) -> Self {
        // Consumed in Task 2; no behavior change yet.
        let _ = user_has_prompted;
        // The `awaiting_tool` heuristic flags any non-question tool_use
        // pending >3s as a permission prompt — but if the PTY is still
        // streaming output the agent is clearly mid-work, so suppress the
        // false-positive `?`. Catches long `Bash` runs and any other
        // tool whose process pipes activity to the parent PTY.
        let pty_active = matches!(seconds_since_activity, Some(s) if s < 2);
        if awaiting_tool && !pty_active {
            return Status::Question;
        }
        match stopped_kind {
            Some(StoppedKind::AwaitingAnswer) => return Status::Question,
            Some(StoppedKind::Complete) => return Status::Complete,
            None => {}
        }
        if stalled {
            return Status::Stalled;
        }
        if session_running {
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

    fn s(secs: u64) -> Option<u64> {
        Some(secs)
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
            Status::classify(true, Some(StoppedKind::Complete), true, s(5), true, true, true),
            Status::Question
        );
    }

    #[test]
    fn awaiting_tool_suppressed_when_pty_active() {
        // Pending tool_use is >3s old, but the PTY is actively streaming
        // output (e.g. an Agent subagent or a long Bash run). Don't paint
        // the false-positive `?` — show the live thinking spinner instead.
        assert_eq!(
            Status::classify(true, None, false, s(0), true, true, false),
            Status::Thinking
        );
        assert_eq!(
            Status::classify(true, None, false, s(1), true, true, false),
            Status::Thinking
        );
    }

    #[test]
    fn awaiting_tool_not_suppressed_when_pty_unknown() {
        // `None` means we have no PTY recency data (e.g. session just
        // attached, no output yet). That's not positive evidence the
        // agent is working, so the permission heuristic must still fire.
        assert_eq!(
            Status::classify(true, None, false, None, true, true, false),
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
                true
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
                true
            ),
            Status::Question
        );
    }

    #[test]
    fn stopped_complete_maps_to_complete() {
        assert_eq!(
            Status::classify(false, Some(StoppedKind::Complete), false, s(1), true, true, true),
            Status::Complete
        );
    }

    #[test]
    fn stalled_outranks_running_recency() {
        assert_eq!(
            Status::classify(false, None, true, s(0), true, true, true),
            Status::Stalled
        );
    }

    #[test]
    fn running_under_30s_is_thinking() {
        assert_eq!(
            Status::classify(false, None, false, s(0), true, true, false),
            Status::Thinking
        );
        assert_eq!(
            Status::classify(false, None, false, s(29), true, true, false),
            Status::Thinking
        );
    }

    #[test]
    fn running_over_30s_is_waiting() {
        assert_eq!(
            Status::classify(false, None, false, s(30), true, true, false),
            Status::Waiting
        );
        assert_eq!(
            Status::classify(false, None, false, s(3600), true, true, false),
            Status::Waiting
        );
    }

    #[test]
    fn no_session_maps_to_idle_regardless_of_prior() {
        assert_eq!(
            Status::classify(false, None, false, None, false, false, true),
            Status::Idle
        );
        assert_eq!(
            Status::classify(false, None, false, None, false, false, false),
            Status::Idle
        );
    }
}
