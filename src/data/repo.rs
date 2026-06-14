use crate::data::store::{Repo, RepoId, Store, now_ms};
use crate::error::{Error, Result};
use crate::git;
use rusqlite::OptionalExtension;
use std::path::{Path, PathBuf};

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

impl Store {
    pub fn add_repo(&self, path: &Path, name: &str, branch_prefix: &str) -> Result<RepoId> {
        let now = now_ms();
        self.conn().execute(
            "INSERT INTO repos (name, path, branch_prefix, created_at, sort_order) \
             VALUES (?1, ?2, ?3, ?4, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM repos))",
            rusqlite::params![name, path.to_string_lossy(), branch_prefix, now],
        )?;
        Ok(RepoId(self.conn().last_insert_rowid()))
    }

    pub fn remove_repo(&self, id: RepoId) -> Result<()> {
        // Clear agent-instance rows before deleting workspaces.
        // `workspace_agents.workspace_id` has no ON DELETE CASCADE, so the
        // FK constraint would block the workspace delete without this.
        //
        // Manual cascade: agent_messages.target_agent_id → workspace_agents → workspaces
        self.conn().execute(
            "DELETE FROM agent_messages WHERE workspace_id IN \
                 (SELECT id FROM workspaces WHERE repo_id = ?1)",
            [id.0],
        )?;
        self.conn().execute(
            "DELETE FROM workspace_agents WHERE workspace_id IN \
                 (SELECT id FROM workspaces WHERE repo_id = ?1)",
            [id.0],
        )?;
        self.conn()
            .execute("DELETE FROM workspaces WHERE repo_id = ?1", [id.0])?;
        self.conn()
            .execute("DELETE FROM repos WHERE id = ?1", [id.0])?;
        Ok(())
    }

    pub fn repos(&self) -> Result<Vec<Repo>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, name, path, branch_prefix, custom_instructions, \
                    setup_script, archive_script, pinned_commands, \
                    related_repos, base_branch, detail_bar_config, \
                    created_at, sort_order \
             FROM repos ORDER BY sort_order, id",
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
                related_repos: r.get(8)?,
                base_branch: r.get(9)?,
                detail_bar_config: r.get(10)?,
                created_at: r.get(11)?,
                sort_order: r.get(12)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Swap the `sort_order` of two repos. Used by the dashboard to move a
    /// repo up/down by one slot. Atomic so a crash can't leave a half-swap.
    pub fn swap_repo_sort_order(&self, a: RepoId, b: RepoId) -> Result<()> {
        let tx = self.conn().unchecked_transaction()?;
        let so_a: i64 = tx.query_row("SELECT sort_order FROM repos WHERE id = ?1", [a.0], |r| {
            r.get(0)
        })?;
        let so_b: i64 = tx.query_row("SELECT sort_order FROM repos WHERE id = ?1", [b.0], |r| {
            r.get(0)
        })?;
        tx.execute(
            "UPDATE repos SET sort_order = ?1 WHERE id = ?2",
            rusqlite::params![so_b, a.0],
        )?;
        tx.execute(
            "UPDATE repos SET sort_order = ?1 WHERE id = ?2",
            rusqlite::params![so_a, b.0],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn set_repo_branch_prefix(&self, id: RepoId, prefix: &str) -> Result<()> {
        self.conn().execute(
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
        self.conn().execute(
            "UPDATE repos SET custom_instructions = ?1 WHERE id = ?2",
            rusqlite::params![instructions, id.0],
        )?;
        Ok(())
    }

    pub fn set_repo_setup_script(&self, id: RepoId, value: Option<&str>) -> Result<()> {
        self.conn().execute(
            "UPDATE repos SET setup_script = ?1 WHERE id = ?2",
            rusqlite::params![value, id.0],
        )?;
        Ok(())
    }

    pub fn set_repo_archive_script(&self, id: RepoId, value: Option<&str>) -> Result<()> {
        self.conn().execute(
            "UPDATE repos SET archive_script = ?1 WHERE id = ?2",
            rusqlite::params![value, id.0],
        )?;
        Ok(())
    }

    pub fn set_repo_pinned_commands(&self, id: RepoId, value: Option<&str>) -> Result<()> {
        self.conn().execute(
            "UPDATE repos SET pinned_commands = ?1 WHERE id = ?2",
            rusqlite::params![value, id.0],
        )?;
        Ok(())
    }

    pub fn set_repo_related_repos(&self, id: RepoId, value: Option<&str>) -> Result<()> {
        self.conn().execute(
            "UPDATE repos SET related_repos = ?1 WHERE id = ?2",
            rusqlite::params![value, id.0],
        )?;
        Ok(())
    }

    pub fn set_repo_name(&self, id: RepoId, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(crate::error::Error::UserInput(
                "repo name cannot be empty".into(),
            ));
        }
        // Check for duplicate name on a different repo.
        let dup: std::result::Result<Option<i64>, _> = self
            .conn()
            .query_row(
                "SELECT id FROM repos WHERE name = ?1 AND id != ?2",
                rusqlite::params![name, id.0],
                |r| r.get(0),
            )
            .optional();
        if let Ok(Some(_existing_id)) = dup {
            return Err(crate::error::Error::UserInput(format!(
                "a repo named '{name}' already exists"
            )));
        }
        // Read the old name for the related_repos cascade.
        let old_name: String =
            self.conn()
                .query_row("SELECT name FROM repos WHERE id = ?1", [id.0], |r| {
                    r.get::<_, String>(0)
                })?;

        self.conn().execute(
            "UPDATE repos SET name = ?1 WHERE id = ?2",
            rusqlite::params![name, id.0],
        )?;

        // Cascade: rewrite related_repos entries in other repos that
        // mention the old name. We do this in Rust to avoid substring
        // false positives (e.g. "front" matching inside "frontend").
        let mut stmt = self.conn().prepare(
            "SELECT id, related_repos FROM repos \
             WHERE related_repos IS NOT NULL AND id != ?1",
        )?;
        let rows: Vec<(i64, String)> =
            match stmt.query_map([id.0], |r| Ok((r.get(0)?, r.get::<_, String>(1)?))) {
                Ok(mapped) => mapped.filter_map(|r| r.ok()).collect(),
                Err(_) => Vec::new(),
            };
        drop(stmt);
        for (other_id, spec) in rows {
            let names = crate::agent::related::parse(&spec);
            if !names.iter().any(|n| n == &old_name) {
                continue;
            }
            let mut new_parts: Vec<&str> = names
                .iter()
                .map(|n| if n == &old_name { name } else { n.as_str() })
                .collect();
            new_parts.dedup();
            let new_spec = new_parts.join(", ");
            self.conn().execute(
                "UPDATE repos SET related_repos = ?1 WHERE id = ?2",
                rusqlite::params![new_spec, other_id],
            )?;
        }

        Ok(())
    }

    pub fn set_repo_base_branch(&self, id: RepoId, value: Option<&str>) -> Result<()> {
        self.conn().execute(
            "UPDATE repos SET base_branch = ?1 WHERE id = ?2",
            rusqlite::params![value, id.0],
        )?;
        Ok(())
    }

    pub fn set_repo_detail_bar_config(&self, id: RepoId, value: Option<&str>) -> Result<()> {
        self.conn().execute(
            "UPDATE repos SET detail_bar_config = ?1 WHERE id = ?2",
            rusqlite::params![value, id.0],
        )?;
        Ok(())
    }
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
    use crate::data::store::RepoId;
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
            detail_bar_config: None,
            created_at: 0,
            sort_order: 0,
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
