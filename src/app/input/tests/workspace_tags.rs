//! The `#` workspace-tags editor.

use super::common::*;
use super::*;
use crate::ui::modal::Modal;
use crossterm::event::KeyModifiers;

async fn open_editor(app: &mut App) -> crate::data::store::WorkspaceId {
    app.refresh().unwrap();
    let ws = app.workspaces[0].1.id;
    app.dashboard.selection = Some(SelectionTarget::Workspace(ws));
    press(app, '#', KeyModifiers::SHIFT).await;
    assert!(
        matches!(app.modal, Some(Modal::WorkspaceTags { .. })),
        "{:?}",
        app.modal
    );
    ws
}

async fn modal_key(app: &mut App, code: KeyCode) {
    let shared = shared_app();
    crate::app::input::modal::handle_key_modal(app, &shared, key(code))
        .await
        .unwrap();
}

async fn type_str(app: &mut App, s: &str) {
    for c in s.chars() {
        modal_key(app, KeyCode::Char(c)).await;
    }
}

fn stored_tags(app: &App, ws: crate::data::store::WorkspaceId) -> Vec<String> {
    app.store
        .workspace_by_id(ws)
        .unwrap()
        .unwrap()
        .tags
        .iter()
        .map(|t| t.to_string())
        .collect()
}

#[tokio::test]
async fn enter_adds_normalized_tag_and_stays_open() {
    let mut app = test_app_with_two_ready_workspaces();
    let ws = open_editor(&mut app).await;
    type_str(&mut app, "Parked").await;
    modal_key(&mut app, KeyCode::Enter).await;
    assert_eq!(stored_tags(&app, ws), ["parked"]);
    let Some(Modal::WorkspaceTags { input, tags, .. }) = &app.modal else {
        panic!("{:?}", app.modal);
    };
    assert!(input.is_empty());
    assert_eq!(tags, &["parked"]);
}

#[tokio::test]
async fn invalid_input_shows_notice_and_writes_nothing() {
    let mut app = test_app_with_two_ready_workspaces();
    let ws = open_editor(&mut app).await;
    type_str(&mut app, "a:b").await;
    modal_key(&mut app, KeyCode::Enter).await;
    assert!(stored_tags(&app, ws).is_empty());
    let Some(Modal::WorkspaceTags { notice, .. }) = &app.modal else {
        panic!("{:?}", app.modal);
    };
    assert!(notice.is_some());
}

#[tokio::test]
async fn space_toggles_existing_tag_off() {
    let mut app = test_app_with_two_ready_workspaces();
    app.refresh().unwrap();
    let ws = app.workspaces[0].1.id;
    app.store.add_workspace_tag(ws, "parked").unwrap();
    open_editor(&mut app).await;
    modal_key(&mut app, KeyCode::Char(' ')).await;
    assert!(stored_tags(&app, ws).is_empty());
    modal_key(&mut app, KeyCode::Char(' ')).await;
    assert_eq!(stored_tags(&app, ws), ["parked"], "space toggles back on");
}

#[tokio::test]
async fn suggestions_include_tags_from_other_workspaces() {
    let mut app = test_app_with_two_ready_workspaces();
    app.refresh().unwrap();
    app.store
        .add_workspace_tag(app.workspaces[1].1.id, "ui")
        .unwrap();
    open_editor(&mut app).await;
    let Some(Modal::WorkspaceTags { known, .. }) = &app.modal else {
        panic!("{:?}", app.modal);
    };
    assert_eq!(known, &["ui"]);
}

#[tokio::test]
async fn esc_closes_and_empty_enter_closes() {
    let mut app = test_app_with_two_ready_workspaces();
    open_editor(&mut app).await;
    modal_key(&mut app, KeyCode::Enter).await;
    assert!(app.modal.is_none());
    open_editor(&mut app).await;
    modal_key(&mut app, KeyCode::Esc).await;
    assert!(app.modal.is_none());
}

#[tokio::test]
async fn hash_on_actions_card_opens_editor() {
    let mut app = test_app_with_two_ready_workspaces();
    app.refresh().unwrap();
    app.dashboard.selection = Some(SelectionTarget::Workspace(app.workspaces[0].1.id));
    app.modal = Some(Modal::WorkspaceActions);
    modal_key(&mut app, KeyCode::Char('#')).await;
    assert!(
        matches!(app.modal, Some(Modal::WorkspaceTags { .. })),
        "{:?}",
        app.modal
    );
}

#[tokio::test]
async fn hash_on_repo_header_does_nothing() {
    let mut app = test_app_with_two_ready_workspaces();
    app.refresh().unwrap();
    app.dashboard.selection = Some(SelectionTarget::Repo(app.repos[0].id));
    press(&mut app, '#', KeyModifiers::SHIFT).await;
    assert!(app.modal.is_none());
}
