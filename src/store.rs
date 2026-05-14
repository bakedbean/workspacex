use crate::error::Result;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RepoId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkspaceId(pub i64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceState { Pending, Ready, Failed, Orphaned }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupStatus { NotRun, Skipped, Ok, Failed }

#[derive(Debug, Clone)]
pub struct Repo {
    pub id: RepoId,
    pub name: String,
    pub path: PathBuf,
    pub branch_prefix: String,
    pub created_at: i64,
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
}

#[derive(Debug, Clone)]
pub struct NewWorkspace<'a> {
    pub repo_id: RepoId,
    pub name: &'a str,
    pub branch: &'a str,
    pub worktree_path: &'a Path,
}

pub struct Store { conn: Connection }

impl Store {
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
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
    UNIQUE(repo_id, name)
);

PRAGMA user_version = 1;
"#;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_in_memory_runs_migrations_idempotently() {
        let store = Store::open_in_memory().unwrap();
        // Calling migrate again should not fail.
        store.migrate().unwrap();
        // Tables exist by querying their count.
        let count: i64 = store.conn.query_row("SELECT count(*) FROM repos", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 0);
    }
}
