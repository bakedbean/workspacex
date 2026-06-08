//! Shared types for clickable footer nav hints.
//!
//! The dashboard and attached/PM footers print a row of keybind pills
//! (`<key> label`). Each pill is also a click target: clicking one behaves
//! exactly like pressing the corresponding key. To stay in lockstep with the
//! keyboard, a hint's action is expressed as a synthetic key event that the
//! input handler routes through the active view's key handler.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// What clicking a footer nav hint should do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FooterHintAction {
    /// Synthesize this key press. The input handler dispatches it through the
    /// focused view's key handler, arming the attached-view leader first when
    /// the view is `Attached`/`AttachedPm` (those footers list leader-prefixed
    /// commands, e.g. `^x e`).
    Key(KeyEvent),
    /// The `^x` leader pill: arm the attached-view leader without dispatching a
    /// follow-up key (mirrors pressing `Ctrl-x` alone).
    ArmLeader,
}

/// A clickable footer hint positioned relative to the start of the footer
/// line. The renderer converts `start_col`/`width` into an absolute screen
/// `Rect` for hit-testing once it knows the footer's on-screen origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FooterHintSpan {
    /// Column offset of the hint's first cell from the start of the line.
    pub start_col: u16,
    /// Width of the clickable run in cells (pill + trailing label).
    pub width: u16,
    pub action: FooterHintAction,
}

/// Map a footer key glyph (as printed in the pill) to the key press it
/// represents. Multi-arrow glyphs collapse to a single representative
/// direction — the natural "forward" one: `↑↓` → Down, `←→` → Right.
/// Returns `None` for glyphs that don't correspond to a single key press
/// (e.g. an unexpected multi-char glyph).
pub fn key_for_glyph(glyph: &str) -> Option<KeyEvent> {
    let code = match glyph {
        "↑↓" | "↓" => KeyCode::Down,
        "↑" => KeyCode::Up,
        "←→" | "→" => KeyCode::Right,
        "←" => KeyCode::Left,
        "↵" => KeyCode::Enter,
        _ => {
            let mut chars = glyph.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None; // unknown multi-char glyph
            }
            KeyCode::Char(c)
        }
    };
    Some(KeyEvent::new(code, KeyModifiers::NONE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_letter_glyph_maps_to_char_key() {
        assert_eq!(
            key_for_glyph("e"),
            Some(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE))
        );
        // Capital binds (like `G`) are matched bare by the handlers.
        assert_eq!(
            key_for_glyph("G"),
            Some(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::NONE))
        );
        assert_eq!(
            key_for_glyph("/"),
            Some(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
        );
    }

    #[test]
    fn arrow_and_enter_glyphs_map_to_directional_keys() {
        assert_eq!(
            key_for_glyph("↑↓").map(|k| k.code),
            Some(KeyCode::Down),
            "vertical-nav glyph collapses to Down"
        );
        assert_eq!(
            key_for_glyph("←→").map(|k| k.code),
            Some(KeyCode::Right),
            "horizontal-focus glyph collapses to Right"
        );
        assert_eq!(key_for_glyph("↵").map(|k| k.code), Some(KeyCode::Enter));
    }

    #[test]
    fn unknown_multichar_glyph_has_no_key() {
        assert_eq!(key_for_glyph("^x"), None);
        assert_eq!(key_for_glyph("ab"), None);
    }
}
