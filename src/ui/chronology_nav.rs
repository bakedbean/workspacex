//! Pure cursor state machine for keyboard navigation of the chronology bar.
//! Kept free of `App`/`ratatui` so every transition is unit-testable.
//!
//! See `docs/superpowers/specs/2026-06-05-chronology-keyboard-navigation-design.md`.

/// A navigation key, already mapped from the raw keystroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKey {
    Up,
    Down,
    Top,
    Bottom,
    Enter,
    Esc,
}

/// Side effect the caller must apply after a transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavAction {
    None,
    Open(usize),
    Exit,
}

/// Pure transition for the single-level entry list. `len` is the entry count.
/// Bounds-safe: never returns an index >= `len` (when `len > 0`).
pub fn nav(sel: usize, key: NavKey, len: usize) -> (usize, NavAction) {
    if key == NavKey::Esc {
        return (sel, NavAction::Exit);
    }
    if len == 0 {
        return (sel, NavAction::None);
    }
    let last = len - 1;
    match key {
        NavKey::Down => ((sel + 1).min(last), NavAction::None),
        NavKey::Up => (sel.saturating_sub(1), NavAction::None),
        NavKey::Top => (0, NavAction::None),
        NavKey::Bottom => (last, NavAction::None),
        NavKey::Enter => (sel, NavAction::Open(sel)),
        NavKey::Esc => unreachable!(),
    }
}

/// Clamp a scroll offset so a `body`-row viewport never scrolls past the end of
/// `len` lines. Returns 0 when everything fits.
pub fn clamp_scroll(scroll: usize, len: usize, body: usize) -> usize {
    let max = len.saturating_sub(body);
    scroll.min(max)
}

/// Adjust the viewport `scroll` so the selected entry index stays visible,
/// given how many entries were visible last frame. One-frame lag is fine.
pub fn adjust_scroll(scroll: usize, sel_index: usize, visible: usize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    if sel_index < scroll {
        return sel_index;
    }
    if visible > 0 && sel_index >= scroll + visible {
        return sel_index + 1 - visible;
    }
    scroll
}

#[cfg(test)]
mod clamp_scroll_tests {
    use super::*;

    #[test]
    fn clamp_scroll_bounds() {
        assert_eq!(clamp_scroll(85, 100, 20), 80);
        assert_eq!(clamp_scroll(5, 100, 20), 5);
        assert_eq!(clamp_scroll(7, 10, 20), 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn down_moves_to_next_entry_clamping_at_last() {
        assert_eq!(nav(0, NavKey::Down, 3), (1, NavAction::None));
        assert_eq!(nav(2, NavKey::Down, 3), (2, NavAction::None));
    }

    #[test]
    fn up_goes_previous_saturating() {
        assert_eq!(nav(1, NavKey::Up, 3), (0, NavAction::None));
        assert_eq!(nav(0, NavKey::Up, 3), (0, NavAction::None));
    }

    #[test]
    fn top_and_bottom() {
        assert_eq!(nav(1, NavKey::Top, 3), (0, NavAction::None));
        assert_eq!(nav(0, NavKey::Bottom, 3), (2, NavAction::None));
    }

    #[test]
    fn enter_opens_current_selection() {
        assert_eq!(nav(1, NavKey::Enter, 3), (1, NavAction::Open(1)));
    }

    #[test]
    fn esc_exits_from_anywhere() {
        assert_eq!(nav(0, NavKey::Esc, 3).1, NavAction::Exit);
        assert_eq!(nav(2, NavKey::Esc, 3).1, NavAction::Exit);
    }

    #[test]
    fn empty_list_is_a_no_op_except_esc() {
        assert_eq!(nav(0, NavKey::Down, 0), (0, NavAction::None));
        assert_eq!(nav(0, NavKey::Enter, 0), (0, NavAction::None));
        assert_eq!(nav(0, NavKey::Esc, 0).1, NavAction::Exit);
    }

    #[test]
    fn adjust_scroll_keeps_selection_visible() {
        assert_eq!(adjust_scroll(5, 2, 4, 10), 2);
        assert_eq!(adjust_scroll(0, 6, 4, 10), 3);
        assert_eq!(adjust_scroll(2, 3, 4, 10), 2);
        assert_eq!(adjust_scroll(3, 0, 4, 0), 0);
    }
}
