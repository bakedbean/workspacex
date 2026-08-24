//! Tests for `crate::app::input`.
//!
//! Split to mirror the production modules: a test for `input::mouse`
//! lives in `tests::mouse`. `common` holds the shared fixtures.
//!
//! Glob-imports the parent so each submodule's `use super::*;` cascades
//! to input's items (App, the handlers, crossterm types).

use super::*;
use crate::app::{SelectionTarget, attach_workspace};
use crate::ui::split::SplitDirection;
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

mod agents;
mod attached_wheel_forwarding;
mod common;
mod confirm_quit;
mod ctrl_x_shift_d;
mod ctrl_z_suppression;
mod dashboard;
mod detail_bar_focus;
mod detail_scroll;
mod keys;
mod leader;
mod leader_view_transition;
mod mouse;
mod new_workspace_notice;
mod pm_pane;
mod process_command;
mod remote;
mod rename_modal;
mod repo_pr_link_click;
mod restore_layout;
mod sort_mode;
mod spawn;
mod updates_panel;
mod workspace_lifecycle;
