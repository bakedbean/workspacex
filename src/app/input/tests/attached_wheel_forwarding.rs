//! attached wheel forwarding.

use super::*;
use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

fn mouse_at_mod(kind: MouseEventKind, col: u16, row: u16, mods: KeyModifiers) -> MouseEvent {
    MouseEvent {
        kind,
        column: col,
        row,
        modifiers: mods,
    }
}

fn spawn_attached_workspace(app: &mut App) -> crate::data::store::WorkspaceId {
    use crate::data::store::NewWorkspace;
    use crate::test_support::EnvGuard;
    let mut env = EnvGuard::new();
    env.set(
        "WSX_CLAUDE_BIN",
        crate::test_support::cat_ignore_args_path(),
    );
    let repo_id = app
        .store
        .add_repo(std::path::Path::new("."), "scratch", "test")
        .unwrap();
    let ws_id = app
        .store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "wheel-fwd-test",
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
    let __inst_13 = test_primary_instance(app, ws_id);
    app.sessions
        .spawn(
            __inst_13,
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
    app.view = crate::ui::View::Attached(AttachedState::single(test_target(app, ws_id)));
    ws_id
}

// Enable SGR mouse reporting on the session's parser and register a
// full-screen pane rect so the cursor at (10,10) is "over" the pane.
fn arm_mouse_mode_and_pane(app: &mut App, ws_id: crate::data::store::WorkspaceId) {
    let session = app.sessions.get(test_primary_instance(app, ws_id)).unwrap();
    {
        let mut p = session.parser.lock().unwrap();
        p.process(b"\x1b[?1000h\x1b[?1006h");
    }
    app.attached_pane_rects = vec![(
        session,
        Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        },
    )];
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plain_wheel_forwards_when_mouse_mode_on() {
    let store = crate::data::store::Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);
    arm_mouse_mode_and_pane(&mut app, ws_id);
    handle_mouse(
        &mut app,
        mouse_at_mod(MouseEventKind::ScrollUp, 10, 10, KeyModifiers::NONE),
    )
    .await;
    // Forwarded to the agent -> wsx scrollback must NOT move.
    assert_eq!(
        app.sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap()
            .scrollback_offset
            .load(Ordering::Relaxed),
        0,
        "plain wheel over a mouse-aware pane is forwarded, not scrolled locally"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shift_wheel_is_escape_hatch_to_scrollback() {
    let store = crate::data::store::Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);
    arm_mouse_mode_and_pane(&mut app, ws_id);
    handle_mouse(
        &mut app,
        mouse_at_mod(MouseEventKind::ScrollUp, 10, 10, KeyModifiers::SHIFT),
    )
    .await;
    // Shift bypasses the agent -> wsx scrollback moves.
    assert_eq!(
        app.sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap()
            .scrollback_offset
            .load(Ordering::Relaxed),
        3,
        "shift+wheel drives wsx scrollback even when the agent has mouse mode on"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plain_wheel_scrolls_when_mouse_mode_off() {
    let store = crate::data::store::Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);
    // Register the pane rect but do NOT enable mouse mode.
    let session = app
        .sessions
        .get(test_primary_instance(&app, ws_id))
        .unwrap();
    app.attached_pane_rects = vec![(
        session,
        Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        },
    )];
    handle_mouse(
        &mut app,
        mouse_at_mod(MouseEventKind::ScrollUp, 10, 10, KeyModifiers::NONE),
    )
    .await;
    assert_eq!(
        app.sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap()
            .scrollback_offset
            .load(Ordering::Relaxed),
        3,
        "without agent mouse mode, plain wheel falls through to wsx scrollback"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plain_wheel_down_forwards_when_mouse_mode_on() {
    let store = crate::data::store::Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);
    arm_mouse_mode_and_pane(&mut app, ws_id);
    // Pre-scroll so a fall-through ScrollDown would drop 5 -> 2; forwarding
    // leaves it at 5.
    app.sessions
        .get(test_primary_instance(&app, ws_id))
        .unwrap()
        .scroll_up(5);
    handle_mouse(
        &mut app,
        mouse_at_mod(MouseEventKind::ScrollDown, 10, 10, KeyModifiers::NONE),
    )
    .await;
    assert_eq!(
        app.sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap()
            .scrollback_offset
            .load(Ordering::Relaxed),
        5,
        "scrolldown over a mouse-aware pane is forwarded, leaving wsx scrollback untouched"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plain_wheel_over_chrome_falls_through_to_scrollback() {
    let store = crate::data::store::Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);
    arm_mouse_mode_and_pane(&mut app, ws_id); // pane rect is height 24 (rows 0..23)
    // Row 30 is below the pane -> no pane under cursor -> scrollback even though
    // mouse mode is on.
    handle_mouse(
        &mut app,
        mouse_at_mod(MouseEventKind::ScrollUp, 10, 30, KeyModifiers::NONE),
    )
    .await;
    assert_eq!(
        app.sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap()
            .scrollback_offset
            .load(Ordering::Relaxed),
        3,
        "wheel over chrome (no pane under cursor) drives wsx scrollback"
    );
}
