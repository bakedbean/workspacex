//! Dashboard navigation: selection, repo folds and ordering, detail bar.

use super::*;
use crate::data::store::Store;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::path::PathBuf;
// `dashboard_renders_split_with_pm_title_when_visible_even_without_session`
// (the PTY-placeholder render test) is gone — the dashboard's PM pane
// now always renders the digest (`render_digest`), whose own render
// tests live in `src/ui/pm_pane.rs::digest_tests`.

use super::common::*;
use crossterm::event::{KeyEvent, KeyModifiers};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dashboard_down_at_last_entry_wraps_to_first() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.selectable = vec![
        SelectionTarget::Repo(crate::data::store::RepoId(1)),
        SelectionTarget::Repo(crate::data::store::RepoId(2)),
        SelectionTarget::Repo(crate::data::store::RepoId(3)),
    ];
    app.select_index(2);
    handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .await
        .unwrap();
    assert_eq!(
        app.dashboard.selected, 0,
        "Down at last should wrap to first"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dashboard_up_at_first_entry_wraps_to_last() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.selectable = vec![
        SelectionTarget::Repo(crate::data::store::RepoId(1)),
        SelectionTarget::Repo(crate::data::store::RepoId(2)),
        SelectionTarget::Repo(crate::data::store::RepoId(3)),
    ];
    app.select_index(0);
    handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
        .await
        .unwrap();
    assert_eq!(app.dashboard.selected, 2, "Up at first should wrap to last");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dashboard_down_in_middle_advances_normally() {
    // Sanity check that wrap-around didn't break the non-edge case.
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.selectable = vec![
        SelectionTarget::Repo(crate::data::store::RepoId(1)),
        SelectionTarget::Repo(crate::data::store::RepoId(2)),
        SelectionTarget::Repo(crate::data::store::RepoId(3)),
    ];
    app.select_index(1);
    handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .await
        .unwrap();
    assert_eq!(app.dashboard.selected, 2);
}

#[tokio::test]
async fn j_alias_advances_selection_like_down() {
    let (mut app, _) = make_app_with_n_repos(3);
    app.select_index(0);
    press(&mut app, 'j', KeyModifiers::NONE).await;
    assert_eq!(app.dashboard.selected, 1, "j should advance like Down");
}

#[tokio::test]
async fn k_alias_retreats_selection_like_up() {
    let (mut app, _) = make_app_with_n_repos(3);
    app.select_index(2);
    press(&mut app, 'k', KeyModifiers::NONE).await;
    assert_eq!(app.dashboard.selected, 1, "k should retreat like Up");
}

#[tokio::test]
async fn k_does_not_open_process_list_anymore() {
    // `k` is now a nav alias for Up. Process list must be opened by `K`.
    let (mut app, _) = make_app_with_n_repos(1);
    app.select_index(0);
    press(&mut app, 'k', KeyModifiers::NONE).await;
    assert!(
        app.modal.is_none(),
        "k must not open ProcessList; it's now a nav alias"
    );
}

#[tokio::test]
async fn shift_k_opens_process_list_on_workspace() {
    use crate::data::store::{NewWorkspace, WorkspaceState};
    let (mut app, ids) = make_app_with_n_repos(1);
    let ws_id = app
        .store
        .insert_workspace(&NewWorkspace {
            repo_id: ids[0],
            name: "alpha",
            branch: "repo-0/alpha",
            worktree_path: std::path::Path::new("/tmp/wsx-test/alpha"),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    app.store
        .set_workspace_state(ws_id, WorkspaceState::Ready)
        .unwrap();
    app.refresh().unwrap();
    // Find and select the workspace row.
    let idx = app
        .selectable
        .iter()
        .position(|t| matches!(t, SelectionTarget::Workspace(id) if *id == ws_id))
        .expect("workspace should appear in selectable list");
    app.select_index(idx);
    press(&mut app, 'K', KeyModifiers::SHIFT).await;
    assert!(
        matches!(app.modal, Some(Modal::ProcessList { workspace_id, .. }) if workspace_id == ws_id),
        "K on a workspace row should open ProcessList"
    );
}

#[tokio::test]
async fn shift_k_moves_selected_repo_up() {
    let (mut app, ids) = make_app_with_n_repos(3);
    app.select_index(1); // select repo-1 (Repo header)
    press(&mut app, 'K', KeyModifiers::SHIFT).await;

    let order: Vec<_> = app.repos.iter().map(|r| r.id).collect();
    assert_eq!(
        order,
        vec![ids[1], ids[0], ids[2]],
        "repo-1 moved above repo-0"
    );
    assert_eq!(
        app.selected_target(),
        Some(SelectionTarget::Repo(ids[1])),
        "selection follows the moved repo"
    );
}

#[tokio::test]
async fn shift_j_moves_selected_repo_down() {
    let (mut app, ids) = make_app_with_n_repos(3);
    app.select_index(1); // select repo-1
    press(&mut app, 'J', KeyModifiers::SHIFT).await;

    let order: Vec<_> = app.repos.iter().map(|r| r.id).collect();
    assert_eq!(
        order,
        vec![ids[0], ids[2], ids[1]],
        "repo-1 moved below repo-2"
    );
    assert_eq!(app.selected_target(), Some(SelectionTarget::Repo(ids[1])));
}

#[tokio::test]
async fn shift_k_at_top_is_noop() {
    let (mut app, ids) = make_app_with_n_repos(3);
    app.select_index(0); // top repo
    press(&mut app, 'K', KeyModifiers::SHIFT).await;
    let order: Vec<_> = app.repos.iter().map(|r| r.id).collect();
    assert_eq!(
        order,
        vec![ids[0], ids[1], ids[2]],
        "no movement at the top"
    );
}

#[tokio::test]
async fn shift_j_at_bottom_is_noop() {
    let (mut app, ids) = make_app_with_n_repos(3);
    app.select_index(2); // bottom repo
    press(&mut app, 'J', KeyModifiers::SHIFT).await;
    let order: Vec<_> = app.repos.iter().map(|r| r.id).collect();
    assert_eq!(
        order,
        vec![ids[0], ids[1], ids[2]],
        "no movement at the bottom"
    );
}

#[tokio::test]
async fn shift_j_repeated_walks_repo_and_selection_follows() {
    let (mut app, ids) = make_app_with_n_repos(3);
    app.select_index(0); // select repo-0 (top)
    // Walk it down twice: [0,1,2] -> [1,0,2] -> [1,2,0].
    press(&mut app, 'J', KeyModifiers::SHIFT).await;
    assert_eq!(
        app.repos.iter().map(|r| r.id).collect::<Vec<_>>(),
        vec![ids[1], ids[0], ids[2]],
        "after first J: repo-0 moved to middle"
    );
    assert_eq!(
        app.selected_target(),
        Some(SelectionTarget::Repo(ids[0])),
        "selection still on the moved repo after first J"
    );
    press(&mut app, 'J', KeyModifiers::SHIFT).await;
    assert_eq!(
        app.repos.iter().map(|r| r.id).collect::<Vec<_>>(),
        vec![ids[1], ids[2], ids[0]],
        "after second J: repo-0 walked to the bottom"
    );
    assert_eq!(
        app.selected_target(),
        Some(SelectionTarget::Repo(ids[0])),
        "selection tracked the repo across both moves"
    );
    // A third J at the bottom is a no-op.
    press(&mut app, 'J', KeyModifiers::SHIFT).await;
    assert_eq!(
        app.repos.iter().map(|r| r.id).collect::<Vec<_>>(),
        vec![ids[1], ids[2], ids[0]],
        "third J at bottom does nothing"
    );
}

#[tokio::test]
async fn shift_j_on_workspace_is_noop_for_order() {
    use crate::data::store::{NewWorkspace, WorkspaceState};
    let (mut app, ids) = make_app_with_n_repos(2);
    // Add a workspace to repo-0 so there is a Workspace entry in selectable.
    let ws_id = app
        .store
        .insert_workspace(&NewWorkspace {
            repo_id: ids[0],
            name: "ws-alpha",
            branch: "repo-0/ws-alpha",
            worktree_path: std::path::Path::new("/tmp/wsx-test/ws-alpha"),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    app.store
        .set_workspace_state(ws_id, WorkspaceState::Ready)
        .unwrap();
    app.refresh().unwrap();
    // Find the workspace row in selectable and select it.
    let idx = app
        .selectable
        .iter()
        .position(|t| matches!(t, SelectionTarget::Workspace(id) if *id == ws_id))
        .expect("workspace should appear in selectable list");
    app.select_index(idx);
    // Capture repo order before pressing Shift+J.
    let order_before: Vec<_> = app.repos.iter().map(|r| r.id).collect();
    press(&mut app, 'J', KeyModifiers::SHIFT).await;
    let order_after: Vec<_> = app.repos.iter().map(|r| r.id).collect();
    assert_eq!(
        order_before, order_after,
        "Shift+J on a workspace row must not reorder repos"
    );
}

#[tokio::test]
async fn h_folds_focused_repo() {
    let (mut app, ids) = make_app_with_n_repos(2);
    app.select_index(0);
    // Start expanded so we can observe the fold.
    app.dashboard.folded.insert(ids[0].0 as u64, false);
    press(&mut app, 'h', KeyModifiers::NONE).await;
    assert_eq!(
        app.dashboard.folded.get(&(ids[0].0 as u64)).copied(),
        Some(true),
        "h should fold the focused repo"
    );
}

#[tokio::test]
async fn l_unfolds_focused_repo() {
    let (mut app, ids) = make_app_with_n_repos(2);
    app.select_index(0);
    app.dashboard.folded.insert(ids[0].0 as u64, true);
    press(&mut app, 'l', KeyModifiers::NONE).await;
    assert_eq!(
        app.dashboard.folded.get(&(ids[0].0 as u64)).copied(),
        Some(false),
        "l should unfold the focused repo"
    );
}

#[tokio::test]
async fn h_is_idempotent_on_already_folded_repo() {
    // Unlike `zz`, `h` should not toggle — pressing it twice keeps the
    // repo folded. This is the behavior that lets you mash `h` while
    // navigating without accidentally re-opening a row.
    let (mut app, ids) = make_app_with_n_repos(2);
    app.select_index(0);
    app.dashboard.folded.insert(ids[0].0 as u64, true);
    press(&mut app, 'h', KeyModifiers::NONE).await;
    press(&mut app, 'h', KeyModifiers::NONE).await;
    assert_eq!(
        app.dashboard.folded.get(&(ids[0].0 as u64)).copied(),
        Some(true),
        "h on an already-folded repo must stay folded"
    );
}

#[tokio::test]
async fn a_alone_is_no_op_on_dashboard() {
    let (mut app, _) = make_app_with_n_repos(2);
    let folded_before = app.dashboard.folded.clone();
    press(&mut app, 'a', KeyModifiers::NONE).await;
    assert!(!app.z_leader_pending, "a alone should not arm the leader");
    assert_eq!(
        app.dashboard.folded, folded_before,
        "a alone should not change fold state"
    );
}

#[tokio::test]
async fn shift_m_alone_is_no_op_on_dashboard() {
    let (mut app, _) = make_app_with_n_repos(2);
    let folded_before = app.dashboard.folded.clone();
    press(&mut app, 'M', KeyModifiers::SHIFT).await;
    assert!(!app.z_leader_pending, "M alone should not arm the leader");
    assert_eq!(
        app.dashboard.folded, folded_before,
        "M alone should not change fold state"
    );
}

#[test]
fn detail_bar_renders_when_workspace_is_selected() {
    let mut app = seed_app_with_workspace();
    let idx = app
        .selectable
        .iter()
        .position(|t| matches!(t, SelectionTarget::Workspace(_)))
        .expect("workspace target present");
    app.select_index(idx);

    let backend = TestBackend::new(120, 30);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| draw_for_test(f, &mut app)).unwrap();
    let buf = term.backend().buffer();
    let rendered = (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        rendered.contains("Reply to agent"),
        "bar visible: {rendered}"
    );
}

#[test]
fn detail_bar_absent_when_repo_header_is_selected() {
    let mut app = seed_app_with_workspace();
    let repo_idx = app
        .selectable
        .iter()
        .position(|t| matches!(t, SelectionTarget::Repo(_)))
        .expect("repo target present");
    app.select_index(repo_idx);

    let backend = TestBackend::new(120, 30);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| draw_for_test(f, &mut app)).unwrap();
    let buf = term.backend().buffer();
    let rendered = (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !rendered.contains("Reply to agent"),
        "bar absent on repo header: {rendered}"
    );
}
