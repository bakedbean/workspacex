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
    /// Epoch ms the row was queued.
    pub created_at: i64,
    /// Epoch ms the TUI injected it into the target's PTY, or retired it as
    /// undeliverable (then `drop_reason` is set). `None` while still queued.
    pub delivered_at: Option<i64>,
    /// Set when the message was retired WITHOUT reaching the agent.
    pub drop_reason: Option<String>,
}

impl AgentMessage {
    /// Whether the message actually reached the agent's session.
    pub fn reached_agent(&self) -> bool {
        self.delivered_at.is_some() && self.drop_reason.is_none()
    }
}

/// Which rows `list_messages` returns, always relative to one agent instance
/// or one workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageScope {
    /// Messages addressed to this instance (its inbox).
    To(AgentInstanceId),
    /// Messages this instance sent.
    From(AgentInstanceId),
    /// Messages queued in this workspace, or sent from one of its agents to
    /// another workspace. A cross-workspace message is stored against the
    /// TARGET's workspace, so the sender side needs the second clause.
    Workspace(WorkspaceId),
}

const MESSAGE_COLUMNS: &str =
    "id, workspace_id, target_agent_id, from_agent_id, body, created_at, delivered_at, drop_reason";

fn row_to_message(r: &rusqlite::Row) -> rusqlite::Result<AgentMessage> {
    Ok(AgentMessage {
        id: r.get(0)?,
        workspace_id: WorkspaceId(r.get(1)?),
        target_agent_id: AgentInstanceId(r.get(2)?),
        from_agent_id: r.get::<_, Option<i64>>(3)?.map(AgentInstanceId),
        body: r.get(4)?,
        created_at: r.get(5)?,
        delivered_at: r.get(6)?,
        drop_reason: r.get(7)?,
    })
}

impl Store {
    pub fn enqueue_message(
        &self,
        workspace_id: WorkspaceId,
        target: AgentInstanceId,
        from: Option<AgentInstanceId>,
        body: &str,
    ) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO agent_messages (workspace_id, target_agent_id, from_agent_id, body, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![workspace_id.0, target.0, from.map(|f| f.0), body, now_ms()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn undelivered_messages(&self) -> Result<Vec<AgentMessage>> {
        let mut stmt = self.conn().prepare_cached(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM agent_messages WHERE delivered_at IS NULL ORDER BY id ASC"
        ))?;
        let rows = stmt.query_map([], row_to_message)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// One message by id, delivered or not.
    pub fn message_by_id(&self, id: i64) -> Result<Option<AgentMessage>> {
        use rusqlite::OptionalExtension;
        let mut stmt = self.conn().prepare_cached(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM agent_messages WHERE id = ?1"
        ))?;
        Ok(stmt.query_row([id], row_to_message).optional()?)
    }

    /// The newest `limit` messages in `scope`, returned oldest-first so a
    /// listing reads top to bottom like a transcript.
    pub fn list_messages(
        &self,
        scope: MessageScope,
        undelivered_only: bool,
        limit: usize,
    ) -> Result<Vec<AgentMessage>> {
        let (clause, key) = match scope {
            MessageScope::To(a) => ("target_agent_id = ?1", a.0),
            MessageScope::From(a) => ("from_agent_id = ?1", a.0),
            MessageScope::Workspace(w) => (
                "(workspace_id = ?1 OR from_agent_id IN \
                 (SELECT id FROM workspace_agents WHERE workspace_id = ?1))",
                w.0,
            ),
        };
        // "Undelivered" means never reached the agent: still queued, or
        // retired as undeliverable.
        let pending = if undelivered_only {
            " AND (delivered_at IS NULL OR drop_reason IS NOT NULL)"
        } else {
            ""
        };
        let mut stmt = self.conn().prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM agent_messages
             WHERE {clause}{pending} ORDER BY id DESC LIMIT ?2"
        ))?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = stmt.query_map(rusqlite::params![key, limit], row_to_message)?;
        let mut v: Vec<AgentMessage> = rows.collect::<std::result::Result<_, _>>()?;
        v.reverse();
        Ok(v)
    }

    /// The oldest message to `target` with an id above `after_id`
    /// (optionally only from `from`), delivered or not. `wsx agent wait`
    /// polls this.
    pub fn first_message_to_after(
        &self,
        target: AgentInstanceId,
        after_id: i64,
        from: Option<AgentInstanceId>,
    ) -> Result<Option<AgentMessage>> {
        use rusqlite::OptionalExtension;
        let mut stmt = self.conn().prepare_cached(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM agent_messages
             WHERE target_agent_id = ?1 AND id > ?2 AND (?3 IS NULL OR from_agent_id = ?3)
             ORDER BY id ASC LIMIT 1"
        ))?;
        Ok(stmt
            .query_row(
                rusqlite::params![target.0, after_id, from.map(|f| f.0)],
                row_to_message,
            )
            .optional()?)
    }

    /// Where a `wsx agent wait` without `--after` starts counting: just below
    /// the oldest message still queued for `target` (it has not reached the
    /// agent yet), else the newest id overall. One statement, so a delivery
    /// acked between "what is queued" and "what is newest" cannot make a
    /// queued message fall through the gap.
    pub fn wait_baseline(&self, target: AgentInstanceId) -> Result<i64> {
        Ok(self.conn().query_row(
            "SELECT COALESCE(
                 (SELECT MIN(id) - 1 FROM agent_messages
                  WHERE target_agent_id = ?1 AND delivered_at IS NULL),
                 (SELECT MAX(id) FROM agent_messages),
                 0)",
            [target.0],
            |r| r.get(0),
        )?)
    }

    /// Retire a message that can never be delivered: stamp `delivered_at` so
    /// the drain stops retrying it, and record why it did not arrive.
    pub fn mark_dropped(&self, id: i64, reason: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE agent_messages SET delivered_at = ?1, drop_reason = ?2 WHERE id = ?3",
            rusqlite::params![now_ms(), reason, id],
        )?;
        Ok(())
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
                shared: false,
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

    #[test]
    fn enqueue_returns_the_new_row_id() {
        let store = Store::open_in_memory().unwrap();
        let (ws, target) = seed(&store);
        let a = store.enqueue_message(ws, target, None, "one").unwrap();
        let b = store.enqueue_message(ws, target, None, "two").unwrap();
        assert!(b > a);
        let got = store.message_by_id(b).unwrap().unwrap();
        assert_eq!(got.body, "two");
        assert!(got.created_at > 0);
        assert_eq!(got.delivered_at, None);
        store.mark_delivered(b).unwrap();
        assert!(
            store
                .message_by_id(b)
                .unwrap()
                .unwrap()
                .delivered_at
                .is_some()
        );
        assert!(store.message_by_id(9999).unwrap().is_none());
    }

    #[test]
    fn list_messages_scopes_by_direction_and_workspace() {
        let store = Store::open_in_memory().unwrap();
        let (ws, codex) = seed(&store);
        let primary = store.primary_instance_id(ws).unwrap().unwrap();
        // A second workspace whose agent messages ours, and vice versa.
        let repo = store.repos().unwrap()[0].id;
        let other = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "other",
                branch: "wsx/other",
                worktree_path: std::path::Path::new("/tmp/r/other"),
                yolo: false,
                agent: AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        let far = store
            .add_primary_agent(other, AgentKind::Claude, 1)
            .unwrap()
            .id;

        let to_codex = store
            .enqueue_message(ws, codex, Some(primary), "a")
            .unwrap();
        let to_primary = store
            .enqueue_message(ws, primary, Some(codex), "b")
            .unwrap();
        let outbound = store
            .enqueue_message(other, far, Some(primary), "c")
            .unwrap();
        let inbound = store.enqueue_message(ws, primary, Some(far), "d").unwrap();
        let unrelated = store.enqueue_message(other, far, None, "e").unwrap();

        let ids = |v: Vec<AgentMessage>| v.into_iter().map(|m| m.id).collect::<Vec<_>>();
        assert_eq!(
            ids(store
                .list_messages(MessageScope::To(primary), false, 50)
                .unwrap()),
            vec![to_primary, inbound]
        );
        assert_eq!(
            ids(store
                .list_messages(MessageScope::From(primary), false, 50)
                .unwrap()),
            vec![to_codex, outbound]
        );
        assert_eq!(
            ids(store
                .list_messages(MessageScope::Workspace(ws), false, 50)
                .unwrap()),
            vec![to_codex, to_primary, outbound, inbound],
            "outbound cross-workspace mail belongs to the sender's workspace too"
        );
        assert!(
            !ids(store
                .list_messages(MessageScope::Workspace(ws), false, 50)
                .unwrap())
            .contains(&unrelated)
        );

        // limit keeps the NEWEST rows, still oldest-first.
        assert_eq!(
            ids(store
                .list_messages(MessageScope::To(primary), false, 1)
                .unwrap()),
            vec![inbound]
        );
        store.mark_delivered(inbound).unwrap();
        assert_eq!(
            ids(store
                .list_messages(MessageScope::To(primary), true, 50)
                .unwrap()),
            vec![to_primary]
        );
        // A dropped message never arrived, so it still counts as undelivered.
        store.mark_dropped(to_primary, "gone").unwrap();
        let dropped = store
            .list_messages(MessageScope::To(primary), true, 50)
            .unwrap();
        assert_eq!(
            dropped.iter().map(|m| m.id).collect::<Vec<_>>(),
            vec![to_primary]
        );
        assert_eq!(dropped[0].drop_reason.as_deref(), Some("gone"));
        assert!(!dropped[0].reached_agent());
        assert!(
            store
                .undelivered_messages()
                .unwrap()
                .iter()
                .all(|m| m.id != to_primary),
            "the drain must not retry a dropped message"
        );
    }

    #[test]
    fn wait_baseline_starts_below_queued_mail_else_at_the_newest_id() {
        let store = Store::open_in_memory().unwrap();
        let (ws, target) = seed(&store);
        assert_eq!(store.wait_baseline(target).unwrap(), 0);
        let seen = store.enqueue_message(ws, target, None, "seen").unwrap();
        store.mark_delivered(seen).unwrap();
        assert_eq!(store.wait_baseline(target).unwrap(), seen);
        let queued = store.enqueue_message(ws, target, None, "queued").unwrap();
        let baseline = store.wait_baseline(target).unwrap();
        assert_eq!(baseline, queued - 1);

        // Found from the baseline even once the dashboard delivers it.
        store.mark_delivered(queued).unwrap();
        let got = store
            .first_message_to_after(target, baseline, None)
            .unwrap();
        assert_eq!(got.map(|m| m.id), Some(queued));
        assert!(
            store
                .first_message_to_after(target, queued, None)
                .unwrap()
                .is_none()
        );

        // `from` filters by sender.
        let primary = store.primary_instance_id(ws).unwrap().unwrap();
        let anon = store.enqueue_message(ws, target, None, "anon").unwrap();
        let signed = store
            .enqueue_message(ws, target, Some(primary), "signed")
            .unwrap();
        assert_eq!(
            store
                .first_message_to_after(target, queued, None)
                .unwrap()
                .map(|m| m.id),
            Some(anon)
        );
        assert_eq!(
            store
                .first_message_to_after(target, queued, Some(primary))
                .unwrap()
                .map(|m| m.id),
            Some(signed)
        );
    }
}
