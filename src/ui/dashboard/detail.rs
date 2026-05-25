//! Bottom-pinned detail bar shown when a workspace is selected on the
//! dashboard. Renders header strip, three-column body, and an inline
//! reply input.
//!
//! See `docs/superpowers/specs/2026-05-24-dashboard-workspace-detail-design.md`.

use crate::detail_bar_config::DetailBarConfig;
use crate::events::{ToolUseCounts, WorkspaceEvents};
use crate::forge::BranchLifecycle;
use crate::git::DiffStats;
use crate::proc::ProcInfo;
use crate::store::{Repo, Workspace};
use crate::ui::dashboard::status::Status;
use crate::ui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    SessionSummary,
    RecentChat,
    ProcsAndFiles,
}

pub fn enabled_columns(cfg: &DetailBarConfig) -> Vec<Column> {
    let mut out = Vec::with_capacity(3);
    if cfg.sections.session_summary {
        out.push(Column::SessionSummary);
    }
    if cfg.sections.recent_chat {
        out.push(Column::RecentChat);
    }
    if cfg.sections.procs_and_files {
        out.push(Column::ProcsAndFiles);
    }
    out
}

/// Width percentages for the enabled body columns. Preserves the
/// legacy 30/40/30 ratio when all three are present; redistributes
/// proportionally otherwise.
pub fn column_widths(cols: &[Column]) -> Vec<u16> {
    use Column::*;
    match cols {
        [] => vec![],
        [_] => vec![100],
        [SessionSummary, RecentChat] => vec![43, 57],
        [SessionSummary, ProcsAndFiles] => vec![50, 50],
        [RecentChat, ProcsAndFiles] => vec![57, 43],
        [SessionSummary, RecentChat, ProcsAndFiles] => vec![30, 40, 30],
        _ => {
            let n = cols.len() as u16;
            let each = 100 / n;
            (0..n).map(|_| each).collect()
        }
    }
}

/// What `app.rs::draw` assembles for the detail bar. Borrowed for the
/// duration of a single draw call.
#[derive(Debug)]
pub struct DetailInputs<'a> {
    pub repo: &'a Repo,
    pub workspace: &'a Workspace,
    pub events: Option<&'a WorkspaceEvents>,
    pub procs: &'a [ProcInfo],
    pub diff: Option<DiffStats>,
    /// Per-file diff stats keyed by path relative to the worktree
    /// root. Used to annotate RECENT FILES entries with `+X −Y`.
    pub diff_per_file: Option<&'a std::collections::HashMap<String, DiffStats>>,
    pub lifecycle: Option<BranchLifecycle>,
    pub pr_title: Option<String>,
    pub pr_number: Option<u32>,
    pub status: Status,
    pub ago_secs: Option<u64>,
    pub reply_draft: &'a str,
    pub reply_focused: bool,
    /// True once the workspace's JSONL has been scanned at least once
    /// (`workspace_events_scanned` on `App`). When false, SESSION
    /// SUMMARY and RECENT CHAT show `loading…` placeholders instead
    /// of derived content.
    pub events_scanned: bool,
    pub config: &'a DetailBarConfig,
}

/// Render the detail bar into `area`. No-op when `area.height` is below
/// the config's `min_rows` (caller is expected to fall back to a
/// condensed banner — see `app.rs::draw`).
pub fn render(f: &mut Frame, area: Rect, inputs: &DetailInputs<'_>, theme: &Theme) {
    if area.height == 0 || area.height < inputs.config.height.min_rows {
        return;
    }
    use ratatui::layout::{Constraint, Direction, Layout};
    use ratatui::widgets::Paragraph;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header strip
            Constraint::Length(1), // rule
            Constraint::Min(1),    // body (3 columns)
            Constraint::Length(1), // rule
            Constraint::Length(1), // reply row
        ])
        .split(area);

    let header = build_header_strip(
        &inputs.workspace.name,
        &inputs.workspace.branch,
        inputs.lifecycle,
        inputs.diff,
        inputs.procs.len() as u32,
        inputs.status,
        inputs.ago_secs,
        theme,
        chunks[0].width as usize,
    );
    f.render_widget(Paragraph::new(header), chunks[0]);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "─".repeat(chunks[1].width as usize),
            theme.dim_style(),
        ))),
        chunks[1],
    );

    let cols = enabled_columns(inputs.config);
    if chunks[2].width >= 80 && cols.len() > 1 {
        let widths = column_widths(&cols);
        let body_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(
                widths
                    .iter()
                    .map(|w| Constraint::Percentage(*w))
                    .collect::<Vec<_>>(),
            )
            .split(chunks[2]);
        for (idx, col) in cols.iter().enumerate() {
            let col_area = body_chunks[idx];
            render_column(f, col_area, *col, inputs, theme, chunks[2].height);
        }
    } else if let Some(only) = cols.first() {
        // Narrow terminal OR single enabled column → render whichever
        // column comes first in display order at full width.
        render_column(f, chunks[2], *only, inputs, theme, chunks[2].height);
    }
    // If `cols` is empty, body region is rendered as blank (no-op).

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "─".repeat(chunks[3].width as usize),
            theme.dim_style(),
        ))),
        chunks[3],
    );

    let reply = build_reply_row(
        inputs.reply_draft,
        inputs.reply_focused,
        theme,
        chunks[4].width as usize,
    );
    f.render_widget(Paragraph::new(reply), chunks[4]);

    if inputs.reply_focused {
        let cx = reply_cursor_x(inputs.reply_draft, chunks[4].width as usize);
        f.set_cursor_position((chunks[4].x + cx, chunks[4].y));
    }
}

fn render_column(
    f: &mut Frame,
    area: Rect,
    col: Column,
    inputs: &DetailInputs<'_>,
    theme: &Theme,
    body_height: u16,
) {
    use ratatui::widgets::Paragraph;
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let created_at_secs = (inputs.workspace.created_at.max(0) / 1000) as u64;
    let created_secs = now_secs.saturating_sub(created_at_secs);

    match col {
        Column::SessionSummary => {
            let lines = build_session_summary(
                if inputs.events_scanned {
                    inputs.events
                } else {
                    None
                },
                inputs.status,
                theme,
                area.width as usize,
                created_secs,
                inputs.ago_secs,
            );
            f.render_widget(Paragraph::new(lines), area);
        }
        Column::RecentChat => {
            let lines = build_recent_chat(
                if inputs.events_scanned {
                    inputs.events
                } else {
                    None
                },
                theme,
                area.width as usize,
                (body_height as usize).saturating_sub(1).max(1),
            );
            f.render_widget(Paragraph::new(lines), area);
        }
        Column::ProcsAndFiles => {
            let lines = build_procs_and_files(
                inputs.procs,
                inputs.events,
                inputs.diff_per_file,
                &inputs.workspace.worktree_path,
                theme,
                area.width as usize,
            );
            f.render_widget(Paragraph::new(lines), area);
        }
    }
}

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

const GUTTER: &str = "▍";

/// One-line header strip at the top of the bar.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_header_strip(
    name: &str,
    branch: &str,
    lifecycle: Option<BranchLifecycle>,
    diff: Option<DiffStats>,
    procs: u32,
    status: Status,
    ago_secs: Option<u64>,
    theme: &Theme,
    width: usize,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled(GUTTER.to_string(), theme.status_style(status)));
    spans.push(Span::raw(" ".to_string()));
    spans.push(Span::styled(
        name.to_string(),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::raw("  ".to_string()));
    spans.push(Span::styled(format!("⎇ {branch}"), theme.dim_style()));

    if let Some(lc) = lifecycle {
        spans.push(Span::raw("  ".to_string()));
        let (glyph, label) = lifecycle_chip(lc);
        spans.push(Span::styled(
            format!("{glyph} {label}"),
            theme
                .lifecycle_style(Some(lc))
                .unwrap_or_else(|| theme.dim_style()),
        ));
    }

    if let Some(d) = diff
        && (d.added > 0 || d.removed > 0)
    {
        spans.push(Span::raw("  ".to_string()));
        spans.push(Span::styled(format!("+{}", d.added), theme.ok_style()));
        spans.push(Span::raw(" ".to_string()));
        spans.push(Span::styled(format!("−{}", d.removed), theme.err_style()));
    }

    spans.push(Span::raw("  ".to_string()));
    let procs_style = if procs > 0 {
        theme.status_style(Status::Thinking)
    } else {
        theme.dim_style()
    };
    spans.push(Span::styled(format!("● {procs} procs"), procs_style));

    spans.push(Span::raw("  ".to_string()));
    spans.push(Span::styled(
        status.glyph().to_string(),
        theme.status_style(status),
    ));
    spans.push(Span::raw(" ".to_string()));
    spans.push(Span::styled(
        status.label().to_string(),
        theme.status_style(status),
    ));

    let ago = format_ago_short(ago_secs);
    spans.push(Span::styled(format!("  · {ago}"), theme.dim_style()));

    // Right-truncate the full line to `width` cells by padding or
    // dropping spans — for v1 we trust the caller to give us enough
    // room (width >= 60); narrow-width handling is in Task 12.
    let _ = width;
    Line::from(spans)
}

fn lifecycle_chip(lc: BranchLifecycle) -> (&'static str, &'static str) {
    match lc {
        BranchLifecycle::PrOpen => ("⏺", "open"),
        BranchLifecycle::PrDraft => ("⏷", "draft"),
        BranchLifecycle::PrMerged => ("⏺", "merged"),
        BranchLifecycle::PrClosed => ("⏸", "closed"),
        BranchLifecycle::PrConflicted => ("⏺", "conflict"),
        BranchLifecycle::NoPr => ("", ""),
    }
}

fn format_ago_short(secs: Option<u64>) -> String {
    match secs {
        None => "—".to_string(),
        Some(s) if s < 60 => format!("{s}s"),
        Some(s) if s < 3600 => format!("{}m", s / 60),
        Some(s) => format!("{}h", s / 3600),
    }
}

/// Build the lines that make up the SESSION SUMMARY column.
///
/// Renders four pieces:
/// 1. The session's initial user prompt (multi-line, respects `\n`),
///    italicized.
/// 2. The action trace synthesized from `tool_use_counts`.
/// 3. The session age — `created Xs/m/h ago`, from `workspace.created_at`.
/// 4. Time since the agent's last PTY activity — `active Xs/m/h ago`,
///    or `active —` when there's no live session.
///
/// The age + activity lines render regardless of whether `events` have
/// been scanned yet, since they're session metadata sourced outside of
/// the JSONL log.
pub(super) fn build_session_summary(
    events: Option<&WorkspaceEvents>,
    status: Status,
    theme: &Theme,
    column_width: usize,
    created_secs: u64,
    ago_secs: Option<u64>,
) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let label_style = Style::default().fg(theme.path).add_modifier(Modifier::BOLD);
    out.push(Line::from(Span::styled(
        "SESSION SUMMARY".to_string(),
        label_style,
    )));

    // Bullet prefix takes the workspace's status color so the SESSION
    // SUMMARY column visually echoes the row's status gutter.
    let prefix = Span::styled("▸ ".to_string(), theme.status_style(status));
    // Continuation indent for wrapped/multi-line prompts: 2 cells, so
    // wrapped lines align with the first character of the prompt text.
    let continuation = Span::raw("  ".to_string());

    match events {
        None => {
            out.push(Line::from(Span::styled(
                "  loading…".to_string(),
                theme.dim_style(),
            )));
        }
        Some(evt) => {
            if let Some(prompt) = evt.first_user_text.as_deref() {
                let trimmed = prompt.trim();
                if !trimmed.is_empty() {
                    // Respect `\n` from the original prompt AND wrap long lines
                    // to the column width so the prompt is fully readable.
                    let inner_width = column_width.saturating_sub(2).max(1);
                    let wrapped = wrap_lines(trimmed, inner_width);
                    let italic = Style::default().add_modifier(Modifier::ITALIC);
                    for (i, line_text) in wrapped.iter().enumerate() {
                        let leader = if i == 0 {
                            prefix.clone()
                        } else {
                            continuation.clone()
                        };
                        out.push(Line::from(vec![
                            leader,
                            Span::styled(line_text.clone(), italic),
                        ]));
                    }
                }
            }

            let trace = format_tool_trace(&evt.tool_use_counts);
            let trace_body = if trace.is_empty() {
                Span::styled("—".to_string(), theme.dim_style())
            } else {
                Span::raw(truncate_to_chars(&trace, column_width.saturating_sub(2)))
            };
            out.push(Line::from(vec![prefix.clone(), trace_body]));
        }
    }

    let created_text = format!("created {} ago", format_ago_short(Some(created_secs)));
    out.push(Line::from(vec![
        prefix.clone(),
        Span::styled(created_text, theme.dim_style()),
    ]));

    let active_text = match ago_secs {
        Some(s) => format!("active {} ago", format_ago_short(Some(s))),
        None => "active —".to_string(),
    };
    out.push(Line::from(vec![
        prefix.clone(),
        Span::styled(active_text, theme.dim_style()),
    ]));

    out
}

/// Build the RECENT CHAT column. `max_body_lines` caps how many content
/// lines render below the column label.
pub(super) fn build_recent_chat(
    events: Option<&WorkspaceEvents>,
    theme: &Theme,
    column_width: usize,
    max_body_lines: usize,
) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let label_style = Style::default().fg(theme.path).add_modifier(Modifier::BOLD);
    out.push(Line::from(Span::styled(
        "RECENT CHAT".to_string(),
        label_style,
    )));

    let Some(evt) = events else {
        out.push(Line::from(Span::styled(
            "  loading…".to_string(),
            theme.dim_style(),
        )));
        return out;
    };

    let Some(text) = evt.last_assistant_text.as_deref() else {
        out.push(Line::from(Span::styled("—".to_string(), theme.dim_style())));
        return out;
    };

    // Word-wrap to column_width. Take the last `max_body_lines` after wrapping.
    let wrapped = wrap_lines(text, column_width);
    let start = wrapped.len().saturating_sub(max_body_lines);
    for line in wrapped.iter().skip(start) {
        out.push(Line::from(Span::styled(line.clone(), theme.dim_style())));
    }
    out
}

/// Build the PROCESSES + RECENT FILES column. Procs go on top, recent
/// files (from `WorkspaceEvents.recent_edited_files`) below, each
/// annotated with a `+X −Y` delta when the per-file diff map has an
/// entry for it.
pub(super) fn build_procs_and_files(
    procs: &[ProcInfo],
    events: Option<&WorkspaceEvents>,
    diff_per_file: Option<&std::collections::HashMap<String, DiffStats>>,
    worktree_path: &std::path::Path,
    theme: &Theme,
    column_width: usize,
) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let label_style = Style::default().fg(theme.path).add_modifier(Modifier::BOLD);

    out.push(Line::from(Span::styled(
        "PROCESSES".to_string(),
        label_style,
    )));
    if procs.is_empty() {
        out.push(Line::from(Span::styled("—".to_string(), theme.dim_style())));
    } else {
        let visible = procs.iter().take(5);
        for p in visible {
            let cmd = truncate_to_chars(&p.command, column_width.saturating_sub(4));
            out.push(Line::from(vec![
                Span::styled("● ".to_string(), theme.status_style(Status::Thinking)),
                Span::styled(cmd, theme.dim_style()),
            ]));
        }
        if procs.len() > 5 {
            out.push(Line::from(Span::styled(
                format!("+{} more", procs.len() - 5),
                theme.dim_style(),
            )));
        }
    }

    out.push(Line::from(Span::styled(
        "RECENT FILES".to_string(),
        label_style,
    )));
    let files: Vec<&String> = events
        .map(|e| e.recent_edited_files.iter().collect())
        .unwrap_or_default();
    if files.is_empty() {
        out.push(Line::from(Span::styled("—".to_string(), theme.dim_style())));
    } else {
        for f in files.iter().take(5) {
            let diff = lookup_file_diff(f, worktree_path, diff_per_file);
            let suffix_width = match diff {
                Some(d) if d.added > 0 || d.removed > 0 => {
                    // "  +A −R" — 2 leading spaces + sign + digits + sep + sign + digits.
                    4 + d.added.to_string().chars().count() + d.removed.to_string().chars().count()
                }
                _ => 0,
            };
            // Show the path relative to the worktree root so the column
            // isn't dominated by the shared `/Users/.../worktrees/...`
            // prefix. Falls back to the absolute path if the file isn't
            // inside the worktree.
            let display = display_relative_path(f, worktree_path);
            let path_width = column_width.saturating_sub(suffix_width);
            let truncated = truncate_to_chars_left(&display, path_width);
            let mut spans: Vec<Span<'static>> = vec![Span::styled(truncated, theme.dim_style())];
            if let Some(d) = diff
                && (d.added > 0 || d.removed > 0)
            {
                spans.push(Span::raw("  ".to_string()));
                spans.push(Span::styled(format!("+{}", d.added), theme.ok_style()));
                spans.push(Span::raw(" ".to_string()));
                spans.push(Span::styled(format!("−{}", d.removed), theme.err_style()));
            }
            out.push(Line::from(spans));
        }
    }
    out
}

/// Render a recent-edited file path relative to the worktree root.
/// Returns the absolute path unchanged when it doesn't sit inside the
/// worktree (rare — only happens if claude wrote a path outside its
/// own cwd).
fn display_relative_path(file: &str, worktree_path: &std::path::Path) -> String {
    std::path::Path::new(file)
        .strip_prefix(worktree_path)
        .ok()
        .and_then(|p| p.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| file.to_string())
}

/// Look up a recent-edited file's per-file diff. `file` is whatever
/// the JSONL `file_path` field contained (usually an absolute path
/// inside the worktree). `worktree_path` is the workspace's worktree
/// root. The diff map is keyed by paths relative to that root.
fn lookup_file_diff(
    file: &str,
    worktree_path: &std::path::Path,
    diff_per_file: Option<&std::collections::HashMap<String, DiffStats>>,
) -> Option<DiffStats> {
    let map = diff_per_file?;
    let rel = std::path::Path::new(file)
        .strip_prefix(worktree_path)
        .ok()?;
    let key = rel.to_str()?;
    map.get(key).copied()
}

const REPLY_CHIP: &str = "┃ Reply to agent ┃";
const REPLY_HINT: &str = "  ↵ send · Esc cancel";

/// Reply input row. Returns a `Line` plus an optional cursor X-offset
/// (within the line) that the caller passes to `f.set_cursor_position`
/// when `focused == true`. The caller adds `area.x` and the row's `y`.
pub(super) fn build_reply_row(
    draft: &str,
    focused: bool,
    theme: &Theme,
    width: usize,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let chip_style = if focused {
        Style::default().fg(theme.path).add_modifier(Modifier::BOLD)
    } else {
        theme.dim_style()
    };
    spans.push(Span::styled(REPLY_CHIP.to_string(), chip_style));
    spans.push(Span::raw(" ".to_string()));

    let hint_width = if focused {
        REPLY_HINT.chars().count()
    } else {
        0
    };
    let chip_width = REPLY_CHIP.chars().count() + 1; // chip + 1 trailing space
    let field_width = width
        .saturating_sub(chip_width)
        .saturating_sub(hint_width)
        .max(1);

    // Right-align the cursor in the visible window: take the LAST
    // `field_width - 1` chars (reserve 1 cell for the cursor when
    // focused; when unfocused that cell holds the trailing space).
    let cursor_room = if focused { 1 } else { 0 };
    let visible_chars = field_width.saturating_sub(cursor_room).max(1);
    let total = draft.chars().count();
    let skip = total.saturating_sub(visible_chars);
    let visible: String = draft.chars().skip(skip).collect();
    let padding = field_width.saturating_sub(visible.chars().count() + cursor_room);
    spans.push(Span::styled(visible, Style::default()));
    if padding > 0 {
        spans.push(Span::raw(" ".repeat(padding)));
    }

    if focused {
        spans.push(Span::styled(REPLY_HINT.to_string(), theme.dim_style()));
    }

    Line::from(spans)
}

/// Cursor x-offset (within the reply row) when focused. Returns the
/// column where `f.set_cursor_position` should be set.
pub(super) fn reply_cursor_x(draft: &str, width: usize) -> u16 {
    let chip_width = REPLY_CHIP.chars().count() + 1;
    let hint_width = REPLY_HINT.chars().count();
    let field_width = width
        .saturating_sub(chip_width)
        .saturating_sub(hint_width)
        .max(1);
    let visible_chars = field_width.saturating_sub(1).max(1);
    let total = draft.chars().count();
    let visible_count = total.min(visible_chars);
    (chip_width + visible_count) as u16
}

/// Greedy word-wrap. Splits long words at the column boundary.
fn wrap_lines(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    for paragraph in text.split('\n') {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if word.chars().count() > width {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
                let mut buf: String = String::new();
                for ch in word.chars() {
                    if buf.chars().count() == width {
                        out.push(std::mem::take(&mut buf));
                    }
                    buf.push(ch);
                }
                if !buf.is_empty() {
                    current = buf;
                }
                continue;
            }
            let projected = if current.is_empty() {
                word.chars().count()
            } else {
                current.chars().count() + 1 + word.chars().count()
            };
            if projected > width {
                out.push(std::mem::take(&mut current));
                current.push_str(word);
            } else {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
            }
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn format_tool_trace(counts: &ToolUseCounts) -> String {
    let mut parts: Vec<String> = Vec::new();
    if counts.read > 0 {
        parts.push(format!(
            "read {} {}",
            counts.read,
            plural("file", counts.read)
        ));
    }
    if counts.edit > 0 {
        parts.push(format!(
            "edited {} {}",
            counts.edit,
            plural("file", counts.edit)
        ));
    }
    if counts.write > 0 {
        parts.push(format!(
            "wrote {} {}",
            counts.write,
            plural("file", counts.write)
        ));
    }
    if counts.bash > 0 {
        parts.push(format!(
            "ran {} {}",
            counts.bash,
            plural("command", counts.bash)
        ));
    }
    if counts.other > 0 {
        parts.push(format!("+{} other actions", counts.other));
    }
    parts.join(", ")
}

fn plural(noun: &str, n: u32) -> String {
    if n == 1 {
        noun.to_string()
    } else {
        format!("{noun}s")
    }
}

fn truncate_to_chars(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let count = s.chars().count();
    if count <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn truncate_to_chars_left(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let count = s.chars().count();
    if count <= max {
        s.to_string()
    } else {
        let skip = count.saturating_sub(max.saturating_sub(1));
        let tail: String = s.chars().skip(skip).collect();
        format!("…{tail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::dashboard::status::Status;
    use crate::ui::theme::Theme;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn line_to_string(line: &ratatui::text::Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn render_to_text(inputs: &DetailInputs<'_>, w: u16, h: u16) -> String {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let theme = Theme::wsx();
                render(f, Rect::new(0, 0, w, h), inputs, &theme);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut s = String::new();
        for y in 0..h {
            for x in 0..w {
                s.push_str(buf[(x, y)].symbol());
            }
            s.push('\n');
        }
        s
    }

    fn seed_workspace() -> (
        crate::store::Store,
        crate::store::Repo,
        crate::store::Workspace,
    ) {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "ws",
                branch: "repo/ws",
                worktree_path: std::path::Path::new("/tmp/r/ws"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        store
            .set_workspace_state(id, WorkspaceState::Ready)
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == repo_id)
            .unwrap();
        let ws = store
            .workspaces(repo_id)
            .unwrap()
            .into_iter()
            .find(|w| w.id == id)
            .unwrap();
        (store, repo, ws)
    }

    #[test]
    fn render_into_zero_area_is_a_noop() {
        // Sanity: rendering into a zero-height area must not panic.
        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        let (_store, repo, ws) = seed_workspace();
        let result = terminal.draw(|f| {
            let theme = Theme::wsx();
            let cfg = DetailBarConfig::default();
            let inputs = DetailInputs {
                repo: &repo,
                workspace: &ws,
                events: None,
                procs: &[],
                diff: None,
                diff_per_file: None,
                lifecycle: None,
                pr_title: None,
                pr_number: None,
                status: Status::Idle,
                ago_secs: None,
                reply_draft: "",
                reply_focused: false,
                events_scanned: false,
                config: &cfg,
            };
            render(f, Rect::new(0, 0, 80, 0), &inputs, &theme);
        });
        assert!(result.is_ok());
    }

    #[test]
    fn header_strip_contains_all_chips_in_order() {
        let theme = Theme::wsx();
        let line = build_header_strip(
            "repo-overview",
            "bakedbean/repo-overview",
            Some(BranchLifecycle::PrOpen),
            Some(DiffStats {
                added: 12,
                removed: 3,
            }),
            2,
            Status::Question,
            Some(29),
            &theme,
            120,
        );
        let text = line_to_string(&line);
        assert!(text.contains("repo-overview"), "name missing: {text:?}");
        assert!(
            text.contains("bakedbean/repo-overview"),
            "branch missing: {text:?}"
        );
        assert!(
            text.contains("+12") && text.contains("−3"),
            "diff missing: {text:?}"
        );
        assert!(
            text.contains("● 2") || text.contains("2 procs"),
            "procs missing: {text:?}"
        );
        assert!(text.contains("?"), "status glyph missing: {text:?}");
        assert!(text.contains("29s"), "ago missing: {text:?}");
    }

    #[test]
    fn header_strip_omits_diff_when_none() {
        let theme = Theme::wsx();
        let line = build_header_strip("ws", "br", None, None, 0, Status::Idle, None, &theme, 80);
        let text = line_to_string(&line);
        assert!(!text.contains("+"), "diff cell should be absent: {text:?}");
        assert!(!text.contains("−"), "diff cell should be absent: {text:?}");
    }

    #[test]
    fn header_strip_omits_lifecycle_when_none() {
        let theme = Theme::wsx();
        let line = build_header_strip("ws", "br", None, None, 0, Status::Idle, None, &theme, 80);
        let text = line_to_string(&line);
        // The PR lifecycle glyph set is { ⏺, ⏵, ⏷, ⏸ } (any specific
        // mapping in theme); none should appear when lifecycle is None.
        // Use a simple proxy: there's no "PR" or "open"/"merged" label.
        let lower = text.to_lowercase();
        assert!(!lower.contains("pr open"), "no pr label: {text:?}");
        assert!(!lower.contains("merged"), "no pr label: {text:?}");
    }

    fn make_events_with(
        first: Option<&str>,
        counts: ToolUseCounts,
        last_assistant: Option<&str>,
    ) -> WorkspaceEvents {
        WorkspaceEvents {
            first_user_text: first.map(str::to_string),
            tool_use_counts: counts,
            last_assistant_text: last_assistant.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn session_summary_renders_initial_prompt_when_present() {
        let theme = Theme::wsx();
        let evt = make_events_with(Some("summarize the repo"), ToolUseCounts::default(), None);
        let lines = build_session_summary(Some(&evt), Status::Idle, &theme, 50, 0, None);
        let joined: String = lines
            .iter()
            .map(line_to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("summarize the repo"), "{joined:?}");
    }

    #[test]
    fn session_summary_tool_trace_omits_zero_counts() {
        let theme = Theme::wsx();
        let evt = make_events_with(
            None,
            ToolUseCounts {
                read: 5,
                edit: 0,
                write: 0,
                bash: 2,
                other: 0,
            },
            None,
        );
        let lines = build_session_summary(Some(&evt), Status::Idle, &theme, 50, 0, None);
        let joined: String = lines
            .iter()
            .map(line_to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("read 5 files"), "{joined:?}");
        assert!(joined.contains("ran 2 commands"), "{joined:?}");
        assert!(
            !joined.contains("edited"),
            "edit fragment should be omitted: {joined:?}"
        );
        assert!(
            !joined.contains("wrote"),
            "write fragment should be omitted: {joined:?}"
        );
    }

    #[test]
    fn session_summary_singular_plural_forms() {
        let theme = Theme::wsx();
        let evt = make_events_with(
            None,
            ToolUseCounts {
                read: 1,
                edit: 1,
                write: 1,
                bash: 1,
                other: 1,
            },
            None,
        );
        let lines = build_session_summary(Some(&evt), Status::Idle, &theme, 100, 0, None);
        let joined: String = lines
            .iter()
            .map(line_to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("read 1 file") && !joined.contains("read 1 files"),
            "{joined:?}"
        );
        assert!(joined.contains("edited 1 file"), "{joined:?}");
        assert!(joined.contains("ran 1 command"), "{joined:?}");
    }

    #[test]
    fn session_summary_shows_loading_when_events_none() {
        let theme = Theme::wsx();
        let lines = build_session_summary(None, Status::Idle, &theme, 50, 0, None);
        let joined: String = lines
            .iter()
            .map(line_to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("loading"), "{joined:?}");
    }

    #[test]
    fn session_summary_respects_newlines_in_initial_prompt() {
        // A prompt with embedded newlines should render across multiple
        // lines (one per paragraph), with continuation lines indented
        // to align with the first character after the bullet prefix.
        let theme = Theme::wsx();
        let evt = make_events_with(
            Some("first line\nsecond line\nthird line"),
            ToolUseCounts::default(),
            None,
        );
        let lines = build_session_summary(Some(&evt), Status::Idle, &theme, 60, 0, None);
        let joined: String = lines
            .iter()
            .map(line_to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("first line"), "first paragraph: {joined:?}");
        assert!(
            joined.contains("second line"),
            "second paragraph: {joined:?}"
        );
        assert!(joined.contains("third line"), "third paragraph: {joined:?}");
        // Label + 3 prompt lines + 1 tool-trace line = 5 lines minimum.
        assert!(lines.len() >= 5, "expected >=5 lines, got {}", lines.len());
    }

    #[test]
    fn session_summary_wraps_long_single_line_prompts() {
        // A single-line prompt longer than the column should wrap so
        // the user can see the whole prompt instead of an ellipsis.
        let theme = Theme::wsx();
        let long_prompt =
            "summarize the entire repository in extreme detail and list every public symbol";
        let evt = make_events_with(Some(long_prompt), ToolUseCounts::default(), None);
        let lines = build_session_summary(Some(&evt), Status::Idle, &theme, 30, 0, None);
        let joined: String = lines
            .iter()
            .map(line_to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("summarize the entire"),
            "head visible: {joined:?}"
        );
        assert!(
            joined.contains("every public symbol"),
            "tail visible too: {joined:?}"
        );
    }

    #[test]
    fn session_summary_renders_created_and_active_ages() {
        let theme = Theme::wsx();
        let evt = make_events_with(Some("hi"), ToolUseCounts::default(), None);
        // 2 hours since creation, 45 seconds since last activity.
        let lines =
            build_session_summary(Some(&evt), Status::Idle, &theme, 50, 2 * 60 * 60, Some(45));
        let joined: String = lines
            .iter()
            .map(line_to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("created 2h ago"),
            "created line: {joined:?}"
        );
        assert!(joined.contains("active 45s ago"), "active line: {joined:?}");
    }

    #[test]
    fn session_summary_active_dash_when_no_session() {
        let theme = Theme::wsx();
        let evt = make_events_with(None, ToolUseCounts::default(), None);
        let lines = build_session_summary(Some(&evt), Status::Idle, &theme, 50, 60, None);
        let joined: String = lines
            .iter()
            .map(line_to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("active —"),
            "active dash when no session: {joined:?}"
        );
    }

    #[test]
    fn session_summary_age_lines_render_even_while_loading() {
        // The created/active lines come from workspace + session data,
        // not from JSONL events, so they should render before the
        // events tail has scanned.
        let theme = Theme::wsx();
        let lines = build_session_summary(None, Status::Idle, &theme, 50, 300, Some(10));
        let joined: String = lines
            .iter()
            .map(line_to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("loading"),
            "loading placeholder: {joined:?}"
        );
        assert!(
            joined.contains("created 5m ago"),
            "created visible: {joined:?}"
        );
        assert!(
            joined.contains("active 10s ago"),
            "active visible: {joined:?}"
        );
    }

    #[test]
    fn session_summary_empty_tool_counts_renders_em_dash() {
        // Per spec: empty tool_use_counts shows a faint `—` so the
        // column structure stays consistent across workspace ages.
        let theme = Theme::wsx();
        let evt = make_events_with(None, ToolUseCounts::default(), None);
        let lines = build_session_summary(Some(&evt), Status::Idle, &theme, 50, 0, None);
        let joined: String = lines
            .iter()
            .map(line_to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("—"),
            "expected em-dash placeholder: {joined:?}"
        );
    }

    #[test]
    fn session_summary_prefix_uses_workspace_status_color() {
        // Per spec: `▸` prefix lines render in the workspace's status color.
        let theme = Theme::wsx();
        let evt = make_events_with(Some("hello"), ToolUseCounts::default(), None);
        let lines = build_session_summary(Some(&evt), Status::Question, &theme, 50, 0, None);
        let expected_fg = theme.status_style(Status::Question).fg;
        let prefix_span = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content.as_ref() == "▸ ")
            .expect("at least one prefix span");
        assert_eq!(
            prefix_span.style.fg, expected_fg,
            "prefix not in status color"
        );
    }

    #[test]
    fn recent_chat_renders_em_dash_when_no_assistant_text() {
        let theme = Theme::wsx();
        let evt = make_events_with(None, ToolUseCounts::default(), None);
        let lines = build_recent_chat(Some(&evt), &theme, 40, 6);
        let joined: String = lines
            .iter()
            .map(line_to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("—"), "{joined:?}");
    }

    #[test]
    fn recent_chat_renders_assistant_text_wrapped() {
        let theme = Theme::wsx();
        let evt = make_events_with(
            None,
            ToolUseCounts::default(),
            Some(
                "This is a longer assistant message that should wrap across multiple lines when the column width is small.",
            ),
        );
        let lines = build_recent_chat(Some(&evt), &theme, 30, 6);
        // Expect at least 2 lines (label + ≥1 content line); total ≤ 1 (label) + 6 (max).
        assert!(
            lines.len() >= 2 && lines.len() <= 7,
            "got {} lines",
            lines.len()
        );
        let joined: String = lines
            .iter()
            .map(line_to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("longer assistant"),
            "content present: {joined:?}"
        );
    }

    #[test]
    fn recent_chat_shows_loading_when_events_none() {
        let theme = Theme::wsx();
        let lines = build_recent_chat(None, &theme, 40, 6);
        let joined: String = lines
            .iter()
            .map(line_to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("loading"), "{joined:?}");
    }

    fn proc(cmd: &str) -> ProcInfo {
        ProcInfo {
            pid: 1234,
            ppid: 1,
            command: cmd.into(),
            cwd: std::path::PathBuf::from("/tmp"),
        }
    }

    #[test]
    fn procs_column_shows_dash_when_empty() {
        let theme = Theme::wsx();
        let evt = make_events_with(None, ToolUseCounts::default(), None);
        let lines = build_procs_and_files(
            &[],
            Some(&evt),
            None,
            std::path::Path::new("/tmp/r/ws"),
            &theme,
            30,
        );
        let joined: String = lines
            .iter()
            .map(line_to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("—"),
            "expected em-dash when no procs/files: {joined:?}"
        );
    }

    #[test]
    fn procs_column_truncates_with_plus_n_more() {
        let theme = Theme::wsx();
        let evt = make_events_with(None, ToolUseCounts::default(), None);
        let procs: Vec<ProcInfo> = (0..7).map(|i| proc(&format!("cmd{i}"))).collect();
        let lines = build_procs_and_files(
            &procs,
            Some(&evt),
            None,
            std::path::Path::new("/tmp/r/ws"),
            &theme,
            30,
        );
        let joined: String = lines
            .iter()
            .map(line_to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("+2 more"), "expected +2 more: {joined:?}");
    }

    #[test]
    fn recent_files_section_renders_paths_relative_to_worktree() {
        // Paths inside the worktree render relative to its root —
        // not as absolute paths with the shared prefix duplicated on
        // every line.
        let theme = Theme::wsx();
        let mut evt = make_events_with(None, ToolUseCounts::default(), None);
        evt.recent_edited_files
            .push_front("/tmp/wt/src/main.rs".to_string());
        evt.recent_edited_files
            .push_front("/tmp/wt/Cargo.toml".to_string());
        let lines = build_procs_and_files(
            &[],
            Some(&evt),
            None,
            std::path::Path::new("/tmp/wt"),
            &theme,
            40,
        );
        let joined: String = lines
            .iter()
            .map(line_to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("Cargo.toml"),
            "Cargo.toml visible: {joined:?}"
        );
        assert!(
            joined.contains("src/main.rs"),
            "src/main.rs visible: {joined:?}"
        );
        assert!(
            !joined.contains("/tmp/wt"),
            "absolute prefix should be stripped: {joined:?}"
        );
    }

    #[test]
    fn recent_files_section_keeps_absolute_path_when_outside_worktree() {
        // Defensive: if claude somehow logged a path outside the
        // worktree, fall back to the original string instead of
        // hiding it.
        let theme = Theme::wsx();
        let mut evt = make_events_with(None, ToolUseCounts::default(), None);
        evt.recent_edited_files
            .push_front("/etc/passwd".to_string());
        let lines = build_procs_and_files(
            &[],
            Some(&evt),
            None,
            std::path::Path::new("/tmp/wt"),
            &theme,
            40,
        );
        let joined: String = lines
            .iter()
            .map(line_to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("/etc/passwd"),
            "absolute path kept: {joined:?}"
        );
    }

    #[test]
    fn recent_files_section_annotates_with_diff_counts() {
        // RECENT FILES entries get a `+A −R` suffix when the per-file
        // diff map has a matching entry (keyed by path relative to
        // the worktree root).
        let theme = Theme::wsx();
        let mut evt = make_events_with(None, ToolUseCounts::default(), None);
        // Absolute paths inside the worktree, matching how the JSONL
        // `file_path` field stores them.
        evt.recent_edited_files
            .push_front("/tmp/wt/src/main.rs".to_string());
        evt.recent_edited_files
            .push_front("/tmp/wt/Cargo.toml".to_string());
        let mut diffs = std::collections::HashMap::new();
        diffs.insert(
            "src/main.rs".to_string(),
            DiffStats {
                added: 12,
                removed: 3,
            },
        );
        diffs.insert(
            "Cargo.toml".to_string(),
            DiffStats {
                added: 1,
                removed: 0,
            },
        );
        let lines = build_procs_and_files(
            &[],
            Some(&evt),
            Some(&diffs),
            std::path::Path::new("/tmp/wt"),
            &theme,
            40,
        );
        let joined: String = lines
            .iter()
            .map(line_to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("+12") && joined.contains("−3"),
            "main.rs delta: {joined:?}"
        );
        assert!(
            joined.contains("+1") && joined.contains("−0"),
            "Cargo.toml delta: {joined:?}"
        );
    }

    #[test]
    fn recent_files_section_omits_diff_when_no_match() {
        // A file not in the per-file map renders without a delta
        // suffix (no fake +0 −0).
        let theme = Theme::wsx();
        let mut evt = make_events_with(None, ToolUseCounts::default(), None);
        evt.recent_edited_files
            .push_front("/tmp/wt/some/untracked.txt".to_string());
        let diffs: std::collections::HashMap<String, DiffStats> = std::collections::HashMap::new();
        let lines = build_procs_and_files(
            &[],
            Some(&evt),
            Some(&diffs),
            std::path::Path::new("/tmp/wt"),
            &theme,
            40,
        );
        let joined: String = lines
            .iter()
            .map(line_to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("untracked.txt"), "path present: {joined:?}");
        assert!(!joined.contains("+0"), "no fake delta: {joined:?}");
        assert!(!joined.contains("−0"), "no fake delta: {joined:?}");
    }

    #[test]
    fn reply_input_row_shows_chip_and_draft() {
        let theme = Theme::wsx();
        let line = build_reply_row("hello agent", false, &theme, 80);
        let text = line_to_string(&line);
        assert!(text.contains("Reply to agent"), "chip present: {text:?}");
        assert!(text.contains("hello agent"), "draft present: {text:?}");
    }

    #[test]
    fn reply_input_row_shows_send_hint_when_focused() {
        let theme = Theme::wsx();
        let line = build_reply_row("", true, &theme, 80);
        let text = line_to_string(&line);
        assert!(
            text.contains("send"),
            "send hint present when focused: {text:?}"
        );
        assert!(
            text.contains("cancel"),
            "cancel hint present when focused: {text:?}"
        );
    }

    #[test]
    fn reply_input_row_hides_hints_when_unfocused() {
        let theme = Theme::wsx();
        let line = build_reply_row("", false, &theme, 80);
        let text = line_to_string(&line);
        assert!(
            !text.contains("send"),
            "send hint absent when unfocused: {text:?}"
        );
        assert!(
            !text.contains("cancel"),
            "cancel hint absent when unfocused: {text:?}"
        );
    }

    #[test]
    fn reply_input_row_scrolls_long_drafts_to_end() {
        // A long draft must show its END (where the cursor lives), not
        // its beginning — otherwise the user can't see what they're typing.
        let theme = Theme::wsx();
        let long: String = "a".repeat(60);
        // Construct with " END" appended so we can detect that the tail is visible.
        let draft = format!("{long} END");
        let line = build_reply_row(&draft, true, &theme, 60);
        let text = line_to_string(&line);
        assert!(text.contains("END"), "tail of draft visible: {text:?}");
    }

    #[test]
    fn full_render_paints_header_body_and_reply_row() {
        let (_store, repo, ws) = seed_workspace();
        let evt = WorkspaceEvents {
            first_user_text: Some("give me a tour".into()),
            tool_use_counts: ToolUseCounts {
                read: 14,
                bash: 2,
                ..Default::default()
            },
            last_assistant_text: Some("Reading the repo now.".into()),
            ..Default::default()
        };
        let cfg = DetailBarConfig::default();
        let inputs = DetailInputs {
            repo: &repo,
            workspace: &ws,
            events: Some(&evt),
            procs: &[],
            diff: Some(DiffStats {
                added: 12,
                removed: 3,
            }),
            diff_per_file: None,
            lifecycle: Some(BranchLifecycle::PrOpen),
            pr_title: None,
            pr_number: None,
            status: Status::Question,
            ago_secs: Some(29),
            reply_draft: "",
            reply_focused: false,
            events_scanned: true,
            config: &cfg,
        };
        let text = render_to_text(&inputs, 120, 10);
        assert!(
            text.contains("repo-overview") || text.contains("ws"),
            "header name: {text:?}"
        );
        assert!(text.contains("SESSION SUMMARY"), "summary label: {text:?}");
        assert!(text.contains("RECENT CHAT"), "chat label: {text:?}");
        assert!(text.contains("PROCESSES"), "procs label: {text:?}");
        assert!(text.contains("Reply to agent"), "reply chip: {text:?}");
        assert!(text.contains("give me a tour"), "initial prompt: {text:?}");
        assert!(text.contains("Reading the repo"), "recent chat: {text:?}");
    }

    #[test]
    fn narrow_terminal_drops_chat_and_procs_columns() {
        let (_store, repo, ws) = seed_workspace();
        let evt = WorkspaceEvents {
            first_user_text: Some("hi".into()),
            last_assistant_text: Some("ack".into()),
            ..Default::default()
        };
        let cfg = DetailBarConfig::default();
        let inputs = DetailInputs {
            repo: &repo,
            workspace: &ws,
            events: Some(&evt),
            procs: &[],
            diff: None,
            diff_per_file: None,
            lifecycle: None,
            pr_title: None,
            pr_number: None,
            status: Status::Idle,
            ago_secs: None,
            reply_draft: "",
            reply_focused: false,
            events_scanned: true,
            config: &cfg,
        };
        let text = render_to_text(&inputs, 70, 10);
        assert!(text.contains("SESSION SUMMARY"), "summary kept: {text:?}");
        assert!(
            !text.contains("RECENT CHAT"),
            "chat dropped on narrow: {text:?}"
        );
        assert!(
            !text.contains("PROCESSES"),
            "procs dropped on narrow: {text:?}"
        );
    }

    use crate::detail_bar_config::{DetailBarConfig, Sections};

    #[test]
    fn renders_three_columns_with_default_config() {
        let cfg = DetailBarConfig::default();
        assert!(cfg.has_body());
        assert!(cfg.sections.session_summary);
        assert!(cfg.sections.recent_chat);
        assert!(cfg.sections.procs_and_files);
    }

    #[test]
    fn enabled_columns_helper_returns_subset() {
        let cfg = DetailBarConfig {
            sections: Sections {
                session_summary: true,
                recent_chat: false,
                procs_and_files: true,
            },
            ..DetailBarConfig::default()
        };
        let cols = enabled_columns(&cfg);
        assert_eq!(cols, vec![Column::SessionSummary, Column::ProcsAndFiles]);
    }

    #[test]
    fn enabled_columns_empty_when_all_disabled() {
        let cfg = DetailBarConfig {
            sections: Sections {
                session_summary: false,
                recent_chat: false,
                procs_and_files: false,
            },
            ..DetailBarConfig::default()
        };
        assert!(enabled_columns(&cfg).is_empty());
    }

    #[test]
    fn column_widths_three_cols_match_legacy() {
        assert_eq!(
            column_widths(&[
                Column::SessionSummary,
                Column::RecentChat,
                Column::ProcsAndFiles
            ]),
            vec![30u16, 40, 30]
        );
    }

    #[test]
    fn column_widths_two_cols_summary_chat() {
        assert_eq!(
            column_widths(&[Column::SessionSummary, Column::RecentChat]),
            vec![43u16, 57]
        );
    }

    #[test]
    fn column_widths_two_cols_summary_procs() {
        assert_eq!(
            column_widths(&[Column::SessionSummary, Column::ProcsAndFiles]),
            vec![50u16, 50]
        );
    }

    #[test]
    fn column_widths_two_cols_chat_procs() {
        assert_eq!(
            column_widths(&[Column::RecentChat, Column::ProcsAndFiles]),
            vec![57u16, 43]
        );
    }

    #[test]
    fn column_widths_single_col_is_full() {
        assert_eq!(column_widths(&[Column::RecentChat]), vec![100u16]);
    }
}
