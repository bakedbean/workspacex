//! `scm_cache` accessors: per-workspace git/PR indicators for the
//! waybar/walker workspace menu. NULL columns mean "never computed" and
//! render as no indicator — distinct from a known "no PR" / "clean".
//! PR fields are written by the TUI's background poll and the
//! `wsx waybar refresh-prs` sweep; git fields by `wsx waybar menu-entries`.

use std::collections::HashMap;

use crate::data::store::{Store, WorkspaceId};
use crate::error::Result;
use crate::git::forge::BranchLifecycle;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScmCacheRow {
    pub pr_lifecycle: Option<BranchLifecycle>,
    pub pr_number: Option<u32>,
    pub dirty: Option<bool>,
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
    pub fetched_at: Option<i64>,
    pub pr_url: Option<String>,
    pub git_fetched_at: Option<i64>,
}

pub(crate) fn lifecycle_to_str(l: BranchLifecycle) -> &'static str {
    match l {
        BranchLifecycle::NoPr => "no_pr",
        BranchLifecycle::PrDraft => "draft",
        BranchLifecycle::PrOpen => "open",
        BranchLifecycle::PrConflicted => "conflicted",
        BranchLifecycle::PrMerged => "merged",
        BranchLifecycle::PrClosed => "closed",
    }
}

pub(crate) fn lifecycle_from_str(s: &str) -> Option<BranchLifecycle> {
    match s {
        "no_pr" => Some(BranchLifecycle::NoPr),
        "draft" => Some(BranchLifecycle::PrDraft),
        "open" => Some(BranchLifecycle::PrOpen),
        "conflicted" => Some(BranchLifecycle::PrConflicted),
        "merged" => Some(BranchLifecycle::PrMerged),
        "closed" => Some(BranchLifecycle::PrClosed),
        _ => None,
    }
}

impl Store {
    pub fn upsert_scm_pr(
        &self,
        id: WorkspaceId,
        lifecycle: BranchLifecycle,
        number: Option<u32>,
        url: Option<&str>,
        fetched_at: i64,
    ) -> Result<()> {
        self.conn().execute(
            // A NULL incoming url keeps any previously cached one: a fetch
            // that knows the lifecycle but not the url must not regress the
            // menu's "Open PR" action. NoPr clears it — no PR, no URL.
            "INSERT INTO scm_cache (workspace_id, pr_lifecycle, pr_number, pr_url, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(workspace_id) DO UPDATE SET
               pr_lifecycle = excluded.pr_lifecycle,
               pr_number    = excluded.pr_number,
               pr_url       = CASE
                 WHEN excluded.pr_url IS NOT NULL THEN excluded.pr_url
                 WHEN excluded.pr_lifecycle = 'no_pr' THEN NULL
                 ELSE scm_cache.pr_url
               END,
               fetched_at   = excluded.fetched_at",
            rusqlite::params![id.0, lifecycle_to_str(lifecycle), number, url, fetched_at],
        )?;
        Ok(())
    }

    pub fn upsert_scm_git(
        &self,
        id: WorkspaceId,
        dirty: bool,
        additions: u32,
        deletions: u32,
        git_fetched_at: i64,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT INTO scm_cache (workspace_id, dirty, additions, deletions, git_fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(workspace_id) DO UPDATE SET
               dirty          = excluded.dirty,
               additions      = excluded.additions,
               deletions      = excluded.deletions,
               git_fetched_at = excluded.git_fetched_at",
            rusqlite::params![id.0, dirty as i64, additions, deletions, git_fetched_at],
        )?;
        Ok(())
    }

    /// NULLs the git-derived fields (dirty/additions/deletions) while
    /// stamping `git_fetched_at` and leaving PR fields untouched. Used when
    /// a fresh git read fails so stale indicators don't linger under a
    /// fresh timestamp.
    pub fn clear_scm_git(&self, id: WorkspaceId, git_fetched_at: i64) -> Result<()> {
        self.conn().execute(
            "INSERT INTO scm_cache (workspace_id, git_fetched_at) VALUES (?1, ?2)
             ON CONFLICT(workspace_id) DO UPDATE SET
               dirty = NULL, additions = NULL, deletions = NULL,
               git_fetched_at = excluded.git_fetched_at",
            rusqlite::params![id.0, git_fetched_at],
        )?;
        Ok(())
    }

    pub fn all_scm_cache(&self) -> Result<HashMap<WorkspaceId, ScmCacheRow>> {
        let mut stmt = self.conn().prepare(
            "SELECT workspace_id, pr_lifecycle, pr_number, dirty, additions, deletions, \
                    fetched_at, pr_url, git_fetched_at
             FROM scm_cache",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                WorkspaceId(r.get(0)?),
                ScmCacheRow {
                    pr_lifecycle: r
                        .get::<_, Option<String>>(1)?
                        .as_deref()
                        .and_then(lifecycle_from_str),
                    pr_number: r.get(2)?,
                    dirty: r.get::<_, Option<i64>>(3)?.map(|v| v != 0),
                    additions: r.get(4)?,
                    deletions: r.get(5)?,
                    fetched_at: r.get(6)?,
                    pr_url: r.get(7)?,
                    git_fetched_at: r.get(8)?,
                },
            ))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (id, cache) = row?;
            map.insert(id, cache);
        }
        Ok(map)
    }
}

#[cfg(test)]
mod scm_cache_tests {
    use crate::data::store::{NewWorkspace, Store};
    use crate::git::forge::BranchLifecycle;
    use crate::pty::session::AgentKind;
    use std::path::Path;

    fn store_with_workspace() -> (Store, crate::data::store::WorkspaceId) {
        let store = Store::open_in_memory().unwrap();
        let repo = store.add_repo(Path::new("/tmp/r"), "r", "x").unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "w",
                branch: "x/w",
                worktree_path: &std::path::PathBuf::from("/tmp/r/w"),
                yolo: false,
                agent: AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        (store, id)
    }

    #[test]
    fn upserts_merge_into_one_row() {
        let (store, id) = store_with_workspace();
        assert!(store.all_scm_cache().unwrap().is_empty());

        store.upsert_scm_git(id, true, 4, 2, 500).unwrap();
        store
            .upsert_scm_pr(id, BranchLifecycle::PrOpen, Some(12), None, 1000)
            .unwrap();

        let row = store.all_scm_cache().unwrap()[&id].clone();
        assert_eq!(row.dirty, Some(true));
        assert_eq!(row.additions, Some(4));
        assert_eq!(row.deletions, Some(2));
        assert_eq!(row.pr_lifecycle, Some(BranchLifecycle::PrOpen));
        assert_eq!(row.pr_number, Some(12));
        assert_eq!(row.fetched_at, Some(1000));

        // A git-only update must not clobber PR fields, and vice versa.
        store.upsert_scm_git(id, false, 0, 0, 600).unwrap();
        let row = store.all_scm_cache().unwrap()[&id].clone();
        assert_eq!(row.pr_lifecycle, Some(BranchLifecycle::PrOpen));
        assert_eq!(row.dirty, Some(false));
    }

    #[test]
    fn pr_number_none_clears_stored_number() {
        let (store, id) = store_with_workspace();
        store
            .upsert_scm_pr(id, BranchLifecycle::PrOpen, Some(7), None, 1000)
            .unwrap();
        store
            .upsert_scm_pr(id, BranchLifecycle::NoPr, None, None, 2000)
            .unwrap();
        let row = store.all_scm_cache().unwrap()[&id].clone();
        assert_eq!(row.pr_lifecycle, Some(BranchLifecycle::NoPr));
        assert_eq!(row.pr_number, None);
    }

    #[test]
    fn null_url_preserves_cached_url_except_for_no_pr() {
        let (store, id) = store_with_workspace();
        let url = "https://github.com/o/r/pull/7";
        store
            .upsert_scm_pr(id, BranchLifecycle::PrOpen, Some(7), Some(url), 1000)
            .unwrap();
        // A later fetch that knows the lifecycle but not the url must not
        // clear the cached one ("Open PR" would silently vanish).
        store
            .upsert_scm_pr(id, BranchLifecycle::PrConflicted, Some(7), None, 2000)
            .unwrap();
        let row = store.all_scm_cache().unwrap()[&id].clone();
        assert_eq!(row.pr_url.as_deref(), Some(url));
        assert_eq!(row.pr_lifecycle, Some(BranchLifecycle::PrConflicted));
        // A fresh url still overwrites.
        let url2 = "https://github.com/o/r/pull/8";
        store
            .upsert_scm_pr(id, BranchLifecycle::PrOpen, Some(8), Some(url2), 3000)
            .unwrap();
        let row = store.all_scm_cache().unwrap()[&id].clone();
        assert_eq!(row.pr_url.as_deref(), Some(url2));
        // NoPr clears: no PR, no URL.
        store
            .upsert_scm_pr(id, BranchLifecycle::NoPr, None, None, 4000)
            .unwrap();
        let row = store.all_scm_cache().unwrap()[&id].clone();
        assert_eq!(row.pr_url, None);
    }

    #[test]
    fn lifecycle_round_trips_through_text() {
        for l in [
            BranchLifecycle::NoPr,
            BranchLifecycle::PrDraft,
            BranchLifecycle::PrOpen,
            BranchLifecycle::PrConflicted,
            BranchLifecycle::PrMerged,
            BranchLifecycle::PrClosed,
        ] {
            assert_eq!(
                super::lifecycle_from_str(super::lifecycle_to_str(l)),
                Some(l),
                "{l:?}"
            );
        }
        assert_eq!(super::lifecycle_from_str("garbage"), None);
    }

    #[test]
    fn remove_workspace_deletes_cache_row() {
        let (store, id) = store_with_workspace();
        store.upsert_scm_git(id, true, 1, 1, 1000).unwrap();
        store.delete_workspace(id).unwrap();
        assert!(store.all_scm_cache().unwrap().is_empty());
    }

    /// Regression: `wsx repo remove` (Store::remove_repo) manually cascades
    /// deletes for agent_messages/workspace_agents before dropping
    /// workspaces, but scm_cache has no ON DELETE CASCADE either. Before the
    /// fix, this failed with "FOREIGN KEY constraint failed" once the menu
    /// or TUI had populated the cache for a workspace in the repo.
    #[test]
    fn remove_repo_deletes_cache_rows_for_its_workspaces() {
        let (store, id) = store_with_workspace();
        store.upsert_scm_git(id, true, 3, 1, 1000).unwrap();
        assert!(!store.all_scm_cache().unwrap().is_empty());

        let repo_id = store.repos().unwrap()[0].id;
        crate::data::repo::remove(&store, repo_id).unwrap();

        assert!(store.all_scm_cache().unwrap().is_empty());
        assert!(store.repos().unwrap().is_empty());
    }

    #[test]
    fn v19_columns_round_trip() {
        let (store, id) = store_with_workspace();
        store
            .upsert_scm_pr(
                id,
                BranchLifecycle::PrOpen,
                Some(12),
                Some("https://github.com/o/r/pull/12"),
                1000,
            )
            .unwrap();
        store.upsert_scm_git(id, true, 4, 2, 2000).unwrap();
        let row = store.all_scm_cache().unwrap()[&id].clone();
        assert_eq!(
            row.pr_url.as_deref(),
            Some("https://github.com/o/r/pull/12")
        );
        assert_eq!(row.git_fetched_at, Some(2000));
        assert_eq!(row.fetched_at, Some(1000));
    }

    #[test]
    fn clear_scm_git_nulls_git_fields_and_stamps_time() {
        let (store, id) = store_with_workspace();
        store.upsert_scm_git(id, true, 4, 2, 1000).unwrap();
        store
            .upsert_scm_pr(id, BranchLifecycle::PrOpen, Some(7), None, 1000)
            .unwrap();
        store.clear_scm_git(id, 3000).unwrap();
        let row = store.all_scm_cache().unwrap()[&id].clone();
        assert_eq!(row.dirty, None);
        assert_eq!(row.additions, None);
        assert_eq!(row.deletions, None);
        assert_eq!(row.git_fetched_at, Some(3000));
        // PR fields untouched.
        assert_eq!(row.pr_number, Some(7));
    }
}
