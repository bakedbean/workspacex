//! rename modal tests.

use super::*;
use crate::data::store::{NewWorkspace, Store};
use crossterm::event::{KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::path::PathBuf;

fn key(code: crossterm::event::KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn screen_text(term: &Terminal<TestBackend>) -> String {
    let buf = term.backend().buffer();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn app_with_workspace() -> (App, crate::data::store::WorkspaceId) {
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
    let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    (app, ws_id)
}

#[test]
fn workspace_actions_card_lists_rename() {
    let (mut app, ws_id) = app_with_workspace();
    app.dashboard.selection = Some(crate::app::SelectionTarget::Workspace(ws_id));
    app.modal = Some(crate::ui::modal::Modal::WorkspaceActions);
    let backend = TestBackend::new(80, 24);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| draw_for_test(f, &mut app)).unwrap();
    assert!(
        screen_text(&term).contains("rename"),
        "actions card must list the rename action"
    );
}

#[test]
fn rename_modal_renders_buffer_and_notice() {
    let (mut app, ws_id) = app_with_workspace();
    app.modal = Some(crate::ui::modal::Modal::RenameWorkspace {
        workspace_id: ws_id,
        name_buffer: "alpha-two".to_string(),
        notice: Some("rename failed: boom".to_string()),
    });
    let backend = TestBackend::new(80, 24);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| draw_for_test(f, &mut app)).unwrap();
    let text = screen_text(&term);
    assert!(
        text.contains("alpha-two"),
        "buffer must render; got {text:?}"
    );
    assert!(
        text.contains("rename failed: boom"),
        "notice must render; got {text:?}"
    );
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
async fn actions_card_r_opens_rename_prefilled() {
    let (mut app, ws_id) = app_with_workspace();
    app.dashboard.selection = Some(crate::app::SelectionTarget::Workspace(ws_id));
    app.modal = Some(crate::ui::modal::Modal::WorkspaceActions);
    let shared = dummy_shared();
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    match &app.modal {
        Some(crate::ui::modal::Modal::RenameWorkspace {
            workspace_id,
            name_buffer,
            notice,
        }) => {
            assert_eq!(*workspace_id, ws_id);
            assert_eq!(name_buffer, "alpha", "buffer pre-fills current name");
            assert!(notice.is_none());
        }
        other => panic!("expected RenameWorkspace modal, got {other:?}"),
    }
}

// ---- name color picker (`C`) ----

/// The picker's state, or a panic naming what modal is actually open.
fn picker_state(app: &App) -> (crate::data::store::WorkspaceId, Option<u8>, usize, String) {
    match &app.modal {
        Some(crate::ui::modal::Modal::NameColorPicker {
            workspace_id,
            current,
            selected,
            filter,
        }) => (*workspace_id, *current, *selected, filter.clone()),
        other => panic!("expected NameColorPicker modal, got {other:?}"),
    }
}

fn stored_color(app: &App, ws_id: crate::data::store::WorkspaceId) -> Option<u8> {
    app.store
        .workspace_by_id(ws_id)
        .unwrap()
        .expect("workspace present")
        .name_color
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shift_c_opens_the_picker_for_the_selected_workspace() {
    let (mut app, ws_id) = app_with_workspace();
    app.store.set_workspace_name_color(ws_id, Some(21)).unwrap();
    app.refresh().unwrap();
    app.selectable = vec![SelectionTarget::Workspace(ws_id)];
    app.select_index(0);

    handle_key_dashboard(&mut app, key(KeyCode::Char('C')))
        .await
        .unwrap();

    let (id, current, selected, filter) = picker_state(&app);
    assert_eq!(id, ws_id);
    assert_eq!(current, Some(21), "snapshots the color already applied");
    assert_eq!(selected, 0);
    assert_eq!(filter, "");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shift_c_without_a_workspace_selected_does_nothing() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.selectable = vec![SelectionTarget::Repo(crate::data::store::RepoId(1))];
    app.select_index(0);

    handle_key_dashboard(&mut app, key(KeyCode::Char('C')))
        .await
        .unwrap();

    assert!(app.modal.is_none(), "got {:?}", app.modal);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actions_card_c_opens_the_picker() {
    let (mut app, ws_id) = app_with_workspace();
    app.dashboard.selection = Some(SelectionTarget::Workspace(ws_id));
    app.selectable = vec![SelectionTarget::Workspace(ws_id)];
    app.select_index(0);
    app.modal = Some(crate::ui::modal::Modal::WorkspaceActions);
    let shared = dummy_shared();

    handle_key_modal(&mut app, &shared, key(KeyCode::Char('C')))
        .await
        .unwrap();

    assert_eq!(picker_state(&app).0, ws_id);
}

#[test]
fn actions_card_lists_the_name_color_action() {
    let (mut app, ws_id) = app_with_workspace();
    app.dashboard.selection = Some(SelectionTarget::Workspace(ws_id));
    app.modal = Some(crate::ui::modal::Modal::WorkspaceActions);
    let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
    term.draw(|f| draw_for_test(f, &mut app)).unwrap();
    assert!(
        screen_text(&term).contains("name color"),
        "actions card must list the name-color action"
    );
}

fn open_picker(app: &mut App, ws_id: crate::data::store::WorkspaceId) {
    app.selectable = vec![SelectionTarget::Workspace(ws_id)];
    app.select_index(0);
    app.modal = Some(crate::ui::modal::Modal::NameColorPicker {
        workspace_id: ws_id,
        current: None,
        selected: 0,
        filter: String::new(),
    });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn typing_filters_the_palette_and_resets_the_cursor() {
    let (mut app, ws_id) = app_with_workspace();
    open_picker(&mut app, ws_id);
    let shared = dummy_shared();

    for c in "d7af87".chars() {
        handle_key_modal(&mut app, &shared, key(KeyCode::Char(c)))
            .await
            .unwrap();
    }

    let (_, _, selected, filter) = picker_state(&app);
    assert_eq!(filter, "d7af87");
    assert_eq!(selected, 0, "cursor lands on the first hit after typing");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn backspace_edits_the_filter_rather_than_closing() {
    let (mut app, ws_id) = app_with_workspace();
    app.modal = Some(crate::ui::modal::Modal::NameColorPicker {
        workspace_id: ws_id,
        current: None,
        selected: 4,
        filter: "d7af".to_string(),
    });
    let shared = dummy_shared();

    handle_key_modal(&mut app, &shared, key(KeyCode::Backspace))
        .await
        .unwrap();

    let (_, _, selected, filter) = picker_state(&app);
    assert_eq!(filter, "d7a");
    assert_eq!(selected, 0, "a narrower/wider set re-seeds the cursor");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn arrows_walk_the_grid() {
    let (mut app, ws_id) = app_with_workspace();
    open_picker(&mut app, ws_id);
    let shared = dummy_shared();

    handle_key_modal(&mut app, &shared, key(KeyCode::Right))
        .await
        .unwrap();
    assert_eq!(picker_state(&app).2, 1);

    handle_key_modal(&mut app, &shared, key(KeyCode::Down))
        .await
        .unwrap();
    assert_eq!(picker_state(&app).2, 17, "down moves a whole row of 16");

    handle_key_modal(&mut app, &shared, key(KeyCode::Left))
        .await
        .unwrap();
    handle_key_modal(&mut app, &shared, key(KeyCode::Up))
        .await
        .unwrap();
    assert_eq!(picker_state(&app).2, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enter_applies_the_focused_color_and_closes() {
    let (mut app, ws_id) = app_with_workspace();
    app.modal = Some(crate::ui::modal::Modal::NameColorPicker {
        workspace_id: ws_id,
        current: None,
        selected: 0,
        filter: "d7af87".to_string(),
    });
    let shared = dummy_shared();

    handle_key_modal(&mut app, &shared, key(KeyCode::Enter))
        .await
        .unwrap();

    assert!(app.modal.is_none(), "picker closes on apply");
    assert_eq!(stored_color(&app, ws_id), Some(180));
    assert_eq!(
        app.workspaces
            .iter()
            .find(|(_, w)| w.id == ws_id)
            .unwrap()
            .1
            .name_color,
        Some(180),
        "the in-memory workspace list is refreshed so the row repaints",
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enter_with_no_matching_color_closes_without_writing() {
    let (mut app, ws_id) = app_with_workspace();
    app.store.set_workspace_name_color(ws_id, Some(21)).unwrap();
    app.modal = Some(crate::ui::modal::Modal::NameColorPicker {
        workspace_id: ws_id,
        current: Some(21),
        selected: 0,
        filter: "zzz".to_string(),
    });
    let shared = dummy_shared();

    handle_key_modal(&mut app, &shared, key(KeyCode::Enter))
        .await
        .unwrap();

    assert!(app.modal.is_none());
    assert_eq!(stored_color(&app, ws_id), Some(21), "existing color kept");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn delete_clears_the_color_back_to_the_theme_default() {
    let (mut app, ws_id) = app_with_workspace();
    app.store
        .set_workspace_name_color(ws_id, Some(180))
        .unwrap();
    app.modal = Some(crate::ui::modal::Modal::NameColorPicker {
        workspace_id: ws_id,
        current: Some(180),
        selected: 0,
        filter: String::new(),
    });
    let shared = dummy_shared();

    handle_key_modal(&mut app, &shared, key(KeyCode::Delete))
        .await
        .unwrap();

    assert!(app.modal.is_none());
    assert_eq!(stored_color(&app, ws_id), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn esc_cancels_without_touching_the_stored_color() {
    let (mut app, ws_id) = app_with_workspace();
    app.store.set_workspace_name_color(ws_id, Some(21)).unwrap();
    app.modal = Some(crate::ui::modal::Modal::NameColorPicker {
        workspace_id: ws_id,
        current: Some(21),
        selected: 40,
        filter: String::new(),
    });
    let shared = dummy_shared();

    handle_key_modal(&mut app, &shared, key(KeyCode::Esc))
        .await
        .unwrap();

    assert!(app.modal.is_none());
    assert_eq!(stored_color(&app, ws_id), Some(21));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_write_surfaces_an_error_instead_of_closing_silently() {
    // A read-only/busy DB used to close the picker and report nothing, so
    // the user could believe a color had been saved when it had not.
    let (mut app, ws_id) = app_with_workspace();
    app.store
        .conn()
        .execute_batch("PRAGMA query_only = ON")
        .unwrap();
    app.modal = Some(crate::ui::modal::Modal::NameColorPicker {
        workspace_id: ws_id,
        current: None,
        selected: 0,
        filter: "d7af87".to_string(),
    });
    let shared = dummy_shared();

    handle_key_modal(&mut app, &shared, key(KeyCode::Enter))
        .await
        .unwrap();

    match &app.modal {
        Some(crate::ui::modal::Modal::Error { message }) => {
            assert!(
                message.contains("color"),
                "the error must name what failed; got {message:?}"
            );
        }
        other => panic!("expected an Error modal, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clicking_a_swatch_applies_that_color() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let (mut app, ws_id) = app_with_workspace();
    open_picker(&mut app, ws_id);
    // Draw so the picker publishes its swatch rects for hit-testing.
    let mut term = Terminal::new(TestBackend::new(80, 30)).unwrap();
    term.draw(|f| draw_for_test(f, &mut app)).unwrap();
    let (idx, rect) = *app
        .name_color_swatch_rects
        .iter()
        .find(|(i, _)| *i == 180)
        .expect("swatch 180 drawn");

    handle_mouse(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await;

    assert_eq!(idx, 180);
    assert_eq!(stored_color(&app, ws_id), Some(180));
    assert!(app.modal.is_none(), "picker closes after a click");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clicking_outside_the_grid_dismisses_without_applying() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let (mut app, ws_id) = app_with_workspace();
    open_picker(&mut app, ws_id);
    let mut term = Terminal::new(TestBackend::new(80, 30)).unwrap();
    term.draw(|f| draw_for_test(f, &mut app)).unwrap();

    handle_mouse(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await;

    assert!(app.modal.is_none());
    assert_eq!(stored_color(&app, ws_id), None);
}

#[test]
fn a_chosen_color_still_paints_the_row_after_a_restart() {
    // The literal requirement: pick a color, quit, come back. Uses a real
    // on-disk DB and a second `App` so nothing survives in memory.
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("wsx.db");

    let ws_id = {
        let store = Store::open(&db).unwrap();
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
        store.set_workspace_name_color(ws_id, Some(180)).unwrap();
        ws_id
    };

    let store = Store::open(&db).unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    assert_eq!(
        app.workspaces
            .iter()
            .find(|(_, w)| w.id == ws_id)
            .expect("workspace reloaded")
            .1
            .name_color,
        Some(180),
    );

    // Repos render folded by default; expand so the row is actually drawn.
    for (rid, _) in app.workspaces.clone() {
        app.dashboard.folded.insert(rid.0 as u64, false);
    }
    let mut term = Terminal::new(TestBackend::new(120, 24)).unwrap();
    term.draw(|f| draw_for_test(f, &mut app)).unwrap();
    let buf = term.backend().buffer();
    let painted = (0..buf.area.height).any(|y| {
        (0..buf.area.width).any(|x| {
            let cell = &buf[(x, y)];
            cell.fg == ratatui::style::Color::Indexed(180) && cell.symbol() != " "
        })
    });
    let text = (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        painted,
        "the branch name is painted in the restored color:\n{text}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rename_modal_esc_cancels_without_changes() {
    let (mut app, ws_id) = app_with_workspace();
    app.modal = Some(crate::ui::modal::Modal::RenameWorkspace {
        workspace_id: ws_id,
        name_buffer: "alpha-two".to_string(),
        notice: None,
    });
    let shared = dummy_shared();
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(app.modal.is_none());
    let (_, ws) = app.workspaces.iter().find(|(_, w)| w.id == ws_id).unwrap();
    assert_eq!(ws.name, "alpha", "esc must not rename");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rename_modal_empty_buffer_shows_notice() {
    let (mut app, ws_id) = app_with_workspace();
    app.modal = Some(crate::ui::modal::Modal::RenameWorkspace {
        workspace_id: ws_id,
        name_buffer: "...".to_string(), // normalizes to None
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
        Some(crate::ui::modal::Modal::RenameWorkspace { notice, .. }) => {
            assert_eq!(notice.as_deref(), Some("name cannot be empty"));
        }
        other => panic!("modal must stay open with notice, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rename_modal_enter_renames_workspace_and_branch() {
    // Real git repo: `rename` runs `git branch -m`.
    let repo_dir = tempfile::TempDir::new().unwrap();
    let r = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .current_dir(repo_dir.path())
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    };
    r(&["init", "-q", "-b", "main"]);
    r(&["config", "user.email", "t@e"]);
    r(&["config", "user.name", "t"]);
    r(&["commit", "--allow-empty", "-q", "-m", "init"]);

    let store = Store::open_in_memory().unwrap();
    let repo_id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
        .await
        .unwrap();
    let repo = store
        .repos()
        .unwrap()
        .into_iter()
        .find(|r| r.id == repo_id)
        .unwrap();
    let base = tempfile::TempDir::new().unwrap();
    let created = crate::data::workspace::create(
        &store,
        &repo,
        Some("alpha"),
        base.path(),
        false,
        false,
        crate::pty::session::AgentKind::Claude,
        tokio_util::sync::CancellationToken::new(),
        |_| {},
    )
    .await
    .unwrap();
    let ws_id = created.workspace.id;

    let mut app = App::new(store, base.path().to_path_buf()).unwrap();
    app.modal = Some(crate::ui::modal::Modal::RenameWorkspace {
        workspace_id: ws_id,
        name_buffer: "Fix Bug!".to_string(), // exercises normalization too
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

    assert!(app.modal.is_none(), "modal closes on success");
    let ws = app
        .store
        .workspaces(repo.id)
        .unwrap()
        .into_iter()
        .find(|w| w.id == ws_id)
        .unwrap();
    assert_eq!(ws.name, "fix-bug");
    assert_eq!(ws.branch, "wsx/fix-bug");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rename_modal_git_failure_keeps_modal_with_notice() {
    // Repo path is not a git repo → `git branch -m` fails.
    let (mut app, ws_id) = app_with_workspace();
    app.modal = Some(crate::ui::modal::Modal::RenameWorkspace {
        workspace_id: ws_id,
        name_buffer: "beta".to_string(),
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
        Some(crate::ui::modal::Modal::RenameWorkspace { notice, .. }) => {
            let text = notice.as_deref().unwrap_or("");
            assert!(text.starts_with("rename failed"), "got notice {notice:?}");
            assert!(
                !text.contains('\n'),
                "notice must stay on one line; got {notice:?}"
            );
        }
        other => panic!("modal must stay open on git failure, got {other:?}"),
    }
    let (_, ws) = app.workspaces.iter().find(|(_, w)| w.id == ws_id).unwrap();
    assert_eq!(ws.name, "alpha", "failed rename must not change the name");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rename_modal_typing_and_backspace_edit_buffer() {
    let (mut app, ws_id) = app_with_workspace();
    app.modal = Some(crate::ui::modal::Modal::RenameWorkspace {
        workspace_id: ws_id,
        name_buffer: "alpha".to_string(),
        notice: Some("stale".to_string()),
    });
    let shared = dummy_shared();
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    match &app.modal {
        Some(crate::ui::modal::Modal::RenameWorkspace {
            name_buffer,
            notice,
            ..
        }) => {
            assert_eq!(name_buffer, "alphx");
            assert!(notice.is_none(), "editing clears a stale notice");
        }
        other => panic!("expected RenameWorkspace modal, got {other:?}"),
    }
}
