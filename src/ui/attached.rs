use crate::pty::render::render_screen;
use crate::pty::session::Session;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use std::sync::Arc;

pub fn render(f: &mut Frame, area: Rect, session: &Arc<Session>, label: &str) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    let term_area = chunks[0];
    let parser = session.parser.lock().unwrap();
    let screen = parser.screen();
    render_screen(screen, f.buffer_mut(), term_area);
    let (cy, cx) = screen.cursor_position();
    if !screen.hide_cursor() {
        f.set_cursor_position((term_area.x + cx, term_area.y + cy));
    }
    drop(parser);

    let footer = format!(" {label}   [Ctrl-a d] detach   [Ctrl-a a] send Ctrl-a ");
    f.render_widget(
        Paragraph::new(footer).style(crate::ui::theme::dim()),
        chunks[1],
    );
}

pub fn resize_session(session: &Arc<Session>, area: Rect) {
    let _ = session.resize(area.width, area.height.saturating_sub(1));
}
