//! Split-pane layout persistence (the `workspace_layouts` table): per-anchor
//! SplitTree JSON with corrupt-row self-healing.

use crate::data::store::{Store, WorkspaceId, now_ms};
use crate::error::Result;
use rusqlite::OptionalExtension;

impl Store {
    pub fn set_workspace_layout(
        &self,
        anchor: WorkspaceId,
        tree: &crate::ui::split::SplitTree,
        focus: &[usize],
    ) -> Result<()> {
        // Serialization is `?`-propagated rather than `.expect()`-ed:
        // serde_json's default 128-deep recursion limit makes a hostile
        // tree theoretically capable of erroring, and a save-flow panic
        // would crash the TUI. Callers (see `save_layout_for`) already
        // log + degrade on save failure.
        let tree_json = serde_json::to_string(tree)?;
        let focus_json = serde_json::to_string(focus)?;
        self.conn().execute(
            "INSERT OR REPLACE INTO workspace_layouts
                (anchor_workspace_id, tree_json, focus_json, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![anchor.0, tree_json, focus_json, now_ms()],
        )?;
        Ok(())
    }

    pub fn get_workspace_layout(
        &self,
        anchor: WorkspaceId,
    ) -> Result<Option<(crate::ui::split::SplitTree, Vec<usize>)>> {
        let row: Option<(String, String)> = self
            .conn()
            .query_row(
                "SELECT tree_json, focus_json FROM workspace_layouts WHERE anchor_workspace_id = ?1",
                [anchor.0],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((tree_json, focus_json)) = row else {
            return Ok(None);
        };
        // Backward-compat: layouts written before split-tree leaves carried an
        // agent instance serialized a leaf as a bare int (`{"Leaf": 5}`), which
        // no longer deserializes into the `Leaf(AttachTarget)` shape. A parse
        // failure is treated as "no saved layout" (the row is dropped and we
        // return `Ok(None)`), so any pre-existing multi-pane layout resets to
        // single-pane once after this change — acceptable for convenience state.
        match (
            serde_json::from_str::<crate::ui::split::SplitTree>(&tree_json),
            serde_json::from_str::<Vec<usize>>(&focus_json),
        ) {
            (Ok(tree), Ok(focus)) => Ok(Some((tree, focus))),
            _ => {
                tracing::warn!(
                    ?anchor,
                    "workspace_layouts row failed to parse; deleting corrupt entry"
                );
                self.conn().execute(
                    "DELETE FROM workspace_layouts WHERE anchor_workspace_id = ?1",
                    [anchor.0],
                )?;
                Ok(None)
            }
        }
    }

    pub fn delete_workspace_layout(&self, anchor: WorkspaceId) -> Result<()> {
        self.conn().execute(
            "DELETE FROM workspace_layouts WHERE anchor_workspace_id = ?1",
            [anchor.0],
        )?;
        Ok(())
    }

    pub fn list_multi_pane_layout_anchors(&self) -> Result<Vec<WorkspaceId>> {
        let mut stmt = self
            .conn()
            .prepare("SELECT anchor_workspace_id, tree_json FROM workspace_layouts ORDER BY anchor_workspace_id")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = Vec::new();
        let mut corrupt: Vec<i64> = Vec::new();
        for row in rows {
            let (anchor, tree_json) = row?;
            match serde_json::from_str::<crate::ui::split::SplitTree>(&tree_json) {
                Ok(tree) => {
                    if tree.leaves().len() > 1 {
                        out.push(WorkspaceId(anchor));
                    }
                }
                Err(_) => corrupt.push(anchor),
            }
        }
        drop(stmt);
        for anchor in corrupt {
            tracing::warn!(
                anchor,
                "workspace_layouts row failed to parse during list; deleting corrupt entry"
            );
            self.conn().execute(
                "DELETE FROM workspace_layouts WHERE anchor_workspace_id = ?1",
                [anchor],
            )?;
        }
        Ok(out)
    }
}
