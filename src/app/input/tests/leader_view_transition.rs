//! leader view transition tests.

use super::*;
use crate::data::store::{NewWorkspace, Store};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

/// Armed Ctrl-X leader must be cleared when the attached view bounces back
/// to Dashboard because the session is gone.  Before the fix, leader_pending
/// would survive the transition and fire against whatever workspace happened
/// to be selected on the dashboard next.
#[tokio::test]
async fn leader_cleared_when_attached_bounces_to_dashboard_on_missing_session() {
    let store = Store::open_in_memory().unwrap();
    let repo_id = store
        .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
        .unwrap();
    let ws_id = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "alpha",
            branch: "repo/alpha",
            worktree_path: std::path::Path::new("/tmp/wsx-test/alpha"),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();

    // Place the app in Attached view — but do NOT spawn a session, so
    // handle_key_attached will immediately bounce back to Dashboard.
    let target = test_target(&app, ws_id);
    app.view = crate::ui::View::Attached(crate::ui::split::AttachedState::single(target));
    // Arm the leader as if the user had pressed Ctrl-X while attached.
    app.leader_pending = true;

    // Drive any key through handle_key_attached.  With no live session
    // it will assign app.view = View::Dashboard and return.
    handle_key_attached(
        &mut app,
        target,
        KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
    )
    .await
    .unwrap();

    assert!(
        matches!(app.view, crate::ui::View::Dashboard),
        "view must transition to Dashboard"
    );
    assert!(
        !app.leader_pending,
        "leader_pending must be cleared on view transition (was still true after bounce)"
    );
}
