//! The dashboard footer's activity-graph window: 24h / 1 week / 1 month.
//!
//! Stored in the `settings` table under key `usage_graph_window` as one of the
//! canonical tokens "24h" | "1w" | "1mo". Read at render time so CLI (`wsx
//! config set usage_graph_window 1w`) and the in-app picker both apply live.

use crate::data::store::Store;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageWindow {
    Day,
    Week,
    Month,
}

impl UsageWindow {
    pub const ALL: [UsageWindow; 3] = [Self::Day, Self::Week, Self::Month];

    /// Total span in hours: 24 / 168 / 720.
    pub fn hours(self) -> u64 {
        match self {
            Self::Day => 24,
            Self::Week => 168,
            Self::Month => 720,
        }
    }

    /// Compact footer label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Day => "24h",
            Self::Week => "1w",
            Self::Month => "1mo",
        }
    }

    /// Canonical token used for persistence (same as `label`).
    pub fn as_setting(self) -> &'static str {
        self.label()
    }

    /// Parse a canonical token; anything unrecognized falls back to `Day`.
    /// Surrounding whitespace is ignored so values read from a file
    /// (`wsx config set <key> @file`, often with a trailing newline) still
    /// parse.
    pub fn from_setting(s: &str) -> UsageWindow {
        match s.trim() {
            "24h" => Self::Day,
            "1w" => Self::Week,
            "1mo" => Self::Month,
            _ => Self::Day,
        }
    }

    /// Position within `ALL` (used to seed/apply the picker selection).
    pub fn index(self) -> usize {
        match self {
            Self::Day => 0,
            Self::Week => 1,
            Self::Month => 2,
        }
    }

    /// Inverse of `index`; out-of-range clamps to the last variant.
    pub fn from_index(i: usize) -> UsageWindow {
        Self::ALL[i.min(Self::ALL.len() - 1)]
    }
}

/// Read the configured window from the store, defaulting to `Day` on missing
/// key, parse failure, or DB error.
pub fn resolve(store: &Store) -> UsageWindow {
    match store.get_setting("usage_graph_window") {
        Ok(Some(s)) => UsageWindow::from_setting(&s),
        _ => UsageWindow::Day,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hours_and_label_per_variant() {
        assert_eq!(UsageWindow::Day.hours(), 24);
        assert_eq!(UsageWindow::Week.hours(), 168);
        assert_eq!(UsageWindow::Month.hours(), 720);
        assert_eq!(UsageWindow::Day.label(), "24h");
        assert_eq!(UsageWindow::Week.label(), "1w");
        assert_eq!(UsageWindow::Month.label(), "1mo");
    }

    #[test]
    fn from_setting_accepts_canonical_tokens_only() {
        assert_eq!(UsageWindow::from_setting("24h"), UsageWindow::Day);
        assert_eq!(UsageWindow::from_setting("1w"), UsageWindow::Week);
        assert_eq!(UsageWindow::from_setting("1mo"), UsageWindow::Month);
        assert_eq!(UsageWindow::from_setting("week"), UsageWindow::Day);
        assert_eq!(UsageWindow::from_setting("1d"), UsageWindow::Day);
        assert_eq!(UsageWindow::from_setting(""), UsageWindow::Day);
        assert_eq!(UsageWindow::from_setting("garbage"), UsageWindow::Day);
    }

    #[test]
    fn from_setting_ignores_surrounding_whitespace() {
        assert_eq!(UsageWindow::from_setting("1w\n"), UsageWindow::Week);
        assert_eq!(UsageWindow::from_setting("  24h  "), UsageWindow::Day);
        assert_eq!(UsageWindow::from_setting("1mo\r\n"), UsageWindow::Month);
    }

    #[test]
    fn as_setting_round_trips_through_from_setting() {
        for w in UsageWindow::ALL {
            assert_eq!(UsageWindow::from_setting(w.as_setting()), w);
        }
    }

    #[test]
    fn index_and_from_index_are_inverse() {
        for (i, w) in UsageWindow::ALL.iter().enumerate() {
            assert_eq!(w.index(), i);
            assert_eq!(UsageWindow::from_index(i), *w);
        }
        assert_eq!(UsageWindow::from_index(99), UsageWindow::Month);
    }
}
