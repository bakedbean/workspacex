//! Platform-neutral workspace row collection shared by the Linux waybar and
//! macOS menubar integrations.

use crate::data::scm_cache::ScmCacheRow;
use crate::data::store::ReportedState;
use crate::data::store::ReportedStatus;
use crate::data::store::Store;
use crate::error::Result;
use std::path::PathBuf;

/// Skip `gh` for a workspace whose PR state was fetched more recently than
/// this. Matches the spirit of the TUI's 30s in-memory throttle but is more
/// conservative: menu opens are burstier than TUI ticks.
pub const PR_REFRESH_THROTTLE_SECS: i64 = 120;

/// Skip re-gathering local git facts (dirty/added/removed) for a workspace
/// whose facts were computed more recently than this.
pub const GIT_REFRESH_THROTTLE_SECS: i64 = 60;

/// Max concurrent per-workspace git fact gathers at menu open (each runs
/// ~3 git subprocesses; unbounded fan-out would spike on large fleets).
pub(crate) const GIT_FACTS_CONCURRENCY: usize = 8;

/// True when `fetched_at` is missing or older than `throttle_secs`.
/// Future timestamps (clock skew) count as fresh — don't refetch.
pub fn is_stale(fetched_at: Option<i64>, now: i64, throttle_secs: i64) -> bool {
    match fetched_at {
        None => true,
        Some(t) => now.saturating_sub(t) >= throttle_secs,
    }
}

/// Collapse control characters (incl. '\n', '\t') to a single space so a
/// value with embedded newlines can't inject fake rows into the picker.
pub fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// Rank used to pick the "worst" state across a fleet — higher wins.
pub fn attention_rank(state: ReportedState) -> u8 {
    match state {
        ReportedState::Blocked => 4,
        ReportedState::Done => 3,
        ReportedState::Waiting => 2,
        ReportedState::Working | ReportedState::Busy => 1,
    }
}

pub fn state_glyph(state: Option<ReportedState>) -> &'static str {
    match state {
        Some(ReportedState::Blocked) => "!",
        Some(ReportedState::Done) => "\u{2713}",
        Some(ReportedState::Waiting) => "\u{2026}",
        Some(ReportedState::Working | ReportedState::Busy) => "\u{21bb}",
        None => "\u{b7}",
    }
}

pub struct RowInput {
    pub id: crate::data::store::WorkspaceId,
    pub repo_name: String,
    pub slug: String,
    pub branch: String,
    pub worktree_path: PathBuf,
    pub status: Option<ReportedStatus>,
    pub cache: ScmCacheRow,
}

/// Git facts for one worktree, or None if git fails (missing worktree, not a
/// repo, …) — the row then renders without dirty/diff indicators.
pub(crate) async fn gather_git_facts(worktree: PathBuf) -> Option<(bool, crate::git::DiffStats)> {
    let st = crate::git::workspace_status(&worktree).await.ok()?;
    let dirty = st.modified > 0 || st.untracked > 0;
    let base = crate::git::resolve_base_branch(&worktree).await;
    let stats = crate::git::workspace_diff_stats(&worktree, &base)
        .await
        .unwrap_or(crate::git::DiffStats {
            added: 0,
            removed: 0,
        });
    Some((dirty, stats))
}

struct WsMeta {
    id: crate::data::store::WorkspaceId,
    repo_name: String,
    slug: String,
    branch: String,
    worktree_path: PathBuf,
}

/// Workspace metadata sorted by repo name then workspace name.
fn workspace_metas(store: &Store) -> Result<Vec<WsMeta>> {
    let mut repos = crate::data::repo::list(store)?;
    repos.sort_by(|a, b| a.name.cmp(&b.name));
    let mut metas = Vec::new();
    for repo in &repos {
        let mut workspaces = store.workspaces(repo.id)?;
        workspaces.sort_by(|a, b| a.name.cmp(&b.name));
        for ws in workspaces {
            metas.push(WsMeta {
                id: ws.id,
                repo_name: repo.name.clone(),
                slug: ws.name,
                branch: ws.branch,
                worktree_path: ws.worktree_path,
            });
        }
    }
    Ok(metas)
}

/// Cache-only rows: store + scm_cache reads, zero subprocesses. The
/// menubar plugin renders from this on every poll.
pub fn collect_rows_cached(store: &Store) -> Result<Vec<RowInput>> {
    let statuses = store.all_workspace_status()?;
    let mut caches = store.all_scm_cache()?;
    Ok(workspace_metas(store)?
        .into_iter()
        .map(|m| RowInput {
            id: m.id,
            status: statuses.get(&m.id).cloned(),
            cache: caches.remove(&m.id).unwrap_or_default(),
            repo_name: m.repo_name,
            slug: m.slug,
            branch: m.branch,
            worktree_path: m.worktree_path,
        })
        .collect())
}

/// Rows sorted by repo name then workspace name (parity with the dmenu
/// picker). Git-local facts are gathered concurrently across workspaces and
/// written through to `scm_cache`; PR fields are read from cache only.
pub async fn collect_rows_fresh(store: &Store) -> Result<Vec<RowInput>> {
    let statuses = store.all_workspace_status()?;
    let mut caches = store.all_scm_cache()?;
    let metas = workspace_metas(store)?;

    // Bounded fan-out: ~3 git subprocesses per workspace, so unbounded
    // join_all would spike process count on large fleets. `buffered` keeps
    // results in input order for the zip below.
    use futures::StreamExt;
    let facts: Vec<_> = futures::stream::iter(
        metas
            .iter()
            .map(|m| gather_git_facts(m.worktree_path.clone())),
    )
    .buffered(GIT_FACTS_CONCURRENCY)
    .collect()
    .await;

    let mut rows = Vec::with_capacity(metas.len());
    for (m, fact) in metas.into_iter().zip(facts) {
        let mut cache = caches.remove(&m.id).unwrap_or_default();
        if let Some((dirty, stats)) = fact {
            cache.dirty = Some(dirty);
            cache.additions = Some(stats.added);
            cache.deletions = Some(stats.removed);
            let _ = store.upsert_scm_git(m.id, dirty, stats.added, stats.removed, unix_now());
        } else {
            // Git failed (missing worktree, not a repo, etc.): suppress stale
            // indicators in-memory while preserving cached PR state.
            cache.dirty = None;
            cache.additions = None;
            cache.deletions = None;
        }
        rows.push(RowInput {
            id: m.id,
            repo_name: m.repo_name,
            slug: m.slug,
            branch: m.branch,
            worktree_path: m.worktree_path,
            status: statuses.get(&m.id).cloned(),
            cache,
        });
    }
    Ok(rows)
}

/// Recompute git facts for every workspace whose git_fetched_at is stale.
/// Bounded fan-out; git failure clears the row's git fields (a stale ● is
/// worse than none). Silent by contract, like run_refresh_prs.
pub async fn refresh_git_facts(store: &Store) -> Result<()> {
    let now = unix_now();
    let caches = store.all_scm_cache()?;
    let targets: Vec<WsMeta> = workspace_metas(store)?
        .into_iter()
        .filter(|m| {
            caches
                .get(&m.id)
                .is_none_or(|c| is_stale(c.git_fetched_at, now, GIT_REFRESH_THROTTLE_SECS))
        })
        .collect();
    use futures::StreamExt;
    let facts: Vec<_> = futures::stream::iter(
        targets
            .iter()
            .map(|m| gather_git_facts(m.worktree_path.clone())),
    )
    .buffered(GIT_FACTS_CONCURRENCY)
    .collect()
    .await;
    for (m, fact) in targets.into_iter().zip(facts) {
        match fact {
            Some((dirty, stats)) => {
                let _ = store.upsert_scm_git(m.id, dirty, stats.added, stats.removed, now);
            }
            None => {
                let _ = store.clear_scm_git(m.id, now);
            }
        }
    }
    Ok(())
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Sequentially refresh PR state for every workspace outside the throttle
/// window. Silent by contract: improves the cache or does nothing.
pub async fn run_refresh_prs(store: &Store) -> Result<()> {
    let caches = store.all_scm_cache()?;
    for repo in crate::data::repo::list(store)? {
        for ws in store.workspaces(repo.id)? {
            let fetched = caches.get(&ws.id).and_then(|c| c.fetched_at);
            if !is_stale(fetched, unix_now(), PR_REFRESH_THROTTLE_SECS) {
                continue;
            }
            if let Ok(Some(status)) =
                crate::git::forge::fetch_pr_status(&ws.worktree_path, &ws.branch).await
            {
                let _ = store.upsert_scm_pr(
                    ws.id,
                    status.lifecycle,
                    status.number,
                    status.url.as_deref(),
                    unix_now(),
                );
            }
            // Err / Ok(None): leave cached state alone (transient failure
            // must not clobber a known lifecycle).
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staleness_decision() {
        assert!(is_stale(None, 1000, 120));
        assert!(is_stale(Some(880), 1000, 120));
        assert!(!is_stale(Some(881), 1000, 120));
        assert!(!is_stale(Some(2000), 1000, 120)); // clock skew: don't refetch
    }

    #[tokio::test]
    async fn cached_collect_reads_cache_without_git() {
        use crate::data::store::{NewWorkspace, Store};
        use crate::pty::session::AgentKind;

        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "x")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "w",
                branch: "x/w",
                // Nonexistent on purpose: cached collect must not care.
                worktree_path: &std::path::PathBuf::from("/nonexistent/r/w"),
                yolo: false,
                agent: AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        store.upsert_scm_git(id, true, 4, 2, 1000).unwrap();

        let rows = collect_rows_cached(&store).unwrap();
        assert_eq!(rows.len(), 1);
        // The id is what lets callers join per-workspace tables (recaps,
        // status) onto a row.
        assert_eq!(rows[0].id, id);
        // Cache values pass through untouched — fresh mode would have
        // suppressed them because git fails on the missing worktree.
        assert_eq!(rows[0].cache.dirty, Some(true));
        assert_eq!(rows[0].cache.additions, Some(4));
        assert_eq!(
            rows[0].worktree_path.display().to_string(),
            "/nonexistent/r/w"
        );
    }

    #[tokio::test]
    async fn refresh_git_facts_clears_failed_and_skips_fresh() {
        use crate::data::store::{NewWorkspace, Store};
        use crate::pty::session::AgentKind;

        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "x")
            .unwrap();
        let mut ids = vec![];
        for name in ["stale", "fresh"] {
            ids.push(
                store
                    .insert_workspace(&NewWorkspace {
                        repo_id: repo,
                        name,
                        branch: &format!("x/{name}"),
                        worktree_path: &std::path::PathBuf::from(format!("/nonexistent/{name}")),
                        yolo: false,
                        agent: AgentKind::Claude,
                        shared: false,
                    })
                    .unwrap(),
            );
        }
        // stale: git_fetched_at long ago → swept; git fails → cleared, restamped.
        store.upsert_scm_git(ids[0], true, 4, 2, 0).unwrap();
        // fresh: stamped now → skipped, indicators survive even though the
        // worktree is equally nonexistent.
        store
            .upsert_scm_git(ids[1], true, 9, 9, unix_now())
            .unwrap();

        refresh_git_facts(&store).await.unwrap();

        let caches = store.all_scm_cache().unwrap();
        let stale = caches[&ids[0]].clone();
        assert_eq!(stale.dirty, None, "failed git must clear stale indicators");
        assert!(stale.git_fetched_at.unwrap() > 0, "sweep must restamp");
        let fresh = caches[&ids[1]].clone();
        assert_eq!(fresh.dirty, Some(true), "fresh row must not be swept");
        assert_eq!(fresh.additions, Some(9));
    }
}
