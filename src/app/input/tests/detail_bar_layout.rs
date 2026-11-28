//! The detail-bar layout editor (`b` on the dashboard).

use super::common::*;
use super::*;
use crate::config::detail_bar_config::{DetailBarConfig, resolve_global_only};
use crate::data::store::Store;
use crate::ui::modal::Modal;
use crossterm::event::{KeyEvent, KeyModifiers};
use std::path::PathBuf;

fn app() -> App {
    App::new(
        Store::open_in_memory().unwrap(),
        PathBuf::from("/tmp/wsx-test"),
    )
    .unwrap()
}

async fn open(app: &mut App) {
    handle_key_dashboard(app, KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE))
        .await
        .unwrap();
}

async fn press(app: &mut App, code: KeyCode) {
    let shared = shared_app();
    handle_key_modal(app, &shared, KeyEvent::new(code, KeyModifiers::NONE))
        .await
        .unwrap();
}

async fn press_all(app: &mut App, keys: &str) {
    for c in keys.chars() {
        press(app, KeyCode::Char(c)).await;
    }
}

fn editor(app: &App) -> &crate::ui::modal::LayoutEditor {
    match &app.modal {
        Some(Modal::DetailBarLayout(ed)) => ed,
        other => panic!("editor not open: {other:?}"),
    }
}

fn stored(app: &App) -> Vec<Vec<String>> {
    resolve_global_only(&app.store).containers
}

fn ids(spec: &[&[&str]]) -> Vec<Vec<String>> {
    spec.iter()
        .map(|c| c.iter().map(|s| s.to_string()).collect())
        .collect()
}

#[tokio::test]
async fn b_opens_on_the_current_layout_and_closes_again() {
    let mut app = app();
    open(&mut app).await;
    assert_eq!(
        editor(&app).containers,
        DetailBarConfig::default().containers
    );
    assert_eq!(editor(&app).scope_label, "global");
    press(&mut app, KeyCode::Char('b')).await;
    assert!(app.modal.is_none());
}

#[tokio::test]
async fn menu_adds_a_module_to_the_focused_container_and_saves_it() {
    let mut app = app();
    open(&mut app).await;
    // Container 2, then open the menu and walk to AGENTS (5th entry).
    press(&mut app, KeyCode::Char('l')).await;
    press(&mut app, KeyCode::Char('a')).await;
    press_all(&mut app, "jjjj").await;
    press(&mut app, KeyCode::Enter).await;
    let want = ids(&[
        &["session_summary"],
        &["recent_chat", "agents"],
        &["processes", "recent_files"],
    ]);
    assert_eq!(editor(&app).containers, want);
    assert_eq!(stored(&app), want);
    // The menu stays open so several modules can be added in a row.
    assert_eq!(editor(&app).menu, Some(4));
    // Enter again toggles it back out.
    press(&mut app, KeyCode::Enter).await;
    assert_eq!(stored(&app), DetailBarConfig::default().containers);
    press(&mut app, KeyCode::Esc).await;
    assert_eq!(editor(&app).menu, None);
}

#[tokio::test]
async fn menu_moves_a_module_placed_elsewhere() {
    let mut app = app();
    open(&mut app).await;
    press(&mut app, KeyCode::Char('l')).await;
    press(&mut app, KeyCode::Char('a')).await;
    press(&mut app, KeyCode::Enter).await; // SESSION SUMMARY, from container 1
    assert_eq!(
        stored(&app),
        ids(&[
            &[],
            &["recent_chat", "session_summary"],
            &["processes", "recent_files"]
        ])
    );
}

#[tokio::test]
async fn d_removes_the_highlighted_module() {
    let mut app = app();
    open(&mut app).await;
    press_all(&mut app, "llj").await; // container 3, recent_files
    press(&mut app, KeyCode::Char('d')).await;
    assert_eq!(
        stored(&app),
        ids(&[&["session_summary"], &["recent_chat"], &["processes"]])
    );
    assert_eq!(editor(&app).cursor, (2, 0));
}

#[tokio::test]
async fn shift_keys_rearrange_modules() {
    let mut app = app();
    open(&mut app).await;
    press_all(&mut app, "llj").await; // recent_files
    press(&mut app, KeyCode::Char('K')).await; // above processes
    press(&mut app, KeyCode::Char('H')).await; // into container 2
    assert_eq!(
        stored(&app),
        ids(&[
            &["session_summary"],
            &["recent_files", "recent_chat"],
            &["processes"]
        ])
    );
    assert_eq!(editor(&app).cursor, (1, 0));
}

#[tokio::test]
async fn containers_can_be_added_swapped_and_removed() {
    let mut app = app();
    open(&mut app).await;
    press(&mut app, KeyCode::Char('n')).await;
    assert_eq!(stored(&app).len(), 4);
    assert_eq!(editor(&app).cursor, (1, 0));
    press(&mut app, KeyCode::Char('n')).await;
    assert_eq!(
        editor(&app).notice.as_deref(),
        Some("the bar holds at most 4 containers")
    );
    press(&mut app, KeyCode::Char('>')).await;
    assert_eq!(
        stored(&app),
        ids(&[
            &["session_summary"],
            &["recent_chat"],
            &[],
            &["processes", "recent_files"]
        ])
    );
    press(&mut app, KeyCode::Char('X')).await;
    assert_eq!(stored(&app), DetailBarConfig::default().containers);
}

#[tokio::test]
async fn reset_restores_the_default_layout() {
    let mut app = app();
    app.store
        .set_setting("detail_bar_config", r#"{"containers": [["agents"]]}"#)
        .unwrap();
    open(&mut app).await;
    assert_eq!(editor(&app).containers, ids(&[&["agents"]]));
    press(&mut app, KeyCode::Char('X')).await;
    assert_eq!(
        editor(&app).notice.as_deref(),
        Some("the bar needs one container")
    );
    press(&mut app, KeyCode::Char('r')).await;
    assert_eq!(stored(&app), DetailBarConfig::default().containers);
}

#[tokio::test]
async fn repo_with_containers_override_edits_that_override() {
    let mut app = app();
    let rid = app
        .store
        .add_repo(std::path::Path::new("/tmp/r"), "demo", "")
        .unwrap();
    app.store
        .set_repo_detail_bar_config(rid, Some(r#"{"containers": [["processes"]]}"#))
        .unwrap();
    app.refresh().unwrap();
    let idx = app
        .selectable
        .iter()
        .position(|t| matches!(t, SelectionTarget::Repo(r) if *r == rid))
        .unwrap();
    app.select_index(idx);
    open(&mut app).await;
    assert_eq!(editor(&app).scope_label, "repo demo");
    press(&mut app, KeyCode::Char('d')).await;
    let repo = app.repos.iter().find(|r| r.id == rid).unwrap();
    let cfg = crate::config::detail_bar_config::resolve(repo, &app.store);
    assert_eq!(cfg.containers, ids(&[&[]]));
    // The global layout is untouched.
    assert!(
        app.store
            .get_setting("detail_bar_config")
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn menu_toggle_clears_duplicates_of_a_module() {
    let mut app = app();
    app.store
        .set_setting(
            "detail_bar_config",
            r#"{"containers": [["agents", "agents"], ["agents"]]}"#,
        )
        .unwrap();
    open(&mut app).await;
    press(&mut app, KeyCode::Char('a')).await;
    press_all(&mut app, "jjjj").await; // AGENTS
    press(&mut app, KeyCode::Enter).await; // in container 1: toggles out
    assert_eq!(stored(&app), ids(&[&[], &[]]));
}

#[tokio::test]
async fn failed_save_shows_the_stored_layout() {
    let mut app = app();
    app.store
        .set_setting("detail_bar_config", "{broken")
        .unwrap();
    open(&mut app).await;
    press(&mut app, KeyCode::Char('d')).await;
    let ed = editor(&app);
    assert!(
        ed.notice.as_deref().unwrap().contains("could not save"),
        "{:?}",
        ed.notice
    );
    assert_eq!(ed.containers, DetailBarConfig::default().containers);
    assert_eq!(
        app.store
            .get_setting("detail_bar_config")
            .unwrap()
            .as_deref(),
        Some("{broken")
    );
}

#[tokio::test]
async fn reset_forgets_dragged_widths() {
    let mut app = app();
    app.dashboard.detail_widths = Some(vec![5, 1, 1]);
    open(&mut app).await;
    press(&mut app, KeyCode::Char('r')).await;
    assert_eq!(app.dashboard.detail_widths, None);
}
