//! Dragging the dashboard detail bar's top edge to a new height.

use super::common::*;
use super::*;
use crate::config::detail_bar_config::DetailBarConfig;
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

/// The dashboard with its only workspace selected, drawn once.
fn app() -> App {
    let mut app = seed_app_with_workspace();
    let idx = app
        .selectable
        .iter()
        .position(|t| matches!(t, SelectionTarget::Workspace(_)))
        .expect("workspace target present");
    app.select_index(idx);
    draw(&mut app);
    app
}

fn draw(app: &mut App) {
    draw_at(app, 40);
}

fn draw_at(app: &mut App, h: u16) {
    let mut term = Terminal::new(TestBackend::new(120, h)).unwrap();
    term.draw(|f| crate::app::render::draw_for_test(f, app))
        .unwrap();
}

fn left(kind: MouseEventKind, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column: 5,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn bar(app: &App) -> ratatui::layout::Rect {
    app.detail_bar_drag.expect("detail bar drawn").rect
}

/// Dragging the bar's top edge up grows it, its bottom held still, and
/// the height is kept for the next run.
#[tokio::test]
async fn dragging_the_top_edge_resizes_the_bar() {
    let mut app = app();
    let before = bar(&app);
    let bottom = before.y + before.height;
    let shared = shared_app();

    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Down(MouseButton::Left), before.y),
    )
    .await;
    assert_eq!(app.dashboard.detail_resize.map(|p| p.grab), Some(0));
    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Drag(MouseButton::Left), before.y - 5),
    )
    .await;
    draw(&mut app);
    let after = bar(&app);
    assert_eq!(after.height, before.height + 5);
    assert_eq!(after.y + after.height, bottom, "the bottom edge stays put");

    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Up(MouseButton::Left), after.y),
    )
    .await;
    assert!(app.dashboard.detail_resize.is_none());
    assert_eq!(
        app.store
            .get_setting(DetailBarConfig::HEIGHT_SETTING)
            .unwrap(),
        Some(after.height.to_string())
    );
}

/// A drag grabbed on the rule under the header keeps that rule under the
/// pointer, and a drag past either end clamps to the bounds.
#[tokio::test]
async fn a_drag_clamps_to_the_bounds() {
    let mut app = app();
    let d = app.detail_bar_drag.unwrap();
    let shared = shared_app();
    for (kind, row) in [
        (MouseEventKind::Down(MouseButton::Left), d.rect.y + 1),
        (MouseEventKind::Drag(MouseButton::Left), 0),
    ] {
        handle_mouse(&mut app, &shared, left(kind, row)).await;
    }
    assert_eq!(app.dashboard.detail_height, Some(d.max));
    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Up(MouseButton::Left), 39),
    )
    .await;
    assert_eq!(app.dashboard.detail_height, Some(d.min));
}

/// A press inside the bar, below its edge, doesn't start a drag.
#[tokio::test]
async fn a_press_inside_the_bar_is_not_a_drag() {
    let mut app = app();
    let r = bar(&app);
    let shared = shared_app();
    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Down(MouseButton::Left), r.y + 3),
    )
    .await;
    assert!(app.dashboard.detail_resize.is_none());
}

/// A fresh press mid-drag (its release lost outside the terminal) ends
/// the drag without resizing.
#[tokio::test]
async fn a_fresh_press_cancels_the_drag() {
    let mut app = app();
    let r = bar(&app);
    let shared = shared_app();
    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Down(MouseButton::Left), r.y),
    )
    .await;
    assert_eq!(app.dashboard.detail_resize.map(|p| p.grab), Some(0));
    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Down(MouseButton::Left), r.y + 3),
    )
    .await;
    assert!(app.dashboard.detail_resize.is_none());
    assert_eq!(app.dashboard.detail_height, None);
}

/// A press and release on the edge without moving is a click: the bar
/// keeps its automatic size and nothing is saved.
#[tokio::test]
async fn a_click_without_a_drag_keeps_automatic_sizing() {
    let mut app = app();
    let r = bar(&app);
    let shared = shared_app();
    for (kind, row) in [
        (MouseEventKind::Down(MouseButton::Left), r.y + 1),
        (MouseEventKind::Drag(MouseButton::Left), r.y + 1),
        (MouseEventKind::Up(MouseButton::Left), r.y + 1),
    ] {
        handle_mouse(&mut app, &shared, left(kind, row)).await;
    }
    assert!(app.dashboard.detail_resize.is_none());
    assert_eq!(app.dashboard.detail_height, None);
    assert_eq!(
        app.store
            .get_setting(DetailBarConfig::HEIGHT_SETTING)
            .unwrap(),
        None
    );
}

/// A modal opening mid-drag ends the drag without resizing.
#[tokio::test]
async fn a_modal_cancels_the_drag() {
    let mut app = app();
    let r = bar(&app);
    let shared = shared_app();
    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Down(MouseButton::Left), r.y),
    )
    .await;
    app.modal = Some(crate::ui::modal::Modal::WorkspaceActions);
    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Drag(MouseButton::Left), r.y - 5),
    )
    .await;
    assert!(app.dashboard.detail_resize.is_none());
    assert_eq!(app.dashboard.detail_height, None);
}

/// With the PM pane showing, a dragged bar still leaves the list its
/// minimum: the bounds come out of the room above the PM pane.
#[tokio::test]
async fn the_pm_pane_shrinks_the_bounds() {
    let mut app = app();
    let full = app.detail_bar_drag.unwrap().max;
    app.pm_visible = true;
    draw(&mut app);
    let d = app.detail_bar_drag.expect("room for a drag");
    assert!(d.max < full);
    app.dashboard.detail_height = Some(u16::MAX);
    draw(&mut app);
    let r = bar(&app);
    assert_eq!(r.height, d.max);
    assert!(r.y >= DetailBarConfig::LIST_MIN_ROWS, "list keeps its rows");
}

/// A terminal too short to leave the list its minimum offers no drag
/// (and `DetailBarConfig::height_for` ignores a saved height there).
#[tokio::test]
async fn a_short_terminal_offers_no_drag() {
    let mut app = app();
    app.dashboard.detail_height = Some(12);
    // 18 rows: the bar still shows, but 17 above the footer can't hold
    // its 8-row minimum and the list's 10.
    draw_at(&mut app, 18);
    assert!(app.detail_bar_drag.is_none());
    draw_at(&mut app, 19);
    assert!(app.detail_bar_drag.is_some());
}
