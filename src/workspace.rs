use crate::error::{Error, Result};
use crate::git;
use crate::names;
use crate::setup::{self, SetupLine, SetupResult};
use crate::store::{
    NewWorkspace, Repo, SetupStatus, Store, Workspace, WorkspaceId, WorkspaceState,
};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct CreatedWorkspace {
    pub workspace: Workspace,
    pub setup_result: SetupResult,
}

/// Create a new workspace: insert pending row, create worktree, mark
/// ready, run setup script, record setup status.
pub async fn create<F: FnMut(SetupLine) + Send>(
    store: &Store,
    repo: &Repo,
    name: Option<&str>,
    worktree_base: &Path,
    on_setup_line: F,
) -> Result<CreatedWorkspace> {
    let name = match name {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => names::generate(),
    };
    let branch = if repo.branch_prefix.is_empty() {
        name.clone()
    } else {
        format!("{}/{}", repo.branch_prefix.trim_end_matches('/'), name)
    };
    let worktree_path = worktree_base.join(&repo.name).join(&name);

    let id = store.insert_workspace(&NewWorkspace {
        repo_id: repo.id,
        name: &name,
        branch: &branch,
        worktree_path: &worktree_path,
    })?;

    if let Err(e) = git::create_worktree(&repo.path, &branch, &worktree_path).await {
        store.set_workspace_state(id, WorkspaceState::Failed)?;
        return Err(e);
    }
    store.set_workspace_state(id, WorkspaceState::Ready)?;

    let setup_result = setup::run_setup(&repo.path, &worktree_path, on_setup_line).await?;
    let status = match &setup_result {
        SetupResult::Ok => SetupStatus::Ok,
        SetupResult::Skipped => SetupStatus::Skipped,
        SetupResult::Failed { .. } => SetupStatus::Failed,
    };
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
    let archive_result = setup::run_archive(&repo.path, &ws.worktree_path, on_archive_line).await?;
    if !opts.keep_worktree && ws.worktree_path.exists() {
        git::remove_worktree(&repo.path, &ws.worktree_path).await?;
    }
    let _ = git::branch_delete(&repo.path, &ws.branch, opts.force_branch_delete).await;
    store.delete_workspace(ws.id)?;
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
    let id = store.insert_workspace(&NewWorkspace {
        repo_id: repo.id,
        name,
        branch: &branch,
        worktree_path: &info.path,
    })?;
    store.set_workspace_state(id, WorkspaceState::Ready)?;
    store.set_setup_status(id, SetupStatus::Skipped)?;
    Ok(id)
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
        let id = crate::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let base = TempDir::new().unwrap();

        let created = create(&store, &repo, Some("alpha"), base.path(), |_| {})
            .await
            .unwrap();
        assert_eq!(created.workspace.name, "alpha");
        assert_eq!(created.workspace.branch, "wsx/alpha");
        assert_eq!(created.workspace.state, WorkspaceState::Ready);
        assert_eq!(created.workspace.setup_status, SetupStatus::Skipped);
        assert!(created.workspace.worktree_path.join(".git").exists());
    }

    #[tokio::test]
    async fn create_generates_name_when_none_given() {
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let id = crate::repo::add(&store, repo_dir.path(), "demo", "")
            .await
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let base = TempDir::new().unwrap();
        let created = create(&store, &repo, None, base.path(), |_| {})
            .await
            .unwrap();
        assert!(created.workspace.name.contains('-'));
    }

    #[tokio::test]
    async fn create_records_setup_failure_but_keeps_workspace_ready() {
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        std::fs::write(
            repo_dir.path().join(".claudette.json"),
            r#"{"setup":{"command":"sh","args":["-c","exit 1"]}}"#,
        )
        .unwrap();
        let id = crate::repo::add(&store, repo_dir.path(), "demo", "")
            .await
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let base = TempDir::new().unwrap();
        let created = create(&store, &repo, Some("a"), base.path(), |_| {})
            .await
            .unwrap();
        assert_eq!(created.workspace.state, WorkspaceState::Ready);
        assert_eq!(created.workspace.setup_status, SetupStatus::Failed);
    }

    #[tokio::test]
    async fn archive_removes_row_and_worktree() {
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let id = crate::repo::add(&store, repo_dir.path(), "demo", "")
            .await
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let base = TempDir::new().unwrap();
        let created = create(&store, &repo, Some("doomed"), base.path(), |_| {})
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

    #[tokio::test]
    async fn discover_finds_untracked_worktree() {
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let id = crate::repo::add(&store, repo_dir.path(), "demo", "")
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
        git::create_worktree(&repo.path, "orphan", &wt)
            .await
            .unwrap();
        let found = discover_untracked(&repo, &store).await.unwrap();
        assert!(found.iter().any(|w| w.path == wt));
    }
}
