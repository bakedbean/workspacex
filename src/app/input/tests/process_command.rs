//! process command tests.

use super::*;
use crate::data::store::{Store, WorkspaceId};
use crate::ui::modal::Modal;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

fn shared() -> SharedApp {
    Arc::new(Mutex::new(
        App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap(),
    ))
}

fn process_list(input: Option<String>) -> Modal {
    Modal::ProcessList {
        workspace_id: WorkspaceId(1),
        selected: 0,
        input,
        notice: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r_enters_input_mode() {
    let mut app = App::new(
        Store::open_in_memory().unwrap(),
        PathBuf::from("/tmp/wsx-test"),
    )
    .unwrap();
    app.modal = Some(process_list(None));
    let shared = shared();
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(matches!(
        app.modal,
        Some(Modal::ProcessList { input: Some(ref b), .. }) if b.is_empty()
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn typing_appends_and_backspace_pops() {
    let mut app = App::new(
        Store::open_in_memory().unwrap(),
        PathBuf::from("/tmp/wsx-test"),
    )
    .unwrap();
    app.modal = Some(process_list(Some(String::new())));
    let shared = shared();
    for c in ['l', 's'] {
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        )
        .await
        .unwrap();
    }
    assert!(matches!(
        app.modal,
        Some(Modal::ProcessList { input: Some(ref b), .. }) if b == "ls"
    ));
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(matches!(
        app.modal,
        Some(Modal::ProcessList { input: Some(ref b), .. }) if b == "l"
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn esc_in_input_mode_returns_to_list_mode() {
    let mut app = App::new(
        Store::open_in_memory().unwrap(),
        PathBuf::from("/tmp/wsx-test"),
    )
    .unwrap();
    app.modal = Some(process_list(Some("npm".to_string())));
    let shared = shared();
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(matches!(
        app.modal,
        Some(Modal::ProcessList { input: None, .. })
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enter_with_empty_command_is_a_noop() {
    let mut app = App::new(
        Store::open_in_memory().unwrap(),
        PathBuf::from("/tmp/wsx-test"),
    )
    .unwrap();
    app.modal = Some(process_list(Some("   ".to_string())));
    let shared = shared();
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(matches!(
        app.modal,
        Some(Modal::ProcessList { input: Some(ref b), notice: None, .. }) if b == "   "
    ));
}

use ratatui::Terminal;
use ratatui::backend::TestBackend;

#[test]
fn footer_shows_run_hint_in_list_mode() {
    let theme = crate::ui::theme::Theme::default();
    let backend = TestBackend::new(80, 24);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| {
        crate::ui::modal::render_process_list(f, f.area(), "demo", &[], 0, None, None, &theme);
    })
    .unwrap();
    let buf = term.backend().buffer();
    let rendered = (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("[r] run"), "{rendered}");
}

#[test]
fn footer_shows_input_prompt_in_input_mode() {
    let theme = crate::ui::theme::Theme::default();
    let backend = TestBackend::new(80, 24);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| {
        crate::ui::modal::render_process_list(
            f,
            f.area(),
            "demo",
            &[],
            0,
            Some("cargo run"),
            None,
            &theme,
        );
    })
    .unwrap();
    let buf = term.backend().buffer();
    let rendered = (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("run: cargo run"), "{rendered}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn question_mark_ignored_without_workspace_selection() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let k = KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE);

    // 1. No selection (empty selectable, selection = None).
    handle_key_dashboard(&mut app, k).await.unwrap();
    assert!(
        app.modal.is_none(),
        "? with no selection should not open WorkspaceActions, got {:?}",
        app.modal
    );

    // 2. Repo selected — ? must be a no-op.
    app.selectable = vec![SelectionTarget::Repo(crate::data::store::RepoId(1))];
    app.select_index(0);
    handle_key_dashboard(&mut app, k).await.unwrap();
    assert!(
        app.modal.is_none(),
        "? with a repo selected should not open WorkspaceActions, got {:?}",
        app.modal
    );

    // 3. Workspace selected — positive control.
    app.selectable = vec![SelectionTarget::Workspace(WorkspaceId(1))];
    app.select_index(0);
    handle_key_dashboard(&mut app, k).await.unwrap();
    assert!(
        matches!(app.modal, Some(Modal::WorkspaceActions)),
        "? with workspace selected should open WorkspaceActions, got {:?}",
        app.modal
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn question_mark_opens_and_closes_workspace_actions_overlay() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.selectable = vec![SelectionTarget::Workspace(WorkspaceId(1))];
    app.select_index(0);

    let open = KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE);
    handle_key_dashboard(&mut app, open).await.unwrap();
    assert!(
        matches!(app.modal, Some(Modal::WorkspaceActions)),
        "expected WorkspaceActions modal open, got {:?}",
        app.modal
    );

    // Verify Esc closes it.
    let shared = Arc::new(Mutex::new(
        App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap(),
    ));
    let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    handle_key_modal(&mut app, &shared, esc).await.unwrap();
    assert!(app.modal.is_none(), "expected overlay dismissed on Esc");

    // Verify '?' also toggles the overlay closed while it is open.
    let open2 = KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE);
    handle_key_dashboard(&mut app, open2).await.unwrap();
    assert!(
        matches!(app.modal, Some(Modal::WorkspaceActions)),
        "expected WorkspaceActions modal open on second open, got {:?}",
        app.modal
    );
    let close_q = KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE);
    handle_key_modal(&mut app, &shared, close_q).await.unwrap();
    assert!(app.modal.is_none(), "expected overlay dismissed on '?'");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workspace_actions_overlay_navigates_and_dismisses() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let shared = shared();
    app.selectable = vec![SelectionTarget::Workspace(WorkspaceId(1))];
    app.select_index(0);

    // 1. Open the overlay with '?'.
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(
        matches!(app.modal, Some(Modal::WorkspaceActions)),
        "expected WorkspaceActions modal open, got {:?}",
        app.modal
    );

    // 2. Down via handle_key_modal keeps the card open.
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(
        matches!(app.modal, Some(Modal::WorkspaceActions)),
        "Down should keep WorkspaceActions overlay open, got {:?}",
        app.modal
    );

    // 3. Action key 'c' closes the card (no workspace selected, so the
    //    action itself no-ops — the important thing is the overlay closes).
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(
        app.modal.is_none(),
        "action key 'c' should close the overlay"
    );

    // 4. Re-open with '?', then Enter closes the card.
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(
        matches!(app.modal, Some(Modal::WorkspaceActions)),
        "expected WorkspaceActions modal open again, got {:?}",
        app.modal
    );
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    // With no workspace selected, Enter is a no-op on the dashboard and sets
    // no new modal. Assert that the WorkspaceActions card at minimum is gone.
    assert!(
        !matches!(app.modal, Some(Modal::WorkspaceActions)),
        "Enter should close the WorkspaceActions overlay, got {:?}",
        app.modal
    );
}
