use crate::error::Result;
use crate::pty::session::AgentKind;
use rusqlite::{Connection, OptionalExtension};
use std::path::{Path, PathBuf};

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

/// The agent-facing status vocabulary. Distinct from the six *display*
/// `Status` states: an agent never reports itself idle or stalled — those
/// stay wsx-inferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportedState {
    Working,
    Waiting,
    Blocked,
    Done,
    /// Hook-inferred: the agent's turn ended but background work (subagents,
    /// shell tasks, workflows) is still in flight, so the session is parked and
    /// will auto-resume — it is *not* done. Never settable via `wsx status set`
    /// (it is absent from `parse`, the agent-facing vocabulary); only the Claude
    /// `Stop` hook emits it, from a non-empty `background_tasks` payload. It
    /// round-trips through storage via `from_stored`.
    Busy,
}

impl ReportedState {
    pub fn as_str(self) -> &'static str {
        match self {
            ReportedState::Working => "working",
            ReportedState::Waiting => "waiting",
            ReportedState::Blocked => "blocked",
            ReportedState::Done => "done",
            ReportedState::Busy => "busy",
        }
    }

    /// Parse the *agent-facing* vocabulary (what `wsx status set` accepts).
    /// Deliberately excludes the internal `busy` state — agents cannot push it.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "working" => Some(ReportedState::Working),
            "waiting" => Some(ReportedState::Waiting),
            "blocked" => Some(ReportedState::Blocked),
            "done" => Some(ReportedState::Done),
            _ => None,
        }
    }

    /// Parse a state read back from storage. Superset of `parse` that also
    /// accepts the internal `busy` token so a hook-emitted `Busy` round-trips.
    pub fn from_stored(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "busy" => Some(ReportedState::Busy),
            other => Self::parse(other),
        }
    }
}

/// A row from the `workspace_status` table: the last status an agent pushed.
#[derive(Debug, Clone)]
pub struct ReportedStatus {
    pub state: ReportedState,
    pub message: Option<String>,
    pub source: String,
    pub reported_at: i64,
}

/// A row from the `workspace_recap` table: the goal / state / next digest a
/// workspace's agent maintains via `wsx recap set`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRecap {
    pub goal: Option<String>,
    pub state: Option<String>,
    pub next: Option<String>,
    pub updated_at: i64,
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
    pub shared: bool,
}

#[derive(Debug, Clone)]
pub struct NewWorkspace<'a> {
    pub repo_id: RepoId,
    pub name: &'a str,
    pub branch: &'a str,
    pub worktree_path: &'a Path,
    pub yolo: bool,
    pub agent: AgentKind,
    pub shared: bool,
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
        // Writers (the TUI and a sibling `wsx status` CLI process) contend for
        // the single WAL writer slot; wait up to 3s rather than erroring out
        // immediately with SQLITE_BUSY.
        conn.pragma_update(None, "busy_timeout", 3000)?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn insert_workspace(&self, w: &NewWorkspace) -> Result<WorkspaceId> {
        let now = now_ms();
        let agent_str = w.agent.store_value();
        self.conn.execute(
            "INSERT INTO workspaces (repo_id, name, branch, worktree_path, state, setup_status, created_at, yolo, agent, shared)
             VALUES (?1, ?2, ?3, ?4, 'Pending', 'NotRun', ?5, ?6, ?7, ?8)",
            rusqlite::params![w.repo_id.0, w.name, w.branch, w.worktree_path.to_string_lossy(), now, w.yolo as i64, agent_str, w.shared as i64],
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

    pub fn set_workspace_shared(&self, id: WorkspaceId, shared: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE workspaces SET shared = ?1 WHERE id = ?2",
            rusqlite::params![shared as i64, id.0],
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
            "SELECT id, repo_id, name, branch, worktree_path, state, setup_status, created_at, yolo, agent, shared
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

    /// Fetch a single workspace by its id.
    pub fn workspace_by_id(&self, id: WorkspaceId) -> Result<Option<Workspace>> {
        let r = self
            .conn
            .query_row(
                "SELECT id, repo_id, name, branch, worktree_path, state, setup_status, created_at, yolo, agent, shared
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
            "SELECT id, repo_id, name, branch, worktree_path, state, setup_status, created_at, yolo, agent, shared
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

pub(crate) fn now_ms() -> i64 {
    crate::time::now_ms()
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
        shared: r.get::<_, i64>(10)? != 0,
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
                shared: false,
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
    fn workspace_shared_flag_roundtrips_and_flips() {
        let store = Store::open_in_memory().unwrap();
        let repo = store.add_repo(Path::new("/tmp/r"), "r", "wsx").unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "w",
                branch: "wsx/w",
                worktree_path: Path::new("/tmp/r/w"),
                yolo: false,
                agent: AgentKind::Claude,
                shared: true,
            })
            .unwrap();
        assert!(store.workspace_by_id(id).unwrap().unwrap().shared);
        store.set_workspace_shared(id, false).unwrap();
        assert!(!store.workspace_by_id(id).unwrap().unwrap().shared);
    }

    #[test]
    fn migrate_v16_is_idempotent() {
        let store = Store::open_in_memory().unwrap();
        store.migrate_for_test().unwrap(); // second run must not error
        let v: i64 = store
            .conn()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert!(v >= 16);
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
                shared: false,
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
                shared: false,
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
                shared: false,
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
                shared: false,
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
                shared: false,
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
                shared: false,
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
                shared: false,
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
                shared: false,
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
                shared: false,
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
                shared: false,
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
                shared: false,
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
                shared: false,
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
                shared: false,
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
                shared: false,
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
        assert_eq!(v, 17);
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
    fn workspace_status_round_trips() {
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
            .unwrap(); // returns WorkspaceId

        assert!(store.workspace_status(ws).unwrap().is_none());

        store
            .set_workspace_status(ws, ReportedState::Blocked, Some("need a decision"), "model")
            .unwrap();
        let got = store.workspace_status(ws).unwrap().unwrap();
        assert_eq!(got.state, ReportedState::Blocked);
        assert_eq!(got.message.as_deref(), Some("need a decision"));
        assert_eq!(got.source, "model");
        assert!(got.reported_at > 0);

        // INSERT OR REPLACE: second write wins, keyed by workspace.
        store
            .set_workspace_status(ws, ReportedState::Done, None, "hook")
            .unwrap();
        let got = store.workspace_status(ws).unwrap().unwrap();
        assert_eq!(got.state, ReportedState::Done);
        assert_eq!(got.message, None);
        assert_eq!(got.source, "hook");

        store.clear_workspace_status(ws).unwrap();
        assert!(store.workspace_status(ws).unwrap().is_none());
    }

    #[test]
    fn all_workspace_status_returns_map() {
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
            .set_workspace_status(ws, ReportedState::Working, None, "model")
            .unwrap();
        let map = store.all_workspace_status().unwrap();
        assert_eq!(map.get(&ws).map(|s| s.state), Some(ReportedState::Working));
    }

    #[test]
    fn reported_state_parse_round_trips() {
        for st in [
            ReportedState::Working,
            ReportedState::Waiting,
            ReportedState::Blocked,
            ReportedState::Done,
        ] {
            assert_eq!(ReportedState::parse(st.as_str()), Some(st));
        }
        assert_eq!(ReportedState::parse("nonsense"), None);
    }

    #[test]
    fn busy_state_round_trips_but_is_not_agent_settable() {
        // `Busy` is hook-internal: it must survive a storage round-trip via
        // `from_stored`, yet stay out of the agent-facing `parse` vocabulary so
        // `wsx status set busy` is rejected.
        assert_eq!(
            ReportedState::from_stored(ReportedState::Busy.as_str()),
            Some(ReportedState::Busy)
        );
        assert_eq!(ReportedState::parse("busy"), None);
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

    #[test]
    fn open_sets_busy_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.db")).unwrap();
        let ms: i64 = store
            .conn()
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            ms, 3000,
            "Store::open must set busy_timeout to exactly 3000ms"
        );
    }
}
