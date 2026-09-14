//! Test-only comparison helpers for the bar engine. Parity tests render a
//! legacy line and an engine line into one-row buffers and compare cells.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

pub fn plain(line: &Line<'_>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

pub fn render_line(line: &Line<'_>, width: u16) -> Buffer {
    let mut term = Terminal::new(TestBackend::new(width, 1)).unwrap();
    term.draw(|f| f.render_widget(Paragraph::new(line.clone()), f.area()))
        .unwrap();
    term.backend().buffer().clone()
}

/// Compare row `ya` of `a` with row `yb` of `b` over `width` cells: symbol
/// and background everywhere; foreground and modifiers only where the
/// symbol is not whitespace (the grammar puts fg/bold on pad cells, which
/// is invisible on screen).
pub fn assert_rows_match(a: &Buffer, ya: u16, b: &Buffer, yb: u16, width: u16) {
    let row = |buf: &Buffer, y: u16| -> String {
        (0..width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect()
    };
    let (ta, tb) = (row(a, ya), row(b, yb));
    for x in 0..width {
        let (ca, cb) = (&a[(x, ya)], &b[(x, yb)]);
        assert_eq!(
            ca.symbol(),
            cb.symbol(),
            "symbol at col {x}\n expected: {ta:?}\n actual:   {tb:?}"
        );
        assert_eq!(ca.bg, cb.bg, "bg at col {x} ({:?})\n {ta:?}", ca.symbol());
        if ca.symbol().trim().is_empty() {
            continue;
        }
        assert_eq!(ca.fg, cb.fg, "fg at col {x} ({:?})\n {ta:?}", ca.symbol());
        assert_eq!(
            ca.modifier,
            cb.modifier,
            "modifier at col {x} ({:?})\n {ta:?}",
            ca.symbol()
        );
    }
}

pub fn assert_lines_match(expected: &Line<'_>, actual: &Line<'_>, width: u16) {
    let (a, b) = (render_line(expected, width), render_line(actual, width));
    assert_rows_match(&a, 0, &b, 0, width);
}
