//! The xterm-256 palette behind the per-workspace name color.
//!
//! A workspace's chosen color is stored as a palette INDEX (`workspaces
//! .name_color`), not as a hex string: the index is what the terminal is
//! actually asked to paint, so it round-trips exactly and honors the user's
//! terminal theme for the first 16 slots. Hex is the *user-facing* identity —
//! the picker shows it and filters on it — so this module owns the index↔hex
//! mapping in one place.

use ratatui::style::Color;

/// Number of slots in the palette — the picker grid's universe.
pub const PALETTE_LEN: usize = 256;

/// Default hexes for the 16 ANSI slots. Terminals re-skin these from their own
/// theme, so what the user actually sees may differ; these are the canonical
/// xterm defaults, used for display and for hex filtering.
const ANSI: [(&str, &str); 16] = [
    ("black", "000000"),
    ("red", "800000"),
    ("green", "008000"),
    ("yellow", "808000"),
    ("blue", "000080"),
    ("magenta", "800080"),
    ("cyan", "008080"),
    ("white", "c0c0c0"),
    ("brightblack", "808080"),
    ("brightred", "ff0000"),
    ("brightgreen", "00ff00"),
    ("brightyellow", "ffff00"),
    ("brightblue", "0000ff"),
    ("brightmagenta", "ff00ff"),
    ("brightcyan", "00ffff"),
    ("brightwhite", "ffffff"),
];

/// The six intensity levels each channel of the 6x6x6 cube steps through.
const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// The RGB bytes behind palette index `i`.
pub fn rgb(index: u8) -> (u8, u8, u8) {
    match index {
        0..=15 => {
            let h = ANSI[index as usize].1;
            let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0);
            (byte(0), byte(2), byte(4))
        }
        16..=231 => {
            let n = index - 16;
            (
                CUBE_LEVELS[(n / 36) as usize],
                CUBE_LEVELS[(n % 36 / 6) as usize],
                CUBE_LEVELS[(n % 6) as usize],
            )
        }
        // 24-step gray ramp, 8..=238 in steps of 10.
        _ => {
            let v = 8 + (index - 232) * 10;
            (v, v, v)
        }
    }
}

/// Lowercase 6-digit hex for palette index `index`, without a leading `#`.
pub fn hex(index: u8) -> String {
    let (r, g, b) = rgb(index);
    format!("{r:02x}{g:02x}{b:02x}")
}

/// Human name for the 16 ANSI slots (`"brightblue"`), `""` for the rest —
/// the cube and gray ramp are identified by hex alone.
pub fn name(index: u8) -> &'static str {
    match index {
        0..=15 => ANSI[index as usize].0,
        _ => "",
    }
}

/// The ratatui color for a palette index.
pub fn color(index: u8) -> Color {
    Color::Indexed(index)
}

/// Palette indices matching `filter`, in palette order. An empty filter (after
/// trimming whitespace and a leading `#`) matches everything; otherwise an
/// index matches when the needle is a substring of its hex or of its ANSI
/// name. Case-insensitive, so a pasted `#D7AF87` finds 180.
pub fn matching(filter: &str) -> Vec<u8> {
    let needle = filter.trim().trim_start_matches('#').to_ascii_lowercase();
    (0..=255u8)
        .filter(|&i| {
            needle.is_empty()
                || hex(i).contains(&needle)
                || (!name(i).is_empty() && name(i).contains(&needle))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_and_gray_ramp_hexes_match_xterm() {
        // 6x6x6 color cube: index = 16 + 36r + 6g + b over levels
        // [0, 95, 135, 175, 215, 255].
        assert_eq!(hex(16), "000000");
        assert_eq!(hex(21), "0000ff");
        assert_eq!(hex(196), "ff0000");
        assert_eq!(hex(231), "ffffff");
        assert_eq!(hex(180), "d7af87");
        // 24-step gray ramp: 8 + 10i.
        assert_eq!(hex(232), "080808");
        assert_eq!(hex(255), "eeeeee");
    }

    #[test]
    fn ansi_slots_carry_names_and_default_hexes() {
        assert_eq!((name(0), hex(0)), ("black", "000000".to_string()));
        assert_eq!((name(1), hex(1)), ("red", "800000".to_string()));
        assert_eq!((name(9), hex(9)), ("brightred", "ff0000".to_string()));
        assert_eq!((name(12), hex(12)), ("brightblue", "0000ff".to_string()));
        assert_eq!((name(7), hex(7)), ("white", "c0c0c0".to_string()));
        assert_eq!((name(15), hex(15)), ("brightwhite", "ffffff".to_string()));
    }

    #[test]
    fn non_ansi_slots_have_no_name() {
        assert_eq!(name(16), "");
        assert_eq!(name(180), "");
        assert_eq!(name(255), "");
    }

    #[test]
    fn empty_filter_matches_the_whole_palette() {
        let all = matching("");
        assert_eq!(all.len(), 256);
        assert_eq!(all[0], 0);
        assert_eq!(all[255], 255);
    }

    #[test]
    fn filter_matches_a_hex_substring() {
        let hits = matching("d7af87");
        assert_eq!(hits, vec![180]);
        // A partial prefix keeps every color that starts with it.
        assert!(matching("d7af").contains(&180));
        assert!(matching("d7af").len() > 1);
    }

    #[test]
    fn filter_matches_an_ansi_name() {
        assert_eq!(matching("brightblue"), vec![12]);
        // "red" is a substring of "brightred", so both match.
        let reds = matching("red");
        assert!(reds.contains(&1) && reds.contains(&9));
    }

    #[test]
    fn filter_ignores_a_leading_hash_and_case() {
        assert_eq!(matching("#D7AF87"), vec![180]);
        assert_eq!(matching("  D7af87 "), vec![180]);
        assert_eq!(matching("BrightBlue"), vec![12]);
    }

    #[test]
    fn filter_with_no_match_is_empty() {
        assert!(matching("zzz").is_empty());
        assert!(matching("d7af8z").is_empty());
    }

    #[test]
    fn color_is_the_palette_index() {
        assert_eq!(color(180), Color::Indexed(180));
    }

    #[test]
    fn every_index_renders_a_six_digit_hex() {
        for i in 0..=255u8 {
            let h = hex(i);
            assert_eq!(h.len(), 6, "index {i} produced {h:?}");
            assert!(
                h.chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
            );
        }
    }
}
