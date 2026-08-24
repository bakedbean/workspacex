//! 8-frame braille spinner driven by `app.tick`. The tick runs at
//! `app::TICK` (125ms / 8 Hz), so one tick is one frame — matching the V5
//! spec's 8 fps target exactly.
//!
//! This divisor is coupled to the tick rate: it was `tick / 8` back when the
//! tick ran at 16ms to poll for PTY output. Change `app::TICK` and this must
//! move with it, or the spinner silently drifts off 8 fps.

pub const SPINNER: [char; 8] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧'];

/// Pick the spinner frame for a given tick counter.
pub fn frame(tick: u32) -> char {
    SPINNER[(tick as usize) % 8]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn frame_zero_is_first_glyph() {
        assert_eq!(frame(0), '⠋');
    }

    #[test]
    fn frame_advances_every_tick() {
        // One tick per frame: at the 125ms `app::TICK` this is 8 fps.
        assert_eq!(frame(0), '⠋');
        assert_eq!(frame(1), '⠙');
        assert_eq!(frame(2), '⠹');
    }

    #[test]
    fn frame_wraps_after_eight_ticks() {
        assert_eq!(frame(8), '⠋');
        assert_eq!(frame(9), '⠙');
    }

    #[test]
    fn a_full_cycle_takes_one_second_at_the_tick_rate() {
        // Pins the coupling the module doc warns about: 8 frames × `app::TICK`
        // must stay at the spec's 8 fps. If `TICK` moves and this divisor
        // doesn't, this fails instead of the spinner quietly drifting.
        assert_eq!(
            crate::app::run::TICK * SPINNER.len() as u32,
            Duration::from_secs(1)
        );
    }
}
