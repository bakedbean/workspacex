// bell — extracted from src/app.rs (see docs/superpowers/specs/2026-05-25-app-rs-refactor-design.md)

use crate::app::activity::ActivityState;
use crate::data::store::Store;
use std::io::Write;

#[derive(Debug, Clone, Copy)]
pub enum BellPattern {
    Off,
    Single,
    Double,
    Triple,
}

impl BellPattern {
    fn from_setting(s: Option<&str>) -> Option<Self> {
        match s {
            Some("off") | Some("false") | Some("0") => Some(BellPattern::Off),
            Some("single") => Some(BellPattern::Single),
            Some("double") => Some(BellPattern::Double),
            Some("triple") => Some(BellPattern::Triple),
            _ => None, // caller uses its own default
        }
    }
}

/// Window after wsx starts during which a first-observation of an
/// alertable workspace is treated as cold-start noise (visual marker
/// only, no bell). Sized to comfortably cover the 2s tail-loop tick so
/// every initial scan lands inside the window.
pub const COLD_START_WINDOW: std::time::Duration = std::time::Duration::from_secs(3);

/// Decide what to do when a workspace's activity classification changes.
/// Returns `(mark_attention, fire_bell)`.
///
/// During the cold-start window the bell is suppressed on the very first
/// observation of a workspace (`prev.is_none()`), so wsx doesn't ring
/// once per workspace at launch when several agents were already waiting
/// before startup. The visual attention marker still fires so the
/// dashboard reflects current state. Outside the window a first
/// observation rings normally — a workspace that just appeared
/// (newly created or freshly imported) and is already alertable is
/// something the user wants to be notified about.
pub fn alert_decision(
    prev: Option<ActivityState>,
    activity: ActivityState,
    notifications_on: bool,
    is_cold_start: bool,
) -> (bool, bool) {
    if !notifications_on || !activity.is_alertable() || prev == Some(activity) {
        return (false, false);
    }
    let fire_bell = prev.is_some() || !is_cold_start;
    (true, fire_bell)
}

/// Pick the bell pattern for a given alertable state. Reads per-state
/// overrides from the store, falling back to sensible defaults.
fn bell_pattern_for(state: ActivityState, store: &Store) -> BellPattern {
    let (key, default_pattern) = match state {
        ActivityState::AwaitingAnswer => ("notification_bell_question", BellPattern::Double),
        ActivityState::Complete => ("notification_bell_complete", BellPattern::Single),
        ActivityState::Awaiting => ("notification_bell_permission", BellPattern::Single),
        ActivityState::Stalled => ("notification_bell_stalled", BellPattern::Triple),
        // Non-alertable states never call fire_bell, but be safe.
        _ => return BellPattern::Off,
    };
    let stored = store.get_setting(key).ok().flatten();
    BellPattern::from_setting(stored.as_deref()).unwrap_or(default_pattern)
}

/// Emit a terminal-bell pattern for an alertable state. Multi-bell
/// patterns spawn a detached thread to space the writes (~120ms apart)
/// so the engine event loop isn't blocked.
///
/// Residual race: the first bell fires synchronously outside ratatui's
/// `draw()` closure (see the run loop's drain of `pending_bells`), but
/// the 2nd/3rd bells in a Double/Triple sequence land 120ms+ later,
/// which can overlap with subsequent frame flushes. `\x07` mid-escape
/// is undefined per the VT spec but is silently dropped by iTerm2 and
/// other modern terminals; visible corruption has not been observed.
/// The fully race-free alternative is a synchronized bell worker
/// coordinating with the TUI backend — non-trivial refactor for a
/// theoretical issue. Reassess if real-world corruption appears.
pub fn fire_bell(state: ActivityState, store: &Store) {
    let pattern = bell_pattern_for(state, store);
    let count = match pattern {
        BellPattern::Off => return,
        BellPattern::Single => 1,
        BellPattern::Double => 2,
        BellPattern::Triple => 3,
    };
    if count == 1 {
        let _ = std::io::stdout().write_all(b"\x07");
        let _ = std::io::stdout().flush();
        return;
    }
    std::thread::spawn(move || {
        for i in 0..count {
            if i > 0 {
                std::thread::sleep(std::time::Duration::from_millis(120));
            }
            let _ = std::io::stdout().write_all(b"\x07");
            let _ = std::io::stdout().flush();
        }
    });
}

#[cfg(test)]
mod bell_tests {
    use super::*;

    #[test]
    fn bell_pattern_off_for_non_alertable() {
        let store = crate::data::store::Store::open_in_memory().expect("in-memory store");
        assert!(matches!(
            bell_pattern_for(ActivityState::Active, &store),
            BellPattern::Off
        ));
    }

    #[test]
    fn bell_pattern_defaults_match_spec() {
        let store = crate::data::store::Store::open_in_memory().expect("in-memory store");
        assert!(matches!(
            bell_pattern_for(ActivityState::AwaitingAnswer, &store),
            BellPattern::Double
        ));
        assert!(matches!(
            bell_pattern_for(ActivityState::Complete, &store),
            BellPattern::Single
        ));
        assert!(matches!(
            bell_pattern_for(ActivityState::Awaiting, &store),
            BellPattern::Single
        ));
        assert!(matches!(
            bell_pattern_for(ActivityState::Stalled, &store),
            BellPattern::Triple
        ));
    }

    #[test]
    fn bell_pattern_override_off_suppresses_default() {
        let store = crate::data::store::Store::open_in_memory().expect("in-memory store");
        store
            .set_setting("notification_bell_question", "off")
            .unwrap();
        assert!(matches!(
            bell_pattern_for(ActivityState::AwaitingAnswer, &store),
            BellPattern::Off
        ));
    }

    #[test]
    fn bell_pattern_override_single_replaces_default_double() {
        let store = crate::data::store::Store::open_in_memory().expect("in-memory store");
        store
            .set_setting("notification_bell_question", "single")
            .unwrap();
        assert!(matches!(
            bell_pattern_for(ActivityState::AwaitingAnswer, &store),
            BellPattern::Single
        ));
    }

    #[test]
    fn alert_decision_suppresses_bell_on_first_observation_during_cold_start() {
        // Cold start: prev is None, workspace already alertable.
        // Visual marker should light up, bell should stay silent.
        let (mark, ring) = alert_decision(None, ActivityState::AwaitingAnswer, true, true);
        assert!(mark, "visual marker must surface on first observation");
        assert!(!ring, "bell must NOT ring during cold start");
    }

    #[test]
    fn alert_decision_rings_on_first_observation_after_cold_start() {
        // A new workspace appears mid-session and is already alertable
        // (e.g. it raced ahead and asked a question before the tail loop
        // could record an intermediate Active). User wants to know.
        let (mark, ring) = alert_decision(None, ActivityState::AwaitingAnswer, true, false);
        assert!(mark);
        assert!(
            ring,
            "bell must ring for a fresh workspace after cold start"
        );
    }

    #[test]
    fn alert_decision_rings_on_transition_into_alertable() {
        // Active -> AwaitingAnswer: real mid-session transition, ring
        // regardless of cold-start window.
        for is_cold_start in [true, false] {
            let (mark, ring) = alert_decision(
                Some(ActivityState::Active),
                ActivityState::AwaitingAnswer,
                true,
                is_cold_start,
            );
            assert!(mark);
            assert!(ring, "transition with prev=Some must always ring");
        }
    }

    #[test]
    fn alert_decision_rings_on_transition_between_alertable_states() {
        // Complete -> Awaiting: permission prompt arrives before the user
        // replied to a prior end_turn. Both alertable, different — ring.
        let (mark, ring) = alert_decision(
            Some(ActivityState::Complete),
            ActivityState::Awaiting,
            true,
            false,
        );
        assert!(mark);
        assert!(ring);
    }

    #[test]
    fn alert_decision_silent_when_alertable_state_persists() {
        // Re-classifying as the same alertable state across polls must
        // not re-ring or re-mark.
        let (mark, ring) = alert_decision(
            Some(ActivityState::AwaitingAnswer),
            ActivityState::AwaitingAnswer,
            true,
            false,
        );
        assert!(!mark);
        assert!(!ring);
    }

    #[test]
    fn alert_decision_silent_for_non_alertable_target() {
        // Transition into Active or Idle is not an alert.
        let (mark, ring) = alert_decision(
            Some(ActivityState::Complete),
            ActivityState::Active,
            true,
            false,
        );
        assert!(!mark);
        assert!(!ring);
    }

    #[test]
    fn alert_decision_silent_when_notifications_off() {
        // Global notification kill switch suppresses everything, even
        // legitimate mid-session transitions.
        let (mark, ring) = alert_decision(
            Some(ActivityState::Active),
            ActivityState::AwaitingAnswer,
            false,
            false,
        );
        assert!(!mark);
        assert!(!ring);
    }
}
