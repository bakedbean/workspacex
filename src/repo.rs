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

pub fn list(store: &Store) -> Result<Vec<Repo>> {
    store.repos()
}

pub fn remove(store: &Store, id: RepoId) -> Result<()> {
    store.remove_repo(id)
}

/// Resolve the effective branch prefix for a repo: per-repo value if set,
/// otherwise the global default from settings, otherwise empty.
pub fn resolve_branch_prefix(repo: &Repo, store: &Store) -> Result<String> {
    if !repo.branch_prefix.is_empty() {
        return Ok(repo.branch_prefix.clone());
    }
    Ok(store.get_setting("branch_prefix")?.unwrap_or_default())
}

/// Combine global custom_instructions with per-repo custom_instructions
/// (global first, blank line, repo). Returns None if both are unset.
pub fn resolve_custom_instructions(repo: &Repo, store: &Store) -> Result<Option<String>> {
    let global = store.get_setting("custom_instructions")?;
    let per_repo = repo.custom_instructions.clone();
    Ok(match (global, per_repo) {
        (None, None) => None,
        (Some(g), None) => Some(g),
        (None, Some(r)) => Some(r),
        (Some(g), Some(r)) => Some(format!("{g}\n\n{r}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn init_git_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        let run = |args: &[&str]| {
            let s = std::process::Command::new("git")
                .current_dir(dir.path())
                .args(args)
                .status()
                .unwrap();
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

#[cfg(test)]
mod settings_tests {
    use super::*;
    use crate::store::RepoId;
    use std::path::PathBuf;

    fn repo(prefix: &str, instructions: Option<&str>) -> Repo {
        Repo {
            id: RepoId(1),
            name: "demo".into(),
            path: PathBuf::from("/r"),
            branch_prefix: prefix.into(),
            custom_instructions: instructions.map(|s| s.to_string()),
            setup_script: None,
            archive_script: None,
            pinned_commands: None,
            related_repos: None,
            base_branch: None,
            created_at: 0,
        }
    }

    #[test]
    fn branch_prefix_repo_overrides_global() {
        let store = Store::open_in_memory().unwrap();
        store.set_setting("branch_prefix", "global").unwrap();
        assert_eq!(
            resolve_branch_prefix(&repo("repo", None), &store).unwrap(),
            "repo"
        );
        assert_eq!(
            resolve_branch_prefix(&repo("", None), &store).unwrap(),
            "global"
        );
    }

    #[test]
    fn branch_prefix_falls_back_to_empty() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(resolve_branch_prefix(&repo("", None), &store).unwrap(), "");
    }

    #[test]
    fn custom_instructions_concatenate() {
        let store = Store::open_in_memory().unwrap();
        store
            .set_setting("custom_instructions", "global text")
            .unwrap();
        let combined = resolve_custom_instructions(&repo("", Some("repo text")), &store).unwrap();
        assert_eq!(combined.as_deref(), Some("global text\n\nrepo text"));
    }

    #[test]
    fn custom_instructions_global_only() {
        let store = Store::open_in_memory().unwrap();
        store
            .set_setting("custom_instructions", "only global")
            .unwrap();
        let c = resolve_custom_instructions(&repo("", None), &store).unwrap();
        assert_eq!(c.as_deref(), Some("only global"));
    }

    #[test]
    fn custom_instructions_none_when_unset() {
        let store = Store::open_in_memory().unwrap();
        assert!(
            resolve_custom_instructions(&repo("", None), &store)
                .unwrap()
                .is_none()
        );
    }
}
