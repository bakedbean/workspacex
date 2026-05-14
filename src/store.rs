use crate::error::Result;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RepoId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkspaceId(pub i64);

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
}

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

pub struct Store {
    conn: Connection,
}

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

    pub fn add_repo(&self, path: &Path, name: &str, branch_prefix: &str) -> Result<RepoId> {
        let now = now_ms();
        self.conn.execute(
            "INSERT INTO repos (name, path, branch_prefix, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![name, path.to_string_lossy(), branch_prefix, now],
        )?;
        Ok(RepoId(self.conn.last_insert_rowid()))
    }

    pub fn remove_repo(&self, id: RepoId) -> Result<()> {
        self.conn
            .execute("DELETE FROM workspaces WHERE repo_id = ?1", [id.0])?;
        self.conn
            .execute("DELETE FROM repos WHERE id = ?1", [id.0])?;
        Ok(())
    }

    pub fn repos(&self) -> Result<Vec<Repo>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, path, branch_prefix, created_at FROM repos ORDER BY id")?;
        let rows = stmt.query_map([], |r| {
            Ok(Repo {
                id: RepoId(r.get(0)?),
                name: r.get(1)?,
                path: PathBuf::from(r.get::<_, String>(2)?),
                branch_prefix: r.get(3)?,
                created_at: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn insert_workspace(&self, w: &NewWorkspace) -> Result<WorkspaceId> {
        let now = now_ms();
        self.conn.execute(
            "INSERT INTO workspaces (repo_id, name, branch, worktree_path, state, setup_status, created_at)
             VALUES (?1, ?2, ?3, ?4, 'Pending', 'NotRun', ?5)",
            rusqlite::params![w.repo_id.0, w.name, w.branch, w.worktree_path.to_string_lossy(), now],
        )?;
        Ok(WorkspaceId(self.conn.last_insert_rowid()))
    }

    pub fn delete_workspace(&self, id: WorkspaceId) -> Result<()> {
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
            "SELECT id, repo_id, name, branch, worktree_path, state, setup_status, created_at
             FROM workspaces WHERE repo_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([repo_id.0], |r| {
            Ok(Workspace {
                id: WorkspaceId(r.get(0)?),
                repo_id: RepoId(r.get(1)?),
                name: r.get(2)?,
                branch: r.get(3)?,
                worktree_path: PathBuf::from(r.get::<_, String>(4)?),
                state: parse_state(&r.get::<_, String>(5)?),
                setup_status: parse_setup(&r.get::<_, String>(6)?),
                created_at: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
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
    }
}
fn parse_setup(s: &str) -> SetupStatus {
    match s {
        "Ok" => SetupStatus::Ok,
        "Failed" => SetupStatus::Failed,
        "Skipped" => SetupStatus::Skipped,
        _ => SetupStatus::NotRun,
    }
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
}
