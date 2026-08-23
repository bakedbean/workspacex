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
//!   [`pickers`]    name color and usage window

pub(super) mod agents;
pub(super) mod panels;
pub(super) mod pickers;
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
        Modal::SetupProgress { .. } => workspace::setup_progress(app, shared, k).await?,
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
        Modal::UpdatesPanel {
            selected,
            sort,
            filter,
        } => panels::updates_panel(app, shared, k, selected, sort, filter).await?,
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
        Modal::RemoteListLoading { .. } => remote::remote_list_loading(app, shared, k).await?,
        Modal::RenameWorkspace {
            workspace_id,
            name_buffer,
            notice: _,
        } => workspace::rename_workspace(app, shared, k, workspace_id, name_buffer).await?,
    }
    Ok(())
}
