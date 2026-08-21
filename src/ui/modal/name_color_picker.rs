//! The `C` per-workspace name-color picker: an xterm-256 swatch grid with a
//! hex/name filter box.

use super::*;
use crate::config::name_color;

/// Swatches per grid row. 16 divides 256 exactly, so the unfiltered palette
/// fills a clean 16x16 block and the ANSI/cube/gray sections line up.
pub const GRID_COLS: usize = 16;

/// Cells one swatch occupies including its trailing separator.
const SWATCH_STRIDE: u16 = 3;
/// Visible width of a swatch itself ("\u{2588}\u{2588}").
const SWATCH_WIDTH: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// Move the grid cursor within a filtered list of `len` swatches. Horizontal
/// steps move one swatch, vertical steps a whole row; every direction clamps
/// rather than wrapping, so holding a key parks at an edge instead of jumping
/// across the palette.
pub fn move_selection(selected: usize, len: usize, dir: Dir) -> usize {
    if len == 0 {
        return 0;
    }
    let last = len - 1;
    match dir {
        Dir::Left => selected.saturating_sub(1),
        Dir::Right => (selected + 1).min(last),
        // No row above: stay put rather than snapping to the first swatch.
        Dir::Up => selected.checked_sub(GRID_COLS).unwrap_or(selected),
        // A partial last row can leave no swatch directly below; land on the
        // final one instead of refusing to move.
        Dir::Down => (selected + GRID_COLS).min(last),
    }
}

/// Draw the centered swatch grid for `filter`, with `selected` indexing the
/// FILTERED list and `current` marking the workspace's existing color.
/// Returns `(palette index, rect)` per drawn swatch for click hit-testing.
pub fn render_name_color_picker(
    f: &mut Frame,
    area: Rect,
    filter: &str,
    selected: usize,
    current: Option<u8>,
    theme: &Theme,
) -> Vec<(u8, Rect)> {
    let hits = name_color::matching(filter);
    let rows = hits.len().div_ceil(GRID_COLS).max(1);

    // 1 filter line + blank + grid + blank + info + footer, inside the border.
    let grid_w = GRID_COLS as u16 * SWATCH_STRIDE - 1;
    let w = grid_w + 4; // 1 cell of padding each side + 2 border columns
    let h = rows as u16 + 7;
    let inner = panel_frame(f, area, w, h, "workspace name color", theme);
    if inner.width == 0 || inner.height == 0 {
        return Vec::new();
    }

    let cursor = hits.get(selected).copied();
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(" filter: ".to_string(), theme.dim_style()),
        Span::styled(filter.to_string(), theme.header_style()),
        Span::styled("\u{2588}".to_string(), theme.header_style()),
    ]));
    lines.push(Line::from(""));

    let mut rects: Vec<(u8, Rect)> = Vec::new();
    if hits.is_empty() {
        lines.push(Line::from(Span::styled(
            format!(" no color matches \u{201c}{}\u{201d}", filter.trim()),
            theme.err_style(),
        )));
    } else {
        for (row, chunk) in hits.chunks(GRID_COLS).enumerate() {
            let mut spans: Vec<Span<'static>> = vec![Span::raw(" ")];
            for (col, &idx) in chunk.iter().enumerate() {
                let flat = row * GRID_COLS + col;
                // The cursor and the applied color are drawn as different
                // glyphs rather than different colors: the swatch IS its
                // color, so any recoloring would misrepresent it.
                let glyph = if flat == selected {
                    "[]"
                } else if Some(idx) == current {
                    "\u{2593}\u{2593}"
                } else {
                    "\u{2588}\u{2588}"
                };
                spans.push(Span::styled(
                    glyph.to_string(),
                    Style::default().fg(name_color::color(idx)),
                ));
                spans.push(Span::raw(" "));
                rects.push((
                    idx,
                    Rect {
                        x: inner.x + 1 + col as u16 * SWATCH_STRIDE,
                        y: inner.y + 2 + row as u16,
                        width: SWATCH_WIDTH,
                        height: 1,
                    },
                ));
            }
            lines.push(Line::from(spans));
        }
    }

    lines.push(Line::from(""));
    lines.push(match cursor {
        Some(idx) => Line::from(vec![
            Span::styled(" ".to_string(), theme.dim_style()),
            Span::styled(
                "\u{2588}\u{2588} ".to_string(),
                Style::default().fg(name_color::color(idx)),
            ),
            Span::styled(format!("{idx:<4}"), theme.header_style()),
            Span::styled(format!("#{}", name_color::hex(idx)), theme.header_style()),
            Span::styled(
                match name_color::name(idx) {
                    "" => String::new(),
                    n => format!("  {n}"),
                },
                theme.dim_style(),
            ),
        ]),
        None => Line::from(""),
    });
    lines.push(Line::from(Span::styled(
        " \u{2190}\u{2191}\u{2193}\u{2192} move   Enter pick   Del reset   Esc cancel".to_string(),
        theme.dim_style(),
    )));

    f.render_widget(Paragraph::new(lines), inner);
    rects
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn left_and_right_step_one_swatch() {
        assert_eq!(move_selection(5, 256, Dir::Right), 6);
        assert_eq!(move_selection(5, 256, Dir::Left), 4);
    }

    #[test]
    fn stepping_past_either_end_stays_put() {
        assert_eq!(move_selection(0, 256, Dir::Left), 0);
        assert_eq!(move_selection(255, 256, Dir::Right), 255);
    }

    #[test]
    fn up_and_down_step_a_whole_row() {
        assert_eq!(move_selection(20, 256, Dir::Down), 20 + GRID_COLS);
        assert_eq!(move_selection(20, 256, Dir::Up), 20 - GRID_COLS);
    }

    #[test]
    fn up_from_the_first_row_stays_put() {
        assert_eq!(move_selection(3, 256, Dir::Up), 3);
    }

    #[test]
    fn down_from_a_partial_last_row_clamps_to_the_final_swatch() {
        // 20 entries = one full row of 16 plus a partial row of 4. Stepping
        // down from column 9 has no swatch below it, so it lands on the last.
        assert_eq!(move_selection(9, 20, Dir::Down), 19);
    }

    #[test]
    fn moving_within_an_empty_result_set_is_a_no_op() {
        for dir in [Dir::Left, Dir::Right, Dir::Up, Dir::Down] {
            assert_eq!(move_selection(0, 0, dir), 0, "{dir:?}");
        }
    }

    fn render(filter: &str, selected: usize) -> (Vec<(u8, Rect)>, String) {
        let theme = Theme::ansi();
        let mut term = Terminal::new(TestBackend::new(80, 30)).unwrap();
        let mut rects = Vec::new();
        term.draw(|f| {
            rects = render_name_color_picker(f, f.area(), filter, selected, Some(21), &theme);
        })
        .unwrap();
        let buf = term.backend().buffer();
        let text = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        (rects, text)
    }

    #[test]
    fn one_clickable_rect_per_filtered_color_in_palette_order() {
        let (rects, _) = render("d7af", 0);
        let expected = name_color::matching("d7af");
        assert!(expected.len() > 1, "filter should keep several colors");
        assert_eq!(
            rects.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
            expected,
            "rects carry the filtered indices, in palette order"
        );
    }

    #[test]
    fn swatches_lay_out_in_rows_of_sixteen() {
        let (rects, _) = render("", 0);
        assert_eq!(rects.len(), 256);
        assert_eq!(rects[15].1.y, rects[0].1.y, "16 swatches share a row");
        assert_eq!(rects[16].1.y, rects[0].1.y + 1, "the 17th wraps");
        assert_eq!(rects[16].1.x, rects[0].1.x, "and returns to column 0");
        assert!(rects[1].1.x > rects[0].1.x, "columns advance rightwards");
    }

    #[test]
    fn the_focused_swatch_is_named_by_index_and_hex() {
        let selected = name_color::matching("")
            .iter()
            .position(|&i| i == 180)
            .unwrap();
        let (_, text) = render("", selected);
        assert!(text.contains("180"), "shows the palette index:\n{text}");
        assert!(text.contains("d7af87"), "shows the hex:\n{text}");
    }

    #[test]
    fn the_filter_text_is_echoed_so_typing_is_visible() {
        let (_, text) = render("d7af", 0);
        assert!(text.contains("d7af"), "filter echoed:\n{text}");
    }

    #[test]
    fn a_filter_matching_nothing_says_so_instead_of_drawing_an_empty_grid() {
        let (rects, text) = render("zzz", 0);
        assert!(rects.is_empty());
        assert!(
            text.contains("no color matches"),
            "expected a no-match notice:\n{text}"
        );
    }

    #[test]
    fn an_out_of_range_selection_does_not_panic() {
        // The selection is clamped on filter edits, but a stale index must
        // never take the renderer down.
        let (rects, _) = render("d7af", 999);
        assert!(!rects.is_empty());
    }
}
