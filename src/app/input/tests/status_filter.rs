//! Clearing the header tally's status filter from the keyboard.

use super::*;
use crate::data::store::Store;
use crate::ui::dashboard::status::Status;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

fn app() -> App {
    let mut app = App::new(
        Store::open_in_memory().unwrap(),
        PathBuf::from("/tmp/wsx-test"),
    )
    .unwrap();
    app.view = crate::ui::View::Dashboard;
    app
}

async fn press(app: &mut App, code: KeyCode) {
    handle_key_dashboard(app, KeyEvent::new(code, KeyModifiers::NONE))
        .await
        .unwrap();
}

#[tokio::test]
async fn esc_clears_the_status_filter() {
    let mut app = app();
    app.dashboard.status_filter = Some(Status::Question);
    press(&mut app, KeyCode::Esc).await;
    assert_eq!(app.dashboard.status_filter, None);
}

/// With both filters on, the first Esc clears the typed needle and the
/// second drops the status — innermost first, so one stray Esc doesn't
/// throw away both.
#[tokio::test]
async fn esc_clears_the_typed_filter_before_the_status() {
    let mut app = app();
    app.dashboard.status_filter = Some(Status::Idle);
    app.dashboard.filter = Some("auth".into());
    press(&mut app, KeyCode::Esc).await;
    assert_eq!(app.dashboard.filter, None);
    assert_eq!(app.dashboard.status_filter, Some(Status::Idle));
    press(&mut app, KeyCode::Esc).await;
    assert_eq!(app.dashboard.status_filter, None);
}

/// Typing a needle while a status is active keeps the status: the two
/// filters compose.
#[tokio::test]
async fn typing_a_filter_keeps_the_status() {
    let mut app = app();
    app.dashboard.status_filter = Some(Status::Question);
    press(&mut app, KeyCode::Char('/')).await;
    press(&mut app, KeyCode::Char('a')).await;
    assert_eq!(app.dashboard.filter.as_deref(), Some("a"));
    assert_eq!(app.dashboard.status_filter, Some(Status::Question));
}
