//! Roster of agent instances attached to a workspace.
//!
//! An *agent instance* is one agent attached to a workspace. The workspace's
//! original (creation-time) agent is its primary instance; additional agents
//! — including duplicates of the same kind — are non-primary instances.

use crate::data::store::{AgentInstanceId, WorkspaceId};
use crate::pty::session::AgentKind;

#[derive(Debug, Clone)]
pub struct AgentInstance {
    pub id: AgentInstanceId,
    pub workspace_id: WorkspaceId,
    pub agent: AgentKind,
    pub ordinal: i64,
    pub is_primary: bool,
    pub session_ref: Option<String>,
    pub created_at: i64,
}

/// The single source of truth for an instance's display/address name.
/// `ordinal` is 1-based; the first instance of a kind (ordinal 1, and
/// defensively anything < 1) gets the bare agent name, while ordinal >= 2
/// gets a `name#N` suffix.
/// The footer, the `wsx agent send` CLI, and delivered message banners all
/// call this so they cannot disagree about what "claude#2" is called.
pub fn instance_label(agent: AgentKind, ordinal: i64) -> String {
    if ordinal <= 1 {
        agent.display_name().to_string()
    } else {
        format!("{}#{}", agent.display_name(), ordinal)
    }
}

impl AgentInstance {
    pub fn label(&self) -> String {
        instance_label(self.agent, self.ordinal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_label_omits_suffix_for_first_and_adds_for_rest() {
        // ordinal < 1 collapses to the bare name (locks in the `<= 1`
        // boundary against a future refactor to `== 1`).
        assert_eq!(instance_label(AgentKind::Claude, 0), "claude");
        assert_eq!(instance_label(AgentKind::Claude, 1), "claude");
        assert_eq!(instance_label(AgentKind::Claude, 2), "claude#2");
        assert_eq!(instance_label(AgentKind::Codex, 3), "codex#3");
    }
}
