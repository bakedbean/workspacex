use crate::store::RepoId;
use crate::ui::theme::Theme;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub enum Modal {
    NewWorkspace {
        repo_id: RepoId,
        name_buffer: String,
        yolo: bool,
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
    UpdatesPanel {
        /// Index into the modal's ordered workspace list. Up/Down adjust
        /// it; Enter switches `app.view` to that workspace.
        selected: usize,
    },
    ProcessList {
        workspace_id: crate::store::WorkspaceId,
        selected: usize,
    },
    RepoSettings {
        repo_id: crate::store::RepoId,
        selected: usize,
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

pub fn render(f: &mut Frame, area: Rect, modal: &Modal, theme: &Theme) {
    // UpdatesPanel and ProcessList are rendered by their dedicated
    // helpers directly from `draw()` because they need live App state.
    // This function should never be called with those variants; guard
    // defensively.
    if matches!(
        modal,
        Modal::UpdatesPanel { .. } | Modal::ProcessList { .. } | Modal::RepoSettings { .. }
    ) {
        return;
    }
    let rect = centered(area, 60, 12);
    f.render_widget(Clear, rect);
    let (title, body) = match modal {
        Modal::NewWorkspace {
            name_buffer, yolo, ..
        } => (
            if *yolo {
                "new workspace (permissive)"
            } else {
                "new workspace"
            },
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
        // UpdatesPanel is handled by the early-return above; this arm is
        // unreachable but required for exhaustiveness.
        Modal::UpdatesPanel { .. } => unreachable!("UpdatesPanel must not reach render()"),
        Modal::ProcessList { .. } => unreachable!("ProcessList must not reach render()"),
        Modal::RepoSettings { .. } => unreachable!("RepoSettings must not reach render()"),
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

/// Compute the order in which workspaces appear in the updates panel.
/// Returns workspace IDs in the same order the renderer walks them —
/// grouped by repo (in App's repo order), sorted within each repo by
/// (attention, failed, activity_rank, recency).
///
/// Used by both the renderer (to draw rows) and the key handler (to map
/// the selected index back to a workspace id).
pub fn ordered_workspaces_for_panel(
    repos: &[crate::store::Repo],
    workspaces: &[(RepoId, crate::store::Workspace)],
    events: &HashMap<crate::store::WorkspaceId, crate::events::WorkspaceEvents>,
    activity: &HashMap<crate::store::WorkspaceId, crate::ui::updates_bar::ActivityState>,
    needs_attention: &HashSet<crate::store::WorkspaceId>,
) -> Vec<crate::store::WorkspaceId> {
    let mut out = Vec::new();
    for repo in repos {
        let mut ws_for_repo: Vec<&crate::store::Workspace> = workspaces
            .iter()
            .filter(|(rid, _)| *rid == repo.id)
            .map(|(_, w)| w)
            .collect();
        ws_for_repo.sort_by_key(|w| sort_key(w, events, activity, needs_attention));
        out.extend(ws_for_repo.into_iter().map(|w| w.id));
    }
    out
}

fn sort_key(
    w: &crate::store::Workspace,
    events: &HashMap<crate::store::WorkspaceId, crate::events::WorkspaceEvents>,
    activity: &HashMap<crate::store::WorkspaceId, crate::ui::updates_bar::ActivityState>,
    needs_attention: &HashSet<crate::store::WorkspaceId>,
) -> (u8, u8, u8, i64) {
    let attention = if needs_attention.contains(&w.id) {
        0
    } else {
        1
    };
    let activity_rank = match activity.get(&w.id).copied() {
        Some(crate::ui::updates_bar::ActivityState::Awaiting)
        | Some(crate::ui::updates_bar::ActivityState::AwaitingAnswer)
        | Some(crate::ui::updates_bar::ActivityState::Complete)
        | Some(crate::ui::updates_bar::ActivityState::Stalled)
        | Some(crate::ui::updates_bar::ActivityState::Waiting) => 0,
        Some(crate::ui::updates_bar::ActivityState::Active)
        | Some(crate::ui::updates_bar::ActivityState::Idle) => 1,
        Some(crate::ui::updates_bar::ActivityState::Off) => 2,
        None => 3,
    };
    let failed = if w.state == crate::store::WorkspaceState::Failed {
        1
    } else {
        0
    };
    let recency = -events
        .get(&w.id)
        .and_then(|e| e.latest.as_ref().map(|s| s.timestamp_ms))
        .unwrap_or(0);
    (attention, failed, activity_rank, recency)
}

/// Render the floating workspace-updates panel. Reads live App state via
/// borrowed slices so the panel updates on every render tick.
#[allow(clippy::too_many_arguments)]
pub fn render_updates_panel(
    f: &mut Frame,
    area: Rect,
    repos: &[crate::store::Repo],
    workspaces: &[(RepoId, crate::store::Workspace)],
    events: &HashMap<crate::store::WorkspaceId, crate::events::WorkspaceEvents>,
    activity: &HashMap<crate::store::WorkspaceId, crate::ui::updates_bar::ActivityState>,
    needs_attention: &HashSet<crate::store::WorkspaceId>,
    awaiting: &HashMap<crate::store::WorkspaceId, (String, i64)>,
    selected: usize,
    now_ms: i64,
    theme: &Theme,
) {
    // Sizing: ~80 cols wide, ~25 rows tall, but never larger than the area.
    let w = area.width.clamp(20, 80);
    let h = area.height.clamp(8, 25);
    let rect = centered(area, w, h);
    f.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Workspace updates ")
        .style(theme.dim_style());
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let body_area = chunks[0];
    let footer_area = chunks[1];

    let order = ordered_workspaces_for_panel(repos, workspaces, events, activity, needs_attention);
    // workspace_id -> position in `order` so we can match against `selected`.
    let pos_of: HashMap<crate::store::WorkspaceId, usize> =
        order.iter().enumerate().map(|(i, id)| (*id, i)).collect();

    let mut lines: Vec<Line> = Vec::new();
    for repo in repos {
        lines.push(Line::from(Span::styled(
            repo.name.clone(),
            theme.header_style(),
        )));
        let ws_for_repo: Vec<&crate::store::Workspace> = workspaces
            .iter()
            .filter(|(rid, _)| *rid == repo.id)
            .map(|(_, w)| w)
            .filter(|w| pos_of.contains_key(&w.id))
            .collect();
        // Already pre-sorted in `order`; preserve that ordering here too.
        let mut ws_sorted = ws_for_repo;
        ws_sorted.sort_by_key(|w| pos_of.get(&w.id).copied().unwrap_or(usize::MAX));
        if ws_sorted.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (no workspaces)".to_string(),
                theme.dim_style(),
            )));
        } else {
            for w in ws_sorted {
                let is_selected = pos_of.get(&w.id).copied() == Some(selected);
                lines.push(workspace_row(
                    w,
                    events.get(&w.id),
                    activity.get(&w.id).copied(),
                    needs_attention.contains(&w.id),
                    awaiting.get(&w.id),
                    is_selected,
                    now_ms,
                    theme,
                ));
            }
        }
        lines.push(Line::from(""));
    }
    if repos.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no repos)".to_string(),
            theme.dim_style(),
        )));
    }

    f.render_widget(Paragraph::new(lines).style(theme.dim_style()), body_area);
    f.render_widget(
        Paragraph::new(
            "[\u{2191}/\u{2193}] move   [enter] switch   [v] vsplit   [s] hsplit   [esc] close",
        )
        .style(theme.dim_style()),
        footer_area,
    );
}

#[allow(clippy::too_many_arguments)]
fn workspace_row<'a>(
    w: &'a crate::store::Workspace,
    events: Option<&'a crate::events::WorkspaceEvents>,
    activity: Option<crate::ui::updates_bar::ActivityState>,
    needs_attention: bool,
    awaiting: Option<&'a (String, i64)>,
    is_selected: bool,
    now_ms: i64,
    theme: &Theme,
) -> Line<'a> {
    use crate::ui::updates_bar::{ActivityState, format_age};
    let glyph = if w.state == crate::store::WorkspaceState::Failed {
        '✕'
    } else if needs_attention {
        match activity {
            Some(ActivityState::AwaitingAnswer) => '?',
            Some(ActivityState::Complete) => '\u{2713}', // ✓ CHECK MARK
            _ => '⚠',
        }
    } else {
        match activity {
            Some(ActivityState::Active) | Some(ActivityState::Idle) => '●',
            Some(ActivityState::AwaitingAnswer) => '?',
            Some(ActivityState::Complete) => '\u{2713}',
            Some(ActivityState::Awaiting)
            | Some(ActivityState::Stalled)
            | Some(ActivityState::Waiting) => '⚠',
            Some(ActivityState::Off) | None => {
                if events.and_then(|e| e.latest.as_ref()).is_some() {
                    '↻'
                } else {
                    '○'
                }
            }
        }
    };
    let (status_text, age_anchor_ms) = if let Some((tool, ts)) = awaiting {
        (format!("awaiting permission: {tool}"), Some(*ts))
    } else if needs_attention {
        let label = match activity {
            Some(ActivityState::AwaitingAnswer) => "question",
            Some(ActivityState::Complete) => "complete",
            Some(ActivityState::Stalled) => "stalled",
            _ => "waiting",
        };
        (
            label.to_string(),
            events.and_then(|e| e.latest.as_ref().map(|s| s.timestamp_ms)),
        )
    } else if matches!(
        activity,
        Some(ActivityState::Active) | Some(ActivityState::Idle)
    ) {
        let text = events
            .and_then(|e| e.latest.as_ref().map(|s| s.display.clone()))
            .unwrap_or_else(|| "active".to_string());
        let ts = events.and_then(|e| e.latest.as_ref().map(|s| s.timestamp_ms));
        (text, ts)
    } else if w.state == crate::store::WorkspaceState::Failed {
        ("failed".to_string(), None)
    } else if events.and_then(|e| e.latest.as_ref()).is_some() {
        ("resumable".to_string(), None)
    } else {
        ("no session".to_string(), None)
    };
    let age = age_anchor_ms.map(|t| format_age(now_ms.saturating_sub(t)));
    let suffix = match age {
        Some(a) => format!(" ({a})"),
        None => String::new(),
    };
    let body = format!("  {glyph} {:<20} {}{}", w.name, status_text, suffix);
    if is_selected {
        Line::from(Span::styled(body, theme.selected_style()))
    } else {
        Line::from(body)
    }
}

/// Render the floating process-list modal. Reads live App state via
/// borrowed slices so the modal updates on every render tick.
pub fn render_process_list(
    f: &mut Frame,
    area: Rect,
    workspace_name: &str,
    procs: &[crate::proc::ProcInfo],
    selected: usize,
    theme: &Theme,
) {
    let w = area.width.clamp(20, 80);
    let h = area.height.clamp(8, 25);
    let rect = centered(area, w, h);
    f.render_widget(Clear, rect);

    let title = format!(" Processes — {workspace_name} ");
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .style(theme.dim_style());
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let body_area = chunks[0];
    let footer_area = chunks[1];

    if procs.is_empty() {
        f.render_widget(
            Paragraph::new("(no tracked processes)").style(theme.dim_style()),
            body_area,
        );
    } else {
        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(Span::styled(
            format!("  {:<7} {:<20} {}", "PID", "COMMAND", "CWD"),
            theme.header_style(),
        )));
        for (i, p) in procs.iter().enumerate() {
            let body = format!(
                "  {:<7} {:<20} {}",
                p.pid,
                truncate(&p.command, 20),
                p.cwd.display()
            );
            if i == selected {
                lines.push(Line::from(Span::styled(body, theme.selected_style())));
            } else {
                lines.push(Line::from(body));
            }
        }
        f.render_widget(Paragraph::new(lines), body_area);
    }
    f.render_widget(
        Paragraph::new("[\u{2191}/\u{2193}] move   [k] term   [K] kill   [esc] close")
            .style(theme.dim_style()),
        footer_area,
    );
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('\u{2026}');
        out
    }
}

/// Render the floating repo-settings modal. Live state — reads
/// current values from the borrowed `Repo` struct.
pub fn render_repo_settings(
    f: &mut Frame,
    area: Rect,
    repo_name: &str,
    repo: &crate::store::Repo,
    selected: usize,
    theme: &Theme,
) {
    let w = area.width.clamp(40, 90);
    let h = area.height.clamp(8, 16);
    let rect = centered(area, w, h);
    f.render_widget(Clear, rect);

    let title = format!(" Repo settings — {repo_name} ");
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .style(theme.dim_style());
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let body_area = chunks[0];
    let footer_area = chunks[1];

    let rows: [(crate::app::RepoSettingField, Option<&str>); 6] = [
        (
            crate::app::RepoSettingField::BranchPrefix,
            if repo.branch_prefix.is_empty() {
                None
            } else {
                Some(repo.branch_prefix.as_str())
            },
        ),
        (
            crate::app::RepoSettingField::CustomInstructions,
            repo.custom_instructions.as_deref(),
        ),
        (
            crate::app::RepoSettingField::SetupScript,
            repo.setup_script.as_deref(),
        ),
        (
            crate::app::RepoSettingField::ArchiveScript,
            repo.archive_script.as_deref(),
        ),
        (
            crate::app::RepoSettingField::PinnedCommands,
            repo.pinned_commands.as_deref(),
        ),
        (
            crate::app::RepoSettingField::RelatedRepos,
            repo.related_repos.as_deref(),
        ),
    ];

    let mut lines: Vec<Line> = Vec::new();
    for (i, (field, value)) in rows.iter().enumerate() {
        let label_pad = 22; // width of the longest label + breathing room
        let preview = value
            .map(|v| preview_value(v, 60))
            .unwrap_or_else(|| "(unset)".to_string());
        let body = format!("  {:<width$} {}", field.label(), preview, width = label_pad);
        let style = if value.is_none() {
            theme.dim_style()
        } else {
            Style::default()
        };
        if i == selected {
            lines.push(Line::from(Span::styled(body, theme.selected_style())));
        } else {
            lines.push(Line::from(Span::styled(body, style)));
        }
    }
    f.render_widget(Paragraph::new(lines), body_area);

    f.render_widget(
        Paragraph::new("[\u{2191}/\u{2193}] move   [enter] edit   [d] clear   [esc] close")
            .style(theme.dim_style()),
        footer_area,
    );
}

/// First non-empty line, trimmed and truncated. Used by render_repo_settings.
fn preview_value(s: &str, max: usize) -> String {
    let first_line = s.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let trimmed = first_line.trim();
    if trimmed.chars().count() <= max {
        trimmed.to_string()
    } else {
        let mut out: String = trimmed.chars().take(max.saturating_sub(1)).collect();
        out.push('\u{2026}');
        out
    }
}

#[cfg(test)]
mod preview_tests {
    use super::*;

    #[test]
    fn preview_value_returns_first_nonempty_line() {
        assert_eq!(preview_value("\n  \nhello\nworld", 60), "hello");
    }

    #[test]
    fn preview_value_truncates_with_ellipsis() {
        let long = "x".repeat(100);
        let out = preview_value(&long, 60);
        assert!(out.ends_with('\u{2026}'));
        assert_eq!(out.chars().count(), 60);
    }

    #[test]
    fn preview_value_empty_returns_empty() {
        assert_eq!(preview_value("", 60), "");
    }
}

#[cfg(test)]
mod workspace_row_tests {
    use super::*;
    use crate::store::{Workspace, WorkspaceId, WorkspaceState};
    use crate::ui::updates_bar::ActivityState;
    use std::path::PathBuf;

    fn fixture_workspace(name: &str) -> Workspace {
        Workspace {
            id: WorkspaceId(1),
            repo_id: crate::store::RepoId(1),
            name: name.to_string(),
            branch: "main".to_string(),
            worktree_path: PathBuf::from("/tmp/ws"),
            state: WorkspaceState::Ready,
            setup_status: crate::store::SetupStatus::Ok,
            created_at: 0,
            yolo: false,
        }
    }

    /// Concatenate every span's content into a single String so tests can
    /// match against the rendered text regardless of styling.
    fn line_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn workspace_row_uses_question_glyph_for_awaiting_answer() {
        let theme = Theme::default_theme();
        let w = fixture_workspace("alpha");
        let line = workspace_row(
            &w,
            None,
            Some(ActivityState::AwaitingAnswer),
            true,
            None,
            false,
            10_000,
            &theme,
        );
        let body = line_text(&line);
        assert!(body.contains("? "), "expected '?' glyph in: {body}");
        assert!(
            body.contains("question"),
            "expected 'question' status text in: {body}"
        );
    }

    #[test]
    fn workspace_row_uses_check_glyph_for_complete() {
        let theme = Theme::default_theme();
        let w = fixture_workspace("alpha");
        let line = workspace_row(
            &w,
            None,
            Some(ActivityState::Complete),
            true,
            None,
            false,
            10_000,
            &theme,
        );
        let body = line_text(&line);
        assert!(
            body.contains('\u{2713}'),
            "expected '✓' glyph in: {body}"
        );
        assert!(
            body.contains("complete"),
            "expected 'complete' status text in: {body}"
        );
    }

    #[test]
    fn workspace_row_uses_warning_glyph_for_awaiting_permission() {
        let theme = Theme::default_theme();
        let w = fixture_workspace("alpha");
        let line = workspace_row(
            &w,
            None,
            Some(ActivityState::Awaiting),
            true,
            None,
            false,
            10_000,
            &theme,
        );
        let body = line_text(&line);
        assert!(body.contains('⚠'), "expected '⚠' glyph in: {body}");
    }
}
