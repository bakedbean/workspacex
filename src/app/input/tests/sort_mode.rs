//! sort mode tests.

use super::*;
use crate::data::store::Store;
use crate::ui::dashboard::sort::{BLOCKED_PIN_MAX_AGE_DEFAULT_SECS, SortMode};
use crossterm::event::{KeyEvent, KeyModifiers};
use std::path::PathBuf;

fn app_with_settings(settings: &[(&str, &str)]) -> App {
    let store = Store::open_in_memory().unwrap();
    for (k, v) in settings {
        store.set_setting(k, v).unwrap();
    }
    App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap()
}

async fn press_o(app: &mut App) {
    handle_key_dashboard(
        app,
        KeyEvent::new(crossterm::event::KeyCode::Char('o'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
}

#[test]
fn a_fresh_install_sorts_by_recency() {
    let app = app_with_settings(&[]);
    assert_eq!(app.dashboard.sort_mode, SortMode::Recency);
    assert_eq!(
        app.dashboard.blocked_pin_max_age_secs,
        BLOCKED_PIN_MAX_AGE_DEFAULT_SECS
    );
}

#[test]
fn a_persisted_sort_mode_is_restored_at_startup() {
    let app = app_with_settings(&[("dashboard_sort_mode", "status")]);
    assert_eq!(app.dashboard.sort_mode, SortMode::Status);
}

#[test]
fn a_configured_blocked_pin_window_is_restored_at_startup() {
    let app = app_with_settings(&[("dashboard_blocked_pin_max_age_secs", "3600")]);
    assert_eq!(app.dashboard.blocked_pin_max_age_secs, 3600);
}

#[test]
fn an_unparseable_blocked_pin_window_falls_back_to_the_default() {
    let app = app_with_settings(&[("dashboard_blocked_pin_max_age_secs", "soon")]);
    assert_eq!(
        app.dashboard.blocked_pin_max_age_secs,
        BLOCKED_PIN_MAX_AGE_DEFAULT_SECS
    );
}

/// `wsx config set <key> @file` stores the file's contents verbatim,
/// trailing newline included. Parsing untrimmed would silently discard
/// the value and report success, so both keys trim like the existing
/// `dashboard_branch_width` does.
#[test]
fn a_setting_written_from_a_file_keeps_its_trailing_newline_out_of_the_parse() {
    let app = app_with_settings(&[
        ("dashboard_sort_mode", "status\n"),
        ("dashboard_blocked_pin_max_age_secs", "3600\n"),
    ]);
    assert_eq!(app.dashboard.sort_mode, SortMode::Status);
    assert_eq!(app.dashboard.blocked_pin_max_age_secs, 3600);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn o_cycles_the_sort_mode() {
    let mut app = app_with_settings(&[]);
    press_o(&mut app).await;
    assert_eq!(app.dashboard.sort_mode, SortMode::Status);
    press_o(&mut app).await;
    assert_eq!(app.dashboard.sort_mode, SortMode::Recency);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn o_persists_the_new_sort_mode() {
    let mut app = app_with_settings(&[]);
    press_o(&mut app).await;
    assert_eq!(
        app.store.get_setting("dashboard_sort_mode").unwrap(),
        Some("status".to_string()),
        "the choice must survive a restart, like the theme does"
    );
}
