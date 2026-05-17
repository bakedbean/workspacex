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
/// When `pinned` is non-empty, a one-row chip bar is inserted between the
/// terminal area and the status/footer rows.
/// Returns the per-chip clickable Rects for mouse hit-testing.
pub fn render(
    f: &mut Frame,
    area: Rect,
    session: &Arc<Session>,
    label: &str,
    attention_line: Option<&str>,
    pinned: &[crate::pinned::PinnedCommand],
    theme: &Theme,
) -> Vec<Rect> {
    let chip_height = if pinned.is_empty() { 0 } else { 1 };
    let status_height = if attention_line.is_some() { 1 } else { 0 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(chip_height),
            Constraint::Length(status_height),
            Constraint::Length(1),
        ])
        .split(area);
    let term_area = chunks[0];
    let chip_area = chunks[1];
    let status_area = chunks[2];
    let footer_area = chunks[3];

    let offset = session
        .scrollback_offset
        .load(std::sync::atomic::Ordering::Relaxed);
    let mut parser = session.parser.lock().unwrap();
    parser.set_scrollback(offset);
    let screen = parser.screen();
    render_screen(screen, f.buffer_mut(), term_area);
    let (cy, cx) = screen.cursor_position();
    if !screen.hide_cursor() && offset == 0 {
        f.set_cursor_position((term_area.x + cx, term_area.y + cy));
    }
    drop(parser);

    if let Some(text) = attention_line {
        let line = format!(" ⚠ {text}");
        f.render_widget(Paragraph::new(line).style(theme.warn_style()), status_area);
    }

    let footer = format!(
        " {label}   [Ctrl-x] d=detach u=updates e=edit t=term v=diff k=procs x=send-Ctrl-x "
    );
    f.render_widget(Paragraph::new(footer).style(theme.dim_style()), footer_area);

    if chip_height == 1 {
        render_chip_row(f, chip_area, pinned, theme)
    } else {
        Vec::new()
    }
}

pub fn resize_session(session: &Arc<Session>, area: Rect, footer_rows: u16) {
    let _ = session.resize(area.width, area.height.saturating_sub(footer_rows));
}

/// Compute the clickable Rect for each chip that fits within `area`.
/// Returns one Rect per chip rendered left-to-right; chips that don't fit
/// are dropped from the end. The full chip text is `[N] <label>` joined by
/// 3-space gaps. Labels are individually truncated to 12 columns first.
pub fn layout_chip_row(
    area: ratatui::layout::Rect,
    pinned: &[crate::pinned::PinnedCommand],
) -> Vec<ratatui::layout::Rect> {
    let mut rects = Vec::new();
    let mut x = area.x;
    let max_x = area.x.saturating_add(area.width);
    const GAP: u16 = 3;
    for (i, cmd) in pinned.iter().enumerate().take(9) {
        let label = crate::pinned::truncate_label(&cmd.label, 12);
        // Chip text: "[N] label"  (4 chars for "[N] " plus label chars)
        let chip_chars = 4 + label.chars().count() as u16;
        if i > 0 {
            x = x.saturating_add(GAP);
        }
        if x.saturating_add(chip_chars) > max_x {
            break;
        }
        rects.push(ratatui::layout::Rect {
            x,
            y: area.y,
            width: chip_chars,
            height: 1,
        });
        x = x.saturating_add(chip_chars);
    }
    rects
}

fn render_chip_row(
    f: &mut Frame,
    area: ratatui::layout::Rect,
    pinned: &[crate::pinned::PinnedCommand],
    theme: &Theme,
) -> Vec<ratatui::layout::Rect> {
    let rects = layout_chip_row(area, pinned);
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(rects.len() * 3);
    for (i, _r) in rects.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("   "));
        }
        let label = crate::pinned::truncate_label(&pinned[i].label, 12);
        spans.push(Span::styled(format!("[{}]", i + 1), theme.dim_style()));
        spans.push(Span::raw(format!(" {label}")));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
    rects
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pinned::PinnedCommand;

    fn cmds(specs: &[(&str, &str)]) -> Vec<PinnedCommand> {
        specs
            .iter()
            .map(|(l, c)| PinnedCommand {
                label: (*l).into(),
                command: (*c).into(),
            })
            .collect()
    }

    #[test]
    fn chip_row_layout_returns_rects_for_each_visible_chip() {
        let area = ratatui::layout::Rect::new(0, 0, 80, 1);
        let pinned = cmds(&[("PR", "/pr"), ("FB", "/fb"), ("UR", "/ur")]);
        let rects = layout_chip_row(area, &pinned);
        assert_eq!(rects.len(), 3);
        for r in &rects {
            assert!(r.width > 0);
            assert_eq!(r.y, 0);
        }
        // Chips render left-to-right with at least one column of gap.
        assert!(rects[1].x > rects[0].x + rects[0].width);
    }

    #[test]
    fn chip_row_drops_trailing_chips_when_too_narrow() {
        let area = ratatui::layout::Rect::new(0, 0, 12, 1);
        let pinned = cmds(&[("PR", "/pr"), ("FB", "/fb"), ("UR", "/ur")]);
        let rects = layout_chip_row(area, &pinned);
        // Exact count depends on chip widths; at width 12 we expect strictly
        // fewer than 3, with at least 1.
        assert!(!rects.is_empty(), "should render at least one chip");
        assert!(rects.len() < 3, "should drop trailing chips at width 12");
    }

    #[test]
    fn chip_row_empty_list_returns_no_rects() {
        let area = ratatui::layout::Rect::new(0, 0, 80, 1);
        assert!(layout_chip_row(area, &[]).is_empty());
    }
}
