//! Claude Code status integration: hooks wired via `--settings`, and parsing of
//! Claude hook event payloads into `ReportedState`.

use super::{SpawnWiring, StatusIntegration};
use crate::data::store::ReportedState;
use std::path::Path;

pub struct ClaudeStatus;

impl StatusIntegration for ClaudeStatus {
    /// Mapping (see the design spec's Fidelity findings):
    /// - `UserPromptSubmit`                            -> Working
    /// - `PreToolUse` for AskUserQuestion/ExitPlanMode -> Blocked
    /// - `Notification` permission_prompt              -> Blocked
    /// - `Notification` idle_prompt                    -> Waiting
    /// - `Stop` with a non-empty `background_tasks`     -> Busy (parked, not done)
    /// - `Stop` with a `?`-terminated last message     -> Blocked (best-effort)
    /// - `Stop` otherwise                              -> Done
    fn parse_event(&self, json: &serde_json::Value) -> Option<ReportedState> {
        let event = json.get("hook_event_name")?.as_str()?;
        match event {
            "UserPromptSubmit" => Some(ReportedState::Working),
            "PreToolUse" => {
                let tool = json.get("tool_name").and_then(|v| v.as_str()).unwrap_or("");
                if matches!(tool, "AskUserQuestion" | "ExitPlanMode") {
                    Some(ReportedState::Blocked)
                } else {
                    None
                }
            }
            "Notification" => match json.get("notification_type").and_then(|v| v.as_str()) {
                Some("permission_prompt") => Some(ReportedState::Blocked),
                Some("idle_prompt") => Some(ReportedState::Waiting),
                _ => None,
            },
            "Stop" => {
                // A `Stop` fires whenever the main agent's turn ends — including
                // when it dispatched a background subagent/task and yielded to
                // wait for it. `background_tasks` (Claude Code v2.1.145+) is
                // non-empty in exactly that case, distinguishing "parked, will
                // auto-resume" from a genuine completion. Older Claude omits the
                // field; treat absent/empty as "nothing pending". Reported as
                // Busy so the dashboard keeps showing work-in-progress rather
                // than flipping to ✓ while a subagent runs in its own session.
                let background_pending = json
                    .get("background_tasks")
                    .and_then(|v| v.as_array())
                    .is_some_and(|tasks| !tasks.is_empty());
                if background_pending {
                    return Some(ReportedState::Busy);
                }
                // `last_assistant_message` is observed but undocumented; degrade
                // to Done when absent. A trailing `?` is the best-effort
                // prose-question signal, otherwise the turn read as a completion.
                let ends_with_q = json
                    .get("last_assistant_message")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim_end().ends_with('?'))
                    .unwrap_or(false);
                Some(if ends_with_q {
                    ReportedState::Blocked
                } else {
                    ReportedState::Done
                })
            }
            _ => None,
        }
    }

    /// Every Claude hook payload carries `session_id`, but only the
    /// main-conversation lifecycle events are trusted here: `SessionStart`
    /// (fires on startup, `--resume`, `/clear` and compaction — so the id is
    /// captured before the user types, and re-captured when `/clear` moves
    /// the instance to a new conversation), `UserPromptSubmit` and `Stop`.
    /// `PreToolUse`/`Notification` also fire inside subagents and are left
    /// out so a subagent's payload can never relabel its parent.
    fn session_id_from_event(&self, json: &serde_json::Value) -> Option<String> {
        let event = json.get("hook_event_name")?.as_str()?;
        if !matches!(event, "SessionStart" | "UserPromptSubmit" | "Stop") {
            return None;
        }
        let id = json.get("session_id")?.as_str()?.trim();
        (!id.is_empty()).then(|| id.to_string())
    }

    fn spawn_wiring(&self, wsx_bin: &Path, fast_mode: bool) -> Option<SpawnWiring> {
        Some(SpawnWiring {
            args: vec!["--settings".to_string(), settings_json(fast_mode, wsx_bin)],
        })
    }
}

/// Build the `--settings` JSON string for a Claude spawn. Always includes the
/// status hooks (each calling `wsx status from-hook --agent claude`); includes
/// `"fastMode": true` only when `fast_mode` is set.
///
/// `SessionStart` is wired purely for session-id capture (it maps to no
/// status); it must stay silent on stdout, since Claude injects a
/// `SessionStart` hook's stdout into the conversation as context.
fn settings_json(fast_mode: bool, wsx_bin: &Path) -> String {
    let cmd = format!("{} status from-hook --agent claude", shell_quote(wsx_bin));
    let entry = |ev: &str| {
        (
            ev.to_string(),
            serde_json::json!([{ "hooks": [{ "type": "command", "command": cmd }] }]),
        )
    };
    let hooks: serde_json::Map<String, serde_json::Value> = [
        "SessionStart",
        "UserPromptSubmit",
        "PreToolUse",
        "Notification",
        "Stop",
    ]
    .into_iter()
    .map(entry)
    .collect();

    let mut root = serde_json::Map::new();
    if fast_mode {
        root.insert("fastMode".into(), serde_json::Value::Bool(true));
    }
    root.insert("hooks".into(), serde_json::Value::Object(hooks));
    serde_json::Value::Object(root).to_string()
}

/// Minimal POSIX single-quote escaping for a path embedded in a hook command.
fn shell_quote(p: &Path) -> String {
    format!("'{}'", p.to_string_lossy().replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(json: serde_json::Value) -> Option<ReportedState> {
        ClaudeStatus.parse_event(&json)
    }

    #[test]
    fn user_prompt_submit_is_working() {
        assert_eq!(
            ev(serde_json::json!({"hook_event_name": "UserPromptSubmit"})),
            Some(ReportedState::Working)
        );
    }

    #[test]
    fn pretooluse_question_tools_are_blocked() {
        for tool in ["AskUserQuestion", "ExitPlanMode"] {
            assert_eq!(
                ev(serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": tool})),
                Some(ReportedState::Blocked)
            );
        }
        assert_eq!(
            ev(serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": "Bash"})),
            None
        );
    }

    #[test]
    fn notification_types_map_or_ignore() {
        assert_eq!(
            ev(
                serde_json::json!({"hook_event_name": "Notification", "notification_type": "permission_prompt"})
            ),
            Some(ReportedState::Blocked)
        );
        assert_eq!(
            ev(
                serde_json::json!({"hook_event_name": "Notification", "notification_type": "idle_prompt"})
            ),
            Some(ReportedState::Waiting)
        );
        assert_eq!(
            ev(
                serde_json::json!({"hook_event_name": "Notification", "notification_type": "auth_success"})
            ),
            None
        );
    }

    #[test]
    fn stop_distinguishes_question_from_completion() {
        assert_eq!(
            ev(
                serde_json::json!({"hook_event_name": "Stop", "last_assistant_message": "All done."})
            ),
            Some(ReportedState::Done)
        );
        assert_eq!(
            ev(
                serde_json::json!({"hook_event_name": "Stop", "last_assistant_message": "Which option do you prefer?"})
            ),
            Some(ReportedState::Blocked)
        );
        assert_eq!(
            ev(serde_json::json!({"hook_event_name": "Stop"})),
            Some(ReportedState::Done)
        );
    }

    #[test]
    fn stop_with_background_tasks_is_busy() {
        // A subagent (or any task) still in flight at turn end means the
        // session is parked and will auto-resume — not done. Reported as Busy
        // even though `last_assistant_message` reads like a completion.
        assert_eq!(
            ev(serde_json::json!({
                "hook_event_name": "Stop",
                "last_assistant_message": "Waiting for the implementer subagent to finish.",
                "background_tasks": [{"id": "t1", "type": "subagent", "status": "running"}]
            })),
            Some(ReportedState::Busy)
        );
    }

    #[test]
    fn stop_with_empty_background_tasks_is_done() {
        // Empty array (v2.1.145+, nothing in flight) reads as a real
        // completion, same as the absent-field case (older Claude Code).
        assert_eq!(
            ev(serde_json::json!({
                "hook_event_name": "Stop",
                "last_assistant_message": "All done.",
                "background_tasks": []
            })),
            Some(ReportedState::Done)
        );
    }

    #[test]
    fn unknown_event_is_ignored() {
        assert_eq!(
            ev(serde_json::json!({"hook_event_name": "SubagentStop"})),
            None
        );
        assert_eq!(ev(serde_json::json!({})), None);
    }

    #[test]
    fn spawn_wiring_emits_settings_with_hooks_and_bin() {
        let w = ClaudeStatus
            .spawn_wiring(Path::new("/usr/local/bin/wsx"), true)
            .unwrap();
        assert_eq!(w.args[0], "--settings");
        let v: serde_json::Value = serde_json::from_str(&w.args[1]).unwrap();
        assert_eq!(v["fastMode"], serde_json::json!(true));
        assert!(v["hooks"]["Stop"].is_array());
        let cmd = v["hooks"]["Stop"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(cmd.contains("/usr/local/bin/wsx"));
        assert!(cmd.ends_with("status from-hook --agent claude"));
    }

    #[test]
    fn spawn_wiring_hooks_session_start_for_id_capture() {
        let w = ClaudeStatus
            .spawn_wiring(Path::new("/usr/local/bin/wsx"), false)
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&w.args[1]).unwrap();
        let cmd = v["hooks"]["SessionStart"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(cmd.ends_with("status from-hook --agent claude"));
    }

    #[test]
    fn session_id_captured_from_main_lifecycle_events_only() {
        let sid = |ev: &str| {
            ClaudeStatus.session_id_from_event(
                &serde_json::json!({"hook_event_name": ev, "session_id": "abc-123"}),
            )
        };
        for ev in ["SessionStart", "UserPromptSubmit", "Stop"] {
            assert_eq!(sid(ev).as_deref(), Some("abc-123"), "{ev}");
        }
        for ev in ["PreToolUse", "Notification", "SubagentStop", "PostToolUse"] {
            assert_eq!(sid(ev), None, "{ev} may come from a subagent");
        }
        assert_eq!(
            ClaudeStatus.session_id_from_event(
                &serde_json::json!({"hook_event_name": "Stop", "session_id": "  "})
            ),
            None
        );
        assert_eq!(
            ClaudeStatus.session_id_from_event(&serde_json::json!({"hook_event_name": "Stop"})),
            None
        );
    }

    #[test]
    fn spawn_wiring_omits_fastmode_when_false() {
        let w = ClaudeStatus
            .spawn_wiring(Path::new("/usr/local/bin/wsx"), false)
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&w.args[1]).unwrap();
        assert!(v.get("fastMode").is_none());
        assert!(v["hooks"]["Notification"].is_array());
    }
}
