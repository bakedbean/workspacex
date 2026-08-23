//! Input handling: every keystroke, paste, and mouse event the TUI sees.
//!
//! `handle_event` is the single entry point. It routes by what currently has
//! focus -- dashboard, an attached PTY pane, or a modal -- to one handler per
//! surface, each in its own file:
//!
//!   [`dashboard`]  the workspace list
//!   [`attached`]   a focused PTY pane (local or ssh-remote)
//!   [`modal`]      whatever modal is up
//!   [`leader`]     the `Ctrl-x` prefix chord
//!   [`footer`]     hint bar and the detail bar's reply input
//!   [`mouse`]      clicks and wheel, including hit-testing
//!   [`keys`]       key <-> byte encoding and input tracing

pub mod attached;
pub mod dashboard;
pub mod footer;
pub mod keys;
pub mod leader;
pub mod modal;
pub mod mouse;

pub(in crate::app::input) use attached::*;
pub(in crate::app::input) use dashboard::*;
pub(in crate::app::input) use footer::*;
pub(in crate::app::input) use keys::*;
pub(in crate::app::input) use leader::*;
pub(in crate::app::input) use modal::*;
pub(in crate::app::input) use mouse::*;

use crate::app::{App, SharedApp};
use crate::error::Result;
use crate::ui::View;
use crate::ui::modal::Modal;
use crossterm::event::{Event as CtEvent, KeyCode, KeyEventKind};

// Test-only imports: the moved test modules access `draw_for_test`,
// `AttachedState`, `Arc`, and `Mutex` through `super::*` glob imports
// that cascade from the surrounding `tests` module.
#[cfg(test)]
use crate::app::build_spawn_info;
#[cfg(test)]
use crate::app::draw_for_test;
#[cfg(test)]
use crate::ui::split::AttachedState;
#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
use tokio::sync::Mutex;

/// Leader key for attached-view actions (detach, open updates panel, send
/// literal leader to claude). Chosen to be free in raw mode and to avoid
/// collision with tmux's default `Ctrl-b` prefix (or any non-default
/// `Ctrl-a` setup).
pub(in crate::app::input) const LEADER_KEY: KeyCode = KeyCode::Char('x');

/// Surface a failed external-tool launch as an error modal. The `e`/`t`/`v`/
/// `g`/`c` handlers in both the dashboard and attached views share this so a
/// launch failure always reports the same way.
pub(in crate::app::input) fn report_external_open<E: std::fmt::Display>(
    app: &mut App,
    result: std::result::Result<(), E>,
) {
    if let Err(e) = result {
        app.modal = Some(Modal::Error {
            message: e.to_string(),
        });
    }
}

/// Clear the PR/diff poll throttles so the background loop refetches on its
/// next tick — the digest's manual refresh.
pub(in crate::app::input) fn nudge_status_refresh(app: &mut App) {
    app.pr_last_poll_ms.clear();
    app.diff_last_poll_ms.clear();
}

pub(in crate::app::input) async fn handle_paste(
    app: &mut App,
    shared: &SharedApp,
    content: String,
) -> Result<()> {
    // PTY path: forward the whole paste as one bracketed sequence to
    // whichever session is currently driving the foreground (attached
    // workspace, full-screen PM, or the embedded PM pane when focused
    // on the dashboard). When a modal owns the input (e.g. New Workspace
    // name field), skip this branch so the per-char fallback can feed
    // the modal handler.
    let session = if app.modal.is_none() {
        active_session(app)
    } else {
        None
    };
    if let Some(session) = session {
        let bytes = wrap_paste_bytes(&content);
        if input_trace_enabled() {
            tracing::info!(
                target: "wsx::input_trace",
                bytes = bytes.len(),
                "paste -> bracketed PTY write"
            );
        }
        session.scroll_to_live();
        let _ = session
            .writer
            .send(crate::pty::session::WriteReq::Bytes(bytes))
            .await;
        return Ok(());
    }
    if input_trace_enabled() {
        tracing::info!(
            target: "wsx::input_trace",
            chars = content.chars().count(),
            modal_open = app.modal.is_some(),
            view = ?std::mem::discriminant(&app.view),
            "paste -> per-char fallback (newlines become Enter)"
        );
    }
    // Non-attached fallback: forward each char as if typed, translating
    // control chars to the KeyCodes crossterm would have emitted live so
    // modal handlers see paste-with-newlines as multiple Enter presses
    // rather than literal '\n' Chars.
    for c in content.chars() {
        dispatch_key(app, shared, paste_char_to_key(c)).await?;
    }
    Ok(())
}

pub(in crate::app::input) async fn dispatch_key(
    app: &mut App,
    shared: &SharedApp,
    k: crossterm::event::KeyEvent,
) -> Result<()> {
    if app.modal.is_some() {
        handle_key_modal(app, shared, k).await?;
    } else {
        match &app.view {
            View::Dashboard => handle_key_dashboard(app, k).await?,
            View::Attached(state) => {
                let id = match state.focused_target() {
                    Some(id) => id,
                    None => {
                        app.leader_pending = false;
                        app.view = View::Dashboard;
                        return Ok(());
                    }
                };
                handle_key_attached(app, id, k).await?
            }
            View::AttachedRemote => handle_key_attached_remote(app, k).await?,
        }
    }
    Ok(())
}

pub(crate) async fn handle_event(app: &mut App, shared: &SharedApp, evt: CtEvent) -> Result<()> {
    trace_event(&evt);
    match evt {
        CtEvent::Key(k) if k.kind == KeyEventKind::Press => dispatch_key(app, shared, k).await?,
        CtEvent::Mouse(m) => handle_mouse(app, m).await,
        CtEvent::Paste(content) => handle_paste(app, shared, content).await?,
        CtEvent::Resize(cols, rows) => {
            // Record the new terminal size; the run loop's tick applies it to
            // backgrounded sessions once the resize settles (debounced), so a
            // window drag doesn't trigger a repaint storm. See
            // `crate::app::resize_sync`.
            app.resize_debounce
                .note(cols, rows, crate::util::time::now_ms_u64());
        }
        _ => {}
    }
    Ok(())
}

/// Test helper: resolve (seeding if necessary) the primary agent instance id
/// for a workspace, so tests can spawn/look up sessions keyed by instance the
/// same way production paths do.
///
/// Unlike production's `resolve_primary_instance`, this reads directly from the
/// store rather than the `app.workspaces` in-memory mirror, because many tests
/// insert workspace rows straight into the store without refreshing the mirror.
/// The seeded agent kind is irrelevant — sessions are keyed only by the id.
#[cfg(test)]
pub(crate) fn test_primary_instance(
    app: &App,
    ws: crate::data::store::WorkspaceId,
) -> crate::data::store::AgentInstanceId {
    if let Some(i) = app.store.primary_instance_id(ws).unwrap() {
        return i;
    }
    app.store
        .add_primary_agent(ws, crate::pty::session::AgentKind::Claude, 0)
        .unwrap()
        .id
}

/// Test helper: the attach target for a workspace's primary agent instance.
/// Mirrors production single-pane behavior, where a single-agent workspace's
/// leaf carries `(workspace_id, primary_instance_id)`.
#[cfg(test)]
pub(crate) fn test_target(
    app: &App,
    ws: crate::data::store::WorkspaceId,
) -> crate::ui::split::AttachTarget {
    crate::ui::split::AttachTarget {
        workspace_id: ws,
        instance: test_primary_instance(app, ws),
    }
}

#[cfg(test)]
mod tests;
