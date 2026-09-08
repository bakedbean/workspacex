//! The agent picker and missing-agent modals.

use super::*;
use crate::data::store::Store;
use crate::test_support::EnvGuard;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::path::PathBuf;
// `dashboard_renders_split_with_pm_title_when_visible_even_without_session`
// (the PTY-placeholder render test) is gone — the dashboard's PM pane
// now always renders the digest (`render_digest`), whose own render
// tests live in `src/ui/pm_pane.rs::digest_tests`.

use crossterm::event::{KeyEvent, KeyModifiers};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ensure_workspace_session_sets_modal_when_binary_missing() {
    use crate::data::store::{NewWorkspace, WorkspaceState};
    use crate::pty::session::AgentKind;
    let mut env = EnvGuard::new();
    env.set("WSX_HERMES_BIN", "/nonexistent/wsx-test-hermes");
    let store = Store::open_in_memory().unwrap();
    let repo_id = store
        .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
        .unwrap();
    let id = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "ws",
            branch: "repo/ws",
            worktree_path: std::path::Path::new("/tmp/wsx-test/ws"),
            yolo: false,
            agent: AgentKind::Hermes,
            shared: false,
        })
        .unwrap();
    store
        .set_workspace_state(id, WorkspaceState::Ready)
        .unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let outcome = crate::app::ensure_workspace_session(&mut app, id).unwrap();
    assert!(matches!(outcome, crate::app::AttachReady::AgentMissing));
    match app.modal {
        Some(crate::ui::modal::Modal::AgentMissing {
            ws_id,
            agent,
            ref binary,
        }) => {
            assert_eq!(ws_id, id);
            assert_eq!(agent, AgentKind::Hermes);
            assert_eq!(binary, "/nonexistent/wsx-test-hermes");
        }
        ref other => panic!("expected AgentMissing modal, got {other:?}"),
    }
}

#[test]
fn agent_missing_modal_renders_binary_name() {
    use crate::pty::session::AgentKind;
    use crate::ui::modal::Modal;
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.modal = Some(Modal::AgentMissing {
        ws_id: crate::data::store::WorkspaceId(1),
        agent: AgentKind::Hermes,
        binary: "/nonexistent/hermes".to_string(),
    });
    let backend = TestBackend::new(80, 24);
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
        rendered.contains("Hermes is not installed")
            || rendered.contains("hermes is not installed"),
        "expected 'Hermes is not installed' line:\n{rendered}"
    );
    assert!(
        rendered.contains("/nonexistent/hermes"),
        "expected binary path in modal body:\n{rendered}"
    );
    assert!(
        rendered.contains('s') && rendered.contains("switch agent"),
        "expected switch-agent hint:\n{rendered}"
    );
}

#[test]
fn agent_picker_modal_renders_four_agents_with_current_marker() {
    use crate::pty::session::AgentKind;
    use crate::ui::modal::Modal;
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.modal = Some(Modal::AgentPicker {
        ws_id: crate::data::store::WorkspaceId(1),
        selected: 0,
        current: AgentKind::Hermes,
    });
    let backend = TestBackend::new(80, 24);
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
        rendered.contains("claude"),
        "expected claude row: {rendered}"
    );
    assert!(rendered.contains("pi"), "expected pi row: {rendered}");
    assert!(
        rendered.contains("hermes"),
        "expected hermes row: {rendered}"
    );
    assert!(rendered.contains("codex"), "expected codex row: {rendered}");
    assert!(
        rendered.contains("current"),
        "expected current marker: {rendered}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_missing_modal_esc_dismisses() {
    use crate::pty::session::AgentKind;
    use crate::ui::modal::Modal;
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.modal = Some(Modal::AgentMissing {
        ws_id: crate::data::store::WorkspaceId(1),
        agent: AgentKind::Hermes,
        binary: "hermes".to_string(),
    });
    let shared = Arc::new(Mutex::new(
        App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap(),
    ));
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(crossterm::event::KeyCode::Esc, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(app.modal.is_none(), "Esc should dismiss AgentMissing");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_missing_modal_s_opens_picker_with_current_preselected() {
    use crate::pty::session::AgentKind;
    use crate::ui::modal::Modal;
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = crate::data::store::WorkspaceId(42);
    app.modal = Some(Modal::AgentMissing {
        ws_id,
        agent: AgentKind::Hermes,
        binary: "hermes".to_string(),
    });
    let shared = Arc::new(Mutex::new(
        App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap(),
    ));
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(crossterm::event::KeyCode::Char('s'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    match app.modal {
        Some(Modal::AgentPicker {
            ws_id: picker_ws,
            selected,
            current,
        }) => {
            assert_eq!(picker_ws, ws_id);
            assert_eq!(current, AgentKind::Hermes);
            assert_eq!(AgentKind::ALL[selected], AgentKind::Hermes);
        }
        ref other => panic!("expected AgentPicker, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_picker_down_advances_and_clamps() {
    use crate::pty::session::AgentKind;
    use crate::ui::modal::Modal;
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.modal = Some(Modal::AgentPicker {
        ws_id: crate::data::store::WorkspaceId(1),
        selected: 0,
        current: AgentKind::Claude,
    });
    let shared = Arc::new(Mutex::new(
        App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap(),
    ));

    // Derived from `AgentKind::ALL` rather than hardcoded: step down
    // through every index, then press once more to prove the clamp. A
    // literal list here silently broke when a fifth agent kind was added.
    let last = AgentKind::ALL.len() - 1;
    let steps: Vec<usize> = (1..=last).chain(std::iter::once(last)).collect();
    for expected in steps {
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(crossterm::event::KeyCode::Down, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        match app.modal {
            Some(Modal::AgentPicker { selected, .. }) => {
                assert_eq!(selected, expected, "Down step");
            }
            ref other => panic!("expected AgentPicker, got {other:?}"),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_picker_enter_persists_and_retries_attach() {
    use crate::data::store::{NewWorkspace, WorkspaceState};
    use crate::pty::session::AgentKind;
    use crate::test_support::EnvGuard;
    use crate::ui::modal::Modal;
    // Switch from broken Hermes (won't spawn) to Claude (substituted with `cat`,
    // which spawns fine), so the retry attach succeeds.
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
            worktree_path: std::path::Path::new("."),
            yolo: false,
            agent: AgentKind::Hermes,
            shared: false,
        })
        .unwrap();
    store
        .set_workspace_state(id, WorkspaceState::Ready)
        .unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let claude_idx = AgentKind::ALL
        .iter()
        .position(|k| *k == AgentKind::Claude)
        .unwrap();
    app.modal = Some(Modal::AgentPicker {
        ws_id: id,
        selected: claude_idx,
        current: AgentKind::Hermes,
    });
    let shared = Arc::new(Mutex::new(
        App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap(),
    ));
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(crossterm::event::KeyCode::Enter, KeyModifiers::NONE),
    )
    .await
    .unwrap();

    // Store now reports Claude.
    let stored = app
        .store
        .workspaces(repo_id)
        .unwrap()
        .into_iter()
        .find(|w| w.id == id)
        .expect("workspace present");
    assert_eq!(stored.agent, AgentKind::Claude);
    // In-memory mirror also updated.
    let mem = app
        .workspaces
        .iter()
        .find(|(_, w)| w.id == id)
        .expect("workspace in memory")
        .1
        .clone();
    assert_eq!(mem.agent, AgentKind::Claude);
    // A session exists.
    assert!(
        app.sessions.get(test_primary_instance(&app, id)).is_some(),
        "session should be alive"
    );
    // Modal closed.
    assert!(app.modal.is_none(), "modal should be cleared on success");
}

/// Fixture for the agents-panel `x` (remove peer) tests: one workspace
/// with a primary Claude instance and one Codex peer, both holding a
/// fake running session so the attached view has something to draw.
/// Returns `(app, ws, primary, peer)`.
fn app_with_primary_and_peer() -> (
    App,
    crate::data::store::WorkspaceId,
    crate::data::store::AgentInstanceId,
    crate::data::store::AgentInstanceId,
) {
    use crate::pty::session::{AgentKind, SessionStatus};
    let mut app = App::new(
        Store::open_in_memory().unwrap(),
        PathBuf::from("/tmp/wsx-test"),
    )
    .unwrap();
    let ws = app.test_workspace("peer-remove");
    let primary = app
        .store
        .add_primary_agent(ws, AgentKind::Claude, 1)
        .unwrap()
        .id;
    let peer = app
        .store
        .add_workspace_agent(ws, AgentKind::Codex)
        .unwrap()
        .id;
    app.test_spawn_session(primary, SessionStatus::Running { pid: 1 });
    app.test_spawn_session(peer, SessionStatus::Running { pid: 2 });
    app.refresh().unwrap();
    (app, ws, primary, peer)
}

fn target(
    ws: crate::data::store::WorkspaceId,
    instance: crate::data::store::AgentInstanceId,
) -> crate::ui::split::AttachTarget {
    crate::ui::split::AttachTarget {
        workspace_id: ws,
        instance,
    }
}

async fn press_x_in_agents_panel(app: &mut App, ws: crate::data::store::WorkspaceId) {
    use crate::ui::modal::Modal;
    app.modal = Some(Modal::AgentsPanel {
        workspace_id: ws,
        selected: 0,
    });
    let shared = Arc::new(Mutex::new(
        App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap(),
    ));
    handle_key_modal(
        app,
        &shared,
        KeyEvent::new(crossterm::event::KeyCode::Char('x'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
}

/// The removed peer's pane must be dropped from the split tree in the
/// same keystroke, and the panel dismissed. Without the prune the next
/// `draw_attached` sees a leaf with no session and bounces the whole
/// view to the dashboard.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agents_panel_x_dismisses_modal_and_drops_removed_pane() {
    let (mut app, ws, primary, peer) = app_with_primary_and_peer();
    let mut state = crate::ui::AttachedState::single(target(ws, primary));
    assert!(state.split(SplitDirection::Vertical, target(ws, peer)));
    // Focus back on the primary pane.
    state.focus = vec![0];
    app.view = View::Attached(state);

    press_x_in_agents_panel(&mut app, ws).await;

    assert!(
        app.modal.is_none(),
        "agents panel should close after removal"
    );
    assert!(app.sessions.get(peer).is_none(), "peer session killed");
    let View::Attached(state) = &app.view else {
        panic!("expected to stay attached, got {:?}", app.view);
    };
    assert_eq!(state.leaves(), vec![target(ws, primary)]);
    assert_eq!(state.focused_target(), Some(target(ws, primary)));
}

/// When the focused pane is the one being removed, focus moves to a
/// surviving pane in the same workspace rather than dangling.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agents_panel_x_moves_focus_off_removed_pane() {
    let (mut app, ws, primary, peer) = app_with_primary_and_peer();
    let mut state = crate::ui::AttachedState::single(target(ws, primary));
    assert!(state.split(SplitDirection::Vertical, target(ws, peer)));
    // `split` focuses the new (peer) pane.
    assert_eq!(state.focused_target(), Some(target(ws, peer)));
    app.view = View::Attached(state);

    press_x_in_agents_panel(&mut app, ws).await;

    assert!(app.modal.is_none());
    let View::Attached(state) = &app.view else {
        panic!("expected to stay attached, got {:?}", app.view);
    };
    assert_eq!(state.focused_target(), Some(target(ws, primary)));
}

/// If the removed peer was the only pane, re-target to the workspace's
/// primary instead of falling out to the dashboard.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agents_panel_x_on_sole_pane_falls_back_to_primary() {
    let (mut app, ws, primary, peer) = app_with_primary_and_peer();
    app.view = View::Attached(crate::ui::AttachedState::single(target(ws, peer)));

    press_x_in_agents_panel(&mut app, ws).await;

    assert!(app.modal.is_none());
    let View::Attached(state) = &app.view else {
        panic!("expected to stay attached, got {:?}", app.view);
    };
    assert_eq!(state.focused_target(), Some(target(ws, primary)));
}

/// The sole-pane fallback must be a plain single-pane attach to the
/// primary, NOT `attach_workspace`: that would restore the saved layout,
/// re-spawning its side panes (and ejecting to the dashboard if one can't
/// spawn). The saved layout itself is left alone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agents_panel_x_on_sole_pane_ignores_saved_layout() {
    let (mut app, ws, primary, peer) = app_with_primary_and_peer();
    // Saved layout: (primary | other-workspace primary), focused on the
    // second pane. The second workspace has no session, so restoring
    // this layout would leave a sessionless leaf.
    let other_ws = app.test_workspace("peer-remove-other");
    let other_primary = app
        .store
        .add_primary_agent(other_ws, crate::pty::session::AgentKind::Claude, 1)
        .unwrap()
        .id;
    let mut saved = crate::ui::AttachedState::single(target(ws, primary));
    assert!(saved.split(SplitDirection::Vertical, target(other_ws, other_primary)));
    app.store
        .set_workspace_layout(ws, &saved.tree, &saved.focus)
        .unwrap();
    app.view = View::Attached(crate::ui::AttachedState::single(target(ws, peer)));

    press_x_in_agents_panel(&mut app, ws).await;

    let View::Attached(state) = &app.view else {
        panic!("expected to stay attached, got {:?}", app.view);
    };
    assert_eq!(state.leaves(), vec![target(ws, primary)]);
    assert_eq!(state.focused_target(), Some(target(ws, primary)));
    // Saved layout untouched.
    let (tree, _) = app
        .store
        .get_workspace_layout(ws)
        .unwrap()
        .expect("saved layout still present");
    assert_eq!(tree.leaves(), saved.leaves());
}

/// If the primary can't be spawned for the sole-pane fallback, the
/// AgentMissing modal replaces the panel and the view falls to the
/// dashboard rather than an unrenderable empty tree.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agents_panel_x_on_sole_pane_with_unspawnable_primary_shows_agent_missing() {
    use crate::pty::session::{AgentKind, SessionStatus};
    let mut env = EnvGuard::new();
    env.set("WSX_CLAUDE_BIN", "/nonexistent/wsx-test-claude");
    let mut app = App::new(
        Store::open_in_memory().unwrap(),
        PathBuf::from("/tmp/wsx-test"),
    )
    .unwrap();
    let ws = app.test_workspace("peer-remove-missing");
    app.store
        .set_workspace_state(ws, crate::data::store::WorkspaceState::Ready)
        .unwrap();
    let _primary = app
        .store
        .add_primary_agent(ws, AgentKind::Claude, 1)
        .unwrap()
        .id;
    let peer = app
        .store
        .add_workspace_agent(ws, AgentKind::Codex)
        .unwrap()
        .id;
    // Only the peer has a session; the primary has none and can't spawn.
    app.test_spawn_session(peer, SessionStatus::Running { pid: 2 });
    app.refresh().unwrap();
    app.view = View::Attached(crate::ui::AttachedState::single(target(ws, peer)));

    press_x_in_agents_panel(&mut app, ws).await;

    assert!(
        matches!(
            app.modal,
            Some(crate::ui::modal::Modal::AgentMissing { ws_id, .. }) if ws_id == ws
        ),
        "expected AgentMissing modal, got {:?}",
        app.modal
    );
    assert!(
        matches!(app.view, View::Dashboard),
        "expected dashboard, got {:?}",
        app.view
    );
}
