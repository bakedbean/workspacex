#![allow(clippy::arc_with_non_send_sync)]

use std::process::Command as StdCmd;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::Mutex;

use ratatui::Terminal;
use ratatui::backend::TestBackend;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dashboard_renders_with_one_repo_one_workspace() {
    unsafe {
        std::env::set_var("WSX_CLAUDE_BIN", wsx::test_support::cat_path());
    }

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

    let store = wsx::store::Store::open_in_memory().unwrap();
    let repo_id = wsx::repo::add(&store, repo_dir.path(), "demo", "wsx")
        .await
        .unwrap();
    let repo = store
        .repos()
        .unwrap()
        .into_iter()
        .find(|r| r.id == repo_id)
        .unwrap();
    let base = TempDir::new().unwrap();
    wsx::workspace::create(
        &store,
        &repo,
        Some("alpha"),
        base.path(),
        false,
        wsx::pty::session::AgentKind::Claude,
        tokio_util::sync::CancellationToken::new(),
        |_| {},
    )
    .await
    .unwrap();

    let app = Arc::new(Mutex::new(
        wsx::app::App::new(store, base.path().to_path_buf()).unwrap(),
    ));
    let backend = TestBackend::new(80, 10);
    let mut term = Terminal::new(backend).unwrap();

    // Single draw — we just want to verify the dashboard renders without panicking
    // and shows the workspace we created. V5's `default_fold` heuristic
    // auto-collapses repos whose workspaces have no live activity, which
    // hides the workspace row from a freshly-created repo. Force-expand
    // so the assertion below sees both the repo and the workspace.
    {
        let mut g = app.lock().await;
        g.dashboard.folded.insert(repo_id.0 as u64, false);
        term.draw(|f| wsx::app::draw_for_test(f, &mut g)).unwrap();
    }
    let buf = term.backend().buffer();
    // Replace the existing line-by-line scan with a substring check on the
    // combined text (any layout that shows both "demo" and "alpha" passes).
    let mut all_text = String::new();
    for y in 0..10 {
        let line: String = (0..80).map(|x| buf[(x, y)].symbol().to_string()).collect();
        all_text.push_str(&line);
        all_text.push('\n');
    }
    assert!(
        all_text.contains("demo") && all_text.contains("alpha"),
        "dashboard did not show demo and alpha:\n{all_text}"
    );

    unsafe {
        std::env::remove_var("WSX_CLAUDE_BIN");
    }
}
