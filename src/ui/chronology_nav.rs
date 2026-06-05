//! Pure cursor state machine for keyboard navigation of the chronology bar.
//! Kept free of `App`/`ratatui` so every transition is unit-testable.
//!
//! See `docs/superpowers/specs/2026-06-05-chronology-keyboard-navigation-design.md`.

/// In-pane cursor while the chronology bar is keyboard-focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChronoSel {
    Entry(usize),
    Detail(usize),
}

impl Default for ChronoSel {
    fn default() -> Self {
        ChronoSel::Entry(0)
    }
}

impl ChronoSel {
    /// The entry index this cursor refers to (entry or its detail).
    pub fn index(self) -> usize {
        match self {
            ChronoSel::Entry(i) | ChronoSel::Detail(i) => i,
        }
    }
}

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
    Expand(usize),
    Collapse(usize),
    Open(usize),
    Exit,
}

/// Pure transition. `expanded` is the currently-expanded entry index (the bar
/// allows one at a time); `len` is the entry count. Bounds-safe: never returns
/// an index >= `len` (when `len > 0`).
pub fn nav(sel: ChronoSel, key: NavKey, expanded: Option<usize>, len: usize) -> (ChronoSel, NavAction) {
    if key == NavKey::Esc {
        return (sel, NavAction::Exit);
    }
    if len == 0 {
        return (sel, NavAction::None);
    }
    let last = len - 1;
    match (sel, key) {
        (ChronoSel::Entry(i), NavKey::Down) => {
            if expanded == Some(i) {
                (ChronoSel::Detail(i), NavAction::None)
            } else {
                (ChronoSel::Entry((i + 1).min(last)), NavAction::None)
            }
        }
        (ChronoSel::Detail(i), NavKey::Down) => (ChronoSel::Entry((i + 1).min(last)), NavAction::None),
        (ChronoSel::Detail(i), NavKey::Up) => (ChronoSel::Entry(i), NavAction::None),
        (ChronoSel::Entry(i), NavKey::Up) => (ChronoSel::Entry(i.saturating_sub(1)), NavAction::None),
        (_, NavKey::Top) => (ChronoSel::Entry(0), NavAction::None),
        (_, NavKey::Bottom) => (ChronoSel::Entry(last), NavAction::None),
        (ChronoSel::Entry(i), NavKey::Enter) => {
            if expanded == Some(i) {
                (ChronoSel::Entry(i), NavAction::Collapse(i))
            } else {
                (ChronoSel::Entry(i), NavAction::Expand(i))
            }
        }
        (ChronoSel::Detail(i), NavKey::Enter) => (ChronoSel::Detail(i), NavAction::Open(i)),
        (_, NavKey::Esc) => unreachable!(),
    }
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
mod tests {
    use super::*;

    #[test]
    fn down_moves_to_next_entry_when_collapsed() {
        assert_eq!(nav(ChronoSel::Entry(0), NavKey::Down, None, 3), (ChronoSel::Entry(1), NavAction::None));
    }

    #[test]
    fn down_steps_into_detail_when_expanded() {
        assert_eq!(nav(ChronoSel::Entry(1), NavKey::Down, Some(1), 3), (ChronoSel::Detail(1), NavAction::None));
    }

    #[test]
    fn down_from_detail_goes_to_next_entry() {
        assert_eq!(nav(ChronoSel::Detail(1), NavKey::Down, Some(1), 3), (ChronoSel::Entry(2), NavAction::None));
    }

    #[test]
    fn up_from_detail_returns_to_entry() {
        assert_eq!(nav(ChronoSel::Detail(2), NavKey::Up, Some(2), 3), (ChronoSel::Entry(2), NavAction::None));
    }

    #[test]
    fn up_from_entry_goes_previous_saturating() {
        assert_eq!(nav(ChronoSel::Entry(1), NavKey::Up, None, 3), (ChronoSel::Entry(0), NavAction::None));
        assert_eq!(nav(ChronoSel::Entry(0), NavKey::Up, None, 3), (ChronoSel::Entry(0), NavAction::None));
    }

    #[test]
    fn down_clamps_at_last() {
        assert_eq!(nav(ChronoSel::Entry(2), NavKey::Down, None, 3), (ChronoSel::Entry(2), NavAction::None));
    }

    #[test]
    fn top_and_bottom() {
        assert_eq!(nav(ChronoSel::Detail(1), NavKey::Top, Some(1), 3), (ChronoSel::Entry(0), NavAction::None));
        assert_eq!(nav(ChronoSel::Entry(0), NavKey::Bottom, None, 3), (ChronoSel::Entry(2), NavAction::None));
    }

    #[test]
    fn enter_toggles_expand_on_entry() {
        assert_eq!(nav(ChronoSel::Entry(1), NavKey::Enter, None, 3), (ChronoSel::Entry(1), NavAction::Expand(1)));
        assert_eq!(nav(ChronoSel::Entry(1), NavKey::Enter, Some(1), 3), (ChronoSel::Entry(1), NavAction::Collapse(1)));
    }

    #[test]
    fn enter_on_detail_opens() {
        assert_eq!(nav(ChronoSel::Detail(1), NavKey::Enter, Some(1), 3), (ChronoSel::Detail(1), NavAction::Open(1)));
    }

    #[test]
    fn esc_exits_from_anywhere() {
        assert_eq!(nav(ChronoSel::Entry(0), NavKey::Esc, None, 3).1, NavAction::Exit);
        assert_eq!(nav(ChronoSel::Detail(2), NavKey::Esc, Some(2), 3).1, NavAction::Exit);
    }

    #[test]
    fn empty_list_only_exits() {
        assert_eq!(nav(ChronoSel::Entry(0), NavKey::Down, None, 0), (ChronoSel::Entry(0), NavAction::None));
        assert_eq!(nav(ChronoSel::Entry(0), NavKey::Enter, None, 0), (ChronoSel::Entry(0), NavAction::None));
        assert_eq!(nav(ChronoSel::Entry(0), NavKey::Esc, None, 0).1, NavAction::Exit);
    }

    #[test]
    fn adjust_scroll_keeps_selection_visible() {
        assert_eq!(adjust_scroll(5, 2, 4, 10), 2);
        assert_eq!(adjust_scroll(0, 6, 4, 10), 3);
        assert_eq!(adjust_scroll(2, 3, 4, 10), 2);
        assert_eq!(adjust_scroll(3, 0, 4, 0), 0);
    }

    #[test]
    fn index_extracts_entry_index() {
        assert_eq!(ChronoSel::Entry(4).index(), 4);
        assert_eq!(ChronoSel::Detail(7).index(), 7);
    }
}
