use crate::error::Result;
use rusqlite::{Connection, OptionalExtension};
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
    pub custom_instructions: Option<String>,
    pub setup_script: Option<String>,
    pub archive_script: Option<String>,
    pub pinned_commands: Option<String>,
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
    pub yolo: bool,
}

#[derive(Debug, Clone)]
pub struct NewWorkspace<'a> {
    pub repo_id: RepoId,
    pub name: &'a str,
    pub branch: &'a str,
    pub worktree_path: &'a Path,
    pub yolo: bool,
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
        let mut stmt = self.conn.prepare(
            "SELECT id, name, path, branch_prefix, custom_instructions, \
                    setup_script, archive_script, pinned_commands, created_at \
             FROM repos ORDER BY id",
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
                created_at: r.get(8)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
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

    pub fn insert_workspace(&self, w: &NewWorkspace) -> Result<WorkspaceId> {
        let now = now_ms();
        self.conn.execute(
            "INSERT INTO workspaces (repo_id, name, branch, worktree_path, state, setup_status, created_at, yolo)
             VALUES (?1, ?2, ?3, ?4, 'Pending', 'NotRun', ?5, ?6)",
            rusqlite::params![w.repo_id.0, w.name, w.branch, w.worktree_path.to_string_lossy(), now, w.yolo as i64],
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
            "SELECT id, repo_id, name, branch, worktree_path, state, setup_status, created_at, yolo
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
                yolo: r.get::<_, i64>(8)? != 0,
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
                yolo: false,
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
            })
            .unwrap();
        store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "wild",
                branch: "wsx/wild",
                worktree_path: Path::new("/wts/wild"),
                yolo: true,
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
