use crate::config::usage_window::UsageWindow;
use crate::data::store::RepoId;
use crate::git::forge::BranchLifecycle;
use crate::ui::dashboard::status::Status;
use crate::ui::theme::Theme;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use std::collections::{HashMap, HashSet};

mod agents_panel;
mod archive;
mod process_list;
mod repo_settings;
mod updates_panel;
mod usage_picker;

// `render()` below dispatches the ArchiveRunning variant to this.
use archive::render_archive_steps;
// Panel renderers called from app::render via `crate::ui::modal::*`.
pub use agents_panel::render_agents_panel;
pub use process_list::render_process_list;
pub use repo_settings::render_repo_settings;
pub use updates_panel::{ordered_workspaces_for_panel, render_updates_panel};
pub use usage_picker::render_usage_window_picker;

/// Which phase of `workspace::archive_with_app` is currently running.
/// Used by `Modal::ArchiveRunning` to drive the per-step progress UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveStep {
    /// Phase 1: running the repo's archive script (if any).
    Script,
    /// Phase 2: `git worktree remove` — usually the slow one.
    RemoveWorktree,
    /// Phase 3: `git branch -D`.
    DeleteBranch,
    /// Phase 4: sqlite row + MCP entry cleanup.
    Cleanup,
}

#[derive(Debug, Clone)]
pub enum Modal {
    NewWorkspace {
        repo_id: RepoId,
        name_buffer: String,
        yolo: bool,
        agent: crate::pty::session::AgentKind,
    },
    ConfirmArchive {
        workspace_id: crate::data::store::WorkspaceId,
        name: String,
    },
    SetupRunning {
        cancel: tokio_util::sync::CancellationToken,
    },
    ArchiveRunning {
        step: ArchiveStep,
        /// Whether the repo has an archive script configured. Drives
        /// whether the Script row renders as in-progress/done or
        /// "(skipped)".
        script_present: bool,
    },
    Error {
        message: String,
    },
    UpdatesPanel {
        /// Index into the modal's ordered workspace list. Up/Down adjust
        /// it; Enter switches `app.view` to that workspace.
        selected: usize,
    },
    ProcessList {
        workspace_id: crate::data::store::WorkspaceId,
        selected: usize,
        /// `None` = list mode; `Some(buffer)` = the user is typing a command to run.
        input: Option<String>,
        /// Last launch result (success path or error), shown below the list.
        notice: Option<String>,
    },
    RepoSettings {
        repo_id: crate::data::store::RepoId,
        selected: usize,
    },
    AgentMissing {
        ws_id: crate::data::store::WorkspaceId,
        agent: crate::pty::session::AgentKind,
        binary: String,
    },
    AgentPicker {
        ws_id: crate::data::store::WorkspaceId,
        selected: usize,
        current: crate::pty::session::AgentKind,
    },
    AgentsPanel {
        workspace_id: crate::data::store::WorkspaceId,
        selected: usize, // index into AgentKind::ALL for the add-picker
    },
    UsageWindowPicker {
        /// Index into `UsageWindow::ALL` for the cursor selection. The current
        /// (applied) window is read separately from the store at render time.
        selected: usize,
    },
    /// Static reference card for the workspace-only actions
    /// (edit/term/diff/lazygit/chronox) — the ones that act only on a
    /// selected workspace. Carries no state — dismissed without side effects.
    WorkspaceActions,
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

/// Draw a centered, bordered modal box of size `w`×`h` (centered within
/// `area`) titled `title`: clears the region, paints the dim-styled border,
/// and returns the inner content area. Shared framing for the floating panel
/// renderers (updates, processes, repo settings, agents) so they look
/// identical; each caller lays its own body/footer split inside the returned
/// inner rect.
fn panel_frame<'a>(
    f: &mut Frame,
    area: Rect,
    w: u16,
    h: u16,
    title: impl Into<Line<'a>>,
    theme: &Theme,
) -> Rect {
    let rect = centered(area, w, h);
    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .style(theme.dim_style());
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    inner
}

pub fn render(f: &mut Frame, area: Rect, modal: &Modal, tick: u32, theme: &Theme) {
    // UpdatesPanel and ProcessList are rendered by their dedicated
    // helpers directly from `draw()` because they need live App state.
    // This function should never be called with those variants; guard
    // defensively.
    if matches!(
        modal,
        Modal::UpdatesPanel { .. }
            | Modal::ProcessList { .. }
            | Modal::RepoSettings { .. }
            | Modal::AgentsPanel { .. }
            | Modal::UsageWindowPicker { .. }
    ) {
        return;
    }
    let rect = centered(area, 60, 14);
    f.render_widget(Clear, rect);
    let (title, body) = match modal {
        Modal::NewWorkspace {
            name_buffer,
            yolo,
            agent,
            ..
        } => {
            let agent_label = agent.display_name();
            (
                if *yolo {
                    "new workspace (permissive)"
                } else {
                    "new workspace"
                },
                format!(
                    "name: {name_buffer}\nagent: {agent_label}  [tab] toggle\n\n[enter] create   [esc] cancel"
                ),
            )
        }
        Modal::ConfirmArchive { name, .. } => (
            "archive workspace",
            format!("archive '{name}'?\n\n[y] yes   [n]/[esc] cancel"),
        ),
        Modal::SetupRunning { .. } => {
            let frame = crate::ui::dashboard::spinner::frame(tick);
            let body = format!("  {frame} Creating workspace…\n\n  [esc] cancel",);
            ("new workspace", body)
        }
        Modal::ArchiveRunning {
            step,
            script_present,
        } => {
            let body = render_archive_steps(*step, *script_present, tick);
            ("archive workspace", body)
        }
        Modal::Error { message } => ("error", message.clone()),
        // UpdatesPanel is handled by the early-return above; this arm is
        // unreachable but required for exhaustiveness.
        Modal::UpdatesPanel { .. } => unreachable!("UpdatesPanel must not reach render()"),
        Modal::ProcessList { .. } => unreachable!("ProcessList must not reach render()"),
        Modal::RepoSettings { .. } => unreachable!("RepoSettings must not reach render()"),
        Modal::AgentsPanel { .. } => unreachable!("AgentsPanel must not reach render()"),
        Modal::UsageWindowPicker { .. } => {
            unreachable!("UsageWindowPicker must not reach render()")
        }
        Modal::AgentMissing { agent, binary, .. } => (
            "agent not installed",
            format!(
                "{name} is not installed.\n\n\
                 The `{binary}` binary was not found on PATH.\n\
                 Install it, then re-enter the workspace.\n\n\
                 s    switch agent for this workspace\n\
                 Esc  dismiss",
                name = capitalize_first(agent.display_name()),
                binary = binary,
            ),
        ),
        Modal::WorkspaceActions => (
            "workspace actions",
            "These apply to the selected workspace:\n\n  \
             e   edit        t   term\n  \
             v   diff        g   lazygit\n  \
             c   chronox\n\n  \
             ?/Esc  close"
                .to_string(),
        ),
        Modal::AgentPicker {
            selected, current, ..
        } => {
            let list = crate::pty::session::AgentKind::ALL
                .iter()
                .enumerate()
                .map(|(i, k)| {
                    let marker = if i == *selected { ">" } else { " " };
                    let current_tag = if *k == *current { "  (current)" } else { "" };
                    format!("{marker}  {name}{current_tag}", name = k.display_name())
                })
                .collect::<Vec<_>>()
                .join("\n");
            (
                "pick an agent",
                format!(
                    "Choose an agent for this workspace:\n\n{list}\n\n\
                     \u{2191}\u{2193} move   Enter confirm   Esc cancel"
                ),
            )
        }
    };
    let style = if matches!(modal, Modal::Error { .. }) {
        theme.err_style()
    } else {
        theme.header_style()
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

/// Uppercase only the first character of `s`. Used to render the agent
/// name in the AgentMissing modal as a proper sentence-start without
/// changing the canonical lowercase form returned by `AgentKind::display_name`.
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
