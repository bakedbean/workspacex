use crate::store::RepoId;
use crate::ui::theme;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

#[derive(Debug, Clone)]
pub enum Modal {
    NewWorkspace {
        repo_id: RepoId,
        name_buffer: String,
    },
    ConfirmArchive {
        workspace_id: crate::store::WorkspaceId,
        name: String,
    },
    SetupRunning {
        log: Vec<String>,
    },
    Error {
        message: String,
    },
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let popup = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(h),
            Constraint::Min(0),
        ])
        .split(area)[1];
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(w),
            Constraint::Min(0),
        ])
        .split(popup)[1]
}

pub fn render(f: &mut Frame, area: Rect, modal: &Modal) {
    let rect = centered(area, 60, 12);
    f.render_widget(Clear, rect);
    let (title, body) = match modal {
        Modal::NewWorkspace { name_buffer, .. } => (
            "new workspace",
            format!("name: {name_buffer}\n\n[enter] create   [esc] cancel"),
        ),
        Modal::ConfirmArchive { name, .. } => (
            "archive workspace",
            format!("archive '{name}'?\n\n[y] yes   [n]/[esc] cancel"),
        ),
        Modal::SetupRunning { log } => {
            let last: Vec<String> = log.iter().rev().take(8).cloned().collect();
            let body = last.into_iter().rev().collect::<Vec<_>>().join("\n");
            ("setup running", body)
        }
        Modal::Error { message } => ("error", message.clone()),
    };
    let style = if matches!(modal, Modal::Error { .. }) {
        theme::err()
    } else {
        theme::header()
    };
    let para = Paragraph::new(body)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_alignment(Alignment::Left),
        )
        .style(style);
    f.render_widget(para, rect);
}
