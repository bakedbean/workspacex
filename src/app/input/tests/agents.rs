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
