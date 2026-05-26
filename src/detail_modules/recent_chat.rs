//! Recent chat module. Renders the agent's most recent user/assistant
//! turns for the selected workspace.

use crate::detail_modules::{DetailContext, DetailModule};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};

pub struct RecentChat;

impl DetailModule for RecentChat {
    fn id(&self) -> &'static str { "recent_chat" }
    fn title(&self) -> &'static str { "RECENT CHAT" }
    fn height_hint(&self, _ctx: &DetailContext<'_>) -> Constraint {
        Constraint::Min(3)
    }
    fn render(&self, area: Rect, ctx: &DetailContext<'_>, frame: &mut Frame<'_>) {
        use ratatui::widgets::Paragraph;
        let lines = crate::ui::dashboard::detail::build_recent_chat(
            if ctx.events_scanned { ctx.events } else { None },
            ctx.theme,
            area.width as usize,
            (area.height as usize).saturating_sub(1).max(1),
        );
        frame.render_widget(Paragraph::new(lines), area);
    }
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
