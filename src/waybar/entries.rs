//! Rich walker/elephant menu entries: `wsx waybar menu-entries --json`
//! emits display-ready rows; `wsx waybar refresh-prs` sweeps the PR cache.
//! See docs/superpowers/specs/2026-07-26-elephant-menu-design.md.

use crate::data::scm_cache::ScmCacheRow;
use crate::data::store::ReportedStatus;
use crate::data::store::Store;
use crate::error::Result;
use crate::git::forge::BranchLifecycle;
use crate::waybar::menu::sanitize;
use std::path::PathBuf;

/// Skip `gh` for a workspace whose PR state was fetched more recently than
/// this. Matches the spirit of the TUI's 30s in-memory throttle but is more
/// conservative: menu opens are burstier than TUI ticks.
pub const PR_REFRESH_THROTTLE_SECS: i64 = 120;

/// Max concurrent per-workspace git fact gathers at menu open (each runs
/// ~3 git subprocesses; unbounded fan-out would spike on large fleets).
const GIT_FACTS_CONCURRENCY: usize = 8;

const GLYPH_BRANCH: &str = "\u{e0a0}"; // powerline branch
const GLYPH_PR: &str = "\u{f407}"; // nf-oct-git_pull_request
const GLYPH_MERGED: &str = "\u{f419}"; // nf-oct-git_merge
const GLYPH_DIRTY: &str = "\u{25cf}"; // ●

#[derive(serde::Serialize, Debug, PartialEq)]
pub struct MenuEntry {
    pub text: String,
    pub subtext: String,
    pub icon: String,
    pub action: String,
}

pub(crate) fn needs_pr_refresh(fetched_at: Option<i64>, now: i64) -> bool {
    match fetched_at {
        None => true,
        Some(t) => now.saturating_sub(t) >= PR_REFRESH_THROTTLE_SECS,
    }
}

/// PR indicator, or None when there is no PR or the state was never fetched
/// (deliberately identical renderings — an unknown must not claim "no PR",
/// and "no PR" earns no glyph).
fn pr_segment(row: &ScmCacheRow) -> Option<String> {
    let lifecycle = row.pr_lifecycle?;
    let (glyph, suffix) = match lifecycle {
        BranchLifecycle::NoPr => return None,
        BranchLifecycle::PrOpen => (GLYPH_PR, None),
        BranchLifecycle::PrDraft => (GLYPH_PR, Some("draft")),
        BranchLifecycle::PrConflicted => (GLYPH_PR, Some("conflict")),
        BranchLifecycle::PrMerged => (GLYPH_MERGED, None),
        BranchLifecycle::PrClosed => (GLYPH_PR, Some("closed")),
    };
    let mut parts = vec![glyph.to_string()];
    if let Some(n) = row.pr_number {
        parts.push(format!("#{n}"));
    }
    if let Some(s) = suffix {
        parts.push(s.to_string());
    }
    Some(parts.join(" "))
}

pub(crate) fn compose_text(repo: &str, slug: &str, row: &ScmCacheRow) -> String {
    let mut parts = vec![format!("{}/{}", sanitize(repo), sanitize(slug))];
    if let Some(pr) = pr_segment(row) {
        parts.push(pr);
    }
    if row.dirty == Some(true) {
        parts.push(GLYPH_DIRTY.to_string());
    }
    if let (Some(a), Some(d)) = (row.additions, row.deletions) {
        if a + d > 0 {
            parts.push(format!("+{a} \u{2212}{d}"));
        }
    }
    parts.join("  ")
}

pub(crate) fn compose_subtext(branch: &str, status: Option<&ReportedStatus>) -> String {
    let b = format!("{GLYPH_BRANCH} {}", sanitize(branch));
    let Some(s) = status else {
        return b;
    };
    match s.message.as_deref().filter(|m| !m.is_empty()) {
        Some(m) => format!("{b} \u{2014} {}: {}", s.state.as_str(), sanitize(m)),
        None => format!("{b} \u{2014} {}", s.state.as_str()),
    }
}

fn quote(s: &str) -> String {
    shlex::try_quote(s)
        .map(|c| c.into_owned())
        // Only fails on interior NUL, which cannot survive sqlite TEXT
        // anyway; drop the offending byte rather than emit an unquoted arg.
        .unwrap_or_else(|_| format!("'{}'", s.replace(['\'', '\0'], "")))
}

pub(crate) fn action_cmd(wsx_bin: &str, repo: &str, slug: &str) -> String {
    format!(
        "{} waybar jump {} {}",
        quote(wsx_bin),
        quote(repo),
        quote(slug)
    )
}

pub(crate) struct RowInput {
    pub repo_name: String,
    pub slug: String,
    pub branch: String,
    pub status: Option<ReportedStatus>,
    pub cache: ScmCacheRow,
}

pub(crate) fn build_entries(rows: &[RowInput], wsx_bin: &str) -> Vec<MenuEntry> {
    rows.iter()
        .map(|r| MenuEntry {
            text: compose_text(&r.repo_name, &r.slug, &r.cache),
            subtext: compose_subtext(&r.branch, r.status.as_ref()),
            icon: crate::waybar::status::glyph(r.status.as_ref().map(|s| s.state)).to_string(),
            action: action_cmd(wsx_bin, &r.repo_name, &r.slug),
        })
        .collect()
}

/// Git facts for one worktree, or None if git fails (missing worktree, not a
/// repo, …) — the row then renders without dirty/diff indicators.
async fn gather_git_facts(worktree: PathBuf) -> Option<(bool, crate::git::DiffStats)> {
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
async fn collect_rows(store: &Store) -> Result<Vec<RowInput>> {
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
    for ((id, repo_name, slug, branch, _), fact) in metas.into_iter().zip(facts) {
        let mut cache = caches.remove(&id).unwrap_or_default();
        if let Some((dirty, stats)) = fact {
            cache.dirty = Some(dirty);
            cache.additions = Some(stats.added);
            cache.deletions = Some(stats.removed);
            let _ = store.upsert_scm_git(id, dirty, stats.added, stats.removed);
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
            status: statuses.get(&id).cloned(),
            cache,
        });
    }
    Ok(rows)
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Fire-and-forget `wsx waybar refresh-prs` so PR data self-heals by the
/// next menu open even when the TUI is not running.
fn spawn_pr_sweep(wsx_bin: &str) {
    use std::process::Stdio;
    let _ = std::process::Command::new(wsx_bin)
        .args(["waybar", "refresh-prs"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

pub async fn run_menu_entries(store: &Store) -> Result<()> {
    let rows = collect_rows(store).await?;
    let wsx_bin = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "wsx".into());
    let entries = build_entries(&rows, &wsx_bin);
    // Serialization of plain strings cannot fail.
    println!(
        "{}",
        serde_json::to_string(&entries).expect("serialize entries")
    );
    spawn_pr_sweep(&wsx_bin);
    Ok(())
}

/// Sequentially refresh PR state for every workspace outside the throttle
/// window. Silent by contract: improves the cache or does nothing.
pub async fn run_refresh_prs(store: &Store) -> Result<()> {
    let caches = store.all_scm_cache()?;
    for repo in crate::data::repo::list(store)? {
        for ws in store.workspaces(repo.id)? {
            let fetched = caches.get(&ws.id).and_then(|c| c.fetched_at);
            if !needs_pr_refresh(fetched, unix_now()) {
                continue;
            }
            if let Ok(Some(status)) =
                crate::git::forge::fetch_pr_status(&ws.worktree_path, &ws.branch).await
            {
                let _ = store.upsert_scm_pr(ws.id, status.lifecycle, status.number, unix_now());
            }
            // Err / Ok(None): leave cached state alone (transient failure
            // must not clobber a known lifecycle).
        }
    }
    Ok(())
}

#[cfg(test)]
mod entry_tests {
    use super::*;
    use crate::data::store::{ReportedState, ReportedStatus};

    fn status(state: ReportedState, msg: Option<&str>) -> ReportedStatus {
        ReportedStatus {
            state,
            message: msg.map(str::to_string),
            source: "test".into(),
            reported_at: 0,
        }
    }

    #[test]
    fn text_plain_when_cache_empty() {
        assert_eq!(
            compose_text("r", "w", &ScmCacheRow::default()),
            "r/w".to_string()
        );
    }

    #[test]
    fn text_with_all_indicators() {
        let row = ScmCacheRow {
            pr_lifecycle: Some(BranchLifecycle::PrOpen),
            pr_number: Some(123),
            dirty: Some(true),
            additions: Some(45),
            deletions: Some(12),
            fetched_at: Some(0),
        };
        let text = compose_text("workspacex", "fix-bug", &row);
        assert!(text.starts_with("workspacex/fix-bug"), "{text}");
        assert!(text.contains("#123"), "{text}");
        assert!(text.contains('\u{25cf}'), "{text}");
        assert!(text.contains("+45 \u{2212}12"), "{text}");
    }

    #[test]
    fn no_pr_and_unknown_render_identically() {
        let unknown = ScmCacheRow::default();
        let no_pr = ScmCacheRow {
            pr_lifecycle: Some(BranchLifecycle::NoPr),
            fetched_at: Some(0),
            ..Default::default()
        };
        assert_eq!(
            compose_text("r", "w", &unknown),
            compose_text("r", "w", &no_pr)
        );
    }

    #[test]
    fn draft_conflict_closed_carry_labels() {
        for (l, label) in [
            (BranchLifecycle::PrDraft, "draft"),
            (BranchLifecycle::PrConflicted, "conflict"),
            (BranchLifecycle::PrClosed, "closed"),
        ] {
            let row = ScmCacheRow {
                pr_lifecycle: Some(l),
                pr_number: Some(7),
                ..Default::default()
            };
            let text = compose_text("r", "w", &row);
            assert!(text.contains(label), "{l:?}: {text}");
            assert!(text.contains("#7"), "{l:?}: {text}");
        }
    }

    #[test]
    fn merged_uses_merge_glyph_without_label() {
        let row = ScmCacheRow {
            pr_lifecycle: Some(BranchLifecycle::PrMerged),
            pr_number: Some(9),
            ..Default::default()
        };
        let text = compose_text("r", "w", &row);
        assert!(text.contains('\u{f419}'), "{text}");
        assert!(!text.contains("merged"), "{text}");
    }

    #[test]
    fn clean_zero_diff_shows_no_noise() {
        let row = ScmCacheRow {
            dirty: Some(false),
            additions: Some(0),
            deletions: Some(0),
            ..Default::default()
        };
        assert_eq!(compose_text("r", "w", &row), "r/w");
    }

    #[test]
    fn subtext_variants() {
        assert_eq!(compose_subtext("x/w", None), "\u{e0a0} x/w".to_string());
        assert_eq!(
            compose_subtext("x/w", Some(&status(ReportedState::Working, Some("fixing")))),
            "\u{e0a0} x/w \u{2014} working: fixing".to_string()
        );
        assert_eq!(
            compose_subtext("x/w", Some(&status(ReportedState::Done, None))),
            "\u{e0a0} x/w \u{2014} done".to_string()
        );
    }

    #[test]
    fn subtext_sanitizes_newlines() {
        let s = compose_subtext("x/w", Some(&status(ReportedState::Working, Some("a\nb"))));
        assert!(!s.contains('\n'), "{s}");
    }

    #[test]
    fn action_cmd_quotes_spacey_repo() {
        let cmd = action_cmd("/usr/bin/wsx", "meals backend", "api-fix");
        assert_eq!(cmd, "/usr/bin/wsx waybar jump 'meals backend' api-fix");
    }

    #[test]
    fn throttle_decision() {
        assert!(needs_pr_refresh(None, 1000));
        assert!(needs_pr_refresh(Some(880), 1000));
        assert!(!needs_pr_refresh(Some(881), 1000));
        assert!(!needs_pr_refresh(Some(2000), 1000)); // clock skew: don't refetch
    }

    #[test]
    fn menu_entry_serializes_with_lowercase_keys() {
        let e = MenuEntry {
            text: "t".into(),
            subtext: "s".into(),
            icon: "i".into(),
            action: "a".into(),
        };
        let v = serde_json::to_value([e]).unwrap();
        assert_eq!(v[0]["text"], "t");
        assert_eq!(v[0]["subtext"], "s");
        assert_eq!(v[0]["icon"], "i");
        assert_eq!(v[0]["action"], "a");
    }

    #[tokio::test]
    async fn collect_rows_sorted_and_composed() {
        use crate::data::store::{NewWorkspace, Store};
        use crate::pty::session::AgentKind;

        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "x")
            .unwrap();
        let mut ids = vec![];
        for name in ["zeta", "alpha"] {
            ids.push(
                store
                    .insert_workspace(&NewWorkspace {
                        repo_id: repo,
                        name,
                        branch: &format!("x/{name}"),
                        worktree_path: &std::path::PathBuf::from(format!("/nonexistent/r/{name}")),
                        yolo: false,
                        agent: AgentKind::Claude,
                        shared: false,
                    })
                    .unwrap(),
            );
        }
        store
            .upsert_scm_pr(
                ids[0],
                crate::git::forge::BranchLifecycle::PrOpen,
                Some(5),
                0,
            )
            .unwrap();
        // Pre-seed zeta (ids[0]) with stale dirty/diff indicators: when
        // git fails on its nonexistent worktree, these must be suppressed
        // in-memory (not persisted to DB), so the row renders without ● and
        // +N −N but keeps its PR indicator (#5).
        store.upsert_scm_git(ids[0], true, 4, 2).unwrap();

        let rows = super::collect_rows(&store).await.unwrap();
        let entries = super::build_entries(&rows, "/bin/wsx");

        assert_eq!(entries.len(), 2);
        // Sorted by workspace name within repo.
        assert!(entries[0].text.starts_with("r/alpha"), "{:?}", entries[0]);
        assert!(entries[1].text.starts_with("r/zeta"), "{:?}", entries[1]);
        // Branch always present in subtext.
        assert!(entries[0].subtext.contains("x/alpha"), "{:?}", entries[0]);
        assert_eq!(entries[0].action, "/bin/wsx waybar jump r alpha");
        // zeta (entries[1]) carries the cached PR indicator #5 even though
        // its worktree is missing. Stale dirty/diff indicators are suppressed
        // when git fails: text contains #5 but NOT ● or +4.
        assert!(entries[1].text.contains("#5"), "{:?}", entries[1]);
        assert!(
            !entries[1].text.contains('\u{25cf}'),
            "stale dirty indicator should be suppressed: {:?}",
            entries[1]
        );
        assert!(
            !entries[1].text.contains("+4"),
            "stale diff indicator should be suppressed: {:?}",
            entries[1]
        );
    }
}
