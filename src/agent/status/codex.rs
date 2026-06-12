//! Codex status integration. Codex's `hooks.*` system is unusable for wsx
//! worktrees at v0.137 (silently gated behind project trust), but its `notify`
//! program fires reliably on `agent-turn-complete` with no trust gate and is
//! injectable via `-c`. We wire `notify` to call back into
//! `wsx status from-notify`, mapping the turn-end event to Done/Blocked exactly
//! as Claude's `Stop` hook does. Turn-start / working stays on the tier-3 JSONL
//! heuristic; the `fresh_reported` gate hands off between them.

use super::{SpawnWiring, StatusIntegration};
use crate::data::store::ReportedState;
use std::path::Path;

pub struct CodexStatus;

impl StatusIntegration for CodexStatus {
    /// Codex's `notify` fires only `agent-turn-complete`. Map it like Claude's
    /// `Stop`: a `?`-terminated final message reads as a blocking prose
    /// question, otherwise the turn completed. The payload uses kebab-case keys
    /// and arrives via argv (see `from-notify`), but `parse_event` only sees the
    /// already-parsed JSON value.
    fn parse_event(&self, json: &serde_json::Value) -> Option<ReportedState> {
        if json.get("type").and_then(|v| v.as_str()) != Some("agent-turn-complete") {
            return None;
        }
        let ends_with_q = json
            .get("last-assistant-message")
            .and_then(|v| v.as_str())
            .map(|s| s.trim_end().ends_with('?'))
            .unwrap_or(false);
        Some(if ends_with_q {
            ReportedState::Blocked
        } else {
            ReportedState::Done
        })
    }

    // `_fast_mode` is unused: Codex has no fast-mode equivalent (that flag is a
    // Claude `--settings` concept). The `-c notify` wiring is the same regardless.
    fn spawn_wiring(&self, wsx_bin: &Path, _fast_mode: bool) -> Option<SpawnWiring> {
        // Codex appends the JSON payload as the final argv element, so the
        // invoked command becomes:
        //   <wsx_bin> status from-notify --agent codex '<json>'
        let bin = wsx_bin.to_string_lossy();
        let array = [bin.as_ref(), "status", "from-notify", "--agent", "codex"]
            .iter()
            .map(|s| toml_quote(s))
            .collect::<Vec<_>>()
            .join(",");
        Some(SpawnWiring {
            args: vec!["-c".to_string(), format!("notify=[{array}]")],
        })
    }
}

/// Quote a string as a TOML basic string for embedding in a `-c notify=[...]`
/// override value. Escapes backslashes and double-quotes.
fn toml_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(json: serde_json::Value) -> Option<ReportedState> {
        CodexStatus.parse_event(&json)
    }

    #[test]
    fn turn_complete_is_done() {
        assert_eq!(
            ev(serde_json::json!({"type": "agent-turn-complete", "last-assistant-message": "All set."})),
            Some(ReportedState::Done)
        );
    }

    #[test]
    fn turn_complete_with_question_is_blocked() {
        assert_eq!(
            ev(serde_json::json!({"type": "agent-turn-complete", "last-assistant-message": "Which library should I use?"})),
            Some(ReportedState::Blocked)
        );
    }

    #[test]
    fn turn_complete_without_message_degrades_to_done() {
        assert_eq!(
            ev(serde_json::json!({"type": "agent-turn-complete"})),
            Some(ReportedState::Done)
        );
    }

    #[test]
    fn other_or_missing_type_is_ignored() {
        assert_eq!(ev(serde_json::json!({"type": "session-start"})), None);
        assert_eq!(ev(serde_json::json!({})), None);
    }

    #[test]
    fn spawn_wiring_emits_notify_pointing_at_from_notify() {
        let w = CodexStatus
            .spawn_wiring(Path::new("/usr/local/bin/wsx"), false)
            .unwrap();
        assert_eq!(w.args[0], "-c");
        let val = &w.args[1];
        assert!(val.starts_with("notify=["), "got: {val}");
        assert!(val.contains("/usr/local/bin/wsx"));
        assert!(val.contains("from-notify"));
        assert!(val.contains("--agent"));
        assert!(val.contains("codex"));
    }

    #[test]
    fn spawn_wiring_toml_escapes_bin_path() {
        // A path with a space, a backslash, and an embedded double-quote must
        // stay valid TOML inside the array. (`toml` is not a dependency, so
        // assert the escaped substrings directly rather than re-parsing.)
        let w = CodexStatus
            .spawn_wiring(Path::new(r#"/o dd\"wsx"#), false)
            .unwrap();
        // Backslash is doubled and the double-quote is backslash-escaped:
        // input `/o dd\"wsx` -> TOML string `"/o dd\\\"wsx"`.
        assert!(
            w.args[1].contains(r#""/o dd\\\"wsx""#),
            "got: {}",
            w.args[1]
        );
    }
}
