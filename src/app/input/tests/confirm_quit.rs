//! confirm quit tests.

use super::*;
use crate::data::store::{NewWorkspace, SetupStatus, Store, WorkspaceState};
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn y_persists_cancelled_for_every_in_flight_create_before_quitting() {
    let store = Store::open_in_memory().unwrap();
    let repo_id = store
        .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
        .unwrap();
    // One create still mid-fetch (setup_status is still the default —
    // `Running` was never written yet, the exact case the startup sweep
    // cannot repair), and one create already past that point.
    let fetching = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "fetching",
            branch: "repo/fetching",
            worktree_path: std::path::Path::new("/tmp/wsx-test/fetching"),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    let running = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "running",
            branch: "repo/running",
            worktree_path: std::path::Path::new("/tmp/wsx-test/running"),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    store
        .set_workspace_state(running, WorkspaceState::Ready)
        .unwrap();
    store
        .set_setup_status(running, SetupStatus::Running)
        .unwrap();

    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let fetching_token = tokio_util::sync::CancellationToken::new();
    app.in_flight.insert(
        fetching,
        crate::data::in_flight::InFlight::create(
            crate::data::progress::SetupProgress::shared(),
            fetching_token.clone(),
        ),
    );
    let running_token = tokio_util::sync::CancellationToken::new();
    app.in_flight.insert(
        running,
        crate::data::in_flight::InFlight::create(
            crate::data::progress::SetupProgress::shared(),
            running_token.clone(),
        ),
    );
    app.modal = Some(Modal::ConfirmQuit {
        creates: 2,
        archives: 0,
    });
    let shared_app = shared();

    handle_key_modal(
        &mut app,
        &shared_app,
        KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE),
    )
    .await
    .unwrap();

    assert!(app.quit, "quit must be set");
    assert!(fetching_token.is_cancelled());
    assert!(running_token.is_cancelled());
    assert_eq!(
        app.store
            .workspace_by_id(fetching)
            .unwrap()
            .unwrap()
            .setup_status,
        SetupStatus::Cancelled,
        "a create still in its fetch phase (never wrote Running) must \
         still land on Cancelled, not be left for a startup sweep that \
         only repairs Running rows"
    );
    assert_eq!(
        app.store
            .workspace_by_id(running)
            .unwrap()
            .unwrap()
            .setup_status,
        SetupStatus::Cancelled,
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn y_does_not_touch_archive_rows() {
    let store = Store::open_in_memory().unwrap();
    let repo_id = store
        .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
        .unwrap();
    let archiving = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "archiving",
            branch: "repo/archiving",
            worktree_path: std::path::Path::new("/tmp/wsx-test/archiving"),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    store
        .set_workspace_state(archiving, WorkspaceState::Ready)
        .unwrap();
    store.set_setup_status(archiving, SetupStatus::Ok).unwrap();

    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.in_flight.insert(
        archiving,
        crate::data::in_flight::InFlight::archive(
            crate::data::progress::SetupProgress::shared(),
            tokio_util::sync::CancellationToken::new(),
        ),
    );
    app.modal = Some(Modal::ConfirmQuit {
        creates: 0,
        archives: 1,
    });
    let shared_app = shared();

    handle_key_modal(
        &mut app,
        &shared_app,
        KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE),
    )
    .await
    .unwrap();

    assert!(app.quit);
    // Archive has no cancellation and is simply abandoned — its row's
    // setup_status (a `create` concept) must be untouched.
    assert_eq!(
        app.store
            .workspace_by_id(archiving)
            .unwrap()
            .unwrap()
            .setup_status,
        SetupStatus::Ok,
        "quitting must not rewrite an archiving row's setup_status"
    );
}
