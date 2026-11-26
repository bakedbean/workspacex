//! Dragging the separators between the dashboard detail bar's containers
//! sideways to resize them.

use super::common::*;
use super::*;
use crate::config::detail_bar_config::DetailBarConfig;
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

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
    draw_at(app, 120);
}

fn draw_at(app: &mut App, w: u16) {
    let mut term = Terminal::new(TestBackend::new(w, 40)).unwrap();
    term.draw(|f| crate::app::render::draw_for_test(f, app))
        .unwrap();
}

fn left(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn columns(app: &App) -> Vec<Rect> {
    assert!(
        app.detail_columns.len() > 1,
        "containers drawn side by side"
    );
    app.detail_columns.clone()
}

/// The screen column of the separator right of container `i`, and a row
/// inside the containers' content.
fn separator(cols: &[Rect], i: usize) -> (u16, u16) {
    (cols[i].x + cols[i].width, cols[i].y + 2)
}

fn widths(cols: &[Rect]) -> Vec<u16> {
    cols.iter().map(|c| c.width).collect()
}

/// Dragging the first separator right widens the first container at the
/// second's expense, leaves the rest alone, and keeps the widths.
#[tokio::test]
async fn dragging_a_separator_resizes_its_neighbours() {
    let mut app = app();
    let before = columns(&app);
    let (x, y) = separator(&before, 0);
    let shared = shared_app();

    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Down(MouseButton::Left), x, y),
    )
    .await;
    assert_eq!(app.dashboard.detail_column_resize.map(|p| p.right), Some(1));
    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Drag(MouseButton::Left), x + 6, y),
    )
    .await;
    draw(&mut app);
    let after = columns(&app);
    assert_eq!(after[0].width, before[0].width + 6);
    assert_eq!(after[1].width, before[1].width - 6);
    assert_eq!(after[2], before[2], "the far container stays put");

    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Up(MouseButton::Left), x + 6, y),
    )
    .await;
    assert!(app.dashboard.detail_column_resize.is_none());
    assert_eq!(
        app.store
            .get_setting(DetailBarConfig::WIDTHS_SETTING)
            .unwrap(),
        Some(DetailBarConfig::format_widths(&widths(&after)))
    );
}

/// The saved widths are proportions: a narrower terminal keeps the split.
#[tokio::test]
async fn saved_widths_scale_with_the_terminal() {
    let mut app = app();
    app.dashboard.detail_widths = Some(vec![2, 1, 1]);
    draw_at(&mut app, 162);
    let wide = widths(&columns(&app));
    assert_eq!(wide, vec![80, 40, 40]);
    draw_at(&mut app, 102);
    let narrow = widths(&columns(&app));
    assert_eq!(narrow, vec![50, 25, 25]);
}

/// Widths saved for a different number of containers are ignored.
#[tokio::test]
async fn widths_for_another_layout_share_evenly() {
    let mut app = app();
    let even = widths(&columns(&app));
    app.dashboard.detail_widths = Some(vec![3, 1]);
    draw(&mut app);
    assert_eq!(widths(&columns(&app)), even);
}

/// A drag past a neighbour clamps so each container keeps its minimum.
#[tokio::test]
async fn a_drag_clamps_to_the_minimum_width() {
    let mut app = app();
    let cols = columns(&app);
    let (x, y) = separator(&cols, 0);
    let shared = shared_app();
    for (kind, col) in [
        (MouseEventKind::Down(MouseButton::Left), x),
        (MouseEventKind::Drag(MouseButton::Left), 0),
    ] {
        handle_mouse(&mut app, &shared, left(kind, col, y)).await;
    }
    let w = app.dashboard.detail_widths.clone().unwrap();
    assert_eq!(w[0], DetailBarConfig::CONTAINER_MIN_WIDTH);
    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Drag(MouseButton::Left), 119, y),
    )
    .await;
    let w = app.dashboard.detail_widths.clone().unwrap();
    assert_eq!(w[1], DetailBarConfig::CONTAINER_MIN_WIDTH);
    assert_eq!(w[0] + w[1], cols[0].width + cols[1].width);
}

/// A press and release on a separator without moving changes nothing.
#[tokio::test]
async fn a_click_without_a_drag_keeps_the_widths() {
    let mut app = app();
    let (x, y) = separator(&columns(&app), 1);
    let shared = shared_app();
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Drag(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        handle_mouse(&mut app, &shared, left(kind, x, y)).await;
    }
    assert!(app.dashboard.detail_column_resize.is_none());
    assert_eq!(app.dashboard.detail_widths, None);
    assert_eq!(
        app.store
            .get_setting(DetailBarConfig::WIDTHS_SETTING)
            .unwrap(),
        None
    );
}

/// A press inside a container, off the separator, doesn't start a drag.
#[tokio::test]
async fn a_press_inside_a_container_is_not_a_drag() {
    let mut app = app();
    let (x, y) = separator(&columns(&app), 0);
    let shared = shared_app();
    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Down(MouseButton::Left), x - 1, y),
    )
    .await;
    assert!(app.dashboard.detail_column_resize.is_none());
}

/// A fresh press or a modal mid-drag ends the drag without resizing.
#[tokio::test]
async fn a_fresh_press_or_a_modal_cancels_the_drag() {
    let mut app = app();
    let (x, y) = separator(&columns(&app), 0);
    let shared = shared_app();
    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Down(MouseButton::Left), x, y),
    )
    .await;
    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Down(MouseButton::Left), 5, 5),
    )
    .await;
    assert!(app.dashboard.detail_column_resize.is_none());

    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Down(MouseButton::Left), x, y),
    )
    .await;
    app.modal = Some(crate::ui::modal::Modal::WorkspaceActions);
    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Drag(MouseButton::Left), x + 6, y),
    )
    .await;
    assert!(app.dashboard.detail_column_resize.is_none());
    assert_eq!(app.dashboard.detail_widths, None);
}

/// Under 80 columns the bar shows one container, with nothing to drag.
#[tokio::test]
async fn a_narrow_terminal_offers_no_separator() {
    let mut app = app();
    draw_at(&mut app, 79);
    assert!(app.detail_columns.is_empty());
}

/// A container dragged to its minimum keeps it when the terminal narrows,
/// so long as every container still fits its minimum.
#[tokio::test]
async fn a_minimum_width_survives_a_narrower_terminal() {
    let mut app = app();
    let (x, y) = separator(&columns(&app), 0);
    let shared = shared_app();
    for (kind, col) in [
        (MouseEventKind::Down(MouseButton::Left), x),
        (MouseEventKind::Up(MouseButton::Left), 0),
    ] {
        handle_mouse(&mut app, &shared, left(kind, col, y)).await;
    }
    draw_at(&mut app, 80);
    let narrow = widths(&columns(&app));
    assert_eq!(narrow[0], DetailBarConfig::CONTAINER_MIN_WIDTH);
    assert_eq!(narrow.iter().sum::<u16>(), 78);
}
