//! new workspace notice tests.

use super::*;
use crate::data::store::{NewWorkspace, Store};
use crossterm::event::KeyEvent;
use std::path::PathBuf;

fn app_with_existing_workspace() -> (App, crate::data::store::RepoId) {
    let store = Store::open_in_memory().unwrap();
    let repo_id = store
        .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
        .unwrap();
    store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "alpha",
            branch: "repo/alpha",
            worktree_path: std::path::Path::new("/tmp/r/alpha"),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    (app, repo_id)
}

fn dummy_shared() -> std::sync::Arc<tokio::sync::Mutex<App>> {
    std::sync::Arc::new(tokio::sync::Mutex::new(
        App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap(),
    ))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enter_with_a_taken_name_shows_inline_notice_and_does_not_spawn() {
    let (mut app, repo_id) = app_with_existing_workspace();
    app.modal = Some(crate::ui::modal::Modal::NewWorkspace {
        repo_id,
        name_buffer: "alpha".to_string(),
        yolo: false,
        shared: false,
        agent: crate::pty::session::AgentKind::Claude,
        notice: None,
    });
    let shared = dummy_shared();
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    match &app.modal {
        Some(crate::ui::modal::Modal::NewWorkspace {
            name_buffer,
            notice,
            ..
        }) => {
            assert_eq!(name_buffer, "alpha", "buffer must survive the refusal");
            assert_eq!(
                notice.as_deref(),
                Some("a workspace named 'alpha' already exists")
            );
        }
        other => panic!("expected NewWorkspace modal with a notice, got {other:?}"),
    }
    assert!(
        app.in_flight.is_empty(),
        "a duplicate name must never spawn a create task"
    );
    assert_eq!(
        app.store.workspaces(repo_id).unwrap().len(),
        1,
        "no second row should be inserted"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn typing_after_a_duplicate_notice_clears_it() {
    let (mut app, repo_id) = app_with_existing_workspace();
    // Simulate: user already hit a duplicate once (notice set), then
    // starts editing the buffer — mirrors `RenameWorkspace`'s Backspace/
    // Char arms, which clear `notice` on any edit.
    app.modal = Some(crate::ui::modal::Modal::NewWorkspace {
        repo_id,
        name_buffer: "alpha".to_string(),
        yolo: false,
        shared: false,
        agent: crate::pty::session::AgentKind::Claude,
        notice: Some("a workspace named 'alpha' already exists".to_string()),
    });
    let shared = dummy_shared();
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    match &app.modal {
        Some(crate::ui::modal::Modal::NewWorkspace {
            name_buffer,
            notice,
            ..
        }) => {
            assert_eq!(name_buffer, "alpha2");
            assert!(notice.is_none(), "editing must clear the stale notice");
        }
        other => panic!("expected NewWorkspace modal, got {other:?}"),
    }
}
