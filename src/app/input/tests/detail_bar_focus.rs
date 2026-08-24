//! detail bar focus tests.

use super::*;
use crate::data::store::{NewWorkspace, Store, WorkspaceState};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

fn make_app_with_workspace_selected() -> App {
    let store = Store::open_in_memory().unwrap();
    let repo_id = store
        .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
        .unwrap();
    let id = store
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
    store
        .set_workspace_state(id, WorkspaceState::Ready)
        .unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    // Force-expand the repo so the workspace stays in `selectable`
    // (idle repos default-fold).
    app.dashboard.folded.insert(repo_id.0 as u64, false);
    let idx = app
        .selectable
        .iter()
        .position(|t| matches!(t, SelectionTarget::Workspace(_)))
        .unwrap();
    app.select_index(idx);
    app
}

#[tokio::test]
async fn tab_on_workspace_moves_focus_to_detail_bar_reply() {
    let mut app = make_app_with_workspace_selected();
    assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
    handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .await
        .unwrap();
    assert!(matches!(app.focus, crate::ui::PaneFocus::DetailBarReply));
}

#[tokio::test]
async fn tab_in_detail_bar_returns_focus_to_dashboard() {
    let mut app = make_app_with_workspace_selected();
    app.focus = crate::ui::PaneFocus::DetailBarReply;
    handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .await
        .unwrap();
    assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
}

#[tokio::test]
async fn esc_in_detail_bar_clears_draft_and_returns_to_dashboard() {
    let mut app = make_app_with_workspace_selected();
    app.focus = crate::ui::PaneFocus::DetailBarReply;
    app.dashboard.reply_draft = "half-typed message".to_string();
    handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .unwrap();
    assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
    assert_eq!(app.dashboard.reply_draft, "");
}

#[tokio::test]
async fn char_in_detail_bar_appends_to_draft() {
    let mut app = make_app_with_workspace_selected();
    app.focus = crate::ui::PaneFocus::DetailBarReply;
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert_eq!(app.dashboard.reply_draft, "hi");
    // Focus must NOT have changed (this is a regression guard
    // against accidentally letting dashboard hotkeys fire).
    assert!(matches!(app.focus, crate::ui::PaneFocus::DetailBarReply));
}

#[tokio::test]
async fn backspace_in_detail_bar_pops_last_char() {
    let mut app = make_app_with_workspace_selected();
    app.focus = crate::ui::PaneFocus::DetailBarReply;
    app.dashboard.reply_draft = "abc".to_string();
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert_eq!(app.dashboard.reply_draft, "ab");
}

#[tokio::test]
async fn arrow_down_while_focused_returns_to_dashboard_and_clears_draft() {
    let mut app = make_app_with_workspace_selected();
    app.focus = crate::ui::PaneFocus::DetailBarReply;
    app.dashboard.reply_draft = "draft".to_string();
    handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .await
        .unwrap();
    assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
    assert_eq!(app.dashboard.reply_draft, "");
}

// Issue 2: Tab cycle should include PM when visible.
#[tokio::test]
async fn tab_in_detail_bar_routes_to_pm_when_visible() {
    let mut app = make_app_with_workspace_selected();
    app.pm_visible = true;
    app.focus = crate::ui::PaneFocus::DetailBarReply;
    handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .await
        .unwrap();
    assert!(matches!(app.focus, crate::ui::PaneFocus::ProjectManager));
}

// Issue 3: Arrow navigation in Dashboard focus must clear the reply draft
// so it cannot be sent to the wrong workspace.
#[tokio::test]
async fn arrow_down_in_dashboard_focus_clears_reply_draft() {
    let mut app = make_app_with_workspace_selected();
    // Compose a draft in DetailBarReply, then Tab back to Dashboard.
    app.focus = crate::ui::PaneFocus::DetailBarReply;
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
        .await
        .unwrap();
    assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
    assert_eq!(app.dashboard.reply_draft, "hi");

    // Now arrow-navigate. The draft should be discarded so it can't
    // be sent to the wrong workspace.
    handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .await
        .unwrap();
    assert_eq!(
        app.dashboard.reply_draft, "",
        "draft must clear on navigation"
    );
}

/// Ctrl-X + digit fires a pinned chip even when focus is on
/// DetailBarReply. The draft must be preserved across Ctrl-X (the leader
/// arm) and cleared once the chip fires.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ctrl_x_digit_works_while_reply_focused() {
    use crate::data::store::NewWorkspace;
    use crate::test_support::EnvGuard;

    let mut env = EnvGuard::new();
    env.set(
        "WSX_CLAUDE_BIN",
        crate::test_support::cat_ignore_args_path(),
    );

    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();

    // Spawn a workspace with a live PTY session (uses `cat` as the binary
    // so any bytes we write are echoed back to the screen).
    let repo_id = app
        .store
        .add_repo(std::path::Path::new("."), "scratch", "test")
        .unwrap();
    let ws_id = app
        .store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "reply-chord-test",
            branch: "main",
            worktree_path: std::path::Path::new("."),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    let mode = crate::pty::session::SpawnMode::Fresh {
        rename_ctx: None,
        custom_instructions: None,
        doctrine: None,
        additional_dirs: vec![],
        yolo: false,
    };
    let __inst_12 = test_primary_instance(&app, ws_id);
    app.sessions
        .spawn(
            __inst_12,
            ws_id,
            std::path::Path::new("."),
            80,
            24,
            mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
            crate::pty::session::AgentKind::Claude,
            None,
        )
        .unwrap();

    app.view = crate::ui::View::Dashboard;
    app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
    app.select_index(0);
    app.focus = crate::ui::PaneFocus::DetailBarReply;
    app.dashboard.reply_draft = "half-typed message".to_string();

    app.pinned_commands_cache = vec![crate::commands::pinned::PinnedCommand {
        label: "PR".into(),
        command: "/pull-request".into(),
    }];
    app.chip_rects = vec![ratatui::layout::Rect {
        x: 5,
        y: 30,
        width: 7,
        height: 1,
    }];

    // Drive Ctrl-X through the real dispatcher (handle_key_dashboard),
    // which first gives handle_detail_bar_reply_key a crack at it
    // because focus == DetailBarReply.
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();

    // Leader must be armed.
    assert!(
        app.leader_pending,
        "Ctrl-X while reply is focused must arm leader_pending"
    );
    // Draft must be intact — Ctrl-X must NOT insert '^X'.
    assert_eq!(
        app.dashboard.reply_draft, "half-typed message",
        "Ctrl-X must not mutate the reply draft"
    );

    // Drive '1' through the same dispatcher.
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
    )
    .await
    .unwrap();

    // After chip fires: draft echoes the dispatched command (cleared
    // by the tick handler when reply_draft_clear_at_ms expires);
    // focus back to Dashboard.
    assert_eq!(
        app.dashboard.reply_draft, "/pull-request",
        "firing a chip must echo the command into the reply draft"
    );
    assert!(
        app.dashboard.reply_draft_clear_at_ms.is_some(),
        "firing a chip must set the reply_draft auto-clear deadline"
    );
    assert!(
        matches!(app.focus, crate::ui::PaneFocus::Dashboard),
        "firing a chip must return focus to Dashboard"
    );

    // Wait for the cat PTY to echo the command back.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let session = app
        .sessions
        .get(test_primary_instance(&app, ws_id))
        .unwrap();
    let parser = session.parser.lock().unwrap();
    let screen_text = parser.screen().contents();
    assert!(
        screen_text.contains("/pull-request"),
        "Ctrl-X+1 while reply focused must dispatch /pull-request to PTY; got: {screen_text:?}"
    );
}
