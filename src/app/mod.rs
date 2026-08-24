//! The TUI application.
//!
//! [`App`] holds all mutable state; [`run`] is the event loop that drives it.
//! Everything else is grouped by what it does to that state:
//!
//!   [`state`]      App itself, construction, refresh, live-session queries
//!   [`selection`]  what the dashboard cursor points at
//!   [`status`]     classifying sessions into what the dashboard shows
//!   [`session`]    starting, attaching to, and tearing down PTY sessions
//!   [`spawn`]      deciding what to launch before a PTY exists
//!   [`reconcile`]  folding background create/archive results back in
//!   [`remote`]     workspaces shared from another host over ssh
//!   [`input`]      keys, mouse, paste
//!   [`render`]     drawing a frame
//!
//! `state`, `selection`, and `status` each contribute methods to `App`; an
//! inherent impl may be split across files within a crate.

#![allow(clippy::collapsible_if)]

use crate::data::store::{Repo, Store, Workspace, WorkspaceId};
use crate::error::Result;
use crate::pty::session::SessionManager;
use crate::ui::View;
use crate::ui::dashboard::DashboardState;
use crate::ui::modal::Modal;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

pub mod activity;
pub mod background;
pub mod bell;
pub mod input;
#[cfg(unix)]
pub mod ipc;
pub mod messaging;
pub mod render;
pub mod resize_sync;
pub use crate::app::activity::{ActivityState, classify_activity, classify_activity_with_events};
pub use crate::app::background::{
    branch_drift_poll, branch_drift_poll_with, tail_workspace_events,
};
pub use crate::app::bell::{BellPattern, COLD_START_WINDOW, alert_decision, fire_bell};
pub use crate::app::render::draw_for_test;

pub(crate) mod reconcile;
pub(crate) mod remote;
pub(crate) mod repo_setting;
pub(crate) mod run;
pub(crate) mod selection;
pub(crate) mod session;
pub(crate) mod spawn;
pub(crate) mod state;
pub(crate) mod status;
pub(crate) mod types;

pub use remote::{RemoteList, RemoteTarget};
pub use repo_setting::RepoSettingField;
pub use run::run;
pub use state::App;
pub use types::{AppEvent, AttachReady, PendingEdit, SelectionTarget, StoppedKind};

pub(crate) use reconcile::{reconcile_archive_result, reconcile_create_result};
pub(crate) use remote::{attach_remote, detach_remote, reconcile_remote_list, remote_rows};
pub(crate) use repo_setting::apply_repo_setting;
pub(crate) use run::rescan_processes;
pub(crate) use selection::reset_detail_scroll_on_workspace_change;
pub(crate) use session::{
    attach_workspace, ensure_instance_session, ensure_workspace_session, restore_attached_state,
    save_layout_for, schedule_detach_refresh, toggle_workspace_shared,
};
pub(crate) use spawn::{
    build_added_spawn_info, build_spawn_info, resolve_primary_instance, tmux_name_for,
};
pub(crate) use status::derive_stopped_kind;
pub(crate) use types::MAX_ACTIVITY_HOURS;

pub type SharedApp = Arc<Mutex<App>>;
