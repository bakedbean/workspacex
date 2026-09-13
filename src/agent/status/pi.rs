//! pi status integration: session identity only. pi has no lifecycle hooks
//! or notify program for *status*, so `parse_event` stays `None` (tier 1 +
//! tier 3 carry status). What it does have is an extension API, and the
//! extension wsx loads on every spawn (`agent::pi_extension`) reports each
//! `session_start` through `wsx status from-notify --agent pi`.

use super::StatusIntegration;
use crate::data::store::ReportedState;

pub struct PiStatus;

impl StatusIntegration for PiStatus {
    fn parse_event(&self, _json: &serde_json::Value) -> Option<ReportedState> {
        None
    }

    /// `{type: "session_start", reason, session_id}` from the wsx extension.
    /// Every reason counts: `startup` records the session a fresh or resumed
    /// pi actually opened, `new`/`resume`/`fork` follow the user's switch.
    fn session_id_from_event(&self, json: &serde_json::Value) -> Option<String> {
        if json.get("type").and_then(|v| v.as_str()) != Some("session_start") {
            return None;
        }
        let id = json.get("session_id")?.as_str()?.trim();
        (!id.is_empty()).then(|| id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_id_captured_from_session_start_only() {
        let sid = |json: serde_json::Value| PiStatus.session_id_from_event(&json);
        assert_eq!(
            sid(serde_json::json!({"type": "session_start", "reason": "new", "session_id": "abc"}))
                .as_deref(),
            Some("abc")
        );
        assert_eq!(
            sid(serde_json::json!({"type": "other", "session_id": "abc"})),
            None
        );
        assert_eq!(sid(serde_json::json!({"type": "session_start"})), None);
        assert_eq!(
            PiStatus.parse_event(&serde_json::json!({"type": "session_start"})),
            None,
            "pi reports identity, not status"
        );
    }
}
