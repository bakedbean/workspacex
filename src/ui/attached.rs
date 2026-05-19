use crate::pinned::{PinnedCommand, truncate_label};
use crate::pty::render::render_screen;
use crate::pty::session::Session;
use crate::ui::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::widgets::Paragraph;
use std::sync::Arc;

/// One pane in the attached view: a workspace's PTY plus its label,
/// the rect it occupies, and whether it's the focused pane (cursor + chip
/// chrome). For the single-pane case the slice has one entry; for vim-style
/// splits there's one entry per leaf.
pub struct PaneSpec<'a> {
    pub session: &'a Arc<Session>,
    pub label: &'a str,
    pub rect: Rect,
    pub focused: bool,
}

/// Render one or more attached panes plus the shared chrome (optional
/// chip row, optional attention line, footer). Returns the per-chip
/// clickable Rects for mouse hit-testing.
///
/// Layout (top to bottom):
///   - the pane area, subdivided per `panes[i].rect` (which the caller
///     pre-computed from `SplitTree::layout`),
///   - one row of pinned-command chips (only when `pinned` is non-empty),
///   - one row of cross-workspace attention status (only when `Some`),
///   - one row of footer hints.
///
/// When there are multiple panes, each pane also gets a 1-row title bar
/// at the top of its rect showing the workspace name and a focus marker.
/// Single-pane mode skips the title bar so it looks identical to the
/// previous single-attached view.
#[allow(clippy::too_many_arguments)]
pub fn render_panes(
    f: &mut Frame,
    panes: &[PaneSpec<'_>],
    chip_area: Rect,
    status_area: Rect,
    footer_area: Rect,
    footer_label: &str,
    multi_pane_footer: bool,
    attention_line: Option<Line<'static>>,
    pinned: &[PinnedCommand],
    theme: &Theme,
) -> Vec<Rect> {
    let show_titles = panes.len() > 1;

    for pane in panes {
        render_one_pane(f, pane, show_titles, theme);
    }

    if let Some(line) = attention_line {
        let mut spans = vec![Span::raw(" ".to_string())];
        spans.extend(line.spans);
        f.render_widget(Paragraph::new(Line::from(spans)), status_area);
    }

    f.render_widget(
        Paragraph::new(footer_line(footer_label, multi_pane_footer, theme)),
        footer_area,
    );

    if !pinned.is_empty() {
        render_chip_row(f, chip_area, pinned, theme)
    } else {
        Vec::new()
    }
}

fn render_one_pane(f: &mut Frame, pane: &PaneSpec<'_>, show_title: bool, theme: &Theme) {
    let (title_area, term_area) = if show_title {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(pane.rect);
        (Some(chunks[0]), chunks[1])
    } else {
        (None, pane.rect)
    };

    if let Some(area) = title_area {
        // V5-style: ▎ gutter in accent color when focused, idle when not;
        // workspace name in bold. Focused row gets the selection bg fill
        // so the focus indicator is unmistakable even at a glance.
        let gutter_style = if pane.focused {
            Style::default().fg(theme.waiting)
        } else {
            Style::default().fg(theme.idle)
        };
        let name_style = if pane.focused {
            theme
                .selected_style()
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(theme.dim)
                .add_modifier(Modifier::BOLD)
        };
        let row_bg = if pane.focused {
            Style::default().bg(theme.selected_bg)
        } else {
            Style::default()
        };
        let spans = vec![
            Span::styled("▎".to_string(), gutter_style),
            Span::styled(format!(" {} ", pane.label), name_style),
        ];
        f.render_widget(Paragraph::new(Line::from(spans)).style(row_bg), area);
    }

    let offset = pane
        .session
        .scrollback_offset
        .load(std::sync::atomic::Ordering::Relaxed);
    let mut parser = pane.session.parser.lock().unwrap();
    parser.set_scrollback(offset);
    let screen = parser.screen();
    render_screen(screen, f.buffer_mut(), term_area);
    if pane.focused {
        let (cy, cx) = screen.cursor_position();
        if !screen.hide_cursor() && offset == 0 {
            f.set_cursor_position((term_area.x + cx, term_area.y + cy));
        }
    }
    drop(parser);
}

/// Carve the attached view's full `area` into pane / chip / status /
/// footer sub-areas. Empty-height rects are returned for absent rows so
/// the caller can pass them straight through to `render_panes`.
pub fn layout_chrome(
    area: Rect,
    attention_present: bool,
    pinned_present: bool,
) -> (Rect, Rect, Rect, Rect) {
    let chip_h = if pinned_present { 1 } else { 0 };
    let status_h = if attention_present { 1 } else { 0 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(chip_h),
            Constraint::Length(status_h),
            Constraint::Length(1),
        ])
        .split(area);
    (chunks[0], chunks[1], chunks[2], chunks[3])
}

/// Resize a session's PTY to fill its pane area (minus a per-pane title
/// row when `multi_pane` is true).
pub fn resize_pane(session: &Arc<Session>, pane_rect: Rect, multi_pane: bool) {
    let title: u16 = if multi_pane { 1 } else { 0 };
    let _ = session.resize(pane_rect.width, pane_rect.height.saturating_sub(title));
}

/// V5-styled footer: workspace label in `header_style`, then the `^x`
/// leader, then per-keybind chips (`<key>` in dim+bold, ` <label>` in
/// `path` color), separated by 2 spaces. Matches the dashboard footer's
/// chip pattern.
fn footer_line(label: &str, multi_pane: bool, theme: &Theme) -> Line<'static> {
    let keys: &[(&str, &str)] = if multi_pane {
        &[
            ("d", "close-pane"),
            ("←→", "focus"),
            ("u", "updates"),
            ("e", "edit"),
            ("t", "term"),
            ("v", "diff"),
            ("k", "procs"),
            ("x", "send-^x"),
        ]
    } else {
        &[
            ("d", "detach"),
            ("u", "updates"),
            ("e", "edit"),
            ("t", "term"),
            ("v", "diff"),
            ("k", "procs"),
            ("x", "send-^x"),
        ]
    };
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(2 + keys.len() * 2 + 3);
    spans.push(Span::raw(" ".to_string()));
    spans.push(Span::styled(label.to_string(), theme.header_style()));
    spans.push(Span::raw("   ".to_string()));
    spans.push(Span::styled(
        "^x".to_string(),
        Style::default().fg(theme.dim).add_modifier(Modifier::BOLD),
    ));
    for (key, lbl) in keys {
        spans.push(Span::raw("  ".to_string()));
        spans.push(Span::styled(
            (*key).to_string(),
            Style::default().fg(theme.dim).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {lbl}"),
            Style::default().fg(theme.path),
        ));
    }
    Line::from(spans)
}

/// Compute the clickable Rect for each chip that fits within `area`.
/// Returns one Rect per chip rendered left-to-right; chips that don't fit
/// are dropped from the end. The chip text is `<N> <label>` joined by
/// 2-space gaps (V5 footer convention). Labels are individually
/// truncated to 12 columns first.
pub fn layout_chip_row(area: Rect, pinned: &[PinnedCommand]) -> Vec<Rect> {
    let mut rects = Vec::new();
    let mut x = area.x;
    let max_x = area.x.saturating_add(area.width);
    const GAP: u16 = 2;
    for (i, cmd) in pinned.iter().enumerate().take(9) {
        let label = truncate_label(&cmd.label, 12);
        // Chip text: "N label"  (2 chars for "N " plus label chars)
        let chip_chars = 2 + label.chars().count() as u16;
        if i > 0 {
            x = x.saturating_add(GAP);
        }
        if x.saturating_add(chip_chars) > max_x {
            break;
        }
        rects.push(Rect {
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
    area: Rect,
    pinned: &[PinnedCommand],
    theme: &Theme,
) -> Vec<Rect> {
    let rects = layout_chip_row(area, pinned);
    let key_style = Style::default().fg(theme.dim).add_modifier(Modifier::BOLD);
    let label_style = Style::default().fg(theme.path);
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(rects.len() * 3);
    for (i, (_rect, cmd)) in rects.iter().zip(pinned.iter()).enumerate() {
        if i > 0 {
            spans.push(Span::raw("  ".to_string()));
        }
        let label = truncate_label(&cmd.label, 12);
        spans.push(Span::styled(format!("{}", i + 1), key_style));
        spans.push(Span::styled(format!(" {label}"), label_style));
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
