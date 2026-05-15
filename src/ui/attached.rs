use crate::pty::render::render_screen;
use crate::pty::session::Session;
use crate::ui::theme::Theme;
use crate::ui::updates_bar::{UpdatesRow, UpdatesRowKind};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use std::sync::Arc;

/// Render the attached-workspace view. When `status_row` is `Some`, a
/// one-line indicator showing another workspace's update is inserted
/// above the footer; when `None`, the term gets that row back.
pub fn render(
    f: &mut Frame,
    area: Rect,
    session: &Arc<Session>,
    label: &str,
    status_row: Option<&UpdatesRow>,
    theme: &Theme,
) {
    let status_height = if status_row.is_some() { 1 } else { 0 };
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

    if let Some(row) = status_row {
        let style = match row.kind {
            UpdatesRowKind::Attention => theme.warn_style(),
            UpdatesRowKind::Activity => theme.ok_style(),
        };
        let text = format!(" {} {}", row.glyph, row.text);
        f.render_widget(Paragraph::new(text).style(style), status_area);
    }

    let footer = format!(
        " {label}   [Ctrl-a d] detach   [Ctrl-a u] updates   [Ctrl-a a] send Ctrl-a "
    );
    f.render_widget(Paragraph::new(footer).style(theme.dim_style()), footer_area);
}

pub fn resize_session(session: &Arc<Session>, area: Rect, footer_rows: u16) {
    let _ = session.resize(area.width, area.height.saturating_sub(footer_rows));
}
