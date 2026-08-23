//! Key handling while the dashboard (the workspace list) has focus.
//!
//! Also the selection and fold helpers it drives, which the mouse
//! handler reuses so a click and its keyboard equivalent cannot drift.

use super::*;
use crate::app::{App, SelectionTarget, attach_workspace};
use crate::error::Result;
use crate::ui::modal::Modal;
use crossterm::event::{KeyCode, KeyModifiers};

// Test-only imports: the moved test modules access `draw_for_test`,
// `AttachedState`, `Arc`, and `Mutex` through `super::*` glob imports
// that cascade from the surrounding `tests` module.

/// Apply a scroll delta to whichever session is currently in focus.
/// `up=true` scrolls toward older content (higher offset).
pub(in crate::app::input) fn scroll_active(app: &App, rows: usize, up: bool) {
    let Some(session) = active_session(app) else {
        return;
    };
    if up {
        session.scroll_up(rows);
    } else {
        session.scroll_down(rows);
    }
}

/// Aggregate the current `StatusCounts` for one repo by classifying each
/// of its live workspaces. Used by the `z` (fold) keybinding so we can
/// look up the same default-fold state the renderer would compute.
pub(in crate::app::input) fn current_repo_counts(
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
pub(in crate::app::input) fn toggle_focused_fold(app: &mut App) {
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
pub(in crate::app::input) fn move_selected_repo(app: &mut App, up: bool) -> Result<()> {
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
pub(in crate::app::input) fn open_pr_for_workspace(
    app: &App,
    ws_id: crate::data::store::WorkspaceId,
) {
    if let Some((_, ws)) = app.workspaces.iter().find(|(_, w)| w.id == ws_id) {
        crate::git::forge::open_pr_in_browser(&ws.worktree_path, &ws.branch);
    }
}

/// The repo path a click should open the author-filtered PR list for, or
/// `None` if the click missed every repo header's PR link. Resolving the
/// path here (rather than in the mouse handler) keeps the decision
/// testable without spawning a browser. A rect whose repo was unregistered
/// between the draw and the click resolves to `None`.
pub(crate) fn repo_pr_link_target(
    app: &App,
    m: &crossterm::event::MouseEvent,
) -> Option<std::path::PathBuf> {
    let (repo_id, _) = app
        .dashboard_repo_pr_rects
        .iter()
        .find(|(_, r)| rect_contains(r, m))?;
    app.repos
        .iter()
        .find(|r| r.id == *repo_id)
        .map(|r| r.path.clone())
}

/// Vim-style `h` (fold) / `l` (unfold) on the focused row. Unlike
/// [`toggle_focused_fold`], this is idempotent: pressing `h` on an
/// already-folded repo leaves it folded.
pub(in crate::app::input) fn set_focused_fold(app: &mut App, fold: bool) {
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
pub(in crate::app::input) fn expand_all_repos(app: &mut App) {
    for r in &app.repos {
        app.dashboard.folded.insert(r.id.0 as u64, false);
    }
}

/// `zM` action: fold every registered repo by inserting an explicit
/// `true` in `dashboard.folded`. Overrides the renderer's heuristic.
pub(in crate::app::input) fn fold_all_repos(app: &mut App) {
    for r in &app.repos {
        app.dashboard.folded.insert(r.id.0 as u64, true);
    }
}

/// Clamp the PM digest selection after a filter edit shrinks the card list,
/// so the selection marker never points past the visible cards.
pub(in crate::app::input) fn clamp_pm_selection(app: &mut App) {
    let count = crate::ui::pm_pane::card_count(&app.build_pm_digest());
    app.pm_digest_selected = app.pm_digest_selected.min(count.saturating_sub(1));
}

pub(in crate::app::input) async fn handle_key_dashboard(
    app: &mut App,
    k: crossterm::event::KeyEvent,
) -> Result<()> {
    // PM digest focus handling: j/k navigate the flattened card list,
    // Enter attaches to the selected workspace, Tab/Esc return focus to
    // the dashboard, q/p close the pane, and r nudges a cache refresh.
    if app.pm_visible && matches!(app.focus, crate::ui::PaneFocus::ProjectManager) {
        // Defensive: PM focus means the dashboard's z-leader cannot be
        // meaningfully consumed here. Clear it so it doesn't leak across
        // focus transitions.
        app.z_leader_pending = false;
        // Filter editing intercepts printable chars, Backspace, and Esc
        // before the single-key bindings below — while typing, letters
        // like j/k/q/p/r are filter text, not shortcuts. Arrows, Enter,
        // and Tab fall through and keep their meanings.
        if app.pm_filter.is_some() {
            match k.code {
                KeyCode::Esc => {
                    app.pm_filter = None;
                    return Ok(());
                }
                KeyCode::Backspace => {
                    if let Some(buf) = app.pm_filter.as_mut() {
                        buf.pop();
                    }
                    clamp_pm_selection(app);
                    return Ok(());
                }
                KeyCode::Char(c)
                    if !c.is_control()
                        && !k.modifiers.contains(KeyModifiers::CONTROL)
                        && !k.modifiers.contains(KeyModifiers::ALT) =>
                {
                    if let Some(buf) = app.pm_filter.as_mut() {
                        buf.push(c);
                    }
                    clamp_pm_selection(app);
                    return Ok(());
                }
                _ => {}
            }
        }
        let digest = app.build_pm_digest();
        let count = crate::ui::pm_pane::card_count(&digest);
        match k.code {
            KeyCode::Tab | KeyCode::Esc => {
                app.focus = crate::ui::PaneFocus::Dashboard;
            }
            KeyCode::Char('q') | KeyCode::Char('p') => {
                app.pm_visible = false;
                app.pm_filter = None;
                app.focus = crate::ui::PaneFocus::Dashboard;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                app.pm_digest_selected = (app.pm_digest_selected + 1).min(count.saturating_sub(1));
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.pm_digest_selected = app.pm_digest_selected.saturating_sub(1);
            }
            KeyCode::Enter => {
                let idx = app.pm_digest_selected.min(count.saturating_sub(1));
                if let Some(card) = crate::ui::pm_pane::card_at(&digest, idx) {
                    let ws_id = card.workspace_id;
                    attach_workspace(app, ws_id)?;
                }
            }
            KeyCode::Char('r') => {
                nudge_status_refresh(app);
            }
            KeyCode::Char('/') => {
                app.pm_filter = Some(String::new());
            }
            _ => {}
        }
        return Ok(());
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
        app.leader_selected = 0;
        return Ok(());
    }
    match (k.code, k.modifiers) {
        (KeyCode::Char('q'), _) => {
            if app.in_flight.is_empty() {
                app.quit = true;
            } else {
                let creates = app
                    .in_flight
                    .values()
                    .filter(|f| f.kind == crate::data::in_flight::InFlightKind::Create)
                    .count();
                app.modal = Some(Modal::ConfirmQuit {
                    creates,
                    archives: app.in_flight.len() - creates,
                });
            }
        }
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
                    shared: false,
                    agent: crate::pty::session::AgentKind::from_store(&app.store),
                    notice: None,
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
                    shared: false,
                    agent: crate::pty::session::AgentKind::from_store(&app.store),
                    notice: None,
                });
            }
        }
        (KeyCode::Char('S'), _) => {
            // Capital S opens the modal with shared: true (tmux-shared
            // workspace, joinable by other users on the host via `tmux
            // attach`). Mirrors the n/N pattern above.
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
                    yolo: false,
                    shared: true,
                    agent: crate::pty::session::AgentKind::from_store(&app.store),
                    notice: None,
                });
            }
        }
        (KeyCode::Char('H'), _) => {
            // Capital H opens a picker over the configured shared hosts
            // (`wsx config edit shared_hosts`), sorted by name. No workspace
            // or repo selection is required — the fetch that follows (Enter
            // in the picker) targets a remote host, not the local tree.
            let hosts: Vec<(String, String)> = crate::commands::shared_hosts::list(&app.store)
                .unwrap_or_default()
                .into_iter()
                .map(|h| (h.name, h.dest))
                .collect();
            if hosts.is_empty() {
                app.modal = Some(Modal::Error {
                    message: "no shared hosts configured — add name=ssh-dest lines via `wsx config edit shared_hosts`".into(),
                });
            } else {
                app.modal = Some(Modal::RemoteHostPicker { hosts, selected: 0 });
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
            if let Some(SelectionTarget::Workspace(id)) = app.selected_target()
                // A workspace with any in-flight entry (create or archive)
                // already has a live cancellation handle and/or a worktree
                // being mutated; opening the archive confirm here would let
                // a second archive spawn on top of it, and the registry
                // insert on 'y' would clobber the existing entry so the
                // still-running operation's in_flight record — and its
                // cancel handle — becomes unreachable. Refuse silently, the
                // same way `attach_is_blocked` refuses attach.
                && !app.in_flight.contains_key(&id)
            {
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
        (KeyCode::Char('T'), _) => {
            // Shift+T toggles a workspace between direct and tmux-shared,
            // via a confirmation modal (running sessions restart via
            // --continue). No-op on a Repo header — sharing is per-workspace.
            if let Some(SelectionTarget::Workspace(id)) = app.selected_target()
                && let Some((_, ws)) = app.workspaces.iter().find(|(_, w)| w.id == id)
            {
                let agents = app.store.workspace_agents(id)?;
                let running_count = agents
                    .iter()
                    .filter(|inst| app.instance_is_running(inst.id))
                    .count();
                app.modal = Some(Modal::ConfirmShare {
                    workspace_id: id,
                    name: ws.name.clone(),
                    to_shared: !ws.shared,
                    running_count,
                    stopped_count: agents.len() - running_count,
                });
            }
        }
        (KeyCode::Char('r'), _)
            if app.pm_visible && matches!(app.focus, crate::ui::PaneFocus::Dashboard) =>
        {
            // Manual refresh of the PM digest: nudge the throttled pollers
            // so the next tick refetches PR/diff state. Only fires from
            // Dashboard focus; PM-focused 'r' is handled above.
            nudge_status_refresh(app);
        }
        (KeyCode::Char('G'), _) => {
            use crate::ui::dashboard::layout::GroupMode;
            app.dashboard.group_mode = match app.dashboard.group_mode {
                GroupMode::Repo => GroupMode::Attention,
                GroupMode::Attention => GroupMode::Repo,
            };
        }
        (KeyCode::Char('o'), _) => {
            app.dashboard.cycle_sort_mode(&app.store);
        }
        (KeyCode::Char('z'), _) => {
            app.z_leader_pending = true;
        }
        (KeyCode::Char('/'), _) => {
            app.dashboard.filter = Some(String::new());
        }
        (KeyCode::Char('?'), _) => {
            if matches!(app.selected_target(), Some(SelectionTarget::Workspace(_))) {
                app.modal = Some(Modal::WorkspaceActions);
            }
        }
        // `C` (not `c`, which is chronox) opens the name-color picker for the
        // selected workspace. `current` is snapshotted here so the grid can
        // mark the applied color without re-reading the store each frame.
        (KeyCode::Char('C'), _) => {
            if let Some(SelectionTarget::Workspace(ws_id)) = app.selected_target() {
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
        (KeyCode::Char('p'), _) => {
            // Closing drops the filter; opening must never inherit a
            // stale one either.
            app.pm_filter = None;
            if app.pm_visible {
                app.pm_visible = false;
                app.focus = crate::ui::PaneFocus::Dashboard;
            } else {
                app.pm_visible = true;
                app.pm_digest_selected = 0;
                app.focus = crate::ui::PaneFocus::ProjectManager;
            }
        }
        _ => {}
    }
    Ok(())
}

/// The updates panel's ordered workspace list. Returns an owned Vec so the
/// borrow of `app` ends at the call — the caller mutates `app.modal` right
/// after. Both the key handler and `app::render` must derive row order from
/// identical inputs or the selection indices drift from the drawn rows.
pub(in crate::app::input) fn panel_order(
    app: &App,
    sort: crate::ui::modal::UpdatesSort,
    filter: Option<&str>,
) -> Vec<crate::data::store::WorkspaceId> {
    let activity_translated: std::collections::HashMap<
        crate::data::store::WorkspaceId,
        crate::ui::updates_bar::ActivityState,
    > = app
        .workspace_activity
        .iter()
        .map(|(k, v)| (*k, crate::app::render::translate_activity(*v)))
        .collect();
    let statuses = app.classified_statuses();
    let awaiting = app.awaiting_permission_map();
    let inputs = crate::ui::modal::PanelInputs {
        repos: &app.repos,
        workspaces: &app.workspaces,
        events: &app.workspace_events,
        activity: &activity_translated,
        needs_attention: &app.workspace_needs_attention,
        awaiting: &awaiting,
        statuses: &statuses,
        lifecycles: &app.pr_lifecycle,
    };
    crate::ui::modal::ordered_workspaces_for_panel(&inputs, sort, filter)
}

/// Keep the cursor on its workspace across a re-order (a sort cycle or a
/// filter edit) rather than on its index. Falls back to clamping the old
/// index into range when the workspace is gone from the new order — which
/// is what a filter that hid it does.
pub(in crate::app::input) fn reselect(
    selected_id: Option<crate::data::store::WorkspaceId>,
    new_order: &[crate::data::store::WorkspaceId],
    old_index: usize,
) -> usize {
    if let Some(pos) = selected_id.and_then(|id| new_order.iter().position(|w| *w == id)) {
        return pos;
    }
    old_index.min(new_order.len().saturating_sub(1))
}
