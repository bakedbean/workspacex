//! The bottom panel's summon rules: one table of every key that opens a
//! panel, shared by the surfaces that open panels and by an open panel.
//!
//! Every panel follows the same rules:
//!
//! - a panel's key opens it, replacing whatever panel is already up;
//! - the open panel's own key closes it, even if the selection has since
//!   moved somewhere that key would not open it;
//! - a key that opens nothing for the current selection (`?` on a repo
//!   header) leaves the open panel where it is.
//!
//! A panel keeps a summon key for itself only while it is taking text, or
//! when the key is one of its own verbs (see [`claims`]). Progress cards for
//! work in flight keep every key.
//!
//! The surfaces' key handlers open panels through [`Panel::open`] too, so the
//! key that opens a panel from the dashboard and the key that switches to it
//! from another panel cannot drift apart.

use crate::app::{App, SelectionTarget};
use crate::data::store::WorkspaceId;
use crate::error::Result;
use crate::ui::View;
use crate::ui::modal::Modal;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Every panel a key can summon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app::input) enum Panel {
    WorkspaceActions,
    HideRules,
    RepoSettings,
    GlobalSettings,
    WorkspaceTags,
    NameColor,
    ProcessList,
    RemoteHosts,
    Agents,
    Updates,
    PromptTag,
    Recap,
}

/// Where a panel is summoned from. The panel's target follows from it: the
/// selected row on the dashboard, the focused pane when attached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app::input) enum Surface {
    Dashboard,
    Attached(WorkspaceId),
}

/// A summon key: a bare key, or the key after the `Ctrl-x` leader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Chord {
    Key(char),
    Leader(char),
}

const DASHBOARD: &[(Chord, Panel)] = &[
    (Chord::Key('?'), Panel::WorkspaceActions),
    (Chord::Key('F'), Panel::HideRules),
    (Chord::Key('s'), Panel::RepoSettings),
    (Chord::Key(','), Panel::GlobalSettings),
    (Chord::Key('#'), Panel::WorkspaceTags),
    (Chord::Key('C'), Panel::NameColor),
    (Chord::Key('K'), Panel::ProcessList),
    (Chord::Key('H'), Panel::RemoteHosts),
    (Chord::Leader('a'), Panel::Agents),
];

const ATTACHED: &[(Chord, Panel)] = &[
    (Chord::Leader('u'), Panel::Updates),
    (Chord::Leader('a'), Panel::Agents),
    (Chord::Leader('k'), Panel::ProcessList),
    (Chord::Leader('<'), Panel::PromptTag),
    (Chord::Leader('#'), Panel::WorkspaceTags),
    (Chord::Leader('R'), Panel::Recap),
];

impl Surface {
    /// The surface under the open panel, if it is one panels open from.
    fn current(app: &App) -> Option<Self> {
        match &app.view {
            View::Dashboard => Some(Self::Dashboard),
            View::Attached(state) => state
                .focused_target()
                .map(|t| Self::Attached(t.workspace_id)),
            View::AttachedRemote => None,
        }
    }

    fn bindings(self) -> &'static [(Chord, Panel)] {
        match self {
            Self::Dashboard => DASHBOARD,
            Self::Attached(_) => ATTACHED,
        }
    }

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
    /// Whether `modal` is this panel, in any of its states.
    fn is_open(self, modal: &Modal) -> bool {
        matches!(
            (self, modal),
            (Self::WorkspaceActions, Modal::WorkspaceActions)
                | (Self::HideRules, Modal::HideRules { .. })
                | (Self::RepoSettings, Modal::RepoSettings { .. })
                | (Self::GlobalSettings, Modal::GlobalSettings { .. })
                | (Self::WorkspaceTags, Modal::WorkspaceTags { .. })
                | (Self::NameColor, Modal::NameColorPicker { .. })
                | (Self::ProcessList, Modal::ProcessList { .. })
                | (Self::RemoteHosts, Modal::RemoteHostPicker { .. })
                | (Self::Agents, Modal::AgentsPanel { .. })
                | (Self::Updates, Modal::UpdatesPanel { .. })
                | (Self::PromptTag, Modal::PromptTag(_))
                | (Self::Recap, Modal::Recap { .. })
        )
    }

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
            Self::GlobalSettings => {
                app.modal = Some(Modal::GlobalSettings { selected: 0 });
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
            Self::Recap => {
                if let Some(workspace_id) = on.workspace(app) {
                    app.modal = Some(Modal::Recap { workspace_id });
                }
            }
        }
        Ok(())
    }
}

/// Whether the open `modal` keeps the plain key `c` for itself rather than
/// letting it summon another panel: everything printable while it takes
/// text, and the few summon keys a panel binds as its own verb.
fn claims(modal: &Modal, c: char) -> bool {
    match modal {
        Modal::NewWorkspace { .. }
        | Modal::RenameWorkspace { .. }
        | Modal::RemoteNewWorkspace(_)
        | Modal::PromptTag(_)
        | Modal::WorkspaceTags { .. }
        | Modal::NameColorPicker { .. }
        | Modal::ProcessList { input: Some(_), .. }
        | Modal::UpdatesPanel {
            filter: Some(_), ..
        } => true,
        // `K` sends SIGKILL to the highlighted process.
        Modal::ProcessList { .. } => c == 'K',
        // `s` opens the selected workspace in a horizontal split.
        Modal::UpdatesPanel { .. } => c == 's',
        // `s` switches to a different agent.
        Modal::AgentMissing { .. } => c == 's',
        _ => false,
    }
}

/// Cards for work in flight. They keep every key, so a summon key cannot
/// pull the progress out from under the work that reports to it.
fn in_flight(modal: &Modal) -> bool {
    matches!(
        modal,
        Modal::RemoteListLoading { .. }
            | Modal::RemoteShareRunning { .. }
            | Modal::RemoteReposLoading { .. }
            | Modal::RemoteCreateRunning(_)
    )
}

/// Apply the summon rules to a key pressed while a panel is open. Returns
/// `true` when the key was consumed; otherwise it belongs to the open panel.
pub(in crate::app::input) fn switch(app: &mut App, k: KeyEvent) -> Result<bool> {
    let Some(open) = app.modal.as_ref() else {
        return Ok(false);
    };
    let Some(on) = Surface::current(app) else {
        return Ok(false);
    };
    if in_flight(open) {
        return Ok(false);
    }
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let alt = k.modifiers.contains(KeyModifiers::ALT);
    // A leader armed over a panel waits for its chord's second key; anything
    // that is not a chord just disarms it, as on the surfaces.
    let chord = if std::mem::take(&mut app.leader_pending) {
        match k.code {
            KeyCode::Char(c) => Chord::Leader(c),
            _ => return Ok(true),
        }
    } else {
        match k.code {
            KeyCode::Char('x') if ctrl => {
                app.leader_pending = true;
                return Ok(true);
            }
            KeyCode::Char(c) if !ctrl && !alt && !claims(open, c) => Chord::Key(c),
            _ => return Ok(false),
        }
    };
    let Some(&(_, panel)) = on.bindings().iter().find(|(ch, _)| *ch == chord) else {
        return Ok(matches!(chord, Chord::Leader(_)));
    };
    if panel.is_open(open) {
        app.modal = None;
        return Ok(true);
    }
    let prev = app.modal.take();
    let opened = panel.open(app, on);
    if app.modal.is_none() {
        app.modal = prev;
    }
    opened.map(|()| true)
}
