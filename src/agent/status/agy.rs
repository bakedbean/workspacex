//! Antigravity CLI (`agy`) status integration. agy runs lifecycle hooks from a
//! `hooks.json` in any `.agents/` directory between the cwd and the repo root,
//! piping a camelCase JSON payload on stdin. wsx merges one named hook entry
//! into the worktree's `.agents/hooks.json` at spawn
//! (`pty::workspace_prep::prepare_agy_workspace`), each handler calling
//! `wsx status from-hook --agent agy`.
//!
//! `PreToolUse` is deliberately never wired: agy treats a pre-tool hook's
//! stdout as a permission decision, and `from-hook` prints nothing, which agy
//! reads as a denial. Observed live on agy 1.5.18: a `{}` reply blocked
//! `run_command` outright.

use super::StatusIntegration;
use crate::data::store::ReportedState;
use std::path::Path;

/// The key wsx owns inside `.agents/hooks.json`. agy merges every named entry,
/// so a repo's own hooks sit beside this one untouched.
pub const HOOK_NAME: &str = "wsx-status";

pub struct AgyStatus;

/// agy events that mark the main conversation's turn boundaries.
fn event(json: &serde_json::Value) -> Option<&str> {
    json.get("hookEventName")?.as_str()
}

/// `PreInvocation` fires before every model call; only the first of a turn
/// (`invocationNum` 0) marks the turn starting.
fn is_turn_start(json: &serde_json::Value) -> bool {
    event(json) == Some("PreInvocation")
        && json.get("invocationNum").and_then(|v| v.as_u64()) == Some(0)
}

impl StatusIntegration for AgyStatus {
    /// - `PreInvocation` #0                     -> Working
    /// - `Stop` with `fullyIdle: false`         -> Busy (background work pending)
    /// - `Stop` with `terminationReason: ERROR` -> Blocked (needs the user)
    /// - `Stop` otherwise                       -> Done
    fn parse_event(&self, json: &serde_json::Value) -> Option<ReportedState> {
        if is_turn_start(json) {
            return Some(ReportedState::Working);
        }
        if event(json) != Some("Stop") {
            return None;
        }
        if json.get("fullyIdle").and_then(|v| v.as_bool()) == Some(false) {
            return Some(ReportedState::Busy);
        }
        Some(
            match json.get("terminationReason").and_then(|v| v.as_str()) {
                Some("ERROR") => ReportedState::Blocked,
                _ => ReportedState::Done,
            },
        )
    }

    /// Every payload carries `conversationId`, the argument
    /// `agy --conversation <id>` resumes. Trusted on the same lifecycle events
    /// Claude's integration trusts (session start, turn start, turn end).
    fn session_id_from_event(&self, json: &serde_json::Value) -> Option<String> {
        let trusted = matches!(event(json), Some("SessionStart" | "Stop")) || is_turn_start(json);
        if !trusted {
            return None;
        }
        let id = json.get("conversationId")?.as_str()?.trim();
        (!id.is_empty()).then(|| id.to_string())
    }
}

/// The value wsx writes under [`HOOK_NAME`] in `.agents/hooks.json`.
/// `SessionStart`, `PreInvocation` and `Stop` take a flat handler list; agy
/// accepts empty stdout from all three.
pub fn hook_entry(wsx_bin: &Path) -> serde_json::Value {
    let command = format!(
        "'{}' status from-hook --agent agy",
        wsx_bin.to_string_lossy().replace('\'', r"'\''")
    );
    let handlers = serde_json::json!([{ "type": "command", "command": command, "timeout": 10 }]);
    serde_json::json!({
        "SessionStart": handlers,
        "PreInvocation": handlers,
        "Stop": handlers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(v: serde_json::Value) -> Option<ReportedState> {
        AgyStatus.parse_event(&v)
    }

    #[test]
    fn first_invocation_of_a_turn_is_working() {
        assert_eq!(
            ev(json!({"hookEventName": "PreInvocation", "invocationNum": 0})),
            Some(ReportedState::Working)
        );
        assert_eq!(
            ev(json!({"hookEventName": "PreInvocation", "invocationNum": 3})),
            None
        );
        assert_eq!(ev(json!({"hookEventName": "SessionStart"})), None);
        assert_eq!(ev(json!({"hookEventName": "PostInvocation"})), None);
    }

    #[test]
    fn stop_maps_idle_error_and_completion() {
        let stop = |extra: serde_json::Value| {
            let mut v = json!({"hookEventName": "Stop", "fullyIdle": true,
                               "terminationReason": "NO_TOOL_CALL"});
            v.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            ev(v)
        };
        assert_eq!(stop(json!({})), Some(ReportedState::Done));
        assert_eq!(stop(json!({"fullyIdle": false})), Some(ReportedState::Busy));
        assert_eq!(
            stop(json!({"terminationReason": "ERROR"})),
            Some(ReportedState::Blocked)
        );
        assert_eq!(
            stop(json!({"terminationReason": "USER_CANCELED"})),
            Some(ReportedState::Done)
        );
    }

    #[test]
    fn conversation_id_captured_from_lifecycle_events_only() {
        let sid = |v: serde_json::Value| AgyStatus.session_id_from_event(&v);
        for v in [
            json!({"hookEventName": "SessionStart", "conversationId": "c1"}),
            json!({"hookEventName": "PreInvocation", "invocationNum": 0, "conversationId": "c1"}),
            json!({"hookEventName": "Stop", "conversationId": "c1"}),
        ] {
            assert_eq!(sid(v).as_deref(), Some("c1"));
        }
        assert_eq!(
            sid(json!({"hookEventName": "PostToolUse", "conversationId": "c1"})),
            None
        );
        assert_eq!(
            sid(json!({"hookEventName": "Stop", "conversationId": " "})),
            None
        );
    }

    #[test]
    fn hook_entry_never_wires_pre_tool_use() {
        let entry = hook_entry(Path::new("/opt/it's/wsx"));
        let obj = entry.as_object().unwrap();
        assert!(!obj.contains_key("PreToolUse"));
        assert_eq!(
            entry["Stop"][0]["command"],
            r"'/opt/it'\''s/wsx' status from-hook --agent agy"
        );
    }
}
