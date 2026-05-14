use crate::error::Result;
use crate::pty::session::SessionManager;
use crate::store::{Repo, Store, Workspace, WorkspaceId};
use crate::ui::View;
use crate::ui::dashboard::DashboardState;
use crate::ui::modal::Modal;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

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
            store, sessions: SessionManager::new(),
            view: View::Dashboard, modal: None, dashboard: DashboardState::default(),
            repos: Vec::new(), workspaces: Vec::new(), worktree_base,
            ctrl_a_pending: false, quit: false,
        };
        // Sweep stale Pending rows from previous runs.
        let _ = app.store.sweep_stale_pending(std::time::Duration::from_secs(300));
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
