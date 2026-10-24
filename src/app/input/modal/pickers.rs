//! Small option pickers: workspace name color, workspace tags, usage window
//! and hide rules.

use super::*;
use crate::app::{App, SharedApp};
use crate::error::Result;
use crate::ui::modal::move_selection;
use crate::ui::modal::{Modal, TagTarget};
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
    rows: Vec<(crate::data::hide_rules::HideRule, usize)>,
    selected: usize,
) -> Result<()> {
    let last = rows.len().saturating_sub(1);
    let selected = match k.code {
        KeyCode::Esc | KeyCode::Enter => {
            app.modal = None;
            return Ok(());
        }
        KeyCode::Up | KeyCode::Char('k') => selected.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => (selected + 1).min(last),
        KeyCode::Char(' ') => {
            let Some((rule, _)) = rows.get(selected).cloned() else {
                return Ok(());
            };
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
        rows,
        selected,
    });
    Ok(())
}

/// Every tag in use plus any this repo still has a rule for, then PR rules,
/// each with how many of this repo's workspaces it matches.
pub(crate) fn hide_rule_rows(
    app: &App,
    repo: crate::data::store::RepoId,
    rules: &crate::data::hide_rules::HideRules,
) -> Result<Vec<(crate::data::hide_rules::HideRule, usize)>> {
    use crate::data::hide_rules::HideRule;
    let mut tag_rules: Vec<HideRule> = app
        .store
        .all_tags()?
        .iter()
        .map(|(t, _)| HideRule::tag(t))
        .collect();
    tag_rules.extend(
        rules
            .iter()
            .filter(|r| matches!(r, HideRule::Tag(_)))
            .cloned(),
    );
    tag_rules.sort();
    tag_rules.dedup();
    let ids: Vec<_> = app
        .workspaces
        .iter()
        .filter(|(rid, _)| *rid == repo)
        .map(|(_, w)| w.id)
        .collect();
    Ok(tag_rules
        .into_iter()
        .chain(HideRule::PR_RULES)
        .map(|rule| {
            let n = ids
                .iter()
                .filter(|id| rule.matches(&app.hide_subject(**id)))
                .count();
            (rule, n)
        })
        .collect())
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
        target: TagTarget::Local(ws),
        name: w.name,
        known,
        tags: w.tags.iter().map(|t| t.to_string()).collect(),
        input,
        selected,
        browsing: false,
        notice: None,
    });
    Ok(())
}

/// Open the `#` editor for the attached remote workspace, from its listed
/// record. Known tags are every tag on the host's listed workspaces.
pub(crate) fn open_remote_workspace_tags(app: &mut App) {
    let (Some(list), Some(rec)) = (
        app.remote_list.as_ref(),
        crate::app::attached_remote_record(app),
    ) else {
        app.modal = Some(Modal::Error {
            message: "can't tag this remote workspace: it isn't in the host's listing".into(),
        });
        return;
    };
    let target = TagTarget::Remote {
        dest: list.dest.clone(),
        repo: rec.repo.clone(),
        workspace: rec.workspace.clone(),
    };
    reopen_tags_editor(app, &target, String::new(), 0, &[]);
}

/// The listed record a remote tag target names.
fn remote_record_mut<'a>(
    app: &'a mut App,
    repo: &str,
    workspace: &str,
) -> Option<&'a mut crate::commands::shared::SharedWorkspaceRecord> {
    app.remote_list
        .as_mut()?
        .records
        .iter_mut()
        .find(|r| r.repo == repo && r.workspace == workspace)
}

/// Reopen the `#` editor on `target` with fresh snapshots; see
/// `open_workspace_tags` for `seen`.
fn reopen_tags_editor(
    app: &mut App,
    target: &TagTarget,
    input: String,
    selected: usize,
    seen: &[String],
) {
    let (repo, workspace) = match target {
        TagTarget::Local(ws) => {
            if let Err(e) = open_workspace_tags(app, *ws, input, selected, seen) {
                app.modal = Some(Modal::Error {
                    message: format!("could not load tags: {e}"),
                });
            }
            return;
        }
        TagTarget::Remote {
            repo, workspace, ..
        } => (repo, workspace),
    };
    let mut known: Vec<String> = app
        .remote_list
        .iter()
        .flat_map(|l| l.records.iter().flat_map(|r| r.tags.iter().cloned()))
        .chain(seen.iter().cloned())
        .collect();
    known.sort();
    known.dedup();
    let tags = remote_record_mut(app, repo, workspace)
        .map(|r| r.tags.clone())
        .unwrap_or_default();
    app.modal = Some(Modal::WorkspaceTags {
        target: target.clone(),
        name: workspace.clone(),
        known,
        tags,
        input,
        selected,
        browsing: false,
        notice: None,
    });
}

/// Serializes remote tag writes so a quick add-then-remove of one tag lands
/// on the host in the order it was typed.
static REMOTE_TAG_WRITES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Write one tag change to the host in the background. The listed record is
/// updated up front so the editor and bar reflect it at once; a failed write
/// rolls it back and says why on the editor (or in an error modal).
fn write_remote_tag(app: &mut App, shared: &SharedApp, target: &TagTarget, tag: &str, add: bool) {
    let TagTarget::Remote {
        dest,
        repo,
        workspace,
    } = target.clone()
    else {
        return;
    };
    if let Some(rec) = remote_record_mut(app, &repo, &workspace) {
        set_tag(&mut rec.tags, tag, add);
    }
    let shared = shared.clone();
    let tag = tag.to_string();
    let target = target.clone();
    tokio::spawn(async move {
        let _order = REMOTE_TAG_WRITES.lock().await;
        let result =
            crate::commands::shared_hosts::tag_remote(&dest, &repo, &workspace, &tag, add).await;
        let Err(e) = result else {
            return;
        };
        let mut g = shared.lock().await;
        if let Some(rec) = remote_record_mut(&mut g, &repo, &workspace) {
            set_tag(&mut rec.tags, &tag, !add);
        }
        let message = format!("could not save tags: {e}");
        match g.modal.as_mut() {
            Some(Modal::WorkspaceTags {
                target: t,
                tags,
                notice,
                ..
            }) if *t == target => {
                set_tag(tags, &tag, !add);
                *notice = Some(message);
            }
            None => g.modal = Some(Modal::Error { message }),
            Some(_) => tracing::warn!(%message, "remote tag write failed"),
        }
    });
}

/// Add or drop `tag` in a sorted tag list.
fn set_tag(tags: &mut Vec<String>, tag: &str, add: bool) {
    tags.retain(|t| t != tag);
    if add {
        tags.push(tag.to_string());
        tags.sort();
    }
}

/// Every edit writes through immediately; `Esc` (or `Enter` on empty input) closes.
#[allow(clippy::too_many_arguments)]
pub(super) async fn workspace_tags_editor(
    app: &mut App,
    shared: &SharedApp,
    k: crossterm::event::KeyEvent,
    target: TagTarget,
    name: String,
    known: Vec<String>,
    tags: Vec<String>,
    mut input: String,
    selected: usize,
    browsing: bool,
) -> Result<()> {
    use crate::data::tags::{TAG_RULE, normalize_tag};
    let items = crate::ui::modal::workspace_tags::tag_items(&known, &tags, &input);
    let reopen = |app: &mut App, input: String, selected: usize, browsing: bool, notice| {
        app.modal = Some(Modal::WorkspaceTags {
            target: target.clone(),
            name: name.clone(),
            known: known.clone(),
            tags: tags.clone(),
            input,
            selected,
            browsing,
            notice,
        });
    };
    let write = |app: &mut App, tag: &str, add: bool| -> Result<bool> {
        let workspace_id = match &target {
            TagTarget::Local(ws) => *ws,
            TagTarget::Remote { .. } => {
                write_remote_tag(app, shared, &target, tag, add);
                return Ok(true);
            }
        };
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
    // Toggle the highlighted row, keeping the filter, cursor, and browse mode.
    let toggle = |app: &mut App, input: String| -> Result<()> {
        if let Some(it) = items.get(selected)
            && write(app, &it.name, !it.checked)?
        {
            reopen_tags_editor(app, &target, input, selected, &known);
            if let Some(Modal::WorkspaceTags { browsing: b, .. }) = app.modal.as_mut() {
                *b = browsing;
            }
        }
        Ok(())
    };
    match k.code {
        KeyCode::Esc => app.modal = None,
        KeyCode::Enter if browsing => toggle(app, input)?,
        KeyCode::Enter if input.trim().is_empty() => app.modal = None,
        KeyCode::Enter => match normalize_tag(&input) {
            Some(t) => {
                if write(app, &t, true)? {
                    reopen_tags_editor(app, &target, String::new(), 0, &known);
                }
            }
            None => {
                let notice = format!("invalid tag: {TAG_RULE}");
                reopen(app, input, selected, false, Some(notice));
            }
        },
        // Tags can't contain spaces, so a space mid-typing is dropped rather
        // than toggling whatever row the cursor happens to be on.
        KeyCode::Char(' ') if !input.is_empty() && !browsing => {}
        KeyCode::Char(' ') => toggle(app, input)?,
        KeyCode::Up => reopen(app, input, selected.saturating_sub(1), true, None),
        KeyCode::Down => {
            let last = items.len().saturating_sub(1);
            reopen(app, input, (selected + 1).min(last), true, None)
        }
        KeyCode::Backspace => {
            input.pop();
            reopen(app, input, 0, false, None);
        }
        KeyCode::Char(c)
            if !k.modifiers.contains(KeyModifiers::CONTROL)
                && !k.modifiers.contains(KeyModifiers::ALT) =>
        {
            input.push(c);
            reopen(app, input, 0, false, None);
        }
        _ => {}
    }
    Ok(())
}
