//! The full-screen browsing panels: updates, processes, repo settings.

use super::*;
use crate::app::{
    App, AttachReady, PendingEdit, RepoSettingField, SharedApp, apply_global_setting,
    apply_repo_setting, ensure_workspace_session, rescan_processes, restore_attached_state,
};
use crate::error::Result;
use crate::ui::View;
use crate::ui::modal::{Modal, PanelRow};
use crate::ui::split::SplitDirection;
use crossterm::event::{KeyCode, KeyModifiers};
// Test-only imports: the moved test modules access `draw_for_test`,
// `AttachedState`, `Arc`, and `Mutex` through `super::*` glob imports
// that cascade from the surrounding `tests` module.

pub(super) async fn updates_panel(
    app: &mut App,
    _shared: &SharedApp,
    k: crossterm::event::KeyEvent,
    selected: usize,
    filter: Option<String>,
) -> Result<()> {
    let selected_now = selected;
    // Build the same ordered workspace list the renderer uses, so
    // arrow keys and Enter operate on the same indices.
    let order = panel_order(app, filter.as_deref());
    // Filter-input mode: while the buffer is live, printable keys
    // edit it rather than firing j/k/o/l/v/s, and Esc clears the
    // filter instead of closing the panel. Arrows and Enter fall
    // through so the panel stays navigable mid-search. Mirrors the
    // dashboard's filter intercept (see `handle_key_dashboard`).
    if let Some(buf) = filter.as_ref() {
        let edited: Option<Option<String>> = match k.code {
            KeyCode::Esc => Some(None),
            KeyCode::Backspace => {
                let mut b = buf.clone();
                b.pop();
                Some(Some(b))
            }
            KeyCode::Char(c)
                if !c.is_control()
                    && !k.modifiers.contains(KeyModifiers::CONTROL)
                    && !k.modifiers.contains(KeyModifiers::ALT) =>
            {
                let mut b = buf.clone();
                b.push(c);
                Some(Some(b))
            }
            _ => None,
        };
        if let Some(new_filter) = edited {
            let new_order = panel_order(app, new_filter.as_deref());
            app.modal = Some(Modal::UpdatesPanel {
                selected: reselect(order.get(selected_now), &new_order, selected_now),
                filter: new_filter,
            });
            return Ok(());
        }
    }
    match k.code {
        KeyCode::Esc => {
            app.modal = None;
        }
        // Up/Down wrap at either end, like the dashboard list: the last
        // row is one keystroke from the first. An empty list has nowhere
        // to wrap to and lands the cursor on 0. A cursor past the end (the
        // list shrank under it) is clamped back into range first, so Up
        // reaches the last real row instead of stepping down from a stale
        // index.
        KeyCode::Up | KeyCode::Char('k') => {
            let max = order.len().saturating_sub(1);
            let new_sel = if selected_now == 0 {
                max
            } else {
                (selected_now - 1).min(max)
            };
            app.modal = Some(Modal::UpdatesPanel {
                selected: new_sel,
                filter: filter.clone(),
            });
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let max = order.len().saturating_sub(1);
            let new_sel = if selected_now >= max {
                0
            } else {
                selected_now + 1
            };
            app.modal = Some(Modal::UpdatesPanel {
                selected: new_sel,
                filter: filter.clone(),
            });
        }
        // `o` and `G` drive the dashboard's own sort and group modes — the
        // panel lists workspaces in the dashboard's order, so the keys that
        // change that order are the dashboard's keys, with the same
        // persistence. The cursor follows the selected workspace to its new
        // row rather than staying on the same index.
        KeyCode::Char('o') | KeyCode::Char('G') => {
            if k.code == KeyCode::Char('o') {
                app.dashboard.cycle_sort_mode(&app.store);
            } else {
                app.dashboard.toggle_group_mode();
            }
            let new_order = panel_order(app, filter.as_deref());
            let new_sel = reselect(order.get(selected_now), &new_order, selected_now);
            app.modal = Some(Modal::UpdatesPanel {
                selected: new_sel,
                filter: filter.clone(),
            });
        }
        // `/` arms filter mode. Reached only when `filter` is None —
        // an active buffer swallows printable keys above.
        KeyCode::Char('/') => {
            app.modal = Some(Modal::UpdatesPanel {
                selected: selected_now,
                filter: Some(String::new()),
            });
        }
        // A hidden group row folds like the dashboard's: `l` opens it,
        // `Enter` toggles it, and `h` closes it from the group or one of its
        // rows, landing on the group. The state is the dashboard's own.
        KeyCode::Enter | KeyCode::Char('l')
            if matches!(order.get(selected_now), Some(PanelRow::HiddenGroup { .. })) =>
        {
            if let Some(group) = group_under_cursor(app, order.get(selected_now)) {
                let open =
                    k.code == KeyCode::Char('l') || !app.dashboard.expanded_hidden.contains(&group);
                set_group_expanded(app, group, open, selected_now, filter);
            }
        }
        KeyCode::Char('h') => {
            if let Some(group) = group_under_cursor(app, order.get(selected_now))
                .filter(|g| app.dashboard.expanded_hidden.contains(g))
            {
                set_group_expanded(app, group, false, selected_now, filter);
            }
        }
        // A split needs a workspace; on a group row it does nothing rather
        // than falling through to the arm's panel dismissal.
        KeyCode::Char('v') | KeyCode::Char('s')
            if matches!(order.get(selected_now), Some(PanelRow::HiddenGroup { .. })) => {}
        // 'l' mirrors the dashboard's vim-style attach binding.
        KeyCode::Enter | KeyCode::Char('l') => {
            if let Some(&PanelRow::Workspace(ws_id)) = order.get(selected_now) {
                // Mirror the dashboard-attach flow: clear the
                // alert, spawn (or resume) the PTY, switch view.
                app.workspace_needs_attention.remove(&ws_id);
                match ensure_workspace_session(app, ws_id)? {
                    AttachReady::Ok => {
                        if app
                            .primary_instance(ws_id)
                            .and_then(|i| app.sessions.get(i))
                            .is_some()
                        {
                            if let Some(restored) = restore_attached_state(app, ws_id) {
                                app.leader_pending = false;
                                app.view = View::Attached(restored);
                            }
                        }
                    }
                    AttachReady::AgentMissing => {
                        // Modal::AgentMissing is set; leave view alone.
                    }
                    AttachReady::Refused => {
                        // A live archive refused the attach; leave view alone.
                    }
                    AttachReady::WorktreeMissing => {
                        // The missing-worktree error is set; leave view alone.
                    }
                }
            }
            // Only close the Updates-panel if AgentMissing or the
            // missing-worktree error didn't replace the modal — otherwise
            // we'd wipe the new modal.
            if !matches!(
                app.modal,
                Some(Modal::AgentMissing { .. } | Modal::Error { .. })
            ) {
                app.modal = None;
            }
        }
        KeyCode::Char('v') | KeyCode::Char('s') => {
            // Vim-style splits: 'v' = vertical (panes side-by-side),
            // 's' = horizontal (stacked). Only valid when there's
            // already an attached pane to split — otherwise behaves
            // like Enter (just attach).
            let dir = if matches!(k.code, KeyCode::Char('v')) {
                SplitDirection::Vertical
            } else {
                SplitDirection::Horizontal
            };
            if let Some(&PanelRow::Workspace(ws_id)) = order.get(selected_now) {
                app.workspace_needs_attention.remove(&ws_id);
                match ensure_workspace_session(app, ws_id)? {
                    AttachReady::Ok => {
                        if let Some(instance) = app.primary_instance(ws_id)
                            && app.sessions.get(instance).is_some()
                        {
                            // Splitting from the dashboard targets the
                            // workspace's primary instance — preserves
                            // pre-multi-agent behavior (the leaf for a
                            // single-agent workspace is its primary).
                            let target = crate::ui::split::AttachTarget {
                                workspace_id: ws_id,
                                instance,
                            };
                            match &mut app.view {
                                View::Attached(state) => {
                                    // Same pane already focused: switch focus
                                    // instead of splitting onto itself.
                                    if state.focused_target() == Some(target) {
                                        // no-op
                                    } else if state.leaves().contains(&target) {
                                        // Already open in another pane —
                                        // just refocus there.
                                        if let Some(p) = state
                                            .tree
                                            .leaf_paths()
                                            .into_iter()
                                            .find(|p| state.tree.leaf_at(p) == Some(target))
                                        {
                                            state.focus = p;
                                        }
                                    } else {
                                        state.split(dir, target);
                                    }
                                }
                                _ => {
                                    // No attached pane yet — restore saved layout or attach plainly.
                                    if let Some(restored) = restore_attached_state(app, ws_id) {
                                        app.leader_pending = false;
                                        app.view = View::Attached(restored);
                                    }
                                }
                            }
                        }
                    }
                    AttachReady::AgentMissing => {
                        // Modal::AgentMissing is set; leave view alone.
                    }
                    AttachReady::Refused => {
                        // A live archive refused the attach; leave view alone.
                    }
                    AttachReady::WorktreeMissing => {
                        // The missing-worktree error is set; leave view alone.
                    }
                }
            }
            // Only close the Updates-panel if AgentMissing or the
            // missing-worktree error didn't replace the modal — otherwise
            // we'd wipe the new modal.
            if !matches!(
                app.modal,
                Some(Modal::AgentMissing { .. } | Modal::Error { .. })
            ) {
                app.modal = None;
            }
        }
        _ => {}
    }

    Ok(())
}

/// The hidden group the panel cursor is in: the group row itself, or one of
/// an expanded group's rows.
fn group_under_cursor(
    app: &App,
    row: Option<&PanelRow>,
) -> Option<(
    crate::data::store::RepoId,
    crate::data::hide_rules::HideRule,
)> {
    match row? {
        PanelRow::HiddenGroup { repo, rule } => Some((*repo, rule.clone())),
        PanelRow::Workspace(id) => app.hidden_group_of(*id),
    }
}

/// Open or close a hidden group, keeping the panel cursor on its row.
fn set_group_expanded(
    app: &mut App,
    group: (
        crate::data::store::RepoId,
        crate::data::hide_rules::HideRule,
    ),
    open: bool,
    selected: usize,
    filter: Option<String>,
) {
    if open {
        app.dashboard.expanded_hidden.insert(group.clone());
    } else {
        app.dashboard.expanded_hidden.remove(&group);
    }
    let (repo, rule) = group;
    let new_order = panel_order(app, filter.as_deref());
    app.modal = Some(Modal::UpdatesPanel {
        selected: reselect(
            Some(&PanelRow::HiddenGroup { repo, rule }),
            &new_order,
            selected,
        ),
        filter,
    });
}

pub(super) async fn process_list(
    app: &mut App,
    _shared: &SharedApp,
    k: crossterm::event::KeyEvent,
    workspace_id: crate::data::store::WorkspaceId,
    mut selected: usize,
    input: Option<String>,
    notice: Option<String>,
) -> Result<()> {
    let procs = app
        .workspace_processes
        .get(&workspace_id)
        .cloned()
        .unwrap_or_default();

    // Input mode: capture keystrokes into the command buffer.
    if let Some(mut buffer) = input {
        match k.code {
            KeyCode::Esc => {
                app.modal = Some(Modal::ProcessList {
                    workspace_id,
                    selected,
                    input: None,
                    notice,
                });
            }
            KeyCode::Enter => {
                let command = buffer.trim().to_string();
                if command.is_empty() {
                    // Empty command: stay in input mode, keep the buffer.
                    app.modal = Some(Modal::ProcessList {
                        workspace_id,
                        selected,
                        input: Some(buffer),
                        notice,
                    });
                } else {
                    let new_notice = launch_workspace_command(app, workspace_id, &command);
                    app.modal = Some(Modal::ProcessList {
                        workspace_id,
                        selected,
                        input: None,
                        notice: Some(new_notice),
                    });
                    // Best-effort: the just-spawned process may not have
                    // surfaced in `lsof` yet, so it usually appears on the
                    // next periodic scan rather than this one. The notice
                    // confirms the launch in the meantime.
                    rescan_processes(app).await;
                }
            }
            KeyCode::Backspace => {
                buffer.pop();
                app.modal = Some(Modal::ProcessList {
                    workspace_id,
                    selected,
                    input: Some(buffer),
                    notice,
                });
            }
            KeyCode::Char(c) => {
                buffer.push(c);
                app.modal = Some(Modal::ProcessList {
                    workspace_id,
                    selected,
                    input: Some(buffer),
                    notice,
                });
            }
            _ => {
                app.modal = Some(Modal::ProcessList {
                    workspace_id,
                    selected,
                    input: Some(buffer),
                    notice,
                });
            }
        }
        return Ok(());
    }

    // List mode.
    // ProcessList intentionally does NOT alias j/k to nav like the other
    // list modals: `k` here means SIGTERM and `K` means SIGKILL, so
    // vim-style movement would clash with the kill verbs. Arrow keys are
    // the only navigation; `r` opens the run-command input.
    match k.code {
        KeyCode::Esc => {
            app.modal = None;
        }
        KeyCode::Up => {
            selected = selected.saturating_sub(1);
            app.modal = Some(Modal::ProcessList {
                workspace_id,
                selected,
                input: None,
                notice,
            });
        }
        KeyCode::Down => {
            if !procs.is_empty() {
                selected = (selected + 1).min(procs.len() - 1);
            }
            app.modal = Some(Modal::ProcessList {
                workspace_id,
                selected,
                input: None,
                notice,
            });
        }
        KeyCode::Char('r') => {
            // Clear any prior launch notice when starting a fresh
            // command so a stale "started" line doesn't linger.
            app.modal = Some(Modal::ProcessList {
                workspace_id,
                selected,
                input: Some(String::new()),
                notice: None,
            });
        }
        KeyCode::Char('k') => {
            if let Some(p) = procs.get(selected) {
                let _ = crate::activity::proc::kill_pid(p.pid, "TERM").await;
                rescan_processes(app).await;
            }
        }
        KeyCode::Char('K') => {
            if let Some(p) = procs.get(selected) {
                let _ = crate::activity::proc::kill_pid(p.pid, "KILL").await;
                rescan_processes(app).await;
            }
        }
        _ => {}
    }

    Ok(())
}

pub(super) async fn repo_settings(
    app: &mut App,
    _shared: &SharedApp,
    k: crossterm::event::KeyEvent,
    repo_id: crate::data::store::RepoId,
    mut selected: usize,
) -> Result<()> {
    match k.code {
        KeyCode::Esc => {
            app.modal = None;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            selected = selected.saturating_sub(1);
            app.modal = Some(Modal::RepoSettings { repo_id, selected });
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let max = RepoSettingField::ALL.len() - 1;
            selected = (selected + 1).min(max);
            app.modal = Some(Modal::RepoSettings { repo_id, selected });
        }
        KeyCode::Enter => {
            let field =
                RepoSettingField::ALL[selected.min(RepoSettingField::ALL.len().saturating_sub(1))];
            app.pending_edit = Some(PendingEdit::Repo { repo_id, field });
            app.modal = None;
        }
        KeyCode::Char('d') => {
            let field =
                RepoSettingField::ALL[selected.min(RepoSettingField::ALL.len().saturating_sub(1))];
            let _ = apply_repo_setting(app, repo_id, field, "");
            let _ = app.refresh();
            app.modal = Some(Modal::RepoSettings { repo_id, selected });
        }
        _ => {}
    };
    Ok(())
}

pub(super) fn global_settings(
    app: &mut App,
    k: crossterm::event::KeyEvent,
    selected: usize,
) -> Result<()> {
    use crate::config::settings::SETTINGS;
    let selected = selected.min(SETTINGS.len() - 1);
    let def = &SETTINGS[selected];
    match k.code {
        KeyCode::Esc => app.modal = None,
        KeyCode::Up | KeyCode::Char('k') => {
            app.modal = Some(Modal::GlobalSettings {
                selected: selected.saturating_sub(1),
            });
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.modal = Some(Modal::GlobalSettings {
                selected: (selected + 1).min(SETTINGS.len() - 1),
            });
        }
        KeyCode::Enter => {
            let current = app.store.get_setting(def.key)?;
            match def.next_value(current.as_deref()) {
                Some(next) => {
                    if let Err(e) = apply_global_setting(app, def.key, &next) {
                        app.modal = Some(Modal::Error {
                            message: e.to_string(),
                        });
                    }
                }
                None => {
                    app.pending_edit = Some(PendingEdit::Global {
                        key: def.key,
                        selected,
                    });
                    app.modal = None;
                }
            }
        }
        KeyCode::Char('d') => {
            if let Err(e) = apply_global_setting(app, def.key, "") {
                app.modal = Some(Modal::Error {
                    message: e.to_string(),
                });
            }
        }
        _ => {}
    }
    Ok(())
}
