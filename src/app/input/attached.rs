//! Key handling while a PTY pane has focus -- local panes and the
//! ssh-attached remote view. Most keys forward straight to the child.

use super::*;
use crate::app::{App, SelectionTarget, ensure_workspace_session};
use crate::error::Result;
use crate::ui::View;
use crossterm::event::{KeyCode, KeyModifiers};

// Test-only imports: the moved test modules access `draw_for_test`,
// `AttachedState`, `Arc`, and `Mutex` through `super::*` glob imports
// that cascade from the surrounding `tests` module.

/// Returns the session that should receive scroll input given the current
/// view + focus, or None when there is no targetable session.
pub(in crate::app::input) fn active_session(
    app: &App,
) -> Option<std::sync::Arc<crate::pty::session::Session>> {
    match &app.view {
        View::Attached(state) => state
            .focused_target()
            .and_then(|target| app.sessions.get(target.instance)),
        View::AttachedRemote => app.remote.clone(),
        // The dashboard's PM pane is now the digest — there is no PTY to
        // scroll or forward paste into, so PM-focused dashboard input
        // falls through to the default (no target).
        View::Dashboard => None,
    }
}

/// Resolve the session that should receive a pinned-command dispatch.
/// In the attached view this is the focused pane; on the dashboard it
/// is the currently selected workspace.
pub(in crate::app::input) fn chip_target_session(
    app: &App,
) -> Option<std::sync::Arc<crate::pty::session::Session>> {
    match &app.view {
        View::Attached(state) => state
            .focused_target()
            .and_then(|target| app.sessions.get(target.instance)),
        View::Dashboard => match app.selected_target() {
            Some(SelectionTarget::Workspace(id)) => {
                app.primary_instance(id).and_then(|i| app.sessions.get(i))
            }
            _ => None,
        },
        // The remote attach shows the host's global pinned commands; firing one
        // writes into the ssh PTY, driving the remote agent (see `render.rs`
        // AttachedRemote). Mirrors `active_session`'s remote arm.
        View::AttachedRemote => app.remote.clone(),
    }
}

/// Dispatch the pinned command at `idx` to the chip-target session.
/// No-op when:
///   - `idx` exceeds the number of *visible* chip rects (the row may
///     have truncated some chips at narrow widths),
///   - the cache has no command at `idx` (defensive),
///   - no chip target can be resolved.
///
/// When dispatched from `View::Dashboard`, also clears any in-flight
/// reply draft and returns focus to the dashboard. In other views
/// (attached, attached-PM) the dispatch is byte-only so it matches the
/// attached-view keyboard chord and doesn't trample dashboard state the
/// user can't see.
pub(in crate::app::input) async fn fire_chip(app: &mut App, idx: usize) {
    if idx >= app.chip_rects.len() {
        return;
    }
    let cmd = match app.pinned_commands_cache.get(idx) {
        Some(c) => c.clone(),
        None => return,
    };
    // On the dashboard the selected workspace may not have a live
    // session yet (the user hasn't attached). Auto-spawn one in place
    // so the chip command isn't silently dropped. In the attached
    // view the session already exists by definition.
    if matches!(app.view, View::Dashboard) {
        if let Some(SelectionTarget::Workspace(ws_id)) = app.selected_target() {
            let _ = ensure_workspace_session(app, ws_id);
        }
    }
    let session = match chip_target_session(app) {
        Some(s) => s,
        None => return,
    };
    let command_text = cmd.command.clone();
    let mut bytes = cmd.command.into_bytes();
    bytes.push(b'\r');
    session.scroll_to_live();
    let _ = session
        .writer
        .send(crate::pty::session::WriteReq::Bytes(bytes))
        .await;
    if matches!(app.view, View::Dashboard) {
        // Echo the dispatched command into the reply input so the user
        // sees what was sent. The tick handler clears it after the
        // deadline elapses (or earlier if the user interacts with the
        // input directly).
        app.dashboard.reply_draft = command_text;
        let now_ms = crate::util::time::now_ms_u64();
        app.dashboard.reply_draft_clear_at_ms = Some(now_ms + 600);
        app.focus = crate::ui::PaneFocus::Dashboard;
    }
}

pub(in crate::app::input) async fn handle_key_attached(
    app: &mut App,
    target: crate::ui::split::AttachTarget,
    k: crossterm::event::KeyEvent,
) -> Result<()> {
    // The focused leaf carries the agent instance directly; the workspace id
    // drives workspace-level actions (open editor/diff/process list, etc.).
    let id = target.workspace_id;
    let session = match app.sessions.get(target.instance) {
        Some(s) => s,
        None => {
            app.leader_pending = false;
            app.view = View::Dashboard;
            return Ok(());
        }
    };
    // Leader armed: ↑↓ move the overlay highlight (leader stays armed); Enter
    // fires the highlighted action; Esc just dismisses the overlay; any other
    // key is a direct accelerator that fires immediately and clears the leader.
    if app.leader_pending {
        let multi_pane = matches!(&app.view, View::Attached(s) if s.leaf_count() > 1);
        let items = crate::ui::attached::nav_menu_items(multi_pane);
        match k.code {
            KeyCode::Up => {
                let n = items.len();
                app.leader_selected = (app.leader_selected + n - 1) % n;
                return Ok(());
            }
            KeyCode::Down => {
                let n = items.len();
                app.leader_selected = (app.leader_selected + 1) % n;
                return Ok(());
            }
            KeyCode::Esc => {
                // Esc dismisses the nav overlay only; it must not detach to the
                // dashboard (that's the "d" accelerator's job).
                app.leader_pending = false;
                return Ok(());
            }
            KeyCode::Enter => {
                app.leader_pending = false;
                if let Some(key) = crate::ui::attached::nav_item_key(&items, app.leader_selected) {
                    return dispatch_leader_action(app, target, key).await;
                }
                return Ok(());
            }
            _ => {
                app.leader_pending = false;
                return dispatch_leader_action(app, target, k).await;
            }
        }
    }
    if k.code == LEADER_KEY && k.modifiers.contains(KeyModifiers::CONTROL) {
        app.leader_pending = true;
        app.leader_selected = 0;
        return Ok(());
    }
    let bytes = encode_key(k);
    if !bytes.is_empty() {
        session.scroll_to_live();
        let _ = session
            .writer
            .send(crate::pty::session::WriteReq::Bytes(bytes))
            .await;
    }
    // Auto-rename capture (local mode only): buffer printable chars; on Enter,
    // attempt rename if the workspace name is still a generated slug. In the
    // default `claude` mode the rename happens via system-prompt + branch
    // poller, so the PTY-interception path stays inert.
    let mode = std::env::var("WSX_RENAME_MODE").unwrap_or_else(|_| "claude".to_string());
    if mode == "local" {
        match k.code {
            KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                session.capture_char(c)
            }
            KeyCode::Backspace => session.capture_backspace(),
            KeyCode::Enter => {
                if let Some(prompt) = session.take_first_prompt() {
                    if let Some(slug) = crate::data::workspace::slugify_prompt(&prompt) {
                        let ws_info = app
                            .workspaces
                            .iter()
                            .find(|(_, w)| w.id == id)
                            .map(|(_, w)| w.clone());
                        if let Some(ws) = ws_info {
                            if crate::util::names::is_generated_slug(&ws.name) {
                                let repo = app.repos.iter().find(|r| r.id == ws.repo_id).cloned();
                                if let Some(repo) = repo {
                                    // Fire-and-forget: rename failure shouldn't disrupt the keystroke.
                                    let _ = crate::data::workspace::rename(
                                        &app.store, &repo, &ws, &slug,
                                    )
                                    .await;
                                    app.refresh()?;
                                }
                            }
                        }
                    }
                }
            }
            _ => {} // arrows, function keys, etc. — not part of the prompt
        }
    }
    Ok(())
}

/// Full-screen remote-attach key handler. No leader menu:
/// `Ctrl-x d` detaches (severing only the local ssh
/// client), everything else is forwarded verbatim to the remote agent's PTY.
/// If the ssh child has already exited (e.g. tmux printed `can't find session`
/// for a stale name, then the client quit), any key bounces to the dashboard
/// with an error modal instead of writing into a dead PTY.
pub(in crate::app::input) async fn handle_key_attached_remote(
    app: &mut App,
    k: crossterm::event::KeyEvent,
) -> Result<()> {
    let session = match app.remote.clone() {
        Some(s) => s,
        None => {
            app.leader_pending = false;
            app.view = View::Dashboard;
            return Ok(());
        }
    };
    // Dead ssh client (stale session name, remote tmux gone, network drop):
    // surface it and return to the dashboard on the next keypress.
    if matches!(
        *session.status.read().unwrap(),
        crate::pty::session::SessionStatus::Exited { .. }
    ) {
        let label = app
            .remote_target
            .as_ref()
            .map(|t| format!("{}/{}", t.host_name, t.tmux))
            .unwrap_or_default();
        app.leader_pending = false;
        crate::app::detach_remote(app);
        app.modal = Some(crate::ui::modal::Modal::Error {
            message: format!("remote session ended: {label}"),
        });
        return Ok(());
    }
    if app.leader_pending {
        app.leader_pending = false;
        if k.code == KeyCode::Char('d') {
            crate::app::detach_remote(app);
            return Ok(());
        }
        // `^x <digit>` fires the matching global pinned command into the remote
        // session, mirroring the local attached chord (`handle_leader_key`).
        if let KeyCode::Char(c @ '1'..='9') = k.code {
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
            return Ok(());
        }
        // Any other key after the leader is a no-op (no remote leader menu).
        return Ok(());
    }
    if k.code == LEADER_KEY && k.modifiers.contains(KeyModifiers::CONTROL) {
        app.leader_pending = true;
        app.leader_selected = 0;
        return Ok(());
    }
    let bytes = encode_key(k);
    if !bytes.is_empty() {
        session.scroll_to_live();
        let _ = session
            .writer
            .send(crate::pty::session::WriteReq::Bytes(bytes))
            .await;
    }
    Ok(())
}
