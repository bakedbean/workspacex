//! Renders the dashboard's status strip, and owns `GroupMode`. The top
//! chrome and the footer (keybinds + sparkline) render through the bar
//! engine instead; see `crate::ui::bar::dashboard_header` and
//! `crate::ui::bar::dashboard_footer`.

use crate::ui::dashboard::sort::StatusCounts;
use crate::ui::dashboard::status::Status;
use crate::ui::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupMode {
    #[default]
    Repo,
    Attention,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
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
}
