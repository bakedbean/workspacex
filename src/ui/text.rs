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
    fn truncate_pad_fills_to_exact_width() {
        assert_eq!(truncate_pad("hi", 5), "hi   ");
        assert_eq!(truncate_pad("hi", 2), "hi");
        // Over-width pads to exactly `target` (ellipsis included).
        assert_eq!(truncate_pad("hello", 4).chars().count(), 4);
    }
}
