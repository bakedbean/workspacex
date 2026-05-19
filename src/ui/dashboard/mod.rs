//! Top-level dashboard render entry point. Owns `DashboardState` and
//! the public `DashboardInputs` type that the caller assembles in `app.rs`.

pub mod by_attention;
pub mod by_repo;
#[cfg(test)] pub(crate) mod fixture;
pub mod layout;
pub mod row;
pub mod sort;
pub mod sparkline;
pub mod spinner;
pub mod status;

use crate::app::SelectionTarget;
use crate::store::Repo;
use crate::ui::dashboard::by_attention::{AttentionData, FlatRow, QuietRepo};
use crate::ui::dashboard::by_repo::RepoView;
use crate::ui::dashboard::layout::GroupMode;
use crate::ui::dashboard::row::RowInputs;
use crate::ui::dashboard::sort::{default_fold, StatusCounts};
use crate::ui::dashboard::status::Status;
use crate::ui::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{List, ListState, Paragraph};
use std::collections::HashMap;

/// Per-workspace inputs the caller has already classified.
#[derive(Debug, Clone)]
pub struct WorkspaceItem<'a> {
    pub repo: &'a Repo,
    pub workspace_id: crate::store::WorkspaceId,
    pub status: Status,
    pub row: RowInputs,
}

/// What `app.rs` passes to `render()`. Replaces the old `Item` enum.
#[derive(Debug, Clone)]
pub struct DashboardInputs<'a> {
    pub repos: Vec<&'a Repo>,
    pub workspaces: Vec<WorkspaceItem<'a>>,
    pub activity: &'a [u32],
}

#[derive(Debug, Default)]
pub struct DashboardState {
    pub list_state: ListState,
    pub group_mode: GroupMode,
    /// Explicit user fold overrides; absent = use `default_fold(counts)`.
    pub folded: HashMap<u64, bool>,
    pub filter: Option<String>,
    pub selection: Option<SelectionTarget>,
}

impl Default for GroupMode {
    fn default() -> Self { GroupMode::Repo }
}

pub fn render(
    f: &mut Frame,
    area: Rect,
    inputs: &DashboardInputs<'_>,
    state: &mut DashboardState,
    tick: u32,
    theme: &Theme,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // top chrome
            Constraint::Length(1), // status strip
            Constraint::Length(1), // spacer
            Constraint::Min(0),    // main list
            Constraint::Length(1), // footer
        ])
        .split(area);
    let width = chunks[3].width as usize;

    let global_counts = StatusCounts::from_iter(inputs.workspaces.iter().map(|w| w.status));

    f.render_widget(
        Paragraph::new(layout::top_chrome(
            state.group_mode,
            inputs.repos.len(),
            inputs.workspaces.len(),
            chunks[0].width as usize,
            theme,
        )),
        chunks[0],
    );
    f.render_widget(
        Paragraph::new(layout::status_strip(global_counts, theme)),
        chunks[1],
    );

    let items = match state.group_mode {
        GroupMode::Repo => render_by_repo(inputs, state, tick, width, theme),
        GroupMode::Attention => render_by_attention(inputs, tick, width, theme),
    };
    let list = List::new(items).highlight_style(theme.selected_style());
    f.render_stateful_widget(list, chunks[3], &mut state.list_state);

    f.render_widget(
        Paragraph::new(layout::footer(
            inputs.activity,
            env!("CARGO_PKG_VERSION"),
            chunks[4].width as usize,
            theme,
        )),
        chunks[4],
    );
}

fn render_by_repo<'a>(
    inputs: &DashboardInputs<'a>,
    state: &mut DashboardState,
    tick: u32,
    width: usize,
    theme: &Theme,
) -> Vec<ratatui::widgets::ListItem<'static>> {
    let mut views: Vec<RepoView<'a>> = inputs
        .repos
        .iter()
        .map(|r| {
            let mut workspaces: Vec<RowInputs> = inputs
                .workspaces
                .iter()
                .filter(|w| w.repo.id == r.id)
                .map(|w| w.row.clone())
                .collect();
            workspaces.sort_by(|a, b| b.status.priority().cmp(&a.status.priority()));
            let counts = StatusCounts::from_iter(workspaces.iter().map(|w| w.status));
            let repo_id_u64 = r.id.0 as u64;
            let expanded = match state.folded.get(&repo_id_u64).copied() {
                Some(explicit) => !explicit,
                None => !default_fold(counts),
            };
            RepoView {
                id: repo_id_u64,
                name: &r.name,
                path: r.path.to_str().unwrap_or(""),
                counts,
                expanded,
                workspaces,
            }
        })
        .collect();
    by_repo::order_repos(&mut views);
    let items = by_repo::render_list(&views, tick, width, theme);
    let _ = state.selection;
    items
}

fn render_by_attention<'a>(
    inputs: &DashboardInputs<'a>,
    tick: u32,
    width: usize,
    theme: &Theme,
) -> Vec<ratatui::widgets::ListItem<'static>> {
    let mut rows: Vec<FlatRow> = inputs
        .workspaces
        .iter()
        .map(|w| FlatRow {
            repo_name: w.repo.name.clone(),
            row: w.row.clone(),
        })
        .collect();
    rows.sort_by(|a, b| b.row.status.priority().cmp(&a.row.status.priority()));
    let mut quiet: Vec<QuietRepo> = Vec::new();
    for r in &inputs.repos {
        let repo_rows: Vec<&WorkspaceItem<'_>> = inputs
            .workspaces
            .iter()
            .filter(|w| w.repo.id == r.id)
            .collect();
        let count = repo_rows.len();
        let all_idle = !repo_rows.is_empty()
            && repo_rows.iter().all(|w| matches!(w.status, Status::Idle));
        if count == 0 || all_idle {
            quiet.push(QuietRepo {
                name: r.name.clone(),
                path: r.path.to_string_lossy().into_owned(),
                workspace_count: count,
                all_idle,
            });
        }
    }
    let data = AttentionData {
        needs_attention: rows.iter().filter(|r| matches!(r.row.status,
            Status::Question | Status::Stalled | Status::Waiting)).cloned().collect(),
        working: rows.iter().filter(|r| matches!(r.row.status, Status::Thinking)).cloned().collect(),
        recent: rows.iter().filter(|r| matches!(r.row.status, Status::Complete)).cloned().collect(),
        idle: rows.iter().filter(|r| matches!(r.row.status, Status::Idle))
            .filter(|r| {
                // Idle rows from quiet repos already appear under QUIET REPOS.
                !quiet.iter().any(|q| q.name == r.repo_name)
            })
            .cloned().collect(),
        quiet_repos: quiet,
    };
    by_attention::render_list(&data, tick, width, theme)
}

#[cfg(test)]
mod tests;
