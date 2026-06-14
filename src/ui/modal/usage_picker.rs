//! Extracted from ui/modal.rs.

use super::*;

/// Position a `w`x`h` popup so its bottom edge sits directly above `anchor`,
/// left-aligned to it, clamped to stay fully within `screen`.
fn picker_rect(anchor: Rect, screen: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(screen.width);
    let h = h.min(screen.height);
    let max_x = screen.x + screen.width.saturating_sub(w);
    let x = anchor.x.clamp(screen.x, max_x);
    let y = anchor.y.saturating_sub(h).max(screen.y);
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}

/// Render the anchored usage-window picker above the footer graph. Returns the
/// per-option row `Rect`s (in `UsageWindow::ALL` order) for click hit-testing.
pub fn render_usage_window_picker(
    f: &mut Frame,
    screen: Rect,
    selected: usize,
    current: UsageWindow,
    graph_rect: Option<Rect>,
    theme: &Theme,
) -> Vec<Rect> {
    let w: u16 = 18;
    let h: u16 = UsageWindow::ALL.len() as u16 + 2; // options + top/bottom border
    let anchor = graph_rect.unwrap_or(Rect {
        x: screen.x + screen.width.saturating_sub(w),
        y: screen.y + screen.height.saturating_sub(1),
        width: w,
        height: 1,
    });
    let rect = picker_rect(anchor, screen, w, h);
    f.render_widget(Clear, rect);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut rows: Vec<Rect> = Vec::new();
    for (i, win) in UsageWindow::ALL.iter().enumerate() {
        let dot = if *win == current { "•" } else { " " };
        let style = if i == selected {
            theme.selected_bg_style()
        } else {
            theme.header_style()
        };
        lines.push(Line::from(Span::styled(
            format!(" {dot} {}", win.label()),
            style,
        )));
        rows.push(Rect {
            x: rect.x + 1,
            y: rect.y + 1 + i as u16,
            width: rect.width.saturating_sub(2),
            height: 1,
        });
    }

    let para = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title("usage window")
            .title_alignment(Alignment::Left),
    );
    f.render_widget(para, rect);
    rows
}

#[cfg(test)]
mod usage_picker_tests {
    use super::*;
    use ratatui::layout::Rect;

    fn screen() -> Rect {
        Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 30,
        }
    }

    #[test]
    fn picker_sits_just_above_anchor_left_aligned() {
        // Anchor = graph on the footer row (y = 29), x = 70.
        let anchor = Rect {
            x: 70,
            y: 29,
            width: 27,
            height: 1,
        };
        let r = picker_rect(anchor, screen(), 18, 5);
        assert_eq!(r.x, 70); // left-aligned to anchor
        assert_eq!(r.y, 24); // 29 - 5, directly above
        assert_eq!(r.width, 18);
        assert_eq!(r.height, 5);
    }

    #[test]
    fn picker_clamps_to_right_edge() {
        let anchor = Rect {
            x: 95,
            y: 29,
            width: 5,
            height: 1,
        };
        let r = picker_rect(anchor, screen(), 18, 5);
        assert_eq!(r.x, 100 - 18);
    }

    #[test]
    fn picker_clamps_to_top_edge() {
        let anchor = Rect {
            x: 10,
            y: 2,
            width: 27,
            height: 1,
        };
        let r = picker_rect(anchor, screen(), 18, 5);
        assert_eq!(r.y, 0);
    }
}
