//! Key handling while a modal is up.
//!
//! `handle_key_modal` is a dispatcher and nothing else: one arm per `Modal`
//! variant, each delegating to a handler that takes that variant's own fields
//! as parameters, so a handler's signature states exactly what state its
//! modal carries. The match stays exhaustive on purpose -- adding a modal
//! without teaching it to handle keys is a compile error.
//!
//!   [`workspace`]  create, rename, and the confirmations
//!   [`panels`]     updates, process list, repo settings
//!   [`agents`]     agent picker, agents panel, missing-agent notice
//!   [`remote`]     shared-workspace browsing on another host
//!   [`pickers`]    name color, workspace tags, usage window, hide rules
//!   [`prompt_tag`]  the XML prompt-tag picker and body box

pub(super) mod agents;
pub(super) mod panels;
pub(super) mod pickers;
pub(super) mod prompt_tag;
pub(super) mod remote;
pub(super) mod workspace;

use super::*;
use crate::app::{App, SharedApp};
use crate::error::Result;
use crate::ui::modal::Modal;

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

/// The dashboard keys that open a drawer card with no text input: `?` the
/// workspace actions, `F` the hide rules, `s` the repo settings. While one of
/// those cards is up, its own key closes it and another's key replaces it,
/// so the last key pressed always wins the drawer.
const DRAWER_SWITCH_KEYS: [char; 3] = ['?', 'F', 's'];

/// The [`DRAWER_SWITCH_KEYS`] key that opens `modal`, if it is one of those
/// cards.
fn drawer_key(modal: &Modal) -> Option<char> {
    match modal {
        Modal::WorkspaceActions => Some('?'),
        Modal::HideRules { .. } => Some('F'),
        Modal::RepoSettings { .. } => Some('s'),
        _ => None,
    }
}

/// Handle a drawer-switch key pressed in one of the [`DRAWER_SWITCH_KEYS`]
/// cards. Returns `true` when the key was consumed. The card's own key closes
/// it outright, even when the selection has since moved somewhere that key
/// would not open it. Another card's key that would not open for the current
/// selection (`?` on a repo header) leaves the open card in place rather than
/// closing it to nothing.
pub(super) async fn switch_drawer(app: &mut App, k: crossterm::event::KeyEvent) -> Result<bool> {
    let crossterm::event::KeyCode::Char(c) = k.code else {
        return Ok(false);
    };
    if !DRAWER_SWITCH_KEYS.contains(&c) {
        return Ok(false);
    }
    let Some(open) = app.modal.take() else {
        return Ok(false);
    };
    if drawer_key(&open) == Some(c) {
        return Ok(true);
    }
    let opened = handle_key_dashboard(app, k).await;
    if app.modal.is_none() {
        app.modal = Some(open);
    }
    opened.map(|()| true)
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
            name_buffer,
            yolo,
            shared: ws_shared,
            agent,
            notice: _,
        } => {
            workspace::new_workspace(app, shared, k, repo_id, name_buffer, yolo, ws_shared, agent)
                .await?
        }
        Modal::ConfirmArchive {
            workspace_id,
            name: _,
        } => workspace::confirm_archive(app, shared, k, workspace_id).await?,
        Modal::ConfirmQuit { .. } => workspace::confirm_quit(app, shared, k).await?,
        Modal::ConfirmShare { workspace_id, .. } => {
            workspace::confirm_share(app, shared, k, workspace_id).await?
        }
        Modal::SetupLog { .. } => workspace::setup_log(app, shared, k).await?,
        Modal::Error { .. } => workspace::error(app, shared, k).await?,
        Modal::NameColorPicker {
            workspace_id,
            current,
            selected,
            filter,
        } => {
            pickers::name_color_picker(app, shared, k, workspace_id, current, selected, filter)
                .await?
        }
        Modal::WorkspaceActions => workspace::workspace_actions(app, shared, k).await?,
        Modal::UpdatesPanel { selected, filter } => {
            panels::updates_panel(app, shared, k, selected, filter).await?
        }
        Modal::ProcessList {
            workspace_id,
            selected,
            input,
            notice,
        } => panels::process_list(app, shared, k, workspace_id, selected, input, notice).await?,
        Modal::RepoSettings { repo_id, selected } => {
            panels::repo_settings(app, shared, k, repo_id, selected).await?
        }
        Modal::AgentMissing { ws_id, agent, .. } => {
            agents::agent_missing(app, shared, k, ws_id, agent).await?
        }
        Modal::AgentPicker {
            ws_id,
            selected,
            current,
        } => agents::agent_picker(app, shared, k, ws_id, selected, current).await?,
        Modal::AgentsPanel {
            workspace_id,
            selected,
        } => agents::agents_panel(app, shared, k, workspace_id, selected).await?,
        Modal::UsageWindowPicker { selected } => {
            pickers::usage_window_picker(app, shared, k, selected).await?
        }
        Modal::RemoteWorkspaceList { selected, notice } => {
            remote::remote_workspace_list(app, shared, k, selected, notice).await?
        }
        Modal::RemoteHostPicker { hosts, selected } => {
            remote::remote_host_picker(app, shared, k, hosts, selected).await?
        }
        Modal::WorkspaceTags {
            workspace_id,
            name,
            known,
            tags,
            input,
            selected,
            browsing,
            ..
        } => {
            pickers::workspace_tags_editor(
                app,
                k,
                workspace_id,
                name,
                known,
                tags,
                input,
                selected,
                browsing,
            )
            .await?
        }
        Modal::HideRules {
            repo_id,
            repo_name,
            rules,
            rows,
            selected,
        } => pickers::hide_rules_picker(app, k, repo_id, repo_name, rules, rows, selected).await?,
        Modal::RemoteListLoading { .. } => remote::remote_list_loading(app, shared, k).await?,
        Modal::RemoteShareConfirm {
            selected,
            host_name,
            repo,
            workspace,
            shared: ws_shared,
        } => {
            remote::remote_share_confirm(
                app, shared, k, selected, host_name, repo, workspace, ws_shared,
            )
            .await?
        }
        Modal::RemoteShareRunning { selected, .. } => {
            remote::remote_share_running(app, k, selected).await?
        }
        Modal::RemoteReposLoading { selected, .. } => {
            remote::remote_repos_loading(app, k, selected).await?
        }
        Modal::RemoteNewWorkspace(form) => {
            remote::remote_new_workspace(app, shared, k, form).await?
        }
        Modal::RemoteCreateRunning(form) => remote::remote_create_running(app, k, form).await?,
        Modal::RenameWorkspace {
            workspace_id,
            name_buffer,
            notice: _,
        } => workspace::rename_workspace(app, shared, k, workspace_id, name_buffer).await?,
        Modal::PromptTag(modal) => prompt_tag::prompt_tag(app, shared, k, modal).await?,
    }
    Ok(())
}
