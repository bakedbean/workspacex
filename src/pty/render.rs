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
            let glyph = cell.contents();
            cell_buf.set_symbol(if glyph.is_empty() {
                " "
            } else {
                glyph.as_str()
            });
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
    fn renders_red_fg() {
        let mut p = Parser::new(2, 10, 0);
        p.process(b"\x1b[31mX");
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 2));
        render_screen(p.screen(), &mut buf, Rect::new(0, 0, 10, 2));
        assert_eq!(buf[(0, 0)].fg, Color::Indexed(1));
    }
}
