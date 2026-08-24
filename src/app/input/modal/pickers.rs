//! Small option pickers: workspace name color and usage window.

use super::*;
use crate::app::{App, SharedApp};
use crate::error::Result;
use crate::ui::modal::Modal;
use crate::ui::modal::move_selection;
use crossterm::event::{KeyCode, KeyModifiers};
// Test-only imports: the moved test modules access `draw_for_test`,
// `AttachedState`, `Arc`, and `Mutex` through `super::*` glob imports
// that cascade from the surrounding `tests` module.

pub(super) async fn name_color_picker(
    app: &mut App,
    _shared: &SharedApp,
    k: crossterm::event::KeyEvent,
    workspace_id: crate::data::store::WorkspaceId,
    current: Option<u8>,
    selected: usize,
    filter: String,
) -> Result<()> {
    use crate::ui::modal::Dir;
    let hits = crate::config::name_color::matching(&filter);
    // Re-open the modal with the same identity but a new cursor/filter.
    macro_rules! reopen {
        ($selected:expr, $filter:expr) => {
            app.modal = Some(Modal::NameColorPicker {
                workspace_id,
                current,
                selected: $selected,
                filter: $filter,
            })
        };
    }
    match k.code {
        KeyCode::Esc => app.modal = None,
        KeyCode::Enter => match hits.get(selected).copied() {
            Some(idx) => apply_name_color(app, workspace_id, Some(idx))?,
            // An empty result set (a filter matching nothing) has
            // nothing to apply: close, leaving the stored color alone.
            // Passing `None` through here would CLEAR it instead.
            None => app.modal = None,
        },
        KeyCode::Delete => apply_name_color(app, workspace_id, None)?,
        KeyCode::Left => reopen!(move_selection(selected, hits.len(), Dir::Left), filter),
        KeyCode::Right => reopen!(move_selection(selected, hits.len(), Dir::Right), filter),
        KeyCode::Up => reopen!(move_selection(selected, hits.len(), Dir::Up), filter),
        KeyCode::Down => reopen!(move_selection(selected, hits.len(), Dir::Down), filter),
        // The cursor indexes the FILTERED list, so any edit re-seeds it
        // to the first hit rather than pointing at an unrelated color.
        KeyCode::Backspace => {
            let mut filter = filter;
            filter.pop();
            reopen!(0, filter);
        }
        KeyCode::Char(c)
            if !k.modifiers.contains(KeyModifiers::CONTROL)
                && !k.modifiers.contains(KeyModifiers::ALT) =>
        {
            let mut filter = filter;
            filter.push(c);
            reopen!(0, filter);
        }
        _ => {}
    }

    Ok(())
}

pub(super) async fn usage_window_picker(
    app: &mut App,
    _shared: &SharedApp,
    k: crossterm::event::KeyEvent,
    selected: usize,
) -> Result<()> {
    match k.code {
        KeyCode::Up => {
            let n = if selected == 0 {
                crate::config::usage_window::UsageWindow::ALL.len() - 1
            } else {
                selected - 1
            };
            app.modal = Some(Modal::UsageWindowPicker { selected: n });
        }
        KeyCode::Down => {
            let n = if selected + 1 >= crate::config::usage_window::UsageWindow::ALL.len() {
                0
            } else {
                selected + 1
            };
            app.modal = Some(Modal::UsageWindowPicker { selected: n });
        }
        KeyCode::Enter => {
            let win = crate::config::usage_window::UsageWindow::from_index(selected);
            if let Err(e) = app
                .store
                .set_setting("usage_graph_window", win.as_setting())
            {
                tracing::warn!(error = %e, "failed to persist usage_graph_window");
            }
            app.modal = None;
        }
        KeyCode::Esc => {
            app.modal = None;
        }
        _ => {}
    };
    Ok(())
}
