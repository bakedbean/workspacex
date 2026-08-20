//! Shared text-column helpers for fixed-width TUI layouts. Column width is
//! measured in `char`s (not display cells) — the app's existing convention;
//! see `dashboard::row` where these originated before being shared with the
//! remote-workspace picker.

/// Truncate `s` to at most `target` chars, replacing the last kept char with
/// `…` when it overflows. `target == 0` yields an empty string.
pub(crate) fn truncate(s: &str, target: usize) -> String {
    let len = s.chars().count();
    if len <= target {
        s.to_string()
    } else if target == 0 {
        String::new()
    } else {
        let mut out: String = s.chars().take(target - 1).collect();
        out.push('…');
        out
    }
}

/// Cap on an echoed filter needle in chrome (the updates-panel footer, the
/// dashboard top bar). Long enough to recognize what you typed, short
/// enough that it can't push the surrounding hints off the line.
pub(crate) const FILTER_ECHO_MAX: usize = 24;

/// Truncate `s` to at most `target` chars, cutting at a word boundary: keep
/// as many whole words as fit (with the `…` counted against the budget) and
/// attach `…` directly to the last kept word. Degrades to plain [`truncate`]
/// when not even the first word fits — a mid-word cut beats an empty cell.
pub(crate) fn truncate_words(s: &str, target: usize) -> String {
    if s.chars().count() <= target {
        return s.to_string();
    }
    if target == 0 {
        return String::new();
    }
    let mut out = String::new();
    // Running char count — recomputing `out.chars().count()` per word would
    // make this quadratic, and it runs during per-frame row synthesis.
    let mut out_len = 0usize;
    for word in s.split_whitespace() {
        let sep = usize::from(!out.is_empty());
        let word_len = word.chars().count();
        // `< target` (not `<=`) keeps one char of budget for the `…`.
        if out_len + sep + word_len < target {
            if sep == 1 {
                out.push(' ');
            }
            out.push_str(word);
            out_len += sep + word_len;
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

/// [`truncate`] then right-pad with spaces to exactly `target` chars, so the
/// result always occupies `target` columns — the building block for aligned
/// columns.
pub(crate) fn truncate_pad(s: &str, target: usize) -> String {
    let mut out = truncate(s, target);
    let len = out.chars().count();
    if len < target {
        out.push_str(&" ".repeat(target - len));
    }
    out
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
}
