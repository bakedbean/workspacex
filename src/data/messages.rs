//! Asynchronous inbox for agent-to-agent prompts. The CLI (`wsx agent send`)
//! enqueues rows; the TUI drains them on its tick and injects them into the
//! target agent's session.

use crate::data::store::{AgentInstanceId, Store, WorkspaceId, now_ms};
use crate::error::Result;

#[derive(Debug, Clone)]
pub struct AgentMessage {
    pub id: i64,
    pub workspace_id: WorkspaceId,
    pub target_agent_id: AgentInstanceId,
    pub from_agent_id: Option<AgentInstanceId>,
    pub body: String,
}

impl Store {
    pub fn enqueue_message(
        &self,
        workspace_id: WorkspaceId,
        target: AgentInstanceId,
        from: Option<AgentInstanceId>,
        body: &str,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT INTO agent_messages (workspace_id, target_agent_id, from_agent_id, body, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![workspace_id.0, target.0, from.map(|f| f.0), body, now_ms()],
        )?;
        Ok(())
    }

    pub fn undelivered_messages(&self) -> Result<Vec<AgentMessage>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT id, workspace_id, target_agent_id, from_agent_id, body
             FROM agent_messages WHERE delivered_at IS NULL ORDER BY id ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(AgentMessage {
                id: r.get(0)?,
                workspace_id: WorkspaceId(r.get(1)?),
                target_agent_id: AgentInstanceId(r.get(2)?),
                from_agent_id: r.get::<_, Option<i64>>(3)?.map(AgentInstanceId),
                body: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn mark_delivered(&self, id: i64) -> Result<()> {
        self.conn().execute(
            "UPDATE agent_messages SET delivered_at = ?1 WHERE id = ?2",
            rusqlite::params![now_ms(), id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::store::{NewWorkspace, Store};
    use crate::pty::session::AgentKind;

    fn seed(store: &Store) -> (WorkspaceId, AgentInstanceId) {
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
                agent: AgentKind::Claude,
            })
            .unwrap();
        store.add_primary_agent(ws, AgentKind::Claude, 1).unwrap();
        let target = store.add_workspace_agent(ws, AgentKind::Codex).unwrap();
        (ws, target.id)
    }

    #[test]
    fn enqueue_then_drain_then_mark_delivered() {
        let store = Store::open_in_memory().unwrap();
        let (ws, target) = seed(&store);
        store
            .enqueue_message(ws, target, None, "please review")
            .unwrap();
        let pending = store.undelivered_messages().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].body, "please review");
        assert_eq!(pending[0].target_agent_id, target);
        store.mark_delivered(pending[0].id).unwrap();
        assert!(store.undelivered_messages().unwrap().is_empty());
    }

    #[test]
    fn delete_workspace_with_queued_messages_does_not_fk_violate() {
        let store = Store::open_in_memory().unwrap();
        let (ws, target) = seed(&store);
        store.enqueue_message(ws, target, None, "msg").unwrap();
        // Must not error on the FK from agent_messages -> workspace_agents.
        store.delete_workspace(ws).unwrap();
        assert!(store.undelivered_messages().unwrap().is_empty());
    }
}
