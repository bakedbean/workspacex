use ratatui::style::{Color, Style};

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// Repo headers, modal titles, "wsx — Workspaces" banner.
    pub header_fg: Color,
    pub header_bg: Option<Color>,
    /// Selected row highlight. fg+bg are always paired.
    pub selected_fg: Color,
    pub selected_bg: Color,
    /// Sub-lines, empty hints, dashboard footer.
    pub dim: Color,
    /// Success/positive states (reserved for future per-span styling).
    pub ok: Color,
    /// Warning/in-progress states (reserved).
    pub warn: Color,
    /// Error states and the error modal.
    pub err: Color,
    /// Attention markers (`!`, awaiting-permission) (reserved).
    pub attention: Color,
}

impl Theme {
    /// Default theme using ANSI-named colors so the user's terminal palette
    /// is respected. Matches the original wsx look.
    pub fn default_theme() -> Self {
        Self {
            header_fg: Color::Cyan,
            header_bg: Some(Color::Reset),
            selected_fg: Color::Black,
            selected_bg: Color::Cyan,
            dim: Color::DarkGray,
            ok: Color::Green,
            warn: Color::Yellow,
            err: Color::Red,
            attention: Color::Magenta,
        }
    }

    pub fn dracula() -> Self {
        // https://draculatheme.com/contribute
        let purple = Color::Rgb(0xbd, 0x93, 0xf9);
        let pink = Color::Rgb(0xff, 0x79, 0xc6);
        let cyan = Color::Rgb(0x8b, 0xe9, 0xfd);
        let green = Color::Rgb(0x50, 0xfa, 0x7b);
        let yellow = Color::Rgb(0xf1, 0xfa, 0x8c);
        let red = Color::Rgb(0xff, 0x55, 0x55);
        let comment = Color::Rgb(0x62, 0x72, 0xa4);
        let bg = Color::Rgb(0x28, 0x2a, 0x36);
        let _ = cyan; // reserved for later use
        Self {
            header_fg: purple,
            header_bg: None,
            selected_fg: bg,
            selected_bg: pink,
            dim: comment,
            ok: green,
            warn: yellow,
            err: red,
            attention: pink,
        }
    }

    pub fn nord() -> Self {
        // https://www.nordtheme.com/docs/colors-and-palettes
        let polar0 = Color::Rgb(0x2e, 0x34, 0x40); // background
        let polar3 = Color::Rgb(0x4c, 0x56, 0x6a); // muted
        let frost1 = Color::Rgb(0x88, 0xc0, 0xd0); // header cyan
        let aurora_red = Color::Rgb(0xbf, 0x61, 0x6a);
        let aurora_orange = Color::Rgb(0xd0, 0x87, 0x70);
        let aurora_yellow = Color::Rgb(0xeb, 0xcb, 0x8b);
        let aurora_green = Color::Rgb(0xa3, 0xbe, 0x8c);
        Self {
            header_fg: frost1,
            header_bg: None,
            selected_fg: polar0,
            selected_bg: frost1,
            dim: polar3,
            ok: aurora_green,
            warn: aurora_yellow,
            err: aurora_red,
            attention: aurora_orange,
        }
    }

    /// Look up a theme by name. Unknown names fall back to the default.
    pub fn by_name(name: &str) -> Self {
        match name {
            "dracula" => Self::dracula(),
            "nord" => Self::nord(),
            _ => Self::default_theme(),
        }
    }

    pub fn header_style(&self) -> Style {
        let mut s = Style::default().fg(self.header_fg);
        if let Some(bg) = self.header_bg {
            s = s.bg(bg);
        }
        s
    }
    pub fn selected_style(&self) -> Style {
        Style::default().fg(self.selected_fg).bg(self.selected_bg)
    }
    pub fn dim_style(&self) -> Style {
        Style::default().fg(self.dim)
    }
    pub fn ok_style(&self) -> Style {
        Style::default().fg(self.ok)
    }
    pub fn warn_style(&self) -> Style {
        Style::default().fg(self.warn)
    }
    pub fn err_style(&self) -> Style {
        Style::default().fg(self.err)
    }
    pub fn attention_style(&self) -> Style {
        Style::default().fg(self.attention)
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::default_theme()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn by_name_resolves_known_themes() {
        // Use a discriminating field. Default and Dracula differ on `header_fg`.
        assert_eq!(Theme::by_name("default").header_fg, Color::Cyan);
        assert!(matches!(
            Theme::by_name("dracula").header_fg,
            Color::Rgb(0xbd, 0x93, 0xf9)
        ));
        assert!(matches!(
            Theme::by_name("nord").header_fg,
            Color::Rgb(0x88, 0xc0, 0xd0)
        ));
    }

    #[test]
    fn unknown_theme_falls_back_to_default() {
        assert_eq!(Theme::by_name("bogus").header_fg, Color::Cyan);
        assert_eq!(Theme::by_name("").header_fg, Color::Cyan);
    }
}
