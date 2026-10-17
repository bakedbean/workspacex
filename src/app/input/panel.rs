//! Every panel a key can summon, and how each one opens. The surfaces' key
//! handlers open panels through [`Panel::open`], so a panel opens the same
//! way whichever key or surface summons it.

use crate::app::{App, SelectionTarget};
use crate::data::store::WorkspaceId;
use crate::error::Result;
use crate::ui::modal::Modal;

/// Every panel a key can summon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app::input) enum Panel {
    WorkspaceActions,
    HideRules,
    RepoSettings,
    WorkspaceTags,
    NameColor,
    ProcessList,
    RemoteHosts,
    Agents,
    Updates,
    PromptTag,
}

/// Where a panel is summoned from. The panel's target follows from it: the
/// selected row on the dashboard, the focused pane when attached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app::input) enum Surface {
    Dashboard,
    Attached(WorkspaceId),
}

impl Surface {
    /// The workspace a workspace panel acts on, if any.
    fn workspace(self, app: &App) -> Option<WorkspaceId> {
        match self {
            Self::Dashboard => match app.selected_target() {
                Some(SelectionTarget::Workspace(id)) => Some(id),
                _ => None,
            },
            Self::Attached(id) => Some(id),
        }
    }
}

impl Panel {
    /// Open this panel for `on`'s target, replacing `app.modal`. Leaves the
    /// modal untouched when the panel has nothing to open for (a workspace
    /// panel with a repo header selected).
    pub(in crate::app::input) fn open(self, app: &mut App, on: Surface) -> Result<()> {
        match self {
            Self::WorkspaceActions => {
                if on.workspace(app).is_some() {
                    app.modal = Some(Modal::WorkspaceActions);
                }
            }
            Self::HideRules => {
                if let Some(repo_id) = app.selected_repo()
                    && let Some(repo) = app.repos.iter().find(|r| r.id == repo_id)
                {
                    let (repo_name, rules) = (repo.name.clone(), repo.hide_rules.clone());
                    let rows = super::modal::pickers::hide_rule_rows(app, repo_id, &rules)?;
                    app.modal = Some(Modal::HideRules {
                        repo_id,
                        repo_name,
                        rules,
                        rows,
                        selected: 0,
                    });
                }
            }
            Self::RepoSettings => {
                let repo_id = app
                    .selected_repo()
                    .or_else(|| app.repos.first().map(|r| r.id));
                if let Some(repo_id) = repo_id {
                    app.modal = Some(Modal::RepoSettings {
                        repo_id,
                        selected: 0,
                    });
                }
            }
            Self::WorkspaceTags => {
                if let Some(ws) = on.workspace(app) {
                    super::modal::pickers::open_workspace_tags(app, ws, String::new(), 0, &[])?;
                }
            }
            // `current` is snapshotted here so the grid can mark the applied
            // color without re-reading the store each frame.
            Self::NameColor => {
                if let Some(ws_id) = on.workspace(app) {
                    let current = app
                        .workspaces
                        .iter()
                        .find(|(_, w)| w.id == ws_id)
                        .and_then(|(_, w)| w.name_color);
                    app.modal = Some(Modal::NameColorPicker {
                        workspace_id: ws_id,
                        current,
                        selected: 0,
                        filter: String::new(),
                    });
                }
            }
            Self::ProcessList => {
                if let Some(workspace_id) = on.workspace(app) {
                    app.modal = Some(Modal::ProcessList {
                        workspace_id,
                        selected: 0,
                        input: None,
                        notice: None,
                    });
                }
            }
            // No selection required: the fetch that follows (Enter in the
            // picker) targets a remote host, not the local tree.
            Self::RemoteHosts => {
                let hosts: Vec<(String, String)> = crate::commands::shared_hosts::list(&app.store)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|h| (h.name, h.dest))
                    .collect();
                app.modal = Some(if hosts.is_empty() {
                    Modal::Error {
                        message: "no shared hosts configured — add name=ssh-dest lines via `wsx config edit shared_hosts`".into(),
                    }
                } else {
                    Modal::RemoteHostPicker { hosts, selected: 0 }
                });
            }
            Self::Agents => {
                if let Some(workspace_id) = on.workspace(app) {
                    app.modal = Some(Modal::AgentsPanel {
                        workspace_id,
                        selected: 0,
                    });
                }
            }
            Self::Updates => {
                app.modal = Some(Modal::UpdatesPanel {
                    selected: 0,
                    filter: None,
                });
            }
            Self::PromptTag => {
                app.modal = Some(Modal::PromptTag(crate::ui::modal::PromptTagModal::pick()));
            }
        }
        Ok(())
    }
}
