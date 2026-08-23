//! Key handling while a modal is up.
//!
//! One arm per `Modal` variant. The match stays exhaustive so adding a
//! modal without teaching it to handle keys is a compile error.

use super::*;
use crate::app::{
    App, AttachReady, PendingEdit, RepoSettingField, SelectionTarget, SharedApp,
    apply_repo_setting, attach_workspace, ensure_instance_session, ensure_workspace_session,
    reconcile_create_result, rescan_processes, restore_attached_state,
};
use crate::error::Result;
use crate::ui::View;
use crate::ui::modal::Modal;
use crate::ui::modal::move_selection;
use crate::ui::split::SplitDirection;
use crossterm::event::{KeyCode, KeyModifiers};

// Test-only imports: the moved test modules access `draw_for_test`,
// `AttachedState`, `Arc`, and `Mutex` through `super::*` glob imports
// that cascade from the surrounding `tests` module.

/// Write a workspace's name color (or clear it with `None`), refresh so the
/// dashboard row repaints from the new value, and close the picker. Shared by
/// the picker's Enter/Delete keys and by a swatch click.
pub(in crate::app::input) fn apply_name_color(
    app: &mut App,
    ws_id: crate::data::store::WorkspaceId,
    color: Option<u8>,
) -> Result<()> {
    app.modal = None;
    // A failed write must not read as success: the picker has already closed,
    // so swallowing the error would leave the user believing the color was
    // saved. Surface it in the error modal instead.
    if let Err(e) = app.store.set_workspace_name_color(ws_id, color) {
        tracing::warn!(error = %e, "failed to persist workspace name color");
        app.modal = Some(Modal::Error {
            message: format!("could not save the name color: {e}"),
        });
        return Ok(());
    }
    app.refresh()?;
    Ok(())
}

pub(in crate::app::input) async fn handle_key_modal(
    app: &mut App,
    shared: &SharedApp,
    k: crossterm::event::KeyEvent,
) -> Result<()> {
    let modal = app.modal.clone().unwrap();
    match modal {
        Modal::NewWorkspace {
            repo_id,
            mut name_buffer,
            yolo,
            shared: ws_shared,
            mut agent,
            notice: _,
        } => match k.code {
            KeyCode::Esc => {
                app.modal = None;
            }
            KeyCode::Tab => {
                agent = match agent {
                    crate::pty::session::AgentKind::Claude => crate::pty::session::AgentKind::Pi,
                    crate::pty::session::AgentKind::Pi => crate::pty::session::AgentKind::Hermes,
                    crate::pty::session::AgentKind::Hermes => crate::pty::session::AgentKind::Codex,
                    crate::pty::session::AgentKind::Codex => crate::pty::session::AgentKind::Omp,
                    crate::pty::session::AgentKind::Omp => crate::pty::session::AgentKind::Claude,
                };
                app.modal = Some(Modal::NewWorkspace {
                    repo_id,
                    name_buffer,
                    yolo,
                    shared: ws_shared,
                    agent,
                    notice: None,
                });
            }
            KeyCode::Char('s') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                app.modal = Some(Modal::NewWorkspace {
                    repo_id,
                    name_buffer,
                    yolo,
                    shared: !ws_shared,
                    agent,
                    notice: None,
                });
            }
            KeyCode::Enter => {
                let name = if name_buffer.trim().is_empty() {
                    None
                } else {
                    Some(name_buffer.trim().to_string())
                };
                // F5 part 1: validate the name up front. Without this, the
                // most common no-row failure — the UNIQUE(repo_id, name)
                // violation when the user types a name that already exists
                // — produced no row, therefore no badge, therefore no
                // feedback at all; the modal just closed. Mirrors
                // `RenameWorkspace`'s `notice` field/shape above.
                if let Some(n) = &name {
                    let taken = app
                        .store
                        .workspaces(repo_id)
                        .map(|rows| rows.iter().any(|w| &w.name == n))
                        .unwrap_or(false);
                    if taken {
                        app.modal = Some(Modal::NewWorkspace {
                            repo_id,
                            name_buffer,
                            yolo,
                            shared: ws_shared,
                            agent,
                            notice: Some(format!("a workspace named '{n}' already exists")),
                        });
                        return Ok(());
                    }
                }
                // Resolve the final name here rather than letting
                // `create_with_app` auto-generate one when `name` is
                // `None` — `reconcile_create_result` needs the exact name
                // that will be (or would have been) inserted so it can
                // tell, on failure, whether a row exists at all (F5 part 2).
                let final_name = name.unwrap_or_else(crate::util::names::generate);
                let repo = app.repos.iter().find(|r| r.id == repo_id).unwrap().clone();
                let base = app.worktree_base.clone();
                let cancel = tokio_util::sync::CancellationToken::new();
                let create_gen = app.alloc_create_gen();
                let progress = crate::data::progress::SetupProgress::shared();
                app.modal = None;
                let shared_clone = shared.clone();
                let name_for_reconcile = final_name.clone();
                tokio::spawn(async move {
                    let result = crate::data::workspace::create_with_app(
                        shared_clone.clone(),
                        repo,
                        Some(final_name),
                        base,
                        yolo,
                        ws_shared,
                        agent,
                        progress,
                        cancel,
                    )
                    .await;
                    reconcile_create_result(
                        shared_clone,
                        create_gen,
                        repo_id,
                        name_for_reconcile,
                        result,
                    )
                    .await;
                });
            }
            KeyCode::Backspace => {
                name_buffer.pop();
                app.modal = Some(Modal::NewWorkspace {
                    repo_id,
                    name_buffer,
                    yolo,
                    shared: ws_shared,
                    agent,
                    notice: None,
                });
            }
            KeyCode::Char(c) => {
                name_buffer.push(c);
                app.modal = Some(Modal::NewWorkspace {
                    repo_id,
                    name_buffer,
                    yolo,
                    shared: ws_shared,
                    agent,
                    notice: None,
                });
            }
            _ => {}
        },
        Modal::ConfirmArchive {
            workspace_id,
            name: _,
        } => match k.code {
            KeyCode::Char('y') => {
                let (repo, ws) = {
                    let ws = app
                        .workspaces
                        .iter()
                        .find(|(_, w)| w.id == workspace_id)
                        .map(|(_, w)| w.clone());
                    let repo = ws
                        .as_ref()
                        .and_then(|w| app.repos.iter().find(|r| r.id == w.repo_id).cloned());
                    match (repo, ws) {
                        (Some(r), Some(w)) => (r, w),
                        _ => {
                            app.modal = None;
                            return Ok(());
                        }
                    }
                };
                let archive_gen = app.alloc_archive_gen();
                let progress = crate::data::progress::SetupProgress::shared();
                app.in_flight.insert(
                    ws.id,
                    crate::data::in_flight::InFlight::archive(
                        progress.clone(),
                        tokio_util::sync::CancellationToken::new(),
                    ),
                );
                app.modal = None;
                let shared_clone = shared.clone();
                let ws_id = ws.id;
                tokio::spawn(async move {
                    let result = crate::data::workspace::archive_with_app(
                        shared_clone.clone(),
                        repo,
                        ws,
                        crate::data::workspace::ArchiveOpts {
                            force_branch_delete: true,
                            ..Default::default()
                        },
                    )
                    .await;
                    crate::app::reconcile_archive_result(shared_clone, archive_gen, ws_id, result)
                        .await;
                });
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                app.modal = None;
            }
            _ => {}
        },
        Modal::ConfirmQuit { .. } => match k.code {
            KeyCode::Char('y') => {
                // Cancel creates on the way out so their rows land on Cancelled
                // rather than waiting for the next startup sweep to resolve them.
                // Archive has no cancellation and is simply abandoned; it is
                // self-healing, since remove_worktree falls back to remove_dir_all
                // once git no longer recognises the path.
                //
                // Firing the token alone is not enough: the detached task
                // never gets a chance to observe it before shutdown, so the
                // task-level `set_setup_status(Cancelled)` writes in
                // `workspace::create`/`create_with_app` may never run. Worse,
                // a create still in its fetch phase hasn't even written
                // `SetupStatus::Running` yet, so the startup sweep (which only
                // repairs rows stuck on `Running`) would not repair it either
                // — leaving a row that looks healthy but has no dependencies
                // installed. Persist a terminal status synchronously here,
                // in the same locked handler, before quitting. This must not
                // block on the tasks themselves finishing — that would
                // reintroduce exactly the quit-time blocking this feature
                // removed.
                for (id, f) in app.in_flight.iter() {
                    if f.kind == crate::data::in_flight::InFlightKind::Create {
                        f.cancel.cancel();
                        if let Err(e) = app
                            .store
                            .set_setup_status(*id, crate::data::store::SetupStatus::Cancelled)
                        {
                            tracing::warn!(
                                error = %e,
                                "failed to persist Cancelled on an in-flight create while quitting"
                            );
                        }
                    }
                }
                app.quit = true;
            }
            KeyCode::Char('n') | KeyCode::Esc => app.modal = None,
            _ => {}
        },
        Modal::ConfirmShare { workspace_id, .. } => match k.code {
            KeyCode::Char('y') => {
                if let Err(e) = crate::app::toggle_workspace_shared(app, workspace_id) {
                    app.modal = Some(Modal::Error {
                        message: e.to_string(),
                    });
                } else if !matches!(app.modal, Some(Modal::AgentMissing { .. })) {
                    // Only clear the modal if toggle_workspace_shared didn't
                    // leave an AgentMissing modal up for the user (mirrors the
                    // UpdatesPanel Enter handler's rule above) — otherwise
                    // we'd wipe that modal right back off.
                    app.modal = None;
                }
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                app.modal = None;
            }
            _ => {}
        },
        Modal::SetupProgress { .. } => {
            // A viewer onto App::in_flight, not an owner: Esc/Enter just
            // closes it, leaving the background create running. Every other
            // key is ignored.
            if matches!(k.code, KeyCode::Esc | KeyCode::Enter) {
                app.modal = None;
            }
        }
        Modal::Error { .. } => {
            if matches!(k.code, KeyCode::Esc | KeyCode::Enter) {
                app.modal = None;
            }
        }
        Modal::NameColorPicker {
            workspace_id,
            current,
            selected,
            filter,
        } => {
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
        }
        Modal::WorkspaceActions => match k.code {
            // Dismiss without side effects.
            KeyCode::Esc | KeyCode::Char('?') => {
                app.modal = None;
            }
            // Vertical navigation moves the dashboard selection underneath
            // while the reference card stays open, so the user can target a
            // workspace and then fire an action against it.
            KeyCode::Up | KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('k') => {
                handle_key_dashboard(app, k).await?;
            }
            // Workspace actions (and Enter/open) act on the current selection,
            // then close the card.
            KeyCode::Char('e')
            | KeyCode::Char('t')
            | KeyCode::Char('v')
            | KeyCode::Char('g')
            | KeyCode::Char('c')
            | KeyCode::Char('C')
            | KeyCode::Enter => {
                app.modal = None;
                handle_key_dashboard(app, k).await?;
            }
            // Rename is handled in-modal (not forwarded): bare `r` on the
            // dashboard is the PM-digest refresh nudge.
            KeyCode::Char('r') => {
                let ws = match app.selected_target() {
                    Some(SelectionTarget::Workspace(ws_id)) => app
                        .workspaces
                        .iter()
                        .find(|(_, w)| w.id == ws_id)
                        .map(|(_, w)| (w.id, w.name.clone())),
                    _ => None,
                };
                app.modal = ws.map(|(workspace_id, name_buffer)| Modal::RenameWorkspace {
                    workspace_id,
                    name_buffer,
                    notice: None,
                });
            }
            // Open the progress viewer for a workspace with work in flight.
            KeyCode::Char('o') => {
                if let Some(SelectionTarget::Workspace(ws_id)) = app.selected_target()
                    && app.in_flight.contains_key(&ws_id)
                {
                    app.modal = Some(Modal::SetupProgress {
                        workspace_id: ws_id,
                    });
                }
            }
            // Cancel an in-flight CREATE. Archive is not cancellable.
            KeyCode::Char('x') => {
                if let Some(SelectionTarget::Workspace(ws_id)) = app.selected_target()
                    && let Some(f) = app.in_flight.get(&ws_id)
                    && f.kind == crate::data::in_flight::InFlightKind::Create
                {
                    f.cancel.cancel();
                    app.modal = None;
                }
            }
            // Everything else is inert while the card is open.
            _ => {}
        },
        Modal::UpdatesPanel {
            selected,
            sort,
            filter,
        } => {
            let selected_now = selected;
            // Build the same ordered workspace list the renderer uses, so
            // arrow keys and Enter operate on the same indices.
            let order = panel_order(app, sort, filter.as_deref());
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
                    let selected_id = order.get(selected_now).copied();
                    let new_order = panel_order(app, sort, new_filter.as_deref());
                    app.modal = Some(Modal::UpdatesPanel {
                        selected: reselect(selected_id, &new_order, selected_now),
                        sort,
                        filter: new_filter,
                    });
                    return Ok(());
                }
            }
            match k.code {
                KeyCode::Esc => {
                    app.modal = None;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    let new_sel = selected_now.saturating_sub(1);
                    app.modal = Some(Modal::UpdatesPanel {
                        selected: new_sel,
                        sort,
                        filter: filter.clone(),
                    });
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let max = order.len().saturating_sub(1);
                    let new_sel = (selected_now + 1).min(max);
                    app.modal = Some(Modal::UpdatesPanel {
                        selected: new_sel,
                        sort,
                        filter: filter.clone(),
                    });
                }
                // 'o' (order) cycles the sort mode. The cursor follows the
                // selected workspace to its new row rather than staying on
                // the same index.
                KeyCode::Char('o') => {
                    let selected_id = order.get(selected_now).copied();
                    let new_sort = sort.cycle();
                    let new_order = panel_order(app, new_sort, filter.as_deref());
                    let new_sel = reselect(selected_id, &new_order, selected_now);
                    app.modal = Some(Modal::UpdatesPanel {
                        selected: new_sel,
                        sort: new_sort,
                        filter: filter.clone(),
                    });
                }
                // `/` arms filter mode. Reached only when `filter` is None —
                // an active buffer swallows printable keys above.
                KeyCode::Char('/') => {
                    app.modal = Some(Modal::UpdatesPanel {
                        selected: selected_now,
                        sort,
                        filter: Some(String::new()),
                    });
                }
                // 'l' mirrors the dashboard's vim-style attach binding.
                KeyCode::Enter | KeyCode::Char('l') => {
                    if let Some(ws_id) = order.get(selected_now).copied() {
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
                        }
                    }
                    // Only close the Updates-panel if AgentMissing didn't
                    // replace the modal — otherwise we'd wipe the new modal.
                    if !matches!(app.modal, Some(Modal::AgentMissing { .. })) {
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
                    if let Some(ws_id) = order.get(selected_now).copied() {
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
                                                if let Some(p) =
                                                    state.tree.leaf_paths().into_iter().find(|p| {
                                                        state.tree.leaf_at(p) == Some(target)
                                                    })
                                                {
                                                    state.focus = p;
                                                }
                                            } else {
                                                state.split(dir, target);
                                            }
                                        }
                                        _ => {
                                            // No attached pane yet — restore saved layout or attach plainly.
                                            if let Some(restored) =
                                                restore_attached_state(app, ws_id)
                                            {
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
                        }
                    }
                    // Only close the Updates-panel if AgentMissing didn't
                    // replace the modal — otherwise we'd wipe the new modal.
                    if !matches!(app.modal, Some(Modal::AgentMissing { .. })) {
                        app.modal = None;
                    }
                }
                _ => {}
            }
        }
        Modal::ProcessList {
            workspace_id,
            mut selected,
            input,
            notice,
        } => {
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
        }
        Modal::RepoSettings {
            repo_id,
            mut selected,
        } => match k.code {
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
                let field = RepoSettingField::ALL
                    [selected.min(RepoSettingField::ALL.len().saturating_sub(1))];
                app.pending_edit = Some(PendingEdit { repo_id, field });
                app.modal = None;
            }
            KeyCode::Char('d') => {
                let field = RepoSettingField::ALL
                    [selected.min(RepoSettingField::ALL.len().saturating_sub(1))];
                let _ = apply_repo_setting(app, repo_id, field, "");
                let _ = app.refresh();
                app.modal = Some(Modal::RepoSettings { repo_id, selected });
            }
            _ => {}
        },
        Modal::AgentMissing { ws_id, agent, .. } => match k.code {
            KeyCode::Esc | KeyCode::Enter => {
                app.modal = None;
            }
            KeyCode::Char('s') => {
                let selected = crate::pty::session::AgentKind::ALL
                    .iter()
                    .position(|k| *k == agent)
                    .unwrap_or(0);
                app.modal = Some(Modal::AgentPicker {
                    ws_id,
                    selected,
                    current: agent,
                });
            }
            _ => {}
        },
        Modal::AgentPicker {
            ws_id,
            selected,
            current,
        } => {
            use crate::pty::session::AgentKind;
            match k.code {
                KeyCode::Esc => {
                    app.modal = None;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    let new_sel = selected.saturating_sub(1);
                    app.modal = Some(Modal::AgentPicker {
                        ws_id,
                        selected: new_sel,
                        current,
                    });
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let new_sel = (selected + 1).min(AgentKind::ALL.len() - 1);
                    app.modal = Some(Modal::AgentPicker {
                        ws_id,
                        selected: new_sel,
                        current,
                    });
                }
                KeyCode::Enter => {
                    let new_agent = AgentKind::ALL[selected];
                    app.store.set_workspace_agent(ws_id, new_agent)?;
                    // Mirror to in-memory copy so the dashboard doesn't show stale
                    // agent until poll_external_changes catches up.
                    if let Some((_, ws)) = app.workspaces.iter_mut().find(|(_, w)| w.id == ws_id) {
                        ws.agent = new_agent;
                    }
                    app.modal = None;
                    attach_workspace(app, ws_id)?;
                }
                _ => {}
            }
        }
        Modal::AgentsPanel {
            workspace_id,
            selected,
        } => {
            use crate::pty::session::AgentKind;
            match k.code {
                KeyCode::Esc => {
                    app.modal = None;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    app.modal = Some(Modal::AgentsPanel {
                        workspace_id,
                        selected: selected.saturating_sub(1),
                    });
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    app.modal = Some(Modal::AgentsPanel {
                        workspace_id,
                        selected: (selected + 1).min(AgentKind::ALL.len() - 1),
                    });
                }
                KeyCode::Enter => {
                    // Defensively bound the index: navigation clamps `selected`,
                    // but guard against a stale/large value so this can never panic.
                    let idx = selected.min(AgentKind::ALL.len().saturating_sub(1));
                    let kind = AgentKind::ALL[idx];
                    let inst = app.store.add_workspace_agent(workspace_id, kind)?;
                    // Spawn it now. ensure_instance_session sets Modal::AgentMissing
                    // (and returns AgentMissing) if the binary is absent — in that
                    // case leave that modal up; otherwise close the panel. Refused
                    // (a live archive) is vanishingly unlikely here — the panel
                    // requires an attached workspace — but is handled the same way.
                    match ensure_instance_session(app, inst.id, true)? {
                        AttachReady::AgentMissing | AttachReady::Refused => {}
                        AttachReady::Ok => app.modal = None,
                    }
                    // Refill `agent_roster` so it reflects the new instance —
                    // nothing else on this path goes through `refresh()`.
                    app.refresh()?;
                }
                KeyCode::Char('a') => {
                    for kind in AgentKind::ALL {
                        let inst = app.store.add_workspace_agent(workspace_id, kind)?;
                        let _ = ensure_instance_session(app, inst.id, true)?;
                    }
                    app.modal = None;
                    // Refill `agent_roster` so it reflects the four new
                    // instances — nothing else on this path goes through
                    // `refresh()`.
                    app.refresh()?;
                }
                KeyCode::Char('x') => {
                    // Remove the most-recently-added non-primary instance.
                    if let Some(last) = app
                        .store
                        .workspace_agents(workspace_id)?
                        .into_iter()
                        .rfind(|i| !i.is_primary)
                    {
                        app.sessions.remove(last.id);
                        app.store.remove_workspace_agent(last.id)?;
                        // Refill `agent_roster` so it reflects the removal —
                        // nothing else on this path goes through `refresh()`.
                        app.refresh()?;
                    }
                }
                _ => {}
            }
        }
        Modal::UsageWindowPicker { selected } => match k.code {
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
        },
        Modal::RemoteWorkspaceList {
            mut selected,
            notice,
        } => {
            let row_count = app
                .remote_list
                .as_ref()
                .map(|l| crate::app::remote_rows(l).len())
                .unwrap_or(0);
            match k.code {
                // Ephemeral contract: the listing only exists for this
                // modal's lifetime. Esc discards both the modal and the
                // fetched data so nothing stale lingers on `App` for the
                // next `H` fetch to trip over.
                KeyCode::Esc => {
                    app.modal = None;
                    app.remote_list = None;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    selected = selected.saturating_sub(1);
                    app.modal = Some(Modal::RemoteWorkspaceList { selected, notice });
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if row_count > 0 {
                        selected = (selected + 1).min(row_count - 1);
                    }
                    app.modal = Some(Modal::RemoteWorkspaceList { selected, notice });
                }
                KeyCode::Enter => {
                    // Resolve the selected row's tmux session (only alive rows
                    // carry one) and the host from `remote_list`, then attach
                    // over ssh. The remote list is left intact so a later
                    // detach lands back on the same modal-less dashboard.
                    let target = app.remote_list.as_ref().and_then(|list| {
                        let rows = crate::app::remote_rows(list);
                        rows.get(selected).and_then(|r| {
                            r.alive.then_some(r.tmux_session).flatten().map(|tmux| {
                                crate::app::RemoteTarget {
                                    host_name: list.host_name.clone(),
                                    dest: list.dest.clone(),
                                    tmux: tmux.to_string(),
                                }
                            })
                        })
                    });
                    match target {
                        Some(target) => {
                            if let Err(e) = crate::app::attach_remote(app, target, 80, 24) {
                                app.modal = Some(Modal::RemoteWorkspaceList {
                                    selected,
                                    notice: Some(format!("attach failed: {e}")),
                                });
                            }
                            // On success `attach_remote` set the view + cleared
                            // the modal; nothing more to do here.
                        }
                        None => {
                            app.modal = Some(Modal::RemoteWorkspaceList {
                                selected,
                                notice: Some("no live session to attach to".to_string()),
                            });
                        }
                    }
                }
                KeyCode::Char('r') => {
                    // Re-run the host picker's Enter flow for the same host:
                    // same gen allocation / RemoteListLoading / reconcile
                    // path, just triggered from inside the list instead of
                    // the host picker.
                    if let Some(list) = &app.remote_list {
                        let host_name = list.host_name.clone();
                        let dest = list.dest.clone();
                        let fetch_gen = app.alloc_remote_gen();
                        app.modal = Some(Modal::RemoteListLoading {
                            host_name: host_name.clone(),
                        });
                        let shared_clone = shared.clone();
                        tokio::spawn(async move {
                            let result =
                                crate::commands::shared_hosts::fetch_shared_list(&dest).await;
                            crate::app::reconcile_remote_list(
                                shared_clone,
                                fetch_gen,
                                host_name,
                                dest,
                                result,
                            )
                            .await;
                        });
                    } else {
                        app.modal = Some(Modal::RemoteWorkspaceList { selected, notice });
                    }
                }
                _ => {
                    app.modal = Some(Modal::RemoteWorkspaceList { selected, notice });
                }
            }
        }
        Modal::RemoteHostPicker { hosts, selected } => match k.code {
            KeyCode::Esc => {
                app.modal = None;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let new_sel = selected.saturating_sub(1);
                app.modal = Some(Modal::RemoteHostPicker {
                    hosts,
                    selected: new_sel,
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let new_sel = (selected + 1).min(hosts.len().saturating_sub(1));
                app.modal = Some(Modal::RemoteHostPicker {
                    hosts,
                    selected: new_sel,
                });
            }
            KeyCode::Enter => {
                // `hosts` is only ever populated by the `H` dashboard arm,
                // which refuses to open this modal when the list is empty
                // (it opens Modal::Error instead) — so indexing here can't
                // panic.
                let (name, dest) = hosts[selected].clone();
                let fetch_gen = app.alloc_remote_gen();
                app.modal = Some(Modal::RemoteListLoading {
                    host_name: name.clone(),
                });
                let shared_clone = shared.clone();
                tokio::spawn(async move {
                    let result = crate::commands::shared_hosts::fetch_shared_list(&dest).await;
                    crate::app::reconcile_remote_list(shared_clone, fetch_gen, name, dest, result)
                        .await;
                });
            }
            _ => {}
        },
        Modal::RemoteListLoading { .. } => {
            if k.code == KeyCode::Esc {
                // Close immediately and drop the pending generation so the
                // in-flight fetch's eventual reconcile is a no-op (its gen
                // guard checks `pending_remote_gen == Some(my_gen)`) rather
                // than reopening a modal after the user has backed out.
                app.modal = None;
                app.pending_remote_gen = None;
            }
        }
        Modal::RenameWorkspace {
            workspace_id,
            mut name_buffer,
            notice: _,
        } => match k.code {
            KeyCode::Esc => {
                app.modal = None;
            }
            KeyCode::Enter => {
                match crate::data::workspace::normalize_slug(&name_buffer) {
                    None => {
                        app.modal = Some(Modal::RenameWorkspace {
                            workspace_id,
                            name_buffer,
                            notice: Some("name cannot be empty".to_string()),
                        });
                    }
                    Some(slug) => {
                        let ws = app
                            .workspaces
                            .iter()
                            .find(|(_, w)| w.id == workspace_id)
                            .map(|(_, w)| w.clone());
                        let repo = ws
                            .as_ref()
                            .and_then(|w| app.repos.iter().find(|r| r.id == w.repo_id).cloned());
                        match (ws, repo) {
                            (Some(ws), Some(repo)) if slug != ws.name => {
                                match crate::data::workspace::rename(&app.store, &repo, &ws, &slug)
                                    .await
                                {
                                    Ok(()) => {
                                        app.modal = None;
                                        app.refresh()?;
                                    }
                                    Err(e) => {
                                        // Git stderr can span lines; the notice
                                        // renders on a single modal line.
                                        let msg = e
                                            .to_string()
                                            .split_whitespace()
                                            .collect::<Vec<_>>()
                                            .join(" ");
                                        app.modal = Some(Modal::RenameWorkspace {
                                            workspace_id,
                                            name_buffer,
                                            notice: Some(format!("rename failed: {msg}")),
                                        });
                                    }
                                }
                            }
                            // Unchanged name: nothing to do.
                            (Some(_), Some(_)) => {
                                app.modal = None;
                            }
                            // Workspace/repo vanished underneath (archived
                            // elsewhere): close quietly and resync.
                            _ => {
                                app.modal = None;
                                app.refresh()?;
                            }
                        }
                    }
                }
            }
            KeyCode::Backspace => {
                name_buffer.pop();
                app.modal = Some(Modal::RenameWorkspace {
                    workspace_id,
                    name_buffer,
                    notice: None,
                });
            }
            KeyCode::Char(c)
                if !k.modifiers.contains(KeyModifiers::CONTROL)
                    && !k.modifiers.contains(KeyModifiers::ALT) =>
            {
                name_buffer.push(c);
                app.modal = Some(Modal::RenameWorkspace {
                    workspace_id,
                    name_buffer,
                    notice: None,
                });
            }
            _ => {}
        },
    }
    Ok(())
}
