//! Bottom-pinned detail bar shown when a workspace is selected on the
//! dashboard. Renders header strip, three-column body, and an inline
//! reply input.
//!
//! See `docs/superpowers/specs/2026-05-24-dashboard-workspace-detail-design.md`.

/// Minimum rows the bar needs to render usefully (1 header + 1 rule + 3
/// body + 1 rule + 1 input + 1 spacing slack).
pub const MIN_HEIGHT: u16 = 8;

/// Compute the detail bar's preferred height given the total available
/// height. Targets ~22% of the area, clamped to `[MIN_HEIGHT, 14]`.
pub fn preferred_height(total_height: u16) -> u16 {
    let target = (u32::from(total_height) * 22 / 100) as u16;
    target.clamp(MIN_HEIGHT, 14)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferred_height_clamps_to_min_on_short_terminal() {
        // 22% of 20 = 4 → clamps up to MIN_HEIGHT (8).
        assert_eq!(preferred_height(20), MIN_HEIGHT);
    }

    #[test]
    fn preferred_height_returns_22_percent_for_typical_terminal() {
        // 22% of 50 = 11 → within range.
        assert_eq!(preferred_height(50), 11);
    }

    #[test]
    fn preferred_height_clamps_to_14_on_tall_terminal() {
        // 22% of 100 = 22 → clamps down to 14.
        assert_eq!(preferred_height(100), 14);
    }

    #[test]
    fn preferred_height_handles_zero_height() {
        // 22% of 0 = 0 → clamps up to MIN_HEIGHT.
        assert_eq!(preferred_height(0), MIN_HEIGHT);
    }
}
