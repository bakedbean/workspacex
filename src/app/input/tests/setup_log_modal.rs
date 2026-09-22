//! `o` on the workspace-actions card: the setup-log viewer.
//!
//! The key used to be inert unless the workspace had work in flight, which
//! made the card's "setup log" label wrong for every workspace that had
//! finished building. These cover the states it now has to handle.

use super::common::shared_app;
use super::*;
use crate::data::in_flight::InFlight;
use crate::data::progress::SetupProgress;
use crate::data::store::{NewWorkspace, Store};
use crate::ui::modal::Modal;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;
use tempfile::TempDir;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// An App whose `log_dir` is a temp directory. The returned `TempDir` must
/// be held for the life of the test: dropping it deletes the logs, and
/// letting `App::new`'s real `Dirs::discover()` value stand would point
/// these tests at the developer's own `~/.local/state/wsx/logs`.
fn app_with_workspace() -> (App, crate::data::store::WorkspaceId, TempDir) {
    let store = Store::open_in_memory().unwrap();
    let repo_id = store
        .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
        .unwrap();
    let ws_id = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "alpha",
            branch: "repo/alpha",
            worktree_path: std::path::Path::new("."),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    let logs = TempDir::new().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.log_dir = logs.path().to_path_buf();
    app.refresh().unwrap();
    app.selectable = vec![SelectionTarget::Workspace(ws_id)];
    app.select_index(0);
    (app, ws_id, logs)
}

/// Write a finished setup log for repo `repo`, workspace `alpha`.
fn seed_log(logs: &TempDir, body: &str) {
    let mut w = crate::data::setup_log::create(
        logs.path(),
        "repo",
        "alpha",
        std::path::Path::new("/wt/alpha"),
        1,
    )
    .unwrap();
    crate::data::setup_log::write_line(
        &mut w,
        &crate::data::setup::SetupLine::Stdout(body.to_string()),
    )
    .unwrap();
    crate::data::setup_log::write_footer(&mut w, &crate::data::setup::SetupResult::Ok).unwrap();
}

/// The regression this change is about: nothing in flight, `o` must still
/// open the viewer rather than silently doing nothing.
#[tokio::test]
async fn o_opens_the_viewer_for_a_workspace_with_nothing_in_flight() {
    let (mut app, ws_id, _logs) = app_with_workspace();
    app.modal = Some(Modal::WorkspaceActions);
    handle_key_modal(&mut app, &shared_app(), key(KeyCode::Char('o')))
        .await
        .unwrap();
    match &app.modal {
        Some(Modal::SetupLog {
            workspace_id,
            stored,
            scroll,
        }) => {
            assert_eq!(*workspace_id, ws_id);
            assert!(
                stored.is_some(),
                "with no live work the persisted log is the source"
            );
            assert_eq!(*scroll, 0, "opens at the tail");
        }
        other => panic!("expected the setup-log viewer, got {other:?}"),
    }
}

/// While a create is running the live ring buffer is the source, so `stored`
/// stays `None` — the log file is still buffered and would read back empty.
#[tokio::test]
async fn o_tails_live_work_rather_than_the_file() {
    let (mut app, ws_id, _logs) = app_with_workspace();
    app.in_flight.insert(
        ws_id,
        InFlight::create(
            SetupProgress::shared(),
            tokio_util::sync::CancellationToken::new(),
        ),
    );
    app.modal = Some(Modal::WorkspaceActions);
    handle_key_modal(&mut app, &shared_app(), key(KeyCode::Char('o')))
        .await
        .unwrap();
    assert!(
        matches!(&app.modal, Some(Modal::SetupLog { stored: None, .. })),
        "expected a live tail, got {:?}",
        app.modal
    );
}

/// A viewer opened mid-build must pick up the persisted log once the build
/// ends, instead of going blank when the ring buffer is dropped with the
/// `in_flight` entry.
#[test]
fn viewer_switches_to_the_persisted_log_when_the_work_ends() {
    let (mut app, ws_id, logs) = app_with_workspace();
    seed_log(&logs, "compiled 41 crates");
    app.in_flight.insert(
        ws_id,
        InFlight::create(
            SetupProgress::shared(),
            tokio_util::sync::CancellationToken::new(),
        ),
    );
    app.modal = Some(Modal::SetupLog {
        workspace_id: ws_id,
        stored: None,
        scroll: 0,
    });

    // Still running: the live tail stays the source.
    app.sync_setup_log_viewer();
    assert!(matches!(
        &app.modal,
        Some(Modal::SetupLog { stored: None, .. })
    ));

    app.in_flight.remove(&ws_id);
    app.sync_setup_log_viewer();
    let Some(Modal::SetupLog {
        stored: Some(lines),
        ..
    }) = &app.modal
    else {
        panic!(
            "expected the persisted log to take over, got {:?}",
            app.modal
        );
    };
    assert!(
        lines.contains(&"compiled 41 crates".to_string()),
        "the real file's contents should be on screen: {lines:?}"
    );

    // The read is one-shot. `sync_setup_log_viewer` runs on every tick, so if
    // a loaded viewer did not short-circuit it would re-read the file ~30
    // times a second under the App lock. Deleting the log and ticking again
    // must leave what is already loaded untouched.
    std::fs::remove_file(crate::data::setup_log::setup_log_path(
        logs.path(),
        "repo",
        "alpha",
    ))
    .unwrap();
    app.sync_setup_log_viewer();
    assert!(
        matches!(&app.modal, Some(Modal::SetupLog { stored: Some(l), .. })
            if l.contains(&"compiled 41 crates".to_string())),
        "a loaded viewer must not re-read on every tick: {:?}",
        app.modal
    );
}

#[tokio::test]
async fn scroll_keys_move_the_window_and_esc_closes() {
    let (mut app, ws_id, _logs) = app_with_workspace();
    app.modal = Some(Modal::SetupLog {
        workspace_id: ws_id,
        stored: Some((0..50).map(|i| format!("line {i}")).collect()),
        scroll: 0,
    });
    let s = shared_app();

    handle_key_modal(&mut app, &s, key(KeyCode::Up))
        .await
        .unwrap();
    handle_key_modal(&mut app, &s, key(KeyCode::Up))
        .await
        .unwrap();
    assert!(matches!(
        &app.modal,
        Some(Modal::SetupLog { scroll: 2, .. })
    ));

    handle_key_modal(&mut app, &s, key(KeyCode::Down))
        .await
        .unwrap();
    assert!(matches!(
        &app.modal,
        Some(Modal::SetupLog { scroll: 1, .. })
    ));

    // Down at the tail saturates rather than wrapping.
    handle_key_modal(&mut app, &s, key(KeyCode::Char('G')))
        .await
        .unwrap();
    handle_key_modal(&mut app, &s, key(KeyCode::Down))
        .await
        .unwrap();
    assert!(matches!(
        &app.modal,
        Some(Modal::SetupLog { scroll: 0, .. })
    ));

    // `g` asks for the top; the renderer is what clamps it.
    handle_key_modal(&mut app, &s, key(KeyCode::Char('g')))
        .await
        .unwrap();
    assert!(matches!(
        &app.modal,
        Some(Modal::SetupLog {
            scroll: usize::MAX,
            ..
        })
    ));

    handle_key_modal(&mut app, &s, key(KeyCode::Esc))
        .await
        .unwrap();
    assert!(app.modal.is_none(), "esc closes the viewer");
}

/// A clamping draw is NOT guaranteed between two key events — the event loop
/// handles input at full speed and redraws on a frame floor, and `handle_paste`
/// dispatches a whole pasted string without drawing at all. So the scroll
/// increments have to survive landing on the `usize::MAX` that `g` parks there.
/// Before this was saturating, `g` then `k` panicked in debug and wrapped to
/// the tail in release.
#[tokio::test]
async fn a_scroll_burst_with_no_draw_between_keys_cannot_overflow() {
    let (mut app, ws_id, _logs) = app_with_workspace();
    app.modal = Some(Modal::SetupLog {
        workspace_id: ws_id,
        stored: Some((0..50).map(|i| format!("line {i}")).collect()),
        scroll: 0,
    });
    let s = shared_app();

    // No render runs between any of these, exactly as in a paste burst.
    for k in [
        KeyCode::Char('g'),
        KeyCode::Char('k'),
        KeyCode::Up,
        KeyCode::PageUp,
    ] {
        handle_key_modal(&mut app, &s, key(k)).await.unwrap();
    }
    assert!(
        matches!(
            &app.modal,
            Some(Modal::SetupLog {
                scroll: usize::MAX,
                ..
            })
        ),
        "scrolling up past the top must stick at MAX, not wrap: {:?}",
        app.modal
    );
}
