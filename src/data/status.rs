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
    /// attributes the push to the primary instance; an `agent` no longer
    /// attached to the workspace drops the push. A workspace with no
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
        let tx = self.status_tx()?;
        let agent = match status_target(&tx, id, agent)? {
            Target::Agent(a) => a,
            Target::Gone => return Ok(()),
            Target::NoAgents => {
                tx.execute(
                    "INSERT OR REPLACE INTO workspace_status \
                     (workspace_id, state, message, source, reported_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![id.0, state.as_str(), message, source, now_ms()],
                )?;
                tx.commit()?;
                return Ok(());
            }
        };
        adopt_legacy_writes(&tx, id)?;
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

    /// Every status write runs in an IMMEDIATE transaction: it reads before
    /// it writes (target validation, legacy adoption), and a DEFERRED
    /// transaction that later upgrades to a writer fails with SQLITE_BUSY
    /// instead of waiting on `busy_timeout`. Holding the write lock from the
    /// start also means an agent removed by another process between our
    /// validation and our insert cannot happen: removal needs the same lock.
    fn status_tx(&self) -> Result<rusqlite::Transaction<'_>> {
        Ok(rusqlite::Transaction::new_unchecked(
            self.conn(),
            rusqlite::TransactionBehavior::Immediate,
        )?)
    }

    /// Apply a hook/notify-sourced push (`wsx status from-hook` /
    /// `from-notify`) for `agent` (`None` = the primary). Unlike an explicit
    /// `wsx status set`, a hook push is an *inference* from the harness's
    /// lifecycle events, so it does not simply overwrite the row: `merge_hook`
    /// decides what it does given the row it lands on. Explicit pushes go
    /// through `set_agent_status` and stay authoritative.
    ///
    /// The stored row is read and written inside one IMMEDIATE transaction
    /// (`status_tx`), so the decision still holds when the write lands even
    /// though every `wsx status from-hook` is its own short-lived process
    /// racing the same row.
    pub fn apply_hook_status(
        &self,
        id: WorkspaceId,
        agent: Option<AgentInstanceId>,
        state: ReportedState,
        source: &str,
    ) -> Result<()> {
        let tx = self.status_tx()?;
        // The table and key column the push lands in: the agent's own row, or
        // the workspace row directly when it has no instance rows at all.
        let (table, key_col, key) = match status_target(&tx, id, agent)? {
            Target::Agent(a) => {
                adopt_legacy_writes(&tx, id)?;
                ("agent_status", "agent_id", a.0)
            }
            Target::Gone => return Ok(()),
            Target::NoAgents => ("workspace_status", "workspace_id", id.0),
        };
        let prev: Option<(String, String)> = tx
            .query_row(
                &format!("SELECT state, source FROM {table} WHERE {key_col} = ?1"),
                [key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let prev = prev.and_then(|(st, src)| Some((ReportedState::from_stored(&st)?, src)));
        let now = now_ms();
        match merge_hook(prev.as_ref().map(|(st, src)| (*st, src.as_str())), state) {
            HookMerge::Drop => return Ok(()),
            HookMerge::Refresh => {
                tx.execute(
                    &format!("UPDATE {table} SET reported_at = ?1 WHERE {key_col} = ?2"),
                    rusqlite::params![now, key],
                )?;
            }
            HookMerge::KeepMessage => {
                tx.execute(
                    &format!(
                        "UPDATE {table} SET state = ?1, source = ?2, reported_at = ?3 \
                         WHERE {key_col} = ?4"
                    ),
                    rusqlite::params![state.as_str(), source, now, key],
                )?;
            }
            HookMerge::Replace => {
                if table == "agent_status" {
                    tx.execute(
                        "INSERT OR REPLACE INTO agent_status \
                             (agent_id, workspace_id, state, message, source, reported_at) \
                         VALUES (?1, ?2, ?3, NULL, ?4, ?5)",
                        rusqlite::params![key, id.0, state.as_str(), source, now],
                    )?;
                } else {
                    tx.execute(
                        "INSERT OR REPLACE INTO workspace_status \
                             (workspace_id, state, message, source, reported_at) \
                         VALUES (?1, ?2, NULL, ?3, ?4)",
                        rusqlite::params![id.0, state.as_str(), source, now],
                    )?;
                }
            }
        }
        if table == "agent_status" {
            rederive_workspace_status(&tx, id)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Clear every agent's status and the derived workspace row.
    pub fn clear_workspace_status(&self, id: WorkspaceId) -> Result<()> {
        let tx = self.status_tx()?;
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
        let tx = self.status_tx()?;
        adopt_legacy_writes(&tx, id)?;
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
            "SELECT s.agent_id, s.state, s.message, s.source, s.reported_at \
             FROM agent_status s JOIN workspace_agents a ON a.id = s.agent_id \
             WHERE s.workspace_id = ?1 AND a.workspace_id = ?1",
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
                "SELECT s.state, s.message, s.source, s.reported_at \
                 FROM agent_status s JOIN workspace_agents a \
                   ON a.id = s.agent_id AND a.workspace_id = s.workspace_id \
                 WHERE s.agent_id = ?1",
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

/// Where a push lands.
enum Target {
    Agent(AgentInstanceId),
    /// The workspace has no instance rows: write `workspace_status` directly.
    NoAgents,
    /// The named agent is no longer attached (removed since the caller
    /// resolved it): drop the push rather than orphan a row or pin a dead
    /// peer's state on the primary.
    Gone,
}

/// Resolve a push's target inside the write transaction, so the answer still
/// holds when the insert lands: `agent` when it is attached to workspace
/// `id`, the primary when `agent` is `None`.
fn status_target(
    conn: &rusqlite::Connection,
    id: WorkspaceId,
    agent: Option<AgentInstanceId>,
) -> Result<Target> {
    if let Some(a) = agent {
        let attached: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM workspace_agents WHERE id = ?1 AND workspace_id = ?2)",
            rusqlite::params![a.0, id.0],
            |r| r.get(0),
        )?;
        return Ok(if attached {
            Target::Agent(a)
        } else {
            Target::Gone
        });
    }
    Ok(conn
        .query_row(
            "SELECT id FROM workspace_agents WHERE workspace_id = ?1 AND is_primary = 1",
            [id.0],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
        .map_or(Target::NoAgents, |p| Target::Agent(AgentInstanceId(p))))
}

/// Fold in what a pre-per-agent `wsx` binary did to workspace `id` since the
/// last derivation. Such a binary shares this database until it is rebuilt
/// (the user's installed `wsx` runs every agent's hooks) and knows only the
/// `workspace_status` row, so without this its pushes would be overwritten
/// by the next re-derivation and its clears undone by it.
///
/// Both are detectable because a derived row always copies an agent row:
/// - a workspace row newer than every agent row was written directly, so it
///   becomes the primary's status (the agent an old binary described);
/// - no workspace row while agent rows exist means it was deleted directly
///   (`wsx status clear`), so the agent rows go too.
fn adopt_legacy_writes(conn: &rusqlite::Connection, id: WorkspaceId) -> Result<()> {
    // Orphans included: a derived row copied from an agent an older binary
    // has since removed is still derived, not a direct write.
    let newest_agent: Option<i64> = conn.query_row(
        "SELECT MAX(reported_at) FROM agent_status WHERE workspace_id = ?1",
        [id.0],
        |r| r.get(0),
    )?;
    let row: Option<i64> = conn
        .query_row(
            "SELECT reported_at FROM workspace_status WHERE workspace_id = ?1",
            [id.0],
            |r| r.get(0),
        )
        .optional()?;
    match (row, newest_agent) {
        (Some(at), newest) if newest.is_none_or(|n| at > n) => {
            conn.execute(
                "INSERT OR REPLACE INTO agent_status \
                     (agent_id, workspace_id, state, message, source, reported_at) \
                 SELECT a.id, s.workspace_id, s.state, s.message, s.source, s.reported_at \
                 FROM workspace_status s \
                 JOIN workspace_agents a ON a.workspace_id = s.workspace_id AND a.is_primary = 1 \
                 WHERE s.workspace_id = ?1",
                [id.0],
            )?;
        }
        (None, Some(_)) => {
            conn.execute("DELETE FROM agent_status WHERE workspace_id = ?1", [id.0])?;
        }
        _ => {}
    }
    Ok(())
}

/// What a hook push does to the row it lands on (see `merge_hook`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookMerge {
    /// Write the hook's state with no message, attributed to the hook.
    Replace,
    /// Keep the stored state, message and source; refresh `reported_at` only.
    Refresh,
    /// Write the hook's state, keeping the stored message.
    KeepMessage,
    /// Leave the row untouched.
    Drop,
}

/// Decide what a hook reporting `hook` does to a stored row `prev`
/// (`(state, source)`, `None` when the agent has none).
///
/// **An idle prompt never replaces `Busy`.** `Busy` records a *condition* —
/// background work is in flight — while every other state records an
/// *event*. Claude keeps firing its `Notification` `idle_prompt` on a timer
/// for as long as the input box is free, which is exactly the whole time a
/// session sits parked on a background subagent. Were it to overwrite `Busy`
/// with `Waiting`, `classify` would consult the JSONL `stopped_kind` and flip
/// the row back to the turn's ✓ Complete — the false completion `Busy` exists
/// to prevent. `Working`, `Blocked` (a permission prompt) and `Done` (a `Stop`
/// whose `background_tasks` has emptied) all still supersede `Busy`.
///
/// **A model-set row keeps its message.** The doctrine has agents push
/// `wsx status set <state> --message …` at every transition, and the harness
/// hook reports the turn's end seconds later. Last-writer-wins erased every
/// such message. So, over a row whose source is `model`:
/// - a hook repeating the stored state only refreshes `reported_at` — it adds
///   nothing but freshness, which the dashboard's gate
///   (`app::status::fresh_reported`) needs after the transcript grew past a
///   mid-turn `set`;
/// - a turn-end or idle hook (`Done`, `Blocked`, `Waiting`) never overrides a
///   model-set `Done`, `Blocked` or `Waiting`: those are the model's own
///   account of why its turn ended, and the hook's `Done`/`Blocked` split is a
///   `?`-suffix guess. Refresh it instead;
/// - a model `Working` parking on background work (`Busy`) keeps its message:
///   it describes the work still in flight;
/// - anything else is new information and replaces the row, message and all:
///   `Working`/`Busy` over a model turn-end is a new turn the old message no
///   longer describes, and a turn-end over a model `Working` means the model
///   finished without saying so.
///
/// All of this is per agent row: one peer's hook never touches another's.
/// Arrival order, not event order, decides: hook processes are independent,
/// so a stalled `Stop` landing after a newer `UserPromptSubmit` can briefly
/// resurrect a stale state until the following event. Self-correcting.
///
/// Nothing here expires a stale state from a session that died mid-flight;
/// the dashboard's escape is the `session_running` guard in
/// `Status::classify`. Waybar and the menubar rows render
/// `all_workspace_status` directly and show the last stored state.
fn merge_hook(prev: Option<(ReportedState, &str)>, hook: ReportedState) -> HookMerge {
    use ReportedState::*;
    let Some((prev, source)) = prev else {
        return HookMerge::Replace;
    };
    if prev == Busy && hook == Waiting {
        return HookMerge::Drop;
    }
    if source != "model" {
        return HookMerge::Replace;
    }
    let turn_end = |s| matches!(s, Done | Blocked | Waiting);
    match (prev, hook) {
        _ if prev == hook => HookMerge::Refresh,
        _ if turn_end(prev) && turn_end(hook) => HookMerge::Refresh,
        (Working, Busy) => HookMerge::KeepMessage,
        _ => HookMerge::Replace,
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
    // Rows for agents no longer attached here (removed by another process,
    // or by an older binary that doesn't know this table) never count.
    conn.execute(
        "DELETE FROM agent_status WHERE workspace_id = ?1 AND agent_id NOT IN \
             (SELECT id FROM workspace_agents WHERE workspace_id = ?1)",
        [id.0],
    )?;
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
        // `clear_workspace_status` deletes the row, so `merge_hook` sees no
        // previous state and must insert rather than silently no-op.
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
    fn push_for_an_agent_not_attached_here_is_dropped() {
        // The store only sees an explicit id the CLI already matched to this
        // workspace, so a mismatch at write time means the agent went away
        // in between. Neither orphan a row nor pin its state on the primary.
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
        assert!(store.agent_statuses(ws).unwrap().is_empty());
        assert_eq!(state_of(&store, ws), None);
        let _ = primary;
    }

    #[test]
    fn push_for_a_removed_peer_is_dropped() {
        // The removal race: a hook resolved its peer, then the peer was
        // removed before the hook's write landed.
        let (store, ws, primary, peer) = store_with_peer();
        store
            .set_agent_status(ws, Some(primary), ReportedState::Done, None, "hook")
            .unwrap();
        store.remove_workspace_agent(peer).unwrap();
        store
            .apply_hook_status(ws, Some(peer), ReportedState::Blocked, "hook")
            .unwrap();
        let all = store.agent_statuses(ws).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[&primary].state, ReportedState::Done);
        assert_eq!(state_of(&store, ws), Some(ReportedState::Done));
        let n: i64 = store
            .conn()
            .query_row("SELECT count(*) FROM agent_status", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "no orphan row");
    }

    #[test]
    fn derived_row_follows_the_full_priority_order_then_recency() {
        use ReportedState::*;
        let (store, ws) = store_with_workspace();
        let agents: Vec<_> = (0..5)
            .map(|_| store.add_workspace_agent(ws, AgentKind::Codex).unwrap().id)
            .collect();
        // Report least urgent first; each new, more urgent one must win.
        for (a, st) in agents.iter().zip([Done, Waiting, Busy, Working, Blocked]) {
            store
                .set_agent_status(ws, Some(*a), st, None, "hook")
                .unwrap();
            assert_eq!(state_of(&store, ws), Some(st));
        }
        // Equal urgency: the newest push wins.
        std::thread::sleep(std::time::Duration::from_millis(2));
        store
            .set_agent_status(ws, Some(agents[0]), Blocked, Some("newer"), "model")
            .unwrap();
        let got = store.workspace_status(ws).unwrap().unwrap();
        assert_eq!(got.message.as_deref(), Some("newer"));
    }

    #[test]
    fn orphan_rows_never_count() {
        // A row for an agent an older binary removed (it doesn't know this
        // table) must neither show nor win the derivation.
        let (store, ws, primary, _) = store_with_peer();
        store
            .conn()
            .execute(
                "INSERT INTO agent_status (agent_id, workspace_id, state, message, source, reported_at) \
                 VALUES (999999, ?1, 'blocked', NULL, 'hook', 1)",
                [ws.0],
            )
            .unwrap();
        assert!(
            !store
                .agent_statuses(ws)
                .unwrap()
                .contains_key(&AgentInstanceId(999999))
        );
        store
            .set_agent_status(ws, Some(primary), ReportedState::Done, None, "hook")
            .unwrap();
        assert_eq!(state_of(&store, ws), Some(ReportedState::Done));
    }

    /// What a pre-per-agent binary does: touch only the workspace row.
    fn legacy_write(store: &Store, ws: WorkspaceId, state: &str, at: i64) {
        store
            .conn()
            .execute(
                "INSERT OR REPLACE INTO workspace_status \
                     (workspace_id, state, message, source, reported_at) \
                 VALUES (?1, ?2, NULL, 'hook', ?3)",
                rusqlite::params![ws.0, state, at],
            )
            .unwrap();
    }

    #[test]
    fn a_legacy_binary_push_is_adopted_as_the_primarys() {
        // New primary Working, then an old binary reports the primary Done.
        // A later peer push must not resurrect the primary's Working.
        let (store, ws, primary, peer) = store_with_peer();
        store
            .set_agent_status(ws, Some(primary), ReportedState::Working, None, "hook")
            .unwrap();
        legacy_write(&store, ws, "done", now_ms() + 1_000);
        store
            .set_agent_status(ws, Some(peer), ReportedState::Waiting, None, "hook")
            .unwrap();
        let all = store.agent_statuses(ws).unwrap();
        assert_eq!(all[&primary].state, ReportedState::Done);
        assert_eq!(state_of(&store, ws), Some(ReportedState::Waiting));
    }

    #[test]
    fn a_derived_row_is_not_mistaken_for_a_legacy_push() {
        // The derived row copies a peer's row; that is not a direct write and
        // must not be copied onto the primary.
        let (store, ws, primary, peer) = store_with_peer();
        store
            .set_agent_status(ws, Some(peer), ReportedState::Blocked, None, "model")
            .unwrap();
        store
            .set_agent_status(ws, Some(peer), ReportedState::Working, None, "model")
            .unwrap();
        assert!(!store.agent_statuses(ws).unwrap().contains_key(&primary));
    }

    #[test]
    fn a_legacy_binary_clear_clears_every_agent() {
        let (store, ws, primary, peer) = store_with_peer();
        store
            .set_agent_status(ws, Some(peer), ReportedState::Blocked, None, "model")
            .unwrap();
        // Old `wsx status clear`: deletes only the workspace row.
        store
            .conn()
            .execute(
                "DELETE FROM workspace_status WHERE workspace_id = ?1",
                [ws.0],
            )
            .unwrap();
        store
            .set_agent_status(ws, Some(primary), ReportedState::Working, None, "hook")
            .unwrap();
        let all = store.agent_statuses(ws).unwrap();
        assert!(!all.contains_key(&peer), "the peer's Blocked was cleared");
        assert_eq!(state_of(&store, ws), Some(ReportedState::Working));
    }

    #[test]
    fn an_upgrade_that_lost_the_race_does_not_backfill_again() {
        // Two processes open a pre-v26 database at once and both see no
        // agent_status table. The first creates + backfills; the second must
        // re-check under the lock rather than backfill again (which failed
        // Store::open with a UNIQUE conflict).
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("state.db");
        {
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
            store
                .set_workspace_status(ws, ReportedState::Working, None, "hook")
                .unwrap();
            store
                .conn()
                .execute_batch("DROP TABLE agent_status")
                .unwrap();
        }
        let first = rusqlite::Connection::open(&db).unwrap();
        let second = rusqlite::Connection::open(&db).unwrap();
        crate::data::schema::create_agent_status_locked(&first).unwrap();
        crate::data::schema::create_agent_status_locked(&second).unwrap();
        let n: i64 = second
            .query_row("SELECT count(*) FROM agent_status", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
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

    fn status_of(store: &Store, ws: WorkspaceId) -> ReportedStatus {
        store.workspace_status(ws).unwrap().unwrap()
    }

    /// Backdate every status row so a later write's `reported_at` is
    /// observably newer.
    fn backdate(store: &Store, ws: WorkspaceId) {
        for table in ["agent_status", "workspace_status"] {
            store
                .conn()
                .execute(
                    &format!("UPDATE {table} SET reported_at = 1 WHERE workspace_id = ?1"),
                    [ws.0],
                )
                .unwrap();
        }
    }

    #[test]
    fn turn_end_hook_keeps_a_model_done_and_its_message() {
        // The live failure: codex ran `wsx status set done --message 'rt test'`
        // and its turn-end notify, two seconds later, replaced the row with a
        // message-less `done (notify)`.
        let (store, ws) = store_with_workspace();
        store
            .set_workspace_status(ws, ReportedState::Done, Some("rt test"), "model")
            .unwrap();
        backdate(&store, ws);
        store
            .apply_hook_status(ws, None, ReportedState::Done, "notify")
            .unwrap();
        let got = status_of(&store, ws);
        assert_eq!(got.state, ReportedState::Done);
        assert_eq!(got.message.as_deref(), Some("rt test"));
        assert_eq!(got.source, "model");
        // Refreshed, so the dashboard's freshness gate still honours it after
        // the transcript grew past the mid-turn `set`.
        assert!(got.reported_at > 1);
    }

    #[test]
    fn turn_end_hook_never_overrides_a_model_turn_end_state() {
        use ReportedState::*;
        for model in [Done, Blocked, Waiting] {
            for hook in [Done, Blocked, Waiting] {
                let (store, ws) = store_with_workspace();
                store
                    .set_workspace_status(ws, model, Some("said so"), "model")
                    .unwrap();
                store.apply_hook_status(ws, None, hook, "hook").unwrap();
                let got = status_of(&store, ws);
                assert_eq!(
                    (got.state, got.message.as_deref(), got.source.as_str()),
                    (model, Some("said so"), "model"),
                    "model {model:?} then hook {hook:?}"
                );
            }
        }
    }

    #[test]
    fn same_state_hook_keeps_a_model_working_message() {
        let (store, ws) = store_with_workspace();
        store
            .set_workspace_status(ws, ReportedState::Working, Some("running tests"), "model")
            .unwrap();
        store
            .apply_hook_status(ws, None, ReportedState::Working, "hook")
            .unwrap();
        let got = status_of(&store, ws);
        assert_eq!(got.state, ReportedState::Working);
        assert_eq!(got.message.as_deref(), Some("running tests"));
        assert_eq!(got.source, "model");
    }

    #[test]
    fn busy_hook_over_model_working_keeps_the_message() {
        // The turn parked on background work the model already described.
        let (store, ws) = store_with_workspace();
        store
            .set_workspace_status(ws, ReportedState::Working, Some("running tests"), "model")
            .unwrap();
        store
            .apply_hook_status(ws, None, ReportedState::Busy, "hook")
            .unwrap();
        let got = status_of(&store, ws);
        assert_eq!(got.state, ReportedState::Busy);
        assert_eq!(got.message.as_deref(), Some("running tests"));
        assert_eq!(got.source, "hook");
    }

    #[test]
    fn new_work_supersedes_a_model_done_and_clears_its_message() {
        // A new turn started: "tests green" under `working` would describe
        // the previous turn, so the message goes with the state.
        for hook in [ReportedState::Working, ReportedState::Busy] {
            let (store, ws) = store_with_workspace();
            store
                .set_workspace_status(ws, ReportedState::Done, Some("tests green"), "model")
                .unwrap();
            store.apply_hook_status(ws, None, hook, "hook").unwrap();
            let got = status_of(&store, ws);
            assert_eq!(got.state, hook);
            assert_eq!(got.message, None);
            assert_eq!(got.source, "hook");
        }
    }

    #[test]
    fn turn_end_hook_still_supersedes_a_model_working() {
        // The model forgot to report its finish: the hook's turn-end is the
        // better signal, and "running tests" no longer describes anything.
        let (store, ws) = store_with_workspace();
        store
            .set_workspace_status(ws, ReportedState::Working, Some("running tests"), "model")
            .unwrap();
        store
            .apply_hook_status(ws, None, ReportedState::Done, "hook")
            .unwrap();
        let got = status_of(&store, ws);
        assert_eq!(got.state, ReportedState::Done);
        assert_eq!(got.message, None);
    }

    #[test]
    fn model_message_survives_a_full_turn_of_hooks() {
        // The doctrine sequence: prompt submitted, the model reports working
        // then done with messages, the turn ends — and the next prompt starts
        // fresh.
        let (store, ws) = store_with_workspace();
        let hook = |s| store.apply_hook_status(ws, None, s, "hook").unwrap();
        hook(ReportedState::Working);
        store
            .set_workspace_status(ws, ReportedState::Working, Some("tests"), "model")
            .unwrap();
        store
            .set_workspace_status(ws, ReportedState::Done, Some("shipped"), "model")
            .unwrap();
        hook(ReportedState::Done);
        hook(ReportedState::Waiting); // idle_prompt timer
        let got = status_of(&store, ws);
        assert_eq!(
            (got.state, got.message.as_deref()),
            (ReportedState::Done, Some("shipped"))
        );
        hook(ReportedState::Working);
        let got = status_of(&store, ws);
        assert_eq!((got.state, got.message), (ReportedState::Working, None));
    }

    #[test]
    fn model_rules_apply_per_agent() {
        // A peer's hook must not touch the primary's model row, and the
        // derived row carries the model's message.
        let (store, ws, primary, peer) = store_with_peer();
        store
            .set_agent_status(
                ws,
                Some(primary),
                ReportedState::Blocked,
                Some("need a call"),
                "model",
            )
            .unwrap();
        store
            .apply_hook_status(ws, Some(peer), ReportedState::Done, "hook")
            .unwrap();
        store
            .apply_hook_status(ws, Some(primary), ReportedState::Done, "hook")
            .unwrap();
        let got = status_of(&store, ws);
        assert_eq!(
            (got.state, got.message.as_deref()),
            (ReportedState::Blocked, Some("need a call"))
        );
    }

    #[test]
    fn model_rules_apply_without_agent_rows() {
        // Pre-multi-agent fixtures write `workspace_status` directly.
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
        store
            .set_workspace_status(ws, ReportedState::Done, Some("rt test"), "model")
            .unwrap();
        store
            .apply_hook_status(ws, None, ReportedState::Done, "notify")
            .unwrap();
        let got = status_of(&store, ws);
        assert_eq!(
            (got.state, got.message.as_deref()),
            (ReportedState::Done, Some("rt test"))
        );
        store
            .apply_hook_status(ws, None, ReportedState::Working, "hook")
            .unwrap();
        assert_eq!(status_of(&store, ws).message, None);
    }
}
