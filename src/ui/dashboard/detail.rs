//! Bottom-pinned detail bar shown when a workspace is selected on the
//! dashboard. Renders header strip, three-column body, and an inline
//! reply input.
//!
//! See `docs/superpowers/specs/2026-05-24-dashboard-workspace-detail-design.md`.

use crate::detail_bar_config::DetailBarConfig;
use crate::events::WorkspaceEvents;
use crate::forge::BranchLifecycle;
use crate::git::DiffStats;
use crate::proc::ProcInfo;
use crate::store::{Repo, Workspace};
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
}

/// Render the detail bar into `area`. No-op when `area.height` is below
/// the config's `minimum_height()` — which is `CHROME_ROWS` (4) when no
/// sections are enabled, or `min_rows` otherwise (caller is expected to
/// fall back to a condensed banner — see `app.rs::draw`).
pub fn render(f: &mut Frame, area: Rect, inputs: &DetailInputs<'_>, theme: &Theme) {
    if area.height == 0 || area.height < inputs.config.minimum_height() {
        return;
    }
    use ratatui::layout::{Constraint, Direction, Layout};
    use ratatui::widgets::Paragraph;

    let body_constraint = if inputs.config.has_body() {
        Constraint::Min(1)
    } else {
        Constraint::Length(0)
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header strip
            Constraint::Length(1), // rule
            body_constraint,       // body (N containers, or 0 when empty)
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

    render_body(f, chunks[2], inputs, theme);

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

fn render_body(f: &mut Frame, area: Rect, inputs: &DetailInputs<'_>, theme: &Theme) {
    use ratatui::layout::{Constraint, Direction, Layout};

    let cfg = inputs.config;
    if !cfg.has_body() || area.height == 0 {
        return;
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

    let widths = equal_widths(containers.len());
    let column_areas = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            widths
                .iter()
                .map(|w| Constraint::Percentage(*w))
                .collect::<Vec<_>>(),
        )
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

    for (col_area, ids) in column_areas.iter().zip(containers.iter()) {
        render_container(f, *col_area, ids, &ctx, inputs.registry, theme);
    }
}

fn render_container(
    f: &mut Frame,
    area: Rect,
    module_ids: &[String],
    ctx: &crate::detail_modules::DetailContext<'_>,
    reg: &crate::detail_modules::Registry,
    theme: &Theme,
) {
    use ratatui::layout::{Constraint, Direction, Layout};
    use ratatui::widgets::Paragraph;

    if module_ids.is_empty() || area.height == 0 {
        return;
    }

    enum Slot<'a> {
        Found(&'a dyn crate::detail_modules::DetailModule),
        Unknown(&'a str),
    }
    let slots: Vec<Slot<'_>> = module_ids
        .iter()
        .map(|id| match reg.get(id) {
            Some(m) => Slot::Found(m),
            None => {
                tracing::warn!(id = %id, "detail_bar: unknown module id in container");
                Slot::Unknown(id.as_str())
            }
        })
        .collect();

    // Per slot: [title row, body, gap row]. Last slot's gap is Length(0).
    // Unknown placeholder body = Length(0); only the title row renders.
    let constraints: Vec<Constraint> = slots
        .iter()
        .enumerate()
        .flat_map(|(i, slot)| {
            let last = i == slots.len() - 1;
            let body = match slot {
                Slot::Found(m) => m.height_hint(ctx),
                Slot::Unknown(_) => Constraint::Length(0),
            };
            let title = Constraint::Length(1);
            let gap = if last {
                Constraint::Length(0)
            } else {
                Constraint::Length(1)
            };
            [title, body, gap]
        })
        .collect();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let label_style = Style::default()
        .fg(ctx.theme.path)
        .add_modifier(Modifier::BOLD);

    for (i, slot) in slots.iter().enumerate() {
        let title_area = chunks[i * 3];
        let body_area = chunks[i * 3 + 1];
        match slot {
            Slot::Found(m) => {
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(m.title().to_string(), label_style))),
                    title_area,
                );
                m.render(body_area, ctx, f);
            }
            Slot::Unknown(id) => {
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        format!("[unknown: {id}]"),
                        theme.dim_style(),
                    ))),
                    title_area,
                );
            }
        }
    }
}

fn equal_widths(n: usize) -> Vec<u16> {
    match n {
        0 => vec![],
        1 => vec![100],
        2 => vec![50, 50],
        3 => vec![33, 33, 34],
        4 => vec![25, 25, 25, 25],
        _ => unreachable!("sanitize() guarantees containers.len() in 1..=4"),
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

/// Render the PROCESSES module body. Returns the label row plus one
/// row per process (capped at 5, with a "+N more" suffix when over
/// the cap), or a single "—" placeholder when empty.
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

/// Render the RECENT FILES module body. Returns the label row plus
/// one row per file (capped at 5), each annotated with per-file diff
/// stats when available. Single "—" placeholder when empty.
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
            let suffix_width = match diff {
                Some(d) if d.added > 0 || d.removed > 0 => {
                    4 + d.added.to_string().chars().count() + d.removed.to_string().chars().count()
                }
                _ => 0,
            };
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
        let reg = make_registry();
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
                registry: &reg,
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
        let evt = crate::events::WorkspaceEvents {
            first_user_text: Some("give me a tour".into()),
            tool_use_counts: crate::events::ToolUseCounts {
                read: 14,
                bash: 2,
                ..Default::default()
            },
            last_assistant_text: Some("Reading the repo now.".into()),
            ..Default::default()
        };
        let cfg = DetailBarConfig::default();
        let reg = make_registry();
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
            registry: &reg,
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
    }

    #[test]
    fn chrome_only_mode_renders_header_and_reply_no_body_labels() {
        let (_store, repo, ws) = seed_workspace();
        let evt = crate::events::WorkspaceEvents {
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
            registry: &reg,
        };
        // Width 100, height exactly CHROME_ROWS (4).
        let text = render_to_text(&inputs, 100, DetailBarConfig::CHROME_ROWS);
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
        let evt = crate::events::WorkspaceEvents {
            first_user_text: Some("hi".into()),
            last_assistant_text: Some("ack".into()),
            ..Default::default()
        };
        let cfg = DetailBarConfig::default();
        let reg = make_registry();
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
            registry: &reg,
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
        let mut cfg = DetailBarConfig::default();
        cfg.containers = vec![vec!["seshun_summary".into()]];
        let reg = make_registry();
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
            events_scanned: true,
            config: &cfg,
            registry: &reg,
        };
        let text = render_to_text(&inputs, 120, 10);
        assert!(
            text.contains("[unknown: seshun_summary]"),
            "expected unknown placeholder in: {text:?}",
        );
    }

    #[test]
    fn render_one_container_fills_full_width() {
        let (_store, repo, ws) = seed_workspace();
        let evt = crate::events::WorkspaceEvents {
            last_assistant_text: Some("hello".into()),
            ..Default::default()
        };
        let mut cfg = DetailBarConfig::default();
        cfg.containers = vec![vec!["recent_chat".into()]];
        let reg = make_registry();
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
            registry: &reg,
        };
        let text = render_to_text(&inputs, 120, 10);
        assert!(text.contains("RECENT CHAT"), "chat title: {text:?}");
        // Other module titles must NOT appear when only recent_chat is configured.
        assert!(!text.contains("SESSION SUMMARY"), "summary leaked: {text:?}");
        assert!(!text.contains("PROCESSES"), "procs leaked: {text:?}");
        assert!(!text.contains("RECENT FILES"), "files leaked: {text:?}");
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
    fn equal_widths_one_through_four() {
        assert_eq!(equal_widths(1), vec![100]);
        assert_eq!(equal_widths(2), vec![50, 50]);
        assert_eq!(equal_widths(3), vec![33, 33, 34]);
        assert_eq!(equal_widths(4), vec![25, 25, 25, 25]);
    }

    #[test]
    fn equal_widths_zero_is_empty() {
        assert_eq!(equal_widths(0), Vec::<u16>::new());
    }
}
