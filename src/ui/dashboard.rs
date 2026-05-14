use crate::app::SelectionTarget;
use crate::store::{Repo, SetupStatus, Workspace, WorkspaceState};
use crate::ui::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

#[derive(Debug, Clone)]
pub enum Item<'a> {
    Header {
        repo: &'a Repo,
    },
    Workspace {
        repo: &'a Repo,
        workspace: &'a Workspace,
        session_running: bool,
        seconds_since_activity: Option<u64>,
        has_prior_session: bool,
    },
    EmptyHint,
    Spacer,
}

#[derive(Default)]
pub struct DashboardState {
    pub selected: usize,
    pub list_state: ListState,
}

pub fn render(
    f: &mut Frame,
    area: Rect,
    items: &[Item],
    selected: Option<SelectionTarget>,
    state: &mut DashboardState,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    let header_text = Paragraph::new("wsx — Workspaces").style(theme::header());
    f.render_widget(header_text, chunks[0]);

    let mut selected_idx: Option<usize> = None;
    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(idx, item)| match item {
            Item::Header { repo } => {
                if let Some(SelectionTarget::Repo(id)) = selected
                    && id == repo.id
                {
                    selected_idx = Some(idx);
                }
                let line = format!("▌ {}    {}", repo.name, repo.path.display());
                ListItem::new(line).style(theme::header())
            }
            Item::Workspace {
                repo: _,
                workspace,
                session_running,
                seconds_since_activity,
                has_prior_session,
            } => {
                if let Some(SelectionTarget::Workspace(id)) = selected
                    && id == workspace.id
                {
                    selected_idx = Some(idx);
                }
                let dot = match (*session_running, &workspace.state, *has_prior_session) {
                    (true, _, _) => "●",
                    (false, WorkspaceState::Failed, _) => "✕",
                    (false, _, true) => "↻",
                    _ => "○",
                };
                let setup_badge = match workspace.setup_status {
                    SetupStatus::Ok | SetupStatus::Skipped | SetupStatus::NotRun => "",
                    SetupStatus::Failed => " [setup-failed]",
                };
                let activity = match (*seconds_since_activity, *has_prior_session) {
                    (Some(s), _) if s < 2 => "active",
                    (Some(s), _) if s < 30 => "idle",
                    (Some(_), _) => "waiting",
                    (None, true) => "resumable",
                    (None, false) => "off",
                };
                let line = format!(
                    "  {dot} {name}  [{branch}]  {activity}{setup_badge}",
                    name = workspace.name,
                    branch = workspace.branch,
                );
                ListItem::new(line)
            }
            Item::EmptyHint => {
                ListItem::new("  (no workspaces — press n to create one)").style(theme::dim())
            }
            Item::Spacer => ListItem::new(""),
        })
        .collect();

    state.list_state.select(selected_idx);
    let list = List::new(list_items)
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

    fn repo(id: i64, name: &str) -> Repo {
        Repo {
            id: RepoId(id),
            name: name.into(),
            path: PathBuf::from(format!("/repos/{name}")),
            branch_prefix: "".into(),
            custom_instructions: None,
            created_at: 0,
        }
    }

    fn workspace(id: i64, repo_id: i64, name: &str, branch: &str) -> Workspace {
        Workspace {
            id: WorkspaceId(id),
            repo_id: RepoId(repo_id),
            name: name.into(),
            branch: branch.into(),
            worktree_path: PathBuf::from(format!("/w/{name}")),
            state: WorkspaceState::Ready,
            setup_status: SetupStatus::Ok,
            created_at: 0,
        }
    }

    fn dump(term: &Terminal<TestBackend>, w: u16, h: u16) -> String {
        let buf = term.backend().buffer();
        let mut s = String::new();
        for y in 0..h {
            let line: String = (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect();
            s.push_str(line.trim_end());
            s.push('\n');
        }
        s
    }

    #[test]
    fn renders_repo_header_with_indented_workspace() {
        let mut term = Terminal::new(TestBackend::new(80, 8)).unwrap();
        let r = repo(1, "demo");
        let w = workspace(1, 1, "alpha", "alpha");
        let items = vec![
            Item::Header { repo: &r },
            Item::Workspace {
                repo: &r,
                workspace: &w,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| {
            render(
                f,
                f.area(),
                &items,
                Some(SelectionTarget::Workspace(WorkspaceId(1))),
                &mut state,
            )
        })
        .unwrap();
        let text = dump(&term, 80, 8);
        assert!(text.contains("▌ demo"), "missing header: {text}");
        assert!(text.contains("alpha"), "missing workspace name: {text}");
        assert!(text.contains("active"), "missing activity column: {text}");
    }

    #[test]
    fn renders_empty_repo_hint() {
        let mut term = Terminal::new(TestBackend::new(80, 8)).unwrap();
        let r = repo(1, "empty");
        let items = vec![Item::Header { repo: &r }, Item::EmptyHint];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, &mut state))
            .unwrap();
        let text = dump(&term, 80, 8);
        assert!(text.contains("▌ empty"));
        assert!(text.contains("press n to create"));
    }

    #[test]
    fn renders_multiple_repos_grouped() {
        let mut term = Terminal::new(TestBackend::new(80, 15)).unwrap();
        let r1 = repo(1, "first");
        let r2 = repo(2, "second");
        let w1 = workspace(1, 1, "alpha", "alpha");
        let w2 = workspace(2, 2, "beta", "beta");
        let items = vec![
            Item::Header { repo: &r1 },
            Item::Workspace {
                repo: &r1,
                workspace: &w1,
                session_running: false,
                seconds_since_activity: None,
                has_prior_session: false,
            },
            Item::Spacer,
            Item::Header { repo: &r2 },
            Item::Workspace {
                repo: &r2,
                workspace: &w2,
                session_running: false,
                seconds_since_activity: None,
                has_prior_session: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, &mut state))
            .unwrap();
        let text = dump(&term, 80, 15);
        let first_pos = text.find("first").expect("first repo header");
        let alpha_pos = text.find("alpha").expect("alpha workspace");
        let second_pos = text.find("second").expect("second repo header");
        let beta_pos = text.find("beta").expect("beta workspace");
        assert!(
            first_pos < alpha_pos && alpha_pos < second_pos && second_pos < beta_pos,
            "ordering wrong:\n{text}"
        );
    }
}
