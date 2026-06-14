//! Database schema: table DDL and the forward-only migration ladder.
//!
//! `migrate()` runs on every `Store::open` / `open_in_memory`. It is
//! idempotent — `SCHEMA_V1` resets `user_version` to 1 each call, so every
//! versioned step is guarded (column-adds via `add_column_if_missing`, schema
//! batches via `CREATE TABLE IF NOT EXISTS`) and safely re-runs. Lives in its
//! own module so the query methods in `store.rs` aren't buried under the DDL.

use crate::data::store::Store;
use crate::error::Result;

impl Store {
    pub(crate) fn migrate(&self) -> Result<()> {
        self.conn().execute_batch(SCHEMA_V1)?;

        let v: i64 = self
            .conn()
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if v < 2 {
            self.conn().execute_batch(SCHEMA_V2_SETTINGS)?;
            self.add_column_if_missing("repos", "custom_instructions", "custom_instructions TEXT")?;
            self.conn().execute("PRAGMA user_version = 2", [])?;
        }
        if v < 3 {
            self.add_column_if_missing("repos", "setup_script", "setup_script TEXT")?;
            self.add_column_if_missing("repos", "archive_script", "archive_script TEXT")?;
            self.conn().execute("PRAGMA user_version = 3", [])?;
        }
        if v < 4 {
            self.add_column_if_missing("workspaces", "yolo", "yolo INTEGER NOT NULL DEFAULT 0")?;
            self.conn().execute("PRAGMA user_version = 4", [])?;
        }
        if v < 5 {
            self.add_column_if_missing("repos", "pinned_commands", "pinned_commands TEXT")?;
            self.conn().execute("PRAGMA user_version = 5", [])?;
        }
        if v < 6 {
            self.add_column_if_missing("repos", "related_repos", "related_repos TEXT")?;
            self.conn().execute("PRAGMA user_version = 6", [])?;
        }
        if v < 7 {
            self.add_column_if_missing("repos", "base_branch", "base_branch TEXT")?;
            self.conn().execute("PRAGMA user_version = 7", [])?;
        }
        if v < 8 {
            self.conn().execute_batch(SCHEMA_V8_ACTIVITY_BUCKETS)?;
            self.conn().execute("PRAGMA user_version = 8", [])?;
        }
        if v < 9 {
            self.add_column_if_missing(
                "workspaces",
                "agent",
                "agent TEXT NOT NULL DEFAULT 'claude'",
            )?;
            self.conn().execute("PRAGMA user_version = 9", [])?;
        }
        if v < 10 {
            self.conn().execute_batch(SCHEMA_V10_WORKSPACE_LAYOUTS)?;
            self.conn().execute("PRAGMA user_version = 10", [])?;
        }
        if v < 11 {
            self.add_column_if_missing("repos", "detail_bar_config", "detail_bar_config TEXT")?;
            self.conn().execute("PRAGMA user_version = 11", [])?;
        }
        if v < 12 {
            self.conn().execute_batch(SCHEMA_V12_MULTI_AGENT)?;
            // Backfill one primary instance row per existing workspace from the
            // denormalized workspaces.agent column.
            self.conn().execute(
                "INSERT OR IGNORE INTO workspace_agents \
                     (workspace_id, agent, ordinal, is_primary, created_at)
                 SELECT id, agent, 1, 1, created_at FROM workspaces",
                [],
            )?;
            self.conn().execute("PRAGMA user_version = 12", [])?;
        }
        if v < 13 {
            self.add_column_if_missing("repos", "chronology_config", "chronology_config TEXT")?;
            self.conn().execute("PRAGMA user_version = 13", [])?;
        }
        if v < 14 {
            let has_col: i64 = self.conn().query_row(
                "SELECT count(*) FROM pragma_table_info('repos') WHERE name = 'sort_order'",
                [],
                |r| r.get(0),
            )?;
            if has_col == 0 {
                // Add the column and seed initial ranks atomically. Done in one
                // transaction so a crash between the two can't leave every repo
                // stuck at the default sort_order=0: `migrate()` re-runs every
                // startup (SCHEMA_V1 resets user_version to 1), but the column
                // would already exist, so the seed would never run again and
                // swaps (which preserve the value set) would all be no-ops. The
                // transaction makes it all-or-nothing — a failed migration rolls
                // back and retries cleanly on the next open.
                let tx = self.conn().unchecked_transaction()?;
                tx.execute(
                    "ALTER TABLE repos ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0",
                    [],
                )?;
                // Seed a unique, deterministic alphabetical rank (case-insensitive,
                // id as tiebreak) for repos that predate this column. Stays inside
                // the column-creation guard so re-runs never clobber the user's
                // manual ordering.
                tx.execute(
                    "UPDATE repos SET sort_order = (\
                         SELECT COUNT(*) FROM repos r2 \
                         WHERE LOWER(r2.name) < LOWER(repos.name) \
                            OR (LOWER(r2.name) = LOWER(repos.name) AND r2.id < repos.id)\
                     )",
                    [],
                )?;
                tx.commit()?;
            }
            self.conn().execute("PRAGMA user_version = 14", [])?;
        }
        if v < 15 {
            self.conn().execute_batch(SCHEMA_V15_WORKSPACE_STATUS)?;
            self.conn().execute("PRAGMA user_version = 15", [])?;
        }
        Ok(())
    }

    /// Add `column` to `table` only if it is not already present.
    ///
    /// `migrate()` re-runs on every startup (SCHEMA_V1 resets `user_version`),
    /// so each `ALTER TABLE ... ADD COLUMN` must be idempotent — this guards it
    /// behind a `pragma_table_info` existence check and cleanly survives
    /// partial-migration retries. `column_def` is the full definition that
    /// follows `ADD COLUMN` (e.g. `"yolo INTEGER NOT NULL DEFAULT 0"`).
    ///
    /// `table`/`column_def` are interpolated into the SQL because identifiers
    /// cannot be bound; every caller passes a hardcoded literal, never user
    /// input.
    fn add_column_if_missing(&self, table: &str, column: &str, column_def: &str) -> Result<()> {
        let exists: i64 = self.conn().query_row(
            &format!("SELECT count(*) FROM pragma_table_info('{table}') WHERE name = ?1"),
            rusqlite::params![column],
            |r| r.get(0),
        )?;
        if exists == 0 {
            self.conn()
                .execute(&format!("ALTER TABLE {table} ADD COLUMN {column_def}"), [])?;
        }
        Ok(())
    }
}

const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS repos (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    name          TEXT NOT NULL,
    path          TEXT NOT NULL UNIQUE,
    branch_prefix TEXT NOT NULL DEFAULT '',
    created_at    INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS workspaces (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_id        INTEGER NOT NULL REFERENCES repos(id),
    name           TEXT NOT NULL,
    branch         TEXT NOT NULL,
    worktree_path  TEXT NOT NULL UNIQUE,
    state          TEXT NOT NULL,
    setup_status   TEXT NOT NULL,
    created_at     INTEGER NOT NULL,
    yolo           INTEGER NOT NULL DEFAULT 0,
    UNIQUE(repo_id, name)
);

PRAGMA user_version = 1;
"#;

const SCHEMA_V2_SETTINGS: &str = "
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";

const SCHEMA_V8_ACTIVITY_BUCKETS: &str = "
CREATE TABLE IF NOT EXISTS activity_buckets (
    hour_epoch INTEGER PRIMARY KEY,
    max_live   INTEGER NOT NULL
);
";

const SCHEMA_V10_WORKSPACE_LAYOUTS: &str = "
CREATE TABLE IF NOT EXISTS workspace_layouts (
    anchor_workspace_id INTEGER PRIMARY KEY
        REFERENCES workspaces(id) ON DELETE CASCADE,
    tree_json TEXT NOT NULL,
    focus_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
";

const SCHEMA_V15_WORKSPACE_STATUS: &str = "
CREATE TABLE IF NOT EXISTS workspace_status (
    workspace_id INTEGER PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
    state        TEXT NOT NULL,
    message      TEXT,
    source       TEXT NOT NULL,
    reported_at  INTEGER NOT NULL
);
";

const SCHEMA_V12_MULTI_AGENT: &str = r#"
CREATE TABLE IF NOT EXISTS workspace_agents (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id  INTEGER NOT NULL REFERENCES workspaces(id),
    agent         TEXT    NOT NULL,
    -- ordinal is per (workspace, agent): claude ordinal 1 and codex ordinal 1 coexist.
    ordinal       INTEGER NOT NULL,
    is_primary    INTEGER NOT NULL DEFAULT 0,
    session_ref   TEXT,
    created_at    INTEGER NOT NULL,
    UNIQUE(workspace_id, agent, ordinal)
);
CREATE INDEX IF NOT EXISTS idx_workspace_agents_ws ON workspace_agents(workspace_id);
-- Exactly one primary per workspace. The runtime assumes this (primary_instance_id
-- uses query_row); enforce it at the DB layer so a stray second primary can't be
-- inserted and cause nondeterministic resolution.
CREATE UNIQUE INDEX IF NOT EXISTS idx_workspace_agents_one_primary
    ON workspace_agents(workspace_id) WHERE is_primary = 1;

CREATE TABLE IF NOT EXISTS agent_messages (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    -- workspace_id is a denormalized filter column with NO foreign key. Message
    -- rows are cleaned up explicitly (delete_workspace / remove_repo /
    -- remove_workspace_agent) rather than by cascade: target_agent_id's FK to
    -- workspace_agents would otherwise block deleting those parent rows.
    workspace_id    INTEGER NOT NULL,
    target_agent_id INTEGER NOT NULL REFERENCES workspace_agents(id),
    from_agent_id   INTEGER,
    body            TEXT NOT NULL,
    created_at      INTEGER NOT NULL,
    delivered_at    INTEGER
);
CREATE INDEX IF NOT EXISTS idx_agent_messages_undelivered
    ON agent_messages(workspace_id) WHERE delivered_at IS NULL;
"#;
