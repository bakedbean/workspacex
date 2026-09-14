//! Style-string grammar for `[text](style)` and segment `style` fields:
//! space-separated tokens `fg:<c>`, `bg:<c>`, a bare `<c>` (foreground),
//! the modifiers `bold`, `dimmed`, `italic`, `underline`, `none`, and
//! `$name` to patch in a named style (`$style`, `$mark_style`). A color is
//! `#rrggbb`, a 0–255 index, or a name resolved at load time against the
//! palette, then the theme tokens, then the ANSI names.

use crate::ui::theme::Theme;
use ratatui::style::{Color, Modifier, Style};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColorRef {
    /// `#rrggbb` or a 0–255 index: known at parse time.
    Literal(Color),
    /// A palette name, theme token, or ANSI name: resolved at load time.
    Named(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StyleSpec {
    pub fg: Option<ColorRef>,
    pub bg: Option<ColorRef>,
    pub modifiers: Modifier,
    /// `$name` tokens: named styles patched in (in order) before `fg`/`bg`/
    /// `modifiers` are applied on top.
    pub vars: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleError(pub String);

impl std::fmt::Display for StyleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Parse one color token. Names are kept unresolved.
pub fn color_ref(token: &str) -> Result<ColorRef, StyleError> {
    if let Some(hex) = token.strip_prefix('#') {
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(StyleError(format!(
                "bad hex color `{token}` (want #rrggbb)"
            )));
        }
        let value = u32::from_str_radix(hex, 16).expect("validated hex");
        return Ok(ColorRef::Literal(Color::Rgb(
            (value >> 16) as u8,
            (value >> 8) as u8,
            value as u8,
        )));
    }
    if !token.is_empty() && token.chars().all(|c| c.is_ascii_digit()) {
        return token
            .parse::<u8>()
            .map(|index| ColorRef::Literal(Color::Indexed(index)))
            .map_err(|_| StyleError(format!("color index `{token}` out of range (0-255)")));
    }
    if is_ident(token) {
        return Ok(ColorRef::Named(token.to_string()));
    }
    Err(StyleError(format!("bad color `{token}`")))
}

/// The 16 ANSI names, starship spelling: `red`, `bright-red`, `white` (ANSI 7,
/// ratatui `Gray`), `bright-white`, plus `gray`/`grey` and `dark-gray`/`dark-grey`.
pub fn ansi(name: &str) -> Option<Color> {
    let (bright, base) = match name.strip_prefix("bright-") {
        Some(base) => (true, base),
        None => (false, name),
    };
    Some(match (base, bright) {
        ("black", false) => Color::Black,
        ("black", true) => Color::DarkGray,
        ("red", false) => Color::Red,
        ("red", true) => Color::LightRed,
        ("green", false) => Color::Green,
        ("green", true) => Color::LightGreen,
        ("yellow", false) => Color::Yellow,
        ("yellow", true) => Color::LightYellow,
        ("blue", false) => Color::Blue,
        ("blue", true) => Color::LightBlue,
        ("magenta", false) => Color::Magenta,
        ("magenta", true) => Color::LightMagenta,
        ("cyan", false) => Color::Cyan,
        ("cyan", true) => Color::LightCyan,
        ("white", false) => Color::Gray,
        ("white", true) => Color::White,
        ("gray" | "grey", false) => Color::Gray,
        ("dark-gray" | "dark-grey", false) => Color::DarkGray,
        _ => return None,
    })
}

impl StyleSpec {
    pub fn parse(src: &str) -> Result<Self, StyleError> {
        let mut spec = Self::default();
        for token in src.split_whitespace() {
            match token {
                "bold" => spec.modifiers |= Modifier::BOLD,
                "dimmed" => spec.modifiers |= Modifier::DIM,
                "italic" => spec.modifiers |= Modifier::ITALIC,
                "underline" => spec.modifiers |= Modifier::UNDERLINED,
                "none" => {}
                _ => {
                    if let Some(name) = token.strip_prefix('$') {
                        if !is_ident(name) {
                            return Err(StyleError(format!("bad style variable `{token}`")));
                        }
                        spec.vars.push(name.to_string());
                    } else if let Some(color) = token.strip_prefix("fg:") {
                        spec.fg = Some(
                            color_ref(color)
                                .map_err(|error| StyleError(format!("fg: {}", error.0)))?,
                        );
                    } else if let Some(color) = token.strip_prefix("bg:") {
                        spec.bg = Some(
                            color_ref(color)
                                .map_err(|error| StyleError(format!("bg: {}", error.0)))?,
                        );
                    } else {
                        spec.fg = Some(color_ref(token)?);
                    }
                }
            }
        }
        Ok(spec)
    }
}

/// Resolves color names and `$style` variables. Cheap to build per frame:
/// two borrows plus a small owned map of named styles.
#[derive(Debug, Clone)]
pub struct Resolver<'a> {
    pub palette: &'a HashMap<String, Color>,
    pub theme: &'a Theme,
    pub styles: HashMap<String, Style>,
}

impl<'a> Resolver<'a> {
    pub fn new(palette: &'a HashMap<String, Color>, theme: &'a Theme) -> Self {
        Self {
            palette,
            theme,
            styles: HashMap::new(),
        }
    }

    /// Same palette and theme, different `$name` styles.
    pub fn with_styles(&self, styles: HashMap<String, Style>) -> Resolver<'a> {
        Resolver {
            palette: self.palette,
            theme: self.theme,
            styles,
        }
    }

    /// Palette name, then theme token, then ANSI name.
    pub fn color(&self, name: &str) -> Option<Color> {
        self.palette
            .get(name)
            .copied()
            .or_else(|| self.theme.token(name))
            .or_else(|| ansi(name))
    }

    fn color_of(&self, color: &ColorRef) -> Result<Color, StyleError> {
        match color {
            ColorRef::Literal(color) => Ok(*color),
            ColorRef::Named(name) => self
                .color(name)
                .ok_or_else(|| StyleError(format!("unknown color `{name}`"))),
        }
    }

    pub fn resolve(&self, spec: &StyleSpec) -> Result<Style, StyleError> {
        let mut style = Style::default();
        for name in &spec.vars {
            let named = self
                .styles
                .get(name)
                .ok_or_else(|| StyleError(format!("unknown style variable `${name}`")))?;
            style = style.patch(*named);
        }
        if let Some(fg) = &spec.fg {
            style = style.fg(self.color_of(fg)?);
        }
        if let Some(bg) = &spec.bg {
            style = style.bg(self.color_of(bg)?);
        }
        Ok(style.add_modifier(spec.modifiers))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_token_kind() {
        let spec =
            StyleSpec::parse("fg:#ff0000 bg:12 bold dimmed italic underline $style").unwrap();
        assert_eq!(spec.fg, Some(ColorRef::Literal(Color::Rgb(0xff, 0, 0))));
        assert_eq!(spec.bg, Some(ColorRef::Literal(Color::Indexed(12))));
        assert_eq!(
            spec.modifiers,
            Modifier::BOLD | Modifier::DIM | Modifier::ITALIC | Modifier::UNDERLINED
        );
        assert_eq!(spec.vars, vec!["style".to_string()]);
    }

    #[test]
    fn bare_color_is_foreground_and_none_does_not_reset_tokens() {
        assert_eq!(
            StyleSpec::parse("first").unwrap().fg,
            Some(ColorRef::Named("first".into()))
        );
        assert_eq!(StyleSpec::parse("none").unwrap(), StyleSpec::default());
        assert_eq!(StyleSpec::parse("").unwrap(), StyleSpec::default());
        assert_eq!(
            StyleSpec::parse("bold none dim").unwrap(),
            StyleSpec::parse("dim bold").unwrap()
        );
        let spec = StyleSpec::parse("dim dimmed").unwrap();
        assert_eq!(spec.fg, Some(ColorRef::Named("dim".into())));
        assert_eq!(spec.modifiers, Modifier::DIM);
    }

    #[test]
    fn parses_color_boundaries_and_identifiers() {
        for (src, expected) in [
            ("#01aBfF", ColorRef::Literal(Color::Rgb(1, 171, 255))),
            ("0", ColorRef::Literal(Color::Indexed(0))),
            ("255", ColorRef::Literal(Color::Indexed(255))),
            ("_accent-2", ColorRef::Named("_accent-2".into())),
        ] {
            assert_eq!(color_ref(src).unwrap(), expected, "{src}");
        }
        assert_eq!(
            StyleSpec::parse("$_mark-2").unwrap().vars,
            vec!["_mark-2".to_string()]
        );
    }

    #[test]
    fn rejects_malformed_tokens() {
        assert!(StyleSpec::parse("fg:").unwrap_err().0.contains("color"));
        for src in [
            "bg:",
            "#12345",
            "#gggggg",
            "256",
            "-1",
            "$",
            "$2bad",
            "$bad.name",
            "fg:bg:red",
            "2bad",
            "red:blue",
            "éclair",
        ] {
            assert!(StyleSpec::parse(src).is_err(), "accepted {src:?}");
        }
    }

    #[test]
    fn resolution_order_is_palette_then_theme_then_ansi() {
        let theme = Theme::wsx();
        let palette = HashMap::from([
            ("dim".to_string(), Color::Rgb(1, 2, 3)),
            ("red".to_string(), Color::Rgb(4, 5, 6)),
        ]);
        let resolver = Resolver::new(&palette, &theme);
        assert_eq!(resolver.color("dim"), Some(Color::Rgb(1, 2, 3)));
        assert_eq!(resolver.color("red"), Some(Color::Rgb(4, 5, 6)));
        assert_eq!(resolver.color("ok"), Some(theme.ok));
        assert_eq!(resolver.color("bright-blue"), Some(Color::LightBlue));
        assert_eq!(resolver.color("white"), Some(Color::Gray));
        assert_eq!(resolver.color("nope"), None);
    }

    #[test]
    fn ansi_names_cover_normal_bright_and_gray_aliases() {
        for (name, expected) in [
            ("black", Color::Black),
            ("red", Color::Red),
            ("green", Color::Green),
            ("yellow", Color::Yellow),
            ("blue", Color::Blue),
            ("magenta", Color::Magenta),
            ("cyan", Color::Cyan),
            ("white", Color::Gray),
            ("bright-black", Color::DarkGray),
            ("bright-red", Color::LightRed),
            ("bright-green", Color::LightGreen),
            ("bright-yellow", Color::LightYellow),
            ("bright-blue", Color::LightBlue),
            ("bright-magenta", Color::LightMagenta),
            ("bright-cyan", Color::LightCyan),
            ("bright-white", Color::White),
            ("gray", Color::Gray),
            ("grey", Color::Gray),
            ("dark-gray", Color::DarkGray),
            ("dark-grey", Color::DarkGray),
        ] {
            assert_eq!(ansi(name), Some(expected), "{name}");
        }
        assert_eq!(ansi("bright-gray"), None);
        assert_eq!(ansi("Red"), None);
    }

    #[test]
    fn resolve_builds_a_ratatui_style() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let style = resolver
            .resolve(&StyleSpec::parse("fg:ok bg:#101010 bold").unwrap())
            .unwrap();
        assert_eq!(
            style,
            Style::default()
                .fg(theme.ok)
                .bg(Color::Rgb(0x10, 0x10, 0x10))
                .add_modifier(Modifier::BOLD)
        );
    }

    #[test]
    fn unknown_name_is_an_error_naming_it() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        for (src, name) in [
            ("fg:rusty", "rusty"),
            ("bg:rusty", "rusty"),
            ("$mark_style", "mark_style"),
        ] {
            let error = resolver
                .resolve(&StyleSpec::parse(src).unwrap())
                .unwrap_err();
            assert!(error.0.contains(name), "{}", error.0);
        }
    }

    #[test]
    fn style_vars_patch_in_order_then_explicit_tokens_win() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let styles = HashMap::from([
            (
                "style".to_string(),
                Style::default()
                    .fg(Color::Red)
                    .bg(Color::Blue)
                    .add_modifier(Modifier::BOLD | Modifier::ITALIC),
            ),
            (
                "mark_style".to_string(),
                Style::default()
                    .fg(Color::Green)
                    .remove_modifier(Modifier::BOLD),
            ),
        ]);
        let base = Resolver::new(&palette, &theme);
        let resolver = base.with_styles(styles);
        let inherited = resolver
            .resolve(&StyleSpec::parse("$style $mark_style").unwrap())
            .unwrap();
        assert_eq!(inherited.fg, Some(Color::Green));
        assert_eq!(inherited.bg, Some(Color::Blue));
        assert_eq!(inherited.add_modifier, Modifier::ITALIC);
        assert_eq!(inherited.sub_modifier, Modifier::BOLD);
        let explicit = resolver
            .resolve(
                &StyleSpec::parse("fg:ok bold $style $mark_style bg:12 fg:warn bg:13").unwrap(),
            )
            .unwrap();
        assert_eq!(explicit.fg, Some(theme.warn));
        assert_eq!(explicit.bg, Some(Color::Indexed(13)));
        assert_eq!(explicit.add_modifier, Modifier::BOLD | Modifier::ITALIC);
        assert_eq!(explicit.sub_modifier, Modifier::empty());
        assert!(base.resolve(&StyleSpec::parse("$style").unwrap()).is_err());
    }
}
