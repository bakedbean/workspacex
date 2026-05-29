use crate::git::forge::BranchLifecycle;
use crate::data::store::RepoId;
use crate::ui::dashboard::status::Status;
use crate::ui::theme::Theme;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use std::collections::{HashMap, HashSet};

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

pub fn render(f: &mut Frame, area: Rect, modal: &Modal, tick: u32, theme: &Theme) {
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

/// Render the 4-line body of the `ArchiveRunning` modal.
///
/// Each line is one phase of `workspace::archive_with_app`. The
/// `script_present` flag overrides the Script row's marker to
/// "— (skipped)" regardless of `step`, so a no-script repo never
/// shows the Script row spinning during the brief window where
/// `step == Script` and `run_archive` is returning `Skipped`.
fn render_archive_steps(step: ArchiveStep, script_present: bool, tick: u32) -> String {
    let spinner = crate::ui::dashboard::spinner::frame(tick);

    // Per-row marker: '✓' done, spinner in-progress, '·' pending.
    // The script row gets a special '(skipped)' rendering when there
    // is no script configured.
    let script_line = if !script_present {
        "  — Archive script (skipped)".to_string()
    } else {
        let m = marker_for(step, ArchiveStep::Script, spinner);
        format!("  {m} Running archive script")
    };
    let worktree_line = {
        let m = marker_for(step, ArchiveStep::RemoveWorktree, spinner);
        format!("  {m} Removing worktree…")
    };
    let branch_line = {
        let m = marker_for(step, ArchiveStep::DeleteBranch, spinner);
        format!("  {m} Deleting branch")
    };
    let cleanup_line = {
        let m = marker_for(step, ArchiveStep::Cleanup, spinner);
        format!("  {m} Cleaning up registry")
    };

    format!("{script_line}\n{worktree_line}\n{branch_line}\n{cleanup_line}")
}

/// Pick the marker character for `row` given the currently running `current` step.
fn marker_for(current: ArchiveStep, row: ArchiveStep, spinner: char) -> char {
    use std::cmp::Ordering;
    match step_ordinal(row).cmp(&step_ordinal(current)) {
        Ordering::Less => '✓',
        Ordering::Equal => spinner,
        Ordering::Greater => '·',
    }
}

fn step_ordinal(s: ArchiveStep) -> u8 {
    match s {
        ArchiveStep::Script => 0,
        ArchiveStep::RemoveWorktree => 1,
        ArchiveStep::DeleteBranch => 2,
        ArchiveStep::Cleanup => 3,
    }
}

/// Compute the order in which workspaces appear in the updates panel.
/// Returns workspace IDs in the same order the renderer walks them —
/// grouped by repo (in App's repo order), sorted within each repo by
/// (attention, failed, activity_rank, recency).
///
/// Used by both the renderer (to draw rows) and the key handler (to map
/// the selected index back to a workspace id).
pub fn ordered_workspaces_for_panel(
    repos: &[crate::data::store::Repo],
    workspaces: &[(RepoId, crate::data::store::Workspace)],
    events: &HashMap<crate::data::store::WorkspaceId, crate::events::WorkspaceEvents>,
    activity: &HashMap<crate::data::store::WorkspaceId, crate::ui::updates_bar::ActivityState>,
    needs_attention: &HashSet<crate::data::store::WorkspaceId>,
) -> Vec<crate::data::store::WorkspaceId> {
    let mut out = Vec::new();
    for repo in repos {
        let mut ws_for_repo: Vec<&crate::data::store::Workspace> = workspaces
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
    w: &crate::data::store::Workspace,
    events: &HashMap<crate::data::store::WorkspaceId, crate::events::WorkspaceEvents>,
    activity: &HashMap<crate::data::store::WorkspaceId, crate::ui::updates_bar::ActivityState>,
    needs_attention: &HashSet<crate::data::store::WorkspaceId>,
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
    let failed = if w.state == crate::data::store::WorkspaceState::Failed {
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
    repos: &[crate::data::store::Repo],
    workspaces: &[(RepoId, crate::data::store::Workspace)],
    events: &HashMap<crate::data::store::WorkspaceId, crate::events::WorkspaceEvents>,
    activity: &HashMap<crate::data::store::WorkspaceId, crate::ui::updates_bar::ActivityState>,
    needs_attention: &HashSet<crate::data::store::WorkspaceId>,
    awaiting: &HashMap<crate::data::store::WorkspaceId, (String, i64)>,
    statuses: &HashMap<crate::data::store::WorkspaceId, Status>,
    lifecycles: &HashMap<crate::data::store::WorkspaceId, BranchLifecycle>,
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
    let pos_of: HashMap<crate::data::store::WorkspaceId, usize> =
        order.iter().enumerate().map(|(i, id)| (*id, i)).collect();

    let mut lines: Vec<Line> = Vec::new();
    let mut selected_visual_line: Option<usize> = None;
    for repo in repos {
        lines.push(Line::from(Span::styled(
            repo.name.clone(),
            theme.header_style(),
        )));
        let ws_for_repo: Vec<&crate::data::store::Workspace> = workspaces
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
                if is_selected {
                    selected_visual_line = Some(lines.len());
                }
                let status = statuses.get(&w.id).copied().unwrap_or(Status::Idle);
                let lifecycle = lifecycles.get(&w.id).copied();
                lines.push(workspace_row(
                    w,
                    events.get(&w.id),
                    activity.get(&w.id).copied(),
                    needs_attention.contains(&w.id),
                    awaiting.get(&w.id),
                    is_selected,
                    status,
                    lifecycle,
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

    // Stateless scroll: keep the selected workspace centered in the viewport
    // when the rendered lines overflow the body area. Clamped so we never
    // scroll past the last line.
    let scroll_y =
        scroll_offset_for_selected(selected_visual_line, lines.len(), body_area.height as usize);

    // No widget-level style: per-span styles drive the row colors, and
    // every dim element (empty-repo hint, "(no repos)") already self-styles.
    // A widget-level dim would leak into spans with fg=None — notably the
    // workspace name when lifecycle is unknown.
    f.render_widget(Paragraph::new(lines).scroll((scroll_y, 0)), body_area);
    f.render_widget(
        Paragraph::new(
            "[\u{2191}/\u{2193}] move   [enter] switch   [v] vsplit   [s] hsplit   [esc] close",
        )
        .style(theme.dim_style()),
        footer_area,
    );
}

/// Compute the vertical scroll offset for the updates panel so the selected
/// row stays visible. Stateless — called fresh each render. Strategy:
/// center the selected line in the viewport, then clamp to the valid scroll
/// range so we never scroll past the end. Returns 0 when content fits or
/// when there is no selection.
fn scroll_offset_for_selected(
    selected_visual_line: Option<usize>,
    total_lines: usize,
    viewport_height: usize,
) -> u16 {
    let Some(s) = selected_visual_line else {
        return 0;
    };
    if viewport_height == 0 || total_lines <= viewport_height {
        return 0;
    }
    let centered = s.saturating_sub(viewport_height / 2);
    let max_scroll = total_lines.saturating_sub(viewport_height);
    centered.min(max_scroll).min(u16::MAX as usize) as u16
}

#[allow(clippy::too_many_arguments)]
fn workspace_row<'a>(
    w: &'a crate::data::store::Workspace,
    events: Option<&'a crate::events::WorkspaceEvents>,
    activity: Option<crate::ui::updates_bar::ActivityState>,
    needs_attention: bool,
    awaiting: Option<&'a (String, i64)>,
    is_selected: bool,
    status: Status,
    lifecycle: Option<BranchLifecycle>,
    now_ms: i64,
    theme: &Theme,
) -> Line<'a> {
    use crate::ui::updates_bar::{ActivityState, format_age, glyph_for_activity};
    let failed = w.state == crate::data::store::WorkspaceState::Failed;
    let glyph = if failed {
        '✕'
    } else if needs_attention {
        activity.map(glyph_for_activity).unwrap_or('⚠')
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
    } else if failed {
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

    // Failed overrides the canonical status hue with `err` — a failed
    // workspace is the same urgency signal regardless of its prior status.
    let status_fg = if failed {
        theme.err_style()
    } else {
        theme.status_style(status)
    };
    // Lifecycle wins on the name even when the workspace is failed — a
    // failed workspace can still have a merged PR. Bold so the name
    // still reads as a name. When there's no lifecycle hue, explicitly
    // reset fg so the surrounding Block's dim style can't leak through
    // ratatui's style inheritance and dim the workspace name.
    let name_style = theme
        .lifecycle_style(lifecycle)
        .unwrap_or_else(|| Style::default().fg(ratatui::style::Color::Reset))
        .add_modifier(Modifier::BOLD);

    let name_padded = format!("{:<20}", w.name);
    let spans = vec![
        Span::raw("  "),
        Span::styled(format!("{glyph} "), status_fg),
        Span::styled(name_padded, name_style),
        Span::styled(format!(" {status_text}{suffix}"), status_fg),
    ];

    let mut line = Line::from(spans);
    if is_selected {
        // bg-only so per-span fg colors survive; matches the dashboard's
        // List::highlight_style(theme.selected_bg_style()).
        line = line.style(theme.selected_bg_style());
    }
    line
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
    repo: &crate::data::store::Repo,
    selected: usize,
    theme: &Theme,
) {
    let w = area.width.clamp(40, 90);
    let h = area.height.clamp(12, 20);
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

    let rows: [(crate::app::RepoSettingField, Option<&str>); 9] = [
        (
            crate::app::RepoSettingField::RepoName,
            Some(repo.name.as_str()),
        ),
        (
            crate::app::RepoSettingField::BranchPrefix,
            if repo.branch_prefix.is_empty() {
                None
            } else {
                Some(repo.branch_prefix.as_str())
            },
        ),
        (
            crate::app::RepoSettingField::BaseBranch,
            repo.base_branch.as_deref(),
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
        (
            crate::app::RepoSettingField::DetailBarConfig,
            repo.detail_bar_config.as_deref(),
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
mod scroll_offset_tests {
    use super::*;

    #[test]
    fn no_selection_yields_zero_offset() {
        assert_eq!(scroll_offset_for_selected(None, 100, 20), 0);
    }

    #[test]
    fn content_fits_in_viewport_yields_zero_offset() {
        // 10 lines, viewport 20, selected at line 5 — no scroll needed.
        assert_eq!(scroll_offset_for_selected(Some(5), 10, 20), 0);
    }

    #[test]
    fn zero_height_viewport_yields_zero_offset() {
        assert_eq!(scroll_offset_for_selected(Some(50), 100, 0), 0);
    }

    #[test]
    fn selection_in_first_half_does_not_scroll() {
        // Selected at line 4, viewport 20, total 100: centering would put
        // selected at top half, so offset stays 0.
        assert_eq!(scroll_offset_for_selected(Some(4), 100, 20), 0);
    }

    #[test]
    fn selection_centers_in_viewport_when_overflowing() {
        // Selected at line 50, viewport 20, total 100.
        // centered = 50 - 10 = 40. max_scroll = 80. result = 40.
        // Selected appears at viewport row 50 - 40 = 10 (middle).
        assert_eq!(scroll_offset_for_selected(Some(50), 100, 20), 40);
    }

    #[test]
    fn selection_near_end_clamps_to_max_scroll() {
        // Selected at last line (99), viewport 20, total 100.
        // centered = 99 - 10 = 89. max_scroll = 80. clamped to 80.
        // Selected appears at viewport row 99 - 80 = 19 (last row).
        assert_eq!(scroll_offset_for_selected(Some(99), 100, 20), 80);
    }

    #[test]
    fn last_line_selected_in_short_overflow() {
        // total = 22, viewport = 20 — barely overflows by 2.
        // Selected at line 21 (last). centered = 21 - 10 = 11.
        // max_scroll = 2. clamped to 2. selected appears at row 19.
        assert_eq!(scroll_offset_for_selected(Some(21), 22, 20), 2);
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
mod render_archive_steps_tests {
    use super::*;

    #[test]
    fn step_script_with_script_present_marks_script_in_progress() {
        let body = render_archive_steps(ArchiveStep::Script, true, 0);
        // Spinner frame for tick=0 is '⠋' (from spinner::frame tests).
        assert!(
            body.contains("⠋ Running archive script"),
            "body was:\n{body}"
        );
        assert!(body.contains("· Removing worktree"), "body was:\n{body}");
        assert!(body.contains("· Deleting branch"), "body was:\n{body}");
        assert!(body.contains("· Cleaning up registry"), "body was:\n{body}");
    }

    #[test]
    fn step_remove_worktree_marks_script_done_and_worktree_in_progress() {
        let body = render_archive_steps(ArchiveStep::RemoveWorktree, true, 0);
        assert!(
            body.contains("✓ Running archive script"),
            "body was:\n{body}"
        );
        assert!(body.contains("⠋ Removing worktree"), "body was:\n{body}");
        assert!(body.contains("· Deleting branch"), "body was:\n{body}");
        assert!(body.contains("· Cleaning up registry"), "body was:\n{body}");
    }

    #[test]
    fn step_cleanup_marks_everything_but_cleanup_done() {
        let body = render_archive_steps(ArchiveStep::Cleanup, true, 0);
        assert!(
            body.contains("✓ Running archive script"),
            "body was:\n{body}"
        );
        assert!(body.contains("✓ Removing worktree"), "body was:\n{body}");
        assert!(body.contains("✓ Deleting branch"), "body was:\n{body}");
        assert!(body.contains("⠋ Cleaning up registry"), "body was:\n{body}");
    }

    #[test]
    fn script_absent_renders_skipped_regardless_of_step() {
        // Even when step is still Script, no-script repos render
        // the Script row as (skipped) — never spinning.
        for step in [
            ArchiveStep::Script,
            ArchiveStep::RemoveWorktree,
            ArchiveStep::DeleteBranch,
            ArchiveStep::Cleanup,
        ] {
            let body = render_archive_steps(step, false, 0);
            assert!(
                body.contains("— Archive script (skipped)"),
                "step={step:?} body was:\n{body}"
            );
            assert!(
                !body.contains("⠋ Running archive script"),
                "script row should never spin when script_present=false; body was:\n{body}"
            );
        }
    }

    #[test]
    fn spinner_frame_varies_with_tick() {
        // The spinner glyph at tick=0 is '⠋'; at tick=8 it's '⠙'.
        // This sanity-checks that render_archive_steps actually
        // threads `tick` through to spinner::frame.
        let body0 = render_archive_steps(ArchiveStep::RemoveWorktree, true, 0);
        let body8 = render_archive_steps(ArchiveStep::RemoveWorktree, true, 8);
        assert!(body0.contains('⠋'));
        assert!(body8.contains('⠙'));
    }
}

#[cfg(test)]
mod workspace_row_tests {
    use super::*;
    use crate::data::store::{Workspace, WorkspaceId, WorkspaceState};
    use crate::ui::updates_bar::ActivityState;
    use std::path::PathBuf;

    fn fixture_workspace(name: &str) -> Workspace {
        Workspace {
            id: WorkspaceId(1),
            repo_id: crate::data::store::RepoId(1),
            name: name.to_string(),
            branch: "main".to_string(),
            worktree_path: PathBuf::from("/tmp/ws"),
            state: WorkspaceState::Ready,
            setup_status: crate::data::store::SetupStatus::Ok,
            created_at: 0,
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
        }
    }

    /// Concatenate every span's content into a single String so tests can
    /// match against the rendered text regardless of styling.
    fn line_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// Find the first span whose content contains `needle`. Tests use this
    /// to locate the glyph, name, or status-text span by a known substring.
    fn span_containing<'a>(line: &'a Line<'_>, needle: &str) -> &'a Span<'a> {
        line.spans
            .iter()
            .find(|s| s.content.as_ref().contains(needle))
            .unwrap_or_else(|| panic!("no span containing {needle:?}"))
    }

    #[test]
    fn workspace_row_uses_question_glyph_for_awaiting_answer() {
        let theme = Theme::ansi();
        let w = fixture_workspace("alpha");
        let line = workspace_row(
            &w,
            None,
            Some(ActivityState::AwaitingAnswer),
            true,
            None,
            false,
            Status::Question,
            None,
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
        let theme = Theme::ansi();
        let w = fixture_workspace("alpha");
        let line = workspace_row(
            &w,
            None,
            Some(ActivityState::Complete),
            true,
            None,
            false,
            Status::Complete,
            None,
            10_000,
            &theme,
        );
        let body = line_text(&line);
        assert!(body.contains('\u{2713}'), "expected '✓' glyph in: {body}");
        assert!(
            body.contains("complete"),
            "expected 'complete' status text in: {body}"
        );
    }

    #[test]
    fn workspace_row_shows_permission_tool_in_status_text() {
        let theme = Theme::ansi();
        let w = fixture_workspace("alpha");
        let awaiting = ("Bash".to_string(), 5_000i64);
        let line = workspace_row(
            &w,
            None,
            Some(ActivityState::Awaiting),
            true,
            Some(&awaiting),
            false,
            Status::Question,
            None,
            10_000,
            &theme,
        );
        let body = line_text(&line);
        assert!(body.contains('⚠'), "expected '⚠' glyph in: {body}");
        assert!(
            body.contains("awaiting permission: Bash"),
            "expected permission tool name in status text: {body}"
        );
    }

    /// For each of the six canonical Status variants, the glyph and status-
    /// text spans should be painted with theme.status_style(status).fg.
    /// Mirrors the dashboard's gutter/glyph coloring so a glance at the modal
    /// matches a glance at the dashboard.
    #[test]
    fn workspace_row_paints_glyph_and_text_with_status_color() {
        let theme = Theme::ansi();
        let w = fixture_workspace("alpha");
        // (status, activity option, needs_attention, label substring to find)
        let cases: [(Status, Option<ActivityState>, bool, &str); 6] = [
            (
                Status::Question,
                Some(ActivityState::AwaitingAnswer),
                true,
                "question",
            ),
            (
                Status::Complete,
                Some(ActivityState::Complete),
                true,
                "complete",
            ),
            (
                Status::Stalled,
                Some(ActivityState::Stalled),
                true,
                "stalled",
            ),
            (
                Status::Waiting,
                Some(ActivityState::Waiting),
                true,
                "waiting",
            ),
            (
                Status::Thinking,
                Some(ActivityState::Active),
                false,
                "active",
            ),
            (Status::Idle, None, false, "no session"),
        ];
        for (status, activity, needs_attention, label) in cases {
            let line = workspace_row(
                &w,
                None,
                activity,
                needs_attention,
                None,
                false,
                status,
                None,
                10_000,
                &theme,
            );
            let glyph_span = &line.spans[1];
            let text_span = span_containing(&line, label);
            let expected = theme.status_style(status).fg;
            assert_eq!(
                glyph_span.style.fg, expected,
                "glyph fg for {status:?} should match status_style"
            );
            assert_eq!(
                text_span.style.fg, expected,
                "status text fg for {status:?} should match status_style"
            );
        }
    }

    /// Failed workspaces ignore the canonical status hue and paint glyph +
    /// text with err — failure is the same urgency signal regardless of what
    /// the classifier said before the failure.
    #[test]
    fn workspace_row_failed_overrides_status_with_err() {
        let theme = Theme::ansi();
        let mut w = fixture_workspace("alpha");
        w.state = WorkspaceState::Failed;
        let line = workspace_row(
            &w,
            None,
            None,
            false,
            None,
            false,
            Status::Idle, // classifier might say anything; failed wins
            None,
            10_000,
            &theme,
        );
        let glyph_span = &line.spans[1];
        let text_span = span_containing(&line, "failed");
        assert_eq!(glyph_span.style.fg, Some(theme.err));
        assert_eq!(text_span.style.fg, Some(theme.err));
    }

    /// Lifecycle drives the workspace name's foreground color. Mirrors the
    /// dashboard branch column so the modal and dashboard tell the same story
    /// about PR state.
    #[test]
    fn workspace_row_paints_name_with_lifecycle_color() {
        use crate::git::forge::BranchLifecycle::*;
        let theme = Theme::ansi();
        let w = fixture_workspace("alpha");
        // Lifecycles without a hue (NoPr, PrDraft, None) fall back to
        // Color::Reset so the surrounding Block's dim style can't leak
        // through ratatui's style inheritance and dim the name.
        let reset = Some(ratatui::style::Color::Reset);
        let cases = [
            (Some(PrOpen), Some(theme.ok)),
            (Some(PrConflicted), Some(theme.warn)),
            (Some(PrMerged), Some(theme.merged)),
            (Some(PrClosed), Some(theme.err)),
            (Some(NoPr), reset),
            (Some(PrDraft), reset),
            (None, reset),
        ];
        for (lifecycle, expected_fg) in cases {
            let line = workspace_row(
                &w,
                None,
                None,
                false,
                None,
                false,
                Status::Idle,
                lifecycle,
                10_000,
                &theme,
            );
            let name_span = span_containing(&line, "alpha");
            assert_eq!(
                name_span.style.fg, expected_fg,
                "name fg for lifecycle {lifecycle:?}"
            );
            assert!(
                name_span.style.add_modifier.contains(Modifier::BOLD),
                "name should be bold for lifecycle {lifecycle:?}"
            );
        }
    }

    /// Selection should only set the row's background — per-span foregrounds
    /// (status hue, lifecycle hue) must survive so the user can still tell at
    /// a glance which workspace is in what state on the selected row.
    #[test]
    fn workspace_row_selection_keeps_span_foregrounds() {
        let theme = Theme::ansi();
        let w = fixture_workspace("alpha");
        let line = workspace_row(
            &w,
            None,
            Some(ActivityState::Complete),
            true,
            None,
            true, // selected
            Status::Complete,
            Some(crate::git::forge::BranchLifecycle::PrOpen),
            10_000,
            &theme,
        );
        // Line-level style carries only the selected bg, not a foreground.
        assert_eq!(line.style.bg, Some(theme.selected_bg));
        assert_eq!(line.style.fg, None);
        // Per-span foregrounds still match status / lifecycle.
        let glyph_span = &line.spans[1];
        let name_span = span_containing(&line, "alpha");
        let text_span = span_containing(&line, "complete");
        assert_eq!(glyph_span.style.fg, Some(theme.complete));
        assert_eq!(name_span.style.fg, Some(theme.ok));
        assert_eq!(text_span.style.fg, Some(theme.complete));
    }
}
