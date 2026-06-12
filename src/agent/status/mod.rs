//! Per-harness status integration: the only Claude-specific part of agent
//! status reporting. Tier 1 (model push via `wsx status set`) and all storage
//! / classifier infrastructure are harness-agnostic; this module is where a
//! harness's *deterministic* event mechanism plugs in.
//!
//! To add Codex / Pi / Hermes deterministic reporting: add a module here, impl
//! `StatusIntegration`, and route it in `for_agent`. Call its `spawn_wiring`
//! from that agent's spawn builder. Nothing else changes.

pub mod claude;
pub mod codex;

use crate::data::store::ReportedState;
use crate::pty::session::AgentKind;
use std::path::Path;

/// Spawn-time configuration a harness needs so its lifecycle events call back
/// into `wsx status`. Currently just extra CLI args appended to the spawn
/// command (Claude: `--settings <json>`); widen this struct if a future
/// harness needs a config file or env var instead.
#[derive(Debug, Clone, Default)]
pub struct SpawnWiring {
    pub args: Vec<String>,
}

/// The harness-specific half of status reporting.
pub trait StatusIntegration: Sync {
    /// Interpret a deterministic event payload (delivered to `wsx status
    /// from-hook` on stdin) into a reported state, or `None` when the event is
    /// not status-relevant.
    fn parse_event(&self, json: &serde_json::Value) -> Option<ReportedState>;

    /// Spawn-time wiring this harness needs to report deterministically, or
    /// `None` if it has no such mechanism (tier 1 + tier 3 only). `wsx_bin` is
    /// the absolute path to the running wsx binary so callbacks invoke the same
    /// build regardless of PATH.
    fn spawn_wiring(&self, _wsx_bin: &Path, _fast_mode: bool) -> Option<SpawnWiring> {
        None
    }
}

/// A harness with no deterministic mechanism yet. Relies entirely on tier 1
/// (model push) and tier 3 (JSONL heuristic).
pub struct NoopStatus;
impl StatusIntegration for NoopStatus {
    fn parse_event(&self, _json: &serde_json::Value) -> Option<ReportedState> {
        None
    }
}

static CLAUDE: claude::ClaudeStatus = claude::ClaudeStatus;
static CODEX: codex::CodexStatus = codex::CodexStatus;
static NOOP: NoopStatus = NoopStatus;

/// The status integration for an agent kind. Claude has a hook-based
/// implementation; everything else is a no-op for now.
pub fn for_agent(agent: AgentKind) -> &'static dyn StatusIntegration {
    match agent {
        AgentKind::Claude => &CLAUDE,
        AgentKind::Codex => &CODEX,
        _ => &NOOP,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_resolves_to_claude_integration() {
        let ev = serde_json::json!({"hook_event_name": "UserPromptSubmit"});
        assert_eq!(
            for_agent(AgentKind::Claude).parse_event(&ev),
            Some(ReportedState::Working)
        );
    }

    #[test]
    fn other_agents_resolve_to_noop() {
        let ev = serde_json::json!({"hook_event_name": "UserPromptSubmit"});
        for agent in [AgentKind::Pi, AgentKind::Hermes] {
            assert_eq!(for_agent(agent).parse_event(&ev), None);
            assert!(
                for_agent(agent)
                    .spawn_wiring(Path::new("/usr/bin/wsx"), false)
                    .is_none()
            );
        }
    }

    #[test]
    fn codex_resolves_to_codex_integration() {
        let ev = serde_json::json!({"type": "agent-turn-complete", "last-assistant-message": "done"});
        assert_eq!(
            for_agent(AgentKind::Codex).parse_event(&ev),
            Some(ReportedState::Done)
        );
    }
}
