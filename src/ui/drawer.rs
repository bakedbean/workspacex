//! The bottom drawer: the one surface every modal, panel, and menu opens in.
//!
//! Modeled on Neovim's message/cmdline area and Telescope's `bottom_pane`:
//! a full-width strip pinned to the bottom of the screen, filled with the
//! footer chrome background so it reads as the footer growing upward, topped
//! by a single rule that carries the title. No side borders and no centering
//! math, so the only thing a caller decides is how many rows its body needs.
//!
//! The rule of thumb: the drawer is for prompts, choices, and menus. Content
//! with a shape of its own opens in a centered [`float_frame`] instead, in
//! the same chrome so it still reads as part of the family: the name-color
//! swatch grid (fixed 2D layout, gains nothing from full width), and the
//! setup log and process list (content that runs long, which wants most of
//! the screen and a frame that holds still while it streams or grows).
//!
//! Layout, top to bottom:
//!
//! ```text
//! ── title ───────────────────────────────────────────────  (rule)
//!  body …                                                    (body_h rows)
//!  ␣enter␣ create   ␣esc␣ cancel                             (hint row, optional)
//! ```

use crate::ui::theme::Theme;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear};
use std::cell::Cell;

/// Blank columns between the screen edge and the drawer's content.
const GUTTER: u16 = 1;

thread_local! {
    /// `(floor, tallest)` body rows while a [`hold_height`] scope is active.
    static HOLD: Cell<Option<(u16, u16)>> = const { Cell::new(None) };
}

/// Run `draw` with every drawer held to at least `floor` body rows, and return
/// the tallest body asked for (never less than `floor`). The caller carries
/// that into the next frame's `floor`, so an open drawer grows with its
/// content but never shrinks under the user while it stays open. Outside a
/// scope (e.g. a renderer under test) drawers size to their content exactly.
pub fn hold_height<R>(floor: u16, draw: impl FnOnce() -> R) -> (R, u16) {
    let prev = HOLD.replace(Some((floor, floor)));
    let out = draw();
    let tallest = HOLD.replace(prev).map_or(floor, |(_, t)| t);
    (out, tallest)
}

/// Apply the active [`hold_height`] floor to `body_h` and record it.
fn held(body_h: u16) -> u16 {
    HOLD.with(|hold| match hold.get() {
        Some((floor, tallest)) => {
            let body_h = body_h.max(floor);
            hold.set(Some((floor, tallest.max(body_h))));
            body_h
        }
        None => body_h,
    })
}

/// Width of a drawer's body on `area`: full width less the gutters. Panels
/// that wrap or column-fit their rows measure against this before opening
/// the drawer, since the row count they report depends on it.
pub fn body_width(area: Rect) -> u16 {
    area.width.saturating_sub(2 * GUTTER)
}

/// One key hint: the key glyph and what it does.
pub type Hint<'a> = (&'a str, &'a str);

/// The tallest a drawer may grow: it always leaves the top quarter of the
/// screen showing, so whatever the drawer acts on stays in view above it.
/// Tiny terminals get the whole screen.
pub fn max_height(area: Rect) -> u16 {
    if area.height < 12 {
        area.height
    } else {
        area.height - area.height / 4
    }
}

/// The drawer's outer rect for a body of `body_h` rows plus the rule and, when
/// `hints` is set, the hint row: full width, bottom-aligned, capped by
/// [`max_height`].
pub fn rect(area: Rect, body_h: u16, hints: bool) -> Rect {
    let chrome = 1 + u16::from(hints);
    let h = body_h
        .saturating_add(chrome)
        .min(max_height(area))
        .max(chrome.min(area.height));
    Rect {
        x: area.x,
        y: area.y + area.height - h,
        width: area.width,
        height: h,
    }
}

/// Clear and paint a drawer for a `body_h`-row body and return the body rect.
/// The body may come back shorter than asked when the screen is too small;
/// callers that scroll should lay out against the returned height.
pub fn frame<'a>(
    f: &mut Frame,
    area: Rect,
    body_h: u16,
    title: impl Into<Line<'a>>,
    hints: &[Hint<'_>],
    theme: &Theme,
) -> Rect {
    let rect = rect(area, held(body_h), !hints.is_empty());
    f.render_widget(Clear, rect);
    let rule = Style::default().fg(theme.path);
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(rule)
        .title(title_line(title.into(), rule, theme))
        .style(Style::default().bg(theme.bg_alt));
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    let inner = inner.inner(Margin {
        horizontal: GUTTER,
        vertical: 0,
    });
    if hints.is_empty() || inner.height == 0 {
        return inner;
    }
    let hint_row = Rect {
        y: inner.y + inner.height - 1,
        height: 1,
        ..inner
    };
    f.render_widget(hint_line(hints, theme), hint_row);
    Rect {
        height: inner.height - 1,
        ..inner
    }
}

/// Clear and paint a centered `w`×`h` float in drawer chrome (fully bordered,
/// since it has no screen edge to lean on) and return the inner rect. The
/// caller must check the box fits: this clamps to `area`, which would clip.
pub fn float_frame<'a>(
    f: &mut Frame,
    area: Rect,
    w: u16,
    h: u16,
    title: impl Into<Line<'a>>,
    theme: &Theme,
) -> Rect {
    let (w, h) = (w.min(area.width), h.min(area.height));
    let rect = Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, rect);
    let rule = Style::default().fg(theme.path);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(rule)
        .title(title_line(title.into(), rule, theme))
        .style(Style::default().bg(theme.bg_alt));
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    inner
}

/// `── title ` — callers pass titles with or without surrounding spaces, so
/// trim them and supply the spacing here, keeping every drawer's rule
/// identical.
fn title_line<'a>(title: Line<'a>, rule: Style, theme: &Theme) -> Line<'a> {
    let mut spans = vec![Span::styled("──", rule)];
    let text: String = title.spans.iter().map(|s| s.content.as_ref()).collect();
    if !text.trim().is_empty() {
        spans.push(Span::raw(" "));
        let last = title.spans.len().saturating_sub(1);
        for (i, mut s) in title.spans.into_iter().enumerate() {
            if i == 0 {
                s.content = s.content.trim_start().to_string().into();
            }
            if i == last {
                s.content = s.content.trim_end().to_string().into();
            }
            spans.push(s);
        }
        spans.push(Span::raw(" "));
    }
    Line::from(spans).style(theme.header_style())
}

/// A row of key hints in the footer's pill style: `␣key␣ label  ␣key␣ label`.
/// The single renderer for every "what can I press here" line, so the drawer
/// never mixes `[y] yes`, `Enter confirm`, and `y  yes` dialects.
pub fn hint_line(hints: &[Hint<'_>], theme: &Theme) -> Line<'static> {
    let mut spans = Vec::with_capacity(hints.len() * 5);
    for (i, (key, label)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.extend(crate::ui::footer::key_pill_spans(key, theme));
        spans.push(Span::styled(format!(" {label}"), theme.dim_style()));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(h: u16) -> Rect {
        Rect::new(0, 0, 100, h)
    }

    #[test]
    fn drawer_spans_full_width_and_sits_on_the_bottom_edge() {
        let r = rect(screen(40), 5, true);
        assert_eq!((r.x, r.width), (0, 100));
        assert_eq!(r.y + r.height, 40);
        assert_eq!(r.height, 5 + 2);
    }

    #[test]
    fn drawer_sizes_to_content_not_a_minimum() {
        // A one-line confirm is three rows tall, not a 14-row box.
        assert_eq!(rect(screen(40), 1, true).height, 3);
    }

    #[test]
    fn drawer_leaves_the_top_quarter_showing() {
        assert_eq!(rect(screen(40), 100, true).height, 30);
        // Small terminals may use all of it.
        assert_eq!(rect(screen(10), 100, false).height, 10);
    }

    #[test]
    fn a_held_drawer_grows_but_never_shrinks() {
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 30)).unwrap();
        let theme = Theme::wsx();
        let mut floor = 0;
        let mut drawn = Vec::new();
        for want in [3, 6, 2] {
            term.draw(|f| {
                let ((), tallest) = hold_height(floor, || {
                    drawn.push(frame(f, f.area(), want, "t", &[], &theme).height);
                });
                floor = tallest;
            })
            .unwrap();
        }
        assert_eq!(drawn, [3, 6, 6]);
    }

    #[test]
    fn hint_line_uses_one_dialect() {
        let theme = Theme::wsx();
        let text: String = hint_line(&[("enter", "create"), ("esc", "cancel")], &theme)
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(text, " enter  create   esc  cancel");
    }
}
