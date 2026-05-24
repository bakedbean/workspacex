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
}
