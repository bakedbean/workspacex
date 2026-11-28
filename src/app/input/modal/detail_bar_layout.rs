//! Keys for the `b` detail-bar layout editor. Every edit goes through
//! [`apply`], which saves it before the editor shows it, so the editor
//! never displays a layout that isn't stored.

use crate::app::App;
use crate::app::detail_bar_layout::{LayoutScope, WidthsEdit, save_layout, stored_layout};
use crate::config::detail_bar_config::DetailBarConfig;
use crate::config::detail_bar_layout::{self as ops, Containers, Pos};
use crate::ui::modal::{LayoutEditor, Modal};
use crossterm::event::{KeyCode, KeyEvent};

pub(in crate::app::input) fn keys(app: &mut App, k: KeyEvent, mut ed: LayoutEditor) {
    ed.notice = None;
    ed.cursor = ops::clamp(&ed.containers, ed.cursor);
    let close = match ed.menu {
        Some(sel) => menu_keys(app, k, &mut ed, sel),
        None => column_keys(app, k, &mut ed),
    };
    app.modal = (!close).then_some(Modal::DetailBarLayout(ed));
}

/// Keys over the containers. Returns `true` to close the editor.
fn column_keys(app: &mut App, k: KeyEvent, ed: &mut LayoutEditor) -> bool {
    let (col, row) = ed.cursor;
    let cols = ed.containers.len();
    match k.code {
        KeyCode::Esc => return true,
        KeyCode::Left | KeyCode::Char('h') => {
            ed.cursor = ops::clamp(&ed.containers, (col.saturating_sub(1), row));
        }
        KeyCode::Right | KeyCode::Char('l') => {
            ed.cursor = ops::clamp(&ed.containers, ((col + 1).min(cols - 1), row));
        }
        KeyCode::Up | KeyCode::Char('k') => {
            ed.cursor = (col, row.saturating_sub(1));
        }
        KeyCode::Down | KeyCode::Char('j') => {
            ed.cursor = ops::clamp(&ed.containers, (col, row + 1));
        }
        KeyCode::Char('a') | KeyCode::Enter => ed.menu = Some(0),
        KeyCode::Char('d' | 'x') | KeyCode::Delete | KeyCode::Backspace => {
            apply(app, ed, WidthsEdit::Keep, "nothing to remove here", |c| {
                ops::remove(c, (col, row))
            });
        }
        KeyCode::Char('H') => apply(app, ed, WidthsEdit::Keep, "", |c| {
            ops::shift(c, (col, row), true)
        }),
        KeyCode::Char('L') => apply(app, ed, WidthsEdit::Keep, "", |c| {
            ops::shift(c, (col, row), false)
        }),
        KeyCode::Char('K') => apply(app, ed, WidthsEdit::Keep, "", |c| {
            ops::reorder(c, (col, row), true)
        }),
        KeyCode::Char('J') => apply(app, ed, WidthsEdit::Keep, "", |c| {
            ops::reorder(c, (col, row), false)
        }),
        KeyCode::Char(dir @ ('<' | '>')) => {
            let left = dir == '<';
            let other = if left { col.wrapping_sub(1) } else { col + 1 };
            apply(app, ed, WidthsEdit::Swap(col, other), "", |c| {
                ops::swap_container(c, col, left).map(|to| (to, row))
            });
        }
        KeyCode::Char('n') => {
            let msg = format!("the bar holds at most {} containers", ops::MAX_CONTAINERS);
            apply(app, ed, WidthsEdit::Insert(col + 1), &msg, |c| {
                ops::add_container(c, col).map(|to| (to, 0))
            });
        }
        KeyCode::Char('X') => {
            apply(
                app,
                ed,
                WidthsEdit::Remove(col),
                "the bar needs one container",
                |c| ops::remove_container(c, col).map(|to| (to, 0)),
            );
        }
        KeyCode::Char('r') => {
            let default = DetailBarConfig::default().containers;
            apply(app, ed, WidthsEdit::Clear, "", |c| {
                *c = default;
                Some((0, 0))
            });
        }
        _ => {}
    }
    false
}

/// Keys over the module menu. Returns `true` to close the editor.
fn menu_keys(app: &mut App, k: KeyEvent, ed: &mut LayoutEditor, sel: usize) -> bool {
    let ids: Vec<&'static str> = app.registry.ids().collect();
    let Some(&id) = ids.get(sel) else {
        ed.menu = None;
        return false;
    };
    let col = ed.cursor.0;
    match k.code {
        KeyCode::Esc | KeyCode::Char('a') => ed.menu = None,
        KeyCode::Up | KeyCode::Char('k') => ed.menu = Some(sel.saturating_sub(1)),
        KeyCode::Down | KeyCode::Char('j') => ed.menu = Some((sel + 1).min(ids.len() - 1)),
        // Toggle the module in the focused container: add it (moving it
        // from another container), or take it out if it's already here.
        KeyCode::Enter | KeyCode::Char(' ') => match ops::find(&ed.containers, id) {
            Some((c, _)) if c == col => {
                let cursor = ed.cursor;
                apply(app, ed, WidthsEdit::Keep, "", |c| {
                    ops::remove_id(c, id).then_some(cursor)
                });
            }
            _ => apply(app, ed, WidthsEdit::Keep, "", |c| ops::place(c, id, col)),
        },
        KeyCode::Char('d' | 'x') | KeyCode::Delete | KeyCode::Backspace => {
            let cursor = ed.cursor;
            apply(
                app,
                ed,
                WidthsEdit::Keep,
                "that module isn't in the bar",
                |c| ops::remove_id(c, id).then_some(cursor),
            );
        }
        _ => {}
    }
    false
}

/// Run `edit` on a copy of the layout and save the result; on success
/// the editor takes the new layout and the cursor `edit` returned (and
/// on failure, whatever layout is stored). An
/// edit returning `None` doesn't apply: `refused` (when not empty)
/// becomes the notice and nothing is saved.
fn apply(
    app: &mut App,
    ed: &mut LayoutEditor,
    widths: WidthsEdit,
    refused: &str,
    edit: impl FnOnce(&mut Containers) -> Option<Pos>,
) {
    let mut next = ed.containers.clone();
    let Some(cursor) = edit(&mut next) else {
        if !refused.is_empty() {
            ed.notice = Some(refused.to_string());
        }
        return;
    };
    if next == ed.containers && widths == WidthsEdit::Keep {
        ed.cursor = cursor;
        return;
    }
    match save_layout(app, ed.scope, &next, widths) {
        Ok(()) => {
            ed.containers = next;
            ed.cursor = ops::clamp(&ed.containers, cursor);
        }
        // Part of the save may have landed (the layout, but not the
        // widths): show what is stored, not what was intended.
        Err(e) => {
            let target = match ed.scope {
                LayoutScope::Global => "global".to_string(),
                LayoutScope::Repo(_) => ed.scope_label.clone(),
            };
            ed.notice = Some(format!("could not save the {target} layout: {e}"));
            ed.containers = stored_layout(app, ed.scope);
            ed.cursor = ops::clamp(&ed.containers, ed.cursor);
        }
    }
}
