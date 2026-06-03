//! Roster of agent instances attached to a workspace.
//!
//! An *agent instance* is one agent attached to a workspace. The workspace's
//! original (creation-time) agent is its primary instance; additional agents
//! — including duplicates of the same kind — are non-primary instances.

use crate::data::store::{AgentInstanceId, Store, WorkspaceId, now_ms};
use crate::error::Result;
use crate::pty::session::AgentKind;
use rusqlite::OptionalExtension;

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

fn row_to_instance(r: &rusqlite::Row) -> rusqlite::Result<AgentInstance> {
    Ok(AgentInstance {
        id: AgentInstanceId(r.get(0)?),
        workspace_id: WorkspaceId(r.get(1)?),
        agent: AgentKind::from_str_or_default(Some(&r.get::<_, String>(2)?)),
        ordinal: r.get(3)?,
        is_primary: r.get::<_, i64>(4)? != 0,
        session_ref: r.get(5)?,
        created_at: r.get(6)?,
    })
}

impl Store {
    /// All instances for a workspace, primary first then by creation time.
    pub fn workspace_agents(&self, ws: WorkspaceId) -> Result<Vec<AgentInstance>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, workspace_id, agent, ordinal, is_primary, session_ref, created_at
             FROM workspace_agents WHERE workspace_id = ?1
             ORDER BY is_primary DESC, created_at ASC, id ASC",
        )?;
        let rows = stmt.query_map([ws.0], row_to_instance)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Add a non-primary instance, computing the next ordinal for its kind.
    pub fn add_workspace_agent(&self, ws: WorkspaceId, agent: AgentKind) -> Result<AgentInstance> {
        // The MAX(ordinal)+1 SELECT and the INSERT are two statements. The TUI
        // is single-threaded and the CLI is the only other writer, so a race is
        // unlikely; the UNIQUE(workspace_id, agent, ordinal) constraint is the
        // backstop and would surface a clean error (not a panic) on collision.
        let next: i64 = self.conn().query_row(
            "SELECT COALESCE(MAX(ordinal), 0) + 1 FROM workspace_agents
             WHERE workspace_id = ?1 AND agent = ?2",
            rusqlite::params![ws.0, agent.store_value()],
            |r| r.get(0),
        )?;
        let now = now_ms();
        self.conn().execute(
            "INSERT INTO workspace_agents (workspace_id, agent, ordinal, is_primary, created_at)
             VALUES (?1, ?2, ?3, 0, ?4)",
            rusqlite::params![ws.0, agent.store_value(), next, now],
        )?;
        Ok(AgentInstance {
            id: AgentInstanceId(self.conn().last_insert_rowid()),
            workspace_id: ws,
            agent,
            ordinal: next,
            is_primary: false,
            session_ref: None,
            created_at: now,
        })
    }

    /// Seed the primary instance for a freshly created workspace.
    pub fn add_primary_agent(
        &self,
        ws: WorkspaceId,
        agent: AgentKind,
        created_at: i64,
    ) -> Result<AgentInstance> {
        self.conn().execute(
            "INSERT INTO workspace_agents (workspace_id, agent, ordinal, is_primary, created_at)
             VALUES (?1, ?2, 1, 1, ?3)",
            rusqlite::params![ws.0, agent.store_value(), created_at],
        )?;
        Ok(AgentInstance {
            id: AgentInstanceId(self.conn().last_insert_rowid()),
            workspace_id: ws,
            agent,
            ordinal: 1,
            is_primary: true,
            session_ref: None,
            created_at,
        })
    }

    pub fn remove_workspace_agent(&self, id: AgentInstanceId) -> Result<()> {
        // Clear any inbox rows targeting this instance first: agent_messages
        // has an FK (target_agent_id -> workspace_agents.id) with no cascade,
        // and delivered messages are retained, so the row delete below would
        // FK-violate once any message had ever been sent to this agent. The
        // `IN (... WHERE is_primary = 0)` guard mirrors the row delete's
        // own guard so a primary's inbox is never wiped (and each statement is
        // independently safe, so there's no separate-SELECT TOCTOU).
        self.conn().execute(
            "DELETE FROM agent_messages WHERE target_agent_id = ?1
             AND ?1 IN (SELECT id FROM workspace_agents WHERE is_primary = 0)",
            [id.0],
        )?;
        // Atomic: only deletes non-primary rows, so there is no TOCTOU between a
        // separate SELECT and DELETE.
        let deleted = self.conn().execute(
            "DELETE FROM workspace_agents WHERE id = ?1 AND is_primary = 0",
            [id.0],
        )?;
        if deleted == 0 {
            let exists: i64 = self.conn().query_row(
                "SELECT count(*) FROM workspace_agents WHERE id = ?1",
                [id.0],
                |r| r.get(0),
            )?;
            return Err(crate::error::Error::UserInput(if exists == 0 {
                "agent not found".into()
            } else {
                "cannot remove the primary agent".into()
            }));
        }
        Ok(())
    }

    pub fn set_instance_session_ref(&self, id: AgentInstanceId, session_ref: &str) -> Result<()> {
        let n = self.conn().execute(
            "UPDATE workspace_agents SET session_ref = ?1 WHERE id = ?2",
            rusqlite::params![session_ref, id.0],
        )?;
        if n == 0 {
            return Err(crate::error::Error::UserInput("agent not found".into()));
        }
        Ok(())
    }

    /// Resolve a label like "claude" or "claude#2" to an instance id.
    pub fn resolve_instance_label(
        &self,
        ws: WorkspaceId,
        label: &str,
    ) -> Result<Option<AgentInstanceId>> {
        Ok(self
            .workspace_agents(ws)?
            .into_iter()
            .find(|i| i.label() == label)
            .map(|i| i.id))
    }

    /// A single instance by its id.
    pub fn workspace_agents_by_id(&self, id: AgentInstanceId) -> Result<Option<AgentInstance>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT id, workspace_id, agent, ordinal, is_primary, session_ref, created_at
             FROM workspace_agents WHERE id = ?1",
        )?;
        let r = stmt.query_row([id.0], row_to_instance).optional()?;
        Ok(r)
    }

    /// The primary instance id for a workspace.
    pub fn primary_instance_id(&self, ws: WorkspaceId) -> Result<Option<AgentInstanceId>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT id FROM workspace_agents WHERE workspace_id = ?1 AND is_primary = 1",
        )?;
        Ok(stmt
            .query_row([ws.0], |r| r.get::<_, i64>(0))
            .optional()?
            .map(AgentInstanceId))
    }
}

#[cfg(test)]
mod store_tests {
    use super::*;
    use crate::data::store::{NewWorkspace, Store, WorkspaceId};

    fn seed_ws_with_primary(store: &Store) -> WorkspaceId {
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "wsx")
            .unwrap();
        let ws = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "w1",
                branch: "wsx/w1",
                worktree_path: std::path::Path::new("/tmp/r/w1"),
                yolo: false,
                agent: AgentKind::Claude,
            })
            .unwrap();
        store.add_primary_agent(ws, AgentKind::Claude, 1).unwrap();
        ws
    }

    #[test]
    fn add_then_list_computes_ordinals_and_labels() {
        let store = Store::open_in_memory().unwrap();
        let ws = seed_ws_with_primary(&store);
        let second = store.add_workspace_agent(ws, AgentKind::Claude).unwrap();
        let codex = store.add_workspace_agent(ws, AgentKind::Codex).unwrap();
        assert_eq!(second.ordinal, 2);
        assert_eq!(second.label(), "claude#2");
        assert_eq!(codex.ordinal, 1);
        assert_eq!(codex.label(), "codex");

        let all = store.workspace_agents(ws).unwrap();
        assert_eq!(all.len(), 3);
        assert!(all[0].is_primary); // primary first
    }

    #[test]
    fn remove_refuses_primary_but_removes_others() {
        let store = Store::open_in_memory().unwrap();
        let ws = seed_ws_with_primary(&store);
        let primary = store.workspace_agents(ws).unwrap()[0].id;
        assert!(store.remove_workspace_agent(primary).is_err());

        let added = store.add_workspace_agent(ws, AgentKind::Pi).unwrap();
        store.remove_workspace_agent(added.id).unwrap();
        assert_eq!(store.workspace_agents(ws).unwrap().len(), 1);
    }

    #[test]
    fn remove_agent_with_messages_does_not_fk_violate() {
        // agent_messages.target_agent_id FKs to workspace_agents.id (no cascade)
        // and delivered messages are retained, so removing an agent that has
        // ever received a message must clear those rows first.
        let store = Store::open_in_memory().unwrap();
        let ws = seed_ws_with_primary(&store);
        let added = store.add_workspace_agent(ws, AgentKind::Codex).unwrap();
        store.enqueue_message(ws, added.id, None, "ping").unwrap();
        // Would FK-violate without the inbox cleanup in remove_workspace_agent.
        store.remove_workspace_agent(added.id).unwrap();
        assert_eq!(store.workspace_agents(ws).unwrap().len(), 1);
        assert!(store.undelivered_messages().unwrap().is_empty());
    }

    #[test]
    fn duplicate_primary_is_rejected_by_unique_index() {
        // The partial unique index enforces exactly one primary per workspace.
        let store = Store::open_in_memory().unwrap();
        let ws = seed_ws_with_primary(&store); // already has one primary
        assert!(store.add_primary_agent(ws, AgentKind::Codex, 1).is_err());
    }

    #[test]
    fn resolve_label_and_primary_id() {
        let store = Store::open_in_memory().unwrap();
        let ws = seed_ws_with_primary(&store);
        let second = store.add_workspace_agent(ws, AgentKind::Claude).unwrap();
        assert_eq!(
            store.resolve_instance_label(ws, "claude#2").unwrap(),
            Some(second.id)
        );
        assert_eq!(store.resolve_instance_label(ws, "nope").unwrap(), None);
        assert!(store.primary_instance_id(ws).unwrap().is_some());
    }

    #[test]
    fn remove_nonexistent_agent_errors() {
        let store = Store::open_in_memory().unwrap();
        let _ = seed_ws_with_primary(&store);
        assert!(store.remove_workspace_agent(AgentInstanceId(9999)).is_err());
    }

    #[test]
    fn set_session_ref_on_unknown_id_errors() {
        let store = Store::open_in_memory().unwrap();
        let _ = seed_ws_with_primary(&store);
        assert!(
            store
                .set_instance_session_ref(AgentInstanceId(9999), "x")
                .is_err()
        );
    }

    #[test]
    fn set_session_ref_persists() {
        let store = Store::open_in_memory().unwrap();
        let ws = seed_ws_with_primary(&store);
        let added = store.add_workspace_agent(ws, AgentKind::Codex).unwrap();
        store
            .set_instance_session_ref(added.id, "sess-123")
            .unwrap();
        let reloaded = store
            .workspace_agents(ws)
            .unwrap()
            .into_iter()
            .find(|i| i.id == added.id)
            .unwrap();
        assert_eq!(reloaded.session_ref.as_deref(), Some("sess-123"));
    }

    #[test]
    fn workspace_agents_by_id_returns_instance_or_none() {
        let store = Store::open_in_memory().unwrap();
        let ws = seed_ws_with_primary(&store);
        let added = store.add_workspace_agent(ws, AgentKind::Codex).unwrap();
        let got = store.workspace_agents_by_id(added.id).unwrap().unwrap();
        assert_eq!(got.id, added.id);
        assert_eq!(got.agent, AgentKind::Codex);
        assert!(!got.is_primary);
        assert!(
            store
                .workspace_agents_by_id(AgentInstanceId(9999))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn add_primary_agent_seeds_single_primary() {
        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "wsx")
            .unwrap();
        let ws = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "w",
                branch: "wsx/w",
                worktree_path: std::path::Path::new("/tmp/r/w"),
                yolo: false,
                agent: AgentKind::Hermes,
            })
            .unwrap();
        store.add_primary_agent(ws, AgentKind::Hermes, 1).unwrap();
        let all = store.workspace_agents(ws).unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].is_primary);
        assert_eq!(all[0].agent, AgentKind::Hermes);
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
