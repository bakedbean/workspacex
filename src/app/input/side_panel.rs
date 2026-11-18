//! Keyboard focus in the side panel's Files list.
//!
//! `→` on the dashboard, or `Ctrl-x f` attached, opens the panel on Files
//! with focus in the list. While it has focus, `j` / `k` move the file
//! cursor, `g` / `G` jump to the ends, Enter opens the file's diff, `o`
//! opens the file in the editor, and
//! `←` / Esc hand focus back to the rows or the agent. Any other key drops
//! focus and goes where it would have without it, so the panel never
//! swallows a key it has no use for.

use super::modal::file_diff;
use crate::app::{App, SelectionTarget, SharedApp};
use crate::data::store::WorkspaceId;
use crate::ui::View;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::Path;

/// Open changed `file` of the worktree at `worktree` in the editor, at
/// `line`: `file_editor_cmd`, else `editor_cmd`. A file the branch deleted
/// has nothing to open.
pub(in crate::app::input) fn edit_file(
    app: &App,
    worktree: &Path,
    file: &crate::git::ChangedFile,
    line: usize,
) -> std::result::Result<(), String> {
    let abs = worktree.join(&file.path);
    if !abs.exists() {
        return Err(format!(
            "{} is deleted on this branch; nothing to open",
            file.path
        ));
    }
    let setting = |key| {
        app.store
            .get_setting(key)
            .ok()
            .flatten()
            .filter(|c: &String| !c.trim().is_empty())
    };
    let cmd = setting("file_editor_cmd").or_else(|| setting("editor_cmd"));
    crate::commands::external::open_file_in_editor(worktree, &abs, line, cmd.as_deref())
        .map_err(|e| e.to_string())
}

/// `o` in the focused list: the cursor's file in the editor.
fn edit_cursor_file(app: &mut App) {
    let Some(ws) = side_panel_workspace(app) else {
        return;
    };
    let Some(lists) = crate::app::render::dashboard::side_panel_data(app, ws).and_then(|d| d.lists)
    else {
        return;
    };
    let cursor = super::active_side_panel(app).file_cursor;
    let (Some(file), Some(worktree)) = (
        lists
            .files
            .get(cursor.min(lists.files.len().saturating_sub(1))),
        app.workspace_path(ws),
    ) else {
        return;
    };
    if let Err(message) = edit_file(app, &worktree, file, 1) {
        app.modal = Some(crate::ui::modal::Modal::Error { message });
    }
}

/// The workspace the side panel on screen shows: the selected row on the
/// dashboard, the focused pane's workspace attached.
pub(in crate::app::input) fn side_panel_workspace(app: &App) -> Option<WorkspaceId> {
    match &app.view {
        View::Dashboard => match app.dashboard.selection {
            Some(SelectionTarget::Workspace(id)) => Some(id),
            _ => None,
        },
        View::Attached(state) => state.focused_target().map(|t| t.workspace_id),
        View::AttachedRemote => None,
    }
}

/// Open the diff of file `index` in the side panel on screen, moving the
/// panel's cursor onto it.
pub(in crate::app::input) fn open_file(app: &mut App, shared: &SharedApp, index: usize) {
    let Some(ws) = side_panel_workspace(app) else {
        return;
    };
    super::active_side_panel(app).set_cursor(index);
    file_diff::open(app, shared, ws, index);
}

/// Handle `k` if the side panel's Files list has keyboard focus. Returns
/// whether the key was consumed.
pub(in crate::app::input) fn focused_key(app: &mut App, shared: &SharedApp, k: KeyEvent) -> bool {
    // A panel that isn't on screen (too narrow a terminal) has no focus to
    // give, and on the dashboard the reply box or PM pane may hold the
    // keys instead.
    if app.modal.is_some()
        || app.leader_pending
        || app.side_panel_rect.is_none()
        || (matches!(app.view, View::Dashboard)
            && !matches!(app.focus, crate::ui::PaneFocus::Dashboard))
        || !super::active_side_panel(app).is_focused()
    {
        return false;
    }
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let panel = super::active_side_panel(app);
    // Off the Files list there's no cursor: the keys scroll the body,
    // which the draw clamps.
    if panel.tab != crate::ui::dashboard::side_panel::PanelTab::Files {
        match k.code {
            KeyCode::Down | KeyCode::Char('j') if !ctrl => panel.scroll_by(1, false),
            KeyCode::Up | KeyCode::Char('k') if !ctrl => panel.scroll_by(1, true),
            KeyCode::PageDown => panel.scroll_by(10, false),
            KeyCode::PageUp => panel.scroll_by(10, true),
            KeyCode::Home | KeyCode::Char('g') if !ctrl => panel.scroll = 0,
            KeyCode::End | KeyCode::Char('G') if !ctrl => panel.scroll = usize::MAX,
            KeyCode::Esc | KeyCode::Left => panel.focused = false,
            KeyCode::Char('x') if ctrl => return false,
            _ => {
                panel.focused = false;
                return false;
            }
        }
        return true;
    }
    match k.code {
        KeyCode::Down | KeyCode::Char('j') if !ctrl => panel.move_cursor(1),
        KeyCode::Up | KeyCode::Char('k') if !ctrl => panel.move_cursor(-1),
        KeyCode::Home | KeyCode::Char('g') if !ctrl => panel.set_cursor(0),
        // The draw clamps the cursor to the last file.
        KeyCode::End | KeyCode::Char('G') if !ctrl => panel.set_cursor(usize::MAX),
        KeyCode::Enter => {
            let cursor = panel.file_cursor;
            open_file(app, shared, cursor);
        }
        KeyCode::Char('o') if !ctrl => edit_cursor_file(app),
        KeyCode::Esc | KeyCode::Left => panel.focused = false,
        // The `Ctrl-x` leader keeps focus: its chord may be for the panel.
        KeyCode::Char('x') if ctrl => return false,
        _ => {
            panel.focused = false;
            return false;
        }
    }
    true
}
