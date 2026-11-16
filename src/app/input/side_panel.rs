//! Keyboard focus in the side panel's Files list.
//!
//! `→` on the dashboard, or `Ctrl-x f` attached, opens the panel on Files
//! with focus in the list. While it has focus, `j` / `k` move the file
//! cursor, `g` / `G` jump to the ends, Enter opens the file's diff, and
//! `←` / Esc hand focus back to the rows or the agent. Any other key drops
//! focus and goes where it would have without it, so the panel never
//! swallows a key it has no use for.

use super::modal::file_diff;
use crate::app::{App, SelectionTarget, SharedApp};
use crate::data::store::WorkspaceId;
use crate::ui::View;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
