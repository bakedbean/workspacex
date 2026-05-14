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

pub struct App {
    pub store: Store,
    pub sessions: SessionManager,
    pub view: View,
    pub modal: Option<Modal>,
    pub dashboard: DashboardState,
    pub repos: Vec<Repo>,
    pub workspaces: Vec<(crate::store::RepoId, Workspace)>,
    pub worktree_base: PathBuf,
    pub ctrl_a_pending: bool,
    pub quit: bool,
}

impl App {
    pub fn new(store: Store, worktree_base: PathBuf) -> Result<Self> {
        let mut app = Self {
            store,
            sessions: SessionManager::new(),
            view: View::Dashboard,
            modal: None,
            dashboard: DashboardState::default(),
            repos: Vec::new(),
            workspaces: Vec::new(),
            worktree_base,
            ctrl_a_pending: false,
            quit: false,
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
        Ok(())
    }

    pub fn selected_workspace(&self) -> Option<(&Repo, &Workspace)> {
        let (rid, ws) = self.workspaces.get(self.dashboard.selected)?;
        let repo = self.repos.iter().find(|r| &r.id == rid)?;
        Some((repo, ws))
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
            let rows: Vec<dashboard::Row> = app
                .workspaces
                .iter()
                .map(|(rid, ws)| {
                    let repo = app.repos.iter().find(|r| &r.id == rid).unwrap();
                    let session = app.sessions.get(ws.id);
                    let running = session.as_ref().is_some_and(|s| {
                        matches!(
                            *s.status.read().unwrap(),
                            crate::pty::session::SessionStatus::Running { .. }
                        )
                    });
                    let secs = session.as_ref().map(|s| {
                        let last = s.activity_ms.load(std::sync::atomic::Ordering::Relaxed);
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);
                        now.saturating_sub(last) / 1000
                    });
                    dashboard::Row {
                        repo,
                        workspace: ws,
                        session_running: running,
                        seconds_since_activity: secs,
                    }
                })
                .collect();
            dashboard::render(f, area, &rows, &mut app.dashboard);
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
                attached::render(f, area, &session, &label);
            }
        }
    }
    if let Some(m) = &app.modal {
        modal::render(f, area, m);
    }
}

#[doc(hidden)]
pub fn draw_for_test(f: &mut ratatui::Frame, app: &mut App) {
    draw(f, app);
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
        (KeyCode::Up, _) => app.dashboard.selected = app.dashboard.selected.saturating_sub(1),
        (KeyCode::Down, _) => {
            let max = app.workspaces.len().saturating_sub(1);
            app.dashboard.selected = (app.dashboard.selected + 1).min(max);
        }
        (KeyCode::Enter, _) => {
            // Copy out of the immutable borrow before mutating sessions/view.
            let info = app
                .selected_workspace()
                .map(|(_, ws)| (ws.id, ws.worktree_path.clone()));
            if let Some((id, path)) = info {
                let _ = app.sessions.spawn(id, &path, 80, 24)?;
                app.view = View::Attached(id);
            }
        }
        (KeyCode::Char('n'), _) => {
            let first_repo_id = app.repos.first().map(|r| r.id);
            if let Some(id) = first_repo_id {
                app.modal = Some(Modal::NewWorkspace {
                    repo_id: id,
                    name_buffer: String::new(),
                });
            }
        }
        (KeyCode::Char('d'), _) => {
            let info = app
                .selected_workspace()
                .map(|(_, ws)| (ws.id, ws.name.clone()));
            if let Some((id, name)) = info {
                app.modal = Some(Modal::ConfirmArchive {
                    workspace_id: id,
                    name,
                });
            }
        }
        _ => {}
    }
    Ok(())
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
