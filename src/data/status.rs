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
        state: ReportedState::parse(&r.get::<_, String>(0)?).unwrap_or(ReportedState::Working),
        message: r.get(1)?,
        source: r.get(2)?,
        reported_at: r.get(3)?,
    })
}

// Same as `row_to_reported_status` but for queries that select the
// workspace_id in column 0, shifting the status columns to 1..=4.
fn row_to_reported_status_offset1(r: &rusqlite::Row) -> rusqlite::Result<ReportedStatus> {
    Ok(ReportedStatus {
        state: ReportedState::parse(&r.get::<_, String>(1)?).unwrap_or(ReportedState::Working),
        message: r.get(2)?,
        source: r.get(3)?,
        reported_at: r.get(4)?,
    })
}
