use crate::error::Result;
use crate::pty::session::AgentKind;
use rusqlite::{Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RepoId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct WorkspaceId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct AgentInstanceId(pub i64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceState {
    Pending,
    Ready,
    Failed,
    Orphaned,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupStatus {
    NotRun,
    Skipped,
    Ok,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct Repo {
    pub id: RepoId,
    pub name: String,
    pub path: PathBuf,
    pub branch_prefix: String,
    pub custom_instructions: Option<String>,
    pub setup_script: Option<String>,
    pub archive_script: Option<String>,
    pub pinned_commands: Option<String>,
    pub related_repos: Option<String>,
    pub base_branch: Option<String>,
    pub detail_bar_config: Option<String>,
    pub created_at: i64,
    pub sort_order: i64,
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub repo_id: RepoId,
    pub name: String,
    pub branch: String,
    pub worktree_path: PathBuf,
    pub state: WorkspaceState,
    pub setup_status: SetupStatus,
    pub created_at: i64,
    pub yolo: bool,
    pub agent: AgentKind,
}

#[derive(Debug, Clone)]
pub struct NewWorkspace<'a> {
    pub repo_id: RepoId,
    pub name: &'a str,
    pub branch: &'a str,
    pub worktree_path: &'a Path,
    pub yolo: bool,
    pub agent: AgentKind,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(SCHEMA_V1)?;

        let v: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if v < 2 {
            self.conn.execute_batch(SCHEMA_V2_SETTINGS)?;
            // ALTER TABLE only if the column doesn't already exist (handles
            // partial-migration retries cleanly).
            let has_col: i64 = self.conn.query_row(
                "SELECT count(*) FROM pragma_table_info('repos') WHERE name = 'custom_instructions'",
                [],
                |r| r.get(0),
            )?;
            if has_col == 0 {
                self.conn
                    .execute("ALTER TABLE repos ADD COLUMN custom_instructions TEXT", [])?;
            }
            self.conn.execute("PRAGMA user_version = 2", [])?;
        }
        if v < 3 {
            let has_setup: i64 = self.conn.query_row(
                "SELECT count(*) FROM pragma_table_info('repos') WHERE name = 'setup_script'",
                [],
                |r| r.get(0),
            )?;
            if has_setup == 0 {
                self.conn
                    .execute("ALTER TABLE repos ADD COLUMN setup_script TEXT", [])?;
            }
            let has_archive: i64 = self.conn.query_row(
                "SELECT count(*) FROM pragma_table_info('repos') WHERE name = 'archive_script'",
                [],
                |r| r.get(0),
            )?;
            if has_archive == 0 {
                self.conn
                    .execute("ALTER TABLE repos ADD COLUMN archive_script TEXT", [])?;
            }
            self.conn.execute("PRAGMA user_version = 3", [])?;
        }
        if v < 4 {
            let has_yolo: i64 = self.conn.query_row(
                "SELECT count(*) FROM pragma_table_info('workspaces') WHERE name = 'yolo'",
                [],
                |r| r.get(0),
            )?;
            if has_yolo == 0 {
                self.conn.execute(
                    "ALTER TABLE workspaces ADD COLUMN yolo INTEGER NOT NULL DEFAULT 0",
                    [],
                )?;
            }
            self.conn.execute("PRAGMA user_version = 4", [])?;
        }
        if v < 5 {
            let has_col: i64 = self.conn.query_row(
                "SELECT count(*) FROM pragma_table_info('repos') WHERE name = 'pinned_commands'",
                [],
                |r| r.get(0),
            )?;
            if has_col == 0 {
                self.conn
                    .execute("ALTER TABLE repos ADD COLUMN pinned_commands TEXT", [])?;
            }
            self.conn.execute("PRAGMA user_version = 5", [])?;
        }
        if v < 6 {
            let has_col: i64 = self.conn.query_row(
                "SELECT count(*) FROM pragma_table_info('repos') WHERE name = 'related_repos'",
                [],
                |r| r.get(0),
            )?;
            if has_col == 0 {
                self.conn
                    .execute("ALTER TABLE repos ADD COLUMN related_repos TEXT", [])?;
            }
            self.conn.execute("PRAGMA user_version = 6", [])?;
        }
        if v < 7 {
            let has_col: i64 = self.conn.query_row(
                "SELECT count(*) FROM pragma_table_info('repos') WHERE name = 'base_branch'",
                [],
                |r| r.get(0),
            )?;
            if has_col == 0 {
                self.conn
                    .execute("ALTER TABLE repos ADD COLUMN base_branch TEXT", [])?;
            }
            self.conn.execute("PRAGMA user_version = 7", [])?;
        }
        if v < 8 {
            self.conn.execute_batch(SCHEMA_V8_ACTIVITY_BUCKETS)?;
            self.conn.execute("PRAGMA user_version = 8", [])?;
        }
        if v < 9 {
            let has_col: i64 = self.conn.query_row(
                "SELECT count(*) FROM pragma_table_info('workspaces') WHERE name = 'agent'",
                [],
                |r| r.get(0),
            )?;
            if has_col == 0 {
                self.conn.execute(
                    "ALTER TABLE workspaces ADD COLUMN agent TEXT NOT NULL DEFAULT 'claude'",
                    [],
                )?;
            }
            self.conn.execute("PRAGMA user_version = 9", [])?;
        }
        if v < 10 {
            self.conn.execute_batch(SCHEMA_V10_WORKSPACE_LAYOUTS)?;
            self.conn.execute("PRAGMA user_version = 10", [])?;
        }
        if v < 11 {
            let has_col: i64 = self.conn.query_row(
                "SELECT count(*) FROM pragma_table_info('repos') WHERE name = 'detail_bar_config'",
                [],
                |r| r.get(0),
            )?;
            if has_col == 0 {
                self.conn
                    .execute("ALTER TABLE repos ADD COLUMN detail_bar_config TEXT", [])?;
            }
            self.conn.execute("PRAGMA user_version = 11", [])?;
        }
        if v < 12 {
            self.conn.execute_batch(SCHEMA_V12_MULTI_AGENT)?;
            // Backfill one primary instance row per existing workspace from the
            // denormalized workspaces.agent column.
            self.conn.execute(
                "INSERT OR IGNORE INTO workspace_agents \
                     (workspace_id, agent, ordinal, is_primary, created_at)
                 SELECT id, agent, 1, 1, created_at FROM workspaces",
                [],
            )?;
            self.conn.execute("PRAGMA user_version = 12", [])?;
        }
        if v < 13 {
            let has_chronology: i64 = self.conn.query_row(
                "SELECT count(*) FROM pragma_table_info('repos') WHERE name = 'chronology_config'",
                [],
                |r| r.get(0),
            )?;
            if has_chronology == 0 {
                self.conn
                    .execute("ALTER TABLE repos ADD COLUMN chronology_config TEXT", [])?;
            }
            self.conn.execute("PRAGMA user_version = 13", [])?;
        }
        if v < 14 {
            let has_col: i64 = self.conn.query_row(
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
                let tx = self.conn.unchecked_transaction()?;
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
            self.conn.execute("PRAGMA user_version = 14", [])?;
        }
        Ok(())
    }

    pub fn add_repo(&self, path: &Path, name: &str, branch_prefix: &str) -> Result<RepoId> {
        let now = now_ms();
        self.conn.execute(
            "INSERT INTO repos (name, path, branch_prefix, created_at, sort_order) \
             VALUES (?1, ?2, ?3, ?4, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM repos))",
            rusqlite::params![name, path.to_string_lossy(), branch_prefix, now],
        )?;
        Ok(RepoId(self.conn.last_insert_rowid()))
    }

    pub fn remove_repo(&self, id: RepoId) -> Result<()> {
        // Clear agent-instance rows before deleting workspaces.
        // `workspace_agents.workspace_id` has no ON DELETE CASCADE, so the
        // FK constraint would block the workspace delete without this.
        //
        // Manual cascade: agent_messages.target_agent_id → workspace_agents → workspaces
        self.conn.execute(
            "DELETE FROM agent_messages WHERE workspace_id IN \
                 (SELECT id FROM workspaces WHERE repo_id = ?1)",
            [id.0],
        )?;
        self.conn.execute(
            "DELETE FROM workspace_agents WHERE workspace_id IN \
                 (SELECT id FROM workspaces WHERE repo_id = ?1)",
            [id.0],
        )?;
        self.conn
            .execute("DELETE FROM workspaces WHERE repo_id = ?1", [id.0])?;
        self.conn
            .execute("DELETE FROM repos WHERE id = ?1", [id.0])?;
        Ok(())
    }

    pub fn repos(&self) -> Result<Vec<Repo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, path, branch_prefix, custom_instructions, \
                    setup_script, archive_script, pinned_commands, \
                    related_repos, base_branch, detail_bar_config, \
                    created_at, sort_order \
             FROM repos ORDER BY sort_order, id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Repo {
                id: RepoId(r.get(0)?),
                name: r.get(1)?,
                path: PathBuf::from(r.get::<_, String>(2)?),
                branch_prefix: r.get(3)?,
                custom_instructions: r.get(4)?,
                setup_script: r.get(5)?,
                archive_script: r.get(6)?,
                pinned_commands: r.get(7)?,
                related_repos: r.get(8)?,
                base_branch: r.get(9)?,
                detail_bar_config: r.get(10)?,
                created_at: r.get(11)?,
                sort_order: r.get(12)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Swap the `sort_order` of two repos. Used by the dashboard to move a
    /// repo up/down by one slot. Atomic so a crash can't leave a half-swap.
    pub fn swap_repo_sort_order(&self, a: RepoId, b: RepoId) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let so_a: i64 = tx.query_row("SELECT sort_order FROM repos WHERE id = ?1", [a.0], |r| {
            r.get(0)
        })?;
        let so_b: i64 = tx.query_row("SELECT sort_order FROM repos WHERE id = ?1", [b.0], |r| {
            r.get(0)
        })?;
        tx.execute(
            "UPDATE repos SET sort_order = ?1 WHERE id = ?2",
            rusqlite::params![so_b, a.0],
        )?;
        tx.execute(
            "UPDATE repos SET sort_order = ?1 WHERE id = ?2",
            rusqlite::params![so_a, b.0],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn set_repo_branch_prefix(&self, id: RepoId, prefix: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE repos SET branch_prefix = ?1 WHERE id = ?2",
            rusqlite::params![prefix, id.0],
        )?;
        Ok(())
    }

    pub fn set_repo_custom_instructions(
        &self,
        id: RepoId,
        instructions: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE repos SET custom_instructions = ?1 WHERE id = ?2",
            rusqlite::params![instructions, id.0],
        )?;
        Ok(())
    }

    pub fn set_repo_setup_script(&self, id: RepoId, value: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE repos SET setup_script = ?1 WHERE id = ?2",
            rusqlite::params![value, id.0],
        )?;
        Ok(())
    }

    pub fn set_repo_archive_script(&self, id: RepoId, value: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE repos SET archive_script = ?1 WHERE id = ?2",
            rusqlite::params![value, id.0],
        )?;
        Ok(())
    }

    pub fn set_repo_pinned_commands(&self, id: RepoId, value: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE repos SET pinned_commands = ?1 WHERE id = ?2",
            rusqlite::params![value, id.0],
        )?;
        Ok(())
    }

    pub fn set_repo_related_repos(&self, id: RepoId, value: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE repos SET related_repos = ?1 WHERE id = ?2",
            rusqlite::params![value, id.0],
        )?;
        Ok(())
    }

    pub fn set_repo_name(&self, id: RepoId, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(crate::error::Error::UserInput(
                "repo name cannot be empty".into(),
            ));
        }
        // Check for duplicate name on a different repo.
        let dup: std::result::Result<Option<i64>, _> = self
            .conn
            .query_row(
                "SELECT id FROM repos WHERE name = ?1 AND id != ?2",
                rusqlite::params![name, id.0],
                |r| r.get(0),
            )
            .optional();
        if let Ok(Some(_existing_id)) = dup {
            return Err(crate::error::Error::UserInput(format!(
                "a repo named '{name}' already exists"
            )));
        }
        // Read the old name for the related_repos cascade.
        let old_name: String =
            self.conn
                .query_row("SELECT name FROM repos WHERE id = ?1", [id.0], |r| {
                    r.get::<_, String>(0)
                })?;

        self.conn.execute(
            "UPDATE repos SET name = ?1 WHERE id = ?2",
            rusqlite::params![name, id.0],
        )?;

        // Cascade: rewrite related_repos entries in other repos that
        // mention the old name. We do this in Rust to avoid substring
        // false positives (e.g. "front" matching inside "frontend").
        let mut stmt = self.conn.prepare(
            "SELECT id, related_repos FROM repos \
             WHERE related_repos IS NOT NULL AND id != ?1",
        )?;
        let rows: Vec<(i64, String)> =
            match stmt.query_map([id.0], |r| Ok((r.get(0)?, r.get::<_, String>(1)?))) {
                Ok(mapped) => mapped.filter_map(|r| r.ok()).collect(),
                Err(_) => Vec::new(),
            };
        drop(stmt);
        for (other_id, spec) in rows {
            let names = crate::agent::related::parse(&spec);
            if !names.iter().any(|n| n == &old_name) {
                continue;
            }
            let mut new_parts: Vec<&str> = names
                .iter()
                .map(|n| if n == &old_name { name } else { n.as_str() })
                .collect();
            new_parts.dedup();
            let new_spec = new_parts.join(", ");
            self.conn.execute(
                "UPDATE repos SET related_repos = ?1 WHERE id = ?2",
                rusqlite::params![new_spec, other_id],
            )?;
        }

        Ok(())
    }

    pub fn set_repo_base_branch(&self, id: RepoId, value: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE repos SET base_branch = ?1 WHERE id = ?2",
            rusqlite::params![value, id.0],
        )?;
        Ok(())
    }

    pub fn set_repo_detail_bar_config(&self, id: RepoId, value: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE repos SET detail_bar_config = ?1 WHERE id = ?2",
            rusqlite::params![value, id.0],
        )?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        self.conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get::<_, String>(0)
            })
            .optional()
            .map_err(Into::into)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![key, value],
        )?;
        Ok(())
    }

    pub fn delete_setting(&self, key: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM settings WHERE key = ?1", [key])?;
        Ok(())
    }

    pub fn list_settings(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM settings ORDER BY key")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn set_activity_bucket(&self, hour_epoch: u64, max_live: u32) -> Result<()> {
        self.conn.execute(
            "INSERT INTO activity_buckets (hour_epoch, max_live) VALUES (?1, ?2)
             ON CONFLICT(hour_epoch) DO UPDATE SET max_live = excluded.max_live",
            rusqlite::params![hour_epoch as i64, max_live as i64],
        )?;
        Ok(())
    }

    /// Return up to `limit` most-recent buckets in ascending hour order.
    pub fn recent_activity_buckets(&self, limit: usize) -> Result<Vec<(u64, u32)>> {
        let mut stmt = self.conn.prepare(
            "SELECT hour_epoch, max_live FROM activity_buckets
             ORDER BY hour_epoch DESC LIMIT ?1",
        )?;
        let mut rows: Vec<(u64, u32)> = stmt
            .query_map(rusqlite::params![limit as i64], |r| {
                let h: i64 = r.get(0)?;
                let m: i64 = r.get(1)?;
                Ok((h as u64, m as u32))
            })?
            .collect::<rusqlite::Result<_>>()?;
        rows.reverse();
        Ok(rows)
    }

    /// Delete buckets with hour_epoch strictly less than `cutoff`.
    pub fn prune_activity_buckets_before(&self, cutoff: u64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM activity_buckets WHERE hour_epoch < ?1",
            rusqlite::params![cutoff as i64],
        )?;
        Ok(())
    }

    pub fn insert_workspace(&self, w: &NewWorkspace) -> Result<WorkspaceId> {
        let now = now_ms();
        let agent_str = w.agent.store_value();
        self.conn.execute(
            "INSERT INTO workspaces (repo_id, name, branch, worktree_path, state, setup_status, created_at, yolo, agent)
             VALUES (?1, ?2, ?3, ?4, 'Pending', 'NotRun', ?5, ?6, ?7)",
            rusqlite::params![w.repo_id.0, w.name, w.branch, w.worktree_path.to_string_lossy(), now, w.yolo as i64, agent_str],
        )?;
        Ok(WorkspaceId(self.conn.last_insert_rowid()))
    }

    pub fn delete_workspace(&self, id: WorkspaceId) -> Result<()> {
        // `workspace_agents.workspace_id` references `workspaces(id)` WITHOUT
        // `ON DELETE CASCADE`, so clear any agent-instance rows first or the
        // foreign-key constraint would block the delete. (Now that sessions are
        // keyed by primary instance, attached workspaces always have a row.)
        //
        // Manual cascade: agent_messages.target_agent_id → workspace_agents → workspaces
        self.conn
            .execute("DELETE FROM agent_messages WHERE workspace_id = ?1", [id.0])?;
        self.conn.execute(
            "DELETE FROM workspace_agents WHERE workspace_id = ?1",
            [id.0],
        )?;
        self.conn
            .execute("DELETE FROM workspaces WHERE id = ?1", [id.0])?;
        Ok(())
    }

    pub fn rename_workspace(&self, id: WorkspaceId, name: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE workspaces SET name = ?1 WHERE id = ?2",
            rusqlite::params![name, id.0],
        )?;
        Ok(())
    }

    pub fn set_workspace_branch(&self, id: WorkspaceId, branch: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE workspaces SET branch = ?1 WHERE id = ?2",
            rusqlite::params![branch, id.0],
        )?;
        Ok(())
    }

    pub fn set_workspace_agent(
        &self,
        id: WorkspaceId,
        agent: crate::pty::session::AgentKind,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE workspaces SET agent = ?1 WHERE id = ?2",
            rusqlite::params![agent.store_value(), id.0],
        )?;
        // Keep the primary workspace_agents row in sync (the spec's single-writer
        // invariant: workspaces.agent is a denormalized mirror of the primary
        // instance's kind). Compute a collision-free ordinal for the new kind:
        // the next free ordinal among existing rows of that kind (the primary
        // currently holds its OLD kind, so it isn't counted) — which is 1 for the
        // common single-agent replace, or MAX+1 if added instances of the new
        // kind already exist.
        let next_ordinal: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(ordinal), 0) + 1 FROM workspace_agents
             WHERE workspace_id = ?1 AND agent = ?2",
            rusqlite::params![id.0, agent.store_value()],
            |r| r.get(0),
        )?;
        self.conn.execute(
            "UPDATE workspace_agents SET agent = ?1, ordinal = ?2
             WHERE workspace_id = ?3 AND is_primary = 1",
            rusqlite::params![agent.store_value(), next_ordinal, id.0],
        )?;
        Ok(())
    }

    pub fn set_workspace_state(&self, id: WorkspaceId, state: WorkspaceState) -> Result<()> {
        self.conn.execute(
            "UPDATE workspaces SET state = ?1 WHERE id = ?2",
            rusqlite::params![state_label(&state), id.0],
        )?;
        Ok(())
    }

    pub fn set_setup_status(&self, id: WorkspaceId, status: SetupStatus) -> Result<()> {
        self.conn.execute(
            "UPDATE workspaces SET setup_status = ?1 WHERE id = ?2",
            rusqlite::params![setup_label(&status), id.0],
        )?;
        Ok(())
    }

    pub fn workspaces(&self, repo_id: RepoId) -> Result<Vec<Workspace>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, repo_id, name, branch, worktree_path, state, setup_status, created_at, yolo, agent
             FROM workspaces WHERE repo_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([repo_id.0], row_to_workspace)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// SQLite's `data_version` counter — increments whenever ANOTHER
    /// connection commits a write to this database. Our own writes through
    /// this connection do not bump it, so polling this is a cheap way for
    /// the TUI to detect external mutations (e.g. `wsx workspace create`
    /// from a sibling CLI process) without thrashing on self-induced changes.
    pub fn data_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("PRAGMA data_version", [], |r| r.get(0))?)
    }

    pub fn sweep_stale_pending(&self, older_than: std::time::Duration) -> Result<usize> {
        let cutoff = now_ms() - older_than.as_millis() as i64;
        let n = self.conn.execute(
            "UPDATE workspaces SET state = 'Orphaned'
             WHERE state = 'Pending' AND created_at < ?1",
            [cutoff],
        )?;
        Ok(n)
    }

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
        self.conn.execute(
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
            .conn
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
                self.conn.execute(
                    "DELETE FROM workspace_layouts WHERE anchor_workspace_id = ?1",
                    [anchor.0],
                )?;
                Ok(None)
            }
        }
    }

    pub fn delete_workspace_layout(&self, anchor: WorkspaceId) -> Result<()> {
        self.conn.execute(
            "DELETE FROM workspace_layouts WHERE anchor_workspace_id = ?1",
            [anchor.0],
        )?;
        Ok(())
    }

    pub fn list_multi_pane_layout_anchors(&self) -> Result<Vec<WorkspaceId>> {
        let mut stmt = self
            .conn
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
            self.conn.execute(
                "DELETE FROM workspace_layouts WHERE anchor_workspace_id = ?1",
                [anchor],
            )?;
        }
        Ok(out)
    }

    /// Fetch a single workspace by its id.
    pub fn workspace_by_id(&self, id: WorkspaceId) -> Result<Option<Workspace>> {
        let r = self
            .conn
            .query_row(
                "SELECT id, repo_id, name, branch, worktree_path, state, setup_status, created_at, yolo, agent
                 FROM workspaces WHERE id = ?1",
                [id.0],
                row_to_workspace,
            )
            .optional()?;
        Ok(r)
    }

    /// All workspaces across every repo (used by `resolve_current_workspace`).
    pub fn all_workspaces(&self) -> Result<Vec<Workspace>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, repo_id, name, branch, worktree_path, state, setup_status, created_at, yolo, agent
             FROM workspaces ORDER BY id",
        )?;
        let rows = stmt.query_map([], row_to_workspace)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub(crate) fn conn(&self) -> &rusqlite::Connection {
        &self.conn
    }

    #[cfg(test)]
    pub(crate) fn migrate_for_test(&self) -> Result<()> {
        self.migrate()
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

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn row_to_workspace(r: &rusqlite::Row) -> rusqlite::Result<Workspace> {
    Ok(Workspace {
        id: WorkspaceId(r.get(0)?),
        repo_id: RepoId(r.get(1)?),
        name: r.get(2)?,
        branch: r.get(3)?,
        worktree_path: PathBuf::from(r.get::<_, String>(4)?),
        state: parse_state(&r.get::<_, String>(5)?),
        setup_status: parse_setup(&r.get::<_, String>(6)?),
        created_at: r.get(7)?,
        yolo: r.get::<_, i64>(8)? != 0,
        agent: AgentKind::from_str_or_default(Some(&r.get::<_, String>(9)?)),
    })
}

fn state_label(s: &WorkspaceState) -> &'static str {
    match s {
        WorkspaceState::Pending => "Pending",
        WorkspaceState::Ready => "Ready",
        WorkspaceState::Failed => "Failed",
        WorkspaceState::Orphaned => "Orphaned",
    }
}
fn parse_state(s: &str) -> WorkspaceState {
    match s {
        "Pending" => WorkspaceState::Pending,
        "Ready" => WorkspaceState::Ready,
        "Failed" => WorkspaceState::Failed,
        _ => WorkspaceState::Orphaned,
    }
}
fn setup_label(s: &SetupStatus) -> &'static str {
    match s {
        SetupStatus::NotRun => "NotRun",
        SetupStatus::Skipped => "Skipped",
        SetupStatus::Ok => "Ok",
        SetupStatus::Failed => "Failed",
        SetupStatus::Cancelled => "Cancelled",
    }
}
fn parse_setup(s: &str) -> SetupStatus {
    match s {
        "Ok" => SetupStatus::Ok,
        "Failed" => SetupStatus::Failed,
        "Skipped" => SetupStatus::Skipped,
        "Cancelled" => SetupStatus::Cancelled,
        _ => SetupStatus::NotRun,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Layout-test leaf: a workspace paired with a same-numbered instance id.
    /// Layout persistence doesn't validate instance ids, so any id round-trips.
    fn lt(id: WorkspaceId) -> crate::ui::split::AttachTarget {
        crate::ui::split::AttachTarget {
            workspace_id: id,
            instance: AgentInstanceId(id.0),
        }
    }

    #[test]
    fn open_in_memory_runs_migrations_idempotently() {
        let store = Store::open_in_memory().unwrap();
        // Calling migrate again should not fail.
        store.migrate().unwrap();
        // Tables exist by querying their count.
        let count: i64 = store
            .conn
            .query_row("SELECT count(*) FROM repos", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn repo_crud_round_trip() {
        let store = Store::open_in_memory().unwrap();
        let id = store.add_repo(Path::new("/some/repo"), "demo", "").unwrap();
        let repos = store.repos().unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].id, id);
        assert_eq!(repos[0].name, "demo");
        assert_eq!(repos[0].path, PathBuf::from("/some/repo"));
        assert_eq!(repos[0].branch_prefix, "");

        store.remove_repo(id).unwrap();
        assert!(store.repos().unwrap().is_empty());
    }

    #[test]
    fn add_repo_rejects_duplicate_path() {
        let store = Store::open_in_memory().unwrap();
        store.add_repo(Path::new("/a"), "first", "").unwrap();
        let err = store.add_repo(Path::new("/a"), "second", "");
        assert!(err.is_err());
    }

    #[test]
    fn workspace_lifecycle() {
        let store = Store::open_in_memory().unwrap();
        let repo = store.add_repo(Path::new("/r"), "r", "").unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "fix-bug",
                branch: "wsx/fix-bug",
                worktree_path: Path::new("/wts/fix-bug"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();

        let ws = store.workspaces(repo).unwrap();
        assert_eq!(ws.len(), 1);
        assert_eq!(ws[0].state, WorkspaceState::Pending);
        assert_eq!(ws[0].setup_status, SetupStatus::NotRun);

        store
            .set_workspace_state(id, WorkspaceState::Ready)
            .unwrap();
        store.set_setup_status(id, SetupStatus::Ok).unwrap();

        let ws = store.workspaces(repo).unwrap();
        assert_eq!(ws[0].state, WorkspaceState::Ready);
        assert_eq!(ws[0].setup_status, SetupStatus::Ok);

        store.delete_workspace(id).unwrap();
        assert!(store.workspaces(repo).unwrap().is_empty());
    }

    #[test]
    fn settings_crud_round_trip() {
        let store = Store::open_in_memory().unwrap();
        assert!(store.get_setting("foo").unwrap().is_none());
        store.set_setting("foo", "bar").unwrap();
        assert_eq!(store.get_setting("foo").unwrap().as_deref(), Some("bar"));
        store.set_setting("foo", "baz").unwrap(); // upsert
        assert_eq!(store.get_setting("foo").unwrap().as_deref(), Some("baz"));
        let all = store.list_settings().unwrap();
        assert_eq!(all, vec![("foo".to_string(), "baz".to_string())]);
        store.delete_setting("foo").unwrap();
        assert!(store.get_setting("foo").unwrap().is_none());
    }

    #[test]
    fn repo_custom_instructions_round_trip() {
        let store = Store::open_in_memory().unwrap();
        let id = store.add_repo(Path::new("/r"), "demo", "").unwrap();
        let repos = store.repos().unwrap();
        assert_eq!(repos[0].custom_instructions, None);
        store
            .set_repo_custom_instructions(id, Some("Use ruff"))
            .unwrap();
        let repos = store.repos().unwrap();
        assert_eq!(repos[0].custom_instructions.as_deref(), Some("Use ruff"));
        store.set_repo_custom_instructions(id, None).unwrap();
        let repos = store.repos().unwrap();
        assert_eq!(repos[0].custom_instructions, None);
    }

    #[test]
    fn repo_base_branch_round_trip() {
        let store = Store::open_in_memory().unwrap();
        let id = store.add_repo(Path::new("/r"), "demo", "").unwrap();
        let repos = store.repos().unwrap();
        assert_eq!(repos[0].base_branch, None);

        store.set_repo_base_branch(id, Some("origin/main")).unwrap();
        let repos = store.repos().unwrap();
        assert_eq!(repos[0].base_branch.as_deref(), Some("origin/main"));

        store.set_repo_base_branch(id, None).unwrap();
        let repos = store.repos().unwrap();
        assert_eq!(repos[0].base_branch, None);
    }

    #[test]
    fn detail_bar_config_column_round_trips() {
        let store = Store::open_in_memory().unwrap();
        let id = store.add_repo(Path::new("/some/repo"), "demo", "").unwrap();

        // Default: column is NULL.
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        assert!(repo.detail_bar_config.is_none());

        // Set a value, read it back.
        store
            .set_repo_detail_bar_config(id, Some(r#"{"visible":false}"#))
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        assert_eq!(
            repo.detail_bar_config.as_deref(),
            Some(r#"{"visible":false}"#)
        );

        // Clear it back to NULL.
        store.set_repo_detail_bar_config(id, None).unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        assert!(repo.detail_bar_config.is_none());
    }

    #[test]
    fn activity_bucket_round_trip_and_prune() {
        let store = Store::open_in_memory().unwrap();
        store.set_activity_bucket(100, 3).unwrap();
        store.set_activity_bucket(200, 5).unwrap();
        store.set_activity_bucket(300, 1).unwrap();

        // recent_activity_buckets returns in ascending hour order.
        let all = store.recent_activity_buckets(50).unwrap();
        assert_eq!(all, vec![(100, 3), (200, 5), (300, 1)]);

        // Update an existing bucket — upsert semantics.
        store.set_activity_bucket(200, 9).unwrap();
        let updated = store.recent_activity_buckets(50).unwrap();
        assert_eq!(updated, vec![(100, 3), (200, 9), (300, 1)]);

        // Prune drops anything older than the cutoff (exclusive).
        store.prune_activity_buckets_before(200).unwrap();
        let after_prune = store.recent_activity_buckets(50).unwrap();
        assert_eq!(after_prune, vec![(200, 9), (300, 1)]);
    }

    #[test]
    fn set_repo_branch_prefix_updates_value() {
        let store = Store::open_in_memory().unwrap();
        let id = store.add_repo(Path::new("/r"), "demo", "old").unwrap();
        store.set_repo_branch_prefix(id, "new").unwrap();
        assert_eq!(store.repos().unwrap()[0].branch_prefix, "new");
        store.set_repo_branch_prefix(id, "").unwrap();
        assert_eq!(store.repos().unwrap()[0].branch_prefix, "");
    }

    #[test]
    fn repo_setup_and_archive_scripts_default_null() {
        let store = Store::open_in_memory().unwrap();
        let _id = store.add_repo(Path::new("/r"), "demo", "").unwrap();
        let repos = store.repos().unwrap();
        assert_eq!(repos[0].setup_script, None);
        assert_eq!(repos[0].archive_script, None);
    }

    #[test]
    fn repo_setup_script_round_trip() {
        let store = Store::open_in_memory().unwrap();
        let id = store.add_repo(Path::new("/r"), "demo", "").unwrap();
        assert_eq!(store.repos().unwrap()[0].setup_script, None);

        store
            .set_repo_setup_script(id, Some("bun install"))
            .unwrap();
        assert_eq!(
            store.repos().unwrap()[0].setup_script.as_deref(),
            Some("bun install")
        );

        store.set_repo_setup_script(id, None).unwrap();
        assert_eq!(store.repos().unwrap()[0].setup_script, None);
    }

    #[test]
    fn repo_archive_script_round_trip() {
        let store = Store::open_in_memory().unwrap();
        let id = store.add_repo(Path::new("/r"), "demo", "").unwrap();
        assert_eq!(store.repos().unwrap()[0].archive_script, None);

        store
            .set_repo_archive_script(id, Some("rm -rf node_modules"))
            .unwrap();
        assert_eq!(
            store.repos().unwrap()[0].archive_script.as_deref(),
            Some("rm -rf node_modules")
        );

        store.set_repo_archive_script(id, None).unwrap();
        assert_eq!(store.repos().unwrap()[0].archive_script, None);
    }

    #[test]
    fn set_repo_pinned_commands_round_trips() {
        let store = Store::open_in_memory().unwrap();
        let id = store.add_repo(Path::new("/x"), "demo", "").unwrap();
        store
            .set_repo_pinned_commands(id, Some("PR=/pull-request"))
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        assert_eq!(repo.pinned_commands.as_deref(), Some("PR=/pull-request"));

        store.set_repo_pinned_commands(id, None).unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        assert!(repo.pinned_commands.is_none());
    }

    #[test]
    fn set_repo_related_repos_round_trips() {
        let store = Store::open_in_memory().unwrap();
        let id = store.add_repo(Path::new("/x"), "demo", "").unwrap();
        store
            .set_repo_related_repos(id, Some("frontend, marketing"))
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        assert_eq!(repo.related_repos.as_deref(), Some("frontend, marketing"));

        store.set_repo_related_repos(id, None).unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        assert!(repo.related_repos.is_none());
    }

    #[test]
    fn set_repo_name_round_trip() {
        let store = Store::open_in_memory().unwrap();
        let id = store.add_repo(Path::new("/r"), "old-name", "").unwrap();
        store.set_repo_name(id, "new-name").unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        assert_eq!(repo.name, "new-name");
    }

    #[test]
    fn set_repo_name_rejects_empty() {
        let store = Store::open_in_memory().unwrap();
        let id = store.add_repo(Path::new("/r"), "demo", "").unwrap();
        let err = store.set_repo_name(id, "");
        assert!(err.is_err());
        let err = store.set_repo_name(id, "  ");
        assert!(err.is_err());
        // Name unchanged after failed attempts.
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        assert_eq!(repo.name, "demo");
    }

    #[test]
    fn set_repo_name_rejects_duplicate() {
        let store = Store::open_in_memory().unwrap();
        store.add_repo(Path::new("/a"), "existing", "").unwrap();
        let id = store.add_repo(Path::new("/b"), "demo", "").unwrap();
        let err = store.set_repo_name(id, "existing");
        assert!(err.is_err());
        // Renaming to the same name is fine.
        store.set_repo_name(id, "demo").unwrap();
    }

    #[test]
    fn set_repo_name_cascades_to_related_repos() {
        let store = Store::open_in_memory().unwrap();
        let backend = store
            .add_repo(Path::new("/backend"), "backend", "")
            .unwrap();
        let frontend = store
            .add_repo(Path::new("/frontend"), "frontend", "")
            .unwrap();
        // frontend lists backend as a related repo.
        store
            .set_repo_related_repos(frontend, Some("backend, marketing"))
            .unwrap();
        // Rename backend -> api-backend
        store.set_repo_name(backend, "api-backend").unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == frontend)
            .unwrap();
        assert_eq!(
            repo.related_repos.as_deref(),
            Some("api-backend, marketing"),
            "frontend's related_repos should have 'backend' replaced with 'api-backend'"
        );
    }

    #[test]
    fn set_repo_name_does_not_cascade_to_unrelated_repos() {
        let store = Store::open_in_memory().unwrap();
        let backend = store
            .add_repo(Path::new("/backend"), "backend", "")
            .unwrap();
        let frontend = store
            .add_repo(Path::new("/frontend"), "frontend", "")
            .unwrap();
        store
            .set_repo_related_repos(frontend, Some("marketing"))
            .unwrap();
        // Rename backend; frontend doesn't reference it, so should be unchanged.
        store.set_repo_name(backend, "api-backend").unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == frontend)
            .unwrap();
        assert_eq!(repo.related_repos.as_deref(), Some("marketing"));
    }

    #[test]
    fn set_repo_name_no_substring_false_positive_in_related_repos() {
        let store = Store::open_in_memory().unwrap();
        let front = store.add_repo(Path::new("/front"), "front", "").unwrap();
        let frontend = store
            .add_repo(Path::new("/frontend"), "frontend", "")
            .unwrap();
        // frontend lists both "front" and "frontend" (referring to itself?).
        store
            .set_repo_related_repos(frontend, Some("front, marketing"))
            .unwrap();
        // Rename "front" -> "old-front" — should NOT touch "front" inside "frontend"
        store.set_repo_name(front, "old-front").unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == frontend)
            .unwrap();
        assert_eq!(
            repo.related_repos.as_deref(),
            Some("old-front, marketing"),
            "should replace exact name only, no substring damage"
        );
    }

    #[test]
    fn workspace_yolo_round_trip() {
        let store = Store::open_in_memory().unwrap();
        let repo = store.add_repo(Path::new("/r"), "r", "").unwrap();
        store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "tame",
                branch: "wsx/tame",
                worktree_path: Path::new("/wts/tame"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "wild",
                branch: "wsx/wild",
                worktree_path: Path::new("/wts/wild"),
                yolo: true,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        let ws = store.workspaces(repo).unwrap();
        let tame = ws.iter().find(|w| w.name == "tame").unwrap();
        let wild = ws.iter().find(|w| w.name == "wild").unwrap();
        assert!(!tame.yolo);
        assert!(wild.yolo);
    }

    #[test]
    fn sweep_stale_pending_marks_orphaned() {
        use std::time::Duration;
        let store = Store::open_in_memory().unwrap();
        let repo = store.add_repo(Path::new("/r"), "r", "").unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "stuck",
                branch: "wsx/stuck",
                worktree_path: Path::new("/wts/stuck"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        // Backdate the row to look stale.
        store
            .conn
            .execute("UPDATE workspaces SET created_at = 0 WHERE id = ?1", [id.0])
            .unwrap();

        let swept = store.sweep_stale_pending(Duration::from_secs(60)).unwrap();
        assert_eq!(swept, 1);
        let ws = &store.workspaces(repo).unwrap()[0];
        assert_eq!(ws.state, WorkspaceState::Orphaned);
    }

    #[test]
    fn data_version_increments_on_external_writes() {
        // Two connections to the same on-disk DB. data_version on conn A must
        // bump only after conn B commits a write — this is the signal the TUI
        // uses to detect that a sibling `wsx` CLI process added a workspace.
        let dir = tempfile::TempDir::new().unwrap();
        let db = dir.path().join("wsx.db");
        let a = Store::open(&db).unwrap();
        let b = Store::open(&db).unwrap();

        let v0 = a.data_version().unwrap();
        // A self-write through conn A must NOT change A's data_version, or
        // the TUI would refresh on its own edits and we'd churn every tick.
        a.add_repo(Path::new("/from/a"), "from-a", "").unwrap();
        assert_eq!(a.data_version().unwrap(), v0, "self-write must not bump");

        // External write through conn B must bump A's data_version.
        b.add_repo(Path::new("/from/b"), "from-b", "").unwrap();
        assert!(
            a.data_version().unwrap() > v0,
            "external write must bump data_version"
        );
    }

    #[test]
    fn migration_v10_creates_workspace_layouts_table() {
        let store = Store::open_in_memory().unwrap();
        let v: i64 = store
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert!(v >= 10, "user_version should be at least 10, got {v}");
        let count: i64 = store
            .conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='workspace_layouts'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "workspace_layouts table should exist");
    }

    #[test]
    fn setup_status_cancelled_roundtrips() {
        let store = Store::open_in_memory().unwrap();
        // Insert a repo + workspace fixture.
        let repo_id = store
            .add_repo(Path::new("/tmp/demo"), "demo", "wsx")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "alpha",
                branch: "wsx/alpha",
                worktree_path: Path::new("/tmp/demo/alpha"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        store.set_setup_status(id, SetupStatus::Cancelled).unwrap();
        let ws = store.workspaces(repo_id).unwrap();
        assert_eq!(ws[0].setup_status, SetupStatus::Cancelled);
    }

    #[test]
    fn set_then_get_workspace_layout_round_trips() {
        use crate::ui::split::{SplitDirection, SplitTree};
        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/r"), "r", "x")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "a",
                branch: "x/a",
                worktree_path: std::path::Path::new("/r/a"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        let mut tree = SplitTree::Leaf(lt(id));
        tree.split(&[], SplitDirection::Vertical, lt(id));
        let focus = vec![1];
        store.set_workspace_layout(id, &tree, &focus).unwrap();
        let got = store
            .get_workspace_layout(id)
            .unwrap()
            .expect("layout present");
        assert_eq!(got.0.leaves().len(), 2);
        assert_eq!(got.1, focus);
    }

    #[test]
    fn get_workspace_layout_returns_none_when_absent() {
        let store = Store::open_in_memory().unwrap();
        assert!(
            store
                .get_workspace_layout(WorkspaceId(999))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn archiving_workspace_cascades_to_layout_row() {
        use crate::ui::split::SplitTree;
        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/r"), "r", "x")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "a",
                branch: "x/a",
                worktree_path: std::path::Path::new("/r/a"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        store
            .set_workspace_layout(id, &SplitTree::Leaf(lt(id)), &[])
            .unwrap();
        store.delete_workspace(id).unwrap();
        assert!(store.get_workspace_layout(id).unwrap().is_none());
    }

    #[test]
    fn set_workspace_layout_replaces_existing() {
        use crate::ui::split::{SplitDirection, SplitTree};
        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/r"), "r", "x")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "a",
                branch: "x/a",
                worktree_path: std::path::Path::new("/r/a"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        let single = SplitTree::Leaf(lt(id));
        let mut pair = SplitTree::Leaf(lt(id));
        pair.split(&[], SplitDirection::Vertical, lt(id));
        store.set_workspace_layout(id, &single, &[]).unwrap();
        store.set_workspace_layout(id, &pair, &[1]).unwrap();
        let got = store.get_workspace_layout(id).unwrap().unwrap();
        assert_eq!(got.0.leaves().len(), 2, "second write wins");
    }

    #[test]
    fn get_workspace_layout_returns_none_on_corrupted_json_and_deletes_row() {
        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/r"), "r", "x")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "a",
                branch: "x/a",
                worktree_path: std::path::Path::new("/r/a"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO workspace_layouts (anchor_workspace_id, tree_json, focus_json, updated_at)
                 VALUES (?1, 'not-json', '[]', 0)",
                [id.0],
            )
            .unwrap();
        assert!(store.get_workspace_layout(id).unwrap().is_none());
        let count: i64 = store
            .conn
            .query_row(
                "SELECT count(*) FROM workspace_layouts WHERE anchor_workspace_id = ?1",
                [id.0],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "corrupt row deleted on read");
    }

    #[test]
    fn list_multi_pane_layout_anchors_returns_only_multi_leaf_layouts() {
        use crate::ui::split::{SplitDirection, SplitTree};
        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/r"), "r", "x")
            .unwrap();
        let a = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "a",
                branch: "x/a",
                worktree_path: std::path::Path::new("/r/a"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        let b = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "b",
                branch: "x/b",
                worktree_path: std::path::Path::new("/r/b"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        // a: single-leaf layout (should NOT appear).
        store
            .set_workspace_layout(a, &SplitTree::Leaf(lt(a)), &[])
            .unwrap();
        // b: two-leaf layout (should appear).
        let mut pair = SplitTree::Leaf(lt(b));
        pair.split(&[], SplitDirection::Vertical, lt(a));
        store.set_workspace_layout(b, &pair, &[1]).unwrap();
        let got = store.list_multi_pane_layout_anchors().unwrap();
        assert_eq!(got, vec![b], "only multi-pane anchors returned");
    }

    #[test]
    fn list_multi_pane_layout_anchors_deletes_corrupt_rows() {
        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/r"), "r", "x")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "a",
                branch: "x/a",
                worktree_path: std::path::Path::new("/r/a"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        // Plant a corrupt row directly.
        store
            .conn
            .execute(
                "INSERT INTO workspace_layouts (anchor_workspace_id, tree_json, focus_json, updated_at)
                 VALUES (?1, 'not-json', '[]', 0)",
                [id.0],
            )
            .unwrap();
        let got = store.list_multi_pane_layout_anchors().unwrap();
        assert!(got.is_empty(), "corrupt row not returned");
        let count: i64 = store
            .conn
            .query_row(
                "SELECT count(*) FROM workspace_layouts WHERE anchor_workspace_id = ?1",
                [id.0],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "corrupt row deleted by the listing call");
    }

    #[test]
    fn set_workspace_agent_updates_row() {
        use crate::pty::session::AgentKind;
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "ws",
                branch: "repo/ws",
                worktree_path: std::path::Path::new("/tmp/wsx-test/ws"),
                yolo: false,
                agent: AgentKind::Claude,
            })
            .unwrap();
        store.set_workspace_agent(id, AgentKind::Hermes).unwrap();
        let ws = store
            .workspaces(repo_id)
            .unwrap()
            .into_iter()
            .find(|w| w.id == id)
            .expect("workspace present");
        assert_eq!(ws.agent, AgentKind::Hermes);
    }

    #[test]
    fn set_workspace_agent_syncs_primary_instance_row() {
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
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        store
            .add_primary_agent(ws, crate::pty::session::AgentKind::Claude, 1)
            .unwrap();

        store
            .set_workspace_agent(ws, crate::pty::session::AgentKind::Codex)
            .unwrap();

        let primary = store
            .workspace_agents(ws)
            .unwrap()
            .into_iter()
            .find(|i| i.is_primary)
            .unwrap();
        assert_eq!(primary.agent, crate::pty::session::AgentKind::Codex);
        assert_eq!(primary.ordinal, 1); // no added codex existed → ordinal 1
        assert_eq!(primary.label(), "codex");
    }

    #[test]
    fn set_workspace_agent_avoids_ordinal_collision_with_added_instance() {
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
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        store
            .add_primary_agent(ws, crate::pty::session::AgentKind::Claude, 1)
            .unwrap();
        // An added codex already occupies (ws, codex, ordinal 1).
        store
            .add_workspace_agent(ws, crate::pty::session::AgentKind::Codex)
            .unwrap();

        // Replacing the primary's kind to codex must NOT collide on UNIQUE(ws,agent,ordinal).
        store
            .set_workspace_agent(ws, crate::pty::session::AgentKind::Codex)
            .unwrap();

        let all = store.workspace_agents(ws).unwrap();
        let primary = all.iter().find(|i| i.is_primary).unwrap();
        assert_eq!(primary.agent, crate::pty::session::AgentKind::Codex);
        assert_eq!(primary.ordinal, 2); // codex#1 was taken by the added one
        // Both codex instances coexist, distinct ordinals.
        assert_eq!(
            all.iter()
                .filter(|i| i.agent == crate::pty::session::AgentKind::Codex)
                .count(),
            2
        );
    }

    #[test]
    fn migration_v12_backfills_one_primary_instance_per_workspace() {
        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "wsx")
            .unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO workspaces (repo_id, name, branch, worktree_path, state, setup_status, created_at, yolo, agent)
                 VALUES (?1, 'w1', 'wsx/w1', '/tmp/r/w1', 'Ready', 'Ok', 7, 0, 'codex')",
                [repo.0],
            )
            .unwrap();
        let ws = WorkspaceId(store.conn().last_insert_rowid());

        // Simulate a pre-V12 database, then re-run the migration to exercise backfill.
        store
            .conn()
            .execute("DELETE FROM workspace_agents", [])
            .unwrap();
        store
            .conn()
            .execute("PRAGMA user_version = 11", [])
            .unwrap();
        store.migrate_for_test().unwrap();

        let count: i64 = store
            .conn()
            .query_row(
                "SELECT count(*) FROM workspace_agents WHERE workspace_id = ?1 AND is_primary = 1",
                [ws.0],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        let agent: String = store
            .conn()
            .query_row(
                "SELECT agent FROM workspace_agents WHERE workspace_id = ?1",
                [ws.0],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(agent, "codex");

        // Simulate a crash between the DDL and the version bump: re-enter the
        // `v < 12` block with the backfilled rows already present. `INSERT OR
        // IGNORE` must keep this idempotent (count stays 1, not 2).
        store
            .conn()
            .execute("PRAGMA user_version = 11", [])
            .unwrap();
        store.migrate_for_test().unwrap();
        let count_after_reentry: i64 = store
            .conn()
            .query_row(
                "SELECT count(*) FROM workspace_agents WHERE workspace_id = ?1",
                [ws.0],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count_after_reentry, 1);

        let v: i64 = store
            .conn()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 14);
    }

    #[test]
    fn repos_load_in_sort_order() {
        let store = Store::open_in_memory().unwrap();
        // Raw insert with explicit, out-of-order sort_order values.
        store
            .conn()
            .execute(
                "INSERT INTO repos (name, path, branch_prefix, created_at, sort_order) \
                 VALUES ('b','/tmp/wsx-b','',0,2),('a','/tmp/wsx-a','',0,0),('c','/tmp/wsx-c','',0,1)",
                [],
            )
            .unwrap();
        let names: Vec<String> = store.repos().unwrap().into_iter().map(|r| r.name).collect();
        assert_eq!(
            names,
            vec!["a", "c", "b"],
            "ordered by sort_order, not name or id"
        );
    }

    #[test]
    fn migration_seeds_on_upgrade_then_preserves_custom_order() {
        let store = Store::open_in_memory().unwrap();
        // Simulate a genuine pre-v14 database where the column does not exist.
        // (`migrate()` resets user_version to 1 via SCHEMA_V1 on every call, so
        // all blocks re-run regardless of version — dropping the column is what
        // forces the v14 ALTER + one-time seed path.)
        store
            .conn()
            .execute("ALTER TABLE repos DROP COLUMN sort_order", [])
            .unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO repos (name, path, branch_prefix, created_at) \
                 VALUES ('charlie','/tmp/wsx-c','',0),\
                        ('alpha','/tmp/wsx-a','',0),\
                        ('bravo','/tmp/wsx-b','',0)",
                [],
            )
            .unwrap();

        // Upgrade migration: adds the column and seeds alphabetical ranks once.
        store.migrate_for_test().unwrap();
        let repos = store.repos().unwrap();
        assert_eq!(
            repos.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["alpha", "bravo", "charlie"]
        );
        assert_eq!(
            repos.iter().map(|r| r.sort_order).collect::<Vec<_>>(),
            vec![0, 1, 2],
            "pre-existing repos seeded with unique 0-based alphabetical ranks"
        );

        // The user reorders, then the app restarts (migrate runs again). The
        // seed must NOT re-run and clobber the manual order.
        let id = |n: &str| repos.iter().find(|r| r.name == n).unwrap().id;
        store
            .swap_repo_sort_order(id("charlie"), id("alpha"))
            .unwrap();
        store.migrate_for_test().unwrap();
        let names: Vec<String> = store.repos().unwrap().into_iter().map(|r| r.name).collect();
        assert_eq!(
            names,
            vec!["charlie", "bravo", "alpha"],
            "manual order must survive a re-run of migrate (restart)"
        );
    }

    #[test]
    fn sort_order_persists_across_reopen() {
        // Reproduces the real quit/restart scenario: a manual reorder must
        // survive closing and reopening the on-disk database. `migrate()` runs
        // on every `Store::open`, so the v14 seed must not re-run and clobber
        // the user's order.
        let dir = std::env::temp_dir().join(format!("wsx-persist-reopen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("wsx.db");

        let charlie;
        let alpha;
        {
            let store = Store::open(&db).unwrap();
            alpha = store
                .add_repo(std::path::Path::new("/tmp/wsx-alpha"), "alpha", "")
                .unwrap(); // sort_order 0
            store
                .add_repo(std::path::Path::new("/tmp/wsx-bravo"), "bravo", "")
                .unwrap(); // sort_order 1
            charlie = store
                .add_repo(std::path::Path::new("/tmp/wsx-charlie"), "charlie", "")
                .unwrap(); // sort_order 2
            // Move charlie to the top (custom, non-alphabetical order).
            store.swap_repo_sort_order(charlie, alpha).unwrap();
            let names: Vec<String> = store.repos().unwrap().into_iter().map(|r| r.name).collect();
            assert_eq!(
                names,
                vec!["charlie", "bravo", "alpha"],
                "custom order applied in-session"
            );
        } // store dropped → connection closed (simulates quit)

        // Reopen the same file (simulates restart).
        let store = Store::open(&db).unwrap();
        let names: Vec<String> = store.repos().unwrap().into_iter().map(|r| r.name).collect();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            names,
            vec!["charlie", "bravo", "alpha"],
            "manual repo order must persist across restart"
        );
    }

    #[test]
    fn add_repo_appends_to_tail_sort_order() {
        let store = Store::open_in_memory().unwrap();
        // "zeta" then "alpha": even though alpha sorts first by name, the
        // tail-append rule must give zeta=0, alpha=1 (registration order).
        store
            .add_repo(std::path::Path::new("/tmp/wsx-zeta"), "zeta", "")
            .unwrap();
        store
            .add_repo(std::path::Path::new("/tmp/wsx-alpha2"), "alpha", "")
            .unwrap();

        let order =
            |name: &str, repos: &[Repo]| repos.iter().find(|r| r.name == name).unwrap().sort_order;
        let repos = store.repos().unwrap();
        assert_eq!(
            order("zeta", &repos),
            0,
            "first registered → tail of empty list → 0"
        );
        assert_eq!(
            order("alpha", &repos),
            1,
            "second registered → appended after → 1"
        );
    }

    #[test]
    fn swap_repo_sort_order_swaps_two_repos() {
        let store = Store::open_in_memory().unwrap();
        let a = store
            .add_repo(std::path::Path::new("/tmp/wsx-a"), "aaa", "")
            .unwrap(); // sort_order 0
        let b = store
            .add_repo(std::path::Path::new("/tmp/wsx-b"), "bbb", "")
            .unwrap(); // sort_order 1

        store.swap_repo_sort_order(a, b).unwrap();

        let repos = store.repos().unwrap();
        // After swap, bbb (now 0) sorts before aaa (now 1).
        let names: Vec<&str> = repos.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["bbb", "aaa"], "swap reorders the load order");

        let so = |name: &str| repos.iter().find(|r| r.name == name).unwrap().sort_order;
        assert_eq!(so("bbb"), 0);
        assert_eq!(so("aaa"), 1);
    }
}
