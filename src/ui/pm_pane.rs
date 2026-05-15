//! Project Manager pane: renders PM PTY into a sub-rect with focus-aware title.

use crate::pty::render::render_screen;
use crate::pty::session::Session;
use crate::ui::PaneFocus;
use crate::ui::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use std::sync::Arc;

/// Render the PM pane into `area`. When `session` is `None` (pane was
/// just opened and spawn is in flight), a single placeholder line is
/// shown.
pub fn render(
    f: &mut Frame,
    area: Rect,
    session: Option<&Arc<Session>>,
    focus: PaneFocus,
    theme: &Theme,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    let title_area = chunks[0];
    let term_area = chunks[1];

    let title = match focus {
        PaneFocus::ProjectManager => "── Project Manager [Tab/Esc back] ──",
        PaneFocus::Dashboard => "── Project Manager [Tab to focus · r refresh] ──",
    };
    f.render_widget(Paragraph::new(title).style(theme.dim_style()), title_area);

    match session {
        Some(s) => {
            let parser = s.parser.lock().unwrap();
            let screen = parser.screen();
            render_screen(screen, f.buffer_mut(), term_area);
            if matches!(focus, PaneFocus::ProjectManager) && !screen.hide_cursor() {
                let (cy, cx) = screen.cursor_position();
                f.set_cursor_position((term_area.x + cx, term_area.y + cy));
            }
        }
        None => {
            f.render_widget(
                Paragraph::new("starting project manager…").style(theme.dim_style()),
                term_area,
            );
        }
    }
}

/// Recompute PTY dimensions after a terminal resize.
pub fn resize_session(session: &Arc<Session>, area: Rect) {
    // Subtract 1 row for the title bar.
    let _ = session.resize(area.width, area.height.saturating_sub(1));
}
