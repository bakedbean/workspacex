// activity — extracted from src/app.rs (see docs/superpowers/specs/2026-05-25-app-rs-refactor-design.md)

use crate::app::StoppedKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityState {
    /// The agent has stopped its turn and is waiting for an answer
    /// from the user. Higher priority than PTY-recency states.
    AwaitingAnswer,
    /// The agent has stopped its turn with a completed task and is
    /// awaiting acknowledgment. Higher priority than PTY-recency states.
    Complete,
    /// A tool_use has been pending for ≥3s (almost always a permission
    /// prompt). Higher priority than `AwaitingAnswer` / `Complete`.
    Awaiting,
    /// < 2s since last PTY output.
    Active,
    /// 2–30s since last PTY output.
    Idle,
    /// Claude has stalled between turns: the JSONL log hasn't been
    /// appended for >60s, no tool_use is pending, and we've seen at
    /// least one stop_reason in this session. Alertable.
    Stalled,
    /// More than 30s since last PTY output but no JSONL stop signal.
    /// Retained for the recency column; does NOT drive the bell.
    Waiting,
    /// No session attached at all.
    Off,
}

impl ActivityState {
    /// States that should fire a bell + attention marker when entered.
    pub fn is_alertable(self) -> bool {
        matches!(
            self,
            ActivityState::AwaitingAnswer
                | ActivityState::Complete
                | ActivityState::Awaiting
                | ActivityState::Stalled
        )
    }
}

pub fn classify_activity(secs: Option<u64>) -> ActivityState {
    match secs {
        Some(s) if s < 2 => ActivityState::Active,
        Some(s) if s < 30 => ActivityState::Idle,
        Some(_) => ActivityState::Waiting,
        None => ActivityState::Off,
    }
}

/// Compute the activity state for a workspace, combining JSONL-derived
/// signals with PTY-output recency.
///
/// Priority: `Awaiting` (permission prompt) > `AwaitingAnswer` /
/// `Complete` (turn ended) > `Stalled` (mid-tool-chain quiet) >
/// PTY-recency > `Off`.
pub fn classify_activity_with_events(
    secs: Option<u64>,
    running: bool,
    awaiting: bool,
    stopped_kind: Option<StoppedKind>,
    stalled: bool,
) -> ActivityState {
    if awaiting {
        return ActivityState::Awaiting;
    }
    match stopped_kind {
        Some(StoppedKind::AwaitingAnswer) => return ActivityState::AwaitingAnswer,
        Some(StoppedKind::Complete) => return ActivityState::Complete,
        None => {}
    }
    if stalled {
        return ActivityState::Stalled;
    }
    if !running {
        return ActivityState::Off;
    }
    classify_activity(secs)
}

#[cfg(test)]
mod activity_classifier_tests {
    use super::*;

    #[test]
    fn awaiting_wins_over_stopped_over_recency() {
        // awaiting (permission) beats everything.
        assert_eq!(
            classify_activity_with_events(Some(0), true, true, Some(StoppedKind::Complete), false,),
            ActivityState::Awaiting
        );
        assert_eq!(
            classify_activity_with_events(Some(0), true, true, None, false),
            ActivityState::Awaiting
        );
        // stopped beats PTY recency.
        assert_eq!(
            classify_activity_with_events(Some(0), true, false, Some(StoppedKind::Complete), false,),
            ActivityState::Complete
        );
        assert_eq!(
            classify_activity_with_events(
                Some(0),
                true,
                false,
                Some(StoppedKind::AwaitingAnswer),
                false,
            ),
            ActivityState::AwaitingAnswer
        );
    }

    #[test]
    fn stopped_wins_over_stalled() {
        // If we have a terminal stop_reason waiting on the user, that
        // takes priority over the stall detector.
        assert_eq!(
            classify_activity_with_events(Some(0), true, false, Some(StoppedKind::Complete), true,),
            ActivityState::Complete
        );
        assert_eq!(
            classify_activity_with_events(
                Some(0),
                true,
                false,
                Some(StoppedKind::AwaitingAnswer),
                true,
            ),
            ActivityState::AwaitingAnswer
        );
    }

    #[test]
    fn stalled_wins_over_pty_recency() {
        // Stall detector fires before PTY-recency Active/Idle/Waiting.
        assert_eq!(
            classify_activity_with_events(Some(0), true, false, None, true),
            ActivityState::Stalled
        );
        assert_eq!(
            classify_activity_with_events(Some(60), true, false, None, true),
            ActivityState::Stalled
        );
    }

    #[test]
    fn no_session_is_off_even_if_running_false() {
        assert_eq!(
            classify_activity_with_events(None, false, false, None, false),
            ActivityState::Off
        );
        // Even with pty seconds, if running=false → Off.
        assert_eq!(
            classify_activity_with_events(Some(5), false, false, None, false),
            ActivityState::Off
        );
    }

    #[test]
    fn awaiting_fires_even_when_session_not_running() {
        // A pending tool_use is a strong signal regardless of pty state.
        assert_eq!(
            classify_activity_with_events(None, false, true, None, false),
            ActivityState::Awaiting
        );
    }

    #[test]
    fn pty_recency_drives_active_idle_waiting_when_no_event_signals() {
        assert_eq!(
            classify_activity_with_events(Some(0), true, false, None, false),
            ActivityState::Active
        );
        assert_eq!(
            classify_activity_with_events(Some(10), true, false, None, false),
            ActivityState::Idle
        );
        assert_eq!(
            classify_activity_with_events(Some(60), true, false, None, false),
            ActivityState::Waiting
        );
    }

    #[test]
    fn is_alertable_includes_stopped_awaiting_and_stalled() {
        assert!(ActivityState::AwaitingAnswer.is_alertable());
        assert!(ActivityState::Complete.is_alertable());
        assert!(ActivityState::Awaiting.is_alertable());
        assert!(ActivityState::Stalled.is_alertable());
        assert!(!ActivityState::Active.is_alertable());
        assert!(!ActivityState::Idle.is_alertable());
        assert!(!ActivityState::Waiting.is_alertable());
        assert!(!ActivityState::Off.is_alertable());
    }
}
