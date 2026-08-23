//! Clicks and wheel: chips, footer hints, attention rows, scrollback.

use super::*;
use crate::data::store::Store;
use crate::test_support::EnvGuard;
use std::path::PathBuf;
// `dashboard_renders_split_with_pm_title_when_visible_even_without_session`
// (the PTY-placeholder render test) is gone — the dashboard's PM pane
// now always renders the digest (`render_digest`), whose own render
// tests live in `src/ui/pm_pane.rs::digest_tests`.

use super::common::*;
use crossterm::event::KeyModifiers;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wheel_up_scrolls_attached_workspace() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);
    handle_mouse(&mut app, mouse_event(MouseEventKind::ScrollUp)).await;
    assert_eq!(
        app.sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap()
            .scrollback_offset
            .load(std::sync::atomic::Ordering::Relaxed),
        3,
        "one wheel notch = 3 rows"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wheel_down_decreases_offset_saturating() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);
    app.sessions
        .get(test_primary_instance(&app, ws_id))
        .unwrap()
        .scroll_up(5);
    handle_mouse(&mut app, mouse_event(MouseEventKind::ScrollDown)).await;
    assert_eq!(
        app.sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap()
            .scrollback_offset
            .load(std::sync::atomic::Ordering::Relaxed),
        2
    );
}

// `wheel_targets_pm_in_dashboard_when_pm_focused` is gone: the
// dashboard's PM pane is the digest now, which has no PTY to scroll —
// `active_session` no longer has a Dashboard+ProjectManager arm.

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wheel_noop_when_dashboard_focused_no_target() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    // No PM, no attached workspace; view is Dashboard.
    // Just verify the call doesn't panic.
    handle_mouse(&mut app, mouse_event(MouseEventKind::ScrollUp)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn click_in_chip_rect_fires_pinned_command() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let _ws_id = spawn_attached_workspace(&mut app);

    app.pinned_commands_cache = vec![crate::commands::pinned::PinnedCommand {
        label: "PR".into(),
        command: "/pull-request".into(),
    }];
    // Place a 7-wide chip at (5, 30): "[1] PR " = 7 cols.
    app.chip_rects = vec![ratatui::layout::Rect {
        x: 5,
        y: 30,
        width: 7,
        height: 1,
    }];

    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 6,
        row: 30,
        modifiers: KeyModifiers::NONE,
    };
    handle_mouse(&mut app, click).await;

    // wait for PTY cat echo
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let session = active_session(&app).unwrap();
    let parser = session.parser.lock().unwrap();
    let screen_text = parser.screen().contents();
    assert!(
        screen_text.contains("/pull-request"),
        "expected chip click to send /pull-request; got: {screen_text:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn click_outside_chip_rect_does_nothing() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let _ws_id = spawn_attached_workspace(&mut app);

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

    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 50, // outside chip
        row: 10,    // outside chip
        modifiers: KeyModifiers::NONE,
    };
    handle_mouse(&mut app, click).await;

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let session = active_session(&app).unwrap();
    let parser = session.parser.lock().unwrap();
    let screen_text = parser.screen().contents();
    assert!(
        !screen_text.contains("/pull-request"),
        "click outside any chip must not fire; got: {screen_text:?}"
    );
}

/// Clicking the chip-row running-process count (`● Np`) opens the
/// ProcessList modal for the focused workspace, mirroring `K` on it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn click_procs_count_opens_process_list() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);

    // The chip-row procs count reports a clickable rect during draw.
    app.procs_link_rect = Some((
        ws_id,
        ratatui::layout::Rect {
            x: 60,
            y: 30,
            width: 4,
            height: 1,
        },
    ));

    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 61,
        row: 30,
        modifiers: KeyModifiers::NONE,
    };
    handle_mouse(&mut app, click).await;

    assert!(
        matches!(app.modal, Some(Modal::ProcessList { workspace_id, .. }) if workspace_id == ws_id),
        "clicking the procs count should open ProcessList for that workspace; got {:?}",
        app.modal
    );
}

/// A click that misses the procs-count rect must not open the modal.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn click_outside_procs_count_does_not_open_process_list() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);

    app.procs_link_rect = Some((
        ws_id,
        ratatui::layout::Rect {
            x: 60,
            y: 30,
            width: 4,
            height: 1,
        },
    ));

    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 10, // outside the procs rect
        row: 10,
        modifiers: KeyModifiers::NONE,
    };
    handle_mouse(&mut app, click).await;

    assert!(
        !matches!(app.modal, Some(Modal::ProcessList { .. })),
        "click off the procs count must not open ProcessList; got {:?}",
        app.modal
    );
}

/// Regression (#224): while any modal is open, a left click landing on an
/// attention row must be swallowed, not attach to the workspace beneath
/// the overlay.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn click_attention_row_while_modal_open_does_not_attach() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);
    app.view = crate::ui::View::Dashboard;

    app.attention_rects = vec![(
        ws_id,
        ratatui::layout::Rect {
            x: 5,
            y: 10,
            width: 20,
            height: 1,
        },
    )];
    app.modal = Some(Modal::WorkspaceActions);

    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 6,
        row: 10,
        modifiers: KeyModifiers::NONE,
    };
    handle_mouse(&mut app, click).await;

    assert!(
        matches!(app.view, crate::ui::View::Dashboard),
        "attention click under a modal must not attach; got {:?}",
        app.view
    );
    assert!(
        matches!(app.modal, Some(Modal::WorkspaceActions)),
        "the open modal must be untouched by the swallowed click; got {:?}",
        app.modal
    );
}

/// Regression (#224): the modal click gate covers every left-click
/// target, not just attention rows — here the attached-view chip-row
/// procs count (`procs_link_rect` is only ever set by the attached
/// render): a click on it under a modal must not replace that modal
/// with ProcessList.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn click_procs_count_while_modal_open_is_swallowed() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);

    app.procs_link_rect = Some((
        ws_id,
        ratatui::layout::Rect {
            x: 60,
            y: 30,
            width: 4,
            height: 1,
        },
    ));
    app.modal = Some(Modal::Error {
        message: "boom".into(),
    });

    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 61,
        row: 30,
        modifiers: KeyModifiers::NONE,
    };
    handle_mouse(&mut app, click).await;

    assert!(
        matches!(app.modal, Some(Modal::Error { .. })),
        "procs click under a modal must not open ProcessList; got {:?}",
        app.modal
    );
}

/// Clicking a dashboard footer hint fires the corresponding key, exactly
/// as if it had been pressed. `/` enters filter mode.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn click_dashboard_footer_hint_fires_key() {
    use crate::ui::footer::FooterHintAction;
    use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.view = crate::ui::View::Dashboard;

    app.footer_hint_rects = vec![(
        ratatui::layout::Rect {
            x: 0,
            y: 40,
            width: 8,
            height: 1,
        },
        FooterHintAction::Key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)),
    )];

    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 2,
        row: 40,
        modifiers: KeyModifiers::NONE,
    };
    handle_mouse(&mut app, click).await;

    assert!(
        app.dashboard.filter.is_some(),
        "clicking the `/` footer hint must enter filter mode"
    );
}

/// Clicking the `^x` leader pill in the attached footer arms the leader,
/// exactly like pressing Ctrl-x.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn click_attached_footer_leader_pill_arms_leader() {
    use crate::ui::footer::FooterHintAction;
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let _ws_id = spawn_attached_workspace(&mut app);

    app.footer_hint_rects = vec![(
        ratatui::layout::Rect {
            x: 10,
            y: 40,
            width: 4,
            height: 1,
        },
        FooterHintAction::ArmLeader,
    )];

    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 11,
        row: 40,
        modifiers: KeyModifiers::NONE,
    };
    handle_mouse(&mut app, click).await;

    assert!(
        app.leader_pending,
        "clicking the ^x pill must arm the attached-view leader"
    );
}

/// The `^x` pill routes a real `Ctrl-x` through the handlers rather than
/// poking `leader_pending` directly, so a second click behaves like a
/// second `Ctrl-x` keypress: it clears the leader (and sends a literal
/// `^X`) instead of leaving the leader stuck armed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn double_click_attached_leader_pill_does_not_stick_armed() {
    use crate::ui::footer::FooterHintAction;
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let _ws_id = spawn_attached_workspace(&mut app);

    app.footer_hint_rects = vec![(
        ratatui::layout::Rect {
            x: 10,
            y: 40,
            width: 4,
            height: 1,
        },
        FooterHintAction::ArmLeader,
    )];
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 11,
        row: 40,
        modifiers: KeyModifiers::NONE,
    };

    handle_mouse(&mut app, click).await;
    assert!(app.leader_pending, "first ^x click arms the leader");

    handle_mouse(&mut app, click).await;
    assert!(
        !app.leader_pending,
        "second ^x click must clear the leader, matching double Ctrl-x \
         (not leave it stuck armed)"
    );
}

/// Clicking an attached footer keybind hint arms the leader and dispatches
/// the command in one click. `^x a` opens the agents panel.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn click_attached_footer_hint_dispatches_leader_command() {
    use crate::ui::footer::FooterHintAction;
    use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let _ws_id = spawn_attached_workspace(&mut app);

    app.footer_hint_rects = vec![(
        ratatui::layout::Rect {
            x: 20,
            y: 40,
            width: 9,
            height: 1,
        },
        FooterHintAction::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)),
    )];

    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 22,
        row: 40,
        modifiers: KeyModifiers::NONE,
    };
    handle_mouse(&mut app, click).await;

    assert!(
        matches!(app.modal, Some(crate::ui::modal::Modal::AgentsPanel { .. })),
        "clicking the `a` footer hint must open the agents panel via the leader"
    );
    assert!(
        !app.leader_pending,
        "leader must clear once the click's follow-up key is consumed"
    );
}

/// Chip click from `View::Dashboard` dispatches the command to the selected
/// workspace's session, not `active_session` (which returns `None` in the
/// dashboard view).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn click_chip_in_dashboard_view_fires_pinned_command() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);

    // Switch to dashboard view — active_session() now returns None.
    app.view = crate::ui::View::Dashboard;
    // Point selectable at the workspace so selected_target() returns it.
    app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
    app.select_index(0);

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

    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 6,
        row: 30,
        modifiers: KeyModifiers::NONE,
    };
    handle_mouse(&mut app, click).await;

    // Wait for PTY cat echo.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let session = app
        .sessions
        .get(test_primary_instance(&app, ws_id))
        .unwrap();
    let parser = session.parser.lock().unwrap();
    let screen_text = parser.screen().contents();
    assert!(
        screen_text.contains("/pull-request"),
        "dashboard chip click must dispatch /pull-request to the workspace session; got: {screen_text:?}"
    );
}

/// A chip click from the dashboard echoes the dispatched command
/// into the reply input as visual confirmation, and sets a
/// wall-clock deadline (`reply_draft_clear_at_ms`) so the tick
/// handler wipes it shortly afterward.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chip_dispatch_echoes_command_into_reply_input() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);

    app.view = crate::ui::View::Dashboard;
    app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
    app.select_index(0);
    // No pre-existing draft.
    assert_eq!(app.dashboard.reply_draft, "");
    assert!(app.dashboard.reply_draft_clear_at_ms.is_none());

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

    let now_before_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    handle_mouse(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 6,
            row: 30,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await;

    // Draft echoes the dispatched command.
    assert_eq!(
        app.dashboard.reply_draft, "/pull-request",
        "chip dispatch must echo the command into reply_draft"
    );
    // Deadline is set in the future (sanity bound: within 5 seconds).
    let deadline = app
        .dashboard
        .reply_draft_clear_at_ms
        .expect("deadline must be set");
    assert!(
        deadline > now_before_ms && deadline < now_before_ms + 5_000,
        "deadline {deadline} should be slightly after {now_before_ms}"
    );
}

/// Backspace and Char keystrokes in the reply input cancel any
/// pending chip-echo auto-clear so the user's edits aren't wiped
/// by the tick handler mid-typing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_typing_in_reply_cancels_chip_echo_deadline() {
    use crossterm::event::{KeyCode, KeyEvent};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);

    app.view = crate::ui::View::Dashboard;
    app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
    app.select_index(0);
    app.focus = crate::ui::PaneFocus::DetailBarReply;
    // Simulate state right after a chip dispatch: draft echoes the
    // command, deadline is set.
    app.dashboard.reply_draft = "/pull-request".to_string();
    app.dashboard.reply_draft_clear_at_ms = Some(u64::MAX);

    // User types a char.
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
    )
    .await
    .unwrap();

    // Deadline is cleared so the tick handler won't wipe their edits.
    assert!(
        app.dashboard.reply_draft_clear_at_ms.is_none(),
        "Char keystroke must cancel the chip-echo auto-clear deadline"
    );

    // Reset and try Backspace.
    app.dashboard.reply_draft_clear_at_ms = Some(u64::MAX);
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(
        app.dashboard.reply_draft_clear_at_ms.is_none(),
        "Backspace keystroke must cancel the chip-echo auto-clear deadline"
    );
}

/// A chip click from the dashboard on a workspace with NO live
/// session must auto-spawn one so the chip command isn't silently
/// dropped. Mirrors the production fix for the inline-reply gap.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn click_chip_auto_spawns_session_when_missing() {
    use crate::data::store::NewWorkspace;
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    let mut env = EnvGuard::new();
    env.set(
        "WSX_CLAUDE_BIN",
        crate::test_support::cat_ignore_args_path(),
    );
    let store = Store::open_in_memory().unwrap();
    // Remote-control defaults ON, which appends `--remote-control` to the
    // spawned agent command. The real `claude` understands that flag; the
    // `cat` stand-in does not — it errors out and exits immediately. The
    // command then only lands on screen if the PTY's own echo wins the
    // race against `cat`'s teardown, which flakes under CI load. Disable
    // it so `cat` stays alive and deterministically echoes the dispatched
    // command, mirroring the other fake-binary tests' RemoteOpts::disabled().
    store.set_setting("remote_control", "off").unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let repo_id = app
        .store
        .add_repo(std::path::Path::new("."), "scratch", "test")
        .unwrap();
    let ws_id = app
        .store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "auto-spawn-test",
            branch: "main",
            worktree_path: std::path::Path::new("."),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    app.refresh().unwrap();

    // Critical precondition: NO session spawned for this workspace.
    assert!(
        app.sessions
            .get(test_primary_instance(&app, ws_id))
            .is_none(),
        "precondition: workspace must not have a session yet"
    );

    app.view = crate::ui::View::Dashboard;
    app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
    app.select_index(0);

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

    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 6,
        row: 30,
        modifiers: KeyModifiers::NONE,
    };
    handle_mouse(&mut app, click).await;

    // The session must have been auto-spawned by fire_chip.
    assert!(
        app.sessions
            .get(test_primary_instance(&app, ws_id))
            .is_some(),
        "fire_chip must auto-spawn a session for the selected workspace"
    );

    // And the command must have reached the new session's PTY.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let session = app
        .sessions
        .get(test_primary_instance(&app, ws_id))
        .unwrap();
    let parser = session.parser.lock().unwrap();
    let screen_text = parser.screen().contents();
    assert!(
        screen_text.contains("/pull-request"),
        "chip command must dispatch to the auto-spawned session; got: {screen_text:?}"
    );
}

/// A chip click in the attached view dispatches the command but must
/// NOT clear the dashboard reply draft or overwrite the dashboard
/// pane focus — those state slots aren't visible from the attached
/// view and trampling them would leak across the view boundary.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attached_chip_click_preserves_dashboard_draft_and_focus() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);

    // We're in View::Attached (set by spawn_attached_workspace).
    assert!(matches!(app.view, crate::ui::View::Attached(_)));

    // Seed dashboard-scoped state the user can't see from here.
    app.dashboard.reply_draft = "hello agent".into();
    app.focus = crate::ui::PaneFocus::ProjectManager;

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

    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 6,
        row: 30,
        modifiers: KeyModifiers::NONE,
    };
    handle_mouse(&mut app, click).await;

    // Command still dispatched.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let session = app
        .sessions
        .get(test_primary_instance(&app, ws_id))
        .unwrap();
    let parser = session.parser.lock().unwrap();
    let screen_text = parser.screen().contents();
    assert!(
        screen_text.contains("/pull-request"),
        "attached-view chip click must still dispatch the command; got: {screen_text:?}"
    );
    drop(parser);

    // But dashboard state must be unchanged.
    assert_eq!(
        app.dashboard.reply_draft, "hello agent",
        "attached-view chip click must not clear the dashboard reply draft"
    );
    assert!(
        matches!(app.focus, crate::ui::PaneFocus::ProjectManager),
        "attached-view chip click must not overwrite the dashboard pane focus"
    );
}
