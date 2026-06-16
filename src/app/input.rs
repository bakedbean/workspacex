//! Input dispatch: key, mouse, and paste handlers extracted from src/app.rs.
//!
//! `handle_event` is the public entry point called from the run loop.
//! Per-view handlers (`handle_key_dashboard`, `handle_key_attached`,
//! `handle_key_attached_pm`) route keystrokes to the right place; the modal
//! handler (`handle_key_modal`) takes precedence when a modal is open.

use crate::app::{
    App, AttachReady, PendingEdit, RepoSettingField, SelectionTarget, SharedApp,
    apply_repo_setting, attach_workspace, ensure_instance_session, ensure_workspace_session,
    reconcile_create_result, rescan_processes, restore_attached_state, save_layout_for,
    schedule_detach_refresh,
};
use crate::error::Result;
use crate::ui::View;
use crate::ui::modal::Modal;
use crate::ui::split::{Arrow, CloseOutcome, SplitDirection};
use crossterm::event::{
    Event as CtEvent, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

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
const LEADER_KEY: KeyCode = KeyCode::Char('x');

/// Surface a failed external-tool launch as an error modal. The `e`/`t`/`v`/
/// `g`/`c` handlers in both the dashboard and attached views share this so a
/// launch failure always reports the same way.
fn report_external_open<E: std::fmt::Display>(app: &mut App, result: std::result::Result<(), E>) {
    if let Err(e) = result {
        app.modal = Some(Modal::Error {
            message: e.to_string(),
        });
    }
}

fn encode_key_for_pty(k: &crossterm::event::KeyEvent) -> Option<Vec<u8>> {
    use crossterm::event::{KeyCode, KeyModifiers};
    match (k.code, k.modifiers) {
        (KeyCode::Char(c), m) if m == KeyModifiers::NONE || m == KeyModifiers::SHIFT => {
            Some(c.to_string().into_bytes())
        }
        (KeyCode::Char(c), m) if m.contains(KeyModifiers::CONTROL) => {
            let upper = c.to_ascii_uppercase();
            if ('@'..='_').contains(&upper) {
                Some(vec![(upper as u8) - b'@'])
            } else {
                None
            }
        }
        (KeyCode::Enter, _) => Some(b"\r".to_vec()),
        (KeyCode::Backspace, _) => Some(vec![0x7f]),
        (KeyCode::Up, _) => Some(b"\x1b[A".to_vec()),
        (KeyCode::Down, _) => Some(b"\x1b[B".to_vec()),
        (KeyCode::Right, _) => Some(b"\x1b[C".to_vec()),
        (KeyCode::Left, _) => Some(b"\x1b[D".to_vec()),
        (KeyCode::Tab, _) => Some(b"\t".to_vec()),
        _ => None,
    }
}
fn encode_key(k: crossterm::event::KeyEvent) -> Vec<u8> {
    use KeyCode::*;
    match k.code {
        Char(c) => {
            if k.modifiers.contains(KeyModifiers::CONTROL) && c.is_ascii_alphabetic() {
                vec![(c.to_ascii_lowercase() as u8) - b'a' + 1]
            } else {
                let mut buf = [0u8; 4];
                c.encode_utf8(&mut buf).as_bytes().to_vec()
            }
        }
        Enter => b"\r".to_vec(),
        Backspace => b"\x7f".to_vec(),
        Tab => b"\t".to_vec(),
        Esc => b"\x1b".to_vec(),
        Left => b"\x1b[D".to_vec(),
        Right => b"\x1b[C".to_vec(),
        Up => b"\x1b[A".to_vec(),
        Down => b"\x1b[B".to_vec(),
        _ => vec![],
    }
}
/// Translate a pasted character into the `KeyEvent` crossterm would have
/// emitted if it were typed live. Matters for the non-attached fallback:
/// `\n`/`\r` are Enter (modal submit), `\t` is Tab (focus / autocomplete),
/// printable chars pass through as `Char(c)`.
fn paste_char_to_key(c: char) -> crossterm::event::KeyEvent {
    use crossterm::event::{KeyEvent, KeyModifiers};
    let code = match c {
        '\n' | '\r' => KeyCode::Enter,
        '\t' => KeyCode::Tab,
        _ => KeyCode::Char(c),
    };
    KeyEvent::new(code, KeyModifiers::NONE)
}
/// Wrap a paste payload with the bracketed-paste escape markers claude
/// reads to render `[Pasted N lines]` instead of treating the content as
/// typed input. The output is what gets written to the PTY in one send.
pub(crate) fn wrap_paste_bytes(content: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(content.len() + 12);
    out.extend_from_slice(b"\x1b[200~");
    out.extend_from_slice(content.as_bytes());
    out.extend_from_slice(b"\x1b[201~");
    out
}
/// Apply a scroll delta to whichever session is currently in focus.
/// `up=true` scrolls toward older content (higher offset).
fn scroll_active(app: &App, rows: usize, up: bool) {
    let Some(session) = active_session(app) else {
        return;
    };
    if up {
        session.scroll_up(rows);
    } else {
        session.scroll_down(rows);
    }
}
/// Returns the session that should receive scroll input given the current
/// view + focus, or None when there is no targetable session.
fn active_session(app: &App) -> Option<std::sync::Arc<crate::pty::session::Session>> {
    match &app.view {
        View::Attached(state) => state
            .focused_target()
            .and_then(|target| app.sessions.get(target.instance)),
        View::AttachedPm => app.pm.clone(),
        View::Dashboard
            if app.pm_visible && matches!(app.focus, crate::ui::PaneFocus::ProjectManager) =>
        {
            app.pm.clone()
        }
        _ => None,
    }
}
/// Resolve the session that should receive a pinned-command dispatch.
/// In the attached view this is the focused pane; on the dashboard it
/// is the currently selected workspace.
fn chip_target_session(app: &App) -> Option<std::sync::Arc<crate::pty::session::Session>> {
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
        _ => None,
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
async fn fire_chip(app: &mut App, idx: usize) {
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
    let _ = session.writer.send(bytes).await;
    if matches!(app.view, View::Dashboard) {
        // Echo the dispatched command into the reply input so the user
        // sees what was sent. The tick handler clears it after the
        // deadline elapses (or earlier if the user interacts with the
        // input directly).
        app.dashboard.reply_draft = command_text;
        let now_ms = crate::time::now_ms_u64();
        app.dashboard.reply_draft_clear_at_ms = Some(now_ms + 600);
        app.focus = crate::ui::PaneFocus::Dashboard;
    }
}
/// Aggregate the current `StatusCounts` for one repo by classifying each
/// of its live workspaces. Used by the `z` (fold) keybinding so we can
/// look up the same default-fold state the renderer would compute.
fn current_repo_counts(
    app: &App,
    rid: crate::data::store::RepoId,
) -> crate::ui::dashboard::sort::StatusCounts {
    let iter = app
        .workspaces
        .iter()
        .filter(|(r, _)| *r == rid)
        .map(|(_, w)| app.classify_status(w));
    crate::ui::dashboard::sort::StatusCounts::from_iter(iter)
}
/// Toggle the fold state of the currently focused repo on the
/// dashboard. If a workspace is focused, the repo containing it is
/// the target. Extracted from the original single-key `z` arm so the
/// `zz` chord branch can reuse it.
fn toggle_focused_fold(app: &mut App) {
    let target_rid = match app.selected_target() {
        Some(SelectionTarget::Workspace(wid)) => app
            .workspaces
            .iter()
            .find(|(_, w)| w.id == wid)
            .map(|(rid, _)| *rid),
        Some(SelectionTarget::Repo(rid)) => Some(rid),
        None => None,
    };
    if let Some(rid) = target_rid {
        let id = rid.0 as u64;
        let counts = current_repo_counts(app, rid);
        let new_folded = match app.dashboard.folded.get(&id).copied() {
            Some(explicit) => !explicit,
            None => !crate::ui::dashboard::sort::default_fold(counts),
        };
        app.dashboard.folded.insert(id, new_folded);
    }
}
/// Move the currently selected repo one slot up (`up = true`) or down on the
/// dashboard, persisting the new order. No-op unless a repo *header* is
/// selected, and no-op at the ends of the list. Keeps the selection anchored
/// to the moved repo so repeated presses walk it into place.
fn move_selected_repo(app: &mut App, up: bool) -> Result<()> {
    let Some(SelectionTarget::Repo(rid)) = app.selected_target() else {
        return Ok(());
    };
    let Some(pos) = app.repos.iter().position(|r| r.id == rid) else {
        return Ok(());
    };
    let neighbor = if up {
        pos.checked_sub(1)
    } else if pos + 1 < app.repos.len() {
        Some(pos + 1)
    } else {
        None
    };
    let Some(nb) = neighbor else { return Ok(()) };
    let nb_id = app.repos[nb].id;

    app.store.swap_repo_sort_order(rid, nb_id)?;
    app.refresh()?;

    // Anchor the cursor to the repo we just moved.
    if let Some(idx) = app
        .selectable
        .iter()
        .position(|t| *t == SelectionTarget::Repo(rid))
    {
        app.select_index(idx);
    }
    Ok(())
}

/// Open the selected workspace's PR in the browser. No-op if the workspace
/// id no longer resolves (e.g. removed between draw and click).
fn open_pr_for_workspace(app: &App, ws_id: crate::data::store::WorkspaceId) {
    if let Some((_, ws)) = app.workspaces.iter().find(|(_, w)| w.id == ws_id) {
        crate::git::forge::open_pr_in_browser(&ws.worktree_path, &ws.branch);
    }
}

/// Vim-style `h` (fold) / `l` (unfold) on the focused row. Unlike
/// [`toggle_focused_fold`], this is idempotent: pressing `h` on an
/// already-folded repo leaves it folded.
fn set_focused_fold(app: &mut App, fold: bool) {
    let target_rid = match app.selected_target() {
        Some(SelectionTarget::Workspace(wid)) => app
            .workspaces
            .iter()
            .find(|(_, w)| w.id == wid)
            .map(|(rid, _)| *rid),
        Some(SelectionTarget::Repo(rid)) => Some(rid),
        None => None,
    };
    if let Some(rid) = target_rid {
        app.dashboard.folded.insert(rid.0 as u64, fold);
    }
}
/// `za` action: expand every registered repo by inserting an explicit
/// `false` in `dashboard.folded`. Overrides the renderer's
/// default-fold heuristic so even default-folded repos open.
fn expand_all_repos(app: &mut App) {
    for r in &app.repos {
        app.dashboard.folded.insert(r.id.0 as u64, false);
    }
}
/// `zM` action: fold every registered repo by inserting an explicit
/// `true` in `dashboard.folded`. Overrides the renderer's heuristic.
fn fold_all_repos(app: &mut App) {
    for r in &app.repos {
        app.dashboard.folded.insert(r.id.0 as u64, true);
    }
}
async fn handle_key_dashboard(app: &mut App, k: crossterm::event::KeyEvent) -> Result<()> {
    // PM pane focus handling. When PM is focused, all keystrokes forward
    // to its PTY — including 'p' and 'r' (typing words containing those
    // letters must not toggle the pane or trigger refresh). To use the
    // dashboard's 'p' / 'r' shortcuts, the user presses Tab/Esc first to
    // return focus to the dashboard.
    if app.pm_visible && matches!(app.focus, crate::ui::PaneFocus::ProjectManager) {
        // Defensive: PM focus means the dashboard's z-leader cannot be
        // meaningfully consumed here (keys forward to the PM PTY). Clear
        // it so it doesn't leak across focus transitions.
        app.z_leader_pending = false;
        match (k.code, k.modifiers) {
            (KeyCode::Tab, _) | (KeyCode::Esc, _) => {
                app.focus = crate::ui::PaneFocus::Dashboard;
                return Ok(());
            }
            (KeyCode::Char('o'), m) if m.contains(KeyModifiers::CONTROL) => {
                // Ctrl-O: expand PM to a full-screen attached view so the
                // user can scroll through claude's history naturally.
                if app.pm.is_some() {
                    app.leader_pending = false;
                    app.view = View::AttachedPm;
                }
                return Ok(());
            }
            _ => {
                if let Some(session) = app.pm.as_ref() {
                    if let Some(bytes) = encode_key_for_pty(&k) {
                        session.scroll_to_live();
                        let _ = session.writer.send(bytes).await;
                    }
                }
                return Ok(());
            }
        }
    }
    // DetailBarReply focus: keystrokes go to the reply input.
    if matches!(app.focus, crate::ui::PaneFocus::DetailBarReply) {
        // If the selected target is no longer a workspace (e.g.
        // refresh moved selection), auto-return focus and discard.
        if !matches!(app.selected_target(), Some(SelectionTarget::Workspace(_))) {
            app.focus = crate::ui::PaneFocus::Dashboard;
            app.dashboard.reply_draft.clear();
            return Ok(());
        }
        let consumed = handle_detail_bar_reply_key(app, k).await;
        if consumed {
            return Ok(());
        }
        // Not consumed → fall through so the dashboard handler picks up
        // the key (e.g. arrow nav). `handle_detail_bar_reply_key` has
        // already cleared the draft and reset focus when bailing out.
    }
    // Tab when focus is on Dashboard: workspace selection → DetailBarReply;
    // repo selection with PM visible → ProjectManager.
    if matches!(app.focus, crate::ui::PaneFocus::Dashboard) && k.code == KeyCode::Tab {
        // Treat Tab as a "never mind" for any armed z-leader so it
        // doesn't unexpectedly eat the next dashboard key after the
        // user Tabs back from PM.
        app.z_leader_pending = false;
        let cfg = crate::app::render::resolve_dashboard_detail_cfg(app);
        let is_workspace = matches!(app.selected_target(), Some(SelectionTarget::Workspace(_)));
        if is_workspace && cfg.visible {
            app.focus = crate::ui::PaneFocus::DetailBarReply;
        } else if app.pm_visible {
            app.focus = crate::ui::PaneFocus::ProjectManager;
        }
        return Ok(());
    }
    // Filter input mode: while a filter buffer is active, intercept
    // printable chars, Backspace, and Esc so they edit the buffer
    // rather than triggering single-key shortcuts like 'n' / 'q' / '/'.
    // Navigation keys (arrows, Enter, etc.) still flow through.
    if app.dashboard.filter.is_some() {
        match k.code {
            KeyCode::Esc => {
                app.dashboard.filter = None;
                return Ok(());
            }
            KeyCode::Backspace => {
                if let Some(buf) = app.dashboard.filter.as_mut() {
                    buf.pop();
                }
                return Ok(());
            }
            KeyCode::Char(c)
                if !c.is_control()
                    && !k.modifiers.contains(KeyModifiers::CONTROL)
                    && !k.modifiers.contains(KeyModifiers::ALT) =>
            {
                if let Some(buf) = app.dashboard.filter.as_mut() {
                    buf.push(c);
                }
                return Ok(());
            }
            _ => {}
        }
    }
    // Z-leader chord. When armed by the prior `z` keypress, the next
    // key dispatches and the leader clears unconditionally. Unknown
    // follow-ups are eaten (no fall-through to the main key handler)
    // so accidental `zj` etc. don't move the selection silently.
    if app.z_leader_pending {
        app.z_leader_pending = false;
        match (k.code, k.modifiers) {
            (KeyCode::Char('z'), _) => toggle_focused_fold(app),
            // Vim `zr` / `zR` (reduce fold / open all folds).
            (KeyCode::Char('r'), _) | (KeyCode::Char('R'), _) | (KeyCode::Char('a'), _) => {
                expand_all_repos(app)
            }
            // Match bare `Char('M')` (no SHIFT guard) to match the
            // codebase convention for capital-letter binds like `G` —
            // some terminals + CapsLock report uppercase without SHIFT.
            // Also accept lowercase `m` (Vim `zm`) for muscle-memory compat.
            (KeyCode::Char('M'), _) | (KeyCode::Char('m'), _) => fold_all_repos(app),
            _ => {} // Esc, unknown key, anything else: just clear.
        }
        return Ok(());
    }
    // Ctrl-X leader for pinned-command chord (mirrors the attached
    // view's binding). The next 1..9 fires the matching chip; any
    // other follow-up — including a second Ctrl-X — just clears the
    // leader. Completion is checked BEFORE re-arming so a double
    // Ctrl-X cancels the chord instead of getting stuck armed.
    if app.leader_pending {
        app.leader_pending = false;
        match k.code {
            KeyCode::Char('a') => {
                if let Some(crate::app::SelectionTarget::Workspace(ws_id)) = app.selected_target() {
                    app.modal = Some(crate::ui::modal::Modal::AgentsPanel {
                        workspace_id: ws_id,
                        selected: 0,
                    });
                }
            }
            KeyCode::Char(c @ '1'..='9') => {
                let idx = (c as u8 - b'1') as usize;
                fire_chip(app, idx).await;
            }
            _ => {}
        }
        return Ok(());
    }
    if k.code == LEADER_KEY && k.modifiers.contains(KeyModifiers::CONTROL) {
        app.leader_pending = true;
        return Ok(());
    }
    match (k.code, k.modifiers) {
        (KeyCode::Char('q'), _) => app.quit = true,
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
            let max = app.selectable.len().saturating_sub(1);
            let idx = if app.dashboard.selected == 0 {
                max
            } else {
                app.dashboard.selected - 1
            };
            app.select_index(idx);
            // Clear any in-flight reply draft so it can't leak to the newly
            // selected workspace (draft is tied to the workspace at the time
            // keystrokes arrived, not to wherever the cursor ends up).
            app.dashboard.reply_draft.clear();
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
            let max = app.selectable.len().saturating_sub(1);
            let idx = if app.dashboard.selected >= max {
                0
            } else {
                app.dashboard.selected + 1
            };
            app.select_index(idx);
            // Clear any in-flight reply draft (same rationale as Up/k above).
            app.dashboard.reply_draft.clear();
        }
        (KeyCode::Char('h'), _) => set_focused_fold(app, true),
        (KeyCode::Char('l'), _) => match app.selected_target() {
            Some(SelectionTarget::Workspace(id)) => attach_workspace(app, id)?,
            Some(SelectionTarget::Repo(_)) => set_focused_fold(app, false),
            None => {}
        },
        (KeyCode::Enter, _) | (KeyCode::Char('i'), _) => match app.selected_target() {
            Some(SelectionTarget::Workspace(id)) => attach_workspace(app, id)?,
            Some(SelectionTarget::Repo(id)) => {
                app.modal = Some(Modal::NewWorkspace {
                    repo_id: id,
                    name_buffer: String::new(),
                    yolo: false,
                    agent: crate::pty::session::AgentKind::from_store(&app.store),
                });
            }
            None => {}
        },
        (KeyCode::Char('n'), _) | (KeyCode::Char('N'), _) => {
            // Resolve target repo from the current selection. Falls back to the
            // first repo if nothing is selected (shouldn't normally happen).
            // Capital N opens the modal in YOLO mode (claude launches with
            // --dangerously-skip-permissions on every attach).
            let yolo = matches!(k.code, KeyCode::Char('N'));
            let repo_id = match app.selected_target() {
                Some(SelectionTarget::Repo(id)) => Some(id),
                Some(SelectionTarget::Workspace(wid)) => app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == wid)
                    .map(|(rid, _)| *rid),
                None => app.repos.first().map(|r| r.id),
            };
            if let Some(id) = repo_id {
                app.modal = Some(Modal::NewWorkspace {
                    repo_id: id,
                    name_buffer: String::new(),
                    yolo,
                    agent: crate::pty::session::AgentKind::from_store(&app.store),
                });
            }
        }
        (KeyCode::Char('e'), _) => {
            if let Some(SelectionTarget::Workspace(id)) = app.selected_target()
                && let Some(path) = app.workspace_path(id)
            {
                let cmd = app.store.get_setting("editor_cmd").ok().flatten();
                let r = crate::commands::external::open_in_editor(&path, cmd.as_deref());
                report_external_open(app, r);
            }
        }
        (KeyCode::Char('t'), _) => {
            if let Some(SelectionTarget::Workspace(id)) = app.selected_target()
                && let Some(path) = app.workspace_path(id)
            {
                let cmd = app.store.get_setting("terminal_cmd").ok().flatten();
                let r = crate::commands::external::open_in_terminal(&path, cmd.as_deref());
                report_external_open(app, r);
            }
        }
        (KeyCode::Char('v'), _) => {
            if let Some(SelectionTarget::Workspace(id)) = app.selected_target()
                && let Some(path) = app.workspace_path(id)
            {
                let cmd = app.store.get_setting("diff_cmd").ok().flatten();
                let base = crate::git::resolve_base_branch(&path).await;
                let r = crate::commands::external::open_diff(&path, &base, cmd.as_deref());
                report_external_open(app, r);
            }
            // 'v' on a Repo header is intentionally a no-op.
        }
        (KeyCode::Char('g'), _) => {
            if let Some(SelectionTarget::Workspace(id)) = app.selected_target()
                && let Some(path) = app.workspace_path(id)
            {
                let cmd = app.store.get_setting("lazygit_cmd").ok().flatten();
                let r = crate::commands::external::open_in_lazygit(&path, cmd.as_deref());
                report_external_open(app, r);
            }
            // 'g' on a Repo header is intentionally a no-op.
        }
        (KeyCode::Char('c'), _) => {
            if let Some(SelectionTarget::Workspace(id)) = app.selected_target()
                && let Some(path) = app.workspace_path(id)
            {
                let cmd = app.store.get_setting("chronox_cmd").ok().flatten();
                let r = crate::commands::external::open_in_chronox(&path, cmd.as_deref());
                report_external_open(app, r);
            }
            // 'c' on a Repo header is intentionally a no-op.
        }
        (KeyCode::Char('K'), _) => match app.selected_target() {
            Some(SelectionTarget::Workspace(id)) => {
                app.modal = Some(Modal::ProcessList {
                    workspace_id: id,
                    selected: 0,
                    input: None,
                    notice: None,
                });
            }
            // Shift+K on a repo header moves it up one slot.
            Some(SelectionTarget::Repo(_)) => move_selected_repo(app, true)?,
            None => {}
        },
        (KeyCode::Char('J'), _) => {
            // Shift+J on a repo header moves it down one slot. On a workspace
            // it's a no-op (J is otherwise unbound on the dashboard).
            if let Some(SelectionTarget::Repo(_)) = app.selected_target() {
                move_selected_repo(app, false)?;
            }
        }
        (KeyCode::Char('s'), _) => {
            let repo_id = match app.selected_target() {
                Some(SelectionTarget::Repo(id)) => Some(id),
                Some(SelectionTarget::Workspace(wid)) => app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == wid)
                    .map(|(rid, _)| *rid),
                None => app.repos.first().map(|r| r.id),
            };
            if let Some(id) = repo_id {
                app.modal = Some(Modal::RepoSettings {
                    repo_id: id,
                    selected: 0,
                });
            }
        }
        (KeyCode::Char('d'), _) => {
            if let Some(SelectionTarget::Workspace(id)) = app.selected_target() {
                let name = app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == id)
                    .map(|(_, w)| w.name.clone());
                if let Some(name) = name {
                    app.modal = Some(Modal::ConfirmArchive {
                        workspace_id: id,
                        name,
                    });
                }
            }
            // 'd' on a Repo header is intentionally a no-op.
        }
        (KeyCode::Char('r'), _)
            if app.pm_visible && matches!(app.focus, crate::ui::PaneFocus::Dashboard) =>
        {
            // Manual refresh of the PM pane. Only fires from Dashboard focus
            // so 'r' typed inside PM (when PM is focused) goes to the PTY.
            let dirs = crate::config::Dirs::discover();
            let pm_dir = dirs.pm_dir();
            if let Err(e) =
                crate::agent::pm::refresh_pm(&mut app.sessions, &app.store, &pm_dir).await
            {
                app.modal = Some(Modal::Error {
                    message: e.to_string(),
                });
            }
        }
        (KeyCode::Char('G'), _) => {
            use crate::ui::dashboard::layout::GroupMode;
            app.dashboard.group_mode = match app.dashboard.group_mode {
                GroupMode::Repo => GroupMode::Attention,
                GroupMode::Attention => GroupMode::Repo,
            };
        }
        (KeyCode::Char('z'), _) => {
            app.z_leader_pending = true;
        }
        (KeyCode::Char('/'), _) => {
            app.dashboard.filter = Some(String::new());
        }
        (KeyCode::Char('p'), _) if crate::app::render::pm_enabled(&app.store) => {
            if app.pm_visible {
                // Hide pane; session stays alive.
                app.pm_visible = false;
                app.focus = crate::ui::PaneFocus::Dashboard;
            } else {
                // Open pane. Spawn if not yet spawned this run.
                let dirs = crate::config::Dirs::discover();
                let pm_dir = dirs.pm_dir();
                let custom = app
                    .store
                    .get_setting("pm_custom_instructions")
                    .ok()
                    .flatten();
                let result = if app.pm_auto_summary_sent {
                    // Reopen path: refresh so PM picks up workspace
                    // changes that happened while the pane was hidden.
                    crate::agent::pm::open_pm_with_refresh(
                        &mut app.sessions,
                        &app.store,
                        &pm_dir,
                        custom,
                    )
                    .await
                } else {
                    crate::agent::pm::open_pm_with_auto_summary(
                        &mut app.sessions,
                        &app.store,
                        &pm_dir,
                        custom,
                    )
                    .await
                };
                if let Err(e) = result {
                    app.modal = Some(Modal::Error {
                        message: e.to_string(),
                    });
                    return Ok(());
                }
                app.pm_auto_summary_sent = true;
                app.pm = app.sessions.pm();
                app.pm_visible = true;
                app.focus = crate::ui::PaneFocus::ProjectManager;
            }
        }
        _ => {}
    }
    Ok(())
}
/// Fire a single attached-view leader action for `k` (already-armed leader).
/// Extracted so both the letter-accelerator path and the overlay's Enter path
/// dispatch through identical code. Caller clears `leader_pending` first.
async fn dispatch_leader_action(
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
        KeyCode::Esc => {
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
            let _ = session.writer.send(vec![0x18]).await;
            Ok(())
        }
        KeyCode::Char('u') => {
            app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });
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
                let _ = session.writer.send(bytes).await;
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
async fn handle_key_attached(
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
    // fires the highlighted action; any other key is a direct accelerator that
    // fires immediately. Esc / second Ctrl-x fall through to the dispatch which
    // clears the leader.
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
        let _ = session.writer.send(bytes).await;
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
                            if crate::names::is_generated_slug(&ws.name) {
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
async fn handle_key_attached_pm(app: &mut App, k: crossterm::event::KeyEvent) -> Result<()> {
    let session = match app.pm.clone() {
        Some(s) => s,
        None => {
            app.leader_pending = false;
            app.view = View::Dashboard;
            return Ok(());
        }
    };
    if app.leader_pending {
        app.leader_pending = false;
        match k.code {
            KeyCode::Char('d') => {
                app.view = View::Dashboard;
                return Ok(());
            }
            KeyCode::Char('x') => {
                // Send a literal Ctrl-x (0x18) to claude.
                session.scroll_to_live();
                let _ = session.writer.send(vec![0x18]).await;
                return Ok(());
            }
            KeyCode::Char('u') => {
                app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });
                return Ok(());
            }
            _ => return Ok(()),
        }
    }
    if k.code == LEADER_KEY && k.modifiers.contains(KeyModifiers::CONTROL) {
        app.leader_pending = true;
        return Ok(());
    }
    let bytes = encode_key(k);
    if !bytes.is_empty() {
        session.scroll_to_live();
        let _ = session.writer.send(bytes).await;
    }
    Ok(())
}
/// Resolve the worktree for `workspace_id`, build a per-launch log path under the
/// wsx log dir, and spawn `command` there as a background process. Returns a
/// one-line notice (success with the log path, or an error) for the modal.
fn launch_workspace_command(
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
    let now_ms = crate::time::now_ms_u64();
    let log_dir = crate::config::Dirs::discover().log_dir();
    let log_path = crate::commands::external::background_log_path(&log_dir, workspace_id.0, now_ms);
    match crate::commands::external::spawn_background_command(&worktree, command, &log_path) {
        Ok(()) => format!("\u{25B6} started \u{2192} {}", log_path.display()),
        Err(e) => format!("error: {e}"),
    }
}
async fn handle_key_modal(
    app: &mut App,
    shared: &SharedApp,
    k: crossterm::event::KeyEvent,
) -> Result<()> {
    let modal = app.modal.clone().unwrap();
    match modal {
        Modal::NewWorkspace {
            repo_id,
            mut name_buffer,
            yolo,
            mut agent,
        } => match k.code {
            KeyCode::Esc => {
                app.modal = None;
            }
            KeyCode::Tab => {
                agent = match agent {
                    crate::pty::session::AgentKind::Claude => crate::pty::session::AgentKind::Pi,
                    crate::pty::session::AgentKind::Pi => crate::pty::session::AgentKind::Hermes,
                    crate::pty::session::AgentKind::Hermes => crate::pty::session::AgentKind::Codex,
                    crate::pty::session::AgentKind::Codex => crate::pty::session::AgentKind::Claude,
                };
                app.modal = Some(Modal::NewWorkspace {
                    repo_id,
                    name_buffer,
                    yolo,
                    agent,
                });
            }
            KeyCode::Enter => {
                let name = if name_buffer.trim().is_empty() {
                    None
                } else {
                    Some(name_buffer.clone())
                };
                let repo = app.repos.iter().find(|r| r.id == repo_id).unwrap().clone();
                let base = app.worktree_base.clone();
                let cancel = tokio_util::sync::CancellationToken::new();
                let create_gen = app.alloc_create_gen();
                app.modal = Some(Modal::SetupRunning {
                    cancel: cancel.clone(),
                });
                let shared_clone = shared.clone();
                tokio::spawn(async move {
                    let result = crate::data::workspace::create_with_app(
                        shared_clone.clone(),
                        repo,
                        name,
                        base,
                        yolo,
                        agent,
                        cancel,
                    )
                    .await;
                    reconcile_create_result(shared_clone, create_gen, result).await;
                });
            }
            KeyCode::Backspace => {
                name_buffer.pop();
                app.modal = Some(Modal::NewWorkspace {
                    repo_id,
                    name_buffer,
                    yolo,
                    agent,
                });
            }
            KeyCode::Char(c) => {
                name_buffer.push(c);
                app.modal = Some(Modal::NewWorkspace {
                    repo_id,
                    name_buffer,
                    yolo,
                    agent,
                });
            }
            _ => {}
        },
        Modal::ConfirmArchive {
            workspace_id,
            name: _,
        } => match k.code {
            KeyCode::Char('y') => {
                let (repo, ws) = {
                    let ws = app
                        .workspaces
                        .iter()
                        .find(|(_, w)| w.id == workspace_id)
                        .map(|(_, w)| w.clone());
                    let repo = ws
                        .as_ref()
                        .and_then(|w| app.repos.iter().find(|r| r.id == w.repo_id).cloned());
                    match (repo, ws) {
                        (Some(r), Some(w)) => (r, w),
                        _ => {
                            app.modal = None;
                            return Ok(());
                        }
                    }
                };
                let archive_gen = app.alloc_archive_gen();
                let script_present = repo
                    .archive_script
                    .as_deref()
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false);
                app.modal = Some(Modal::ArchiveRunning {
                    step: crate::ui::modal::ArchiveStep::Script,
                    script_present,
                });
                let shared_clone = shared.clone();
                tokio::spawn(async move {
                    let result = crate::data::workspace::archive_with_app(
                        shared_clone.clone(),
                        repo,
                        ws,
                        crate::data::workspace::ArchiveOpts {
                            force_branch_delete: true,
                            ..Default::default()
                        },
                    )
                    .await;
                    crate::app::reconcile_archive_result(shared_clone, archive_gen, result).await;
                });
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                app.modal = None;
            }
            _ => {}
        },
        Modal::SetupRunning { cancel } => {
            // Esc cancels in-flight create; every other key (including Enter)
            // is intentionally ignored during creation.
            if k.code == KeyCode::Esc {
                cancel.cancel();
                app.modal = None;
                app.pending_create_gen = None;
            }
        }
        Modal::ArchiveRunning { .. } => {
            // Archive is non-cancellable. Swallow all keys until the
            // spawned task completes and reconciles the modal.
        }
        Modal::Error { .. } => {
            if matches!(k.code, KeyCode::Esc | KeyCode::Enter) {
                app.modal = None;
            }
        }
        Modal::UpdatesPanel { selected } => {
            let selected_now = selected;
            // Build the same ordered workspace list the renderer uses, so
            // arrow keys and Enter operate on the same indices.
            let activity_translated: std::collections::HashMap<
                crate::data::store::WorkspaceId,
                crate::ui::updates_bar::ActivityState,
            > = app
                .workspace_activity
                .iter()
                .map(|(k, v)| (*k, crate::app::render::translate_activity(*v)))
                .collect();
            let order = crate::ui::modal::ordered_workspaces_for_panel(
                &app.repos,
                &app.workspaces,
                &app.workspace_events,
                &activity_translated,
                &app.workspace_needs_attention,
            );
            match k.code {
                KeyCode::Esc => {
                    app.modal = None;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    let new_sel = selected_now.saturating_sub(1);
                    app.modal = Some(Modal::UpdatesPanel { selected: new_sel });
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let max = order.len().saturating_sub(1);
                    let new_sel = (selected_now + 1).min(max);
                    app.modal = Some(Modal::UpdatesPanel { selected: new_sel });
                }
                KeyCode::Enter => {
                    if let Some(ws_id) = order.get(selected_now).copied() {
                        // Mirror the dashboard-attach flow: clear the
                        // alert, spawn (or resume) the PTY, switch view.
                        app.workspace_needs_attention.remove(&ws_id);
                        match ensure_workspace_session(app, ws_id)? {
                            AttachReady::Ok => {
                                if app
                                    .primary_instance(ws_id)
                                    .and_then(|i| app.sessions.get(i))
                                    .is_some()
                                {
                                    if let Some(restored) = restore_attached_state(app, ws_id) {
                                        app.leader_pending = false;
                                        app.view = View::Attached(restored);
                                    }
                                }
                            }
                            AttachReady::AgentMissing => {
                                // Modal::AgentMissing is set; leave view alone.
                            }
                        }
                    }
                    // Only close the Updates-panel if AgentMissing didn't
                    // replace the modal — otherwise we'd wipe the new modal.
                    if !matches!(app.modal, Some(Modal::AgentMissing { .. })) {
                        app.modal = None;
                    }
                }
                KeyCode::Char('v') | KeyCode::Char('s') => {
                    // Vim-style splits: 'v' = vertical (panes side-by-side),
                    // 's' = horizontal (stacked). Only valid when there's
                    // already an attached pane to split — otherwise behaves
                    // like Enter (just attach).
                    let dir = if matches!(k.code, KeyCode::Char('v')) {
                        SplitDirection::Vertical
                    } else {
                        SplitDirection::Horizontal
                    };
                    if let Some(ws_id) = order.get(selected_now).copied() {
                        app.workspace_needs_attention.remove(&ws_id);
                        match ensure_workspace_session(app, ws_id)? {
                            AttachReady::Ok => {
                                if let Some(instance) = app.primary_instance(ws_id)
                                    && app.sessions.get(instance).is_some()
                                {
                                    // Splitting from the dashboard targets the
                                    // workspace's primary instance — preserves
                                    // pre-multi-agent behavior (the leaf for a
                                    // single-agent workspace is its primary).
                                    let target = crate::ui::split::AttachTarget {
                                        workspace_id: ws_id,
                                        instance,
                                    };
                                    match &mut app.view {
                                        View::Attached(state) => {
                                            // Same pane already focused: switch focus
                                            // instead of splitting onto itself.
                                            if state.focused_target() == Some(target) {
                                                // no-op
                                            } else if state.leaves().contains(&target) {
                                                // Already open in another pane —
                                                // just refocus there.
                                                if let Some(p) =
                                                    state.tree.leaf_paths().into_iter().find(|p| {
                                                        state.tree.leaf_at(p) == Some(target)
                                                    })
                                                {
                                                    state.focus = p;
                                                }
                                            } else {
                                                state.split(dir, target);
                                            }
                                        }
                                        _ => {
                                            // No attached pane yet — restore saved layout or attach plainly.
                                            if let Some(restored) =
                                                restore_attached_state(app, ws_id)
                                            {
                                                app.leader_pending = false;
                                                app.view = View::Attached(restored);
                                            }
                                        }
                                    }
                                }
                            }
                            AttachReady::AgentMissing => {
                                // Modal::AgentMissing is set; leave view alone.
                            }
                        }
                    }
                    // Only close the Updates-panel if AgentMissing didn't
                    // replace the modal — otherwise we'd wipe the new modal.
                    if !matches!(app.modal, Some(Modal::AgentMissing { .. })) {
                        app.modal = None;
                    }
                }
                _ => {}
            }
        }
        Modal::ProcessList {
            workspace_id,
            mut selected,
            input,
            notice,
        } => {
            let procs = app
                .workspace_processes
                .get(&workspace_id)
                .cloned()
                .unwrap_or_default();

            // Input mode: capture keystrokes into the command buffer.
            if let Some(mut buffer) = input {
                match k.code {
                    KeyCode::Esc => {
                        app.modal = Some(Modal::ProcessList {
                            workspace_id,
                            selected,
                            input: None,
                            notice,
                        });
                    }
                    KeyCode::Enter => {
                        let command = buffer.trim().to_string();
                        if command.is_empty() {
                            // Empty command: stay in input mode, keep the buffer.
                            app.modal = Some(Modal::ProcessList {
                                workspace_id,
                                selected,
                                input: Some(buffer),
                                notice,
                            });
                        } else {
                            let new_notice = launch_workspace_command(app, workspace_id, &command);
                            app.modal = Some(Modal::ProcessList {
                                workspace_id,
                                selected,
                                input: None,
                                notice: Some(new_notice),
                            });
                            // Best-effort: the just-spawned process may not have
                            // surfaced in `lsof` yet, so it usually appears on the
                            // next periodic scan rather than this one. The notice
                            // confirms the launch in the meantime.
                            rescan_processes(app).await;
                        }
                    }
                    KeyCode::Backspace => {
                        buffer.pop();
                        app.modal = Some(Modal::ProcessList {
                            workspace_id,
                            selected,
                            input: Some(buffer),
                            notice,
                        });
                    }
                    KeyCode::Char(c) => {
                        buffer.push(c);
                        app.modal = Some(Modal::ProcessList {
                            workspace_id,
                            selected,
                            input: Some(buffer),
                            notice,
                        });
                    }
                    _ => {
                        app.modal = Some(Modal::ProcessList {
                            workspace_id,
                            selected,
                            input: Some(buffer),
                            notice,
                        });
                    }
                }
                return Ok(());
            }

            // List mode.
            // ProcessList intentionally does NOT alias j/k to nav like the other
            // list modals: `k` here means SIGTERM and `K` means SIGKILL, so
            // vim-style movement would clash with the kill verbs. Arrow keys are
            // the only navigation; `r` opens the run-command input.
            match k.code {
                KeyCode::Esc => {
                    app.modal = None;
                }
                KeyCode::Up => {
                    selected = selected.saturating_sub(1);
                    app.modal = Some(Modal::ProcessList {
                        workspace_id,
                        selected,
                        input: None,
                        notice,
                    });
                }
                KeyCode::Down => {
                    if !procs.is_empty() {
                        selected = (selected + 1).min(procs.len() - 1);
                    }
                    app.modal = Some(Modal::ProcessList {
                        workspace_id,
                        selected,
                        input: None,
                        notice,
                    });
                }
                KeyCode::Char('r') => {
                    // Clear any prior launch notice when starting a fresh
                    // command so a stale "started" line doesn't linger.
                    app.modal = Some(Modal::ProcessList {
                        workspace_id,
                        selected,
                        input: Some(String::new()),
                        notice: None,
                    });
                }
                KeyCode::Char('k') => {
                    if let Some(p) = procs.get(selected) {
                        let _ = crate::activity::proc::kill_pid(p.pid, "TERM").await;
                        rescan_processes(app).await;
                    }
                }
                KeyCode::Char('K') => {
                    if let Some(p) = procs.get(selected) {
                        let _ = crate::activity::proc::kill_pid(p.pid, "KILL").await;
                        rescan_processes(app).await;
                    }
                }
                _ => {}
            }
        }
        Modal::RepoSettings {
            repo_id,
            mut selected,
        } => match k.code {
            KeyCode::Esc => {
                app.modal = None;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                selected = selected.saturating_sub(1);
                app.modal = Some(Modal::RepoSettings { repo_id, selected });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = RepoSettingField::ALL.len() - 1;
                selected = (selected + 1).min(max);
                app.modal = Some(Modal::RepoSettings { repo_id, selected });
            }
            KeyCode::Enter => {
                let field = RepoSettingField::ALL
                    [selected.min(RepoSettingField::ALL.len().saturating_sub(1))];
                app.pending_edit = Some(PendingEdit { repo_id, field });
                app.modal = None;
            }
            KeyCode::Char('d') => {
                let field = RepoSettingField::ALL
                    [selected.min(RepoSettingField::ALL.len().saturating_sub(1))];
                let _ = apply_repo_setting(app, repo_id, field, "");
                let _ = app.refresh();
                app.modal = Some(Modal::RepoSettings { repo_id, selected });
            }
            _ => {}
        },
        Modal::AgentMissing { ws_id, agent, .. } => match k.code {
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
        },
        Modal::AgentPicker {
            ws_id,
            selected,
            current,
        } => {
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
        }
        Modal::AgentsPanel {
            workspace_id,
            selected,
        } => {
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
                    // case leave that modal up; otherwise close the panel.
                    match ensure_instance_session(app, inst.id, true)? {
                        AttachReady::AgentMissing => {} // ensure_instance_session set the modal
                        _ => app.modal = None,
                    }
                }
                KeyCode::Char('a') => {
                    for kind in AgentKind::ALL {
                        let inst = app.store.add_workspace_agent(workspace_id, kind)?;
                        let _ = ensure_instance_session(app, inst.id, true)?;
                    }
                    app.modal = None;
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
                    }
                }
                _ => {}
            }
        }
        Modal::UsageWindowPicker { selected } => match k.code {
            KeyCode::Up => {
                let n = if selected == 0 {
                    crate::config::usage_window::UsageWindow::ALL.len() - 1
                } else {
                    selected - 1
                };
                app.modal = Some(Modal::UsageWindowPicker { selected: n });
            }
            KeyCode::Down => {
                let n = if selected + 1 >= crate::config::usage_window::UsageWindow::ALL.len() {
                    0
                } else {
                    selected + 1
                };
                app.modal = Some(Modal::UsageWindowPicker { selected: n });
            }
            KeyCode::Enter => {
                let win = crate::config::usage_window::UsageWindow::from_index(selected);
                if let Err(e) = app
                    .store
                    .set_setting("usage_graph_window", win.as_setting())
                {
                    tracing::warn!(error = %e, "failed to persist usage_graph_window");
                }
                app.modal = None;
            }
            KeyCode::Esc => {
                app.modal = None;
            }
            _ => {}
        },
    }
    Ok(())
}
/// Handle a key event while [`PaneFocus::DetailBarReply`] is active.
///
/// Returns `true` if the key was consumed (caller should early-return),
/// or `false` if the key should fall through to the main dashboard handler
/// (e.g. navigation keys that also move the selection).
async fn handle_detail_bar_reply_key(app: &mut App, k: crossterm::event::KeyEvent) -> bool {
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
                    let _ = session.writer.send(bytes).await;
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
async fn handle_paste(app: &mut App, shared: &SharedApp, content: String) -> Result<()> {
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
        session.scroll_to_live();
        let _ = session.writer.send(wrap_paste_bytes(&content)).await;
        return Ok(());
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
/// Returns the slot index of the detail-bar container under (col, row),
/// or None if no container rect matches.
fn container_under_cursor(app: &App, col: u16, row: u16) -> Option<usize> {
    app.detail_container_rects
        .iter()
        .enumerate()
        .find_map(|(i, slot)| {
            let r = slot.as_ref()?;
            let in_rect = col >= r.x
                && col < r.x.saturating_add(r.width)
                && row >= r.y
                && row < r.y.saturating_add(r.height);
            if in_rect { Some(i) } else { None }
        })
}

/// Returns the `(session, rect)` of the attached-view pane under (col, row),
/// or None when the cursor is over chrome / no pane. Mirrors
/// `container_under_cursor`'s saturating bounds check.
fn pane_under_cursor(
    app: &App,
    col: u16,
    row: u16,
) -> Option<(
    std::sync::Arc<crate::pty::session::Session>,
    ratatui::layout::Rect,
)> {
    app.attached_pane_rects.iter().find_map(|(session, r)| {
        let in_rect = col >= r.x
            && col < r.x.saturating_add(r.width)
            && row >= r.y
            && row < r.y.saturating_add(r.height);
        if in_rect {
            Some((std::sync::Arc::clone(session), *r))
        } else {
            None
        }
    })
}

/// Bump the scroll offset for container `slot` by `delta` rows. Clamped
/// to [0, u16::MAX] here; the next draw clamps further to the actual
/// content height in `render_container`.
fn adjust_detail_scroll(app: &mut App, slot: usize, delta: u16, up: bool) {
    if slot >= app.detail_scroll_offsets.len() {
        return;
    }
    let cur = app.detail_scroll_offsets[slot];
    app.detail_scroll_offsets[slot] = if up {
        cur.saturating_sub(delta)
    } else {
        cur.saturating_add(delta)
    };
}

/// Route a single synthetic key press through the focused view's key handler,
/// exactly as if it had arrived from the keyboard. Used by footer-hint clicks
/// so they go through the same code paths (leader arming, chord consumption,
/// PTY forwarding) as real keystrokes rather than mutating state directly.
async fn route_footer_key(app: &mut App, k: crossterm::event::KeyEvent) {
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
        View::AttachedPm => {
            if let Err(e) = handle_key_attached_pm(app, k).await {
                tracing::warn!(error = %e, "footer-hint pm dispatch failed");
            }
        }
    }
}

/// Fire a footer nav-hint click by synthesizing the key press(es) the hint
/// stands for and routing them through the normal key handlers — never by
/// poking `leader_pending` directly, so behavior matches the keyboard exactly
/// (including edge cases like an already-armed leader).
///
/// The dashboard footer lists direct keys. The attached/PM footers list
/// leader-prefixed chords, so a labeled key becomes `Ctrl-x` then the key, and
/// the `^x` pill becomes a lone `Ctrl-x` (which arms the leader, or — if it was
/// already armed — clears it and sends a literal `^X`, exactly as pressing
/// `Ctrl-x` twice does).
async fn dispatch_footer_hint(app: &mut App, action: crate::ui::footer::FooterHintAction) {
    use crate::ui::footer::FooterHintAction;
    let leader = crossterm::event::KeyEvent::new(LEADER_KEY, KeyModifiers::CONTROL);
    match action {
        FooterHintAction::ArmLeader => route_footer_key(app, leader).await,
        FooterHintAction::Key(k) => {
            // Attached/PM hints are chords: send the leader first, then the key.
            // The dashboard footer's keys are not leader-prefixed.
            if matches!(app.view, View::Attached(_) | View::AttachedPm) {
                route_footer_key(app, leader).await;
            }
            route_footer_key(app, k).await;
        }
    }
}

async fn handle_mouse(app: &mut App, m: MouseEvent) {
    // Detail-bar container scroll: consume wheel events on the Dashboard
    // view when the cursor is over a container rect. Fall through for
    // wheel events elsewhere (existing scroll_active routing).
    if matches!(
        m.kind,
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
    ) && matches!(app.view, crate::ui::View::Dashboard)
    {
        if let Some(slot) = container_under_cursor(app, m.column, m.row) {
            let up = matches!(m.kind, MouseEventKind::ScrollUp);
            adjust_detail_scroll(app, slot, 3, up);
            return;
        }
    }

    // Attached view: a plain wheel over a pane whose agent has mouse
    // reporting on is forwarded to that agent's PTY so it scrolls its own
    // view (notably its full-screen UI, where wsx has no scrollback).
    // Shift+wheel, panes without mouse mode, and scrolls over chrome all
    // fall through to `scroll_active` (wsx scrollback) below.
    if matches!(
        m.kind,
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
    ) && matches!(
        app.view,
        crate::ui::View::Attached(_) | crate::ui::View::AttachedPm
    ) && !m.modifiers.contains(KeyModifiers::SHIFT)
    {
        if let Some((session, rect)) = pane_under_cursor(app, m.column, m.row) {
            let up = matches!(m.kind, MouseEventKind::ScrollUp);
            let rel_col = m.column.saturating_sub(rect.x).saturating_add(1);
            let rel_row = m.row.saturating_sub(rect.y).saturating_add(1);
            if let Some(bytes) = session.wheel_report_bytes(up, rel_col, rel_row) {
                let _ = session.writer.send(bytes).await;
                return;
            }
        }
    }

    match m.kind {
        MouseEventKind::ScrollUp => scroll_active(app, 3, true),
        MouseEventKind::ScrollDown => scroll_active(app, 3, false),
        MouseEventKind::Down(MouseButton::Left) => {
            // If the usage-window picker is open, a click either applies the
            // option under the cursor or dismisses the picker (click-outside).
            if matches!(app.modal, Some(Modal::UsageWindowPicker { .. })) {
                if let Some(idx) = app.usage_window_option_rects.iter().position(|r| {
                    m.column >= r.x
                        && m.column < r.x.saturating_add(r.width)
                        && m.row >= r.y
                        && m.row < r.y.saturating_add(r.height)
                }) {
                    let win = crate::config::usage_window::UsageWindow::from_index(idx);
                    if let Err(e) = app
                        .store
                        .set_setting("usage_graph_window", win.as_setting())
                    {
                        tracing::warn!(error = %e, "failed to persist usage_graph_window");
                    }
                }
                app.modal = None;
                return;
            }

            // Footer keybind hint click → behave exactly like pressing the
            // printed key. Only when no modal is open (a modal overlays the
            // footer, so clicks "through" it shouldn't fire hints). The footer
            // row doesn't overlap any other click target, so this is checked
            // first and returns early.
            if app.modal.is_none() {
                if let Some(action) = app.footer_hint_rects.iter().find_map(|(r, a)| {
                    let hit = m.column >= r.x
                        && m.column < r.x.saturating_add(r.width)
                        && m.row >= r.y
                        && m.row < r.y.saturating_add(r.height);
                    hit.then_some(*a)
                }) {
                    dispatch_footer_hint(app, action).await;
                    return;
                }
            }

            if let Some(idx) = app.chip_rects.iter().position(|r| {
                m.column >= r.x
                    && m.column < r.x.saturating_add(r.width)
                    && m.row >= r.y
                    && m.row < r.y.saturating_add(r.height)
            }) {
                fire_chip(app, idx).await;
            } else if let Some((ws_id, _)) = app.attention_rects.iter().copied().find(|(_, r)| {
                m.column >= r.x
                    && m.column < r.x.saturating_add(r.width)
                    && m.row >= r.y
                    && m.row < r.y.saturating_add(r.height)
            }) {
                // Clicking an attention entry attaches to that workspace,
                // identical to `Enter` on the dashboard.
                if let Err(e) = attach_workspace(app, ws_id) {
                    tracing::warn!(error = %e, "failed to attach from attention click");
                }
            } else if let Some((inst, _)) = app.agent_chip_rects.iter().copied().find(|(_, r)| {
                m.column >= r.x
                    && m.column < r.x.saturating_add(r.width)
                    && m.row >= r.y
                    && m.row < r.y.saturating_add(r.height)
            }) {
                // Clicking an agent pill retargets the focused pane to that
                // instance, spawning its session if needed.
                if let Err(e) = app.switch_focused_pane_to(inst) {
                    tracing::warn!(error = %e, "failed to switch pane from agent-pill click");
                }
            } else if let Some((ws_id, _)) = app.pr_link_rect.filter(|(_, r)| {
                m.column >= r.x
                    && m.column < r.x.saturating_add(r.width)
                    && m.row >= r.y
                    && m.row < r.y.saturating_add(r.height)
            }) {
                // Clicking the PR chip opens the PR in the browser.
                open_pr_for_workspace(app, ws_id);
            } else if app.modal.is_none()
                && app.usage_graph_rect.is_some_and(|r| {
                    m.column >= r.x
                        && m.column < r.x.saturating_add(r.width)
                        && m.row >= r.y
                        && m.row < r.y.saturating_add(r.height)
                })
            {
                // Clicking the footer activity graph opens the window picker,
                // seeded with the currently-applied window.
                let current = crate::config::usage_window::resolve(&app.store);
                app.modal = Some(Modal::UsageWindowPicker {
                    selected: current.index(),
                });
            }
        }
        _ => {}
    }
}
async fn dispatch_key(
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
            View::AttachedPm => handle_key_attached_pm(app, k).await?,
        }
    }
    Ok(())
}
pub(crate) async fn handle_event(app: &mut App, shared: &SharedApp, evt: CtEvent) -> Result<()> {
    match evt {
        CtEvent::Key(k) if k.kind == KeyEventKind::Press => dispatch_key(app, shared, k).await?,
        CtEvent::Mouse(m) => handle_mouse(app, m).await,
        CtEvent::Paste(content) => handle_paste(app, shared, content).await?,
        CtEvent::Resize(_, _) => {}
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
#[path = "input_tests.rs"]
mod tests;
