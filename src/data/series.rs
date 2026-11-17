//! Workspace series (the `series` / `series_parts` tables): the numbered
//! chain of workspaces a feature ships through, joined via
//! `wsx workspace create --continues`. Part rows have no FK to
//! `workspaces`, so a series outlives archiving its parts.

use crate::data::scm_cache::lifecycle_from_str;
use crate::data::store::{RepoId, Store, WorkspaceId};
use crate::error::Result;
use crate::git::forge::BranchLifecycle;
use rusqlite::OptionalExtension;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SeriesId(pub i64);

/// One workspace's place in a series. `pr_*` is a snapshot: current for an
/// archived part, possibly stale for a live one (prefer live PR state).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesPart {
    pub series_id: SeriesId,
    pub part: u32,
    pub workspace_id: WorkspaceId,
    pub slug: String,
    pub branch: String,
    pub pr_number: Option<u32>,
    pub pr_lifecycle: Option<BranchLifecycle>,
    pub archived_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Series {
    pub id: SeriesId,
    pub repo_id: RepoId,
    pub title: Option<String>,
    /// Ascending by `part`.
    pub parts: Vec<SeriesPart>,
}

impl Series {
    /// The title, or part 1's slug while the series has none.
    pub fn display_title(&self) -> &str {
        self.title
            .as_deref()
            .or_else(|| self.parts.first().map(|p| p.slug.as_str()))
            .unwrap_or_default()
    }
}

/// What `continue_series` did: `started` when the parent wasn't in a
/// series yet and became part 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesJoin {
    pub series_id: SeriesId,
    pub part: u32,
    pub started: bool,
    pub title: Option<String>,
}

const PART_COLUMNS: &str = "p.series_id, p.part, p.workspace_id, p.slug, p.branch, \
                            p.pr_number, p.pr_lifecycle, p.archived_at";

/// Decode the `PART_COLUMNS` starting at column `at`.
pub(crate) fn row_to_part(r: &rusqlite::Row<'_>, at: usize) -> rusqlite::Result<SeriesPart> {
    Ok(SeriesPart {
        series_id: SeriesId(r.get(at)?),
        part: r.get(at + 1)?,
        workspace_id: WorkspaceId(r.get(at + 2)?),
        slug: r.get(at + 3)?,
        branch: r.get(at + 4)?,
        pr_number: r.get(at + 5)?,
        pr_lifecycle: r
            .get::<_, Option<String>>(at + 6)?
            .as_deref()
            .and_then(lifecycle_from_str),
        archived_at: r.get(at + 7)?,
    })
}

impl Store {
    /// Join `child` to `parent`'s series as the next part, first starting a
    /// series (untitled, parent = part 1) when `parent` isn't in one.
    pub fn continue_series(&self, parent: WorkspaceId, child: WorkspaceId) -> Result<SeriesJoin> {
        let tx = self.conn().unchecked_transaction()?;
        let ws_row = |id: WorkspaceId| {
            tx.query_row(
                "SELECT repo_id, name, branch FROM workspaces WHERE id = ?1",
                [id.0],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                },
            )
        };
        let (repo_id, p_slug, p_branch) = ws_row(parent)?;
        let (_, c_slug, c_branch) = ws_row(child)?;
        let insert_part = |series: i64, part: u32, id: WorkspaceId, slug: &str, branch: &str| {
            tx.execute(
                "INSERT INTO series_parts (series_id, part, workspace_id, slug, branch) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![series, part, id.0, slug, branch],
            )
        };
        let existing: Option<i64> = tx
            .query_row(
                "SELECT series_id FROM series_parts WHERE workspace_id = ?1",
                [parent.0],
                |r| r.get(0),
            )
            .optional()?;
        let started = existing.is_none();
        let series = match existing {
            Some(s) => s,
            None => {
                tx.execute(
                    "INSERT INTO series (repo_id, title, created_at) VALUES (?1, NULL, ?2)",
                    rusqlite::params![repo_id, crate::util::time::now_ms() / 1000],
                )?;
                let s = tx.last_insert_rowid();
                insert_part(s, 1, parent, &p_slug, &p_branch)?;
                s
            }
        };
        let part: u32 = tx.query_row(
            "SELECT COALESCE(MAX(part), 0) + 1 FROM series_parts WHERE series_id = ?1",
            [series],
            |r| r.get(0),
        )?;
        insert_part(series, part, child, &c_slug, &c_branch)?;
        let title: Option<String> =
            tx.query_row("SELECT title FROM series WHERE id = ?1", [series], |r| {
                r.get(0)
            })?;
        tx.commit()?;
        Ok(SeriesJoin {
            series_id: SeriesId(series),
            part,
            started,
            title,
        })
    }

    pub fn series_for_workspace(&self, id: WorkspaceId) -> Result<Option<Series>> {
        let series: Option<i64> = self
            .conn()
            .query_row(
                "SELECT series_id FROM series_parts WHERE workspace_id = ?1",
                [id.0],
                |r| r.get(0),
            )
            .optional()?;
        Ok(match series {
            Some(s) => self.load_series(Some(s))?.pop(),
            None => None,
        })
    }

    pub fn all_series(&self) -> Result<Vec<Series>> {
        self.load_series(None)
    }

    /// `None` (or blank) clears it, falling back to part 1's slug.
    pub fn set_series_title(&self, id: SeriesId, title: Option<&str>) -> Result<()> {
        let title = title.map(str::trim).filter(|t| !t.is_empty());
        self.conn().execute(
            "UPDATE series SET title = ?1 WHERE id = ?2",
            rusqlite::params![title, id.0],
        )?;
        Ok(())
    }

    /// Every series (or just `only`), parts ascending.
    fn load_series(&self, only: Option<i64>) -> Result<Vec<Series>> {
        let mut stmt = self.conn().prepare(&format!(
            "SELECT s.id, s.repo_id, s.title, {PART_COLUMNS} \
             FROM series s JOIN series_parts p ON p.series_id = s.id \
             WHERE ?1 IS NULL OR s.id = ?1 ORDER BY s.id, p.part"
        ))?;
        let rows = stmt.query_map([only], |r| {
            let head = (
                SeriesId(r.get(0)?),
                RepoId(r.get(1)?),
                r.get::<_, Option<String>>(2)?,
            );
            Ok((head, row_to_part(r, 3)?))
        })?;
        let mut out: Vec<Series> = Vec::new();
        for row in rows {
            let ((id, repo_id, title), part) = row?;
            match out.last_mut() {
                Some(s) if s.id == id => s.parts.push(part),
                _ => out.push(Series {
                    id,
                    repo_id,
                    title,
                    parts: vec![part],
                }),
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::store::{NewWorkspace, RepoId, Store, WorkspaceId};
    use crate::pty::session::AgentKind;
    use std::path::{Path, PathBuf};

    fn store_with_repo() -> (Store, RepoId) {
        let store = Store::open_in_memory().unwrap();
        let repo = store.add_repo(Path::new("/tmp/r"), "r", "x").unwrap();
        (store, repo)
    }

    fn ws(store: &Store, repo: RepoId, name: &str) -> WorkspaceId {
        store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name,
                branch: &format!("x/{name}"),
                worktree_path: &PathBuf::from(format!("/tmp/r/{name}")),
                yolo: false,
                agent: AgentKind::Claude,
                shared: false,
            })
            .unwrap()
    }

    fn parts(s: &Series) -> Vec<(u32, &str)> {
        s.parts.iter().map(|p| (p.part, p.slug.as_str())).collect()
    }

    #[test]
    fn continuing_a_loose_workspace_starts_a_series() {
        let (store, repo) = store_with_repo();
        let a = ws(&store, repo, "a");
        let b = ws(&store, repo, "b");
        let j = store.continue_series(a, b).unwrap();
        assert!(j.started);
        assert_eq!(j.part, 2);
        assert_eq!(j.title, None);
        let s = store.series_for_workspace(a).unwrap().unwrap();
        assert_eq!(parts(&s), vec![(1, "a"), (2, "b")]);
        assert_eq!(s.parts[1].branch, "x/b");
        assert_eq!(s.repo_id, repo);
        assert_eq!(s.display_title(), "a");
    }

    #[test]
    fn continuing_a_part_appends_and_branches_in_creation_order() {
        let (store, repo) = store_with_repo();
        let [a, b, c, d] = ["a", "b", "c", "d"].map(|n| ws(&store, repo, n));
        store.continue_series(a, b).unwrap();
        let jc = store.continue_series(b, c).unwrap();
        let jd = store.continue_series(b, d).unwrap();
        assert!(!jc.started);
        assert_eq!((jc.part, jd.part), (3, 4));
        let s = store.series_for_workspace(d).unwrap().unwrap();
        assert_eq!(parts(&s), vec![(1, "a"), (2, "b"), (3, "c"), (4, "d")]);
        assert_eq!(store.all_series().unwrap().len(), 1);
    }

    #[test]
    fn title_set_and_cleared() {
        let (store, repo) = store_with_repo();
        let a = ws(&store, repo, "a");
        let b = ws(&store, repo, "b");
        let id = store.continue_series(a, b).unwrap().series_id;
        store.set_series_title(id, Some("Focus view")).unwrap();
        let s = store.series_for_workspace(b).unwrap().unwrap();
        assert_eq!(s.title.as_deref(), Some("Focus view"));
        assert_eq!(s.display_title(), "Focus view");
        assert_eq!(
            store
                .continue_series(b, ws(&store, repo, "c"))
                .unwrap()
                .title
                .as_deref(),
            Some("Focus view")
        );
        store.set_series_title(id, None).unwrap();
        assert_eq!(
            store
                .series_for_workspace(b)
                .unwrap()
                .unwrap()
                .display_title(),
            "a"
        );
    }

    #[test]
    fn workspace_outside_a_series_has_none() {
        let (store, repo) = store_with_repo();
        let a = ws(&store, repo, "a");
        assert!(store.series_for_workspace(a).unwrap().is_none());
        assert!(store.all_series().unwrap().is_empty());
    }

    #[test]
    fn continue_series_unknown_workspace_errs() {
        let (store, repo) = store_with_repo();
        let a = ws(&store, repo, "a");
        assert!(store.continue_series(WorkspaceId(999), a).is_err());
        assert!(store.continue_series(a, WorkspaceId(999)).is_err());
        assert!(
            store.all_series().unwrap().is_empty(),
            "nothing half-written"
        );
    }

    #[test]
    fn migration_reaches_v31_with_series_tables() {
        let (store, _) = store_with_repo();
        let v: i64 = store
            .conn()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 31);
        store.migrate().unwrap();
        for t in ["series", "series_parts"] {
            let n: i64 = store
                .conn()
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [t],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "{t}");
        }
    }
}
