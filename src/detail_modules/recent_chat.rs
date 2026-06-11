//! Recent chat module. Renders the agent's most recent user/assistant
//! turns for the selected workspace.

use crate::detail_modules::{DetailContext, DetailModule};

pub struct RecentChat;

impl DetailModule for RecentChat {
    fn id(&self) -> &'static str {
        "recent_chat"
    }
    fn title(&self) -> &'static str {
        "RECENT CHAT"
    }
    fn lines(&self, ctx: &DetailContext<'_>, width: u16) -> Vec<ratatui::text::Line<'static>> {
        build_lines(ctx, width)
    }
}

fn build_lines(ctx: &DetailContext<'_>, width: u16) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::text::{Line, Span};

    let events = if ctx.events_scanned { ctx.events } else { None };
    let theme = ctx.theme;

    let Some(evt) = events else {
        return vec![Line::from(Span::styled(
            "  loading…".to_string(),
            theme.dim_style(),
        ))];
    };

    let Some(text) = evt.last_assistant_text.as_deref() else {
        return vec![Line::from(Span::styled("—".to_string(), theme.dim_style()))];
    };

    crate::detail_modules::markdown::render(text, width, theme)
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
    fn lines_with_no_events_returns_at_least_one_line() {
        let ctx = stub_context();
        let out = RecentChat.lines(&ctx, 40);
        assert!(
            !out.is_empty(),
            "RecentChat should emit at least one line in empty state"
        );
    }
}
