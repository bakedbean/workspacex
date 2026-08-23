//! Modals that act on one workspace: creating, renaming, and the
//! confirmations guarding destructive or disruptive changes.

use super::*;
use crate::app::{App, SelectionTarget, SharedApp, reconcile_create_result};
use crate::data::store::RepoId;
use crate::error::Result;
use crate::ui::modal::Modal;
use crossterm::event::{KeyCode, KeyModifiers};
// Test-only imports: the moved test modules access `draw_for_test`,
// `AttachedState`, `Arc`, and `Mutex` through `super::*` glob imports
// that cascade from the surrounding `tests` module.

// The parameters after `k` are exactly `Modal::NewWorkspace`'s fields,
// destructured by the dispatcher. Bundling them back into a struct would
// just re-create the variant we already matched on.
#[allow(clippy::too_many_arguments)]
pub(super) async fn new_workspace(
    app: &mut App,
    shared: &SharedApp,
    k: crossterm::event::KeyEvent,
    repo_id: RepoId,
    mut name_buffer: String,
    yolo: bool,
    ws_shared: bool,
    mut agent: crate::pty::session::AgentKind,
) -> Result<()> {
    match k.code {
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
    };
    Ok(())
}

pub(super) async fn confirm_archive(
    app: &mut App,
    shared: &SharedApp,
    k: crossterm::event::KeyEvent,
    workspace_id: crate::data::store::WorkspaceId,
) -> Result<()> {
    match k.code {
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
    };
    Ok(())
}

pub(super) async fn confirm_quit(
    app: &mut App,
    _shared: &SharedApp,
    k: crossterm::event::KeyEvent,
) -> Result<()> {
    match k.code {
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
    };
    Ok(())
}

pub(super) async fn confirm_share(
    app: &mut App,
    _shared: &SharedApp,
    k: crossterm::event::KeyEvent,
    workspace_id: crate::data::store::WorkspaceId,
) -> Result<()> {
    match k.code {
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
    };
    Ok(())
}

pub(super) async fn setup_progress(
    app: &mut App,
    _shared: &SharedApp,
    k: crossterm::event::KeyEvent,
) -> Result<()> {
    // A viewer onto App::in_flight, not an owner: Esc/Enter just
    // closes it, leaving the background create running. Every other
    // key is ignored.
    if matches!(k.code, KeyCode::Esc | KeyCode::Enter) {
        app.modal = None;
    }

    Ok(())
}

pub(super) async fn error(
    app: &mut App,
    _shared: &SharedApp,
    k: crossterm::event::KeyEvent,
) -> Result<()> {
    if matches!(k.code, KeyCode::Esc | KeyCode::Enter) {
        app.modal = None;
    }

    Ok(())
}

pub(super) async fn workspace_actions(
    app: &mut App,
    _shared: &SharedApp,
    k: crossterm::event::KeyEvent,
) -> Result<()> {
    match k.code {
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
    };
    Ok(())
}

pub(super) async fn rename_workspace(
    app: &mut App,
    _shared: &SharedApp,
    k: crossterm::event::KeyEvent,
    workspace_id: crate::data::store::WorkspaceId,
    mut name_buffer: String,
) -> Result<()> {
    match k.code {
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
    };
    Ok(())
}
