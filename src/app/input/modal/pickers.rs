//! Small option pickers: workspace name color, workspace tags, usage window
//! and hide rules.

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

/// Space toggles the rule under the cursor and persists immediately.
pub(super) async fn hide_rules_picker(
    app: &mut App,
    k: crossterm::event::KeyEvent,
    repo_id: crate::data::store::RepoId,
    repo_name: String,
    mut rules: crate::data::hide_rules::HideRules,
    counts: [usize; 6],
    selected: usize,
) -> Result<()> {
    use crate::data::hide_rules::HideRule;
    let last = HideRule::PR_RULES.len() - 1;
    let selected = match k.code {
        KeyCode::Esc | KeyCode::Enter => {
            app.modal = None;
            return Ok(());
        }
        KeyCode::Up | KeyCode::Char('k') => selected.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => (selected + 1).min(last),
        KeyCode::Char(' ') => {
            let rule = HideRule::PR_RULES[selected].clone();
            rules.toggle(rule.clone());
            if let Err(e) = app.store.set_repo_hide_rules(repo_id, &rules) {
                app.modal = Some(Modal::Error {
                    message: format!("could not save hide rules: {e}"),
                });
                return Ok(());
            }
            if !rules.contains(&rule) {
                app.dashboard.expanded_hidden.remove(&(repo_id, rule));
            }
            app.refresh()?;
            selected
        }
        _ => selected,
    };
    app.modal = Some(Modal::HideRules {
        repo_id,
        repo_name,
        rules,
        counts,
        selected,
    });
    Ok(())
}

/// Open (or reopen after a write) the `#` editor for `ws` with fresh snapshots.
/// `seen` keeps names already listed, so unchecking a tag's last use doesn't
/// drop it from the list mid-edit.
pub(crate) fn open_workspace_tags(
    app: &mut App,
    ws: crate::data::store::WorkspaceId,
    input: String,
    selected: usize,
    seen: &[String],
) -> Result<()> {
    let Some(w) = app.store.workspace_by_id(ws)? else {
        app.modal = None;
        return Ok(());
    };
    let mut known: Vec<String> = app.store.all_tags()?.into_iter().map(|(t, _)| t).collect();
    known.extend(seen.iter().cloned());
    known.sort();
    known.dedup();
    app.modal = Some(Modal::WorkspaceTags {
        workspace_id: ws,
        name: w.name,
        known,
        tags: w.tags.iter().map(|t| t.to_string()).collect(),
        input,
        selected,
        notice: None,
    });
    Ok(())
}

/// Every edit writes through immediately; `Esc` (or `Enter` on empty input) closes.
#[allow(clippy::too_many_arguments)]
pub(super) async fn workspace_tags_editor(
    app: &mut App,
    k: crossterm::event::KeyEvent,
    workspace_id: crate::data::store::WorkspaceId,
    name: String,
    known: Vec<String>,
    tags: Vec<String>,
    mut input: String,
    selected: usize,
) -> Result<()> {
    use crate::data::tags::{MAX_TAG_LEN, normalize_tag};
    let items = crate::ui::modal::workspace_tags::tag_items(&known, &tags, &input);
    let reopen = |app: &mut App, input: String, selected: usize, notice: Option<String>| {
        app.modal = Some(Modal::WorkspaceTags {
            workspace_id,
            name: name.clone(),
            known: known.clone(),
            tags: tags.clone(),
            input,
            selected,
            notice,
        });
    };
    let write = |app: &mut App, tag: &str, add: bool| -> Result<bool> {
        let r = if add {
            app.store.add_workspace_tag(workspace_id, tag)
        } else {
            app.store.remove_workspace_tag(workspace_id, tag)
        };
        if let Err(e) = r {
            app.modal = Some(Modal::Error {
                message: format!("could not save tags: {e}"),
            });
            return Ok(false);
        }
        app.refresh()?;
        Ok(true)
    };
    match k.code {
        KeyCode::Esc => app.modal = None,
        KeyCode::Enter if input.trim().is_empty() => app.modal = None,
        KeyCode::Enter => match normalize_tag(&input) {
            Some(t) => {
                if write(app, &t, true)? {
                    open_workspace_tags(app, workspace_id, String::new(), 0, &known)?;
                }
            }
            None => {
                let notice = format!("invalid tag: use 1-{MAX_TAG_LEN} of a-z 0-9 - _");
                reopen(app, input, selected, Some(notice));
            }
        },
        KeyCode::Char(' ') => {
            if let Some(it) = items.get(selected)
                && write(app, &it.name, !it.checked)?
            {
                open_workspace_tags(app, workspace_id, input, selected, &known)?;
            }
        }
        KeyCode::Up => reopen(app, input, selected.saturating_sub(1), None),
        KeyCode::Down => {
            let last = items.len().saturating_sub(1);
            reopen(app, input, (selected + 1).min(last), None)
        }
        KeyCode::Backspace => {
            input.pop();
            reopen(app, input, 0, None);
        }
        KeyCode::Char(c)
            if !k.modifiers.contains(KeyModifiers::CONTROL)
                && !k.modifiers.contains(KeyModifiers::ALT) =>
        {
            input.push(c);
            reopen(app, input, 0, None);
        }
        _ => {}
    }
    Ok(())
}
