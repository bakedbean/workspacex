//! Modals for choosing, adding, and reporting on a workspace's agents.

use crate::app::{App, AttachReady, SharedApp, attach_workspace, ensure_instance_session};
use crate::error::Result;
use crate::ui::modal::Modal;
use crossterm::event::KeyCode;
// Test-only imports: the moved test modules access `draw_for_test`,
// `AttachedState`, `Arc`, and `Mutex` through `super::*` glob imports
// that cascade from the surrounding `tests` module.

pub(super) async fn agent_missing(
    app: &mut App,
    _shared: &SharedApp,
    k: crossterm::event::KeyEvent,
    ws_id: crate::data::store::WorkspaceId,
    agent: crate::pty::session::AgentKind,
) -> Result<()> {
    match k.code {
        KeyCode::Esc | KeyCode::Enter => {
            app.modal = None;
        }
        KeyCode::Char('s') => {
            let selected = crate::pty::session::AgentKind::ALL
                .iter()
                .position(|k| *k == agent)
                .unwrap_or(0);
            app.modal = Some(Modal::AgentPicker {
                ws_id,
                selected,
                current: agent,
            });
        }
        _ => {}
    };
    Ok(())
}

pub(super) async fn agent_picker(
    app: &mut App,
    _shared: &SharedApp,
    k: crossterm::event::KeyEvent,
    ws_id: crate::data::store::WorkspaceId,
    selected: usize,
    current: crate::pty::session::AgentKind,
) -> Result<()> {
    use crate::pty::session::AgentKind;
    match k.code {
        KeyCode::Esc => {
            app.modal = None;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let new_sel = selected.saturating_sub(1);
            app.modal = Some(Modal::AgentPicker {
                ws_id,
                selected: new_sel,
                current,
            });
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let new_sel = (selected + 1).min(AgentKind::ALL.len() - 1);
            app.modal = Some(Modal::AgentPicker {
                ws_id,
                selected: new_sel,
                current,
            });
        }
        KeyCode::Enter => {
            let new_agent = AgentKind::ALL[selected];
            app.store.set_workspace_agent(ws_id, new_agent)?;
            // Mirror to in-memory copy so the dashboard doesn't show stale
            // agent until poll_external_changes catches up.
            if let Some((_, ws)) = app.workspaces.iter_mut().find(|(_, w)| w.id == ws_id) {
                ws.agent = new_agent;
            }
            app.modal = None;
            attach_workspace(app, ws_id)?;
        }
        _ => {}
    }

    Ok(())
}

pub(super) async fn agents_panel(
    app: &mut App,
    _shared: &SharedApp,
    k: crossterm::event::KeyEvent,
    workspace_id: crate::data::store::WorkspaceId,
    selected: usize, // index into AgentKind::ALL for the add-picker,
) -> Result<()> {
    use crate::pty::session::AgentKind;
    match k.code {
        KeyCode::Esc => {
            app.modal = None;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.modal = Some(Modal::AgentsPanel {
                workspace_id,
                selected: selected.saturating_sub(1),
            });
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.modal = Some(Modal::AgentsPanel {
                workspace_id,
                selected: (selected + 1).min(AgentKind::ALL.len() - 1),
            });
        }
        KeyCode::Enter => {
            // Defensively bound the index: navigation clamps `selected`,
            // but guard against a stale/large value so this can never panic.
            let idx = selected.min(AgentKind::ALL.len().saturating_sub(1));
            let kind = AgentKind::ALL[idx];
            let inst = app.store.add_workspace_agent(workspace_id, kind)?;
            // Spawn it now. ensure_instance_session sets Modal::AgentMissing
            // (and returns AgentMissing) if the binary is absent — in that
            // case leave that modal up; otherwise close the panel. Refused
            // (a live archive) is vanishingly unlikely here — the panel
            // requires an attached workspace — but is handled the same way.
            match ensure_instance_session(app, inst.id, true)? {
                AttachReady::AgentMissing | AttachReady::Refused => {}
                AttachReady::Ok => app.modal = None,
            }
            // Refill `agent_roster` so it reflects the new instance —
            // nothing else on this path goes through `refresh()`.
            app.refresh()?;
        }
        KeyCode::Char('a') => {
            for kind in AgentKind::ALL {
                let inst = app.store.add_workspace_agent(workspace_id, kind)?;
                let _ = ensure_instance_session(app, inst.id, true)?;
            }
            app.modal = None;
            // Refill `agent_roster` so it reflects the four new
            // instances — nothing else on this path goes through
            // `refresh()`.
            app.refresh()?;
        }
        KeyCode::Char('x') => {
            // Remove the most-recently-added non-primary instance.
            if let Some(last) = app
                .store
                .workspace_agents(workspace_id)?
                .into_iter()
                .rfind(|i| !i.is_primary)
            {
                app.sessions.remove(last.id);
                app.store.remove_workspace_agent(last.id)?;
                // Refill `agent_roster` so it reflects the removal —
                // nothing else on this path goes through `refresh()`.
                app.refresh()?;
            }
        }
        _ => {}
    }

    Ok(())
}
