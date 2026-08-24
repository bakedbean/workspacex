//! Modals for browsing tmux-shared workspaces on another wsx host.

use crate::app::{App, SharedApp};
use crate::error::Result;
use crate::ui::modal::Modal;
use crossterm::event::KeyCode;
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
    };
    Ok(())
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
