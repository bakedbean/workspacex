use crate::data::progress::{SetupPhase, SharedProgress};
use crate::data::setup::{self, SetupLine, SetupResult};
use crate::data::store::{
    NewWorkspace, Repo, SetupStatus, Store, Workspace, WorkspaceId, WorkspaceState,
};
use crate::error::{Error, Result};
use crate::git;
use crate::names;
use crate::pty::session::AgentKind;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct CreatedWorkspace {
    pub workspace: Workspace,
    pub setup_result: SetupResult,
}

/// Compose a workspace's git branch from its repo's branch prefix and the
/// workspace name: `<prefix>/<name>`, or just `<name>` when no prefix is set.
/// Shared by create, create_with_app, and rename so the shape never drifts.
fn compose_branch(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", prefix.trim_end_matches('/'), name)
    }
}

/// Map a setup run's outcome to the persisted setup status.
fn setup_status_for(result: &SetupResult) -> SetupStatus {
    match result {
        SetupResult::Ok => SetupStatus::Ok,
        SetupResult::Skipped => SetupStatus::Skipped,
        SetupResult::Failed { .. } => SetupStatus::Failed,
    }
}

/// Create a new workspace: insert pending row, create worktree, mark
/// ready, run setup script, record setup status.
// Workspace creation genuinely needs all these inputs; a params struct would
// not improve clarity here.
#[allow(clippy::too_many_arguments)]
pub async fn create<F: FnMut(SetupLine) + Send>(
    store: &Store,
    repo: &Repo,
    name: Option<&str>,
    worktree_base: &Path,
    yolo: bool,
    agent: AgentKind,
    cancel: tokio_util::sync::CancellationToken,
    on_setup_line: F,
) -> Result<CreatedWorkspace> {
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }

    let name = match name {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => names::generate(),
    };
    let prefix = crate::data::repo::resolve_branch_prefix(repo, store)?;
    let branch = compose_branch(&prefix, &name);
    let worktree_path = worktree_base.join(&repo.name).join(&name);

    let base = repo
        .base_branch
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    // Fetch before inserting the workspace row so a fetch failure
    // (network down, bad remote ref) doesn't leave an orphan Pending row.
    git::fetch_for_base(&repo.path, base).await?;
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }

    let id = store.insert_workspace(&NewWorkspace {
        repo_id: repo.id,
        name: &name,
        branch: &branch,
        worktree_path: &worktree_path,
        yolo,
        agent,
    })?;

    // Seed the primary agent instance so the roster is authoritative from birth.
    store.add_primary_agent(id, agent, crate::data::store::now_ms())?;

    if cancel.is_cancelled() {
        store.set_workspace_state(id, WorkspaceState::Failed)?;
        return Err(Error::Cancelled);
    }

    if let Err(e) = git::create_worktree(&repo.path, &branch, base, &worktree_path).await {
        store.set_workspace_state(id, WorkspaceState::Failed)?;
        return Err(e);
    }
    store.set_workspace_state(id, WorkspaceState::Ready)?;

    if cancel.is_cancelled() {
        store.set_setup_status(id, SetupStatus::Cancelled)?;
        return Err(Error::Cancelled);
    }

    let setup_result = setup::run_setup(
        repo.setup_script.as_deref(),
        &repo.path,
        &worktree_path,
        cancel.clone(),
        on_setup_line,
    )
    .await;
    let setup_result = match setup_result {
        Ok(r) => r,
        Err(Error::Cancelled) => {
            store.set_setup_status(id, SetupStatus::Cancelled)?;
            return Err(Error::Cancelled);
        }
        Err(e) => return Err(e),
    };
    let status = setup_status_for(&setup_result);
    store.set_setup_status(id, status)?;

    let ws = store
        .workspaces(repo.id)?
        .into_iter()
        .find(|w| w.id == id)
        .ok_or_else(|| Error::Store(rusqlite::Error::QueryReturnedNoRows))?;
    Ok(CreatedWorkspace {
        workspace: ws,
        setup_result,
    })
}

/// TUI-friendly variant of `create` that interleaves App lock acquisition
/// with the long-running async git/setup phases. Unlike `create`, this
/// function never holds the App lock across `.await` boundaries on git or
/// setup work, so the event loop can continue to tick and redraw.
///
/// Cancellation: same semantics as `create`. Pre-fetch and pre-insert
/// cancellation returns `Err(Cancelled)` cleanly. Cancellation during
/// setup marks the row `SetupStatus::Cancelled` and leaves the worktree
/// on disk.
pub async fn create_with_app(
    app: crate::app::SharedApp,
    repo: Repo,
    name: Option<String>,
    worktree_base: PathBuf,
    yolo: bool,
    agent: AgentKind,
    progress: SharedProgress,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<CreatedWorkspace> {
    // --- Phase 1 (short, locked): compute names/paths, no I/O. ---
    let (final_name, branch, worktree_path) = {
        let g = app.lock().await;
        let resolved_name = match name.as_deref() {
            Some(s) if !s.trim().is_empty() => s.trim().to_string(),
            _ => crate::names::generate(),
        };
        let prefix = crate::data::repo::resolve_branch_prefix(&repo, &g.store)?;
        let branch = compose_branch(&prefix, &resolved_name);
        let worktree_path = worktree_base.join(&repo.name).join(&resolved_name);
        (resolved_name, branch, worktree_path)
    };

    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }

    // --- Phase 2 (unlocked, async): fetch base branch. ---
    if let Ok(mut p) = progress.lock() {
        p.set_phase(SetupPhase::Fetching);
    }
    let base = repo
        .base_branch
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    crate::git::fetch_for_base(&repo.path, base).await?;

    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }

    // --- Phase 3 (short, locked): insert workspace row. ---
    let id = {
        let g = app.lock().await;
        let ws_id = g.store.insert_workspace(&NewWorkspace {
            repo_id: repo.id,
            name: &final_name,
            branch: &branch,
            worktree_path: &worktree_path,
            yolo,
            agent,
        })?;
        // Seed the primary agent instance so the roster is authoritative from birth.
        g.store
            .add_primary_agent(ws_id, agent, crate::data::store::now_ms())?;
        ws_id
    };

    if cancel.is_cancelled() {
        let g = app.lock().await;
        g.store.set_workspace_state(id, WorkspaceState::Failed)?;
        return Err(Error::Cancelled);
    }

    // --- Phase 4 (unlocked, async): create worktree. ---
    if let Ok(mut p) = progress.lock() {
        p.set_phase(SetupPhase::CreatingWorktree);
    }
    let worktree_result =
        crate::git::create_worktree(&repo.path, &branch, base, &worktree_path).await;
    if let Err(e) = worktree_result {
        let g = app.lock().await;
        g.store.set_workspace_state(id, WorkspaceState::Failed)?;
        return Err(e);
    }
    {
        let g = app.lock().await;
        g.store.set_workspace_state(id, WorkspaceState::Ready)?;
    }

    if cancel.is_cancelled() {
        let g = app.lock().await;
        g.store.set_setup_status(id, SetupStatus::Cancelled)?;
        return Err(Error::Cancelled);
    }

    // --- Phase 5 (unlocked, async): run setup script. ---
    if let Ok(mut p) = progress.lock() {
        p.set_phase(SetupPhase::RunningSetup);
    }
    let progress_lines = progress.clone();
    let setup_result = setup::run_setup(
        repo.setup_script.as_deref(),
        &repo.path,
        &worktree_path,
        cancel.clone(),
        move |line| {
            let text = match line {
                SetupLine::Stdout(s) | SetupLine::Stderr(s) => s,
            };
            if let Ok(mut p) = progress_lines.lock() {
                p.push_line(&text);
            }
        },
    )
    .await;
    let setup_result = match setup_result {
        Ok(r) => r,
        Err(Error::Cancelled) => {
            let g = app.lock().await;
            g.store.set_setup_status(id, SetupStatus::Cancelled)?;
            return Err(Error::Cancelled);
        }
        Err(e) => return Err(e),
    };
    let status = setup_status_for(&setup_result);

    // --- Phase 6 (short, locked): finalize. ---
    let ws = {
        let g = app.lock().await;
        g.store.set_setup_status(id, status)?;
        g.store
            .workspaces(repo.id)?
            .into_iter()
            .find(|w| w.id == id)
            .ok_or_else(|| Error::Store(rusqlite::Error::QueryReturnedNoRows))?
    };
    Ok(CreatedWorkspace {
        workspace: ws,
        setup_result,
    })
}

#[derive(Debug, Clone, Default)]
pub struct ArchiveOpts {
    pub keep_worktree: bool,
    pub force_branch_delete: bool,
}

pub async fn archive<F: FnMut(SetupLine) + Send>(
    store: &Store,
    repo: &Repo,
    ws: &Workspace,
    opts: ArchiveOpts,
    on_archive_line: F,
) -> Result<SetupResult> {
    let archive_result = setup::run_archive(
        repo.archive_script.as_deref(),
        &repo.path,
        &ws.worktree_path,
        tokio_util::sync::CancellationToken::new(),
        on_archive_line,
    )
    .await?;
    if !opts.keep_worktree && ws.worktree_path.exists() {
        git::remove_worktree(&repo.path, &ws.worktree_path).await?;
    }
    let _ = git::branch_delete(&repo.path, &ws.branch, opts.force_branch_delete).await;
    store.delete_workspace(ws.id)?;
    if crate::agent::mcp::enabled(store)
        && let Err(e) = crate::agent::mcp::remove_worktree_entry(&ws.worktree_path)
    {
        tracing::warn!(error = %e, "failed to remove worktree entry from ~/.claude.json");
    }
    Ok(archive_result)
}

/// Advance the `step` field of the `ArchiveRunning` modal, if the
/// modal still belongs to this archive flow. Called between phases of
/// `archive_with_app`. The check guards against a stale archive task
/// updating a modal that was replaced (e.g. by `Modal::Error` or by a
/// second archive flow).
async fn advance_archive_step(app: &crate::app::SharedApp, next: crate::ui::modal::ArchiveStep) {
    let mut g = app.lock().await;
    if let Some(crate::ui::modal::Modal::ArchiveRunning { step, .. }) = &mut g.modal {
        *step = next;
    }
}

/// TUI-friendly variant of `archive` that interleaves App lock acquisition
/// with the long-running async git/script phases. Unlike `archive`, this
/// function never holds the App lock across `.await` boundaries on the
/// archive script or `git worktree remove`, so the event loop can continue
/// to tick and redraw.
pub async fn archive_with_app(
    app: crate::app::SharedApp,
    repo: Repo,
    ws: Workspace,
    opts: ArchiveOpts,
) -> Result<SetupResult> {
    // --- Phase 1 (unlocked, async): run the archive script if any. ---
    let archive_result = setup::run_archive(
        repo.archive_script.as_deref(),
        &repo.path,
        &ws.worktree_path,
        tokio_util::sync::CancellationToken::new(),
        |_| {},
    )
    .await?;

    advance_archive_step(&app, crate::ui::modal::ArchiveStep::RemoveWorktree).await;

    // --- Phase 2 (unlocked, async): remove the worktree from disk. ---
    if !opts.keep_worktree && ws.worktree_path.exists() {
        git::remove_worktree(&repo.path, &ws.worktree_path).await?;
    }

    advance_archive_step(&app, crate::ui::modal::ArchiveStep::DeleteBranch).await;

    // --- Phase 3 (unlocked, async): delete the branch. Failures here
    //     are non-fatal and intentionally swallowed, matching `archive`. ---
    let _ = git::branch_delete(&repo.path, &ws.branch, opts.force_branch_delete).await;

    advance_archive_step(&app, crate::ui::modal::ArchiveStep::Cleanup).await;

    // --- Phase 4 (short, locked): delete the store row + clean up MCP. ---
    {
        let g = app.lock().await;
        g.store.delete_workspace(ws.id)?;
        if crate::agent::mcp::enabled(&g.store)
            && let Err(e) = crate::agent::mcp::remove_worktree_entry(&ws.worktree_path)
        {
            tracing::warn!(error = %e, "failed to remove worktree entry from ~/.claude.json");
        }
    }

    Ok(archive_result)
}

/// Untracked worktrees discovered on disk that the store doesn't know about.
pub async fn discover_untracked(repo: &Repo, store: &Store) -> Result<Vec<git::WorktreeInfo>> {
    let live = git::list_worktrees(&repo.path).await?;
    let tracked: std::collections::HashSet<PathBuf> = store
        .workspaces(repo.id)?
        .into_iter()
        .map(|w| w.worktree_path)
        .collect();
    Ok(live
        .into_iter()
        .filter(|w| w.path != repo.path) // exclude main worktree
        .filter(|w| !tracked.contains(&w.path))
        .collect())
}

/// Import an existing worktree into the registry.
pub fn import_existing(
    store: &Store,
    repo: &Repo,
    info: &git::WorktreeInfo,
    name: &str,
) -> Result<WorkspaceId> {
    let branch = info.branch.clone().unwrap_or_else(|| "(detached)".into());
    let agent = AgentKind::Claude;
    let id = store.insert_workspace(&NewWorkspace {
        repo_id: repo.id,
        name,
        branch: &branch,
        worktree_path: &info.path,
        yolo: false,
        agent,
    })?;
    store.add_primary_agent(id, agent, crate::data::store::now_ms())?;
    store.set_workspace_state(id, WorkspaceState::Ready)?;
    store.set_setup_status(id, SetupStatus::Skipped)?;
    Ok(id)
}

/// Slugify a free-text prompt into a kebab-case workspace name.
/// Returns None if the result is too short to be useful.
pub fn slugify_prompt(text: &str) -> Option<String> {
    let cleaned: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let words: Vec<&str> = cleaned.split('-').filter(|s| !s.is_empty()).collect();
    if words.is_empty() {
        return None;
    }

    // Drop stopwords from the front so "fix the bug" -> "fix-bug" not "fix-the".
    const STOP: &[&str] = &[
        "the", "a", "an", "to", "for", "of", "and", "or", "is", "in", "on",
    ];
    let picked: Vec<&&str> = words.iter().filter(|w| !STOP.contains(w)).take(5).collect();
    if picked.is_empty() {
        return None;
    }

    let mut slug = picked.iter().map(|s| **s).collect::<Vec<&str>>().join("-");
    if slug.len() > 32 {
        slug.truncate(32);
        slug = slug.trim_end_matches('-').to_string();
    }
    if slug.len() < 6 { None } else { Some(slug) }
}

/// Rename a workspace's name AND its git branch. Idempotent.
/// Caller is responsible for refreshing App state after.
pub async fn rename(store: &Store, repo: &Repo, ws: &Workspace, new_name: &str) -> Result<()> {
    if new_name == ws.name {
        return Ok(());
    }
    let prefix = crate::data::repo::resolve_branch_prefix(repo, store)?;
    let new_branch = compose_branch(&prefix, new_name);
    // Branch rename first — if it fails (e.g. name collision), DB stays intact.
    git::rename_branch(&repo.path, &ws.branch, &new_branch).await?;
    store.rename_workspace(ws.id, new_name)?;
    store.set_workspace_branch(ws.id, &new_branch)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn init_git_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        let r = |args: &[&str]| {
            assert!(
                std::process::Command::new("git")
                    .current_dir(dir.path())
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        };
        r(&["init", "-q", "-b", "main"]);
        r(&["config", "user.email", "t@e"]);
        r(&["config", "user.name", "t"]);
        r(&["commit", "--allow-empty", "-q", "-m", "init"]);
        dir
    }

    #[tokio::test]
    async fn create_makes_worktree_and_inserts_row() {
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let base = TempDir::new().unwrap();

        let created = create(
            &store,
            &repo,
            Some("alpha"),
            base.path(),
            false,
            crate::pty::session::AgentKind::Claude,
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(created.workspace.name, "alpha");
        assert_eq!(created.workspace.branch, "wsx/alpha");
        assert_eq!(created.workspace.state, WorkspaceState::Ready);
        assert_eq!(created.workspace.setup_status, SetupStatus::Skipped);
        assert!(!created.workspace.yolo);
        assert!(created.workspace.worktree_path.join(".git").exists());
    }

    #[tokio::test]
    async fn create_with_yolo_sets_flag() {
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let base = TempDir::new().unwrap();

        let created = create(
            &store,
            &repo,
            Some("wild"),
            base.path(),
            true,
            crate::pty::session::AgentKind::Claude,
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        assert!(created.workspace.yolo);
    }

    #[tokio::test]
    async fn create_generates_name_when_none_given() {
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let id = crate::data::repo::add(&store, repo_dir.path(), "demo", "")
            .await
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let base = TempDir::new().unwrap();
        let created = create(
            &store,
            &repo,
            None,
            base.path(),
            false,
            crate::pty::session::AgentKind::Claude,
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        assert!(created.workspace.name.contains('-'));
    }

    #[tokio::test]
    async fn create_records_setup_failure_but_keeps_workspace_ready() {
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let id = crate::data::repo::add(&store, repo_dir.path(), "demo", "")
            .await
            .unwrap();
        store.set_repo_setup_script(id, Some("exit 1")).unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let base = TempDir::new().unwrap();
        let created = create(
            &store,
            &repo,
            Some("a"),
            base.path(),
            false,
            crate::pty::session::AgentKind::Claude,
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(created.workspace.state, WorkspaceState::Ready);
        assert_eq!(created.workspace.setup_status, SetupStatus::Failed);
    }

    #[tokio::test]
    async fn archive_removes_row_and_worktree() {
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let id = crate::data::repo::add(&store, repo_dir.path(), "demo", "")
            .await
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let base = TempDir::new().unwrap();
        let created = create(
            &store,
            &repo,
            Some("doomed"),
            base.path(),
            false,
            crate::pty::session::AgentKind::Claude,
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        archive(
            &store,
            &repo,
            &created.workspace,
            ArchiveOpts {
                force_branch_delete: true,
                ..Default::default()
            },
            |_| {},
        )
        .await
        .unwrap();
        assert!(store.workspaces(repo.id).unwrap().is_empty());
        assert!(!created.workspace.worktree_path.exists());
    }

    #[test]
    fn slugify_basic() {
        assert_eq!(
            slugify_prompt("fix the login form validation bug"),
            Some("fix-login-form-validation-bug".into())
        );
        assert_eq!(slugify_prompt("hi"), None);
        assert_eq!(slugify_prompt("..."), None);
        assert_eq!(
            slugify_prompt("Fix Issue #123!!"),
            Some("fix-issue-123".into())
        );
    }

    #[tokio::test]
    async fn rename_updates_name_and_branch() {
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let base = TempDir::new().unwrap();
        let created = create(
            &store,
            &repo,
            Some("alpha"),
            base.path(),
            false,
            crate::pty::session::AgentKind::Claude,
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();

        rename(&store, &repo, &created.workspace, "fix-bug")
            .await
            .unwrap();

        let refreshed = store.workspaces(repo.id).unwrap();
        let ws = refreshed
            .iter()
            .find(|w| w.id == created.workspace.id)
            .unwrap();
        assert_eq!(ws.name, "fix-bug");
        assert_eq!(ws.branch, "wsx/fix-bug");

        // Confirm the git branch was actually renamed.
        let branches = std::process::Command::new("git")
            .current_dir(&repo.path)
            .args(["branch", "--list", "--format=%(refname:short)"])
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&branches.stdout);
        assert!(
            out.lines().any(|b| b == "wsx/fix-bug"),
            "expected wsx/fix-bug branch, got: {out}"
        );
        assert!(!out.lines().any(|b| b == "wsx/alpha"));
    }

    #[tokio::test]
    async fn discover_finds_untracked_worktree() {
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let id = crate::data::repo::add(&store, repo_dir.path(), "demo", "")
            .await
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let base = TempDir::new().unwrap();
        let wt = base.path().join("orphan");
        git::create_worktree(&repo.path, "orphan", None, &wt)
            .await
            .unwrap();
        let found = discover_untracked(&repo, &store).await.unwrap();
        // git worktree list reports canonical paths; macOS resolves $TMPDIR
        // through a /private symlink, so compare canonicalized.
        let wt_canon = std::fs::canonicalize(&wt).unwrap();
        assert!(found.iter().any(|w| {
            std::fs::canonicalize(&w.path)
                .map(|p| p == wt_canon)
                .unwrap_or(false)
        }));
    }

    #[tokio::test]
    async fn create_runs_setup_script_when_set() {
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let id = crate::data::repo::add(&store, repo_dir.path(), "demo", "")
            .await
            .unwrap();
        let base = TempDir::new().unwrap();
        // Touch a marker file inside the worktree.
        store
            .set_repo_setup_script(id, Some("touch wsx-setup-marker"))
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let created = create(
            &store,
            &repo,
            Some("a"),
            base.path(),
            false,
            crate::pty::session::AgentKind::Claude,
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(created.workspace.setup_status, SetupStatus::Ok);
        assert!(
            created
                .workspace
                .worktree_path
                .join("wsx-setup-marker")
                .exists()
        );
    }

    #[tokio::test]
    async fn archive_runs_archive_script_when_set() {
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let id = crate::data::repo::add(&store, repo_dir.path(), "demo", "")
            .await
            .unwrap();
        let base = TempDir::new().unwrap();
        let scratch = TempDir::new().unwrap();
        let marker = scratch.path().join("wsx-archive-marker");
        let script = format!("touch '{}'", marker.display());
        store.set_repo_archive_script(id, Some(&script)).unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let created = create(
            &store,
            &repo,
            Some("doomed"),
            base.path(),
            false,
            crate::pty::session::AgentKind::Claude,
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        archive(
            &store,
            &repo,
            &created.workspace,
            ArchiveOpts {
                force_branch_delete: true,
                ..Default::default()
            },
            |_| {},
        )
        .await
        .unwrap();
        assert!(marker.exists(), "archive script did not run");
    }

    #[tokio::test]
    async fn archive_with_app_removes_workspace_and_worktree() {
        use std::sync::Arc;
        use tokio::sync::Mutex;
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let id = crate::data::repo::add(&store, repo_dir.path(), "demo", "")
            .await
            .unwrap();
        let base = TempDir::new().unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let created = create(
            &store,
            &repo,
            Some("doomed"),
            base.path(),
            false,
            crate::pty::session::AgentKind::Claude,
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        let worktree_path = created.workspace.worktree_path.clone();
        let ws_id = created.workspace.id;
        // Build a minimal App wrapping the populated store so we can pass
        // it as SharedApp.
        let app = crate::app::App::new(store, base.path().to_path_buf()).unwrap();
        let shared = Arc::new(Mutex::new(app));
        let result = archive_with_app(
            shared.clone(),
            repo.clone(),
            created.workspace.clone(),
            ArchiveOpts {
                force_branch_delete: true,
                ..Default::default()
            },
        )
        .await;
        assert!(result.is_ok(), "archive_with_app failed: {result:?}");
        // Worktree is gone from disk.
        assert!(
            !worktree_path.exists(),
            "worktree still present after archive"
        );
        // Workspace row is gone from the store.
        let g = shared.lock().await;
        assert!(
            g.store
                .workspaces(repo.id)
                .unwrap()
                .iter()
                .all(|w| w.id != ws_id),
            "workspace row still present after archive"
        );
    }

    /// Regression test for the `advance_archive_step` wiring inside
    /// `archive_with_app`. The three calls between phases are easy to
    /// drop accidentally in a refactor; this test catches that. We
    /// seed the modal as `ArchiveRunning { Script }`, drive the full
    /// archive, and assert the modal ends on `Cleanup` (the last step
    /// advanced to, just before phase 4 begins). This test calls
    /// `archive_with_app` directly, so `reconcile_archive_result`
    /// never runs and the modal is left in its final advanced state.
    #[tokio::test]
    async fn archive_with_app_advances_modal_step_through_phases() {
        use crate::ui::modal::{ArchiveStep, Modal};
        use std::sync::Arc;
        use tokio::sync::Mutex;
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let id = crate::data::repo::add(&store, repo_dir.path(), "demo", "")
            .await
            .unwrap();
        let base = TempDir::new().unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let created = create(
            &store,
            &repo,
            Some("doomed"),
            base.path(),
            false,
            crate::pty::session::AgentKind::Claude,
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        let app = crate::app::App::new(store, base.path().to_path_buf()).unwrap();
        let shared = Arc::new(Mutex::new(app));
        {
            let mut g = shared.lock().await;
            g.modal = Some(Modal::ArchiveRunning {
                step: ArchiveStep::Script,
                script_present: false,
            });
        }
        let result = archive_with_app(
            shared.clone(),
            repo.clone(),
            created.workspace.clone(),
            ArchiveOpts {
                force_branch_delete: true,
                ..Default::default()
            },
        )
        .await;
        assert!(result.is_ok(), "archive_with_app failed: {result:?}");
        let g = shared.lock().await;
        match &g.modal {
            Some(Modal::ArchiveRunning { step, .. }) => {
                assert_eq!(
                    *step,
                    ArchiveStep::Cleanup,
                    "modal step should have advanced to Cleanup (the last step set before phase 4)"
                );
            }
            other => panic!("expected ArchiveRunning, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_returns_cancelled_when_token_cancelled_before_start() {
        use tokio_util::sync::CancellationToken;
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let base = TempDir::new().unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = create(
            &store,
            &repo,
            Some("alpha"),
            base.path(),
            false,
            crate::pty::session::AgentKind::Claude,
            cancel,
            |_| {},
        )
        .await;
        assert!(matches!(result, Err(Error::Cancelled)), "got {result:?}");
        let rows = store.workspaces(id).unwrap();
        assert!(
            rows.is_empty(),
            "no row should be inserted when pre-cancelled"
        );
    }

    #[tokio::test]
    async fn create_marks_setup_status_cancelled_when_cancelled_during_setup() {
        use tokio_util::sync::CancellationToken;
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        // Configure a slow setup script via the store.
        store.set_repo_setup_script(id, Some("sleep 10")).unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let base = TempDir::new().unwrap();
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            cancel_clone.cancel();
        });
        let result = create(
            &store,
            &repo,
            Some("alpha"),
            base.path(),
            false,
            crate::pty::session::AgentKind::Claude,
            cancel,
            |_| {},
        )
        .await;
        assert!(matches!(result, Err(Error::Cancelled)), "got {result:?}");
        let rows = store.workspaces(id).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].setup_status, SetupStatus::Cancelled);
        assert_eq!(rows[0].state, WorkspaceState::Ready);
        assert!(
            rows[0].worktree_path.exists(),
            "worktree should remain on disk"
        );
    }

    #[tokio::test]
    async fn create_with_app_works_end_to_end_without_holding_lock() {
        use crate::app::App;
        use std::sync::Arc;
        use tokio::sync::Mutex;
        use tokio_util::sync::CancellationToken;
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        let base = TempDir::new().unwrap();
        let app = Arc::new(Mutex::new(
            App::new(store, base.path().to_path_buf()).unwrap(),
        ));
        let repo = {
            let g = app.lock().await;
            g.repos[0].clone()
        };

        let cancel = CancellationToken::new();
        let progress = crate::data::progress::SetupProgress::shared();
        let created = create_with_app(
            app.clone(),
            repo,
            Some("alpha".to_string()),
            base.path().to_path_buf(),
            false,
            crate::pty::session::AgentKind::Claude,
            progress,
            cancel,
        )
        .await
        .unwrap();
        assert_eq!(created.workspace.name, "alpha");
        // The lock should NOT be held at this point — we can grab it.
        let g = app.try_lock().expect("lock should be free");
        drop(g);
    }

    #[tokio::test]
    async fn advance_archive_step_updates_step_when_modal_is_archive_running() {
        use crate::ui::modal::{ArchiveStep, Modal};
        use std::sync::Arc;
        use tokio::sync::Mutex;
        let store = Store::open_in_memory().unwrap();
        let tmp = TempDir::new().unwrap();
        let app = Arc::new(Mutex::new(
            crate::app::App::new(store, tmp.path().to_path_buf()).unwrap(),
        ));
        {
            let mut g = app.lock().await;
            g.modal = Some(Modal::ArchiveRunning {
                step: ArchiveStep::Script,
                script_present: true,
            });
        }
        super::advance_archive_step(&app, ArchiveStep::RemoveWorktree).await;
        let g = app.lock().await;
        match &g.modal {
            Some(Modal::ArchiveRunning {
                step,
                script_present,
            }) => {
                assert_eq!(
                    *step,
                    ArchiveStep::RemoveWorktree,
                    "step should be advanced"
                );
                assert!(*script_present, "script_present should not change");
            }
            other => panic!("expected ArchiveRunning, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn advance_archive_step_is_noop_when_modal_is_different_variant() {
        use crate::ui::modal::{ArchiveStep, Modal};
        use std::sync::Arc;
        use tokio::sync::Mutex;
        let store = Store::open_in_memory().unwrap();
        let tmp = TempDir::new().unwrap();
        let app = Arc::new(Mutex::new(
            crate::app::App::new(store, tmp.path().to_path_buf()).unwrap(),
        ));
        {
            let mut g = app.lock().await;
            g.modal = Some(Modal::Error {
                message: "boom".to_string(),
            });
        }
        super::advance_archive_step(&app, ArchiveStep::RemoveWorktree).await;
        let g = app.lock().await;
        match &g.modal {
            Some(Modal::Error { message }) => {
                assert_eq!(message, "boom", "Error modal should be untouched");
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn advance_archive_step_is_noop_when_modal_is_none() {
        use crate::ui::modal::ArchiveStep;
        use std::sync::Arc;
        use tokio::sync::Mutex;
        let store = Store::open_in_memory().unwrap();
        let tmp = TempDir::new().unwrap();
        let app = Arc::new(Mutex::new(
            crate::app::App::new(store, tmp.path().to_path_buf()).unwrap(),
        ));
        // modal starts as None.
        super::advance_archive_step(&app, ArchiveStep::RemoveWorktree).await;
        let g = app.lock().await;
        assert!(g.modal.is_none(), "modal should remain None");
    }

    #[tokio::test]
    async fn create_branches_off_configured_base() {
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        // Add a second commit on main so HEAD advances.
        let r = |args: &[&str]| {
            assert!(
                std::process::Command::new("git")
                    .current_dir(repo_dir.path())
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        };
        std::fs::write(repo_dir.path().join("b.txt"), "v1").unwrap();
        r(&["add", "b.txt"]);
        r(&["commit", "-q", "-m", "add b"]);
        let prev_out = std::process::Command::new("git")
            .current_dir(repo_dir.path())
            .args(["rev-parse", "HEAD~1"])
            .output()
            .unwrap();
        let prev_sha = String::from_utf8_lossy(&prev_out.stdout).trim().to_string();
        r(&["branch", "staging", &prev_sha]);

        let id = crate::data::repo::add(&store, repo_dir.path(), "demo", "")
            .await
            .unwrap();
        store.set_repo_base_branch(id, Some("staging")).unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let wt_root = TempDir::new().unwrap();

        let created = create(
            &store,
            &repo,
            Some("from-staging"),
            wt_root.path(),
            false,
            crate::pty::session::AgentKind::Claude,
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();

        let head = std::process::Command::new("git")
            .current_dir(&created.workspace.worktree_path)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let wt_head = String::from_utf8_lossy(&head.stdout).trim().to_string();
        assert_eq!(
            wt_head, prev_sha,
            "workspace should be at staging's commit, not main HEAD"
        );
    }
}
