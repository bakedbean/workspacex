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
    yolo: bool,
    on_setup_line: F,
) -> Result<CreatedWorkspace> {
    let name = match name {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => names::generate(),
    };
    let prefix = crate::repo::resolve_branch_prefix(repo, store)?;
    let branch = if prefix.is_empty() {
        name.clone()
    } else {
        format!("{}/{}", prefix.trim_end_matches('/'), name)
    };
    let worktree_path = worktree_base.join(&repo.name).join(&name);

    let base = repo
        .base_branch
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    // Fetch before inserting the workspace row so a fetch failure
    // (network down, bad remote ref) doesn't leave an orphan Pending row.
    git::fetch_for_base(&repo.path, base).await?;

    let id = store.insert_workspace(&NewWorkspace {
        repo_id: repo.id,
        name: &name,
        branch: &branch,
        worktree_path: &worktree_path,
        yolo,
    })?;

    if let Err(e) = git::create_worktree(&repo.path, &branch, base, &worktree_path).await {
        store.set_workspace_state(id, WorkspaceState::Failed)?;
        return Err(e);
    }
    store.set_workspace_state(id, WorkspaceState::Ready)?;

    let setup_result = setup::run_setup(
        repo.setup_script.as_deref(),
        &repo.path,
        &worktree_path,
        tokio_util::sync::CancellationToken::new(),
        on_setup_line,
    )
    .await?;
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
    if crate::mcp::enabled(store)
        && let Err(e) = crate::mcp::remove_worktree_entry(&ws.worktree_path)
    {
        tracing::warn!(error = %e, "failed to remove worktree entry from ~/.claude.json");
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
    let id = store.insert_workspace(&NewWorkspace {
        repo_id: repo.id,
        name,
        branch: &branch,
        worktree_path: &info.path,
        yolo: false,
    })?;
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
    let prefix = crate::repo::resolve_branch_prefix(repo, store)?;
    let new_branch = if prefix.is_empty() {
        new_name.to_string()
    } else {
        format!("{}/{}", prefix.trim_end_matches('/'), new_name)
    };
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

        let created = create(&store, &repo, Some("alpha"), base.path(), false, |_| {})
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

        let created = create(&store, &repo, Some("wild"), base.path(), true, |_| {})
            .await
            .unwrap();
        assert!(created.workspace.yolo);
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
        let created = create(&store, &repo, None, base.path(), false, |_| {})
            .await
            .unwrap();
        assert!(created.workspace.name.contains('-'));
    }

    #[tokio::test]
    async fn create_records_setup_failure_but_keeps_workspace_ready() {
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let id = crate::repo::add(&store, repo_dir.path(), "demo", "")
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
        let created = create(&store, &repo, Some("a"), base.path(), false, |_| {})
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
        let created = create(&store, &repo, Some("doomed"), base.path(), false, |_| {})
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
        let created = create(&store, &repo, Some("alpha"), base.path(), false, |_| {})
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
        git::create_worktree(&repo.path, "orphan", None, &wt)
            .await
            .unwrap();
        let found = discover_untracked(&repo, &store).await.unwrap();
        assert!(found.iter().any(|w| w.path == wt));
    }

    #[tokio::test]
    async fn create_runs_setup_script_when_set() {
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let id = crate::repo::add(&store, repo_dir.path(), "demo", "")
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
        let created = create(&store, &repo, Some("a"), base.path(), false, |_| {})
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
        let id = crate::repo::add(&store, repo_dir.path(), "demo", "")
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
        let created = create(&store, &repo, Some("doomed"), base.path(), false, |_| {})
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

        let id = crate::repo::add(&store, repo_dir.path(), "demo", "")
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
