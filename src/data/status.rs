//! Agent-reported status persistence: the working/blocked/waiting/done state
//! each agent reports, plus the row-mappers shared by the queries.
//!
//! Two tables. `agent_status` holds what each agent instance last pushed,
//! keyed by instance, so peers sharing a workspace no longer overwrite each
//! other. `workspace_status` is the *derived* workspace-level row: every write
//! re-derives it (see `rederive_workspace_status`), so the dashboard, waybar
//! and menubar keep reading one row per workspace exactly as before.

use crate::data::store::{
    AgentInstanceId, ReportedState, ReportedStatus, Store, WorkspaceId, now_ms,
};
use crate::error::Result;
use rusqlite::OptionalExtension;
use std::collections::HashMap;

impl Store {
    /// Record a status for the workspace's primary agent — the attribution
    /// for a push that carries no instance (a human's shell, an editor-hosted
    /// agent). See `set_agent_status`.
    pub fn set_workspace_status(
        &self,
        id: WorkspaceId,
        state: ReportedState,
        message: Option<&str>,
        source: &str,
    ) -> Result<()> {
        self.set_agent_status(id, None, state, message, source)
    }

    /// Record `agent`'s status, then re-derive the workspace row. `None`
    /// attributes the push to the primary instance. A workspace with no
    /// instance rows at all (only reachable from tests and pre-multi-agent
    /// fixtures) gets the workspace row written directly.
    pub fn set_agent_status(
        &self,
        id: WorkspaceId,
        agent: Option<AgentInstanceId>,
        state: ReportedState,
        message: Option<&str>,
        source: &str,
    ) -> Result<()> {
        let Some(agent) = self.status_target(id, agent)? else {
            self.conn().execute(
                "INSERT OR REPLACE INTO workspace_status \
                     (workspace_id, state, message, source, reported_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![id.0, state.as_str(), message, source, now_ms()],
            )?;
            return Ok(());
        };
        // The write comes first, so the transaction takes the write lock up
        // front and a racing hook process waits on busy_timeout rather than
        // deriving from a snapshot the other process is about to change.
        let tx = self.conn().unchecked_transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO agent_status \
                 (agent_id, workspace_id, state, message, source, reported_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![agent.0, id.0, state.as_str(), message, source, now_ms()],
        )?;
        rederive_workspace_status(&tx, id)?;
        tx.commit()?;
        Ok(())
    }

    /// Apply a hook/notify-sourced push (`wsx status from-hook` /
    /// `from-notify`) for `agent` (`None` = the primary), enforcing the one
    /// precedence rule a last-writer-wins row cannot express on its own.
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
    /// is pending, so drop it. The rule is per agent: one peer's idle prompt
    /// never touches another peer's `Busy`.
    ///
    /// Only `Waiting` is suppressed. `Working` (the agent resumed),
    /// `Blocked` (a permission prompt genuinely needs the user) and `Done`
    /// (a `Stop` whose `background_tasks` has emptied) all still supersede
    /// `Busy`, so the next hook event the session emits clears it. That is
    /// arrival order, not event order: hook processes are independent and the
    /// row is last-writer-wins, so a stalled `Stop` landing after a newer
    /// `UserPromptSubmit` can briefly resurrect `Busy` until the following
    /// event. Pre-existing for every state, and self-correcting.
    ///
    /// Nothing here expires a stale `Busy` from a session that died mid-flight;
    /// the dashboard's escape is the `session_running` guard in
    /// `Status::classify`. Consumers that render `all_workspace_status`
    /// directly — waybar (`src/desktop/waybar/status.rs`) and the menubar rows
    /// (`src/desktop/rows.rs`) — have no liveness signal and so show the last
    /// stored state indefinitely, exactly as they already do for a `Working`
    /// push whose session was killed.
    ///
    /// Explicit `wsx status set` pushes are unaffected: they go through
    /// `set_agent_status` and stay authoritative.
    pub fn apply_hook_status(
        &self,
        id: WorkspaceId,
        agent: Option<AgentInstanceId>,
        state: ReportedState,
        source: &str,
    ) -> Result<()> {
        if state != ReportedState::Waiting {
            return self.set_agent_status(id, agent, state, None, source);
        }
        // Conditional upsert rather than read-then-write: every `wsx status
        // from-hook` is its own short-lived process racing the same row, so a
        // separate SELECT could observe a state that no longer holds by the time
        // the write lands — letting a Waiting clobber a Busy written just after
        // the read, or suppressing one against a Busy already superseded. The
        // `DO UPDATE ... WHERE` reads the stored row inside the same statement.
        // A missing row still inserts: with no Busy on record there is nothing
        // to protect.
        let Some(agent) = self.status_target(id, agent)? else {
            self.conn().execute(
                "INSERT INTO workspace_status \
                     (workspace_id, state, message, source, reported_at) \
                 VALUES (?1, ?2, NULL, ?3, ?4) \
                 ON CONFLICT(workspace_id) DO UPDATE SET \
                     state       = excluded.state, \
                     message     = excluded.message, \
                     source      = excluded.source, \
                     reported_at = excluded.reported_at \
                 WHERE workspace_status.state <> ?5",
                rusqlite::params![
                    id.0,
                    state.as_str(),
                    source,
                    now_ms(),
                    ReportedState::Busy.as_str(),
                ],
            )?;
            return Ok(());
        };
        let tx = self.conn().unchecked_transaction()?;
        tx.execute(
            "INSERT INTO agent_status \
                 (agent_id, workspace_id, state, message, source, reported_at) \
             VALUES (?1, ?2, ?3, NULL, ?4, ?5) \
             ON CONFLICT(agent_id) DO UPDATE SET \
                 workspace_id = excluded.workspace_id, \
                 state        = excluded.state, \
                 message      = excluded.message, \
                 source       = excluded.source, \
                 reported_at  = excluded.reported_at \
             WHERE agent_status.state <> ?6",
            rusqlite::params![
                agent.0,
                id.0,
                state.as_str(),
                source,
                now_ms(),
                ReportedState::Busy.as_str(),
            ],
        )?;
        rederive_workspace_status(&tx, id)?;
        tx.commit()?;
        Ok(())
    }

    /// Clear every agent's status and the derived workspace row.
    pub fn clear_workspace_status(&self, id: WorkspaceId) -> Result<()> {
        let tx = self.conn().unchecked_transaction()?;
        tx.execute("DELETE FROM agent_status WHERE workspace_id = ?1", [id.0])?;
        tx.execute(
            "DELETE FROM workspace_status WHERE workspace_id = ?1",
            [id.0],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Clear one agent's status and re-derive the workspace row from the
    /// agents that are left.
    pub fn clear_agent_status(&self, id: WorkspaceId, agent: AgentInstanceId) -> Result<()> {
        let tx = self.conn().unchecked_transaction()?;
        tx.execute(
            "DELETE FROM agent_status WHERE agent_id = ?1 AND workspace_id = ?2",
            rusqlite::params![agent.0, id.0],
        )?;
        rederive_workspace_status(&tx, id)?;
        tx.commit()?;
        Ok(())
    }

    /// Each agent's last reported status in workspace `id`, keyed by
    /// instance. Agents that never reported are absent.
    pub fn agent_statuses(
        &self,
        id: WorkspaceId,
    ) -> Result<HashMap<AgentInstanceId, ReportedStatus>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT agent_id, state, message, source, reported_at \
             FROM agent_status WHERE workspace_id = ?1",
        )?;
        let rows = stmt.query_map([id.0], |r| {
            Ok((
                AgentInstanceId(r.get(0)?),
                row_to_reported_status_offset1(r)?,
            ))
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// One agent's last reported status, if it has reported one.
    pub fn agent_status(&self, agent: AgentInstanceId) -> Result<Option<ReportedStatus>> {
        let r = self
            .conn()
            .query_row(
                "SELECT state, message, source, reported_at \
                 FROM agent_status WHERE agent_id = ?1",
                [agent.0],
                row_to_reported_status,
            )
            .optional()?;
        Ok(r)
    }

    /// Every agent's status across all workspaces, keyed by instance.
    pub fn all_agent_statuses(&self) -> Result<HashMap<AgentInstanceId, ReportedStatus>> {
        let mut stmt = self
            .conn()
            .prepare("SELECT agent_id, state, message, source, reported_at FROM agent_status")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                AgentInstanceId(r.get(0)?),
                row_to_reported_status_offset1(r)?,
            ))
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// The instance a push is attributed to: `agent` when it is attached to
    /// workspace `id`, else the primary. `None` only when the workspace has
    /// no instance rows.
    fn status_target(
        &self,
        id: WorkspaceId,
        agent: Option<AgentInstanceId>,
    ) -> Result<Option<AgentInstanceId>> {
        if let Some(a) = agent {
            if self
                .workspace_agents_by_id(a)?
                .is_some_and(|inst| inst.workspace_id == id)
            {
                return Ok(Some(a));
            }
        }
        self.primary_instance_id(id)
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

/// Rewrite workspace `id`'s derived `workspace_status` row from its agents'
/// rows, or delete it when no agent has reported.
///
/// The workspace shows the state that most needs a human, so one peer's
/// `done` cannot hide another's `blocked`: blocked > working > busy >
/// waiting > done, the most recent push breaking ties. That row's message,
/// source and `reported_at` come along with it, so the dashboard's freshness
/// gate (`app::status::fresh_reported`) judges the push it actually shows.
pub(crate) fn rederive_workspace_status(
    conn: &rusqlite::Connection,
    id: WorkspaceId,
) -> Result<()> {
    conn.execute(
        "DELETE FROM workspace_status WHERE workspace_id = ?1",
        [id.0],
    )?;
    conn.execute(
        "INSERT INTO workspace_status (workspace_id, state, message, source, reported_at) \
         SELECT workspace_id, state, message, source, reported_at \
         FROM agent_status WHERE workspace_id = ?1 \
         ORDER BY CASE state \
                      WHEN 'blocked' THEN 0 \
                      WHEN 'working' THEN 1 \
                      WHEN 'busy'    THEN 2 \
                      WHEN 'waiting' THEN 3 \
                      ELSE 4 \
                  END, \
                  reported_at DESC, agent_id ASC \
         LIMIT 1",
        [id.0],
    )?;
    Ok(())
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
        store.add_primary_agent(ws, AgentKind::Claude, 1).unwrap();
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
            .apply_hook_status(ws, None, ReportedState::Busy, "hook")
            .unwrap();
        store
            .apply_hook_status(ws, None, ReportedState::Waiting, "notify")
            .unwrap();
        let got = store.workspace_status(ws).unwrap().unwrap();
        assert_eq!(got.state, ReportedState::Busy);
        // The suppressed push must leave the row entirely untouched, not just
        // its state — a distinct `source` proves the upsert took no branch.
        assert_eq!(got.source, "hook");
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
                .apply_hook_status(ws, None, ReportedState::Busy, "hook")
                .unwrap();
            store.apply_hook_status(ws, None, next, "hook").unwrap();
            assert_eq!(state_of(&store, ws), Some(next), "{next:?} must win");
        }
    }

    #[test]
    fn waiting_lands_normally_when_not_busy() {
        // The suppression is scoped to the Busy condition — an ordinary idle
        // prompt after a finished turn still reports Waiting.
        let (store, ws) = store_with_workspace();
        store
            .apply_hook_status(ws, None, ReportedState::Done, "hook")
            .unwrap();
        store
            .apply_hook_status(ws, None, ReportedState::Waiting, "hook")
            .unwrap();
        assert_eq!(state_of(&store, ws), Some(ReportedState::Waiting));

        let (store, ws) = store_with_workspace();
        store
            .apply_hook_status(ws, None, ReportedState::Waiting, "hook")
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
            .apply_hook_status(ws, None, ReportedState::Busy, "hook")
            .unwrap();
        store
            .set_workspace_status(ws, ReportedState::Waiting, Some("parked"), "model")
            .unwrap();
        assert_eq!(state_of(&store, ws), Some(ReportedState::Waiting));
    }

    #[test]
    fn waiting_inserts_again_after_a_clear() {
        // `clear_workspace_status` deletes the row, so the conditional upsert
        // must fall through to its INSERT arm rather than silently no-op.
        let (store, ws) = store_with_workspace();
        store
            .apply_hook_status(ws, None, ReportedState::Busy, "hook")
            .unwrap();
        store.clear_workspace_status(ws).unwrap();
        store
            .apply_hook_status(ws, None, ReportedState::Waiting, "hook")
            .unwrap();
        assert_eq!(state_of(&store, ws), Some(ReportedState::Waiting));
    }

    #[test]
    fn busy_is_honored_across_connections() {
        // The rule exists to arbitrate between separate `wsx status from-hook`
        // processes, so it must read the *stored* row and not any connection-
        // local state. Two Stores over one file stand in for two processes.
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("state.db");
        let writer = Store::open(&db).unwrap();
        let repo = writer
            .add_repo(std::path::Path::new("/tmp/r"), "r", "r/")
            .unwrap();
        let ws = writer
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
        writer.add_primary_agent(ws, AgentKind::Claude, 1).unwrap();
        writer
            .apply_hook_status(ws, None, ReportedState::Busy, "hook")
            .unwrap();

        let other = Store::open(&db).unwrap();
        other
            .apply_hook_status(ws, None, ReportedState::Waiting, "hook")
            .unwrap();
        assert_eq!(state_of(&other, ws), Some(ReportedState::Busy));
        assert_eq!(state_of(&writer, ws), Some(ReportedState::Busy));
    }

    #[test]
    fn production_park_sequence_never_reports_done_early() {
        // The whole chain the live bug ran through, replayed from payloads
        // captured off Claude Code 2.1.226: parse_event -> apply_hook_status.
        // The two idle prompts are the 60s notification timer firing while the
        // session sits parked on its subagent.
        use crate::agent::status::for_agent;
        let claude = for_agent(AgentKind::Claude);
        let (store, ws) = store_with_workspace();
        let sequence = [
            (
                serde_json::json!({"hook_event_name": "UserPromptSubmit"}),
                ReportedState::Working,
            ),
            (
                serde_json::json!({
                    "hook_event_name": "Stop",
                    "last_assistant_message": "Dispatched the reviewer.",
                    "background_tasks": [
                        {"id": "a1", "type": "subagent", "status": "running", "description": "probe"}
                    ]
                }),
                ReportedState::Busy,
            ),
            (
                serde_json::json!({
                    "hook_event_name": "Notification",
                    "notification_type": "idle_prompt",
                    "message": "Claude is waiting for your input"
                }),
                ReportedState::Busy,
            ),
            (
                serde_json::json!({
                    "hook_event_name": "Notification",
                    "notification_type": "idle_prompt",
                    "message": "Claude is waiting for your input"
                }),
                ReportedState::Busy,
            ),
            (
                serde_json::json!({"hook_event_name": "UserPromptSubmit"}),
                ReportedState::Working,
            ),
            (
                serde_json::json!({
                    "hook_event_name": "Stop",
                    "last_assistant_message": "All done.",
                    "background_tasks": []
                }),
                ReportedState::Done,
            ),
        ];
        for (payload, want) in sequence {
            if let Some(state) = claude.parse_event(&payload) {
                store.apply_hook_status(ws, None, state, "hook").unwrap();
            }
            assert_eq!(
                state_of(&store, ws),
                Some(want),
                "after {}",
                payload["hook_event_name"]
            );
        }
    }

    #[test]
    fn hook_push_records_its_source() {
        let (store, ws) = store_with_workspace();
        store
            .apply_hook_status(ws, None, ReportedState::Busy, "hook")
            .unwrap();
        let got = store.workspace_status(ws).unwrap().unwrap();
        assert_eq!(got.source, "hook");
        assert_eq!(got.message, None);
    }

    /// A workspace with a primary claude and a second claude peer.
    fn store_with_peer() -> (Store, WorkspaceId, AgentInstanceId, AgentInstanceId) {
        let (store, ws) = store_with_workspace();
        let primary = store.primary_instance_id(ws).unwrap().unwrap();
        let peer = store.add_workspace_agent(ws, AgentKind::Claude).unwrap().id;
        (store, ws, primary, peer)
    }

    #[test]
    fn peers_keep_separate_status_rows() {
        // The bug this table exists for: a peer's push used to overwrite the
        // primary's on the single workspace row.
        let (store, ws, primary, peer) = store_with_peer();
        store
            .set_agent_status(
                ws,
                Some(primary),
                ReportedState::Working,
                Some("impl"),
                "model",
            )
            .unwrap();
        store
            .set_agent_status(
                ws,
                Some(peer),
                ReportedState::Done,
                Some("reviewed"),
                "model",
            )
            .unwrap();
        let all = store.agent_statuses(ws).unwrap();
        assert_eq!(all[&primary].state, ReportedState::Working);
        assert_eq!(all[&primary].message.as_deref(), Some("impl"));
        assert_eq!(all[&peer].state, ReportedState::Done);
        // The peer finishing must not paint the workspace done while the
        // primary is still working.
        assert_eq!(state_of(&store, ws), Some(ReportedState::Working));
    }

    #[test]
    fn derived_row_prefers_the_state_that_needs_a_human() {
        let (store, ws, primary, peer) = store_with_peer();
        store
            .set_agent_status(
                ws,
                Some(peer),
                ReportedState::Blocked,
                Some("need a call"),
                "model",
            )
            .unwrap();
        store
            .set_agent_status(ws, Some(primary), ReportedState::Working, None, "hook")
            .unwrap();
        let got = store.workspace_status(ws).unwrap().unwrap();
        assert_eq!(got.state, ReportedState::Blocked);
        // The derived row carries the chosen agent's message with it.
        assert_eq!(got.message.as_deref(), Some("need a call"));

        // Once the blocker clears, the next-most-urgent state takes over.
        store.clear_agent_status(ws, peer).unwrap();
        assert_eq!(state_of(&store, ws), Some(ReportedState::Working));
        store.clear_agent_status(ws, primary).unwrap();
        assert_eq!(state_of(&store, ws), None);
    }

    #[test]
    fn unattributed_push_lands_on_the_primary() {
        let (store, ws, primary, peer) = store_with_peer();
        store
            .set_workspace_status(ws, ReportedState::Waiting, None, "model")
            .unwrap();
        let all = store.agent_statuses(ws).unwrap();
        assert_eq!(all[&primary].state, ReportedState::Waiting);
        assert!(!all.contains_key(&peer));
    }

    #[test]
    fn instance_from_another_workspace_falls_back_to_the_primary() {
        // A stale or foreign WSX_AGENT_INSTANCE_ID must not write a row under
        // this workspace for an agent that isn't attached to it.
        let (store, ws, primary, _) = store_with_peer();
        let repo = store.repos().unwrap()[0].id;
        let other = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "other",
                branch: "r/other",
                worktree_path: std::path::Path::new("/tmp/r/other"),
                yolo: false,
                agent: AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        let foreign = store
            .add_primary_agent(other, AgentKind::Claude, 1)
            .unwrap()
            .id;
        store
            .set_agent_status(ws, Some(foreign), ReportedState::Done, None, "model")
            .unwrap();
        let all = store.agent_statuses(ws).unwrap();
        assert_eq!(all.keys().copied().collect::<Vec<_>>(), vec![primary]);
    }

    #[test]
    fn busy_suppression_is_per_agent() {
        // A peer's idle prompt is that peer's news; it must neither be
        // swallowed by the primary's Busy nor clobber it.
        let (store, ws, primary, peer) = store_with_peer();
        store
            .apply_hook_status(ws, Some(primary), ReportedState::Busy, "hook")
            .unwrap();
        store
            .apply_hook_status(ws, Some(peer), ReportedState::Waiting, "hook")
            .unwrap();
        let all = store.agent_statuses(ws).unwrap();
        assert_eq!(all[&primary].state, ReportedState::Busy);
        assert_eq!(all[&peer].state, ReportedState::Waiting);
        assert_eq!(state_of(&store, ws), Some(ReportedState::Busy));
    }

    #[test]
    fn clear_workspace_status_clears_every_agent() {
        let (store, ws, primary, peer) = store_with_peer();
        store
            .set_agent_status(ws, Some(primary), ReportedState::Working, None, "model")
            .unwrap();
        store
            .set_agent_status(ws, Some(peer), ReportedState::Working, None, "model")
            .unwrap();
        store.clear_workspace_status(ws).unwrap();
        assert!(store.agent_statuses(ws).unwrap().is_empty());
        assert_eq!(state_of(&store, ws), None);
    }

    #[test]
    fn removing_a_peer_drops_its_status_from_the_derived_row() {
        let (store, ws, primary, peer) = store_with_peer();
        store
            .set_agent_status(ws, Some(primary), ReportedState::Done, None, "model")
            .unwrap();
        store
            .set_agent_status(ws, Some(peer), ReportedState::Blocked, None, "model")
            .unwrap();
        assert_eq!(state_of(&store, ws), Some(ReportedState::Blocked));
        store.remove_workspace_agent(peer).unwrap();
        assert!(!store.agent_statuses(ws).unwrap().contains_key(&peer));
        assert_eq!(state_of(&store, ws), Some(ReportedState::Done));
    }

    #[test]
    fn migration_backfills_the_primary_once() {
        // Simulate a pre-v26 database: a workspace_status row and no
        // agent_status table. Reopening must copy the row onto the primary
        // exactly once, and later reopens (the ladder re-runs every open)
        // must not re-copy a derived row onto a primary that never reported.
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("state.db");
        let (ws, peer) = {
            let store = Store::open(&db).unwrap();
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
            store.add_primary_agent(ws, AgentKind::Claude, 1).unwrap();
            let peer = store.add_workspace_agent(ws, AgentKind::Claude).unwrap().id;
            store
                .conn()
                .execute_batch("DROP TABLE agent_status")
                .unwrap();
            store
                .conn()
                .execute(
                    "INSERT INTO workspace_status (workspace_id, state, message, source, reported_at) \
                     VALUES (?1, 'blocked', 'legacy', 'model', 5)",
                    [ws.0],
                )
                .unwrap();
            (ws, peer)
        };
        let store = Store::open(&db).unwrap();
        let primary = store.primary_instance_id(ws).unwrap().unwrap();
        let all = store.agent_statuses(ws).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[&primary].message.as_deref(), Some("legacy"));

        // Now only the peer has a row; the derived row is the peer's.
        store.clear_agent_status(ws, primary).unwrap();
        store
            .set_agent_status(ws, Some(peer), ReportedState::Working, None, "model")
            .unwrap();
        drop(store);
        let store = Store::open(&db).unwrap();
        let all = store.agent_statuses(ws).unwrap();
        assert!(!all.contains_key(&primary), "backfill must not re-run");
    }
}
