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

    let label = match focus {
        PaneFocus::ProjectManager => "Project Manager [Tab/Esc back]",
        PaneFocus::Dashboard | PaneFocus::DetailBarReply => {
            "Project Manager [Tab to focus · r refresh]"
        }
    };
    let width = title_area.width as usize;
    let used = label.chars().count();
    let gap = 2;
    let rule_len = width.saturating_sub(used + gap);
    let mut spans: Vec<Span<'static>> = vec![Span::styled(label.to_string(), theme.dim_style())];
    if rule_len > 0 {
        spans.push(Span::raw(" ".repeat(gap)));
        spans.push(Span::styled("─".repeat(rule_len), theme.dim_style()));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), title_area);

    match session {
        Some(s) => {
            let offset = s
                .scrollback_offset
                .load(std::sync::atomic::Ordering::Relaxed);
            let mut parser = s.parser.lock().unwrap();
            parser.set_scrollback(offset);
            let screen = parser.screen();
            render_screen(screen, f.buffer_mut(), term_area);
            if matches!(focus, PaneFocus::ProjectManager) && !screen.hide_cursor() && offset == 0 {
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
