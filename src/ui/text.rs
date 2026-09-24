//! Shared text-column helpers for fixed-width TUI layouts. Column width is
//! measured in terminal cells, the unit ratatui draws in: each grapheme
//! cluster takes its `unicode-width` width, so CJK and most emoji take two
//! cells and a combining mark shares its base's cell. See `dashboard::row`,
//! where these originated before being shared with the remote-workspace
//! picker.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// `s` as the grapheme clusters ratatui will draw, each with its width in
/// cells. A control character (a CRLF pair counts as one) becomes a
/// one-cell space: `unicode-width` counts it as one cell in a string, but
/// ratatui draws it inconsistently — `Buffer::set_stringn` skips it, and a
/// `Span` drops `\n` yet sends `\t` or ESC raw to the terminal in a cell of
/// its own. With the control replaced, every draw path agrees with the
/// count, and no helper output carries one.
fn cells(s: &str) -> impl Iterator<Item = (&str, usize)> {
    s.graphemes(true).map(|g| {
        if g.contains(char::is_control) {
            (" ", 1)
        } else {
            (g, g.width())
        }
    })
}

/// Width of `s` in terminal cells, as the helpers below measure it: `日本`
/// is 4, `e\u{301}` is 1, and the `…` they append is 1.
pub(crate) fn display_width(s: &str) -> usize {
    cells(s).map(|(_, w)| w).sum()
}

/// Truncate `s` to at most `target` cells, replacing the tail with `…` when
/// it overflows. The cut falls between grapheme clusters, never through a
/// wide char, so it can land a cell short of `target`. `target == 0`
/// yields an empty string.
pub(crate) fn truncate(s: &str, target: usize) -> String {
    if display_width(s) <= target {
        return cells(s).map(|(g, _)| g).collect();
    }
    if target == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0usize;
    for (g, w) in cells(s) {
        // `<` (not `<=`) keeps one cell of budget for the `…`.
        if used + w < target {
            out.push_str(g);
            used += w;
        } else {
            break;
        }
    }
    out.push('…');
    out
}

/// Cap on an echoed filter needle in chrome (the updates-panel footer, the
/// dashboard top bar). Long enough to recognize what you typed, short
/// enough that it can't push the surrounding hints off the line.
pub(crate) const FILTER_ECHO_MAX: usize = 24;

/// Truncate `s` to at most `target` cells, cutting at a word boundary: keep
/// as many whole words as fit (with the `…` counted against the budget) and
/// attach `…` directly to the last kept word. Degrades to plain [`truncate`]
/// when not even the first word fits — a mid-word cut beats an empty cell.
pub(crate) fn truncate_words(s: &str, target: usize) -> String {
    if display_width(s) <= target {
        return truncate(s, target);
    }
    if target == 0 {
        return String::new();
    }
    let mut out = String::new();
    // Running cell count — recomputing `display_width(&out)` per word would
    // make this quadratic, and it runs during per-frame row synthesis.
    let mut out_width = 0usize;
    for word in s.split_whitespace() {
        let sep = usize::from(!out.is_empty());
        let word_width = display_width(word);
        // `<` (not `<=`) keeps one cell of budget for the `…`.
        if out_width + sep + word_width < target {
            if sep == 1 {
                out.push(' ');
            }
            out.extend(cells(word).map(|(g, _)| g));
            out_width += sep + word_width;
        } else {
            break;
        }
    }
    if out.is_empty() {
        return truncate(s, target);
    }
    out.push('…');
    out
}

/// [`truncate`] then right-pad with spaces to exactly `target` cells, so the
/// result always occupies `target` columns — the building block for aligned
/// columns.
pub(crate) fn truncate_pad(s: &str, target: usize) -> String {
    let mut out = truncate(s, target);
    let width = display_width(&out);
    if width < target {
        out.push_str(&" ".repeat(target - width));
    }
    out
}

/// Abbreviate a token count as `950` / `77k` / `1M` / `1.2M`. The `k` form
/// floors (77_999 → "77k"); exact precision is meaningless for a fill gauge.
pub(crate) fn abbreviate_tokens(n: u64) -> String {
    if n < 1_000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{}k", n / 1_000)
    } else {
        let m = n as f64 / 1_000_000.0;
        if (m - m.round()).abs() < 0.05 {
            format!("{}M", m.round() as u64)
        } else {
            format!("{m:.1}M")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_replaces_overflow_tail_with_ellipsis() {
        assert_eq!(truncate("hello", 5), "hello");
        assert_eq!(truncate("hello", 4), "hel…");
        assert_eq!(truncate("hello", 0), "");
    }

    #[test]
    fn truncate_words_cuts_at_word_boundary() {
        // Fits: untouched.
        assert_eq!(truncate_words("goal seg", 8), "goal seg");
        // Overflow: keep whole words, ellipsis directly attached.
        assert_eq!(truncate_words("goal seg", 6), "goal…");
        assert_eq!(
            truncate_words("Make dashboard PR status", 20),
            "Make dashboard PR…"
        );
        // First word alone doesn't fit: degrade to char truncation.
        assert_eq!(truncate_words("dashboard", 5), "dash…");
        // Degenerate widths.
        assert_eq!(truncate_words("goal seg", 0), "");
    }

    #[test]
    fn truncate_pad_fills_to_exact_width() {
        assert_eq!(truncate_pad("hi", 5), "hi   ");
        assert_eq!(truncate_pad("hi", 2), "hi");
        // Over-width pads to exactly `target` (ellipsis included).
        assert_eq!(truncate_pad("hello", 4).chars().count(), 4);
    }

    /// Wide (CJK, emoji), multi-byte narrow (Cyrillic), multi-char clusters
    /// (combining mark, emoji presentation, ZWJ family), control chars, and
    /// empty.
    const SAMPLES: &[&str] = &[
        "abc",
        "日本語",
        "😀x",
        "Чёрный",
        "",
        "cafe\u{301} ole\u{301}",
        "⚠\u{fe0f} careful",
        "👨\u{200d}👩\u{200d}👧 family",
        "tab\there",
        "line\nbreak",
        "crlf\r\nend",
        "esc\x1b[1m",
    ];

    /// Cells ratatui actually draws for `s`: the column a sentinel lands in
    /// when rendered right after it.
    fn drawn_width(s: &str) -> usize {
        use ratatui::buffer::Buffer;
        use ratatui::layout::Rect;
        use ratatui::text::{Line, Span};
        use ratatui::widgets::{Paragraph, Widget};
        let area = Rect::new(0, 0, 64, 1);
        let mut buf = Buffer::empty(area);
        Paragraph::new(Line::from(vec![Span::raw(s), Span::raw("|")])).render(area, &mut buf);
        (0..area.width)
            .position(|x| buf[(x, 0)].symbol() == "|")
            .unwrap_or_else(|| panic!("sentinel not drawn after {s:?}"))
    }

    #[test]
    fn display_width_counts_cells_not_chars() {
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width("日本語"), 6, "CJK is two cells per char");
        assert_eq!(display_width("😀"), 2, "so is an emoji");
        assert_eq!(display_width("Чёрный"), 6, "Cyrillic is one per char");
        assert_eq!(display_width("e\u{301}"), 1, "a combining mark adds none");
        // The variation selector makes the sign an emoji: two chars, one
        // cluster, two cells — summing per-char widths would say one.
        assert_eq!(display_width("⚠\u{fe0f}"), 2);
        assert_eq!(display_width("…"), 1, "the ellipsis is one cell");
        assert_eq!(display_width(""), 0);
    }

    #[test]
    fn truncate_pad_fills_exactly_the_budget_in_cells() {
        for s in SAMPLES {
            for w in [0usize, 1, 2, 3, 5, 6, 10, 20] {
                let out = truncate_pad(s, w);
                assert_eq!(display_width(&out), w, "truncate_pad({s:?}, {w}) = {out:?}");
                assert_eq!(drawn_width(&out), w, "drawn: truncate_pad({s:?}, {w})");
            }
        }
    }

    #[test]
    fn truncation_never_exceeds_the_budget() {
        for s in SAMPLES.iter().chain(&["日本語 です ね"]) {
            for w in 0..16usize {
                for out in [truncate(s, w), truncate_words(s, w)] {
                    assert!(
                        display_width(&out) <= w && drawn_width(&out) <= w,
                        "{s:?} at {w} cells gave {out:?}, drawn {} wide",
                        drawn_width(&out)
                    );
                }
            }
        }
    }

    #[test]
    fn truncate_never_splits_a_wide_char() {
        // The cut lands a cell short rather than halve `本`; padding makes
        // up the difference.
        assert_eq!(truncate("日本語", 4), "日…");
        assert_eq!(truncate("日本語", 5), "日本…");
        assert_eq!(truncate("日本語", 6), "日本語", "fits exactly");
        assert_eq!(truncate_pad("日本語", 4), "日… ");
        // A cluster is kept whole or dropped whole.
        assert_eq!(truncate("⚠\u{fe0f}x", 2), "…");
        let family = "👨\u{200d}👩\u{200d}👧";
        assert_eq!(truncate(&format!("{family}ab"), 3), format!("{family}…"));
    }

    #[test]
    fn truncate_words_budgets_wide_words_in_cells() {
        // 6 + 1 + 4 + 1 + 2 = 14 cells in 7 chars.
        let s = "日本語 です ね";
        assert_eq!(truncate_words(s, 14), s);
        assert_eq!(truncate_words(s, 12), "日本語 です…");
        assert_eq!(truncate_words(s, 10), "日本語…");
        // First word alone doesn't fit: degrade to cell truncation.
        assert_eq!(truncate_words(s, 5), "日本…");
    }

    #[test]
    fn control_chars_become_one_cell_spaces() {
        // Counted as one cell (unicode-width's string width) and drawn as
        // one, rather than sent raw to the terminal or silently dropped.
        assert_eq!(truncate("a\tb\nc", 10), "a b c");
        assert_eq!(truncate("a\r\nb", 10), "a b", "CRLF is one cluster");
        assert_eq!(truncate("x\x1b[1my", 10), "x [1my");
        assert_eq!(truncate_words("fix\tthe\nlogin bug", 12), "fix the…");
        for s in SAMPLES {
            for w in 0..8usize {
                for out in [truncate(s, w), truncate_words(s, w), truncate_pad(s, w)] {
                    assert!(!out.contains(char::is_control), "{s:?} at {w}: {out:?}");
                }
            }
        }
    }

    #[test]
    fn abbreviate_tokens_uses_k_and_m() {
        assert_eq!(abbreviate_tokens(950), "950");
        assert_eq!(abbreviate_tokens(77_081), "77k");
        assert_eq!(abbreviate_tokens(200_000), "200k");
        assert_eq!(abbreviate_tokens(1_000_000), "1M");
        assert_eq!(abbreviate_tokens(1_250_000), "1.2M");
    }
}
