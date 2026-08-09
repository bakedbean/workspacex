//! Agent-reported workspace status persistence (the `workspace_status` table):
//! the working/blocked/waiting/done state an agent reports for its workspace,
//! plus the row-mappers shared by the single- and all-workspace queries.

use crate::data::store::{ReportedState, ReportedStatus, Store, WorkspaceId, now_ms};
use crate::error::Result;
use rusqlite::OptionalExtension;

impl Store {
    pub fn set_workspace_status(
        &self,
        id: WorkspaceId,
        state: ReportedState,
        message: Option<&str>,
        source: &str,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT OR REPLACE INTO workspace_status \
                 (workspace_id, state, message, source, reported_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![id.0, state.as_str(), message, source, now_ms()],
        )?;
        Ok(())
    }

    /// Apply a hook/notify-sourced push (`wsx status from-hook` /
    /// `from-notify`), enforcing the one precedence rule the flat
    /// `workspace_status` row cannot express on its own.
    ///
    /// `Busy` records a *condition* — background work is in flight — while
    /// every other state records an *event*. Claude keeps firing its
    /// `Notification` `idle_prompt` ("Claude is waiting for your input") on a
    /// timer for as long as the input box is free, which is exactly the whole
    /// time a session sits parked on a background subagent. That push would
    /// overwrite `Busy` with `Waiting`, and since `classify` consults the JSONL
    /// `stopped_kind` above a reported `Waiting`, the row would flip back to the
    /// turn's `end_turn` ✓ Complete — the very false completion `Busy` exists to
    /// prevent. An idle prompt is not evidence of idleness while background work
    /// is pending, so drop it.
    ///
    /// Only `Waiting` is suppressed. `Working` (the agent resumed),
    /// `Blocked` (a permission prompt genuinely needs the user) and `Done`
    /// (a `Stop` whose `background_tasks` has emptied) all still supersede
    /// `Busy`, so the state clears itself the moment the session moves on.
    /// Explicit `wsx status set` pushes are unaffected: they go through
    /// `set_workspace_status` and stay authoritative.
    pub fn apply_hook_status(
        &self,
        id: WorkspaceId,
        state: ReportedState,
        source: &str,
    ) -> Result<()> {
        if state == ReportedState::Waiting
            && self
                .workspace_status(id)?
                .is_some_and(|cur| cur.state == ReportedState::Busy)
        {
            return Ok(());
        }
        self.set_workspace_status(id, state, None, source)
    }

    pub fn clear_workspace_status(&self, id: WorkspaceId) -> Result<()> {
        self.conn().execute(
            "DELETE FROM workspace_status WHERE workspace_id = ?1",
            [id.0],
        )?;
        Ok(())
    }

    pub fn workspace_status(&self, id: WorkspaceId) -> Result<Option<ReportedStatus>> {
        let r = self
            .conn()
            .query_row(
                "SELECT state, message, source, reported_at \
                 FROM workspace_status WHERE workspace_id = ?1",
                [id.0],
                row_to_reported_status,
            )
            .optional()?;
        Ok(r)
    }

    pub fn all_workspace_status(
        &self,
    ) -> Result<std::collections::HashMap<WorkspaceId, ReportedStatus>> {
        let mut stmt = self.conn().prepare(
            "SELECT workspace_id, state, message, source, reported_at FROM workspace_status",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((WorkspaceId(r.get(0)?), row_to_reported_status_offset1(r)?))
        })?;
        let mut map = std::collections::HashMap::new();
        for row in rows {
            let (id, status) = row?;
            map.insert(id, status);
        }
        Ok(map)
    }
}

fn row_to_reported_status(r: &rusqlite::Row) -> rusqlite::Result<ReportedStatus> {
    Ok(ReportedStatus {
        state: ReportedState::from_stored(&r.get::<_, String>(0)?)
            .unwrap_or(ReportedState::Working),
        message: r.get(1)?,
        source: r.get(2)?,
        reported_at: r.get(3)?,
    })
}

// Same as `row_to_reported_status` but for queries that select the
// workspace_id in column 0, shifting the status columns to 1..=4.
fn row_to_reported_status_offset1(r: &rusqlite::Row) -> rusqlite::Result<ReportedStatus> {
    Ok(ReportedStatus {
        state: ReportedState::from_stored(&r.get::<_, String>(1)?)
            .unwrap_or(ReportedState::Working),
        message: r.get(2)?,
        source: r.get(3)?,
        reported_at: r.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::store::NewWorkspace;
    use crate::pty::session::AgentKind;

    fn store_with_workspace() -> (Store, WorkspaceId) {
        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "r/")
            .unwrap();
        let ws = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "w",
                branch: "r/w",
                worktree_path: std::path::Path::new("/tmp/r/w"),
                yolo: false,
                agent: AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        (store, ws)
    }

    fn state_of(store: &Store, ws: WorkspaceId) -> Option<ReportedState> {
        store.workspace_status(ws).unwrap().map(|s| s.state)
    }

    #[test]
    fn idle_prompt_does_not_clobber_busy() {
        // The live failure: a `Stop` with pending `background_tasks` reports
        // Busy, then Claude's idle notification fires on its timer while the
        // session sits parked on the subagent. Letting that Waiting land would
        // drop the background-work fact and hand the row back to the JSONL
        // `end_turn` heuristic, which paints ✓ Complete.
        let (store, ws) = store_with_workspace();
        store
            .apply_hook_status(ws, ReportedState::Busy, "hook")
            .unwrap();
        store
            .apply_hook_status(ws, ReportedState::Waiting, "hook")
            .unwrap();
        assert_eq!(state_of(&store, ws), Some(ReportedState::Busy));
    }

    #[test]
    fn busy_is_superseded_by_every_other_hook_state() {
        // Busy must not be sticky against real progress, or a workspace would
        // spin forever once a subagent finished.
        for next in [
            ReportedState::Working,
            ReportedState::Blocked,
            ReportedState::Done,
        ] {
            let (store, ws) = store_with_workspace();
            store
                .apply_hook_status(ws, ReportedState::Busy, "hook")
                .unwrap();
            store.apply_hook_status(ws, next, "hook").unwrap();
            assert_eq!(state_of(&store, ws), Some(next), "{next:?} must win");
        }
    }

    #[test]
    fn waiting_lands_normally_when_not_busy() {
        // The suppression is scoped to the Busy condition — an ordinary idle
        // prompt after a finished turn still reports Waiting.
        let (store, ws) = store_with_workspace();
        store
            .apply_hook_status(ws, ReportedState::Done, "hook")
            .unwrap();
        store
            .apply_hook_status(ws, ReportedState::Waiting, "hook")
            .unwrap();
        assert_eq!(state_of(&store, ws), Some(ReportedState::Waiting));

        let (store, ws) = store_with_workspace();
        store
            .apply_hook_status(ws, ReportedState::Waiting, "hook")
            .unwrap();
        assert_eq!(state_of(&store, ws), Some(ReportedState::Waiting));
    }

    #[test]
    fn explicit_model_push_still_overrides_busy() {
        // `wsx status set waiting` is a tier-1 push from the agent itself, not
        // an inferred idle notification — it goes through `set_workspace_status`
        // and stays authoritative.
        let (store, ws) = store_with_workspace();
        store
            .apply_hook_status(ws, ReportedState::Busy, "hook")
            .unwrap();
        store
            .set_workspace_status(ws, ReportedState::Waiting, Some("parked"), "model")
            .unwrap();
        assert_eq!(state_of(&store, ws), Some(ReportedState::Waiting));
    }

    #[test]
    fn hook_push_records_its_source() {
        let (store, ws) = store_with_workspace();
        store
            .apply_hook_status(ws, ReportedState::Busy, "hook")
            .unwrap();
        let got = store.workspace_status(ws).unwrap().unwrap();
        assert_eq!(got.source, "hook");
        assert_eq!(got.message, None);
    }
}
