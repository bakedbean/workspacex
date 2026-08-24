//! repo pr link click tests.

use crate::app::App;
use crate::app::input::*;
use crate::data::store::Store;
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use std::path::PathBuf;

fn click_at(column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

/// An app with one repo whose header PR link occupies (10..12, 4).
fn app_with_linked_repo(path: &str) -> (App, crate::data::store::RepoId) {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let repo_id = app
        .store
        .add_repo(std::path::Path::new(path), "scratch", "test")
        .unwrap();
    app.refresh().unwrap();
    app.dashboard_repo_pr_rects = vec![(
        repo_id,
        Rect {
            x: 10,
            y: 4,
            width: 2,
            height: 1,
        },
    )];
    (app, repo_id)
}

#[test]
fn click_on_the_link_targets_that_repos_path() {
    let (app, _) = app_with_linked_repo("/tmp/wsx-linked-repo");
    assert_eq!(
        repo_pr_link_target(&app, &click_at(10, 4)),
        Some(PathBuf::from("/tmp/wsx-linked-repo"))
    );
    // The rect's last column counts too.
    assert_eq!(
        repo_pr_link_target(&app, &click_at(11, 4)),
        Some(PathBuf::from("/tmp/wsx-linked-repo"))
    );
}

#[test]
fn click_beside_the_link_targets_nothing() {
    let (app, _) = app_with_linked_repo("/tmp/wsx-linked-repo");
    for (col, row) in [(9, 4), (12, 4), (10, 3), (10, 5)] {
        assert!(
            repo_pr_link_target(&app, &click_at(col, row)).is_none(),
            "({col},{row}) is outside the link"
        );
    }
}

/// A repo unregistered between the draw that recorded the rect and the
/// click must resolve to nothing rather than panicking on the lookup.
#[test]
fn click_on_a_stale_rect_targets_nothing() {
    let (mut app, repo_id) = app_with_linked_repo("/tmp/wsx-linked-repo");
    app.store.remove_repo(repo_id).unwrap();
    app.refresh().unwrap();
    assert!(repo_pr_link_target(&app, &click_at(10, 4)).is_none());
}
