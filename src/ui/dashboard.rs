use crate::store::{Repo, SetupStatus, Workspace, WorkspaceState};
use crate::ui::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

#[derive(Debug, Clone)]
pub struct Row<'a> {
    pub repo: &'a Repo,
    pub workspace: &'a Workspace,
    pub session_running: bool,
    pub seconds_since_activity: Option<u64>,
    pub has_prior_session: bool,
}

#[derive(Default)]
pub struct DashboardState {
    pub selected: usize,
    pub list_state: ListState,
}

pub fn render(f: &mut Frame, area: Rect, rows: &[Row], state: &mut DashboardState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    let header = Paragraph::new("wsx — Workspaces").style(theme::header());
    f.render_widget(header, chunks[0]);

    let items: Vec<ListItem> = rows
        .iter()
        .map(|r| {
            let dot = match (r.session_running, &r.workspace.state, r.has_prior_session) {
                (true, _, _) => "●",
                (false, WorkspaceState::Failed, _) => "✕",
                (false, _, true) => "↻",
                _ => "○",
            };
            let setup_badge = match r.workspace.setup_status {
                SetupStatus::Ok | SetupStatus::Skipped | SetupStatus::NotRun => "",
                SetupStatus::Failed => " [setup-failed]",
            };
            let activity = match (r.seconds_since_activity, r.has_prior_session) {
                (Some(s), _) if s < 2 => "active",
                (Some(s), _) if s < 30 => "idle",
                (Some(_), _) => "waiting",
                (None, true) => "resumable",
                (None, false) => "off",
            };
            let line = format!(
                "{dot} {repo}/{name}  [{branch}]  {activity}{setup_badge}",
                repo = r.repo.name,
                name = r.workspace.name,
                branch = r.workspace.branch,
            );
            ListItem::new(line)
        })
        .collect();

    state
        .list_state
        .select(Some(state.selected.min(items.len().saturating_sub(1))));
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL))
        .highlight_style(theme::selected());
    f.render_stateful_widget(list, chunks[1], &mut state.list_state);

    let footer =
        Paragraph::new("[enter] attach   [n] new   [d] archive   [q] quit").style(theme::dim());
    f.render_widget(footer, chunks[2]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{RepoId, WorkspaceId};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    #[test]
    fn renders_one_row_with_active_status() {
        let mut term = Terminal::new(TestBackend::new(60, 5)).unwrap();
        let repo = Repo {
            id: RepoId(1),
            name: "demo".into(),
            path: PathBuf::from("/r"),
            branch_prefix: "".into(),
            custom_instructions: None,
            created_at: 0,
        };
        let ws = Workspace {
            id: WorkspaceId(1),
            repo_id: RepoId(1),
            name: "alpha".into(),
            branch: "alpha".into(),
            worktree_path: PathBuf::from("/w"),
            state: WorkspaceState::Ready,
            setup_status: SetupStatus::Ok,
            created_at: 0,
        };
        let rows = vec![Row {
            repo: &repo,
            workspace: &ws,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
        }];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &rows, &mut state))
            .unwrap();
        let buf = term.backend().buffer();
        let line1: String = (0..60).map(|x| buf[(x, 2)].symbol().to_string()).collect();
        assert!(line1.contains("demo/alpha"));
        assert!(line1.contains("active"));
    }
}
