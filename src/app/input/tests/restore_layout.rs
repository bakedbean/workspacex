//! restore layout tests.

use super::*;
use crate::test_support::EnvGuard;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

fn setup_two_workspaces_with_sessions(
    slug: &str,
) -> (
    App,
    crate::data::store::WorkspaceId,
    crate::data::store::WorkspaceId,
) {
    use crate::data::store::{NewWorkspace, Store, WorkspaceState};
    let mut env = EnvGuard::new();
    env.set(
        "WSX_CLAUDE_BIN",
        crate::test_support::cat_ignore_args_path(),
    );
    let store = Store::open_in_memory().unwrap();
    let repo_id = store
        .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
        .unwrap();
    let first_id = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "first",
            branch: "repo/first",
            worktree_path: std::path::Path::new(&format!("/tmp/wsx-{slug}-1")),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    let second_id = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "second",
            branch: "repo/second",
            worktree_path: std::path::Path::new(&format!("/tmp/wsx-{slug}-2")),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    store
        .set_workspace_state(first_id, WorkspaceState::Ready)
        .unwrap();
    store
        .set_workspace_state(second_id, WorkspaceState::Ready)
        .unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    for id in [first_id, second_id] {
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let __inst_11 = test_primary_instance(&app, id);
        app.sessions
            .spawn(
                __inst_11,
                id,
                std::path::Path::new("."),
                80,
                24,
                mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Claude,
                None,
            )
            .unwrap();
    }
    (app, first_id, second_id)
}

fn select_workspace_in_app(app: &mut App, id: crate::data::store::WorkspaceId) {
    let idx = app
        .selectable
        .iter()
        .position(|t| matches!(t, SelectionTarget::Workspace(w) if *w == id))
        .expect("workspace in selectable list");
    app.select_index(idx);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dashboard_enter_restores_saved_layout() {
    use crate::ui::split::{SplitDirection, SplitTree};
    let (mut app, first_id, second_id) = setup_two_workspaces_with_sessions("restore");
    let mut tree = SplitTree::Leaf(test_target(&app, first_id));
    tree.split(&[], SplitDirection::Vertical, test_target(&app, second_id));
    app.store
        .set_workspace_layout(first_id, &tree, &[1])
        .unwrap();
    app.refresh().unwrap();
    select_workspace_in_app(&mut app, first_id);
    handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .unwrap();
    match &app.view {
        crate::ui::View::Attached(s) => {
            let leaf_ws: Vec<_> = s.leaves().iter().map(|t| t.workspace_id).collect();
            assert_eq!(leaf_ws, vec![first_id, second_id]);
            assert_eq!(s.focus, vec![1]);
        }
        _ => panic!("expected attached view with restored layout"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dashboard_enter_falls_back_to_single_pane_when_no_layout() {
    let (mut app, first_id, _second_id) = setup_two_workspaces_with_sessions("fallback");
    select_workspace_in_app(&mut app, first_id);
    handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .unwrap();
    match &app.view {
        crate::ui::View::Attached(s) => {
            let leaf_ws: Vec<_> = s.leaves().iter().map(|t| t.workspace_id).collect();
            assert_eq!(leaf_ws, vec![first_id]);
        }
        _ => panic!("expected single-pane attached view"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn l_key_opens_workspace_like_enter() {
    let (mut app, first_id, _second_id) = setup_two_workspaces_with_sessions("l-key");
    select_workspace_in_app(&mut app, first_id);
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    match &app.view {
        crate::ui::View::Attached(s) => {
            let leaf_ws: Vec<_> = s.leaves().iter().map(|t| t.workspace_id).collect();
            assert_eq!(leaf_ws, vec![first_id]);
        }
        _ => panic!("expected single-pane attached view after 'l' on workspace"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restore_prunes_archived_side_panes() {
    use crate::ui::split::{SplitDirection, SplitTree};
    let (mut app, first_id, second_id) = setup_two_workspaces_with_sessions("prune");
    let mut tree = SplitTree::Leaf(test_target(&app, first_id));
    tree.split(&[], SplitDirection::Vertical, test_target(&app, second_id));
    app.store
        .set_workspace_layout(first_id, &tree, &[1])
        .unwrap();
    // Archive second_id directly and refresh so app.workspaces drops it.
    app.store.delete_workspace(second_id).unwrap();
    app.refresh().unwrap();
    select_workspace_in_app(&mut app, first_id);
    handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .unwrap();
    match &app.view {
        crate::ui::View::Attached(s) => {
            let leaf_ws: Vec<_> = s.leaves().iter().map(|t| t.workspace_id).collect();
            assert_eq!(leaf_ws, vec![first_id], "side pane pruned");
        }
        _ => panic!("expected attached view with pruned layout"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ctrl_x_d_does_not_modify_saved_layout() {
    use crate::ui::split::{AttachedState, SplitDirection, SplitTree};
    let (mut app, first_id, second_id) = setup_two_workspaces_with_sessions("ctrlxd");
    let first_target = test_target(&app, first_id);
    let second_target = test_target(&app, second_id);
    let mut tree = SplitTree::Leaf(first_target);
    tree.split(&[], SplitDirection::Vertical, second_target);
    app.store
        .set_workspace_layout(first_id, &tree, &[1])
        .unwrap();
    let mut state = AttachedState::single(first_target);
    state.split(SplitDirection::Vertical, second_target);
    app.view = crate::ui::View::Attached(state);
    // Close second pane with Ctrl-x d (focus is on second_id from the split).
    handle_key_attached(
        &mut app,
        second_target,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();
    handle_key_attached(
        &mut app,
        second_target,
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    // Close last pane → dashboard.
    handle_key_attached(
        &mut app,
        first_target,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();
    handle_key_attached(
        &mut app,
        first_target,
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(matches!(app.view, crate::ui::View::Dashboard));
    // The stored layout is unchanged.
    let (saved, _) = app.store.get_workspace_layout(first_id).unwrap().unwrap();
    let leaf_ws: Vec<_> = saved.leaves().iter().map(|t| t.workspace_id).collect();
    assert_eq!(leaf_ws, vec![first_id, second_id]);
}
