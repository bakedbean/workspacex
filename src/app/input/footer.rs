//! The footer hint bar and the dashboard detail bar's inline reply input.

use super::*;
use crate::app::{App, SelectionTarget, ensure_workspace_session};
use crate::ui::View;
use crossterm::event::KeyModifiers;

// Test-only imports: the moved test modules access `draw_for_test`,
// `AttachedState`, `Arc`, and `Mutex` through `super::*` glob imports
// that cascade from the surrounding `tests` module.

/// Handle a key event while [`PaneFocus::DetailBarReply`] is active.
///
/// Returns `true` if the key was consumed (caller should early-return),
/// or `false` if the key should fall through to the main dashboard handler
/// (e.g. navigation keys that also move the selection).
pub(in crate::app::input) async fn handle_detail_bar_reply_key(
    app: &mut App,
    k: crossterm::event::KeyEvent,
) -> bool {
    use crossterm::event::{KeyCode, KeyModifiers};

    // If the leader is already armed (Ctrl-X from a previous tick), yield to
    // the dashboard dispatcher so the chord can complete (digit → fire chip).
    if app.leader_pending {
        return false;
    }

    // Arm the leader on Ctrl-X without inserting '^X' into the draft.
    // The next key will arrive here again; the check above then yields it to
    // the dashboard handler which has the chord-completion block.
    if k.code == LEADER_KEY && k.modifiers.contains(KeyModifiers::CONTROL) {
        app.leader_pending = true;
        return true;
    }

    match (k.code, k.modifiers) {
        (KeyCode::Tab, _) => {
            // Spec: Dashboard → DetailBarReply → ProjectManager (when visible)
            // → Dashboard. If PM is not visible, skip straight back to Dashboard.
            if app.pm_visible {
                app.focus = crate::ui::PaneFocus::ProjectManager;
            } else {
                app.focus = crate::ui::PaneFocus::Dashboard;
            }
            true
        }
        (KeyCode::Esc, _) => {
            app.focus = crate::ui::PaneFocus::Dashboard;
            app.dashboard.reply_draft.clear();
            true
        }
        (KeyCode::Enter, _) => {
            let draft = std::mem::take(&mut app.dashboard.reply_draft);
            if let Some(SelectionTarget::Workspace(ws_id)) = app.selected_target() {
                // Auto-spawn the workspace's session if it isn't running
                // yet — otherwise an inline reply on an unattached
                // workspace silently drops.
                let _ = ensure_workspace_session(app, ws_id);
                if let Some(session) = app
                    .primary_instance(ws_id)
                    .and_then(|i| app.sessions.get(i))
                {
                    let mut bytes = draft.into_bytes();
                    bytes.push(b'\r');
                    session.scroll_to_live();
                    let _ = session
                        .writer
                        .send(crate::pty::session::WriteReq::Bytes(bytes))
                        .await;
                }
            }
            app.focus = crate::ui::PaneFocus::Dashboard;
            true
        }
        (KeyCode::Backspace, _) => {
            // The user is editing the draft directly; cancel any
            // pending chip-echo auto-clear so it doesn't wipe their edit.
            app.dashboard.reply_draft_clear_at_ms = None;
            app.dashboard.reply_draft.pop();
            true
        }
        (KeyCode::Char(c), m) if m == KeyModifiers::NONE || m == KeyModifiers::SHIFT => {
            // The user is editing the draft directly; cancel any
            // pending chip-echo auto-clear so it doesn't wipe their edit.
            app.dashboard.reply_draft_clear_at_ms = None;
            app.dashboard.reply_draft.push(c);
            true
        }
        (KeyCode::Up, _)
        | (KeyCode::Down, _)
        | (KeyCode::Left, _)
        | (KeyCode::Right, _)
        | (KeyCode::PageUp, _)
        | (KeyCode::PageDown, _)
        | (KeyCode::Home, _)
        | (KeyCode::End, _) => {
            // Yield to dashboard: it will handle the navigation. Discard draft.
            app.focus = crate::ui::PaneFocus::Dashboard;
            app.dashboard.reply_draft.clear();
            false
        }
        _ => true, // unknown key — swallow rather than fall through
    }
}

/// Route a single synthetic key press through the focused view's key handler,
/// exactly as if it had arrived from the keyboard. Used by footer-hint clicks
/// so they go through the same code paths (leader arming, chord consumption,
/// PTY forwarding) as real keystrokes rather than mutating state directly.
pub(in crate::app::input) async fn route_footer_key(app: &mut App, k: crossterm::event::KeyEvent) {
    match &app.view {
        View::Dashboard => {
            if let Err(e) = handle_key_dashboard(app, k).await {
                tracing::warn!(error = %e, "footer-hint dashboard dispatch failed");
            }
        }
        View::Attached(state) => {
            if let Some(target) = state.focused_target() {
                if let Err(e) = handle_key_attached(app, target, k).await {
                    tracing::warn!(error = %e, "footer-hint attached dispatch failed");
                }
            }
        }
        View::AttachedRemote => {
            if let Err(e) = handle_key_attached_remote(app, k).await {
                tracing::warn!(error = %e, "footer-hint remote dispatch failed");
            }
        }
    }
}

/// Fire a footer nav-hint click by synthesizing the key press(es) the hint
/// stands for and routing them through the normal key handlers — never by
/// poking `leader_pending` directly, so behavior matches the keyboard exactly
/// (including edge cases like an already-armed leader).
///
/// The dashboard footer lists direct keys. The attached footer lists
/// leader-prefixed chords, so a labeled key becomes `Ctrl-x` then the key, and
/// the `^x` pill becomes a lone `Ctrl-x` (which arms the leader, or — if it was
/// already armed — clears it and sends a literal `^X`, exactly as pressing
/// `Ctrl-x` twice does).
pub(in crate::app::input) async fn dispatch_footer_hint(
    app: &mut App,
    action: crate::ui::footer::FooterHintAction,
) {
    use crate::ui::footer::FooterHintAction;
    let leader = crossterm::event::KeyEvent::new(LEADER_KEY, KeyModifiers::CONTROL);
    match action {
        FooterHintAction::ArmLeader => route_footer_key(app, leader).await,
        FooterHintAction::Key(k) => {
            // Attached hints are chords: send the leader first, then the key.
            // The dashboard footer's keys are not leader-prefixed.
            if matches!(app.view, View::Attached(_)) {
                route_footer_key(app, leader).await;
            }
            route_footer_key(app, k).await;
        }
    }
}
