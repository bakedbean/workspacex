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
    Active,   // < 2s since last output
    Idle,     // 2-30s
    Waiting,  // > 30s
    Awaiting, // tool_use pending for ≥3s (almost always a permission prompt)
    Off,      // no session at all
}

fn classify_activity(secs: Option<u64>) -> ActivityState {
    match secs {
        Some(s) if s < 2 => ActivityState::Active,
        Some(s) if s < 30 => ActivityState::Idle,
        Some(_) => ActivityState::Waiting,
        None => ActivityState::Off,
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
    pub ctrl_a_pending: bool,
    pub quit: bool,
    pub workspace_status:
        std::collections::HashMap<crate::store::WorkspaceId, crate::git::WorkspaceStatus>,
    pub workspace_events:
        std::collections::HashMap<crate::store::WorkspaceId, crate::events::WorkspaceEvents>,
    /// Per-workspace tracking for attention-alert state.
    pub workspace_activity: std::collections::HashMap<crate::store::WorkspaceId, ActivityState>,
    /// Workspaces whose alert hasn't been acknowledged (cleared on attach).
    pub workspace_needs_attention: std::collections::HashSet<crate::store::WorkspaceId>,
    pub theme: crate::ui::theme::Theme,
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
            ctrl_a_pending: false,
            quit: false,
            workspace_status: std::collections::HashMap::new(),
            workspace_events: std::collections::HashMap::new(),
            workspace_activity: std::collections::HashMap::new(),
            workspace_needs_attention: std::collections::HashSet::new(),
            theme,
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
                        awaiting_tool: awaiting,
                    });
                }
                if count == 0 {
                    items.push(dashboard::Item::EmptyHint);
                }
                items.push(dashboard::Item::Spacer);
            }

            // Commit the new activity states + fire bell on
            // active|idle -> waiting transitions. Done after the immutable
            // iteration above so we don't conflict with the &app borrow.
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
                let awaiting = app.awaiting_permission(ws.id);
                let activity = if awaiting.is_some() {
                    ActivityState::Awaiting
                } else if running {
                    classify_activity(secs)
                } else {
                    ActivityState::Off
                };
                let prev = app.workspace_activity.get(&ws.id).copied();
                // Fire the bell on either kind of "user needs to act"
                // transition: PTY-quiet → Waiting, or tool_use pending → Awaiting.
                if matches!(
                    (prev, activity),
                    (
                        Some(ActivityState::Active) | Some(ActivityState::Idle),
                        ActivityState::Waiting | ActivityState::Awaiting
                    )
                ) && notifications_on
                {
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
                area,
                &items,
                selected,
                nerd_fonts,
                &app.theme,
                &mut app.dashboard,
            );
        }
        View::Attached(id) => {
            if let Some(session) = app.sessions.get(*id) {
                let label = app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == *id)
                    .map(|(_, w)| w.name.clone())
                    .unwrap_or_default();
                attached::resize_session(&session, area);
                attached::render(f, area, &session, &label, &app.theme);
            }
        }
    }
    if let Some(m) = &app.modal {
        modal::render(f, area, m, &app.theme);
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
                }
            }
        }
        CtEvent::Resize(_, _) => {}
        _ => {}
    }
    Ok(())
}

async fn handle_key_dashboard(app: &mut App, k: crossterm::event::KeyEvent) -> Result<()> {
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
                });
            }
            None => {}
        },
        (KeyCode::Char('n'), _) => {
            // Resolve target repo from the current selection. Falls back to the
            // first repo if nothing is selected (shouldn't normally happen).
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
    let mode = if crate::pty::session::has_prior_session(&ws.worktree_path) {
        crate::pty::session::SpawnMode::Continue {
            custom_instructions: custom,
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
    // Ctrl-a prefix handling.
    if app.ctrl_a_pending {
        app.ctrl_a_pending = false;
        match k.code {
            KeyCode::Char('d') => {
                app.view = View::Dashboard;
                return Ok(());
            }
            KeyCode::Char('a') => {
                let _ = session.writer.send(vec![0x01]).await;
                return Ok(());
            }
            _ => return Ok(()),
        }
    }
    if k.code == KeyCode::Char('a') && k.modifiers.contains(KeyModifiers::CONTROL) {
        app.ctrl_a_pending = true;
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

async fn handle_key_modal(app: &mut App, k: crossterm::event::KeyEvent) -> Result<()> {
    let modal = app.modal.clone().unwrap();
    match modal {
        Modal::NewWorkspace {
            repo_id,
            mut name_buffer,
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
                let result =
                    crate::workspace::create(&app.store, &repo, name.as_deref(), &base, |_| {})
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
                });
            }
            KeyCode::Char(c) => {
                name_buffer.push(c);
                app.modal = Some(Modal::NewWorkspace {
                    repo_id,
                    name_buffer,
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
    }
    Ok(())
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
                }
            }

            // 2) Workspace status — refresh the cache for this workspace.
            if let Ok(status) = crate::git::workspace_status(&path).await {
                let mut g = app.lock().await;
                g.workspace_status.insert(id, status);
            }

            // 3) Tail Claude Code session JSONL for events.
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
                    } = update;
                    let mut g = app.lock().await;
                    let evt = g.workspace_events.entry(id).or_default();
                    evt.file_path = Some(file);
                    evt.byte_offset = new_offset;
                    for (tu_id, tu_name, ts) in tool_use_starts {
                        evt.pending_tool_uses.insert(tu_id, (tu_name, ts));
                    }
                    for tu_id in tool_use_resolves {
                        evt.pending_tool_uses.remove(&tu_id);
                    }
                    for e in events {
                        crate::events::push_event(evt, e);
                    }
                }
            }
        }
    }
}
