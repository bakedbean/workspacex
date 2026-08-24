//! ctrl x shift d tests.

use super::*;
use crate::test_support::EnvGuard;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ctrl_x_shift_d_saves_layout_and_returns_to_dashboard() {
    use crate::data::store::{NewWorkspace, Store, WorkspaceState};
    use crate::ui::split::{AttachedState, SplitDirection};
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
            worktree_path: std::path::Path::new("/tmp/wsx-esc-1"),
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
            worktree_path: std::path::Path::new("/tmp/wsx-esc-2"),
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
    let mode = crate::pty::session::SpawnMode::Fresh {
        rename_ctx: None,
        custom_instructions: None,
        doctrine: None,
        additional_dirs: vec![],
        yolo: false,
    };
    let __inst_9 = test_primary_instance(&app, first_id);
    app.sessions
        .spawn(
            __inst_9,
            first_id,
            std::path::Path::new("."),
            80,
            24,
            mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
            crate::pty::session::AgentKind::Claude,
            None,
        )
        .unwrap();
    let second_mode = crate::pty::session::SpawnMode::Fresh {
        rename_ctx: None,
        custom_instructions: None,
        doctrine: None,
        additional_dirs: vec![],
        yolo: false,
    };
    let __inst_10 = test_primary_instance(&app, second_id);
    app.sessions
        .spawn(
            __inst_10,
            second_id,
            std::path::Path::new("."),
            80,
            24,
            second_mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
            crate::pty::session::AgentKind::Claude,
            None,
        )
        .unwrap();

    let first_target = test_target(&app, first_id);
    let second_target = test_target(&app, second_id);
    let mut state = AttachedState::single(first_target);
    state.split(SplitDirection::Vertical, second_target);
    app.view = crate::ui::View::Attached(state);

    // Send Ctrl-x then Shift-D.
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
        KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT),
    )
    .await
    .unwrap();

    assert!(
        matches!(app.view, crate::ui::View::Dashboard),
        "should return to dashboard"
    );
    let saved = app.store.get_workspace_layout(first_id).unwrap();
    assert!(saved.is_some(), "layout should be saved under first leaf");
    let (tree, _focus) = saved.unwrap();
    let leaf_ws: Vec<_> = tree.leaves().iter().map(|t| t.workspace_id).collect();
    assert_eq!(leaf_ws, vec![first_id, second_id]);
    assert!(
        app.workspaces_with_multi_pane_layouts.contains(&first_id),
        "cache should refresh to include the new layout's anchor"
    );
}
