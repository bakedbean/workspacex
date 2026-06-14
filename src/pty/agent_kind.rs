//! The coding-agent taxonomy.
//!
//! `AgentKind` is the small leaf enum naming which coding agent a session
//! drives. It's re-exported from `pty::session` (and `pty`) so existing
//! `crate::pty::session::AgentKind` paths keep resolving.

/// Which coding agent to spawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Claude,
    Pi,
    Hermes,
    Codex,
}

impl AgentKind {
    /// All agent kinds, in stable display order. Add new variants here when
    /// extending the enum — `const` arrays do not get exhaustiveness checking,
    /// so this is the one place the compiler can't catch a drift.
    pub const ALL: [AgentKind; 4] = [
        AgentKind::Claude,
        AgentKind::Pi,
        AgentKind::Hermes,
        AgentKind::Codex,
    ];

    pub fn from_str_or_default(s: Option<&str>) -> Self {
        match s {
            Some("pi") => AgentKind::Pi,
            Some("hermes") => AgentKind::Hermes,
            Some("codex") => AgentKind::Codex,
            _ => AgentKind::Claude,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Pi => "pi",
            AgentKind::Hermes => "hermes",
            AgentKind::Codex => "codex",
        }
    }

    pub fn default_binary(self) -> &'static str {
        self.display_name()
    }

    pub fn store_value(self) -> &'static str {
        self.display_name()
    }

    pub fn from_store(store: &crate::data::store::Store) -> Self {
        Self::from_str_or_default(store.get_setting("coding_agent").ok().flatten().as_deref())
    }
}
