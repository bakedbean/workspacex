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

/// Black or white ink, whichever reads against palette entry `index`.
///
/// The cursor and the in-use marker are drawn ON the swatch, so they cannot
/// use the swatch's own color — a `[]` painted in #000000 on the black swatch
/// is invisible. Uses the standard sRGB luma weights with a mid threshold.
fn contrast_ink(index: u8) -> Color {
    let (r, g, b) = name_color::rgb(index);
    let luma = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
    if luma > 140.0 {
        Color::Black
    } else {
        Color::White
    }
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

    // `panel_frame` centers by clamping to `area`, which would silently CLIP
    // the grid: the swatches past the edge stop being drawn, but their hit
    // rects would still be published — and `handle_mouse` consults those
    // before the click-outside dismissal, so a click on empty screen would
    // apply an invisible color. Refuse to draw a grid that does not fit, and
    // return no hit rects. Narrowing the filter shrinks `h`, so the picker
    // becomes usable again on a short screen as soon as the user types.
    if area.width < w || area.height < h {
        let inner = panel_frame(f, area, w.min(area.width), 5.min(area.height), "", theme);
        f.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    " terminal too small to pick a color".to_string(),
                    theme.err_style(),
                )),
                Line::from(Span::styled(
                    format!(" needs {w}x{h}, have {}x{}", area.width, area.height),
                    theme.dim_style(),
                )),
                Line::from(Span::styled(
                    // The grid is a fixed 16 columns wide, so a filter can
                    // only ever buy back HEIGHT. Telling a user on a narrow
                    // screen to type would send them at a notice that never
                    // moves.
                    if area.width < w {
                        " needs a wider terminal — Esc".to_string()
                    } else {
                        " type to narrow the grid, or Esc".to_string()
                    },
                    theme.dim_style(),
                )),
            ])
            .wrap(ratatui::widgets::Wrap { trim: false }),
            inner,
        );
        return Vec::new();
    }

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
                // A plain swatch is a solid block in its own color. The cursor
                // and the in-use marker instead carry the color as BACKGROUND
                // and draw their glyph in contrasting ink, so the cell still
                // shows the color AND the marker stays readable on it.
                let (glyph, style) = if flat == selected {
                    (
                        "[]",
                        Style::default()
                            .fg(contrast_ink(idx))
                            .bg(name_color::color(idx)),
                    )
                } else if Some(idx) == current {
                    (
                        "\u{2713} ",
                        Style::default()
                            .fg(contrast_ink(idx))
                            .bg(name_color::color(idx)),
                    )
                } else {
                    (
                        "\u{2588}\u{2588}",
                        Style::default().fg(name_color::color(idx)),
                    )
                };
                spans.push(Span::styled(glyph.to_string(), style));
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
            Span::styled(
                if current == Some(idx) {
                    "  (current)".to_string()
                } else {
                    String::new()
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
    fn contrast_ink_flips_with_the_swatch_luminance() {
        assert_eq!(
            contrast_ink(0),
            Color::White,
            "black swatch needs light ink"
        );
        assert_eq!(
            contrast_ink(15),
            Color::Black,
            "white swatch needs dark ink"
        );
        assert_eq!(contrast_ink(226), Color::Black, "bright yellow is light");
        assert_eq!(contrast_ink(17), Color::White, "navy is dark");
    }

    /// The single buffer cell at `rect`'s origin, as (symbol, fg, bg).
    fn cell_at(
        filter: &str,
        selected: usize,
        current: Option<u8>,
        want: u8,
    ) -> (String, Color, Color) {
        let theme = Theme::ansi();
        let mut term = Terminal::new(TestBackend::new(80, 30)).unwrap();
        let mut rects = Vec::new();
        term.draw(|f| {
            rects = render_name_color_picker(f, f.area(), filter, selected, current, &theme);
        })
        .unwrap();
        let (_, rect) = rects
            .iter()
            .find(|(i, _)| *i == want)
            .unwrap_or_else(|| panic!("swatch {want} drawn"));
        let cell = &term.backend().buffer()[(rect.x, rect.y)];
        (cell.symbol().to_string(), cell.fg, cell.bg)
    }

    #[test]
    fn the_cursor_swatch_keeps_its_color_and_stays_legible_on_black() {
        // Index 0 is #000000: drawing the cursor glyph in the swatch's own
        // color made it invisible. It carries the color as BACKGROUND with
        // contrasting ink instead.
        let (symbol, fg, bg) = cell_at("", 0, None, 0);
        assert_eq!(
            bg,
            Color::Indexed(0),
            "the cursor cell still shows its color"
        );
        assert_eq!(fg, Color::White, "marked with ink that reads against it");
        assert_eq!(symbol, "[");
    }

    #[test]
    fn the_applied_color_is_marked_in_the_grid() {
        let (symbol, fg, bg) = cell_at("", 200, Some(21), 21);
        assert_eq!(bg, Color::Indexed(21));
        assert_eq!(fg, contrast_ink(21));
        assert_eq!(symbol, "\u{2713}", "a check marks the color in use");
    }

    #[test]
    fn a_plain_swatch_is_a_solid_block_in_its_own_color() {
        let (symbol, fg, _) = cell_at("", 0, None, 180);
        assert_eq!(symbol, "\u{2588}");
        assert_eq!(fg, Color::Indexed(180));
    }

    #[test]
    fn the_info_line_says_when_the_cursor_sits_on_the_applied_color() {
        let (_, on) = {
            let idx = name_color::matching("")
                .iter()
                .position(|&i| i == 21)
                .unwrap();
            render_with_current("", idx, Some(21))
        };
        assert!(on.contains("(current)"), "expected the marker:\n{on}");
        let (_, off) = render_with_current("", 0, Some(21));
        assert!(!off.contains("(current)"), "only when focused:\n{off}");
    }

    fn render_with_current(
        filter: &str,
        selected: usize,
        current: Option<u8>,
    ) -> (Vec<(u8, Rect)>, String) {
        let theme = Theme::ansi();
        let mut term = Terminal::new(TestBackend::new(80, 30)).unwrap();
        let mut rects = Vec::new();
        term.draw(|f| {
            rects = render_name_color_picker(f, f.area(), filter, selected, current, &theme);
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
    fn a_filter_matching_nothing_says_so_instead_of_drawing_an_empty_grid() {
        let (rects, text) = render("zzz", 0);
        assert!(rects.is_empty());
        assert!(
            text.contains("no color matches"),
            "expected a no-match notice:\n{text}"
        );
    }

    #[test]
    fn a_screen_too_small_for_the_panel_shows_a_notice_and_no_hit_rects() {
        let theme = Theme::ansi();
        let mut term = Terminal::new(TestBackend::new(40, 18)).unwrap();
        let mut rects = Vec::new();
        term.draw(|f| {
            rects = render_name_color_picker(f, f.area(), "", 0, None, &theme);
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
        assert!(
            rects.is_empty(),
            "a clipped grid must publish no clickable swatches"
        );
        assert!(text.contains("too small"), "expected a notice:\n{text}");
    }

    /// The picker's rendered text on a `w`x`h` screen.
    fn text_at(w: u16, h: u16, filter: &str) -> String {
        let theme = Theme::ansi();
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| {
            render_name_color_picker(f, f.area(), filter, 0, None, &theme);
        })
        .unwrap();
        let buf = term.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn a_short_screen_is_told_that_narrowing_the_filter_helps() {
        // Wide enough, just not tall enough: filtering shrinks the grid, so
        // the picker really does become reachable by typing.
        let text = text_at(60, 12, "");
        assert!(text.contains("too small"), "{text}");
        assert!(text.contains("type to narrow"), "{text}");
    }

    #[test]
    fn a_narrow_screen_is_not_told_to_narrow_the_filter() {
        // The grid is a fixed 16 columns wide, so no filter can ever make it
        // fit a screen this narrow. Promising otherwise sends the user typing
        // at a notice that will not move.
        let text = text_at(40, 40, "");
        assert!(text.contains("too small"), "{text}");
        assert!(
            !text.contains("type to narrow"),
            "narrowing cannot help when width is the blocker:\n{text}"
        );
        assert!(text.contains("wider"), "say what would help:\n{text}");
    }

    #[test]
    fn a_filter_that_shrinks_the_panel_makes_it_usable_again_on_a_short_screen() {
        // Height is what a narrow terminal runs out of first, and narrowing the
        // filter shrinks the grid — so the feature comes back rather than
        // staying dead once the notice has been shown.
        let theme = Theme::ansi();
        let mut term = Terminal::new(TestBackend::new(60, 12)).unwrap();
        let mut rects = Vec::new();
        term.draw(|f| {
            rects = render_name_color_picker(f, f.area(), "d7af87", 0, None, &theme);
        })
        .unwrap();
        assert_eq!(rects.len(), 1, "the single match is drawn and clickable");
    }

    #[test]
    fn every_published_hit_rect_lies_inside_the_screen() {
        // The hit rects are consulted BEFORE the click-outside dismissal, so a
        // rect outside the drawn panel would apply a color the user cannot see.
        for (w, h) in [(80u16, 30u16), (52, 23), (51, 24), (120, 40)] {
            let theme = Theme::ansi();
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            let mut rects = Vec::new();
            term.draw(|f| {
                rects = render_name_color_picker(f, f.area(), "", 0, None, &theme);
            })
            .unwrap();
            for (idx, r) in &rects {
                assert!(
                    r.x + r.width <= w && r.y + r.height <= h,
                    "swatch {idx} at {r:?} escapes the {w}x{h} screen",
                );
            }
        }
    }

    #[test]
    fn an_out_of_range_selection_does_not_panic() {
        // The selection is clamped on filter edits, but a stale index must
        // never take the renderer down.
        let (rects, _) = render("d7af", 999);
        assert!(!rects.is_empty());
    }
}
