use crate::error::{Error, Result};
use crate::git;
use crate::store::{Repo, RepoId, Store};
use std::path::Path;

pub async fn add(store: &Store, path: &Path, name: &str, branch_prefix: &str) -> Result<RepoId> {
    git::validate_repo(path).await?;
    if name.trim().is_empty() {
        return Err(Error::UserInput("repo name cannot be empty".into()));
    }
    store.add_repo(path, name, branch_prefix)
}

pub fn list(store: &Store) -> Result<Vec<Repo>> { store.repos() }

pub fn remove(store: &Store, id: RepoId) -> Result<()> { store.remove_repo(id) }

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn init_git_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        let run = |args: &[&str]| {
            let s = std::process::Command::new("git").current_dir(dir.path()).args(args).status().unwrap();
            assert!(s.success());
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "t@e"]);
        run(&["config", "user.name", "t"]);
        run(&["commit", "--allow-empty", "-q", "-m", "init"]);
        dir
    }

    #[tokio::test]
    async fn add_rejects_non_git_path() {
        let store = Store::open_in_memory().unwrap();
        let dir = TempDir::new().unwrap();
        assert!(add(&store, dir.path(), "x", "").await.is_err());
    }

    #[tokio::test]
    async fn add_rejects_empty_name() {
        let store = Store::open_in_memory().unwrap();
        let dir = init_git_repo();
        assert!(add(&store, dir.path(), "  ", "").await.is_err());
    }

    #[tokio::test]
    async fn add_then_list_then_remove() {
        let store = Store::open_in_memory().unwrap();
        let dir = init_git_repo();
        let id = add(&store, dir.path(), "demo", "wsx").await.unwrap();
        assert_eq!(list(&store).unwrap().len(), 1);
        remove(&store, id).unwrap();
        assert!(list(&store).unwrap().is_empty());
    }
}
