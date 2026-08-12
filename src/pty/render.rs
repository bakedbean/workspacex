use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use vt100::{Color as VtColor, Screen};

/// Render a `vt100::Screen` into the given Ratatui buffer rect.
/// `area.width` / `area.height` are the visible cells; rows beyond the
/// screen height are blanked.
pub fn render_screen(screen: &Screen, buf: &mut Buffer, area: Rect) {
    let (rows, cols) = screen.size();
    for y in 0..area.height {
        for x in 0..area.width {
            let cell_buf = &mut buf[(area.x + x, area.y + y)];
            if y >= rows || x >= cols {
                cell_buf.reset();
                continue;
            }
            let Some(cell) = screen.cell(y, x) else {
                cell_buf.reset();
                continue;
            };
            // `contents()` heap-allocates a String on every call, and this runs
            // once per cell per frame. `has_contents()` answers the blank case
            // without allocating, and blank cells dominate a typical pane.
            //
            // Only safe as a one-way fast path: `has_contents()` tests vt100's
            // bit-packed length byte, whose high bits flag wide characters, so
            // a wide *continuation* cell reports `true` while `contents()` is
            // still empty. False therefore guarantees empty; true does not
            // guarantee non-empty, and must fall through to the exact check —
            // otherwise every wide glyph collapses to a zero-width symbol.
            if !cell.has_contents() {
                cell_buf.set_symbol(" ");
            } else {
                let glyph = cell.contents();
                cell_buf.set_symbol(if glyph.is_empty() {
                    " "
                } else {
                    glyph.as_str()
                });
            }
            cell_buf.set_style(convert_style(cell));
        }
    }
}

fn convert_style(cell: &vt100::Cell) -> Style {
    let mut style = Style::default()
        .fg(convert_color(cell.fgcolor()))
        .bg(convert_color(cell.bgcolor()));
    let mut mods = Modifier::empty();
    if cell.bold() {
        mods |= Modifier::BOLD;
    }
    if cell.italic() {
        mods |= Modifier::ITALIC;
    }
    if cell.underline() {
        mods |= Modifier::UNDERLINED;
    }
    if cell.inverse() {
        mods |= Modifier::REVERSED;
    }
    style.add_modifier = mods;
    style
}

fn convert_color(c: VtColor) -> Color {
    match c {
        VtColor::Default => Color::Reset,
        VtColor::Idx(i) => Color::Indexed(i),
        VtColor::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vt100::Parser;

    #[test]
    fn renders_plain_text() {
        let mut p = Parser::new(3, 10, 0);
        p.process(b"hello");
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 3));
        render_screen(p.screen(), &mut buf, Rect::new(0, 0, 10, 3));
        let line: String = (0..5).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        assert_eq!(line, "hello");
    }

    #[test]
    fn blank_cells_render_as_a_space() {
        // The allocation-free fast path. Untouched cells must still produce a
        // space, not an empty symbol.
        let mut p = Parser::new(2, 10, 0);
        p.process(b"ab");
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 2));
        render_screen(p.screen(), &mut buf, Rect::new(0, 0, 10, 2));
        for x in 2..10 {
            assert_eq!(buf[(x, 0)].symbol(), " ", "blank cell at x={x}");
        }
        assert_eq!(buf[(0, 1)].symbol(), " ", "blank row");
    }

    #[test]
    fn wide_glyph_continuation_cell_renders_as_a_space() {
        // Regression guard for the fast path: vt100 packs wide-character flags
        // into the same byte `has_contents()` tests, so a continuation cell
        // reports "has contents" while its contents are empty. Taking the fast
        // path on it would emit a zero-width symbol and break CJK/emoji panes.
        let mut p = Parser::new(2, 10, 0);
        p.process("世界".as_bytes());
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 2));
        render_screen(p.screen(), &mut buf, Rect::new(0, 0, 10, 2));

        assert_eq!(buf[(0, 0)].symbol(), "世");
        assert_eq!(
            buf[(1, 0)].symbol(),
            " ",
            "continuation cell must not be a zero-width symbol"
        );
        assert_eq!(buf[(2, 0)].symbol(), "界");
        assert_eq!(buf[(3, 0)].symbol(), " ");
    }

    #[test]
    fn combining_characters_survive_the_fast_path() {
        // A cell can hold several codepoints; the fast path must not truncate
        // them to the first.
        let mut p = Parser::new(2, 10, 0);
        p.process("e\u{0301}".as_bytes()); // e + combining acute
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 2));
        render_screen(p.screen(), &mut buf, Rect::new(0, 0, 10, 2));
        assert_eq!(buf[(0, 0)].symbol(), "e\u{0301}");
    }

    #[test]
    fn renders_red_fg() {
        let mut p = Parser::new(2, 10, 0);
        p.process(b"\x1b[31mX");
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 2));
        render_screen(p.screen(), &mut buf, Rect::new(0, 0, 10, 2));
        assert_eq!(buf[(0, 0)].fg, Color::Indexed(1));
    }
}
