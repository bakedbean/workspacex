#![allow(clippy::collapsible_if)]

use crate::error::Result;
use crate::pty::session::SessionManager;
use crate::store::{Repo, Store, Workspace, WorkspaceId};
use crate::ui::View;
use crate::ui::dashboard::DashboardState;
use crate::ui::modal::Modal;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Leader key for attached-view actions (detach, open updates panel, send
/// literal leader to claude). Chosen to be free in raw mode and to avoid
/// collision with tmux's default `Ctrl-b` prefix (or any non-default
/// `Ctrl-a` setup).
const LEADER_KEY: crossterm::event::KeyCode = crossterm::event::KeyCode::Char('x');

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum AppEvent {
    Tick,
    Key(crossterm::event::KeyEvent),
    Resize(u16, u16),
    SetupLine(String),
    SetupFinished { id: WorkspaceId, ok: bool },
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionTarget {
    Repo(crate::store::RepoId),
    Workspace(crate::store::WorkspaceId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityState {
    /// The agent has stopped its turn and is awaiting human input. Derived
    /// from the JSONL `stop_reason` field — the only reliable "agent is
    /// done" signal. Higher priority than PTY-recency states.
    Stopped,
    /// A tool_use has been pending for ≥3s (almost always a permission
    /// prompt). Higher priority than `Stopped`.
    Awaiting,
    /// < 2s since last PTY output.
    Active,
    /// 2–30s since last PTY output.
    Idle,
    /// More than 30s since last PTY output but no JSONL stop signal.
    /// Retained for the recency column; does NOT drive the bell.
    Waiting,
    /// No session attached at all.
    Off,
}

impl ActivityState {
    /// States that should fire a bell + `!` marker when entered.
    pub fn is_alertable(self) -> bool {
        matches!(self, ActivityState::Stopped | ActivityState::Awaiting)
    }
}

fn classify_activity(secs: Option<u64>) -> ActivityState {
    match secs {
        Some(s) if s < 2 => ActivityState::Active,
        Some(s) if s < 30 => ActivityState::Idle,
        Some(_) => ActivityState::Waiting,
        None => ActivityState::Off,
    }
}

/// Compute the activity state for a workspace, combining JSONL-derived
/// signals (`stop_reason` + pending tool_uses) with PTY-output recency.
///
/// Priority: `Awaiting` (tool_use pending ≥3s) > `Stopped` (assistant
/// stop_reason = end_turn/max_tokens/stop_sequence and user hasn't replied)
/// > PTY-recency (Active/Idle/Waiting) > Off (no session).
fn classify_activity_with_events(
    secs: Option<u64>,
    running: bool,
    awaiting: bool,
    stopped: bool,
) -> ActivityState {
    if awaiting {
        return ActivityState::Awaiting;
    }
    if stopped {
        return ActivityState::Stopped;
    }
    if !running {
        return ActivityState::Off;
    }
    classify_activity(secs)
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

pub struct App {
    pub store: Store,
    pub sessions: SessionManager,
    pub view: View,
    pub modal: Option<Modal>,
    pub dashboard: DashboardState,
    pub repos: Vec<Repo>,
    pub workspaces: Vec<(crate::store::RepoId, Workspace)>,
    pub selectable: Vec<SelectionTarget>,
    pub worktree_base: PathBuf,
    pub leader_pending: bool,
    pub quit: bool,
    pub workspace_status:
        std::collections::HashMap<crate::store::WorkspaceId, crate::git::WorkspaceStatus>,
    /// Cached PR lifecycle per workspace. Absent key = never polled; present
    /// key = last successful poll's result.
    pub pr_lifecycle:
        std::collections::HashMap<crate::store::WorkspaceId, crate::forge::BranchLifecycle>,
    /// Last epoch-ms we attempted a PR fetch per workspace (throttle key).
    pub pr_last_poll_ms: std::collections::HashMap<crate::store::WorkspaceId, i64>,
    pub workspace_events:
        std::collections::HashMap<crate::store::WorkspaceId, crate::events::WorkspaceEvents>,
    /// Per-workspace tracking for attention-alert state.
    pub workspace_activity: std::collections::HashMap<crate::store::WorkspaceId, ActivityState>,
    /// Workspaces whose alert hasn't been acknowledged (cleared on attach).
    pub workspace_needs_attention: std::collections::HashSet<crate::store::WorkspaceId>,
    pub theme: crate::ui::theme::Theme,
    pub pm: Option<std::sync::Arc<crate::pty::session::Session>>,
    pub pm_visible: bool,
    pub focus: crate::ui::PaneFocus,
    pub pm_auto_summary_sent: bool,
}

impl App {
    pub fn new(store: Store, worktree_base: PathBuf) -> Result<Self> {
        let theme_name = store
            .get_setting("theme")
            .ok()
            .flatten()
            .unwrap_or_default();
        let theme = crate::ui::theme::Theme::by_name(&theme_name);
        let mut app = Self {
            store,
            sessions: SessionManager::new(),
            view: View::Dashboard,
            modal: None,
            dashboard: DashboardState::default(),
            repos: Vec::new(),
            workspaces: Vec::new(),
            selectable: Vec::new(),
            worktree_base,
            leader_pending: false,
            quit: false,
            workspace_status: std::collections::HashMap::new(),
            pr_lifecycle: std::collections::HashMap::new(),
            pr_last_poll_ms: std::collections::HashMap::new(),
            workspace_events: std::collections::HashMap::new(),
            workspace_activity: std::collections::HashMap::new(),
            workspace_needs_attention: std::collections::HashSet::new(),
            theme,
            pm: None,
            pm_visible: false,
            focus: crate::ui::PaneFocus::Dashboard,
            pm_auto_summary_sent: false,
        };
        // Sweep stale Pending rows from previous runs.
        let _ = app
            .store
            .sweep_stale_pending(std::time::Duration::from_secs(300));
        app.refresh()?;
        Ok(app)
    }

    pub fn refresh(&mut self) -> Result<()> {
        self.repos = self.store.repos()?;
        self.workspaces = Vec::new();
        for r in &self.repos {
            for w in self.store.workspaces(r.id)? {
                self.workspaces.push((r.id, w));
            }
        }
        // Rebuild selection targets: repos in order, each followed by its workspaces.
        self.selectable.clear();
        for repo in &self.repos {
            self.selectable.push(SelectionTarget::Repo(repo.id));
            for (rid, w) in &self.workspaces {
                if *rid == repo.id {
                    self.selectable.push(SelectionTarget::Workspace(w.id));
                }
            }
        }
        if !self.selectable.is_empty() && self.dashboard.selected >= self.selectable.len() {
            self.dashboard.selected = self.selectable.len() - 1;
        }
        Ok(())
    }

    pub fn selected_target(&self) -> Option<SelectionTarget> {
        self.selectable.get(self.dashboard.selected).copied()
    }

    /// If the workspace has any tool_use pending for ≥3s, return the oldest
    /// pending tool's (name, first-seen epoch ms). Returns None otherwise.
    ///
    /// 3 seconds is well past the latency of any auto-approved tool, so a
    /// pending entry that crosses that threshold is almost certainly waiting
    /// on a permission prompt the user needs to address.
    pub fn awaiting_permission(&self, ws_id: crate::store::WorkspaceId) -> Option<(String, i64)> {
        let evt = self.workspace_events.get(&ws_id)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        const STALE_MS: i64 = 3000;
        let mut oldest: Option<(&str, i64)> = None;
        for (name, ts) in evt.pending_tool_uses.values() {
            let age = now - *ts;
            if age >= STALE_MS {
                match oldest {
                    None => oldest = Some((name.as_str(), *ts)),
                    Some((_, t)) if *ts < t => oldest = Some((name.as_str(), *ts)),
                    _ => {}
                }
            }
        }
        oldest.map(|(n, ts)| (n.to_string(), ts))
    }
}

pub type SharedApp = Arc<Mutex<App>>;

use crossterm::event::{Event as CtEvent, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use ratatui::Terminal;
use ratatui::backend::Backend;
use std::time::Duration;

pub async fn run<B: Backend>(terminal: &mut Terminal<B>, app: SharedApp) -> Result<()> {
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(16));

    loop {
        {
            let mut g = app.lock().await;
            terminal.draw(|f| draw(f, &mut g))?;
            if g.quit {
                break;
            }
        }

        tokio::select! {
            _ = tick.tick() => {}
            maybe_evt = events.next() => {
                let Some(Ok(evt)) = maybe_evt else { break; };
                let mut g = app.lock().await;
                handle_event(&mut g, evt).await?;
            }
        }
    }
    Ok(())
}

fn draw(f: &mut ratatui::Frame, app: &mut App) {
    use crate::ui::{attached, dashboard, modal};
    let area = f.area();
    match &app.view {
        View::Dashboard => {
            let (dashboard_area, pm_area) = if app.pm_visible {
                let chunks = ratatui::layout::Layout::default()
                    .direction(ratatui::layout::Direction::Vertical)
                    .constraints([
                        ratatui::layout::Constraint::Percentage(60),
                        ratatui::layout::Constraint::Percentage(40),
                    ])
                    .split(area);
                (chunks[0], Some(chunks[1]))
            } else {
                (area, None)
            };
            let notifications_on = notifications_enabled(&app.store);
            let mut items: Vec<dashboard::Item> = Vec::new();
            for repo in &app.repos {
                items.push(dashboard::Item::Header { repo });
                let mut count = 0usize;
                for (rid, ws) in &app.workspaces {
                    if *rid != repo.id {
                        continue;
                    }
                    count += 1;
                    let session = app.sessions.get(ws.id);
                    let running = session.as_ref().is_some_and(|s| {
                        matches!(
                            *s.status.read().unwrap(),
                            crate::pty::session::SessionStatus::Running { .. }
                        )
                    });
                    let secs = session.as_ref().map(|s| {
                        let last = s.activity_ms.load(std::sync::atomic::Ordering::Relaxed);
                        if last == 0 {
                            return 0;
                        }
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);
                        now.saturating_sub(last) / 1000
                    });
                    let has_prior = crate::pty::session::has_prior_session(&ws.worktree_path);
                    let needs_attention = app.workspace_needs_attention.contains(&ws.id);
                    let awaiting = app.awaiting_permission(ws.id);
                    let stopped = app
                        .workspace_events
                        .get(&ws.id)
                        .is_some_and(|e| e.is_awaiting_user());
                    items.push(dashboard::Item::Workspace {
                        repo,
                        workspace: ws,
                        session_running: running,
                        seconds_since_activity: secs,
                        has_prior_session: has_prior,
                        status: app.workspace_status.get(&ws.id).copied(),
                        latest_event: app
                            .workspace_events
                            .get(&ws.id)
                            .and_then(|e| e.latest.clone()),
                        needs_attention,
                        stopped,
                        lifecycle: app.pr_lifecycle.get(&ws.id).copied(),
                        awaiting_tool: awaiting,
                    });
                }
                if count == 0 {
                    items.push(dashboard::Item::EmptyHint);
                }
                items.push(dashboard::Item::Spacer);
            }

            // Commit the new activity states + fire bell on transitions
            // into an alertable state (Stopped or Awaiting). Fires on:
            //   - first observation of a workspace already in an alertable
            //     state (e.g. wsx just started, agent was already waiting),
            //   - transition from any non-alertable state into Stopped or
            //     Awaiting,
            //   - transition between two different alertable states
            //     (e.g. Stopped -> Awaiting when a permission prompt arrives
            //     while the user hasn't yet replied to the prior end_turn).
            // Does NOT re-fire while an alertable state persists across
            // polls.
            let mut should_ring = false;
            for (_rid, ws) in &app.workspaces {
                let session = app.sessions.get(ws.id);
                let running = session.as_ref().is_some_and(|s| {
                    matches!(
                        *s.status.read().unwrap(),
                        crate::pty::session::SessionStatus::Running { .. }
                    )
                });
                let secs = session.as_ref().map(|s| {
                    let last = s.activity_ms.load(std::sync::atomic::Ordering::Relaxed);
                    if last == 0 {
                        return 0;
                    }
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    now.saturating_sub(last) / 1000
                });
                let awaiting = app.awaiting_permission(ws.id).is_some();
                let stopped = app
                    .workspace_events
                    .get(&ws.id)
                    .is_some_and(|e| e.is_awaiting_user());
                let activity = classify_activity_with_events(secs, running, awaiting, stopped);
                let prev = app.workspace_activity.get(&ws.id).copied();
                if activity.is_alertable() && prev != Some(activity) && notifications_on {
                    app.workspace_needs_attention.insert(ws.id);
                    should_ring = true;
                }
                app.workspace_activity.insert(ws.id, activity);
            }
            if should_ring {
                use std::io::Write;
                let _ = std::io::stdout().write_all(b"\x07");
                let _ = std::io::stdout().flush();
            }

            let selected = app.selected_target();
            let nerd_fonts = nerd_fonts_enabled(&app.store);
            dashboard::render(
                f,
                dashboard_area,
                &items,
                selected,
                nerd_fonts,
                &app.theme,
                &mut app.dashboard,
            );
            if let Some(pm_area) = pm_area {
                if let Some(session) = app.pm.as_ref() {
                    crate::ui::pm_pane::resize_session(session, pm_area);
                }
                crate::ui::pm_pane::render(f, pm_area, app.pm.as_ref(), app.focus, &app.theme);
            }
        }
        View::Attached(id) => {
            if let Some(session) = app.sessions.get(*id) {
                let label = app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == *id)
                    .map(|(_, w)| w.name.clone())
                    .unwrap_or_default();
                // The status row gets the inner width minus the "⚠ " prefix
                // (glyph + space) that `attached::render` prepends.
                let max_width = (area.width as usize).saturating_sub(3);
                let line = if matches!(
                    app.modal,
                    Some(crate::ui::modal::Modal::UpdatesPanel { .. })
                ) {
                    None
                } else {
                    compute_attention_line(app, Some(*id), max_width)
                };
                let footer_rows = if line.is_some() { 2 } else { 1 };
                attached::resize_session(&session, area, footer_rows);
                attached::render(f, area, &session, &label, line.as_deref(), &app.theme);
            }
        }
        View::AttachedPm => {
            if let Some(session) = app.pm.as_ref() {
                let max_width = (area.width as usize).saturating_sub(3);
                let line = if matches!(
                    app.modal,
                    Some(crate::ui::modal::Modal::UpdatesPanel { .. })
                ) {
                    None
                } else {
                    compute_attention_line(app, None, max_width)
                };
                let footer_rows = if line.is_some() { 2 } else { 1 };
                attached::resize_session(session, area, footer_rows);
                attached::render(
                    f,
                    area,
                    session,
                    "project-manager",
                    line.as_deref(),
                    &app.theme,
                );
            } else {
                // PM session went away; bounce to dashboard on next event.
                app.view = View::Dashboard;
            }
        }
    }
    if let Some(m) = &app.modal {
        match m {
            crate::ui::modal::Modal::UpdatesPanel { selected } => {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                let mut awaiting: std::collections::HashMap<
                    crate::store::WorkspaceId,
                    (String, i64),
                > = std::collections::HashMap::new();
                for (_rid, w) in &app.workspaces {
                    if let Some(a) = app.awaiting_permission(w.id) {
                        awaiting.insert(w.id, a);
                    }
                }
                let activity_translated: std::collections::HashMap<
                    crate::store::WorkspaceId,
                    crate::ui::updates_bar::ActivityState,
                > = app
                    .workspace_activity
                    .iter()
                    .map(|(k, v)| (*k, translate_activity(*v)))
                    .collect();
                crate::ui::modal::render_updates_panel(
                    f,
                    area,
                    &app.repos,
                    &app.workspaces,
                    &app.workspace_events,
                    &activity_translated,
                    &app.workspace_needs_attention,
                    &awaiting,
                    *selected,
                    now_ms,
                    &app.theme,
                );
            }
            other => modal::render(f, area, other, &app.theme),
        }
    }
}

#[doc(hidden)]
pub fn draw_for_test(f: &mut ratatui::Frame, app: &mut App) {
    draw(f, app);
}

fn nerd_fonts_enabled(store: &crate::store::Store) -> bool {
    match store.get_setting("nerd_fonts").ok().flatten().as_deref() {
        Some("false") | Some("0") | Some("off") | Some("no") => false,
        _ => true, // default ON
    }
}

fn pm_enabled(store: &Store) -> bool {
    match store.get_setting("pm_enabled").ok().flatten() {
        None => true,
        Some(v) => !matches!(
            v.trim().to_lowercase().as_str(),
            "false" | "0" | "off" | "no"
        ),
    }
}

fn notifications_enabled(store: &crate::store::Store) -> bool {
    match store.get_setting("notifications").ok().flatten().as_deref() {
        Some("off") | Some("false") | Some("0") | Some("no") => false,
        _ => true, // default ON
    }
}

async fn handle_event(app: &mut App, evt: CtEvent) -> Result<()> {
    match evt {
        CtEvent::Key(k) if k.kind == KeyEventKind::Press => {
            if app.modal.is_some() {
                handle_key_modal(app, k).await?;
            } else {
                match &app.view {
                    View::Dashboard => handle_key_dashboard(app, k).await?,
                    View::Attached(id) => handle_key_attached(app, *id, k).await?,
                    View::AttachedPm => handle_key_attached_pm(app, k).await?,
                }
            }
        }
        CtEvent::Resize(_, _) => {}
        _ => {}
    }
    Ok(())
}

async fn handle_key_dashboard(app: &mut App, k: crossterm::event::KeyEvent) -> Result<()> {
    // PM pane focus handling. When PM is focused, most keystrokes forward
    // to its PTY. Tab/Esc swap back to dashboard; `p` and `r` are still
    // handled by the main match below.
    if app.pm_visible && matches!(app.focus, crate::ui::PaneFocus::ProjectManager) {
        match (k.code, k.modifiers) {
            (KeyCode::Tab, _) | (KeyCode::Esc, _) => {
                app.focus = crate::ui::PaneFocus::Dashboard;
                return Ok(());
            }
            (KeyCode::Char('o'), m) if m.contains(KeyModifiers::CONTROL) => {
                // Ctrl-O: expand PM to a full-screen attached view so the
                // user can scroll through claude's history naturally.
                if app.pm.is_some() {
                    app.view = View::AttachedPm;
                }
                return Ok(());
            }
            (KeyCode::Char('p'), _) | (KeyCode::Char('r'), _) => {
                // Fall through to the main match.
            }
            _ => {
                if let Some(session) = app.pm.as_ref() {
                    if let Some(bytes) = encode_key_for_pty(&k) {
                        let _ = session.writer.send(bytes).await;
                    }
                }
                return Ok(());
            }
        }
    }
    // Tab when focus is on Dashboard and PM is visible: swap to PM.
    if app.pm_visible
        && matches!(app.focus, crate::ui::PaneFocus::Dashboard)
        && k.code == KeyCode::Tab
    {
        app.focus = crate::ui::PaneFocus::ProjectManager;
        return Ok(());
    }
    match (k.code, k.modifiers) {
        (KeyCode::Char('q'), _) => app.quit = true,
        (KeyCode::Up, _) => {
            app.dashboard.selected = app.dashboard.selected.saturating_sub(1);
        }
        (KeyCode::Down, _) => {
            let max = app.selectable.len().saturating_sub(1);
            app.dashboard.selected = (app.dashboard.selected + 1).min(max);
        }
        (KeyCode::Enter, _) => match app.selected_target() {
            Some(SelectionTarget::Workspace(id)) => {
                app.workspace_needs_attention.remove(&id);
                if let Some((id, path, mode)) = build_spawn_info(app, id) {
                    let _ = app.sessions.spawn(id, &path, 80, 24, mode)?;
                    app.view = View::Attached(id);
                }
            }
            Some(SelectionTarget::Repo(id)) => {
                app.modal = Some(Modal::NewWorkspace {
                    repo_id: id,
                    name_buffer: String::new(),
                    yolo: false,
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
                });
            }
        }
        (KeyCode::Char('e'), _) => {
            if let Some(SelectionTarget::Workspace(id)) = app.selected_target() {
                let info = app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == id)
                    .map(|(rid, w)| (*rid, w.worktree_path.clone()));
                if let Some((_, path)) = info {
                    let cmd = app.store.get_setting("editor_cmd").ok().flatten();
                    if let Err(e) = crate::external::open_in_editor(&path, cmd.as_deref()) {
                        app.modal = Some(Modal::Error {
                            message: e.to_string(),
                        });
                    }
                }
            }
        }
        (KeyCode::Char('t'), _) => {
            if let Some(SelectionTarget::Workspace(id)) = app.selected_target() {
                let info = app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == id)
                    .map(|(rid, w)| (*rid, w.worktree_path.clone()));
                if let Some((_, path)) = info {
                    let cmd = app.store.get_setting("terminal_cmd").ok().flatten();
                    if let Err(e) = crate::external::open_in_terminal(&path, cmd.as_deref()) {
                        app.modal = Some(Modal::Error {
                            message: e.to_string(),
                        });
                    }
                }
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
            if app.pm_visible && matches!(app.focus, crate::ui::PaneFocus::ProjectManager) =>
        {
            let dirs = crate::config::Dirs::discover();
            let pm_dir = dirs.pm_dir();
            if let Err(e) = crate::pm::refresh_pm(&mut app.sessions, &app.store, &pm_dir).await {
                app.modal = Some(Modal::Error {
                    message: e.to_string(),
                });
            }
        }
        (KeyCode::Char('p'), _) => {
            if pm_enabled(&app.store) {
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
                        crate::pm::open_pm(&mut app.sessions, &app.store, &pm_dir, custom).await
                    } else {
                        crate::pm::open_pm_with_auto_summary(
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
        }
        _ => {}
    }
    Ok(())
}

fn build_spawn_info(
    app: &App,
    ws_id: crate::store::WorkspaceId,
) -> Option<(
    crate::store::WorkspaceId,
    std::path::PathBuf,
    crate::pty::session::SpawnMode,
)> {
    let (rid, ws) = app.workspaces.iter().find(|(_, w)| w.id == ws_id)?;
    let repo = app.repos.iter().find(|r| r.id == *rid)?;
    let custom = crate::repo::resolve_custom_instructions(repo, &app.store)
        .ok()
        .flatten();
    let yolo = ws.yolo;
    let mode = if crate::pty::session::has_prior_session(&ws.worktree_path) {
        crate::pty::session::SpawnMode::Continue {
            custom_instructions: custom,
            yolo,
        }
    } else {
        let rename_ctx = if crate::names::is_generated_slug(&ws.name) {
            let resolved_prefix =
                crate::repo::resolve_branch_prefix(repo, &app.store).unwrap_or_default();
            Some(crate::pty::session::RenameContext {
                current_branch: ws.branch.clone(),
                branch_prefix: resolved_prefix,
            })
        } else {
            None
        };
        crate::pty::session::SpawnMode::Fresh {
            rename_ctx,
            custom_instructions: custom,
            yolo,
        }
    };
    Some((ws_id, ws.worktree_path.clone(), mode))
}

async fn handle_key_attached(
    app: &mut App,
    id: WorkspaceId,
    k: crossterm::event::KeyEvent,
) -> Result<()> {
    let session = match app.sessions.get(id) {
        Some(s) => s,
        None => {
            app.view = View::Dashboard;
            return Ok(());
        }
    };
    // Leader-key prefix handling. See `LEADER_KEY`.
    if app.leader_pending {
        app.leader_pending = false;
        match k.code {
            KeyCode::Char('d') => {
                app.view = View::Dashboard;
                return Ok(());
            }
            KeyCode::Char('x') => {
                // Send a literal Ctrl-x (0x18) to claude.
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
                    if let Some(slug) = crate::workspace::slugify_prompt(&prompt) {
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
                                    let _ = crate::workspace::rename(&app.store, &repo, &ws, &slug)
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

async fn handle_key_attached_pm(app: &mut App, k: crossterm::event::KeyEvent) -> Result<()> {
    let session = match app.pm.clone() {
        Some(s) => s,
        None => {
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
        let _ = session.writer.send(bytes).await;
    }
    Ok(())
}

async fn handle_key_modal(app: &mut App, k: crossterm::event::KeyEvent) -> Result<()> {
    let modal = app.modal.clone().unwrap();
    match modal {
        Modal::NewWorkspace {
            repo_id,
            mut name_buffer,
            yolo,
        } => match k.code {
            KeyCode::Esc => {
                app.modal = None;
            }
            KeyCode::Enter => {
                // Live-log streaming during create is intentionally deferred. The
                // borrow checker would force a channel-based dance to mutate
                // `app.modal` while `app.store` is borrowed inside `workspace::create`.
                // v1: show a static "running..." modal, swap it for the result.
                let name = if name_buffer.trim().is_empty() {
                    None
                } else {
                    Some(name_buffer.clone())
                };
                let repo = app.repos.iter().find(|r| r.id == repo_id).unwrap().clone();
                let base = app.worktree_base.clone();
                app.modal = Some(Modal::SetupRunning {
                    log: vec!["running setup...".into()],
                });
                let result = crate::workspace::create(
                    &app.store,
                    &repo,
                    name.as_deref(),
                    &base,
                    yolo,
                    |_| {},
                )
                .await;
                match result {
                    Ok(_) => {
                        app.modal = None;
                        app.refresh()?;
                    }
                    Err(e) => {
                        app.modal = Some(Modal::Error {
                            message: e.to_string(),
                        });
                    }
                }
            }
            KeyCode::Backspace => {
                name_buffer.pop();
                app.modal = Some(Modal::NewWorkspace {
                    repo_id,
                    name_buffer,
                    yolo,
                });
            }
            KeyCode::Char(c) => {
                name_buffer.push(c);
                app.modal = Some(Modal::NewWorkspace {
                    repo_id,
                    name_buffer,
                    yolo,
                });
            }
            _ => {}
        },
        Modal::ConfirmArchive { workspace_id, name } => match k.code {
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
                let result = crate::workspace::archive(
                    &app.store,
                    &repo,
                    &ws,
                    crate::workspace::ArchiveOpts {
                        force_branch_delete: true,
                        ..Default::default()
                    },
                    |_| {},
                )
                .await;
                match result {
                    Ok(_) => {
                        app.modal = None;
                        app.refresh()?;
                    }
                    Err(e) => {
                        app.modal = Some(Modal::Error {
                            message: e.to_string(),
                        });
                    }
                }
                let _ = name;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                app.modal = None;
            }
            _ => {}
        },
        Modal::Error { .. } | Modal::SetupRunning { .. } => {
            if matches!(k.code, KeyCode::Esc | KeyCode::Enter) {
                app.modal = None;
            }
        }
        Modal::UpdatesPanel { selected } => {
            let selected_now = selected;
            // Build the same ordered workspace list the renderer uses, so
            // arrow keys and Enter operate on the same indices.
            let activity_translated: std::collections::HashMap<
                crate::store::WorkspaceId,
                crate::ui::updates_bar::ActivityState,
            > = app
                .workspace_activity
                .iter()
                .map(|(k, v)| (*k, translate_activity(*v)))
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
                KeyCode::Up => {
                    let new_sel = selected_now.saturating_sub(1);
                    app.modal = Some(Modal::UpdatesPanel { selected: new_sel });
                }
                KeyCode::Down => {
                    let max = order.len().saturating_sub(1);
                    let new_sel = (selected_now + 1).min(max);
                    app.modal = Some(Modal::UpdatesPanel { selected: new_sel });
                }
                KeyCode::Enter => {
                    if let Some(ws_id) = order.get(selected_now).copied() {
                        // Mirror the dashboard-attach flow: clear the
                        // alert, spawn (or resume) the PTY, switch view.
                        app.workspace_needs_attention.remove(&ws_id);
                        if let Some((id, path, mode)) = build_spawn_info(app, ws_id) {
                            let _ = app.sessions.spawn(id, &path, 80, 24, mode)?;
                            app.view = View::Attached(id);
                        }
                    }
                    app.modal = None;
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn compute_attention_line(
    app: &App,
    attached_id: Option<crate::store::WorkspaceId>,
    max_width: usize,
) -> Option<String> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let candidates: Vec<crate::ui::updates_bar::WorkspaceUpdateInfo> = app
        .workspaces
        .iter()
        .map(|(rid, w)| {
            let activity = app
                .workspace_activity
                .get(&w.id)
                .copied()
                .map(translate_activity)
                .unwrap_or(crate::ui::updates_bar::ActivityState::Off);
            let repo_name = app
                .repos
                .iter()
                .find(|r| r.id == *rid)
                .map(|r| r.name.as_str())
                .unwrap_or("");
            crate::ui::updates_bar::WorkspaceUpdateInfo {
                id: w.id,
                name: w.name.as_str(),
                repo_name,
                events: app.workspace_events.get(&w.id),
                activity,
                needs_attention: app.workspace_needs_attention.contains(&w.id),
                awaiting_tool: app.awaiting_permission(w.id),
            }
        })
        .collect();
    let entries = crate::ui::updates_bar::collect_attention(&candidates, attached_id, now_ms);
    crate::ui::updates_bar::format_attention_line(&entries, now_ms, max_width)
}

fn translate_activity(a: ActivityState) -> crate::ui::updates_bar::ActivityState {
    use crate::ui::updates_bar::ActivityState as U;
    match a {
        ActivityState::Stopped => U::Stopped,
        ActivityState::Awaiting => U::Awaiting,
        ActivityState::Active => U::Active,
        ActivityState::Idle => U::Idle,
        ActivityState::Waiting => U::Waiting,
        ActivityState::Off => U::Off,
    }
}

/// Periodically check each live workspace's current git branch against
/// the DB; if claude (or a user) renamed it, update name + branch in the
/// store. Runs forever; cheap when nothing has drifted.
pub async fn branch_drift_poll(app: SharedApp) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        interval.tick().await;
        let snapshot: Vec<(WorkspaceId, std::path::PathBuf, String, String)> = {
            let g = app.lock().await;
            g.workspaces
                .iter()
                .filter_map(|(_, w)| {
                    let repo = g.repos.iter().find(|r| r.id == w.repo_id)?;
                    let prefix =
                        crate::repo::resolve_branch_prefix(repo, &g.store).unwrap_or_default();
                    Some((w.id, w.worktree_path.clone(), w.branch.clone(), prefix))
                })
                .collect()
        };

        for (id, path, db_branch, prefix) in snapshot {
            if !path.exists() {
                continue;
            }

            // 1) Branch drift (existing logic).
            if let Ok(current) = crate::git::current_branch(&path).await {
                if current != db_branch && current != "HEAD" {
                    let new_name = if prefix.is_empty() {
                        current.clone()
                    } else {
                        let strip = format!("{}/", prefix.trim_end_matches('/'));
                        current.strip_prefix(&strip).unwrap_or(&current).to_string()
                    };
                    let mut g = app.lock().await;
                    let _ = g.store.rename_workspace(id, &new_name);
                    let _ = g.store.set_workspace_branch(id, &current);
                    let _ = g.refresh();
                    // Invalidate cached PR state — the new branch may have a
                    // different (or no) PR. Clearing the throttle stamp
                    // makes the next tick poll immediately.
                    g.pr_lifecycle.remove(&id);
                    g.pr_last_poll_ms.remove(&id);
                }
            }

            // 2) Workspace status — refresh the cache for this workspace.
            if let Ok(status) = crate::git::workspace_status(&path).await {
                let mut g = app.lock().await;
                g.workspace_status.insert(id, status);
            }

            // 3) PR lifecycle — throttled to once per 30s per workspace.
            //    gh is a network call, so we don't run it every tick.
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let should_poll_pr = {
                let g = app.lock().await;
                g.pr_last_poll_ms
                    .get(&id)
                    .map(|t| now_ms.saturating_sub(*t) >= 30_000)
                    .unwrap_or(true)
            };
            if should_poll_pr {
                // Mark the attempt before awaiting the fetch, so concurrent
                // ticks don't queue up multiple gh processes.
                {
                    let mut g = app.lock().await;
                    g.pr_last_poll_ms.insert(id, now_ms);
                }
                if let Ok(Some(lifecycle)) =
                    crate::forge::fetch_branch_lifecycle(&path, &db_branch).await
                {
                    let mut g = app.lock().await;
                    g.pr_lifecycle.insert(id, lifecycle);
                }
                // Ok(None) → leave any existing cached value alone; better
                // than clobbering a previously-known state on a transient
                // network error.
            }

            // 4) Tail Claude Code session JSONL for events.
            //
            // Lock-ordering: snapshot the previous offset under the lock,
            // do the file I/O without the lock held, then re-acquire to
            // commit the new offset + events. This keeps the UI responsive
            // even when sessions grow large.
            let current_file = crate::events::locate_session_file(&path);
            let prev_offset = {
                let g = app.lock().await;
                match (g.workspace_events.get(&id), current_file.as_ref()) {
                    (Some(evt), Some(f)) if evt.file_path.as_deref() == Some(f.as_path()) => {
                        evt.byte_offset
                    }
                    _ => 0,
                }
            };
            if let Some(file) = current_file {
                if let Ok(update) = crate::events::tail_session(&file, prev_offset) {
                    let crate::events::TailUpdate {
                        new_offset,
                        events,
                        tool_use_starts,
                        tool_use_resolves,
                        last_stop_reason,
                        human_replied_after_last_stop,
                        reset_from_zero,
                    } = update;
                    let mut g = app.lock().await;
                    let evt = g.workspace_events.entry(id).or_default();
                    // If the session file was replaced (different path) or
                    // truncated/rewound (reset_from_zero), discard all
                    // session-derived state before applying the new batch.
                    // Otherwise stale tool_uses or stop_reasons from the
                    // prior session keep the dashboard stuck on "awaiting".
                    let file_changed = evt.file_path.as_deref() != Some(file.as_path());
                    if file_changed || reset_from_zero {
                        evt.reset_session_state();
                    }
                    evt.file_path = Some(file);
                    evt.byte_offset = new_offset;
                    for (tu_id, tu_name, ts) in tool_use_starts {
                        evt.pending_tool_uses.insert(tu_id, (tu_name, ts));
                    }
                    for tu_id in tool_use_resolves {
                        evt.pending_tool_uses.remove(&tu_id);
                    }
                    // Update the "agent is waiting on user" tracking.
                    // - A fresh assistant stop_reason replaces the prior one
                    //   and resets the user-replied latch (the agent just
                    //   produced a new stopping point).
                    // - `human_replied_after_last_stop` from this batch
                    //   already accounts for within-batch ordering: it's set
                    //   only if real user text appears AFTER the last
                    //   stop_reason in the batch (or anywhere in the batch
                    //   if there's no new stop_reason).
                    if let Some(sr) = last_stop_reason {
                        evt.last_stop_reason = Some(sr);
                        evt.user_replied_since_stop = false;
                    }
                    if human_replied_after_last_stop {
                        evt.user_replied_since_stop = true;
                    }
                    for e in events {
                        crate::events::push_event(evt, e);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod activity_classifier_tests {
    use super::*;

    #[test]
    fn awaiting_wins_over_stopped_over_recency() {
        // awaiting (permission) beats everything.
        assert_eq!(
            classify_activity_with_events(Some(0), true, true, true),
            ActivityState::Awaiting
        );
        assert_eq!(
            classify_activity_with_events(Some(0), true, true, false),
            ActivityState::Awaiting
        );
        // stopped beats PTY recency.
        assert_eq!(
            classify_activity_with_events(Some(0), true, false, true),
            ActivityState::Stopped
        );
    }

    #[test]
    fn no_session_is_off_even_if_running_false() {
        assert_eq!(
            classify_activity_with_events(None, false, false, false),
            ActivityState::Off
        );
        // Even with pty seconds, if running=false → Off.
        assert_eq!(
            classify_activity_with_events(Some(5), false, false, false),
            ActivityState::Off
        );
    }

    #[test]
    fn awaiting_fires_even_when_session_not_running() {
        // A pending tool_use is a strong signal regardless of pty state.
        assert_eq!(
            classify_activity_with_events(None, false, true, false),
            ActivityState::Awaiting
        );
    }

    #[test]
    fn pty_recency_drives_active_idle_waiting_when_no_event_signals() {
        assert_eq!(
            classify_activity_with_events(Some(0), true, false, false),
            ActivityState::Active
        );
        assert_eq!(
            classify_activity_with_events(Some(10), true, false, false),
            ActivityState::Idle
        );
        assert_eq!(
            classify_activity_with_events(Some(60), true, false, false),
            ActivityState::Waiting
        );
    }

    #[test]
    fn is_alertable_includes_stopped_and_awaiting_only() {
        assert!(ActivityState::Stopped.is_alertable());
        assert!(ActivityState::Awaiting.is_alertable());
        assert!(!ActivityState::Active.is_alertable());
        assert!(!ActivityState::Idle.is_alertable());
        assert!(!ActivityState::Waiting.is_alertable());
        assert!(!ActivityState::Off.is_alertable());
    }
}

#[cfg(test)]
mod pm_state_tests {
    use super::*;
    use crate::store::Store;
    use std::path::PathBuf;

    #[test]
    fn app_initializes_pm_state_off() {
        let store = Store::open_in_memory().unwrap();
        let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        assert!(app.pm.is_none());
        assert!(!app.pm_visible);
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
    }

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn dashboard_renders_full_area_when_pm_hidden() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        assert!(!app.pm_visible);
        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw_for_test(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        let rendered = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!rendered.contains("Project Manager"), "{rendered}");
    }

    #[test]
    fn dashboard_renders_split_with_pm_title_when_visible_even_without_session() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.pm_visible = true; // No session yet — the pane shows a placeholder.
        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw_for_test(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        let rendered = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("Project Manager"),
            "expected pane title in rendered buffer:\n{rendered}"
        );
        assert!(
            rendered.contains("Tab to focus"),
            "expected unfocused hint:\n{rendered}"
        );
    }

    use crossterm::event::{KeyEvent, KeyModifiers};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tab_swaps_focus_when_pm_visible() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.pm_visible = true;
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(crossterm::event::KeyCode::Tab, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::ProjectManager));
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(crossterm::event::KeyCode::Tab, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn esc_returns_focus_to_dashboard() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.pm_visible = true;
        app.focus = crate::ui::PaneFocus::ProjectManager;
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(crossterm::event::KeyCode::Esc, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tab_no_op_when_pm_hidden() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        assert!(!app.pm_visible);
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(crossterm::event::KeyCode::Tab, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn updates_panel_modal_esc_closes() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });
        handle_key_modal(
            &mut app,
            KeyEvent::new(crossterm::event::KeyCode::Esc, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(app.modal.is_none(), "Esc should close UpdatesPanel");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn updates_panel_modal_down_advances_selection() {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        // Two workspaces so Down has somewhere to go.
        for (name, branch, path) in [
            ("alpha", "repo/alpha", "/tmp/wsx-test/alpha"),
            ("beta", "repo/beta", "/tmp/wsx-test/beta"),
        ] {
            let id = store
                .insert_workspace(&NewWorkspace {
                    repo_id,
                    name,
                    branch,
                    worktree_path: std::path::Path::new(path),
                    yolo: false,
                })
                .unwrap();
            store
                .set_workspace_state(id, WorkspaceState::Ready)
                .unwrap();
        }
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });
        handle_key_modal(
            &mut app,
            KeyEvent::new(crossterm::event::KeyCode::Down, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        match app.modal {
            Some(crate::ui::modal::Modal::UpdatesPanel { selected }) => {
                assert_eq!(selected, 1, "Down should advance to index 1");
            }
            other => panic!("unexpected modal state: {other:?}"),
        }
        // Down again clamps at the last index.
        handle_key_modal(
            &mut app,
            KeyEvent::new(crossterm::event::KeyCode::Down, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        match app.modal {
            Some(crate::ui::modal::Modal::UpdatesPanel { selected }) => {
                assert_eq!(selected, 1, "Down past last clamps at max");
            }
            other => panic!("unexpected modal state: {other:?}"),
        }
        // Up returns to 0.
        handle_key_modal(
            &mut app,
            KeyEvent::new(crossterm::event::KeyCode::Up, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        match app.modal {
            Some(crate::ui::modal::Modal::UpdatesPanel { selected }) => {
                assert_eq!(selected, 0, "Up should retreat to 0");
            }
            other => panic!("unexpected modal state: {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn updates_panel_modal_enter_switches_view_and_clears_attention() {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let ws_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "blocked",
                branch: "repo/blocked",
                worktree_path: std::path::Path::new("."),
                yolo: false,
            })
            .unwrap();
        store
            .set_workspace_state(ws_id, WorkspaceState::Ready)
            .unwrap();

        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.workspace_needs_attention.insert(ws_id);
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });
        handle_key_modal(
            &mut app,
            KeyEvent::new(crossterm::event::KeyCode::Enter, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(app.modal.is_none(), "Enter should close the modal");
        assert!(
            matches!(app.view, crate::ui::View::Attached(id) if id == ws_id),
            "Enter should switch view to the selected workspace; got {:?}",
            app.view
        );
        assert!(
            !app.workspace_needs_attention.contains(&ws_id),
            "attention flag should clear on Enter"
        );
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn updates_panel_modal_swallows_other_keys() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });
        handle_key_modal(
            &mut app,
            KeyEvent::new(crossterm::event::KeyCode::Char('q'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(app.modal.is_some(), "q should not dismiss UpdatesPanel");
        assert!(!app.quit, "q should not propagate to App::quit");
    }

    #[test]
    fn updates_panel_render_shows_grouped_workspaces() {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let store = Store::open_in_memory().unwrap();
        let repo1 = store
            .add_repo(std::path::Path::new("/tmp/r1"), "repo-alpha", "")
            .unwrap();
        let ws1 = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo1,
                name: "alpha-ws",
                branch: "repo-alpha/alpha-ws",
                worktree_path: std::path::Path::new("/tmp/wsx-test/alpha-ws"),
                yolo: false,
            })
            .unwrap();
        store
            .set_workspace_state(ws1, WorkspaceState::Ready)
            .unwrap();
        let repo2 = store
            .add_repo(std::path::Path::new("/tmp/r2"), "repo-beta", "")
            .unwrap();
        let ws2 = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo2,
                name: "beta-ws",
                branch: "repo-beta/beta-ws",
                worktree_path: std::path::Path::new("/tmp/wsx-test/beta-ws"),
                yolo: false,
            })
            .unwrap();
        store
            .set_workspace_state(ws2, WorkspaceState::Ready)
            .unwrap();

        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });

        let backend = TestBackend::new(100, 30);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw_for_test(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        let rendered = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("Workspace updates"),
            "missing panel title:\n{rendered}"
        );
        assert!(
            rendered.contains("repo-alpha"),
            "missing repo header:\n{rendered}"
        );
        assert!(
            rendered.contains("alpha-ws"),
            "missing workspace row:\n{rendered}"
        );
        assert!(
            rendered.contains("repo-beta"),
            "missing repo header:\n{rendered}"
        );
        assert!(
            rendered.contains("beta-ws"),
            "missing workspace row:\n{rendered}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn attached_view_shows_status_row_for_other_workspace_needing_attention() {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let attached_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "attached-here",
                branch: "repo/attached-here",
                worktree_path: std::path::Path::new("/tmp/wsx-test/attached"),
                yolo: false,
            })
            .unwrap();
        store
            .set_workspace_state(attached_id, WorkspaceState::Ready)
            .unwrap();
        let other_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "the-other",
                branch: "repo/the-other",
                worktree_path: std::path::Path::new("/tmp/wsx-test/other"),
                yolo: false,
            })
            .unwrap();
        store
            .set_workspace_state(other_id, WorkspaceState::Ready)
            .unwrap();

        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            yolo: false,
        };
        app.sessions
            .spawn(attached_id, std::path::Path::new("."), 80, 24, mode)
            .unwrap();
        app.view = crate::ui::View::Attached(attached_id);
        // The new status row exclusively surfaces workspaces with
        // `needs_attention` set — recent activity alone no longer qualifies.
        app.workspace_needs_attention.insert(other_id);

        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw_for_test(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        let rendered = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("the-other"),
            "expected status row mention of the other workspace:\n{rendered}"
        );
        assert!(
            rendered.contains('⚠'),
            "expected attention glyph on status row:\n{rendered}"
        );
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn attached_view_no_status_row_when_no_other_activity() {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let attached_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "only-one",
                branch: "repo/only-one",
                worktree_path: std::path::Path::new("/tmp/wsx-test/only"),
                yolo: false,
            })
            .unwrap();
        store
            .set_workspace_state(attached_id, WorkspaceState::Ready)
            .unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            yolo: false,
        };
        app.sessions
            .spawn(attached_id, std::path::Path::new("."), 80, 24, mode)
            .unwrap();
        app.view = crate::ui::View::Attached(attached_id);

        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw_for_test(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        // The bottom row is the footer with "Ctrl-x d detach". The second-
        // to-last row should NOT contain a status indicator glyph.
        let h = buf.area.height;
        let second_to_last: String = (0..buf.area.width)
            .map(|x| buf[(x, h - 2)].symbol())
            .collect();
        assert!(
            !second_to_last.contains('⚠'),
            "unexpected attention glyph in row {}: {second_to_last:?}",
            h - 2
        );
        assert!(
            !second_to_last.contains('●'),
            "unexpected activity glyph in row {}: {second_to_last:?}",
            h - 2
        );
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn leader_u_in_attached_pm_opens_updates_panel() {
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        // Manually spawn a PM session so handle_key_attached_pm has one.
        let cwd = PathBuf::from(".");
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            yolo: false,
        };
        let s = app.sessions.spawn_pm(&cwd, 80, 24, mode).unwrap();
        app.pm = Some(s);
        app.view = crate::ui::View::AttachedPm;

        // Send the leader (Ctrl-x) then 'u'.
        handle_key_attached_pm(
            &mut app,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        assert!(app.leader_pending);

        handle_key_attached_pm(
            &mut app,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(!app.leader_pending);
        assert!(matches!(
            app.modal,
            Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 })
        ));

        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }
}
