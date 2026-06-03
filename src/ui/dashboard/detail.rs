//! Bottom-pinned detail bar shown when a workspace is selected on the
//! dashboard. Renders header strip, a 1–4 container body (each
//! container holding one or more modules from `crate::detail_modules`),
//! and an inline reply input.
//!
//! See `docs/superpowers/specs/2026-05-25-detail-bar-modules-design.md`.

use crate::activity::events::WorkspaceEvents;
use crate::activity::proc::ProcInfo;
use crate::config::detail_bar_config::DetailBarConfig;
use crate::data::store::{Repo, Workspace};
use crate::git::DiffStats;
use crate::git::forge::BranchLifecycle;
use crate::ui::dashboard::status::Status;
use crate::ui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;

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
    pub pr_title: Option<&'a str>,
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
    pub registry: &'a crate::detail_modules::Registry,
    /// Pinned commands resolved for the selected workspace's repo. When
    /// empty, no chip row is rendered.
    pub pinned: &'a [crate::commands::pinned::PinnedCommand],
    /// Per-slot scroll offsets. Borrowed mutably so the container can
    /// clamp them to the current content height during render.
    pub scroll_offsets: &'a mut [u16; 4],
}

#[derive(Debug, Default)]
pub struct DetailDrawOutput {
    pub chip_rects: Vec<ratatui::layout::Rect>,
    pub container_rects: [Option<ratatui::layout::Rect>; 4],
}

/// Render the detail bar into `area`. No-op when `area.height` is below
/// the config's `minimum_height()` — which is `CHROME_ROWS` (4) when no
/// container has any modules, or `min_rows` otherwise (caller is expected
/// to fall back to a condensed banner — see `app.rs::draw`).
pub fn render(
    f: &mut Frame,
    area: Rect,
    inputs: &mut DetailInputs<'_>,
    theme: &Theme,
) -> DetailDrawOutput {
    use ratatui::layout::{Constraint, Direction, Layout};
    use ratatui::widgets::Paragraph;

    let chip_present = !inputs.pinned.is_empty();
    let has_body = inputs.config.has_body();
    // The body region holds the top horizontal rule, container content,
    // and bottom horizontal rule as a single 3+ row strip — so that
    // vertical separators between containers run uninterrupted across
    // all three rows. When `!has_body` it collapses to just the two
    // rule rows (no content between).
    let body_region_rows: u16 = if has_body { 3 } else { 2 };
    let min_rows: u16 = 1 // header
        + body_region_rows
        + if chip_present { 1 } else { 0 } // chip slot
        + 1; // reply
    let needed = inputs.config.minimum_height().max(min_rows);
    if area.height == 0 || area.height < needed {
        return DetailDrawOutput::default();
    }

    let body_region_constraint = if has_body {
        Constraint::Min(body_region_rows)
    } else {
        Constraint::Length(body_region_rows)
    };
    let constraints: Vec<Constraint> = if chip_present {
        vec![
            Constraint::Length(1), // header
            body_region_constraint,
            Constraint::Length(1), // chips
            Constraint::Length(1), // reply
        ]
    } else {
        vec![
            Constraint::Length(1), // header
            body_region_constraint,
            Constraint::Length(1), // reply
        ]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let header_area = chunks[0];
    let body_region = chunks[1];
    let (chip_area, reply_area) = if chip_present {
        (Some(chunks[2]), chunks[3])
    } else {
        (None, chunks[2])
    };

    let header = build_header_strip(
        &inputs.workspace.name,
        &inputs.workspace.branch,
        inputs.lifecycle,
        inputs.diff,
        inputs.procs.len() as u32,
        inputs.status,
        inputs.ago_secs,
        theme,
        header_area.width as usize,
    );
    f.render_widget(Paragraph::new(header), header_area);

    let container_rects = render_body_region(f, body_region, inputs, theme);

    let chip_rects = if let Some(area) = chip_area {
        crate::ui::attached::render_chip_row(f, area, inputs.pinned, theme)
    } else {
        Vec::new()
    };

    let reply = build_reply_row(
        inputs.reply_draft,
        inputs.reply_focused,
        theme,
        reply_area.width as usize,
    );
    f.render_widget(Paragraph::new(reply), reply_area);

    if inputs.reply_focused {
        let cx = reply_cursor_x(inputs.reply_draft, reply_area.width as usize);
        f.set_cursor_position((reply_area.x + cx, reply_area.y));
    }

    DetailDrawOutput {
        chip_rects,
        container_rects,
    }
}

fn render_body_region(
    f: &mut Frame,
    area: Rect,
    inputs: &mut DetailInputs<'_>,
    theme: &Theme,
) -> [Option<Rect>; 4] {
    use ratatui::layout::{Constraint, Direction, Layout};
    use ratatui::widgets::Paragraph;

    let mut rects: [Option<Rect>; 4] = [None; 4];
    if area.height < 2 || area.width == 0 {
        return rects;
    }
    let cfg = inputs.config;

    // Always draw full-width top and bottom horizontal rules. When
    // multiple containers are present, the vertical separators drawn
    // below overwrite the rule's `─` cells with `┬` / `┴` junctions
    // so both lines stay visually continuous through the intersection.
    let rule_style = theme.dim_style();
    let rule_line = Line::from(Span::styled("─".repeat(area.width as usize), rule_style));
    let top_rule = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    };
    let bottom_rule = Rect {
        x: area.x,
        y: area.y + area.height - 1,
        width: area.width,
        height: 1,
    };
    f.render_widget(Paragraph::new(rule_line.clone()), top_rule);
    f.render_widget(Paragraph::new(rule_line), bottom_rule);

    if !cfg.has_body() {
        return rects;
    }

    // Narrow-terminal collapse: < 80 cols → first non-empty container only.
    let containers: Vec<&Vec<String>> = if area.width < 80 {
        cfg.containers
            .iter()
            .find(|c| !c.is_empty())
            .into_iter()
            .collect()
    } else {
        cfg.containers.iter().collect()
    };

    let n = containers.len();
    if n == 0 {
        return rects;
    }

    // Horizontal split: N columns share the remaining width equally
    // (Fill(1)), with a single 1-cell separator chunk between each
    // pair. No additional gap cells — module renderers already pad
    // their content internally.
    let mut h_constraints: Vec<Constraint> = Vec::with_capacity(2 * n - 1);
    for i in 0..n {
        if i > 0 {
            h_constraints.push(Constraint::Length(1));
        }
        h_constraints.push(Constraint::Fill(1));
    }
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(h_constraints)
        .split(area);

    let ctx = crate::detail_modules::DetailContext {
        repo: inputs.repo,
        workspace: inputs.workspace,
        events: inputs.events,
        procs: inputs.procs,
        diff: inputs.diff,
        diff_per_file: inputs.diff_per_file,
        lifecycle: inputs.lifecycle,
        pr_title: inputs.pr_title,
        pr_number: inputs.pr_number,
        status: inputs.status,
        ago_secs: inputs.ago_secs,
        events_scanned: inputs.events_scanned,
        theme,
    };
    let registry = inputs.registry;
    let scroll_offsets: &mut [u16; 4] = inputs.scroll_offsets;

    // Container content sits between the two rule rows. Column i sits
    // at chunk i*2 (pattern: col, sep, col, sep, col, …).
    for (i, ids) in containers.iter().enumerate() {
        let col = h_chunks[i * 2];
        let content = Rect {
            x: col.x,
            y: col.y + 1,
            width: col.width,
            height: col.height.saturating_sub(2),
        };
        let slot_idx = if area.width < 80 {
            // Narrow-collapse path: only the first non-empty container renders.
            // Its slot index in the App-state array is its original position
            // in cfg.containers — not its position in the filtered view.
            cfg.containers
                .iter()
                .position(|c| !c.is_empty())
                .unwrap_or(0)
        } else {
            // Normal path: container order is preserved, slot = loop index.
            i
        };
        if slot_idx < 4 {
            render_container(
                f,
                content,
                ids,
                &ctx,
                registry,
                theme,
                &mut scroll_offsets[slot_idx],
            );
            rects[slot_idx] = Some(content);
        }
    }

    // Vertical separators run the FULL body-region height. At the top
    // and bottom rule rows we draw `┬` / `┴` instead of `│` so the
    // horizontal rule maintains visual continuity through the
    // intersection (its arms tie into the junction's horizontal arms).
    for i in 1..n {
        let sep_area = h_chunks[i * 2 - 1];
        let last = sep_area.height.saturating_sub(1);
        let sep_lines: Vec<Line<'static>> = (0..sep_area.height)
            .map(|row| {
                let glyph = if row == 0 {
                    "┬"
                } else if row == last {
                    "┴"
                } else {
                    "│"
                };
                Line::from(Span::styled(glyph.to_string(), rule_style))
            })
            .collect();
        f.render_widget(Paragraph::new(sep_lines), sep_area);
    }

    rects
}

fn render_container(
    f: &mut Frame,
    area: Rect,
    module_ids: &[String],
    ctx: &crate::detail_modules::DetailContext<'_>,
    reg: &crate::detail_modules::Registry,
    theme: &Theme,
    offset: &mut u16,
) {
    use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

    if module_ids.is_empty() || area.height == 0 || area.width == 0 {
        return;
    }

    // Reserve the rightmost column for the scrollbar so column width stays
    // stable regardless of whether content currently overflows.
    let content_width = area.width.saturating_sub(1);
    let content_area = Rect {
        x: area.x,
        y: area.y,
        width: content_width,
        height: area.height,
    };
    let bar_area = Rect {
        x: area.x + content_width,
        y: area.y,
        width: 1,
        height: area.height,
    };

    let label_style = Style::default().fg(theme.path).add_modifier(Modifier::BOLD);

    // Build virtual line list: title row + body lines + 1-row gap between
    // modules. Last module has no trailing gap.
    let mut virtual_lines: Vec<Line<'static>> = Vec::new();
    let last_idx = module_ids.len().saturating_sub(1);
    for (i, id) in module_ids.iter().enumerate() {
        match reg.get(id) {
            Some(m) => {
                virtual_lines.push(Line::from(Span::styled(m.title(), label_style)));
                virtual_lines.extend(m.lines(ctx, content_width));
            }
            None => {
                tracing::warn!(id = %id, "detail_bar: unknown module id in container");
                virtual_lines.push(Line::from(Span::styled(
                    format!("[unknown: {id}]"),
                    theme.dim_style(),
                )));
            }
        }
        if i != last_idx {
            virtual_lines.push(Line::from(""));
        }
    }

    let content_height: u16 = virtual_lines.len().min(u16::MAX as usize) as u16;
    let max_offset = content_height.saturating_sub(area.height);
    if *offset > max_offset {
        *offset = max_offset;
    }

    let start = *offset as usize;
    let end = (start + area.height as usize).min(virtual_lines.len());
    let visible: Vec<Line<'static>> = virtual_lines[start..end].to_vec();
    f.render_widget(Paragraph::new(visible), content_area);

    if content_height > area.height {
        let mut state = ScrollbarState::new(content_height as usize)
            .position(*offset as usize)
            .viewport_content_length(area.height as usize);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None);
        f.render_stateful_widget(scrollbar, bar_area, &mut state);
    }
}

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

const GUTTER: &str = "▍";

/// One-line header strip at the top of the bar.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_header_strip(
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

/// Render the PROCESSES module body. Returns one row per process
/// (capped at 5, with a "+N more" suffix when over the cap), or a
/// single "—" placeholder when empty. The host (`render_container`)
/// draws the title row separately.
pub(crate) fn build_processes(
    procs: &[ProcInfo],
    theme: &Theme,
    column_width: usize,
) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
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
    out
}

/// Render the RECENT FILES module body. Returns one row per file
/// (capped at 5), each annotated with per-file diff stats when
/// available, or a single "—" placeholder when empty. The host
/// (`render_container`) draws the title row separately.
pub(crate) fn build_recent_files(
    events: Option<&WorkspaceEvents>,
    diff_per_file: Option<&std::collections::HashMap<String, DiffStats>>,
    worktree_path: &std::path::Path,
    theme: &Theme,
    column_width: usize,
) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let files: Vec<&String> = events
        .map(|e| e.recent_edited_files.iter().collect())
        .unwrap_or_default();
    if files.is_empty() {
        out.push(Line::from(Span::styled("—".to_string(), theme.dim_style())));
    } else {
        for f in files.iter().take(5) {
            let diff = lookup_file_diff(f, worktree_path, diff_per_file);
            // The right-aligned `+added −removed` cluster, when present.
            let counts = match diff {
                Some(d) if d.added > 0 || d.removed > 0 => {
                    Some((format!("+{}", d.added), format!("−{}", d.removed)))
                }
                _ => None,
            };
            // Visible width of that cluster ("+A" + space + "−R").
            let cluster_width = counts
                .as_ref()
                .map(|(added, removed)| added.chars().count() + 1 + removed.chars().count())
                .unwrap_or(0);
            // Reserve the cluster plus a minimum 2-space gutter so the
            // path can never butt up against the counts. With no counts
            // the path takes the full width — no gutter to reserve.
            let path_width = if counts.is_some() {
                column_width.saturating_sub(cluster_width + 2)
            } else {
                column_width
            };
            let display = display_relative_path(f, worktree_path);
            let truncated = truncate_to_chars_left(&display, path_width);
            let mut spans: Vec<Span<'static>> =
                vec![Span::styled(truncated.clone(), theme.dim_style())];
            if let Some((added, removed)) = counts {
                // Pad so the cluster's last glyph lands exactly at
                // `column_width`, keeping counts flush-right across rows.
                let pad = column_width.saturating_sub(cluster_width + truncated.chars().count());
                spans.push(Span::raw(" ".repeat(pad)));
                spans.push(Span::styled(added, theme.ok_style()));
                spans.push(Span::raw(" ".to_string()));
                spans.push(Span::styled(removed, theme.err_style()));
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
pub(crate) fn build_reply_row(
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

    fn make_registry() -> crate::detail_modules::Registry {
        let mut reg = crate::detail_modules::Registry::new();
        crate::detail_modules::register_builtins(&mut reg);
        reg
    }

    fn render_to_text(inputs: &mut DetailInputs<'_>, w: u16, h: u16) -> String {
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
        crate::data::store::Store,
        crate::data::store::Repo,
        crate::data::store::Workspace,
    ) {
        use crate::data::store::{NewWorkspace, Store, WorkspaceState};
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
        let reg = make_registry();
        let result = terminal.draw(|f| {
            let theme = Theme::wsx();
            let cfg = DetailBarConfig::default();
            let mut offsets = [0u16; 4];
            let mut inputs = DetailInputs {
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
                registry: &reg,
                pinned: &[],
                scroll_offsets: &mut offsets,
            };
            render(f, Rect::new(0, 0, 80, 0), &mut inputs, &theme);
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
        let evt = crate::activity::events::WorkspaceEvents {
            first_user_text: Some("give me a tour".into()),
            tool_use_counts: crate::activity::events::ToolUseCounts {
                read: 14,
                bash: 2,
                ..Default::default()
            },
            last_assistant_text: Some("Reading the repo now.".into()),
            ..Default::default()
        };
        let cfg = DetailBarConfig::default();
        let reg = make_registry();
        let mut offsets = [0u16; 4];
        let mut inputs = DetailInputs {
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
            registry: &reg,
            pinned: &[],
            scroll_offsets: &mut offsets,
        };
        let text = render_to_text(&mut inputs, 120, 10);
        assert!(
            text.contains("repo-overview") || text.contains("ws"),
            "header name: {text:?}"
        );
        assert!(text.contains("SESSION SUMMARY"), "summary label: {text:?}");
        assert!(text.contains("RECENT CHAT"), "chat label: {text:?}");
        assert!(text.contains("PROCESSES"), "procs label: {text:?}");
        assert!(text.contains("Reply to agent"), "reply chip: {text:?}");
    }

    #[test]
    fn chrome_only_mode_renders_header_and_reply_no_body_labels() {
        let (_store, repo, ws) = seed_workspace();
        let evt = crate::activity::events::WorkspaceEvents {
            first_user_text: Some("hi".into()),
            last_assistant_text: Some("ack".into()),
            ..Default::default()
        };
        // All containers empty — bar should collapse to 4
        // rows (header + 2 rules + reply input). Use all-empty inner
        // lists (sanitize resets an empty outer vec to defaults, but
        // empty inner lists are preserved as-is).
        let cfg = DetailBarConfig {
            containers: vec![vec![], vec![], vec![]],
            ..DetailBarConfig::default()
        };
        let reg = make_registry();
        let mut offsets = [0u16; 4];
        let mut inputs = DetailInputs {
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
            registry: &reg,
            pinned: &[],
            scroll_offsets: &mut offsets,
        };
        // Width 100, height exactly CHROME_ROWS (4).
        let text = render_to_text(&mut inputs, 100, DetailBarConfig::CHROME_ROWS);
        assert!(text.contains("Reply to agent"), "reply chip: {text:?}");
        assert!(
            !text.contains("SESSION SUMMARY"),
            "no summary label: {text:?}"
        );
        assert!(!text.contains("RECENT CHAT"), "no chat label: {text:?}");
        assert!(!text.contains("PROCESSES"), "no procs label: {text:?}");
        assert!(
            !text.contains("give me a tour"),
            "no initial-prompt body: {text:?}"
        );
    }

    #[test]
    fn narrow_terminal_drops_chat_and_procs_columns() {
        let (_store, repo, ws) = seed_workspace();
        let evt = crate::activity::events::WorkspaceEvents {
            first_user_text: Some("hi".into()),
            last_assistant_text: Some("ack".into()),
            ..Default::default()
        };
        let cfg = DetailBarConfig::default();
        let reg = make_registry();
        let mut offsets = [0u16; 4];
        let mut inputs = DetailInputs {
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
            registry: &reg,
            pinned: &[],
            scroll_offsets: &mut offsets,
        };
        let text = render_to_text(&mut inputs, 70, 10);
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

    #[test]
    fn renders_three_columns_with_default_config() {
        let cfg = DetailBarConfig::default();
        assert!(cfg.has_body());
        // Default containers: session_summary, recent_chat, processes+recent_files
        assert_eq!(cfg.containers.len(), 3);
        assert!(cfg.containers[0].contains(&"session_summary".to_string()));
        assert!(cfg.containers[1].contains(&"recent_chat".to_string()));
        assert!(cfg.containers[2].contains(&"processes".to_string()));
    }

    #[test]
    fn render_with_unknown_module_id_shows_placeholder() {
        let (_store, repo, ws) = seed_workspace();
        let cfg = DetailBarConfig {
            containers: vec![vec!["seshun_summary".into()]],
            ..Default::default()
        };
        let reg = make_registry();
        let mut offsets = [0u16; 4];
        let mut inputs = DetailInputs {
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
            events_scanned: true,
            config: &cfg,
            registry: &reg,
            pinned: &[],
            scroll_offsets: &mut offsets,
        };
        let text = render_to_text(&mut inputs, 120, 10);
        assert!(
            text.contains("[unknown: seshun_summary]"),
            "expected unknown placeholder in: {text:?}",
        );
    }

    #[test]
    fn render_one_container_fills_full_width() {
        let (_store, repo, ws) = seed_workspace();
        let evt = crate::activity::events::WorkspaceEvents {
            last_assistant_text: Some("hello".into()),
            ..Default::default()
        };
        let cfg = DetailBarConfig {
            containers: vec![vec!["recent_chat".into()]],
            ..Default::default()
        };
        let reg = make_registry();
        let mut offsets = [0u16; 4];
        let mut inputs = DetailInputs {
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
            registry: &reg,
            pinned: &[],
            scroll_offsets: &mut offsets,
        };
        let text = render_to_text(&mut inputs, 120, 10);
        assert!(text.contains("RECENT CHAT"), "chat title: {text:?}");
        // Other module titles must NOT appear when only recent_chat is configured.
        assert!(
            !text.contains("SESSION SUMMARY"),
            "summary leaked: {text:?}"
        );
        assert!(!text.contains("PROCESSES"), "procs leaked: {text:?}");
        assert!(!text.contains("RECENT FILES"), "files leaked: {text:?}");
    }

    #[test]
    fn render_with_pinned_includes_chip_row_above_reply() {
        let (_store, repo, ws) = seed_workspace();
        let cfg = DetailBarConfig::default();
        let reg = make_registry();
        let pinned = vec![
            crate::commands::pinned::PinnedCommand {
                label: "PR".into(),
                command: "/pull-request".into(),
            },
            crate::commands::pinned::PinnedCommand {
                label: "FB".into(),
                command: "/feedback".into(),
            },
        ];
        let mut offsets = [0u16; 4];
        let mut inputs = DetailInputs {
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
            events_scanned: true,
            config: &cfg,
            registry: &reg,
            pinned: &pinned,
            scroll_offsets: &mut offsets,
        };
        let text = render_to_text(&mut inputs, 120, 12);
        // Chip labels must appear, and "Reply to agent" must still appear
        // (we only inserted a row, didn't remove the reply row).
        assert!(text.contains("PR"), "chip label PR present: {text:?}");
        assert!(text.contains("FB"), "chip label FB present: {text:?}");
        assert!(
            text.contains("Reply to agent"),
            "reply chip still present: {text:?}"
        );

        // Chip row must sit ABOVE the reply row.
        let pr_line = text
            .lines()
            .position(|l| l.contains(" PR "))
            .expect("PR line");
        let reply_line = text
            .lines()
            .position(|l| l.contains("Reply to agent"))
            .expect("reply line");
        assert!(
            pr_line < reply_line,
            "chip row above reply: pr={pr_line} reply={reply_line}"
        );
    }

    #[test]
    fn render_without_pinned_omits_chip_row() {
        let (_store, repo, ws) = seed_workspace();
        let cfg = DetailBarConfig::default();
        let reg = make_registry();
        let mut offsets = [0u16; 4];
        let mut inputs = DetailInputs {
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
            events_scanned: true,
            config: &cfg,
            registry: &reg,
            pinned: &[],
            scroll_offsets: &mut offsets,
        };
        // Capture render's returned rects via a closure-bound outer mut
        // (Terminal::draw can't propagate values out of its closure).
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 12)).unwrap();
        let mut returned: Vec<ratatui::layout::Rect> = Vec::new();
        terminal
            .draw(|f| {
                let theme = Theme::wsx();
                let out = render(f, Rect::new(0, 0, 120, 12), &mut inputs, &theme);
                returned = out.chip_rects;
            })
            .unwrap();
        assert!(returned.is_empty(), "no chip rects when pinned empty");
    }

    #[test]
    fn render_returns_empty_rects_when_area_too_short_for_chip_row() {
        // Regression guard for the latent cliff flagged in PR #104 review:
        // with chips configured but a chrome-only DetailBarConfig
        // (`minimum_height()` returns CHROME_ROWS == 4), if the available
        // area is 4 rows the layout doesn't fit chrome (5 rows including
        // chip slot). The early-return must bail so we don't return
        // invisible-but-clickable chip rects from a 0-height chunk.
        let (_store, repo, ws) = seed_workspace();
        let cfg = DetailBarConfig {
            containers: vec![vec![], vec![], vec![]], // all empty → no body
            ..DetailBarConfig::default()
        };
        let reg = make_registry();
        let pinned = vec![crate::commands::pinned::PinnedCommand {
            label: "PR".into(),
            command: "/pr".into(),
        }];
        let mut offsets = [0u16; 4];
        let mut inputs = DetailInputs {
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
            events_scanned: true,
            config: &cfg,
            registry: &reg,
            pinned: &pinned,
            scroll_offsets: &mut offsets,
        };
        // Area height exactly CHROME_ROWS (4). With chips present we need 5.
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 4)).unwrap();
        let mut returned: Vec<ratatui::layout::Rect> = Vec::new();
        terminal
            .draw(|f| {
                let theme = Theme::wsx();
                let out = render(f, Rect::new(0, 0, 80, 4), &mut inputs, &theme);
                returned = out.chip_rects;
            })
            .unwrap();
        assert!(
            returned.is_empty(),
            "no chip rects when area is too short to fit chip row + chrome"
        );
    }

    #[test]
    fn build_processes_empty_emits_dash() {
        // Builders return body lines only; the dispatcher draws titles
        // (see render_container). Empty case = single "—" placeholder.
        let theme = Theme::default();
        let lines = build_processes(&[], &theme, 40);
        assert_eq!(lines.len(), 1);
        let placeholder = line_to_string(&lines[0]);
        assert_eq!(placeholder, "—");
    }

    #[test]
    fn build_recent_files_empty_emits_dash() {
        let theme = Theme::default();
        let path = std::path::PathBuf::from("/wt");
        let lines = build_recent_files(None, None, &path, &theme, 40);
        assert_eq!(lines.len(), 1);
        let placeholder = line_to_string(&lines[0]);
        assert_eq!(placeholder, "—");
    }

    #[test]
    fn build_recent_files_right_justifies_diff_counts() {
        // Counts must sit flush against the right edge regardless of how
        // long the path is or how many digits the counts have, so each
        // rendered row fills exactly `column_width` and ends with its
        // own `+added −removed` cluster.
        let theme = Theme::default();
        let worktree = std::path::PathBuf::from("/wt");
        let mut evt = crate::activity::events::WorkspaceEvents::default();
        evt.recent_edited_files
            .push_back("/wt/short.rs".to_string());
        evt.recent_edited_files
            .push_back("/wt/a/longer/nested/path/name.rs".to_string());
        let mut diff = std::collections::HashMap::new();
        diff.insert(
            "short.rs".to_string(),
            DiffStats {
                added: 5,
                removed: 3,
            },
        );
        diff.insert(
            "a/longer/nested/path/name.rs".to_string(),
            DiffStats {
                added: 120,
                removed: 40,
            },
        );

        let column_width = 40;
        let lines = build_recent_files(Some(&evt), Some(&diff), &worktree, &theme, column_width);
        assert_eq!(lines.len(), 2);
        for line in &lines {
            let s = line_to_string(line);
            assert_eq!(
                s.chars().count(),
                column_width,
                "row not right-justified to column_width: {s:?}"
            );
        }
        assert!(line_to_string(&lines[0]).ends_with("+5 −3"));
        assert!(line_to_string(&lines[1]).ends_with("+120 −40"));
    }

    #[test]
    fn build_recent_files_no_diff_uses_full_path_width() {
        // With no per-file diff there is no count cluster, so no gutter
        // should be reserved — a path longer than column_width gets
        // truncated to the full width, not width − 2.
        let theme = Theme::default();
        let worktree = std::path::PathBuf::from("/wt");
        let mut evt = crate::activity::events::WorkspaceEvents::default();
        evt.recent_edited_files
            .push_back("/wt/a/deeply/nested/directory/structure/file.rs".to_string());

        let column_width = 20;
        // No diff map at all → counts == None for every row.
        let lines = build_recent_files(Some(&evt), None, &worktree, &theme, column_width);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            line_to_string(&lines[0]).chars().count(),
            column_width,
            "path should fill the full column when no counts follow it"
        );
    }

    #[test]
    fn body_renders_vertical_separator_between_containers() {
        // Three columns → two vertical rules running floor-to-ceiling
        // of the body region, including the top and bottom horizontal
        // rule rows. Every row between the header and reply must show
        // exactly two `│` glyphs.
        let (_store, repo, ws) = seed_workspace();
        let evt = crate::activity::events::WorkspaceEvents {
            first_user_text: Some("hi".into()),
            last_assistant_text: Some("ack".into()),
            ..Default::default()
        };
        let cfg = DetailBarConfig::default();
        let reg = make_registry();
        let mut offsets = [0u16; 4];
        let mut inputs = DetailInputs {
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
            registry: &reg,
            pinned: &[],
            scroll_offsets: &mut offsets,
        };
        let text = render_to_text(&mut inputs, 120, 10);
        let lines: Vec<&str> = text.lines().collect();
        let reply_idx = lines
            .iter()
            .position(|l| l.contains("Reply to agent"))
            .expect("reply row");
        // Body region: rows 1..reply_idx (after the header, before the
        // reply). Each row carries exactly 2 separator glyphs (3
        // containers → 2 separators), but the glyph differs by row:
        //   - first row (top rule):    `┬`
        //   - middle rows (content):   `│`
        //   - last row (bottom rule):  `┴`
        let body_rows: Vec<&str> = lines[1..reply_idx].to_vec();
        assert!(body_rows.len() >= 3, "body region needs >= 3 rows");
        let last = body_rows.len() - 1;
        for (i, row) in body_rows.iter().enumerate() {
            let total = row
                .chars()
                .filter(|c| matches!(*c, '│' | '┬' | '┴'))
                .count();
            assert_eq!(total, 2, "expected 2 separator glyphs in row {i}: {row:?}");
            let expected = if i == 0 {
                '┬'
            } else if i == last {
                '┴'
            } else {
                '│'
            };
            let kind_count = row.chars().filter(|c| *c == expected).count();
            assert_eq!(kind_count, 2, "expected 2 `{expected}` in row {i}: {row:?}",);
        }
        // The top and bottom rule rows must still show plenty of `─`
        // (otherwise the horizontal rule wouldn't actually be drawn).
        let top_dashes = body_rows[0].chars().filter(|c| *c == '─').count();
        assert!(
            top_dashes >= 10,
            "top rule needs `─` glyphs: {:?}",
            body_rows[0]
        );
        let bot_dashes = body_rows[last].chars().filter(|c| *c == '─').count();
        assert!(
            bot_dashes >= 10,
            "bottom rule needs `─` glyphs: {:?}",
            body_rows[last]
        );
    }

    #[test]
    fn render_container_short_content_no_scrollbar() {
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let reg = make_registry();
        let ids = vec!["processes".to_string()];
        let mut offset: u16 = 0;
        let (_store, repo, workspace) = seed_workspace();
        let theme = Theme::default();
        let ctx = crate::detail_modules::DetailContext {
            repo: &repo,
            workspace: &workspace,
            events: None,
            procs: &[],
            diff: None,
            diff_per_file: None,
            lifecycle: None,
            pr_title: None,
            pr_number: None,
            status: crate::ui::dashboard::status::Status::Idle,
            ago_secs: None,
            events_scanned: true,
            theme: &theme,
        };
        terminal
            .draw(|f| {
                let area = Rect {
                    x: 0,
                    y: 0,
                    width: 40,
                    height: 10,
                };
                render_container(f, area, &ids, &ctx, &reg, &theme, &mut offset);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        // Rightmost column (x=39) should be blank — no scrollbar painted.
        for y in 0..10 {
            let sym = buf[(39, y)].symbol();
            assert_eq!(
                sym, " ",
                "expected blank scrollbar column at row {y}, got {sym:?}"
            );
        }
    }

    #[test]
    fn render_container_tall_content_renders_scrollbar() {
        let backend = TestBackend::new(40, 4); // very short — forces overflow
        let mut terminal = Terminal::new(backend).unwrap();
        let reg = make_registry();
        // Stack two modules so the virtual line list exceeds 4 rows
        // (2 titles + at least 1 body line each + 1 gap = >= 5).
        let ids = vec!["processes".to_string(), "session_summary".to_string()];
        let mut offset: u16 = 0;
        let (_store, repo, workspace) = seed_workspace();
        let theme = Theme::default();
        let ctx = crate::detail_modules::DetailContext {
            repo: &repo,
            workspace: &workspace,
            events: None,
            procs: &[],
            diff: None,
            diff_per_file: None,
            lifecycle: None,
            pr_title: None,
            pr_number: None,
            status: crate::ui::dashboard::status::Status::Idle,
            ago_secs: None,
            events_scanned: true,
            theme: &theme,
        };
        terminal
            .draw(|f| {
                let area = Rect {
                    x: 0,
                    y: 0,
                    width: 40,
                    height: 4,
                };
                render_container(f, area, &ids, &ctx, &reg, &theme, &mut offset);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let any_nonblank = (0..4).any(|y| buf[(39, y)].symbol() != " ");
        assert!(
            any_nonblank,
            "expected scrollbar glyphs in rightmost column"
        );
    }

    #[test]
    fn render_container_clamps_offset_when_content_shrinks() {
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let reg = make_registry();
        let ids = vec!["processes".to_string()];
        let mut offset: u16 = 50; // wildly past end
        let (_store, repo, workspace) = seed_workspace();
        let theme = Theme::default();
        let ctx = crate::detail_modules::DetailContext {
            repo: &repo,
            workspace: &workspace,
            events: None,
            procs: &[],
            diff: None,
            diff_per_file: None,
            lifecycle: None,
            pr_title: None,
            pr_number: None,
            status: crate::ui::dashboard::status::Status::Idle,
            ago_secs: None,
            events_scanned: true,
            theme: &theme,
        };
        terminal
            .draw(|f| {
                let area = Rect {
                    x: 0,
                    y: 0,
                    width: 40,
                    height: 10,
                };
                render_container(f, area, &ids, &ctx, &reg, &theme, &mut offset);
            })
            .unwrap();
        // Title + 1 dash line = 2 rows total. Area is 10 rows. max_offset = 0.
        assert_eq!(offset, 0);
    }
}
