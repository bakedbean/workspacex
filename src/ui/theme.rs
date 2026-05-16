use ratatui::style::{Color, Modifier, Style};

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
    /// Repo header's path/count tail — distinct muted tone so the path
    /// doesn't blur into the workspace sub-lines (which use `dim`).
    pub path: Color,
    /// Powerline-style repo header segment bgs (gradient: dark → mid →
    /// accent), all sharing `seg_fg` for text contrast.
    pub seg_name_bg: Color,
    pub seg_path_bg: Color,
    pub seg_count_bg: Color,
    pub seg_fg: Color,
    /// Success/positive states (reserved for future per-span styling).
    pub ok: Color,
    /// Warning/in-progress states (reserved).
    pub warn: Color,
    /// Error states and the error modal.
    pub err: Color,
    /// Attention markers (`!`, awaiting-permission) (reserved).
    pub attention: Color,
    /// Merged-PR indicator on workspace rows.
    pub merged: Color,
}

impl Theme {
    /// Default theme using ANSI-named colors so the user's terminal palette
    /// is respected. Matches the original wsx look.
    pub fn default_theme() -> Self {
        Self {
            header_fg: Color::Cyan,
            header_bg: Some(Color::Reset),
            selected_fg: Color::White,
            selected_bg: Color::DarkGray,
            dim: Color::DarkGray,
            path: Color::Indexed(67), // 256-color SteelBlue3 — muted blue
            // Powerline gradient: subtle, dark steps.
            seg_name_bg: Color::Indexed(233),
            seg_path_bg: Color::Indexed(235),
            seg_count_bg: Color::Indexed(237),
            seg_fg: Color::Indexed(250),
            ok: Color::Green,
            warn: Color::Yellow,
            err: Color::Red,
            attention: Color::Magenta,
            merged: Color::Magenta,
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
        let current_line = Color::Rgb(0x44, 0x47, 0x5a);
        let foreground = Color::Rgb(0xf8, 0xf8, 0xf2);
        let _ = cyan; // reserved for later use
        Self {
            header_fg: purple,
            header_bg: None,
            selected_fg: foreground,
            selected_bg: current_line,
            dim: comment,
            path: Color::Rgb(0x82, 0x90, 0xb4), // softer blue-grey
            seg_name_bg: Color::Rgb(0x1d, 0x1f, 0x29),
            seg_path_bg: Color::Rgb(0x26, 0x28, 0x33),
            seg_count_bg: Color::Rgb(0x2f, 0x32, 0x3d),
            seg_fg: foreground,
            ok: green,
            warn: yellow,
            err: red,
            attention: pink,
            merged: purple,
        }
    }

    pub fn jellybeans() -> Self {
        // https://github.com/nanotech/jellybeans.vim
        let bg = Color::Rgb(0x15, 0x15, 0x15);
        let blue = Color::Rgb(0x81, 0x97, 0xbf); // keyword
        let yellow = Color::Rgb(0xfa, 0xd0, 0x7a); // function
        let orange = Color::Rgb(0xff, 0xb9, 0x64); // type / warning
        let red = Color::Rgb(0xcf, 0x6a, 0x4c); // number / error
        let green = Color::Rgb(0x99, 0xad, 0x6a); // string
        let purple = Color::Rgb(0xc6, 0xb6, 0xee); // variable
        let gray = Color::Rgb(0x88, 0x88, 0x88); // comment
        let cursor_line = Color::Rgb(0x33, 0x33, 0x33); // jellybeans CursorLine
        let fg = Color::Rgb(0xe8, 0xe8, 0xd3); // jellybeans Normal fg
        let _ = (bg, yellow);
        Self {
            header_fg: blue,
            header_bg: None,
            selected_fg: fg,
            selected_bg: cursor_line,
            dim: gray,
            path: Color::Rgb(0xb8, 0xa0, 0x78), // warmer tan, clearer separation from gray
            seg_name_bg: Color::Rgb(0x10, 0x10, 0x10),
            seg_path_bg: Color::Rgb(0x1c, 0x1c, 0x1c),
            seg_count_bg: Color::Rgb(0x26, 0x26, 0x26),
            seg_fg: fg,
            ok: green,
            warn: orange,
            err: red,
            attention: purple,
            merged: purple,
        }
    }

    pub fn nord() -> Self {
        // https://www.nordtheme.com/docs/colors-and-palettes
        let polar0 = Color::Rgb(0x2e, 0x34, 0x40); // background
        let polar1 = Color::Rgb(0x3b, 0x42, 0x52); // selection bg
        let polar3 = Color::Rgb(0x4c, 0x56, 0x6a); // muted
        let snow_storm1 = Color::Rgb(0xd8, 0xde, 0xe9);
        let frost1 = Color::Rgb(0x88, 0xc0, 0xd0); // header cyan
        let aurora_red = Color::Rgb(0xbf, 0x61, 0x6a);
        let aurora_orange = Color::Rgb(0xd0, 0x87, 0x70);
        let aurora_yellow = Color::Rgb(0xeb, 0xcb, 0x8b);
        let aurora_green = Color::Rgb(0xa3, 0xbe, 0x8c);
        let aurora_purple = Color::Rgb(0xb4, 0x8e, 0xad);
        let _ = polar0;
        Self {
            header_fg: frost1,
            header_bg: None,
            selected_fg: snow_storm1,
            selected_bg: polar1,
            dim: polar3,
            path: Color::Rgb(0x81, 0xa1, 0xc1), // Nord frost3
            seg_name_bg: Color::Rgb(0x22, 0x26, 0x2e),
            seg_path_bg: Color::Rgb(0x2a, 0x2e, 0x36),
            seg_count_bg: Color::Rgb(0x32, 0x37, 0x42),
            seg_fg: snow_storm1,
            ok: aurora_green,
            warn: aurora_yellow,
            err: aurora_red,
            attention: aurora_orange,
            merged: aurora_purple,
        }
    }

    /// Look up a theme by name. Unknown names fall back to the default.
    pub fn by_name(name: &str) -> Self {
        match name {
            "dracula" => Self::dracula(),
            "jellybeans" => Self::jellybeans(),
            "nord" => Self::nord(),
            _ => Self::default_theme(),
        }
    }

    pub fn header_style(&self) -> Style {
        let mut s = Style::default()
            .fg(self.header_fg)
            .add_modifier(Modifier::BOLD);
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
    pub fn path_style(&self) -> Style {
        Style::default().fg(self.path)
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
    pub fn merged_style(&self) -> Style {
        Style::default().fg(self.merged)
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
        assert!(matches!(
            Theme::by_name("jellybeans").header_fg,
            Color::Rgb(0x81, 0x97, 0xbf)
        ));
    }

    #[test]
    fn unknown_theme_falls_back_to_default() {
        assert_eq!(Theme::by_name("bogus").header_fg, Color::Cyan);
        assert_eq!(Theme::by_name("").header_fg, Color::Cyan);
    }
}
