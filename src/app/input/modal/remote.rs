//! Modals for browsing tmux-shared workspaces on another wsx host.

use crate::app::{App, SharedApp};
use crate::error::Result;
use crate::ui::modal::{Modal, RemoteNewWorkspaceForm};
use crossterm::event::{KeyCode, KeyModifiers};
// Test-only imports: the moved test modules access `draw_for_test`,
// `AttachedState`, `Arc`, and `Mutex` through `super::*` glob imports
// that cascade from the surrounding `tests` module.

pub(super) async fn remote_workspace_list(
    app: &mut App,
    shared: &SharedApp,
    k: crossterm::event::KeyEvent,
    mut selected: usize,
    notice: Option<String>,
) -> Result<()> {
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
            // A live row attaches over ssh; a dormant one (no live session)
            // asks to share it on the host first. The remote list is left
            // intact so a later detach lands back on the same modal-less
            // dashboard.
            enum Pick {
                Attach(crate::app::RemoteTarget),
                Share(Modal),
            }
            let pick = app.remote_list.as_ref().and_then(|list| {
                let rows = crate::app::remote_rows(list);
                let row = rows.get(selected)?;
                Some(match row.attach_session() {
                    Some(tmux) => Pick::Attach(crate::app::RemoteTarget {
                        host_name: list.host_name.clone(),
                        dest: list.dest.clone(),
                        tmux: tmux.to_string(),
                    }),
                    None => Pick::Share(Modal::RemoteShareConfirm {
                        selected,
                        host_name: list.host_name.clone(),
                        repo: row.repo.to_string(),
                        workspace: row.workspace.to_string(),
                        shared: row.shared,
                    }),
                })
            });
            match pick {
                Some(Pick::Attach(target)) => {
                    if let Err(e) = crate::app::attach_remote(app, target, 80, 24) {
                        app.modal = Some(Modal::RemoteWorkspaceList {
                            selected,
                            notice: Some(format!("attach failed: {e}")),
                        });
                    }
                    // On success `attach_remote` set the view + cleared
                    // the modal; nothing more to do here.
                }
                Some(Pick::Share(confirm)) => app.modal = Some(confirm),
                None => {
                    app.modal = Some(Modal::RemoteWorkspaceList {
                        selected,
                        notice: Some("no live session to attach to".to_string()),
                    });
                }
            }
        }
        // `N` opens the form in yolo mode, as on the dashboard.
        KeyCode::Char(c @ ('n' | 'N')) => {
            let Some(list) = &app.remote_list else {
                app.modal = Some(Modal::RemoteWorkspaceList { selected, notice });
                return Ok(());
            };
            if !list.can_share {
                app.modal = Some(Modal::RemoteWorkspaceList {
                    selected,
                    notice: Some(format!(
                        "the wsx on {} is too old to create workspaces remotely",
                        list.host_name
                    )),
                });
                return Ok(());
            }
            let host_name = list.host_name.clone();
            let dest = list.dest.clone();
            // Default to the repo of the row the cursor is on.
            let repo = crate::app::remote_rows(list)
                .get(selected)
                .map(|r| r.repo.to_string());
            let yolo = c == 'N';
            let fetch_gen = app.alloc_remote_gen();
            app.modal = Some(Modal::RemoteReposLoading {
                selected,
                host_name: host_name.clone(),
                yolo,
                repo: repo.clone(),
            });
            let shared_clone = shared.clone();
            tokio::spawn(async move {
                let result = crate::commands::shared_hosts::fetch_remote_repos(&dest).await;
                crate::app::reconcile_remote_repos(
                    shared_clone,
                    fetch_gen,
                    crate::app::RemoteFormSeed {
                        selected,
                        host_name,
                        yolo,
                        repo,
                    },
                    result,
                )
                .await;
            });
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
                    let result = crate::commands::shared_hosts::fetch_shared_list(&dest).await;
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

    Ok(())
}

pub(super) async fn remote_host_picker(
    app: &mut App,
    shared: &SharedApp,
    k: crossterm::event::KeyEvent,
    hosts: Vec<(String, String)>,
    selected: usize,
) -> Result<()> {
    match k.code {
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
            open_remote_host(app, shared, name, dest);
        }
        _ => {}
    };
    Ok(())
}

/// Fetch `dest`'s shared-workspace list and show it, behind a loading
/// modal: Enter in the `H` picker, and a click on a `$hosts` chip.
pub(in crate::app::input) fn open_remote_host(
    app: &mut App,
    shared: &SharedApp,
    name: String,
    dest: String,
) {
    let fetch_gen = app.alloc_remote_gen();
    app.modal = Some(Modal::RemoteListLoading {
        host_name: name.clone(),
    });
    let shared_clone = shared.clone();
    tokio::spawn(async move {
        let result = crate::commands::shared_hosts::fetch_shared_list(&dest).await;
        crate::app::reconcile_remote_list(shared_clone, fetch_gen, name, dest, result).await;
    });
}

pub(super) async fn remote_list_loading(
    app: &mut App,
    _shared: &SharedApp,
    k: crossterm::event::KeyEvent,
) -> Result<()> {
    if k.code == KeyCode::Esc {
        // Close immediately and drop the pending generation so the
        // in-flight fetch's eventual reconcile is a no-op (its gen
        // guard checks `pending_remote_gen == Some(my_gen)`) rather
        // than reopening a modal after the user has backed out.
        app.modal = None;
        app.pending_remote_gen = None;
    }

    Ok(())
}

/// `y`/Enter shares the workspace on the host in the background and swaps to
/// `RemoteShareRunning`; `n`/Esc goes back to the list.
#[allow(clippy::too_many_arguments)]
pub(super) async fn remote_share_confirm(
    app: &mut App,
    shared: &SharedApp,
    k: crossterm::event::KeyEvent,
    selected: usize,
    host_name: String,
    repo: String,
    workspace: String,
    ws_shared: bool,
) -> Result<()> {
    match k.code {
        KeyCode::Char('y') | KeyCode::Enter => {
            let Some(dest) = app.remote_list.as_ref().map(|l| l.dest.clone()) else {
                app.modal = None;
                return Ok(());
            };
            let share_gen = app.alloc_remote_gen();
            app.modal = Some(Modal::RemoteShareRunning {
                selected,
                host_name,
                workspace: format!("{repo}/{workspace}"),
            });
            let shared_clone = shared.clone();
            tokio::spawn(async move {
                let result =
                    crate::commands::shared_hosts::share_remote(&dest, &repo, &workspace).await;
                crate::app::reconcile_remote_share(shared_clone, share_gen, selected, result).await;
            });
        }
        KeyCode::Char('n') | KeyCode::Esc => {
            app.modal = Some(Modal::RemoteWorkspaceList {
                selected,
                notice: None,
            });
        }
        _ => {
            app.modal = Some(Modal::RemoteShareConfirm {
                selected,
                host_name,
                repo,
                workspace,
                shared: ws_shared,
            });
        }
    }
    Ok(())
}

pub(super) async fn remote_share_running(
    app: &mut App,
    k: crossterm::event::KeyEvent,
    selected: usize,
) -> Result<()> {
    if k.code == KeyCode::Esc {
        // Drop the generation so the eventual reconcile no-ops instead of
        // yanking the user into an attach they backed out of. The host
        // finishes the share regardless; a refresh shows it.
        app.pending_remote_gen = None;
        app.modal = Some(Modal::RemoteWorkspaceList {
            selected,
            notice: Some("the share carries on on the host; [r] refreshes".to_string()),
        });
    }
    Ok(())
}

/// Esc while the host's repos load: back to the list, dropping the
/// generation so the eventual reconcile no-ops.
pub(super) async fn remote_repos_loading(
    app: &mut App,
    k: crossterm::event::KeyEvent,
    selected: usize,
) -> Result<()> {
    if k.code == KeyCode::Esc {
        app.pending_remote_gen = None;
        app.modal = Some(Modal::RemoteWorkspaceList {
            selected,
            notice: None,
        });
    }
    Ok(())
}

/// The remote new-workspace form: ↑/↓ pick the repo, Tab cycles the agent
/// (starting from the host's default), ^y toggles yolo, and Enter creates the
/// workspace on the host in the background and swaps to
/// `RemoteCreateRunning`.
pub(super) async fn remote_new_workspace(
    app: &mut App,
    shared: &SharedApp,
    k: crossterm::event::KeyEvent,
    mut form: RemoteNewWorkspaceForm,
) -> Result<()> {
    use crate::pty::session::AgentKind;
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    match k.code {
        KeyCode::Esc => {
            app.modal = Some(Modal::RemoteWorkspaceList {
                selected: form.selected,
                notice: None,
            });
            return Ok(());
        }
        KeyCode::Up => form.repo = form.repo.checked_sub(1).unwrap_or(form.repos.len() - 1),
        KeyCode::Down => form.repo = (form.repo + 1) % form.repos.len(),
        KeyCode::Tab => {
            // None (host default) → each kind in turn → back to None.
            form.agent = match form.agent {
                None => AgentKind::ALL.first().copied(),
                Some(a) => AgentKind::ALL
                    .iter()
                    .position(|k| *k == a)
                    .and_then(|i| AgentKind::ALL.get(i + 1).copied()),
            };
        }
        KeyCode::Char('y') if ctrl => form.yolo = !form.yolo,
        KeyCode::Backspace => {
            form.name_buffer.pop();
        }
        KeyCode::Char(c) if !ctrl => form.name_buffer.push(c),
        KeyCode::Enter => {
            if let Some(notice) = refuse_remote_name(app, &form) {
                form.notice = Some(notice);
                app.modal = Some(Modal::RemoteNewWorkspace(form));
                return Ok(());
            }
            // Name it here, as the local `n` does, so the running modal and
            // any error can say which workspace it is.
            if form.name_buffer.trim().is_empty() {
                form.name_buffer = crate::util::names::generate();
            }
            let Some(dest) = app.remote_list.as_ref().map(|l| l.dest.clone()) else {
                app.modal = None;
                return Ok(());
            };
            form.notice = None;
            let create_gen = app.alloc_remote_gen();
            app.modal = Some(Modal::RemoteCreateRunning(form.clone()));
            let shared_clone = shared.clone();
            tokio::spawn(async move {
                let result =
                    crate::commands::shared_hosts::create_remote(&dest, &form.request()).await;
                crate::app::reconcile_remote_create(shared_clone, create_gen, form, result).await;
            });
            return Ok(());
        }
        _ => {}
    }
    form.notice = None;
    app.modal = Some(Modal::RemoteNewWorkspace(form));
    Ok(())
}

/// Why the form's name can't be created on the host, caught before the
/// round trip: one that wouldn't survive the ssh quoting, or one the
/// listing already shows in that repo. The host validates the rest.
fn refuse_remote_name(app: &App, form: &RemoteNewWorkspaceForm) -> Option<String> {
    let name = form.name_buffer.trim();
    if name.is_empty() {
        return None;
    }
    if !crate::commands::shared_hosts::shell_safe(name) {
        return Some("use letters, digits and - _ . / + @ only".to_string());
    }
    let repo = &form.repos[form.repo];
    let taken = app.remote_list.as_ref().is_some_and(|l| {
        l.records
            .iter()
            .any(|r| &r.repo == repo && r.workspace == name)
    });
    taken.then(|| format!("{repo} already has a workspace named '{name}'"))
}

/// Esc while the host creates the workspace: back to the list. The create
/// carries on there; a refresh shows it.
pub(super) async fn remote_create_running(
    app: &mut App,
    k: crossterm::event::KeyEvent,
    form: RemoteNewWorkspaceForm,
) -> Result<()> {
    if k.code == KeyCode::Esc {
        app.pending_remote_gen = None;
        app.modal = Some(Modal::RemoteWorkspaceList {
            selected: form.selected,
            notice: Some("the create carries on on the host; [r] refreshes".to_string()),
        });
    }
    Ok(())
}
