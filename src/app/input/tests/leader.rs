//! The `Ctrl-x` and `z` leader chords.

use super::*;
use crate::data::store::Store;
use crate::test_support::EnvGuard;
use std::path::PathBuf;
// `dashboard_renders_split_with_pm_title_when_visible_even_without_session`
// (the PTY-placeholder render test) is gone — the dashboard's PM pane
// now always renders the digest (`render_digest`), whose own render
// tests live in `src/ui/pm_pane.rs::digest_tests`.

use super::common::*;
use crossterm::event::{KeyEvent, KeyModifiers};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ctrl_x_d_closes_focused_pane_when_split() {
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
            worktree_path: std::path::Path::new("/tmp/wsx-close-1"),
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
            worktree_path: std::path::Path::new("/tmp/wsx-close-2"),
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
        let __inst_2 = test_primary_instance(&app, id);
        app.sessions
            .spawn(
                __inst_2,
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
    // Start in a 2-pane split with `second` focused.
    let first_target = test_target(&app, first_id);
    let second_target = test_target(&app, second_id);
    let mut state = AttachedState::single(first_target);
    state.split(SplitDirection::Vertical, second_target);
    app.view = crate::ui::View::Attached(state);

    // Ctrl-x d closes JUST the focused pane; should leave `first` alone.
    handle_key_attached(
        &mut app,
        second_target,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();
    assert!(app.leader_pending);
    handle_key_attached(
        &mut app,
        second_target,
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    match &app.view {
        crate::ui::View::Attached(state) => {
            assert_eq!(state.leaf_count(), 1, "should drop down to 1 pane");
            assert_eq!(state.focused_target(), Some(first_target));
        }
        other => panic!("expected Attached view; got {other:?}"),
    }

    // Ctrl-x d on the last pane detaches fully.
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
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ctrl_x_d_detach_schedules_refresh_for_attached_workspace() {
    // The detail bar shows the workspace's events/diff/procs from
    // app state, which is normally refreshed every 2s by the
    // background poll. When the user detaches back to the
    // dashboard, we want the bar to reflect work just done in the
    // attached session immediately — so detach handlers must clear
    // throttle stamps and queue the workspace for an out-of-band
    // events-tail refresh.
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
    let id = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "ws",
            branch: "repo/ws",
            worktree_path: std::path::Path::new("/tmp/wsx-detach-refresh"),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    store
        .set_workspace_state(id, WorkspaceState::Ready)
        .unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let mode = crate::pty::session::SpawnMode::Fresh {
        rename_ctx: None,
        custom_instructions: None,
        doctrine: None,
        additional_dirs: vec![],
        yolo: false,
    };
    let __inst_3 = test_primary_instance(&app, id);
    app.sessions
        .spawn(
            __inst_3,
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
    let target = test_target(&app, id);
    app.view = crate::ui::View::Attached(AttachedState::single(target));
    // Seed throttle stamps so we can prove the detach handler
    // clears them (forcing the next poll tick to re-fetch).
    app.diff_last_poll_ms.insert(id, 12_345);
    app.pr_last_poll_ms.insert(id, 12_345);
    app.last_proc_scan_ms = 12_345;

    // Ctrl-x d on the last pane fully detaches.
    handle_key_attached(
        &mut app,
        target,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();
    handle_key_attached(
        &mut app,
        target,
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
    )
    .await
    .unwrap();

    assert!(matches!(app.view, crate::ui::View::Dashboard));
    assert!(
        app.pending_workspace_refresh.contains(&id),
        "detached workspace should be queued for events-tail refresh"
    );
    assert!(
        !app.diff_last_poll_ms.contains_key(&id),
        "diff throttle stamp should be cleared on detach"
    );
    assert!(
        !app.pr_last_poll_ms.contains_key(&id),
        "PR throttle stamp should be cleared on detach"
    );
    assert_eq!(
        app.last_proc_scan_ms, 0,
        "proc-scan throttle should be reset on detach"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ctrl_x_shift_d_detach_schedules_refresh_for_attached_workspace() {
    // Same as the d-path test above, for the Ctrl-X Shift-D save+detach.
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
    let id = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "ws",
            branch: "repo/ws",
            worktree_path: std::path::Path::new("/tmp/wsx-esc-refresh"),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    store
        .set_workspace_state(id, WorkspaceState::Ready)
        .unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let mode = crate::pty::session::SpawnMode::Fresh {
        rename_ctx: None,
        custom_instructions: None,
        doctrine: None,
        additional_dirs: vec![],
        yolo: false,
    };
    let __inst_4 = test_primary_instance(&app, id);
    app.sessions
        .spawn(
            __inst_4,
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
    let target = test_target(&app, id);
    app.view = crate::ui::View::Attached(AttachedState::single(target));

    handle_key_attached(
        &mut app,
        target,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();
    handle_key_attached(
        &mut app,
        target,
        KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT),
    )
    .await
    .unwrap();

    assert!(matches!(app.view, crate::ui::View::Dashboard));
    assert!(
        app.pending_workspace_refresh.contains(&id),
        "Shift-D-detached workspace should be queued for refresh"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ctrl_x_arrow_moves_focus_in_split() {
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
    let mut ids = Vec::new();
    for name in ["a", "b"] {
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name,
                branch: &format!("repo/{name}"),
                worktree_path: &std::path::PathBuf::from(format!("/tmp/wsx-arrow-{name}")),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        store
            .set_workspace_state(id, WorkspaceState::Ready)
            .unwrap();
        ids.push(id);
    }
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    for id in &ids {
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let __inst_5 = test_primary_instance(&app, *id);
        app.sessions
            .spawn(
                __inst_5,
                *id,
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
    let target0 = test_target(&app, ids[0]);
    let target1 = test_target(&app, ids[1]);
    let mut state = AttachedState::single(target0);
    state.split(SplitDirection::Vertical, target1);
    // Focus is on ids[1] post-split.
    app.view = crate::ui::View::Attached(state);

    handle_key_attached(
        &mut app,
        target1,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();
    handle_key_attached(
        &mut app,
        target1,
        KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    match &app.view {
        crate::ui::View::Attached(state) => {
            assert_eq!(state.focused_target(), Some(target0));
        }
        other => panic!("expected Attached view; got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ctrl_x_down_enter_fires_highlighted_action() {
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
    let id = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "a",
            branch: "repo/a",
            worktree_path: &std::path::PathBuf::from("/tmp/wsx-nav-a"),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    store
        .set_workspace_state(id, WorkspaceState::Ready)
        .unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let inst = test_primary_instance(&app, id);
    app.sessions
        .spawn(
            inst,
            id,
            std::path::Path::new("."),
            80,
            24,
            crate::pty::session::SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            },
            crate::agent::remote_control::RemoteOpts::disabled(),
            crate::pty::session::AgentKind::Claude,
            None,
        )
        .unwrap();
    let target = test_target(&app, id);
    app.view = crate::ui::View::Attached(AttachedState::single(target));

    // Arm leader (selected=0 => "detach"), Down once => index 1 ("updates").
    handle_key_attached(
        &mut app,
        target,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();
    assert!(app.leader_pending);
    assert_eq!(app.leader_selected, 0);
    handle_key_attached(
        &mut app,
        target,
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert_eq!(app.leader_selected, 1);
    assert!(app.leader_pending, "↑↓ keep the leader armed");

    // Enter fires "updates" — same effect as pressing 'u' after ^x.
    handle_key_attached(
        &mut app,
        target,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(!app.leader_pending);
    assert!(
        matches!(
            app.modal,
            Some(crate::ui::modal::Modal::UpdatesPanel { .. })
        ),
        "Enter on the updates row opens the updates panel"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ctrl_x_esc_dismisses_nav_overlay_without_detaching() {
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
    let id = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "a",
            branch: "repo/a",
            worktree_path: &std::path::PathBuf::from("/tmp/wsx-nav-esc-a"),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    store
        .set_workspace_state(id, WorkspaceState::Ready)
        .unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let inst = test_primary_instance(&app, id);
    app.sessions
        .spawn(
            inst,
            id,
            std::path::Path::new("."),
            80,
            24,
            crate::pty::session::SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            },
            crate::agent::remote_control::RemoteOpts::disabled(),
            crate::pty::session::AgentKind::Claude,
            None,
        )
        .unwrap();
    let target = test_target(&app, id);
    app.view = crate::ui::View::Attached(AttachedState::single(target));

    // Arm the nav overlay with Ctrl-x.
    handle_key_attached(
        &mut app,
        target,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();
    assert!(app.leader_pending, "Ctrl-x must arm the nav overlay");

    // Esc dismisses the overlay and leaves us in the attached chat view —
    // it must NOT detach to the dashboard (that's what 'd' is for).
    handle_key_attached(
        &mut app,
        target,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(!app.leader_pending, "Esc must dismiss the nav overlay");
    assert!(
        matches!(app.view, crate::ui::View::Attached(_)),
        "Esc on the nav overlay must stay attached, not return to the dashboard"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn leader_digit_sends_pinned_command_to_pty() {
    use crossterm::event::{KeyCode, KeyEvent};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);
    let target = test_target(&app, ws_id);

    // Populate the cache directly (the resolution path is tested
    // separately via the resolve() unit tests).
    app.pinned_commands_cache = vec![crate::commands::pinned::PinnedCommand {
        label: "PR".into(),
        command: "/pull-request".into(),
    }];

    // Ctrl-x leader.
    handle_key_attached(
        &mut app,
        target,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();
    assert!(app.leader_pending);

    // '1' — fires chip 1, clears leader.
    handle_key_attached(
        &mut app,
        target,
        KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(!app.leader_pending);

    // cat echoes input back. Verify the screen eventually contains
    // the command text.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let session = app
        .sessions
        .get(test_primary_instance(&app, ws_id))
        .unwrap();
    let parser = session.parser.lock().unwrap();
    let screen_text = parser.screen().contents();
    assert!(
        screen_text.contains("/pull-request"),
        "expected '/pull-request' on screen; got: {screen_text:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn leader_digit_out_of_range_is_noop() {
    use crossterm::event::{KeyCode, KeyEvent};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);
    let target = test_target(&app, ws_id);

    // Only one chip in the cache.
    app.pinned_commands_cache = vec![crate::commands::pinned::PinnedCommand {
        label: "PR".into(),
        command: "/pull-request".into(),
    }];

    // Ctrl-x leader.
    handle_key_attached(
        &mut app,
        target,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();

    // '5' — index 4, out of range for a 1-element cache.
    handle_key_attached(
        &mut app,
        target,
        KeyEvent::new(KeyCode::Char('5'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(!app.leader_pending);

    // No bytes should have been written; cat hasn't echoed anything.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let session = app
        .sessions
        .get(test_primary_instance(&app, ws_id))
        .unwrap();
    let parser = session.parser.lock().unwrap();
    let screen_text = parser.screen().contents();
    assert!(
        !screen_text.contains("/pull-request"),
        "out-of-range digit must not fire any chip; got: {screen_text:?}"
    );
}

/// Ctrl-X arms `leader_pending`; a subsequent digit fires the chip command
/// to the selected workspace's session.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dashboard_ctrl_x_then_digit_fires_pinned_chip() {
    use crossterm::event::{KeyCode, KeyEvent};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);

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

    // Ctrl-X — arms the leader.
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();
    assert!(app.leader_pending, "Ctrl-X must arm leader_pending");

    // '1' — fires chip 0, clears leader.
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(
        !app.leader_pending,
        "leader must clear after digit follow-up"
    );

    // cat echoes input back; verify the command reached the PTY.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let session = app
        .sessions
        .get(test_primary_instance(&app, ws_id))
        .unwrap();
    let parser = session.parser.lock().unwrap();
    let screen_text = parser.screen().contents();
    assert!(
        screen_text.contains("/pull-request"),
        "dashboard Ctrl-X+1 must dispatch /pull-request to the workspace PTY; got: {screen_text:?}"
    );
}

/// Ctrl-X then a non-digit key clears the leader without firing any chip.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dashboard_ctrl_x_then_non_digit_clears_leader_no_fire() {
    use crossterm::event::{KeyCode, KeyEvent};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);

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

    // Ctrl-X — arms the leader.
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();
    assert!(app.leader_pending);

    // 'z' — a key with no leader binding; clears the leader without
    // firing. (Not 'a', which now opens the agents panel.)
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(
        !app.leader_pending,
        "leader must clear after non-digit follow-up"
    );

    // No chip command should have been dispatched.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let session = app
        .sessions
        .get(test_primary_instance(&app, ws_id))
        .unwrap();
    let parser = session.parser.lock().unwrap();
    let screen_text = parser.screen().contents();
    assert!(
        !screen_text.contains("/pull-request"),
        "non-digit follow-up must not fire any chip; got: {screen_text:?}"
    );
}

/// Ctrl-X + a digit whose index exceeds the number of visible chip_rects
/// is a no-op (fire_chip guards on idx >= chip_rects.len()).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dashboard_ctrl_x_digit_beyond_visible_chips_is_noop() {
    use crossterm::event::{KeyCode, KeyEvent};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);

    app.view = crate::ui::View::Dashboard;
    app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
    app.select_index(0);

    // Three commands in cache but only two chip_rects rendered.
    app.pinned_commands_cache = vec![
        crate::commands::pinned::PinnedCommand {
            label: "PR".into(),
            command: "/pull-request".into(),
        },
        crate::commands::pinned::PinnedCommand {
            label: "B".into(),
            command: "/build".into(),
        },
        crate::commands::pinned::PinnedCommand {
            label: "T".into(),
            command: "/test".into(),
        },
    ];
    app.chip_rects = vec![
        ratatui::layout::Rect {
            x: 5,
            y: 30,
            width: 7,
            height: 1,
        },
        ratatui::layout::Rect {
            x: 13,
            y: 30,
            width: 5,
            height: 1,
        },
    ];

    // Ctrl-X then '3' — index 2, beyond chip_rects.len() == 2.
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(!app.leader_pending);

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let session = app
        .sessions
        .get(test_primary_instance(&app, ws_id))
        .unwrap();
    let parser = session.parser.lock().unwrap();
    let screen_text = parser.screen().contents();
    assert!(
        !screen_text.contains("/test"),
        "digit beyond visible chips must not dispatch any command; got: {screen_text:?}"
    );
}

/// A second Ctrl-X while the leader is armed must clear it (cancel the
/// chord), not silently re-arm. Matches the attached-view leader
/// behavior where the follow-up key always clears the leader.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dashboard_double_ctrl_x_clears_leader() {
    use crossterm::event::{KeyCode, KeyEvent};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);

    app.view = crate::ui::View::Dashboard;
    app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
    app.select_index(0);

    // First Ctrl-X: arms.
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();
    assert!(app.leader_pending, "first Ctrl-X must arm leader");

    // Second Ctrl-X: must cancel (clear) the leader, not stay armed.
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();
    assert!(
        !app.leader_pending,
        "second Ctrl-X must cancel the chord, not re-arm"
    );
}

/// Ctrl-X then 'a' opens the AgentsPanel modal for the selected workspace
/// on the dashboard.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dashboard_ctrl_x_then_a_opens_agents_panel() {
    use crate::ui::modal::Modal;
    use crossterm::event::{KeyCode, KeyEvent};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);

    app.view = crate::ui::View::Dashboard;
    app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
    app.select_index(0);
    app.modal = None;

    // Ctrl-X — arms the leader.
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();
    assert!(app.leader_pending, "Ctrl-X must arm leader_pending");

    // 'a' — must open AgentsPanel for the selected workspace.
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(!app.leader_pending, "leader must clear after 'a'");
    match &app.modal {
        Some(Modal::AgentsPanel {
            workspace_id,
            selected,
        }) => {
            assert_eq!(
                *workspace_id, ws_id,
                "AgentsPanel must reference the selected workspace"
            );
            assert_eq!(*selected, 0);
        }
        other => panic!("expected AgentsPanel modal; got {other:?}"),
    }
}

/// Ctrl-X then 'a' opens the AgentsPanel modal for the focused pane's
/// workspace in the attached view.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attached_ctrl_x_then_a_opens_agents_panel() {
    use crate::ui::modal::Modal;
    use crossterm::event::{KeyCode, KeyEvent};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);
    let target = test_target(&app, ws_id);
    app.modal = None;

    // Ctrl-X — arms the leader.
    handle_key_attached(
        &mut app,
        target,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();
    assert!(app.leader_pending, "Ctrl-X must arm leader_pending");

    // 'a' — must open AgentsPanel for the focused workspace.
    handle_key_attached(
        &mut app,
        target,
        KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(!app.leader_pending, "leader must clear after 'a'");
    match &app.modal {
        Some(Modal::AgentsPanel {
            workspace_id,
            selected,
        }) => {
            assert_eq!(
                *workspace_id, ws_id,
                "AgentsPanel must reference the focused workspace"
            );
            assert_eq!(*selected, 0);
        }
        other => panic!("expected AgentsPanel modal; got {other:?}"),
    }
}

#[tokio::test]
async fn z_alone_arms_leader_without_action() {
    let (mut app, _) = make_app_with_n_repos(2);
    let folded_before = app.dashboard.folded.clone();
    press(&mut app, 'z', KeyModifiers::NONE).await;
    assert!(app.z_leader_pending, "z should arm the leader");
    assert_eq!(
        app.dashboard.folded, folded_before,
        "z alone should not change fold state"
    );
}

#[tokio::test]
async fn zz_toggles_focused_repo_fold() {
    let (mut app, ids) = make_app_with_n_repos(2);
    app.select_index(0);
    let rid = ids[0];
    let key = rid.0 as u64;
    let before = app.dashboard.folded.get(&key).copied();
    press(&mut app, 'z', KeyModifiers::NONE).await;
    press(&mut app, 'z', KeyModifiers::NONE).await;
    assert!(!app.z_leader_pending, "leader should clear after zz");
    let after = app.dashboard.folded.get(&key).copied();
    assert_ne!(
        before, after,
        "zz should change the fold state for the focused repo"
    );
}

#[tokio::test]
async fn za_expands_all_repos() {
    let (mut app, ids) = make_app_with_n_repos(3);
    // Pre-fold one repo explicitly so we can see the "expand all" override.
    app.dashboard.folded.insert(ids[0].0 as u64, true);
    press(&mut app, 'z', KeyModifiers::NONE).await;
    press(&mut app, 'a', KeyModifiers::NONE).await;
    assert!(!app.z_leader_pending, "leader should clear after za");
    for id in &ids {
        let key = id.0 as u64;
        assert_eq!(
            app.dashboard.folded.get(&key).copied(),
            Some(false),
            "za should set repo {key} to expanded (false)"
        );
    }
}

#[tokio::test]
async fn z_shift_m_folds_all_repos() {
    let (mut app, ids) = make_app_with_n_repos(3);
    // Pre-expand one repo explicitly so we can see the "fold all" override.
    app.dashboard.folded.insert(ids[0].0 as u64, false);
    press(&mut app, 'z', KeyModifiers::NONE).await;
    press(&mut app, 'M', KeyModifiers::SHIFT).await;
    assert!(!app.z_leader_pending, "leader should clear after zM");
    for id in &ids {
        let key = id.0 as u64;
        assert_eq!(
            app.dashboard.folded.get(&key).copied(),
            Some(true),
            "zM should set repo {key} to folded (true)"
        );
    }
}

#[tokio::test]
async fn z_then_unknown_clears_leader_without_action() {
    let (mut app, _) = make_app_with_n_repos(2);
    let selected_before = app.dashboard.selected;
    let folded_before = app.dashboard.folded.clone();
    press(&mut app, 'z', KeyModifiers::NONE).await;
    press(&mut app, 'x', KeyModifiers::NONE).await;
    assert!(
        !app.z_leader_pending,
        "leader should clear after unknown key"
    );
    assert_eq!(
        app.dashboard.folded, folded_before,
        "unknown follow-up should leave fold state unchanged"
    );
    assert_eq!(
        app.dashboard.selected, selected_before,
        "unknown follow-up should be eaten, not pass through to selection"
    );
}

#[tokio::test]
async fn z_then_esc_clears_leader() {
    let (mut app, _) = make_app_with_n_repos(2);
    let folded_before = app.dashboard.folded.clone();
    press(&mut app, 'z', KeyModifiers::NONE).await;
    press_key(&mut app, KeyCode::Esc).await;
    assert!(!app.z_leader_pending, "Esc should clear the leader");
    assert_eq!(
        app.dashboard.folded, folded_before,
        "Esc should not change fold state"
    );
}

#[tokio::test]
async fn z_m_folds_all_repos_without_shift_modifier() {
    // Some terminals (or CapsLock) report `Char('M')` without
    // KeyModifiers::SHIFT. The chord should still fire — matches
    // the codebase convention for capital-letter binds like `G`.
    let (mut app, ids) = make_app_with_n_repos(3);
    app.dashboard.folded.insert(ids[0].0 as u64, false);
    press(&mut app, 'z', KeyModifiers::NONE).await;
    press(&mut app, 'M', KeyModifiers::NONE).await;
    assert!(!app.z_leader_pending, "leader should clear after zM");
    for id in &ids {
        assert_eq!(
            app.dashboard.folded.get(&(id.0 as u64)).copied(),
            Some(true),
            "zM (no SHIFT) should fold every repo"
        );
    }
}

#[tokio::test]
async fn zm_folds_all_repos() {
    // Vim `zm` (lowercase m) should fold all repos, same as `zM`.
    let (mut app, ids) = make_app_with_n_repos(3);
    app.dashboard.folded.insert(ids[0].0 as u64, false);
    press(&mut app, 'z', KeyModifiers::NONE).await;
    press(&mut app, 'm', KeyModifiers::NONE).await;
    assert!(!app.z_leader_pending, "leader should clear after zm");
    for id in &ids {
        assert_eq!(
            app.dashboard.folded.get(&(id.0 as u64)).copied(),
            Some(true),
            "zm should set repo {id:?} to folded (true)"
        );
    }
}

#[tokio::test]
async fn zr_expands_all_repos() {
    // Vim `zr` (lowercase r) should expand all repos, same as `za`.
    let (mut app, ids) = make_app_with_n_repos(3);
    app.dashboard.folded.insert(ids[0].0 as u64, true);
    press(&mut app, 'z', KeyModifiers::NONE).await;
    press(&mut app, 'r', KeyModifiers::NONE).await;
    assert!(!app.z_leader_pending, "leader should clear after zr");
    for id in &ids {
        assert_eq!(
            app.dashboard.folded.get(&(id.0 as u64)).copied(),
            Some(false),
            "zr should set repo {id:?} to expanded (false)"
        );
    }
}

#[tokio::test]
async fn z_shift_r_expands_all_repos() {
    // Vim `zR` (uppercase R) should also expand all repos.
    let (mut app, ids) = make_app_with_n_repos(3);
    app.dashboard.folded.insert(ids[0].0 as u64, true);
    press(&mut app, 'z', KeyModifiers::NONE).await;
    press(&mut app, 'R', KeyModifiers::SHIFT).await;
    assert!(!app.z_leader_pending, "leader should clear after zR");
    for id in &ids {
        assert_eq!(
            app.dashboard.folded.get(&(id.0 as u64)).copied(),
            Some(false),
            "zR should set repo {id:?} to expanded (false)"
        );
    }
}
