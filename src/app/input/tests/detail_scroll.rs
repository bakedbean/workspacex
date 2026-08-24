//! detail scroll.

use super::*;
use crate::data::store::Store;
use crate::ui::View;
use crate::ui::split::AttachedState;
use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use std::path::PathBuf;

fn mouse_at(kind: MouseEventKind, col: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column: col,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wheel_over_container_scrolls_that_container() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.view = View::Dashboard;
    app.detail_container_rects = [
        Some(Rect {
            x: 0,
            y: 20,
            width: 20,
            height: 5,
        }),
        Some(Rect {
            x: 21,
            y: 20,
            width: 20,
            height: 5,
        }),
        None,
        None,
    ];
    handle_mouse(&mut app, mouse_at(MouseEventKind::ScrollDown, 25, 22)).await;
    assert_eq!(app.detail_scroll_offsets[1], 3);
    assert_eq!(app.detail_scroll_offsets[0], 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wheel_outside_containers_does_not_touch_detail_offsets() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.view = View::Dashboard;
    app.detail_container_rects = [
        Some(Rect {
            x: 0,
            y: 20,
            width: 20,
            height: 5,
        }),
        None,
        None,
        None,
    ];
    handle_mouse(&mut app, mouse_at(MouseEventKind::ScrollDown, 50, 5)).await;
    assert_eq!(app.detail_scroll_offsets, [0; 4]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wheel_in_attached_view_does_not_touch_detail_offsets() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    // Construct an Attached view with a synthetic workspace id. No live
    // session is needed — `scroll_active` no-ops when the focused id has
    // no session, which is the behavior we want to verify (detail
    // offsets stay untouched regardless).
    app.view = View::Attached(AttachedState::single(crate::ui::split::AttachTarget {
        workspace_id: crate::data::store::WorkspaceId(1),
        instance: crate::data::store::AgentInstanceId(1),
    }));
    app.detail_container_rects = [
        Some(Rect {
            x: 0,
            y: 20,
            width: 20,
            height: 5,
        }),
        None,
        None,
        None,
    ];
    handle_mouse(&mut app, mouse_at(MouseEventKind::ScrollDown, 5, 22)).await;
    assert_eq!(app.detail_scroll_offsets, [0; 4]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wheel_up_scrolls_back_with_saturating_sub() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.view = View::Dashboard;
    app.detail_container_rects = [
        Some(Rect {
            x: 0,
            y: 20,
            width: 20,
            height: 5,
        }),
        None,
        None,
        None,
    ];
    app.detail_scroll_offsets[0] = 2;
    handle_mouse(&mut app, mouse_at(MouseEventKind::ScrollUp, 5, 22)).await;
    assert_eq!(app.detail_scroll_offsets[0], 0);
}
