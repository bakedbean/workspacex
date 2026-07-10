//! Agent-authored workspace recap persistence (the `workspace_recap` table):
//! the goal / state / next one-liners a workspace's agent maintains via
//! `wsx recap set`, rendered by the dashboard's PM digest view.

use crate::data::store::{Store, WorkspaceId, WorkspaceRecap, now_ms};
use crate::error::Result;
use rusqlite::OptionalExtension;

impl Store {
    /// Partial upsert: only provided fields change (a `None` leaves the
    /// stored value untouched); `updated_at` always bumps.
    pub fn set_workspace_recap(
        &self,
        id: WorkspaceId,
        goal: Option<&str>,
        state: Option<&str>,
        next: Option<&str>,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT INTO workspace_recap (workspace_id, goal, state, next, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(workspace_id) DO UPDATE SET \
                 goal       = COALESCE(excluded.goal, workspace_recap.goal), \
                 state      = COALESCE(excluded.state, workspace_recap.state), \
                 next       = COALESCE(excluded.next, workspace_recap.next), \
                 updated_at = excluded.updated_at",
            rusqlite::params![id.0, goal, state, next, now_ms()],
        )?;
        Ok(())
    }

    pub fn clear_workspace_recap(&self, id: WorkspaceId) -> Result<()> {
        self.conn().execute(
            "DELETE FROM workspace_recap WHERE workspace_id = ?1",
            [id.0],
        )?;
        Ok(())
    }

    pub fn workspace_recap(&self, id: WorkspaceId) -> Result<Option<WorkspaceRecap>> {
        let r = self
            .conn()
            .query_row(
                "SELECT goal, state, next, updated_at \
                 FROM workspace_recap WHERE workspace_id = ?1",
                [id.0],
                row_to_recap,
            )
            .optional()?;
        Ok(r)
    }

    pub fn all_workspace_recaps(
        &self,
    ) -> Result<std::collections::HashMap<WorkspaceId, WorkspaceRecap>> {
        let mut stmt = self
            .conn()
            .prepare("SELECT workspace_id, goal, state, next, updated_at FROM workspace_recap")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                WorkspaceId(r.get(0)?),
                WorkspaceRecap {
                    goal: r.get(1)?,
                    state: r.get(2)?,
                    next: r.get(3)?,
                    updated_at: r.get(4)?,
                },
            ))
        })?;
        let mut map = std::collections::HashMap::new();
        for row in rows {
            let (id, recap) = row?;
            map.insert(id, recap);
        }
        Ok(map)
    }
}

fn row_to_recap(r: &rusqlite::Row) -> rusqlite::Result<WorkspaceRecap> {
    Ok(WorkspaceRecap {
        goal: r.get(0)?,
        state: r.get(1)?,
        next: r.get(2)?,
        updated_at: r.get(3)?,
    })
}

#[cfg(test)]
mod tests {
    use crate::data::store::{NewWorkspace, Store, WorkspaceId};
    use std::path::Path;

    fn store_with_workspace() -> (Store, WorkspaceId) {
        let store = Store::open_in_memory().unwrap();
        let repo_id = store.add_repo(Path::new("/tmp/r"), "r", "").unwrap();
        let ws = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "w",
                branch: "b/w",
                worktree_path: Path::new("/tmp/wt"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        (store, ws)
    }

    #[test]
    fn recap_round_trips() {
        let (store, ws) = store_with_workspace();
        assert!(store.workspace_recap(ws).unwrap().is_none());
        store
            .set_workspace_recap(
                ws,
                Some("fix auth"),
                Some("tests failing"),
                Some("debug regex"),
            )
            .unwrap();
        let got = store.workspace_recap(ws).unwrap().unwrap();
        assert_eq!(got.goal.as_deref(), Some("fix auth"));
        assert_eq!(got.state.as_deref(), Some("tests failing"));
        assert_eq!(got.next.as_deref(), Some("debug regex"));
        assert!(got.updated_at > 0);
    }

    #[test]
    fn partial_update_preserves_other_fields_and_bumps_updated_at() {
        let (store, ws) = store_with_workspace();
        store
            .set_workspace_recap(ws, Some("fix auth"), Some("starting"), None)
            .unwrap();
        let first = store.workspace_recap(ws).unwrap().unwrap();
        store
            .set_workspace_recap(ws, None, Some("tests green"), Some("open PR"))
            .unwrap();
        let got = store.workspace_recap(ws).unwrap().unwrap();
        assert_eq!(got.goal.as_deref(), Some("fix auth"), "goal must survive");
        assert_eq!(got.state.as_deref(), Some("tests green"));
        assert_eq!(got.next.as_deref(), Some("open PR"));
        assert!(got.updated_at >= first.updated_at);
    }

    #[test]
    fn clear_and_all_recaps() {
        let (store, ws) = store_with_workspace();
        store
            .set_workspace_recap(ws, Some("g"), None, None)
            .unwrap();
        let map = store.all_workspace_recaps().unwrap();
        assert_eq!(map.get(&ws).unwrap().goal.as_deref(), Some("g"));
        store.clear_workspace_recap(ws).unwrap();
        assert!(store.workspace_recap(ws).unwrap().is_none());
        assert!(store.all_workspace_recaps().unwrap().is_empty());
    }

    #[test]
    fn recap_cascade_deletes_with_workspace() {
        let (store, ws) = store_with_workspace();
        store
            .set_workspace_recap(ws, Some("g"), None, None)
            .unwrap();
        store.delete_workspace(ws).unwrap();
        assert!(store.workspace_recap(ws).unwrap().is_none());
    }
}
