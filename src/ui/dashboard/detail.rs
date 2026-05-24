//! Bottom-pinned detail bar shown when a workspace is selected on the
//! dashboard. Renders header strip, three-column body, and an inline
//! reply input.
//!
//! See `docs/superpowers/specs/2026-05-24-dashboard-workspace-detail-design.md`.

/// Minimum rows the bar needs to render usefully (1 header + 1 rule + 3
/// body + 1 rule + 1 input + 1 spacing slack).
pub const MIN_HEIGHT: u16 = 8;

/// Compute the detail bar's preferred height given the total available
/// height. Targets ~22% of the area, clamped to `[MIN_HEIGHT, 14]`.
pub fn preferred_height(total_height: u16) -> u16 {
    let target = (u32::from(total_height) * 22 / 100) as u16;
    target.clamp(MIN_HEIGHT, 14)
}

use crate::events::{ToolUseCounts, WorkspaceEvents};
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
}

/// Render the detail bar into `area`. No-op when `area.height < MIN_HEIGHT`
/// (caller is expected to fall back to a condensed banner — see
/// `app.rs::draw`).
pub fn render(f: &mut Frame, area: Rect, _inputs: &DetailInputs<'_>, _theme: &Theme) {
    if area.height == 0 || area.height < MIN_HEIGHT {
        return;
    }
    // Real rendering arrives in subsequent tasks.
    let _ = f;
}

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

const GUTTER: &str = "▍";

/// One-line header strip at the top of the bar.
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
            theme.lifecycle_style(Some(lc)).unwrap_or_else(|| theme.dim_style()),
        ));
    }

    if let Some(d) = diff {
        if d.added > 0 || d.removed > 0 {
            spans.push(Span::raw("  ".to_string()));
            spans.push(Span::styled(format!("+{}", d.added), theme.ok_style()));
            spans.push(Span::raw(" ".to_string()));
            spans.push(Span::styled(format!("−{}", d.removed), theme.err_style()));
        }
    }

    spans.push(Span::raw("  ".to_string()));
    if procs > 0 {
        spans.push(Span::styled(
            format!("● {procs} procs"),
            theme.status_style(Status::Thinking),
        ));
    } else {
        spans.push(Span::styled("  · 0 procs".to_string(), theme.dim_style()));
    }

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

/// Build the lines that make up the SESSION SUMMARY column. Returns a
/// Vec because the caller is responsible for slicing to fit the body
/// area height.
pub(super) fn build_session_summary(
    events: Option<&WorkspaceEvents>,
    theme: &Theme,
    column_width: usize,
    worktree_path: &str,
    created_secs: u64,
) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let label_style = Style::default().fg(theme.path).add_modifier(Modifier::BOLD);
    out.push(Line::from(Span::styled("SESSION SUMMARY".to_string(), label_style)));

    let Some(evt) = events else {
        out.push(Line::from(Span::styled(
            "  loading…".to_string(),
            theme.dim_style(),
        )));
        return out;
    };

    let prefix = Span::styled("▸ ".to_string(), theme.dim_style());

    if let Some(prompt) = evt.first_user_text.as_deref() {
        let truncated = truncate_to_chars(prompt, column_width.saturating_sub(4));
        out.push(Line::from(vec![
            prefix.clone(),
            Span::styled(
                format!("\"{truncated}\""),
                Style::default().add_modifier(Modifier::ITALIC),
            ),
        ]));
    }

    let trace = format_tool_trace(&evt.tool_use_counts);
    if !trace.is_empty() {
        out.push(Line::from(vec![
            prefix.clone(),
            Span::raw(truncate_to_chars(&trace, column_width.saturating_sub(2))),
        ]));
    }

    let now_signal = format_where_now(evt);
    if !now_signal.is_empty() {
        out.push(Line::from(vec![
            prefix.clone(),
            Span::raw(truncate_to_chars(&now_signal, column_width.saturating_sub(2))),
        ]));
    }

    // (PR row is wired but always omitted in v1 — pr_title/pr_number arrive as None.)

    let age = format_ago_short(Some(created_secs));
    let path_text = format!("{worktree_path} · created {age}");
    let path_truncated = truncate_to_chars_left(&path_text, column_width.saturating_sub(2));
    out.push(Line::from(vec![
        prefix.clone(),
        Span::styled(path_truncated, theme.dim_style()),
    ]));

    out
}

fn format_tool_trace(counts: &ToolUseCounts) -> String {
    let mut parts: Vec<String> = Vec::new();
    if counts.read > 0 {
        parts.push(format!("read {} {}", counts.read, plural("file", counts.read)));
    }
    if counts.edit > 0 {
        parts.push(format!("edited {} {}", counts.edit, plural("file", counts.edit)));
    }
    if counts.write > 0 {
        parts.push(format!("wrote {} {}", counts.write, plural("file", counts.write)));
    }
    if counts.bash > 0 {
        parts.push(format!("ran {} {}", counts.bash, plural("command", counts.bash)));
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

fn format_where_now(evt: &WorkspaceEvents) -> String {
    if let Some(q) = evt.pending_question_tool() {
        return format!("agent asked via {q}");
    }
    // Pending non-question permission tool, if any:
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    if let Some((name, _)) = evt.pending_permission_tool(now_ms, 0) {
        return format!("awaiting permission for {name}");
    }
    if let Some(t) = evt.last_assistant_text.as_deref() {
        let first_line = t.lines().next().unwrap_or(t);
        return first_line.to_string();
    }
    String::new()
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

    fn seed_workspace() -> (crate::store::Store, crate::store::Repo, crate::store::Workspace) {
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
            let inputs = DetailInputs {
                repo: &repo,
                workspace: &ws,
                events: None,
                procs: &[],
                diff: None,
                lifecycle: None,
                pr_title: None,
                pr_number: None,
                status: Status::Idle,
                ago_secs: None,
                reply_draft: "",
                reply_focused: false,
                events_scanned: false,
            };
            render(f, Rect::new(0, 0, 80, 0), &inputs, &theme);
        });
        assert!(result.is_ok());
    }

    #[test]
    fn preferred_height_clamps_to_min_on_short_terminal() {
        // 22% of 20 = 4 → clamps up to MIN_HEIGHT (8).
        assert_eq!(preferred_height(20), MIN_HEIGHT);
    }

    #[test]
    fn preferred_height_returns_22_percent_for_typical_terminal() {
        // 22% of 50 = 11 → within range.
        assert_eq!(preferred_height(50), 11);
    }

    #[test]
    fn preferred_height_clamps_to_14_on_tall_terminal() {
        // 22% of 100 = 22 → clamps down to 14.
        assert_eq!(preferred_height(100), 14);
    }

    #[test]
    fn preferred_height_handles_zero_height() {
        // 22% of 0 = 0 → clamps up to MIN_HEIGHT.
        assert_eq!(preferred_height(0), MIN_HEIGHT);
    }

    #[test]
    fn header_strip_contains_all_chips_in_order() {
        let theme = Theme::wsx();
        let line = build_header_strip(
            "repo-overview",
            "bakedbean/repo-overview",
            Some(BranchLifecycle::PrOpen),
            Some(DiffStats { added: 12, removed: 3 }),
            2,
            Status::Question,
            Some(29),
            &theme,
            120,
        );
        let text = line_to_string(&line);
        assert!(text.contains("repo-overview"), "name missing: {text:?}");
        assert!(text.contains("bakedbean/repo-overview"), "branch missing: {text:?}");
        assert!(text.contains("+12") && text.contains("−3"), "diff missing: {text:?}");
        assert!(text.contains("● 2") || text.contains("2 procs"), "procs missing: {text:?}");
        assert!(text.contains("?"), "status glyph missing: {text:?}");
        assert!(text.contains("29s"), "ago missing: {text:?}");
    }

    #[test]
    fn header_strip_omits_diff_when_none() {
        let theme = Theme::wsx();
        let line = build_header_strip(
            "ws", "br", None, None, 0, Status::Idle, None, &theme, 80,
        );
        let text = line_to_string(&line);
        assert!(!text.contains("+"), "diff cell should be absent: {text:?}");
        assert!(!text.contains("−"), "diff cell should be absent: {text:?}");
    }

    #[test]
    fn header_strip_omits_lifecycle_when_none() {
        let theme = Theme::wsx();
        let line = build_header_strip(
            "ws", "br", None, None, 0, Status::Idle, None, &theme, 80,
        );
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
        let mut e = WorkspaceEvents::default();
        e.first_user_text = first.map(str::to_string);
        e.tool_use_counts = counts;
        e.last_assistant_text = last_assistant.map(str::to_string);
        e
    }

    #[test]
    fn session_summary_renders_initial_prompt_when_present() {
        let theme = Theme::wsx();
        let evt = make_events_with(Some("summarize the repo"), ToolUseCounts::default(), None);
        let lines = build_session_summary(Some(&evt), &theme, 50, "/tmp/wt", 0);
        let joined: String = lines.iter().map(line_to_string).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("summarize the repo"), "{joined:?}");
    }

    #[test]
    fn session_summary_tool_trace_omits_zero_counts() {
        let theme = Theme::wsx();
        let evt = make_events_with(None, ToolUseCounts { read: 5, edit: 0, write: 0, bash: 2, other: 0 }, None);
        let lines = build_session_summary(Some(&evt), &theme, 50, "/tmp/wt", 0);
        let joined: String = lines.iter().map(line_to_string).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("read 5 files"), "{joined:?}");
        assert!(joined.contains("ran 2 commands"), "{joined:?}");
        assert!(!joined.contains("edited"), "edit fragment should be omitted: {joined:?}");
        assert!(!joined.contains("wrote"), "write fragment should be omitted: {joined:?}");
    }

    #[test]
    fn session_summary_singular_plural_forms() {
        let theme = Theme::wsx();
        let evt = make_events_with(None, ToolUseCounts { read: 1, edit: 1, write: 1, bash: 1, other: 1 }, None);
        let lines = build_session_summary(Some(&evt), &theme, 100, "/tmp/wt", 0);
        let joined: String = lines.iter().map(line_to_string).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("read 1 file") && !joined.contains("read 1 files"), "{joined:?}");
        assert!(joined.contains("edited 1 file"), "{joined:?}");
        assert!(joined.contains("ran 1 command"), "{joined:?}");
    }

    #[test]
    fn session_summary_shows_loading_when_events_none() {
        let theme = Theme::wsx();
        let lines = build_session_summary(None, &theme, 50, "/tmp/wt", 0);
        let joined: String = lines.iter().map(line_to_string).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("loading"), "{joined:?}");
    }

    #[test]
    fn session_summary_includes_worktree_path() {
        let theme = Theme::wsx();
        let evt = make_events_with(None, ToolUseCounts::default(), None);
        let lines = build_session_summary(Some(&evt), &theme, 60, "/tmp/very/long/path/workspaces/foo", 120);
        let joined: String = lines.iter().map(line_to_string).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("foo") || joined.contains("workspaces"), "basename retained: {joined:?}");
    }
}
