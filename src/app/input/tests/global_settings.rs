//! The global settings drawer (`,` on the dashboard).

use super::common::*;
use super::*;
use crate::config::settings::SETTINGS;
use crate::data::store::Store;
use crate::ui::dashboard::sort::SortMode;
use crate::ui::modal::Modal;
use crossterm::event::{KeyEvent, KeyModifiers};
use std::path::PathBuf;

fn app_on(key: &str) -> App {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let selected = SETTINGS.iter().position(|d| d.key == key).unwrap();
    app.modal = Some(Modal::GlobalSettings { selected });
    app
}

async fn drawer_key(app: &mut App, code: KeyCode) {
    let shared = shared_app();
    handle_key_modal(app, &shared, KeyEvent::new(code, KeyModifiers::NONE))
        .await
        .unwrap();
}

#[tokio::test]
async fn comma_opens_the_drawer_and_closes_it_again() {
    let mut app = App::new(
        Store::open_in_memory().unwrap(),
        PathBuf::from("/tmp/wsx-test"),
    )
    .unwrap();
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(KeyCode::Char(','), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(matches!(
        app.modal,
        Some(Modal::GlobalSettings { selected: 0 })
    ));
    drawer_key(&mut app, KeyCode::Char(',')).await;
    assert!(app.modal.is_none());
}

#[tokio::test]
async fn j_and_k_move_within_bounds() {
    let mut app = app_on(SETTINGS[0].key);
    drawer_key(&mut app, KeyCode::Char('k')).await;
    assert!(matches!(
        app.modal,
        Some(Modal::GlobalSettings { selected: 0 })
    ));
    drawer_key(&mut app, KeyCode::Char('j')).await;
    assert!(matches!(
        app.modal,
        Some(Modal::GlobalSettings { selected: 1 })
    ));
    let last = SETTINGS.len() - 1;
    app.modal = Some(Modal::GlobalSettings { selected: last });
    drawer_key(&mut app, KeyCode::Down).await;
    assert!(matches!(app.modal, Some(Modal::GlobalSettings { selected }) if selected == last));
}

#[tokio::test]
async fn enter_toggles_an_on_off_key_from_its_default() {
    let mut app = app_on("nerd_fonts");
    drawer_key(&mut app, KeyCode::Enter).await;
    assert_eq!(
        app.store.get_setting("nerd_fonts").unwrap().as_deref(),
        Some("off")
    );
    drawer_key(&mut app, KeyCode::Enter).await;
    assert_eq!(
        app.store.get_setting("nerd_fonts").unwrap().as_deref(),
        Some("on")
    );
    assert!(matches!(app.modal, Some(Modal::GlobalSettings { .. })));
}

#[tokio::test]
async fn enter_steps_a_choice_and_d_resets_it_live() {
    let mut app = app_on("dashboard_sort_mode");
    drawer_key(&mut app, KeyCode::Enter).await;
    assert_eq!(
        app.store
            .get_setting("dashboard_sort_mode")
            .unwrap()
            .as_deref(),
        Some("status")
    );
    assert_eq!(app.dashboard.sort_mode, SortMode::Status);
    drawer_key(&mut app, KeyCode::Char('d')).await;
    assert_eq!(app.store.get_setting("dashboard_sort_mode").unwrap(), None);
    assert_eq!(app.dashboard.sort_mode, SortMode::Recency);
}

#[tokio::test]
async fn enter_on_a_text_key_queues_an_editor_session() {
    let mut app = app_on("editor_cmd");
    drawer_key(&mut app, KeyCode::Enter).await;
    assert!(app.modal.is_none());
    assert!(matches!(
        app.pending_edit,
        Some(crate::app::PendingEdit::Global {
            key: "editor_cmd",
            ..
        })
    ));
}

#[test]
fn an_edited_value_is_validated_like_config_set() {
    let mut app = app_on("usage_graph_window");
    let err = crate::app::apply_global_setting(&mut app, "usage_graph_window", "week\n");
    assert!(err.is_err());
    crate::app::apply_global_setting(&mut app, "usage_graph_window", " 1w\n").unwrap();
    assert_eq!(
        app.store
            .get_setting("usage_graph_window")
            .unwrap()
            .as_deref(),
        Some("1w")
    );
}
