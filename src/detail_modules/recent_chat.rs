//! Recent chat module. Renders the agent's most recent user/assistant
//! turns for the selected workspace.

use crate::detail_modules::{DetailContext, DetailModule};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};

pub struct RecentChat;

impl DetailModule for RecentChat {
    fn id(&self) -> &'static str {
        "recent_chat"
    }
    fn title(&self) -> &'static str {
        "RECENT CHAT"
    }
    fn height_hint(&self, _ctx: &DetailContext<'_>) -> Constraint {
        Constraint::Min(3)
    }
    fn lines(
        &self,
        ctx: &DetailContext<'_>,
        width: u16,
    ) -> Vec<ratatui::text::Line<'static>> {
        build_lines(ctx, width)
    }
    fn render(&self, area: Rect, ctx: &DetailContext<'_>, frame: &mut Frame<'_>) {
        use ratatui::widgets::Paragraph;
        let mut lines = build_lines(ctx, area.width);
        // Tail-slice: preserve the legacy RECENT CHAT visual contract of
        // showing the last `area.height` wrapped lines. Once Task 7's
        // scrolling container takes over and `render` is removed, this
        // slicing moves to the container layer (which uses scroll offset).
        let max = (area.height as usize).max(1);
        if lines.len() > max {
            lines = lines.split_off(lines.len() - max);
        }
        frame.render_widget(Paragraph::new(lines), area);
    }
}

fn build_lines(
    ctx: &DetailContext<'_>,
    width: u16,
) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::text::{Line, Span};

    let events = if ctx.events_scanned { ctx.events } else { None };
    let theme = ctx.theme;
    let column_width = width as usize;

    let mut out: Vec<Line<'static>> = Vec::new();

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

    // Word-wrap to column_width. For display purposes, take all wrapped lines
    // (the container will slice by scroll offset).
    let wrapped = wrap_lines(text, column_width);
    for line in wrapped {
        out.push(Line::from(Span::styled(line, theme.dim_style())));
    }

    out
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detail_modules::tests_helpers::stub_context;

    #[test]
    fn id_is_recent_chat() {
        assert_eq!(RecentChat.id(), "recent_chat");
    }

    #[test]
    fn title_is_uppercase() {
        assert_eq!(RecentChat.title(), "RECENT CHAT");
    }

    #[test]
    fn height_hint_is_min_three() {
        let ctx = stub_context();
        assert_eq!(RecentChat.height_hint(&ctx), Constraint::Min(3));
    }

    #[test]
    fn lines_with_no_events_returns_at_least_one_line() {
        let ctx = stub_context();
        let out = RecentChat.lines(&ctx, 40);
        assert!(!out.is_empty(), "RecentChat should emit at least one line in empty state");
    }
}
