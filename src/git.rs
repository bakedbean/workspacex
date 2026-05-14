use crate::error::{Error, Result};
use std::path::Path;
use tokio::process::Command;

/// Run `git -C <cwd> <args...>` and return stdout on success, mapping
/// non-zero exit + stderr into `Error::Git`.
async fn run(cwd: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git").arg("-C").arg(cwd).args(args)
        .output().await.map_err(|e| Error::Git(format!("spawn git: {e}")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(Error::Git(format!("git {} failed: {stderr}", args.join(" "))));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub async fn validate_repo(path: &Path) -> Result<()> {
    let out = run(path, &["rev-parse", "--is-inside-work-tree"]).await?;
    if out.trim() != "true" {
        return Err(Error::Git(format!("{} is not a git work tree", path.display())));
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

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::process::Command as StdCmd;
    use tempfile::TempDir;

    pub(super) fn init_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        let run = |args: &[&str]| {
            let status = StdCmd::new("git").current_dir(dir.path()).args(args).status().unwrap();
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
}
