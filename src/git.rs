#![allow(clippy::collapsible_if)]

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// Run `git -C <cwd> <args...>` and return stdout on success, mapping
/// non-zero exit + stderr into `Error::Git`.
async fn run(cwd: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .await
        .map_err(|e| Error::Git(format!("spawn git: {e}")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(Error::Git(format!(
            "git {} failed: {stderr}",
            args.join(" ")
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub async fn validate_repo(path: &Path) -> Result<()> {
    let out = run(path, &["rev-parse", "--is-inside-work-tree"]).await?;
    if out.trim() != "true" {
        return Err(Error::Git(format!(
            "{} is not a git work tree",
            path.display()
        )));
    }
    Ok(())
}

pub async fn current_branch(path: &Path) -> Result<String> {
    let out = run(path, &["rev-parse", "--abbrev-ref", "HEAD"]).await?;
    Ok(out.trim().to_string())
}

pub async fn head_commit(path: &Path) -> Result<String> {
    let out = run(path, &["rev-parse", "HEAD"]).await?;
    Ok(out.trim().to_string())
}

pub async fn preflight() -> Result<()> {
    let out = Command::new("git")
        .arg("--version")
        .output()
        .await
        .map_err(|e| Error::Git(format!("git not found on PATH: {e}")))?;
    if !out.status.success() {
        return Err(Error::Git("git --version failed".into()));
    }
    Ok(())
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::process::Command as StdCmd;
    use tempfile::TempDir;

    pub(super) fn init_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        let run = |args: &[&str]| {
            let status = StdCmd::new("git")
                .current_dir(dir.path())
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {:?} failed", args);
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        run(&["commit", "--allow-empty", "-q", "-m", "init"]);
        dir
    }

    #[tokio::test]
    async fn validate_repo_accepts_real_repo() {
        let dir = init_repo();
        validate_repo(dir.path()).await.unwrap();
    }

    #[tokio::test]
    async fn validate_repo_rejects_non_repo() {
        let dir = TempDir::new().unwrap();
        assert!(validate_repo(dir.path()).await.is_err());
    }

    #[tokio::test]
    async fn current_branch_and_head() {
        let dir = init_repo();
        assert_eq!(current_branch(dir.path()).await.unwrap(), "main");
        let head = head_commit(dir.path()).await.unwrap();
        assert_eq!(head.len(), 40);
    }

    #[tokio::test]
    async fn preflight_succeeds_when_git_on_path() {
        preflight().await.unwrap();
    }
}

#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub head: Option<String>,
}

pub async fn create_worktree(repo: &Path, branch: &str, path: &Path) -> Result<()> {
    let path_s = path.to_string_lossy();
    run(repo, &["worktree", "add", "-b", branch, &path_s]).await?;
    Ok(())
}

pub async fn remove_worktree(repo: &Path, path: &Path) -> Result<()> {
    let path_s = path.to_string_lossy();
    run(repo, &["worktree", "remove", "--force", &path_s]).await?;
    Ok(())
}

pub async fn list_worktrees(repo: &Path) -> Result<Vec<WorktreeInfo>> {
    let out = run(repo, &["worktree", "list", "--porcelain"]).await?;
    let mut result = Vec::new();
    let mut cur: Option<WorktreeInfo> = None;
    for line in out.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            if let Some(w) = cur.take() {
                result.push(w);
            }
            cur = Some(WorktreeInfo {
                path: PathBuf::from(p),
                branch: None,
                head: None,
            });
        } else if let Some(h) = line.strip_prefix("HEAD ") {
            if let Some(c) = cur.as_mut() {
                c.head = Some(h.to_string());
            }
        } else if let Some(b) = line.strip_prefix("branch ") {
            if let Some(c) = cur.as_mut() {
                c.branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
            }
        }
    }
    if let Some(w) = cur.take() {
        result.push(w);
    }
    Ok(result)
}

pub async fn branch_delete(repo: &Path, branch: &str, force: bool) -> Result<()> {
    let flag = if force { "-D" } else { "-d" };
    run(repo, &["branch", flag, branch]).await?;
    Ok(())
}

#[cfg(test)]
mod worktree_tests {
    use super::tests::init_repo;
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn create_and_list_worktree() {
        let repo = init_repo();
        let wt_root = TempDir::new().unwrap();
        let wt = wt_root.path().join("feature");
        create_worktree(repo.path(), "feature", &wt).await.unwrap();
        let listed = list_worktrees(repo.path()).await.unwrap();
        assert!(
            listed
                .iter()
                .any(|w| w.path == wt && w.branch.as_deref() == Some("feature"))
        );
    }

    #[tokio::test]
    async fn remove_worktree_cleans_up() {
        let repo = init_repo();
        let wt_root = TempDir::new().unwrap();
        let wt = wt_root.path().join("scratch");
        create_worktree(repo.path(), "scratch", &wt).await.unwrap();
        remove_worktree(repo.path(), &wt).await.unwrap();
        let listed = list_worktrees(repo.path()).await.unwrap();
        assert!(!listed.iter().any(|w| w.path == wt));
    }
}
