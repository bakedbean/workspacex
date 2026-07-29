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

/// Rows sorted by repo name then workspace name (parity with the dmenu
/// picker). Git-local facts are gathered concurrently across workspaces and
/// written through to `scm_cache`; PR fields are read from cache only.
pub async fn collect_rows_fresh(store: &Store) -> Result<Vec<RowInput>> {
    let statuses = store.all_workspace_status()?;
    let mut caches = store.all_scm_cache()?;
    let mut repos = crate::data::repo::list(store)?;
    repos.sort_by(|a, b| a.name.cmp(&b.name));

    let mut metas = Vec::new();
    for repo in &repos {
        let mut workspaces = store.workspaces(repo.id)?;
        workspaces.sort_by(|a, b| a.name.cmp(&b.name));
        for ws in workspaces {
            metas.push((
                ws.id,
                repo.name.clone(),
                ws.name,
                ws.branch,
                ws.worktree_path,
            ));
        }
    }

    // Bounded fan-out: ~3 git subprocesses per workspace, so unbounded
    // join_all would spike process count on large fleets. `buffered` keeps
    // results in input order for the zip below.
    use futures::StreamExt;
    let facts: Vec<_> = futures::stream::iter(
        metas
            .iter()
            .map(|(_, _, _, _, worktree)| gather_git_facts(worktree.clone())),
    )
    .buffered(GIT_FACTS_CONCURRENCY)
    .collect()
    .await;

    let mut rows = Vec::with_capacity(metas.len());
    for ((id, repo_name, slug, branch, worktree_path), fact) in metas.into_iter().zip(facts) {
        let mut cache = caches.remove(&id).unwrap_or_default();
        if let Some((dirty, stats)) = fact {
            cache.dirty = Some(dirty);
            cache.additions = Some(stats.added);
            cache.deletions = Some(stats.removed);
            let _ = store.upsert_scm_git(id, dirty, stats.added, stats.removed, unix_now());
        } else {
            // Git failed (missing worktree, not a repo, etc.): suppress stale
            // indicators in-memory while preserving cached PR state.
            cache.dirty = None;
            cache.additions = None;
            cache.deletions = None;
        }
        rows.push(RowInput {
            repo_name,
            slug,
            branch,
            worktree_path,
            status: statuses.get(&id).cloned(),
            cache,
        });
    }
    Ok(rows)
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
}
