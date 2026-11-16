//! The side panel's diff viewer: opening it on a file, loading that file's
//! diff in the background, and its keys and mouse.

use crate::app::{App, SharedApp};
use crate::data::store::WorkspaceId;
use crate::ui::modal::Modal;
use crate::ui::modal::file_diff::{DiffLoad, FileDiffModal, diff_lines};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::sync::Arc;

/// Lines one wheel notch scrolls the diff.
const WHEEL_LINES: isize = 3;

/// Open the viewer on file `index` of workspace `ws`'s side-panel files.
/// Does nothing until the panel's lists have been polled.
pub(in crate::app::input) fn open(
    app: &mut App,
    shared: &SharedApp,
    ws: WorkspaceId,
    index: usize,
) {
    let Some(data) = crate::app::render::dashboard::side_panel_data(app, ws) else {
        return;
    };
    let Some(lists) = data.lists else {
        return;
    };
    if lists.files.is_empty() {
        return;
    }
    let Some(worktree) = app
        .workspaces
        .iter()
        .find(|(_, w)| w.id == ws)
        .map(|(_, w)| w.worktree_path.clone())
    else {
        return;
    };
    let modal = FileDiffModal::new(
        ws,
        worktree,
        lists.base.clone(),
        Arc::new(lists.files.clone()),
        index,
    );
    app.modal = Some(Modal::FileDiff(Box::new(modal)));
    load(app, shared);
}

/// Load the open viewer's current file in the background. The result lands
/// only if the viewer still shows that file when it arrives.
fn load(app: &App, shared: &SharedApp) {
    let Some(Modal::FileDiff(m)) = &app.modal else {
        return;
    };
    let Some(file) = m.file() else {
        return;
    };
    let (ws, index, path) = (m.workspace_id, m.index, file.path.clone());
    let (worktree, base) = (m.worktree.clone(), m.base.clone());
    let shared = shared.clone();
    tokio::spawn(async move {
        let load = match crate::git::branch_file_diff(&worktree, &base, &path).await {
            Ok(d) => DiffLoad::Ready(Arc::new(diff_lines(&d))),
            Err(e) => DiffLoad::Failed(e.to_string()),
        };
        let mut g = shared.lock().await;
        if let Some(Modal::FileDiff(m)) = g.modal.as_mut()
            && m.workspace_id == ws
            && m.index == index
            && m.file().is_some_and(|f| f.path == path)
        {
            m.load = load;
        }
    });
}

/// Step the viewer `d` files along and load the new one.
fn step(app: &mut App, shared: &SharedApp, d: isize) {
    let Some(Modal::FileDiff(m)) = app.modal.as_mut() else {
        return;
    };
    if m.step(d) {
        load(app, shared);
    }
}

pub(in crate::app::input) fn keys(app: &mut App, shared: &SharedApp, k: KeyEvent) {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    match k.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.modal = None;
            return;
        }
        KeyCode::Char('[') | KeyCode::Left => return step(app, shared, -1),
        KeyCode::Char(']') | KeyCode::Right => return step(app, shared, 1),
        _ => {}
    }
    let Some(Modal::FileDiff(m)) = app.modal.as_mut() else {
        return;
    };
    match k.code {
        KeyCode::Char('d') if ctrl => m.scroll_by((m.view_h / 2).max(1) as isize),
        KeyCode::Char('u') if ctrl => m.scroll_by(-((m.view_h / 2).max(1) as isize)),
        KeyCode::Down | KeyCode::Char('j') => m.scroll_by(1),
        KeyCode::Up | KeyCode::Char('k') => m.scroll_by(-1),
        KeyCode::PageDown | KeyCode::Char('J') | KeyCode::Char(' ') => m.page(true),
        KeyCode::PageUp | KeyCode::Char('K') => m.page(false),
        KeyCode::Home | KeyCode::Char('g') => m.to_top(),
        KeyCode::End | KeyCode::Char('G') => m.to_bottom(),
        KeyCode::Char('m') => m.minimap = !m.minimap,
        _ => {}
    }
}

/// The mouse while the viewer is open: the wheel scrolls the diff from
/// over either pane, and a press or drag on the minimap jumps there. The
/// viewer covers the screen, so it takes every mouse event.
pub(in crate::app::input) fn mouse(app: &mut App, ev: MouseEvent) {
    let Some(Modal::FileDiff(m)) = app.modal.as_mut() else {
        return;
    };
    match ev.kind {
        MouseEventKind::ScrollDown => m.scroll_by(WHEEL_LINES),
        MouseEventKind::ScrollUp => m.scroll_by(-WHEEL_LINES),
        MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left) => {
            m.jump_to_minimap(ev.column, ev.row)
        }
        _ => {}
    }
}
