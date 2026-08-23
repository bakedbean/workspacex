//! Creating, archiving, and sharing a workspace from the UI.

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

use super::common::*;
use crossterm::event::{KeyEvent, KeyModifiers};

#[tokio::test]
async fn i_alias_opens_new_workspace_modal_like_enter_on_repo() {
    // On a repo header, Enter opens the New Workspace modal. `i` (vim
    // insert) should do the same — it's the "enter this thing" verb.
    let (mut app, _) = make_app_with_n_repos(1);
    app.select_index(0);
    assert!(matches!(
        app.selected_target(),
        Some(SelectionTarget::Repo(_))
    ));
    press(&mut app, 'i', KeyModifiers::NONE).await;
    assert!(
        matches!(app.modal, Some(Modal::NewWorkspace { .. })),
        "i on a repo row should open NewWorkspace like Enter; got {:?}",
        app.modal
    );
}

#[tokio::test]
async fn capital_s_opens_new_workspace_modal_with_shared_true() {
    // Capital S opens the NewWorkspace modal pre-set for a tmux-shared
    // workspace, mirroring how capital N pre-sets yolo mode.
    let (mut app, _) = make_app_with_n_repos(1);
    app.select_index(0);
    press(&mut app, 'S', KeyModifiers::SHIFT).await;
    match app.modal {
        Some(Modal::NewWorkspace { shared, yolo, .. }) => {
            assert!(shared, "S should open the modal with shared: true");
            assert!(!yolo, "S should not also enable yolo");
        }
        other => panic!("expected NewWorkspace modal, got {other:?}"),
    }
}

#[tokio::test]
async fn ctrl_s_in_new_workspace_modal_toggles_shared() {
    use crate::ui::modal::Modal;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    let store = Store::open_in_memory().unwrap();
    let repo_dir = init_git_repo();
    let repo_id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
        .await
        .unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    let mut app = App::new(store, tmp.path().to_path_buf()).unwrap();
    app.modal = Some(Modal::NewWorkspace {
        repo_id,
        name_buffer: "alpha".to_string(),
        yolo: false,
        shared: false,
        agent: crate::pty::session::AgentKind::Claude,
        notice: None,
    });
    let shared_app = Arc::new(Mutex::new(
        App::new(Store::open_in_memory().unwrap(), tmp.path().to_path_buf()).unwrap(),
    ));
    let ctrl_s = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
    handle_key_modal(&mut app, &shared_app, ctrl_s)
        .await
        .unwrap();
    match app.modal {
        Some(Modal::NewWorkspace { shared, .. }) => {
            assert!(shared, "Ctrl-s should toggle shared from false to true");
        }
        other => panic!("expected NewWorkspace modal, got {other:?}"),
    }
    // Toggling again flips it back — and plain chars (no Ctrl) still
    // fall through to the name buffer rather than toggling.
    handle_key_modal(&mut app, &shared_app, ctrl_s)
        .await
        .unwrap();
    match &app.modal {
        Some(Modal::NewWorkspace { shared, .. }) => {
            assert!(!shared, "second Ctrl-s should toggle shared back to false");
        }
        other => panic!("expected NewWorkspace modal, got {other:?}"),
    }
    let plain_s = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE);
    handle_key_modal(&mut app, &shared_app, plain_s)
        .await
        .unwrap();
    match app.modal {
        Some(Modal::NewWorkspace {
            shared,
            name_buffer,
            ..
        }) => {
            assert!(!shared, "plain 's' must not toggle shared");
            assert_eq!(
                name_buffer, "alphas",
                "plain 's' should append to the name buffer"
            );
        }
        other => panic!("expected NewWorkspace modal, got {other:?}"),
    }
}

#[tokio::test]
async fn enter_in_new_workspace_modal_backgrounds_create_and_registers_in_flight() {
    use crate::ui::modal::Modal;
    let store = crate::data::store::Store::open_in_memory().unwrap();
    let repo_dir = init_git_repo();
    let repo_id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
        .await
        .unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    let app = Arc::new(Mutex::new(
        App::new(store, tmp.path().to_path_buf()).unwrap(),
    ));
    {
        let mut g = app.lock().await;
        g.modal = Some(Modal::NewWorkspace {
            repo_id,
            name_buffer: "alpha".to_string(),
            yolo: false,
            shared: false,
            agent: crate::pty::session::AgentKind::Claude,
            notice: None,
        });
    }
    // Send Enter.
    let evt = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::empty(),
    );
    {
        let mut g = app.lock().await;
        handle_event(&mut g, &app, CtEvent::Key(evt)).await.unwrap();
        // Create is backgrounded immediately: no modal pops up, so the
        // dashboard is usable right away instead of blocked behind it.
        assert!(
            g.modal.is_none(),
            "create should background without opening a modal; got {:?}",
            g.modal
        );
        assert!(g.pending_create_gen.is_some());
    }
    // Wait for the spawned create task to finish: the pending generation
    // is reset and the workspace materializes.
    wait_until(&app, "create to finish (1 workspace)", |g| {
        g.pending_create_gen.is_none() && g.workspaces.len() == 1
    })
    .await;
    let g = app.lock().await;
    assert!(g.pending_create_gen.is_none());
    assert_eq!(g.workspaces.len(), 1);
    // The reconciler must have removed the finished task's registry entry.
    assert!(
        g.in_flight.is_empty(),
        "in_flight entry should be removed once create finishes"
    );
    let _ = repo_id; // suppress unused warning if not referenced above
}

#[tokio::test]
async fn create_registers_in_flight_and_keeps_running_past_esc() {
    use crate::ui::modal::Modal;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    let store = crate::data::store::Store::open_in_memory().unwrap();
    let repo_dir = init_git_repo();
    let repo_id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
        .await
        .unwrap();
    store
        .set_repo_setup_script(repo_id, Some("sleep 5"))
        .unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    let app = Arc::new(Mutex::new(
        App::new(store, tmp.path().to_path_buf()).unwrap(),
    ));
    // Open the modal and press Enter.
    {
        let mut g = app.lock().await;
        g.modal = Some(Modal::NewWorkspace {
            repo_id,
            name_buffer: "alpha".to_string(),
            yolo: false,
            shared: false,
            agent: crate::pty::session::AgentKind::Claude,
            notice: None,
        });
        let enter = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::empty(),
        );
        handle_event(&mut g, &app, CtEvent::Key(enter))
            .await
            .unwrap();
        assert!(g.modal.is_none(), "create backgrounds without a modal");
    }
    // Wait for Phase 3 to register the in_flight entry now that the
    // workspace id exists.
    wait_until(&app, "in_flight entry to appear", |g| {
        !g.in_flight.is_empty()
    })
    .await;
    // Press Esc — there is no modal open, so this is a no-op; in
    // particular it must NOT cancel the running create (Esc only
    // cancels when it closes a SetupProgress viewer, which requires the
    // modal to be open).
    {
        let mut g = app.lock().await;
        let esc = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::empty(),
        );
        handle_event(&mut g, &app, CtEvent::Key(esc)).await.unwrap();
        assert!(g.modal.is_none());
        assert!(
            g.pending_create_gen.is_some(),
            "create must still be running"
        );
    }
    // Wait for the spawned task to finish normally (not cancelled).
    wait_until(&app, "create to finish successfully", |g| {
        g.workspaces.len() == 1
            && g.workspaces[0].1.setup_status == crate::data::store::SetupStatus::Ok
    })
    .await;
    let g = app.lock().await;
    assert_eq!(g.workspaces.len(), 1);
    assert_eq!(
        g.workspaces[0].1.setup_status,
        crate::data::store::SetupStatus::Ok
    );
    assert!(g.modal.is_none());
    assert!(g.in_flight.is_empty());
}

#[tokio::test]
async fn y_in_confirm_archive_backgrounds_immediately_and_spawns_task() {
    use crate::ui::modal::Modal;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    let store = crate::data::store::Store::open_in_memory().unwrap();
    let repo_dir = init_git_repo();
    let repo_id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
        .await
        .unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    // Create the workspace BEFORE wrapping the store in the App, since
    // App::new takes the store by value.
    let repo = store
        .repos()
        .unwrap()
        .into_iter()
        .find(|r| r.id == repo_id)
        .unwrap();
    let created = crate::data::workspace::create(
        &store,
        &repo,
        Some("doomed"),
        tmp.path(),
        false,
        false,
        crate::pty::session::AgentKind::Claude,
        tokio_util::sync::CancellationToken::new(),
        |_| {},
    )
    .await
    .unwrap();
    let ws_id = created.workspace.id;
    let app = Arc::new(Mutex::new(
        App::new(store, tmp.path().to_path_buf()).unwrap(),
    ));
    // Open the ConfirmArchive modal.
    {
        let mut g = app.lock().await;
        g.modal = Some(Modal::ConfirmArchive {
            workspace_id: ws_id,
            name: created.workspace.name.clone(),
        });
    }
    // Send 'y'.
    let evt = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('y'),
        crossterm::event::KeyModifiers::empty(),
    );
    {
        let mut g = app.lock().await;
        handle_event(&mut g, &app, CtEvent::Key(evt)).await.unwrap();
        // Archive is backgrounded immediately: no modal pops up, and the
        // workspace is registered in in_flight as Archive so the row
        // badges right away.
        assert!(
            g.modal.is_none(),
            "archive should background without opening a modal; got {:?}",
            g.modal
        );
        let f = g.in_flight.get(&ws_id).expect("archive entry registered");
        assert_eq!(f.kind, crate::data::in_flight::InFlightKind::Archive);
        assert!(g.pending_archive_gen.is_some());
    }
    // Wait for the spawned archive task to complete: the in_flight entry
    // and pending generation clear, and the workspace is removed.
    wait_until(
        &app,
        "archive to finish (in_flight cleared, workspace gone)",
        |g| {
            g.in_flight.is_empty()
                && g.pending_archive_gen.is_none()
                && g.workspaces.iter().all(|(_, w)| w.id != ws_id)
        },
    )
    .await;
    let g = app.lock().await;
    assert!(g.in_flight.is_empty());
    assert!(g.pending_archive_gen.is_none());
    assert!(
        g.workspaces.iter().all(|(_, w)| w.id != ws_id),
        "archived workspace should be removed from app.workspaces"
    );
}

#[tokio::test]
async fn y_in_confirm_archive_registers_in_flight_and_keeps_running_past_esc() {
    use crate::ui::modal::Modal;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    let store = crate::data::store::Store::open_in_memory().unwrap();
    let repo_dir = init_git_repo();
    let repo_id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
        .await
        .unwrap();
    // Give the archive a slow archive-script so it's still running
    // when we press Esc.
    store
        .set_repo_archive_script(repo_id, Some("sleep 1"))
        .unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    // Create the workspace before moving the store into the App.
    let repo = store
        .repos()
        .unwrap()
        .into_iter()
        .find(|r| r.id == repo_id)
        .unwrap();
    let created = crate::data::workspace::create(
        &store,
        &repo,
        Some("doomed"),
        tmp.path(),
        false,
        false,
        crate::pty::session::AgentKind::Claude,
        tokio_util::sync::CancellationToken::new(),
        |_| {},
    )
    .await
    .unwrap();
    let ws_id = created.workspace.id;
    let app = Arc::new(Mutex::new(
        App::new(store, tmp.path().to_path_buf()).unwrap(),
    ));
    {
        let mut g = app.lock().await;
        g.modal = Some(Modal::ConfirmArchive {
            workspace_id: ws_id,
            name: created.workspace.name.clone(),
        });
    }
    // Press 'y' to start archiving.
    {
        let mut g = app.lock().await;
        let y = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('y'),
            crossterm::event::KeyModifiers::empty(),
        );
        handle_event(&mut g, &app, CtEvent::Key(y)).await.unwrap();
        assert!(g.modal.is_none(), "archive backgrounds without a modal");
        assert!(g.in_flight.contains_key(&ws_id));
    }
    // Yield briefly so the archive script kicks off but is still
    // running (sleep 1 gives us a 1s window).
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    // Press Esc — there is no modal open, so this is a no-op; in
    // particular it must not disturb the running archive (archive is
    // not cancellable, and there is nothing wired to Esc here anyway).
    {
        let mut g = app.lock().await;
        let esc = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::empty(),
        );
        handle_event(&mut g, &app, CtEvent::Key(esc)).await.unwrap();
        assert!(g.modal.is_none());
        assert!(
            g.in_flight.contains_key(&ws_id),
            "archive must still be running"
        );
    }
    // Wait for the archive to actually finish.
    wait_until(
        &app,
        "archive to finish (in_flight cleared, workspace gone)",
        |g| g.in_flight.is_empty() && g.workspaces.iter().all(|(_, w)| w.id != ws_id),
    )
    .await;
    let g = app.lock().await;
    assert!(g.in_flight.is_empty());
    assert!(
        g.workspaces.iter().all(|(_, w)| w.id != ws_id),
        "workspace should be archived"
    );
}

/// A workspace that already has a live `in_flight` entry — of either
/// kind — must refuse to open the archive-confirm modal on `d`. Without
/// this guard, a second `d`,`y` on a workspace already archiving would
/// replace its `in_flight` entry and spawn a second concurrent archive;
/// the first archive's completion would then remove the entry (and thus
/// the badge and `attach_is_blocked`'s guard) while the second archive
/// is still tearing down the worktree, letting a subsequent attach
/// respawn an agent into a directory being deleted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn d_is_inert_when_an_in_flight_entry_already_exists() {
    use crate::data::in_flight::{InFlight, InFlightKind};
    use crate::data::progress::SetupProgress;
    use crate::data::store::NewWorkspace;

    for kind in [InFlightKind::Create, InFlightKind::Archive] {
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let ws_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "busy",
                branch: "repo/busy",
                worktree_path: std::path::Path::new("/tmp/wsx-busy"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.selectable = vec![SelectionTarget::Workspace(ws_id)];
        app.select_index(0);
        let entry = match kind {
            InFlightKind::Create => InFlight::create(
                SetupProgress::shared(),
                tokio_util::sync::CancellationToken::new(),
            ),
            InFlightKind::Archive => InFlight::archive(
                SetupProgress::shared(),
                tokio_util::sync::CancellationToken::new(),
            ),
        };
        app.in_flight.insert(ws_id, entry);

        handle_key_dashboard(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
        )
        .await
        .unwrap();

        assert!(
            app.modal.is_none(),
            "'d' must not open the archive-confirm modal for a workspace \
             with a {kind:?} entry already in flight; got {:?}",
            app.modal
        );
    }
}

/// Sanity check for the guard above: with no `in_flight` entry for the
/// selected workspace, `d` still opens the archive-confirm modal as
/// normal.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn d_opens_confirm_archive_when_no_entry_in_flight() {
    use crate::data::store::NewWorkspace;
    use crate::ui::modal::Modal;

    let store = Store::open_in_memory().unwrap();
    let repo_id = store
        .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
        .unwrap();
    let ws_id = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "idle",
            branch: "repo/idle",
            worktree_path: std::path::Path::new("/tmp/wsx-idle"),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.selectable = vec![SelectionTarget::Workspace(ws_id)];
    app.select_index(0);
    assert!(app.in_flight.is_empty());

    handle_key_dashboard(
        &mut app,
        KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
    )
    .await
    .unwrap();

    assert!(
        matches!(app.modal, Some(Modal::ConfirmArchive { workspace_id, .. }) if workspace_id == ws_id),
        "'d' with no in-flight work should still open ConfirmArchive; got {:?}",
        app.modal
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capital_t_opens_confirm_share_and_y_flips_shared_and_restarts_session() {
    // T on a selected workspace opens ConfirmShare proposing the flip of
    // the current `shared` flag; `y` commits it via
    // `toggle_workspace_shared`, which restarts any running session so
    // it respawns per the new flag (resuming via --continue).
    //
    // This exercises the *unshare* direction (shared: true -> false):
    // the respawn after unsharing is a plain direct spawn (no tmux
    // binary required), unlike the share direction, whose tmux-backed
    // respawn is covered by the tmux-gated e2e test.
    use crate::data::store::NewWorkspace;
    use crate::ui::modal::Modal;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    let mut env = EnvGuard::new();
    env.set("WSX_CODEX_BIN", crate::test_support::cat_ignore_args_path());

    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let repo_id = app
        .store
        .add_repo(std::path::Path::new("."), "scratch", "test")
        .unwrap();
    let ws_id = app
        .store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "share-toggle-test",
            branch: "main",
            worktree_path: std::path::Path::new("."),
            yolo: false,
            agent: crate::pty::session::AgentKind::Codex,
            shared: true,
        })
        .unwrap();
    let mode = crate::pty::session::SpawnMode::Fresh {
        rename_ctx: None,
        custom_instructions: None,
        doctrine: None,
        additional_dirs: vec![],
        yolo: false,
    };
    let inst = test_primary_instance(&app, ws_id);
    app.sessions
        .spawn(
            inst,
            ws_id,
            std::path::Path::new("."),
            80,
            24,
            mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
            crate::pty::session::AgentKind::Codex,
            None,
        )
        .unwrap();
    app.refresh().unwrap();
    app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
    app.select_index(0);

    let old_session = app.sessions.get(inst).expect("session should be running");

    let shared_app = Arc::new(Mutex::new(app));

    // Press Shift+T: should open ConfirmShare proposing to_shared: false
    // (workspace starts shared: true).
    {
        let mut g = shared_app.lock().await;
        let t = KeyEvent::new(KeyCode::Char('T'), KeyModifiers::SHIFT);
        handle_event(&mut g, &shared_app, CtEvent::Key(t))
            .await
            .unwrap();
        match &g.modal {
            Some(Modal::ConfirmShare {
                workspace_id,
                to_shared,
                ..
            }) => {
                assert_eq!(*workspace_id, ws_id);
                assert!(
                    !*to_shared,
                    "workspace starts shared; T should propose to_shared: false"
                );
            }
            other => panic!("expected ConfirmShare modal, got {other:?}"),
        }
    }

    // Press 'y': flips the store flag and restarts the running instance.
    {
        let mut g = shared_app.lock().await;
        let y = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE);
        handle_event(&mut g, &shared_app, CtEvent::Key(y))
            .await
            .unwrap();
    }

    let g = shared_app.lock().await;
    assert!(
        g.modal.is_none(),
        "y should dismiss ConfirmShare on success; got {:?}",
        g.modal
    );
    let ws = g.store.workspace_by_id(ws_id).unwrap().unwrap();
    assert!(!ws.shared, "y should flip store workspace.shared to false");
    let new_session = g
        .sessions
        .get(inst)
        .expect("instance should have a respawned session");
    assert!(
        !Arc::ptr_eq(&old_session, &new_session),
        "the old session must be gone from app.sessions, replaced by a respawned one"
    );
}

#[tokio::test]
async fn enter_during_setup_running_is_a_noop() {
    use crate::ui::modal::Modal;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    let store = crate::data::store::Store::open_in_memory().unwrap();
    let repo_dir = init_git_repo();
    let repo_id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
        .await
        .unwrap();
    store
        .set_repo_setup_script(repo_id, Some("sleep 1"))
        .unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    let app = Arc::new(Mutex::new(
        App::new(store, tmp.path().to_path_buf()).unwrap(),
    ));
    {
        let mut g = app.lock().await;
        g.modal = Some(Modal::NewWorkspace {
            repo_id,
            name_buffer: "alpha".to_string(),
            yolo: false,
            shared: false,
            agent: crate::pty::session::AgentKind::Claude,
            notice: None,
        });
        let enter = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::empty(),
        );
        handle_event(&mut g, &app, CtEvent::Key(enter))
            .await
            .unwrap();
        // Press Enter again — should not spawn a second create.
        handle_event(&mut g, &app, CtEvent::Key(enter))
            .await
            .unwrap();
        handle_event(&mut g, &app, CtEvent::Key(enter))
            .await
            .unwrap();
    }
    // Wait for the (single) setup to finish. Repeated Enter presses while
    // a create is pending are rejected synchronously, so the count
    // settles at exactly one rather than racing toward duplicates.
    wait_until(&app, "exactly one workspace to be created", |g| {
        g.workspaces.len() == 1
    })
    .await;
    let g = app.lock().await;
    assert_eq!(
        g.workspaces.len(),
        1,
        "exactly one workspace should be created"
    );
}

#[tokio::test]
async fn successful_create_after_esc_does_not_show_error_modal() {
    use crate::ui::modal::Modal;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    let store = crate::data::store::Store::open_in_memory().unwrap();
    let repo_dir = init_git_repo();
    let repo_id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
        .await
        .unwrap();
    // No setup script — create is very fast.
    let tmp = tempfile::TempDir::new().unwrap();
    let app = Arc::new(Mutex::new(
        App::new(store, tmp.path().to_path_buf()).unwrap(),
    ));
    {
        let mut g = app.lock().await;
        g.modal = Some(Modal::NewWorkspace {
            repo_id,
            name_buffer: "alpha".to_string(),
            yolo: false,
            shared: false,
            agent: crate::pty::session::AgentKind::Claude,
            notice: None,
        });
        let enter = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::empty(),
        );
        handle_event(&mut g, &app, CtEvent::Key(enter))
            .await
            .unwrap();
        // Immediately Esc — race against the spawned create completing.
        let esc = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::empty(),
        );
        handle_event(&mut g, &app, CtEvent::Key(esc)).await.unwrap();
    }
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let g = app.lock().await;
    // Regardless of which side won the race, modal must not be Error.
    assert!(
        !matches!(g.modal, Some(Modal::Error { .. })),
        "Esc race should never produce an error modal, got {:?}",
        g.modal
    );
}

#[tokio::test]
async fn create_in_folded_repo_unfolds_and_keeps_new_workspace_selected() {
    use std::sync::Arc;
    use tokio::sync::Mutex;
    let store = crate::data::store::Store::open_in_memory().unwrap();
    let repo_dir = init_git_repo();
    let repo_id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
        .await
        .unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    let repo = store
        .repos()
        .unwrap()
        .into_iter()
        .find(|r| r.id == repo_id)
        .unwrap();
    // Create the workspace up front, since App::new consumes the store.
    let created = crate::data::workspace::create(
        &store,
        &repo,
        Some("feature"),
        tmp.path(),
        false,
        false,
        crate::pty::session::AgentKind::Claude,
        tokio_util::sync::CancellationToken::new(),
        |_| {},
    )
    .await
    .unwrap();
    let new_id = created.workspace.id;
    let mut app = App::new(store, tmp.path().to_path_buf()).unwrap();
    // The owning repo is collapsed in the dashboard — the scenario where a
    // freshly-created workspace would otherwise land on a hidden row and
    // get parked (no highlight, cursor adrift).
    app.dashboard.folded.insert(repo_id.0 as u64, true);
    let my_gen = app.alloc_create_gen();
    let shared = Arc::new(Mutex::new(app));
    crate::app::reconcile_create_result(
        shared.clone(),
        my_gen,
        repo_id,
        "feature".to_string(),
        Ok(created),
    )
    .await;

    let mut g = shared.lock().await;
    let app: &mut App = &mut g;
    assert_eq!(
        app.dashboard.folded.get(&(repo_id.0 as u64)).copied(),
        Some(false),
        "creating a workspace in a folded repo must unfold it"
    );
    assert_eq!(
        app.dashboard.selection,
        Some(SelectionTarget::Workspace(new_id)),
        "selection should move to the newly created workspace"
    );
    // Draw a frame: this rebuilds `selectable` via `visible_targets`,
    // which hides workspaces inside folded repos. With the repo unfolded,
    // the new workspace must be a visible target and remain the live
    // selection rather than parking on an invisible row.
    let backend = TestBackend::new(120, 30);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| draw_for_test(f, app)).unwrap();
    assert!(
        app.selectable.contains(&SelectionTarget::Workspace(new_id)),
        "new workspace should be a visible selection target after draw"
    );
    assert_eq!(
        app.dashboard.selection,
        Some(SelectionTarget::Workspace(new_id)),
        "selection should stay on the new workspace after a draw, not park"
    );
}
