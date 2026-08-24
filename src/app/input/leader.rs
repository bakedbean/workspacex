//! The `Ctrl-x` leader chord: the second keystroke after the prefix.

use super::*;
use crate::app::{App, save_layout_for, schedule_detach_refresh};
use crate::error::Result;
use crate::ui::View;
use crate::ui::modal::Modal;
use crate::ui::split::{Arrow, CloseOutcome};
use crossterm::event::KeyCode;

// Test-only imports: the moved test modules access `draw_for_test`,
// `AttachedState`, `Arc`, and `Mutex` through `super::*` glob imports
// that cascade from the surrounding `tests` module.

/// Fire a single attached-view leader action for `k` (already-armed leader).
/// Extracted so both the letter-accelerator path and the overlay's Enter path
/// dispatch through identical code. Caller clears `leader_pending` first.
pub(in crate::app::input) async fn dispatch_leader_action(
    app: &mut App,
    target: crate::ui::split::AttachTarget,
    k: crossterm::event::KeyEvent,
) -> Result<()> {
    let id = target.workspace_id;
    let session = match app.sessions.get(target.instance) {
        Some(s) => s,
        None => {
            app.view = View::Dashboard;
            return Ok(());
        }
    };
    match k.code {
        KeyCode::Char('d') => {
            // In multi-pane mode, close just the focused pane; the
            // other panes' sessions keep running. Detach to dashboard
            // only when the last pane closes.
            if let View::Attached(state) = &mut app.view {
                if state.leaf_count() > 1 {
                    let closed = state.focused_target();
                    match state.close_focused() {
                        CloseOutcome::Focus(_) => {
                            // Refresh the closed pane's workspace. If another pane
                            // shares the same workspace this may redundantly refresh
                            // a still-attached workspace — harmless (at most one
                            // extra poll).
                            if let Some(cid) = closed {
                                schedule_detach_refresh(app, [cid.workspace_id]);
                            }
                            return Ok(());
                        }
                        CloseOutcome::Empty => {
                            if let Some(cid) = closed {
                                schedule_detach_refresh(app, [cid.workspace_id]);
                            }
                            app.view = View::Dashboard;
                            return Ok(());
                        }
                    }
                }
            }
            let leaves: Vec<_> = match &app.view {
                View::Attached(state) => state.leaves().iter().map(|t| t.workspace_id).collect(),
                _ => Vec::new(),
            };
            schedule_detach_refresh(app, leaves);
            app.view = View::Dashboard;
            Ok(())
        }
        KeyCode::Char('D') => {
            // Shift-D: persist the current (intact) pane layout under its anchor
            // before detaching, so re-attaching restores the same arrangement.
            // Unlike plain `d` — which tears panes down without touching the
            // saved layout — this is the explicit "remember this layout" detach.
            if let View::Attached(state) = &app.view {
                save_layout_for(app, state.clone());
            }
            let leaves: Vec<_> = match &app.view {
                View::Attached(state) => state.leaves().iter().map(|t| t.workspace_id).collect(),
                _ => Vec::new(),
            };
            schedule_detach_refresh(app, leaves);
            app.view = View::Dashboard;
            Ok(())
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => {
            let arrow = match k.code {
                KeyCode::Left => Arrow::Left,
                KeyCode::Right => Arrow::Right,
                KeyCode::Up => Arrow::Up,
                KeyCode::Down => Arrow::Down,
                _ => unreachable!(),
            };
            if let View::Attached(state) = &mut app.view {
                state.focus_direction(arrow);
            }
            Ok(())
        }
        KeyCode::Char('x') => {
            // Send a literal Ctrl-x (0x18) to claude.
            session.scroll_to_live();
            let _ = session
                .writer
                .send(crate::pty::session::WriteReq::Bytes(vec![0x18]))
                .await;
            Ok(())
        }
        KeyCode::Char('u') => {
            app.modal = Some(crate::ui::modal::Modal::UpdatesPanel {
                selected: 0,
                sort: crate::ui::modal::UpdatesSort::default(),
                filter: None,
            });
            Ok(())
        }
        KeyCode::Char('a') => {
            app.modal = Some(crate::ui::modal::Modal::AgentsPanel {
                workspace_id: id,
                selected: 0,
            });
            Ok(())
        }
        KeyCode::Char('e') => {
            if let Some(path) = app.workspace_path(id) {
                let cmd = app.store.get_setting("editor_cmd").ok().flatten();
                let r = crate::commands::external::open_in_editor(&path, cmd.as_deref());
                report_external_open(app, r);
            }
            Ok(())
        }
        KeyCode::Char('t') => {
            if let Some(path) = app.workspace_path(id) {
                let cmd = app.store.get_setting("terminal_cmd").ok().flatten();
                let r = crate::commands::external::open_in_terminal(&path, cmd.as_deref());
                report_external_open(app, r);
            }
            Ok(())
        }
        KeyCode::Char('v') => {
            if let Some(path) = app.workspace_path(id) {
                let cmd = app.store.get_setting("diff_cmd").ok().flatten();
                let base = crate::git::resolve_base_branch(&path).await;
                let r = crate::commands::external::open_diff(&path, &base, cmd.as_deref());
                report_external_open(app, r);
            }
            Ok(())
        }
        KeyCode::Char('g') => {
            if let Some(path) = app.workspace_path(id) {
                let cmd = app.store.get_setting("lazygit_cmd").ok().flatten();
                let r = crate::commands::external::open_in_lazygit(&path, cmd.as_deref());
                report_external_open(app, r);
            }
            Ok(())
        }
        KeyCode::Char('c') => {
            if let Some(path) = app.workspace_path(id) {
                let cmd = app.store.get_setting("chronox_cmd").ok().flatten();
                let r = crate::commands::external::open_in_chronox(&path, cmd.as_deref());
                report_external_open(app, r);
            }
            Ok(())
        }
        KeyCode::Char('k') => {
            app.modal = Some(Modal::ProcessList {
                workspace_id: id,
                selected: 0,
                input: None,
                notice: None,
            });
            Ok(())
        }
        KeyCode::Char(c @ '1'..='9') => {
            let idx = (c as u8 - b'1') as usize;
            if let Some(cmd) = app.pinned_commands_cache.get(idx) {
                let mut bytes = cmd.command.as_bytes().to_vec();
                bytes.push(b'\r');
                session.scroll_to_live();
                let _ = session
                    .writer
                    .send(crate::pty::session::WriteReq::Bytes(bytes))
                    .await;
            }
            Ok(())
        }
        // Fallback: any other leftover letter may be an agent switch key
        // from the footer agents row. Matched against the same
        // `agent_switch_keys` pool the renderer used, so the displayed key
        // equals the bound key. Placed last so it never shadows the
        // specific arms above (the pool excludes all of them).
        KeyCode::Char(c) => {
            let agents = app.store.workspace_agents(id).unwrap_or_default();
            if agents.len() > 1 {
                let keys = crate::ui::attached::agent_switch_keys(agents.len());
                if let Some(idx) = keys.iter().position(|k| *k == c) {
                    app.switch_focused_pane_to(agents[idx].id)?;
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Resolve the worktree for `workspace_id`, build a per-launch log path under the
/// wsx log dir, and spawn `command` there as a background process. Returns a
/// one-line notice (success with the log path, or an error) for the modal.
pub(in crate::app::input) fn launch_workspace_command(
    app: &App,
    workspace_id: crate::data::store::WorkspaceId,
    command: &str,
) -> String {
    let Some(worktree) = app
        .workspaces
        .iter()
        .find(|(_, w)| w.id == workspace_id)
        .map(|(_, w)| w.worktree_path.clone())
    else {
        return "error: workspace not found".to_string();
    };
    let now_ms = crate::util::time::now_ms_u64();
    let log_dir = crate::config::Dirs::discover().log_dir();
    let log_path = crate::commands::external::background_log_path(&log_dir, workspace_id.0, now_ms);
    match crate::commands::external::spawn_background_command(&worktree, command, &log_path) {
        Ok(()) => format!("\u{25B6} started \u{2192} {}", log_path.display()),
        Err(e) => format!("error: {e}"),
    }
}
