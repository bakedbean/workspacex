use crate::pty::render::render_screen;
use crate::pty::session::Session;
use crate::ui::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use std::sync::Arc;

/// Render the attached-workspace view. When `attention_line` is `Some`, a
/// one-line indicator listing other workspaces that need attention is
/// inserted above the footer; when `None`, the term gets that row back.
pub fn render(
    f: &mut Frame,
    area: Rect,
    session: &Arc<Session>,
    label: &str,
    attention_line: Option<&str>,
    theme: &Theme,
) {
    let status_height = if attention_line.is_some() { 1 } else { 0 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(status_height),
            Constraint::Length(1),
        ])
        .split(area);
    let term_area = chunks[0];
    let status_area = chunks[1];
    let footer_area = chunks[2];

    let parser = session.parser.lock().unwrap();
    let screen = parser.screen();
    render_screen(screen, f.buffer_mut(), term_area);
    let (cy, cx) = screen.cursor_position();
    if !screen.hide_cursor() {
        f.set_cursor_position((term_area.x + cx, term_area.y + cy));
    }
    drop(parser);

    if let Some(text) = attention_line {
        let line = format!(" ⚠ {text}");
        f.render_widget(Paragraph::new(line).style(theme.warn_style()), status_area);
    }

    let footer =
        format!(" {label}   [Ctrl-x] d=detach u=updates e=edit t=term v=diff x=send-Ctrl-x ");
    f.render_widget(Paragraph::new(footer).style(theme.dim_style()), footer_area);
}

pub fn resize_session(session: &Arc<Session>, area: Rect, footer_rows: u16) {
    let _ = session.resize(area.width, area.height.saturating_sub(footer_rows));
}
