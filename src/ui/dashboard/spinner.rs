//! 8-frame braille spinner driven by `app.tick`. The renderer treats
//! `Tick` as 60 fps (existing 16ms cadence); dividing by 8 yields ~7.8
//! fps, matching the V5 spec's 8 fps target.

pub const SPINNER: [char; 8] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧'];

/// Pick the spinner frame for a given tick counter.
pub fn frame(tick: u32) -> char {
    SPINNER[((tick / 8) as usize) % 8]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_zero_is_first_glyph() {
        assert_eq!(frame(0), '⠋');
    }

    #[test]
    fn frame_advances_every_eight_ticks() {
        assert_eq!(frame(0), '⠋');
        assert_eq!(frame(7), '⠋');
        assert_eq!(frame(8), '⠙');
        assert_eq!(frame(15), '⠙');
        assert_eq!(frame(16), '⠹');
    }

    #[test]
    fn frame_wraps_after_64_ticks() {
        assert_eq!(frame(64), '⠋');
        assert_eq!(frame(72), '⠙');
    }
}
