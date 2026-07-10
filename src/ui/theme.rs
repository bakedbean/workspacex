use crate::pty::session::AgentKind;
use ratatui::style::{Color, Modifier, Style};

/// Fixed per-agent identity colors. Independent of the active theme so the
/// agent a workspace runs on stays recognizable when the user switches
/// themes — agent identity is constant, unlike the status/lifecycle tones
/// which each theme re-skins.
const AGENT_CLAUDE: Color = Color::Rgb(0xe8, 0x8b, 0x3c); // orange
const AGENT_PI: Color = Color::Rgb(0xa9, 0x7b, 0xd6); // purple
const AGENT_HERMES: Color = Color::Rgb(0xf0, 0xd0, 0x66); // yellow
const AGENT_CODEX: Color = Color::Rgb(0x5b, 0x9d, 0xe0); // blue

/// Brand accent — the electric blue of the "x" and cursor block in the
/// "workspace x" wordmark. Matches the site's `--accent` token (#5ab0ff).
/// Constant across themes, like the agent identity colors above: the brand
/// should read the same everywhere.
pub const BRAND_ACCENT: Color = Color::Rgb(0x5a, 0xb0, 0xff);

/// Color of the "workspace" word in the wordmark — a neutral cream white,
/// paired with the brighter blue [`BRAND_ACCENT`] on the cursor block and "x".
/// Stays distinct from the blue some themes use for repo headers.
pub const BRAND_WORDMARK: Color = Color::Rgb(0xe8, 0xe8, 0xd3);

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
    /// Inline-code and code-block foreground for rendered markdown.
    /// A distinct, legible hue — never darker than `dim`.
    pub code: Color,
    /// Footer bar background — the chrome strip that holds key-chip rows
    /// in both the dashboard and attached views. One step elevated from
    /// the main pane background.
    pub bg_alt: Color,
    /// Per-chip background — the button-like fill behind each key chord
    /// in the footer. One step elevated from `bg_alt` so chips "lift" off
    /// the bar.
    pub bg_soft: Color,
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
    /// 6-state status palette per the V5 design tokens.
    pub question: Color,
    pub stalled: Color,
    pub waiting: Color,
    pub thinking: Color,
    pub complete: Color,
    pub idle: Color,
}

impl Theme {
    /// ANSI-named palette so the user's terminal theme is respected.
    /// Was named `default_theme` pre-V5; the new default is `wsx`.
    pub fn ansi() -> Self {
        Self {
            header_fg: Color::Cyan,
            header_bg: Some(Color::Reset),
            selected_fg: Color::White,
            selected_bg: Color::DarkGray,
            dim: Color::DarkGray,
            path: Color::Indexed(67),
            code: Color::Cyan,
            bg_alt: Color::Indexed(235),
            bg_soft: Color::Indexed(237),
            ok: Color::Green,
            warn: Color::Yellow,
            err: Color::Red,
            attention: Color::Magenta,
            merged: Color::Magenta,
            question: Color::Yellow,
            stalled: Color::Red,
            waiting: Color::Blue,
            thinking: Color::Magenta,
            complete: Color::Green,
            idle: Color::DarkGray,
        }
    }

    /// V5 design tokens — oklch values from `tui.css` converted to sRGB.
    /// This is the new default theme.
    pub fn wsx() -> Self {
        Self {
            header_fg: Color::Rgb(0xeb, 0xeb, 0xeb),
            header_bg: None,
            selected_fg: Color::Rgb(0xff, 0xff, 0xff),
            // Brighter than the original 0x243043 navy so the selection
            // tint reads cleanly against the dark wsx background.
            selected_bg: Color::Rgb(0x35, 0x49, 0x66),
            dim: Color::Rgb(0xb5, 0xb5, 0xb5),
            path: Color::Rgb(0x6b, 0x6e, 0x75),
            code: Color::Rgb(0x7e, 0xb6, 0xb0), // teal
            bg_alt: Color::Rgb(0x13, 0x18, 0x20),
            bg_soft: Color::Rgb(0x18, 0x1f, 0x29),
            ok: Color::Rgb(0x67, 0xc0, 0x89),
            warn: Color::Rgb(0xe4, 0xba, 0x6c),
            err: Color::Rgb(0xd3, 0x62, 0x58),
            attention: Color::Rgb(0xb7, 0x8c, 0xd0),
            merged: Color::Rgb(0xb7, 0x8c, 0xd0),
            question: Color::Rgb(0xe4, 0xba, 0x6c),
            stalled: Color::Rgb(0xd3, 0x62, 0x58),
            waiting: Color::Rgb(0x6e, 0xa7, 0xd8),
            thinking: Color::Rgb(0xb7, 0x8c, 0xd0),
            complete: Color::Rgb(0x67, 0xc0, 0x89),
            idle: Color::Rgb(0x7a, 0x7e, 0x85),
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
        // One luminance step above dracula's canonical `current_line`
        // so the selection bg reads as clearly distinct from the rest
        // of the row, while staying in the dracula purple-grey space.
        let selection_bg = Color::Rgb(0x55, 0x59, 0x6e);
        let _ = current_line;
        Self {
            header_fg: purple,
            header_bg: None,
            selected_fg: foreground,
            selected_bg: selection_bg,
            dim: comment,
            path: Color::Rgb(0x82, 0x90, 0xb4), // softer blue-grey
            code: cyan,
            bg_alt: Color::Rgb(0x21, 0x22, 0x2e),
            bg_soft: Color::Rgb(0x35, 0x37, 0x47),
            ok: green,
            warn: yellow,
            err: red,
            attention: pink,
            merged: purple,
            question: yellow,
            stalled: red,
            waiting: cyan,
            thinking: purple,
            complete: green,
            idle: comment,
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
        // Brighter than jellybeans's CursorLine — the canonical value
        // matches the editor's cursor highlight, which is intentionally
        // subtle for code reading. The dashboard wants something louder.
        let selection_bg = Color::Rgb(0x4a, 0x4a, 0x4a);
        let _ = (bg, yellow, cursor_line);
        Self {
            header_fg: blue,
            header_bg: None,
            selected_fg: fg,
            selected_bg: selection_bg,
            dim: gray,
            path: Color::Rgb(0xb8, 0xa0, 0x78), // warmer tan, clearer separation from gray
            code: green,
            bg_alt: Color::Rgb(0x1c, 0x1c, 0x1c),
            bg_soft: Color::Rgb(0x26, 0x26, 0x26),
            ok: green,
            warn: orange,
            err: red,
            attention: purple,
            merged: purple,
            question: orange,
            stalled: red,
            waiting: blue,
            thinking: purple,
            complete: green,
            idle: gray,
        }
    }

    pub fn nord() -> Self {
        // https://www.nordtheme.com/docs/colors-and-palettes
        let polar0 = Color::Rgb(0x2e, 0x34, 0x40); // background
        let polar1 = Color::Rgb(0x3b, 0x42, 0x52); // canonical selection bg
        let polar2 = Color::Rgb(0x43, 0x4c, 0x5e); // one step above polar1, used as the louder selection bg
        let polar3 = Color::Rgb(0x4c, 0x56, 0x6a); // muted (dim fg color)
        let snow_storm1 = Color::Rgb(0xd8, 0xde, 0xe9);
        let frost1 = Color::Rgb(0x88, 0xc0, 0xd0); // header cyan
        let aurora_red = Color::Rgb(0xbf, 0x61, 0x6a);
        let aurora_orange = Color::Rgb(0xd0, 0x87, 0x70);
        let aurora_yellow = Color::Rgb(0xeb, 0xcb, 0x8b);
        let aurora_green = Color::Rgb(0xa3, 0xbe, 0x8c);
        let aurora_purple = Color::Rgb(0xb4, 0x8e, 0xad);
        let _ = (polar0, polar1);
        Self {
            header_fg: frost1,
            header_bg: None,
            selected_fg: snow_storm1,
            // Nord's canonical selection bg is `polar1`; we lift it one
            // step to `polar2` so the row stands out clearly without
            // colliding with `dim` (polar3), which would make dim spans
            // disappear inside the selected row.
            selected_bg: polar2,
            dim: polar3,
            path: Color::Rgb(0x81, 0xa1, 0xc1), // Nord frost3
            code: Color::Rgb(0x8f, 0xbc, 0xbb), // nord frost0 (nord7)
            bg_alt: Color::Rgb(0x29, 0x2e, 0x39),
            bg_soft: Color::Rgb(0x34, 0x3a, 0x47),
            ok: aurora_green,
            warn: aurora_yellow,
            err: aurora_red,
            attention: aurora_orange,
            merged: aurora_purple,
            question: aurora_yellow,
            stalled: aurora_red,
            waiting: frost1,
            thinking: aurora_purple,
            complete: aurora_green,
            idle: polar3,
        }
    }

    /// Look up a theme by name. Unknown names fall back to the default.
    pub fn by_name(name: &str) -> Self {
        match name {
            "ansi" | "default" => Self::ansi(),
            "wsx" => Self::wsx(),
            "dracula" => Self::dracula(),
            "jellybeans" => Self::jellybeans(),
            "nord" => Self::nord(),
            _ => Self::wsx(),
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
    /// Selection tint that leaves per-span foregrounds intact — only the
    /// row background changes. Used by the dashboard list so status,
    /// lifecycle, and dim colors remain readable on the selected row.
    pub fn selected_bg_style(&self) -> Style {
        Style::default().bg(self.selected_bg)
    }
    pub fn dim_style(&self) -> Style {
        Style::default().fg(self.dim)
    }
    /// Per-chip background fill for key/chord chips inside the footer bar.
    /// Pair with the chip's own fg style (dim+bold for keys, path for
    /// labels) to get the V5 "button" look.
    pub fn chip_bg_style(&self) -> Style {
        Style::default().bg(self.bg_soft)
    }
    pub fn path_style(&self) -> Style {
        Style::default().fg(self.path)
    }
    /// Heading line in rendered markdown — brighter than body, bold.
    pub fn md_heading_style(&self) -> Style {
        Style::default()
            .fg(self.header_fg)
            .add_modifier(Modifier::BOLD)
    }
    /// Inline code and fenced code blocks in rendered markdown.
    pub fn md_code_style(&self) -> Style {
        Style::default().fg(self.code)
    }
    /// List-item bullet/number marker in rendered markdown.
    pub fn md_bullet_style(&self) -> Style {
        Style::default().fg(self.attention)
    }
    /// Blockquote body in rendered markdown — dim and italic.
    pub fn md_quote_style(&self) -> Style {
        Style::default().fg(self.dim).add_modifier(Modifier::ITALIC)
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

    /// Map a `BranchLifecycle` to its theme color. Returns `None` for
    /// lifecycle states that intentionally have no color (NoPr, PrDraft)
    /// so each caller can pick its own fallback — the dashboard branch
    /// column dims unknowns, the updates-panel name stays default-bold.
    pub fn lifecycle_style(&self, lc: Option<crate::git::forge::BranchLifecycle>) -> Option<Style> {
        use crate::git::forge::BranchLifecycle::*;
        match lc {
            Some(PrOpen) => Some(self.ok_style()),
            Some(PrConflicted) => Some(self.warn_style()),
            Some(PrMerged) => Some(self.merged_style()),
            Some(PrClosed) => Some(self.err_style()),
            _ => None,
        }
    }

    /// Fixed identity color for a workspace's coding agent. Ignores `self`
    /// by design — see the `AGENT_*` constants — but lives on `Theme` so
    /// every color decision stays in one module, alongside `status_style`
    /// and `lifecycle_style`.
    pub fn agent_style(&self, agent: AgentKind) -> Style {
        let fg = match agent {
            AgentKind::Claude => AGENT_CLAUDE,
            AgentKind::Pi => AGENT_PI,
            AgentKind::Hermes => AGENT_HERMES,
            AgentKind::Codex => AGENT_CODEX,
        };
        Style::default().fg(fg)
    }

    pub fn status_style(&self, s: crate::ui::dashboard::status::Status) -> Style {
        use crate::ui::dashboard::status::Status::*;
        let fg = match s {
            Question => self.question,
            Stalled => self.stalled,
            Waiting => self.waiting,
            Thinking => self.thinking,
            Complete => self.complete,
            Idle => self.idle,
        };
        Style::default().fg(fg)
    }
}

/// The glyph shown left of a branch name, chosen by PR lifecycle. Shared by
/// the dashboard row and the remote shared-workspace picker so the two can't
/// drift. Nerd-font mode uses per-lifecycle PR icons; the plain fallback is a
/// single branch glyph regardless of lifecycle (color still distinguishes the
/// states). Color comes from [`Theme::lifecycle_style`], not from here.
pub(crate) fn branch_glyph(
    lifecycle: Option<crate::git::forge::BranchLifecycle>,
    nerd_fonts: bool,
) -> &'static str {
    use crate::git::forge::BranchLifecycle::*;
    if !nerd_fonts {
        return "\u{2387}"; // ⎇
    }
    match lifecycle {
        Some(PrMerged) => "\u{f419}",
        Some(PrClosed) => "\u{f4dc}",
        Some(PrDraft) => "\u{f4dd}",
        Some(PrOpen | PrConflicted) => "\u{f407}",
        _ => "\u{e0a0}",
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::wsx()
    }
}

#[cfg(test)]
mod branch_glyph_tests {
    use super::branch_glyph;
    use crate::git::forge::BranchLifecycle::*;

    #[test]
    fn plain_mode_is_always_the_branch_glyph() {
        for lc in [None, Some(PrOpen), Some(PrMerged), Some(PrClosed)] {
            assert_eq!(branch_glyph(lc, false), "\u{2387}", "lifecycle {lc:?}");
        }
    }

    #[test]
    fn nerd_mode_picks_a_distinct_glyph_per_lifecycle() {
        assert_eq!(branch_glyph(Some(PrMerged), true), "\u{f419}");
        assert_eq!(branch_glyph(Some(PrClosed), true), "\u{f4dc}");
        assert_eq!(branch_glyph(Some(PrDraft), true), "\u{f4dd}");
        assert_eq!(branch_glyph(Some(PrOpen), true), "\u{f407}");
        assert_eq!(branch_glyph(Some(PrConflicted), true), "\u{f407}");
        // NoPr and "unknown" both fall through to the neutral branch glyph.
        assert_eq!(branch_glyph(Some(NoPr), true), "\u{e0a0}");
        assert_eq!(branch_glyph(None, true), "\u{e0a0}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::dashboard::status::Status;

    #[test]
    fn by_name_resolves_known_themes() {
        assert_eq!(Theme::by_name("ansi").header_fg, Color::Cyan);
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
        assert!(matches!(
            Theme::by_name("bogus").header_fg,
            Color::Rgb(0xeb, 0xeb, 0xeb)
        ));
        assert!(matches!(
            Theme::by_name("").header_fg,
            Color::Rgb(0xeb, 0xeb, 0xeb)
        ));
    }

    #[test]
    fn wsx_is_the_default_theme() {
        let t = Theme::default();
        assert_eq!(t.question, Color::Rgb(0xe4, 0xba, 0x6c));
        assert_eq!(t.stalled, Color::Rgb(0xd3, 0x62, 0x58));
        assert_eq!(t.waiting, Color::Rgb(0x6e, 0xa7, 0xd8));
        assert_eq!(t.thinking, Color::Rgb(0xb7, 0x8c, 0xd0));
        assert_eq!(t.complete, Color::Rgb(0x67, 0xc0, 0x89));
        assert_eq!(t.idle, Color::Rgb(0x7a, 0x7e, 0x85));
    }

    #[test]
    fn ansi_theme_uses_named_colors() {
        let t = Theme::ansi();
        assert_eq!(t.question, Color::Yellow);
        assert_eq!(t.stalled, Color::Red);
        assert_eq!(t.waiting, Color::Blue);
        assert_eq!(t.thinking, Color::Magenta);
        assert_eq!(t.complete, Color::Green);
        assert_eq!(t.idle, Color::DarkGray);
    }

    #[test]
    fn status_style_maps_each_state() {
        let t = Theme::default();
        assert_eq!(t.status_style(Status::Question).fg, Some(t.question));
        assert_eq!(t.status_style(Status::Stalled).fg, Some(t.stalled));
        assert_eq!(t.status_style(Status::Waiting).fg, Some(t.waiting));
        assert_eq!(t.status_style(Status::Thinking).fg, Some(t.thinking));
        assert_eq!(t.status_style(Status::Complete).fg, Some(t.complete));
        assert_eq!(t.status_style(Status::Idle).fg, Some(t.idle));
    }

    #[test]
    fn by_name_resolves_wsx() {
        assert_eq!(Theme::by_name("wsx").question, Color::Rgb(0xe4, 0xba, 0x6c));
    }

    #[test]
    fn agent_style_maps_each_kind_to_fixed_rgb() {
        use crate::pty::session::AgentKind;
        let t = Theme::wsx();
        assert_eq!(
            t.agent_style(AgentKind::Claude).fg,
            Some(Color::Rgb(0xe8, 0x8b, 0x3c))
        );
        assert_eq!(
            t.agent_style(AgentKind::Pi).fg,
            Some(Color::Rgb(0xa9, 0x7b, 0xd6))
        );
        assert_eq!(
            t.agent_style(AgentKind::Hermes).fg,
            Some(Color::Rgb(0xf0, 0xd0, 0x66))
        );
        assert_eq!(
            t.agent_style(AgentKind::Codex).fg,
            Some(Color::Rgb(0x5b, 0x9d, 0xe0))
        );
    }

    #[test]
    fn markdown_styles_are_distinct_and_weighted() {
        let t = Theme::wsx();

        // Heading is brighter than body and bold.
        let h = t.md_heading_style();
        assert_eq!(h.fg, Some(t.header_fg));
        assert!(h.add_modifier.contains(Modifier::BOLD));

        // Code has its own color, distinct from the body grey.
        assert_ne!(t.code, t.dim);
        assert_eq!(t.md_code_style().fg, Some(t.code));

        // Bullet marker uses the accent color.
        assert_eq!(t.md_bullet_style().fg, Some(t.attention));

        // Blockquote body is dim + italic.
        let q = t.md_quote_style();
        assert_eq!(q.fg, Some(t.dim));
        assert!(q.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn agent_colors_are_theme_independent() {
        use crate::pty::session::AgentKind;
        let a = Theme::wsx();
        let b = Theme::dracula();
        for agent in AgentKind::ALL {
            assert_eq!(a.agent_style(agent).fg, b.agent_style(agent).fg);
        }
    }
}
