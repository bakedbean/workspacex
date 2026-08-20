#![allow(clippy::arc_with_non_send_sync, clippy::collapsible_if)]

use std::process::Command as StdCmd;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::Mutex;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn branch_rename_propagates_to_store() {
    // Set up a real git repo + worktree, manually run `git branch -m`,
    // then assert the poller picks it up within ~5s — and that it drops the
    // PR state the old branch left in scm_cache.
    let repo_dir = TempDir::new().unwrap();
    let r = |args: &[&str]| {
        assert!(
            StdCmd::new("git")
                .current_dir(repo_dir.path())
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    };
    r(&["init", "-q", "-b", "main"]);
    r(&["config", "user.email", "t@e"]);
    r(&["config", "user.name", "t"]);
    r(&["commit", "--allow-empty", "-q", "-m", "init"]);

    let store = wsx::data::store::Store::open_in_memory().unwrap();
    let repo_id = wsx::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
        .await
        .unwrap();
    let repo = store
        .repos()
        .unwrap()
        .into_iter()
        .find(|r| r.id == repo_id)
        .unwrap();
    let base = TempDir::new().unwrap();
    let created = wsx::data::workspace::create(
        &store,
        &repo,
        Some("placeholder"),
        base.path(),
        false,
        false,
        wsx::pty::session::AgentKind::Claude,
        tokio_util::sync::CancellationToken::new(),
        |_| {},
    )
    .await
    .unwrap();

    // Seed the cache the way a poll against the OLD branch would have: an
    // approved, open PR. The cache-only surfaces (`wsx waybar menu-entries`,
    // `wsx menubar plugin`) render straight from this row, so if drift
    // leaves it behind they show the old branch's ✓ under the new branch's
    // name.
    store
        .upsert_scm_pr(
            created.workspace.id,
            &wsx::git::forge::PrStatus {
                lifecycle: wsx::git::forge::BranchLifecycle::PrOpen,
                number: Some(7),
                url: Some("https://github.com/o/r/pull/7".into()),
                review: Some(wsx::git::forge::ReviewDecision::Approved),
            },
            1_000,
        )
        .unwrap();

    let app = Arc::new(Mutex::new(
        wsx::app::App::new(store, base.path().to_path_buf()).unwrap(),
    ));
    let poll = tokio::spawn(wsx::app::branch_drift_poll(app.clone()));

    // Simulate claude renaming the branch via git directly.
    let wt = &created.workspace.worktree_path;
    let s = StdCmd::new("git")
        .current_dir(wt)
        .args(["branch", "-m", "wsx/placeholder", "wsx/new-name"])
        .status()
        .unwrap();
    assert!(s.success());

    // Wait up to 5s for the poller to notice.
    let mut renamed = false;
    for _ in 0..25 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let g = app.lock().await;
        if let Some((_, w)) = g
            .workspaces
            .iter()
            .find(|(_, w)| w.id == created.workspace.id)
        {
            if w.name == "new-name" && w.branch == "wsx/new-name" {
                renamed = true;
                break;
            }
        }
    }
    // Read the cache while the drift lock is still ours: the poller clears
    // the row inside the same locked block that performs the rename, so
    // observing the rename means the clear has already landed.
    let cached = {
        let g = app.lock().await;
        g.store
            .all_scm_cache()
            .unwrap()
            .get(&created.workspace.id)
            .cloned()
    };
    poll.abort();
    assert!(renamed, "poller did not pick up the rename within 5s");

    // The verdict is the assertion that matters and the one that can't drift
    // back: this temp repo has no remote, so the PR poll later in the same
    // iteration can only produce "no PR" or a `gh` failure, neither of which
    // can resurrect an approval.
    if let Some(row) = cached {
        assert_eq!(
            row.pr_review, None,
            "branch drift must drop the old branch's approval from scm_cache"
        );
        assert_eq!(row.pr_number, None, "and its PR number");
        assert_eq!(row.pr_url, None, "and its PR url");
    }
}
