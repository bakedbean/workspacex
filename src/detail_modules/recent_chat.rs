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
    fn render(&self, area: Rect, ctx: &DetailContext<'_>, frame: &mut Frame<'_>) {
        use ratatui::text::{Line, Span};
        use ratatui::widgets::Paragraph;

        let events = if ctx.events_scanned { ctx.events } else { None };
        let theme = ctx.theme;
        let column_width = area.width as usize;
        // Host draws the title row separately; the full `area` height
        // is available for body content.
        let max_body_lines = (area.height as usize).max(1);

        let mut out: Vec<Line<'static>> = Vec::new();

        let Some(evt) = events else {
            out.push(Line::from(Span::styled(
                "  loading…".to_string(),
                theme.dim_style(),
            )));
            frame.render_widget(Paragraph::new(out), area);
            return;
        };

        let Some(text) = evt.last_assistant_text.as_deref() else {
            out.push(Line::from(Span::styled("—".to_string(), theme.dim_style())));
            frame.render_widget(Paragraph::new(out), area);
            return;
        };

        // Word-wrap to column_width. Take the last `max_body_lines` after wrapping.
        let wrapped = wrap_lines(text, column_width);
        let start = wrapped.len().saturating_sub(max_body_lines);
        for line in wrapped.iter().skip(start) {
            out.push(Line::from(Span::styled(line.clone(), theme.dim_style())));
        }
        frame.render_widget(Paragraph::new(out), area);
    }
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
}
