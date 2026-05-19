//! Renders the three chrome bars around the V5 dashboard list:
//! top chrome, status strip, footer (keybinds + sparkline).

use crate::ui::dashboard::sort::StatusCounts;
use crate::ui::dashboard::sparkline;
use crate::ui::dashboard::status::Status;
use crate::ui::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupMode {
    Repo,
    Attention,
}

pub fn top_chrome(
    group: GroupMode,
    repos: usize,
    workspaces: usize,
    width: usize,
    theme: &Theme,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled("wsx", theme.header_style()));
    spans.push(Span::styled(" · dashboard".to_string(), theme.dim_style()));
    spans.push(Span::raw(" ".repeat(6)));
    spans.push(Span::styled(
        "group: ".to_string(),
        Style::default().fg(theme.path),
    ));
    spans.push(tab_span("repo", group == GroupMode::Repo, theme));
    spans.push(Span::raw(" ".to_string()));
    spans.push(tab_span("attention", group == GroupMode::Attention, theme));

    let right = format!("{repos} repos · {workspaces} workspaces");
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let gap = width.saturating_sub(used + right.chars().count()).max(1);
    spans.push(Span::raw(" ".repeat(gap)));
    spans.push(Span::styled(right, Style::default().fg(theme.path)));
    Line::from(spans)
}

fn tab_span(label: &'static str, active: bool, theme: &Theme) -> Span<'static> {
    if active {
        Span::styled(
            label.to_string(),
            Style::default()
                .fg(theme.selected_fg)
                .bg(theme.selected_bg)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(label.to_string(), Style::default().fg(theme.path))
    }
}

pub fn status_strip(counts: StatusCounts, theme: &Theme) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let cells = [
        (Status::Question, counts.question),
        (Status::Stalled, counts.stalled),
        (Status::Waiting, counts.waiting),
        (Status::Thinking, counts.thinking),
        (Status::Complete, counts.complete),
        (Status::Idle, counts.idle),
    ];
    for (i, (status, n)) in cells.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("   ".to_string()));
        }
        let zero = *n == 0;
        let value_style = if zero {
            theme.dim_style()
        } else {
            theme.status_style(*status).add_modifier(Modifier::BOLD)
        };
        let label_style = if zero {
            theme.dim_style()
        } else {
            Style::default().fg(theme.path)
        };
        spans.push(Span::styled(status.glyph().to_string(), value_style));
        spans.push(Span::styled(format!(" {n}"), value_style));
        spans.push(Span::styled(format!(" {}", status.label()), label_style));
    }
    Line::from(spans)
}

pub fn footer(
    activity_samples: &[u32],
    version: &str,
    width: usize,
    theme: &Theme,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let keys = [
        ("↑↓", "nav"),
        ("↵", "open"),
        ("z", "fold"),
        ("n", "new"),
        ("e", "edit"),
        ("t", "term"),
        ("v", "diff"),
        ("r", "reply"),
        ("g", "group"),
        ("/", "filter"),
        ("q", "quit"),
    ];
    for (i, (key, label)) in keys.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  ".to_string()));
        }
        spans.push(Span::styled(
            (*key).to_string(),
            Style::default().fg(theme.dim).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {label}"),
            Style::default().fg(theme.path),
        ));
    }

    let spark = sparkline::render(activity_samples, 24);
    let right = format!("{version}  24h {spark}");
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let gap = width.saturating_sub(used + right.chars().count()).max(1);
    spans.push(Span::raw(" ".repeat(gap)));
    spans.push(Span::styled(right, Style::default().fg(theme.path)));
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn top_chrome_shows_app_name_and_counts() {
        let theme = Theme::wsx();
        let line = top_chrome(GroupMode::Repo, 9, 14, 100, &theme);
        let t = text(&line);
        assert!(t.starts_with("wsx · dashboard"), "{t:?}");
        assert!(t.contains("group: "));
        assert!(t.contains("repo"));
        assert!(t.contains("attention"));
        assert!(t.trim_end().ends_with("9 repos · 14 workspaces"), "{t:?}");
    }

    #[test]
    fn status_strip_includes_all_six_cells_with_zero_counts() {
        let theme = Theme::wsx();
        let counts = StatusCounts {
            question: 2,
            stalled: 1,
            waiting: 2,
            thinking: 2,
            complete: 3,
            idle: 4,
        };
        let line = status_strip(counts, &theme);
        let t = text(&line);
        assert!(t.contains("? 2 question"));
        assert!(t.contains("! 1 stalled"));
        assert!(t.contains("… 2 waiting"));
        assert!(t.contains("⠋ 2 thinking"));
        assert!(t.contains("✓ 3 complete"));
        assert!(t.contains("· 4 idle"));
    }

    #[test]
    fn status_strip_renders_zero_cells_in_dim() {
        let theme = Theme::wsx();
        let counts = StatusCounts::default();
        let line = status_strip(counts, &theme);
        let t = text(&line);
        assert!(t.contains("? 0 question"));
        assert!(t.contains("· 0 idle"));
    }

    #[test]
    fn footer_includes_keybinds_and_sparkline() {
        let theme = Theme::wsx();
        let samples = vec![1, 2, 3, 4, 5];
        let line = footer(&samples, "v0.5.0", 200, &theme);
        let t = text(&line);
        assert!(t.contains("↑↓ nav"));
        assert!(t.contains("g group"));
        assert!(t.contains("q quit"));
        assert!(t.contains("24h "));
        assert!(t.contains("v0.5.0"));
    }
}
