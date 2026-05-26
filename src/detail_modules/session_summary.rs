//! Session summary module. Shows the agent's current status, last
//! activity, and tool-use trace for the selected workspace.

use crate::detail_modules::{DetailContext, DetailModule};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};

pub struct SessionSummary;

impl DetailModule for SessionSummary {
    fn id(&self) -> &'static str { "session_summary" }
    fn title(&self) -> &'static str { "SESSION SUMMARY" }
    fn height_hint(&self, _ctx: &DetailContext<'_>) -> Constraint {
        Constraint::Min(3)
    }
    fn render(&self, area: Rect, ctx: &DetailContext<'_>, frame: &mut Frame<'_>) {
        use ratatui::widgets::Paragraph;
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let created_at_secs = (ctx.workspace.created_at.max(0) / 1000) as u64;
        let created_secs = now_secs.saturating_sub(created_at_secs);

        let lines = crate::ui::dashboard::detail::build_session_summary(
            if ctx.events_scanned { ctx.events } else { None },
            ctx.status,
            ctx.theme,
            area.width as usize,
            created_secs,
            ctx.ago_secs,
        );
        frame.render_widget(Paragraph::new(lines), area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detail_modules::tests_helpers::stub_context;

    #[test]
    fn id_is_session_summary() {
        assert_eq!(SessionSummary.id(), "session_summary");
    }

    #[test]
    fn title_is_uppercase() {
        assert_eq!(SessionSummary.title(), "SESSION SUMMARY");
    }

    #[test]
    fn height_hint_is_min_three() {
        let ctx = stub_context();
        assert_eq!(SessionSummary.height_hint(&ctx), Constraint::Min(3));
    }
}
