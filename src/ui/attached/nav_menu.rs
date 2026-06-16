//! Single source of truth for the Ctrl-x navigation overlay: the ordered
//! action list (so the renderer and the Enter-dispatch can't drift) plus the
//! overlay renderer itself.

use crate::ui::footer::key_for_glyph;

/// One row in the navigation overlay. `glyph` is the key as printed in the
/// pill (`"d"`, `"x"`, `"←→"`); it doubles as the Enter-dispatch key via
/// [`crate::ui::footer::key_for_glyph`]. `label` is the human description.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NavItem {
    pub glyph: &'static str,
    pub label: &'static str,
}

/// The attached coding-agent view's action list. Multi-pane mode swaps
/// `detach` → `close pane` and adds a `←→ focus pane` row. Order here is the
/// order rows render AND the order ↑↓ walks — it is the contract the Enter
/// handler relies on.
pub fn nav_menu_items(multi_pane: bool) -> Vec<NavItem> {
    let mut items = vec![NavItem {
        glyph: "d",
        label: if multi_pane { "close pane" } else { "detach" },
    }];
    if multi_pane {
        items.push(NavItem { glyph: "←→", label: "focus pane" });
    }
    items.extend([
        NavItem { glyph: "u", label: "updates" },
        NavItem { glyph: "a", label: "agents" },
        NavItem { glyph: "e", label: "edit" },
        NavItem { glyph: "t", label: "open terminal" },
        NavItem { glyph: "v", label: "diff" },
        NavItem { glyph: "g", label: "lazygit" },
        NavItem { glyph: "k", label: "processes" },
        NavItem { glyph: "x", label: "send literal ^x" },
    ]);
    items
}

/// The PM pane's smaller action list.
pub fn pm_nav_menu_items() -> Vec<NavItem> {
    vec![
        NavItem { glyph: "d", label: "detach" },
        NavItem { glyph: "u", label: "updates" },
        NavItem { glyph: "x", label: "send literal ^x" },
    ]
}

/// Resolve a highlighted index to the key press it fires, reusing the footer's
/// glyph→key map (`"←→"` → Right, single chars → that char). `None` when the
/// index is out of range or the glyph has no single-key mapping.
pub fn nav_item_key(items: &[NavItem], selected: usize) -> Option<crossterm::event::KeyEvent> {
    items.get(selected).and_then(|i| key_for_glyph(i.glyph))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_pane_lists_detach_not_close_pane() {
        let items = nav_menu_items(false);
        assert_eq!(items[0], NavItem { glyph: "d", label: "detach" });
        assert!(!items.iter().any(|i| i.label == "focus pane"));
        // Stable tail order the Enter handler depends on.
        assert_eq!(items.last().unwrap().glyph, "x");
    }

    #[test]
    fn multi_pane_swaps_close_pane_and_adds_focus() {
        let items = nav_menu_items(true);
        assert_eq!(items[0], NavItem { glyph: "d", label: "close pane" });
        assert_eq!(items[1], NavItem { glyph: "←→", label: "focus pane" });
        assert!(items.len() == nav_menu_items(false).len() + 1);
    }

    #[test]
    fn nav_item_key_maps_glyph_to_keypress() {
        use crossterm::event::{KeyCode, KeyModifiers};
        let items = nav_menu_items(true);
        let updates = items.iter().position(|i| i.glyph == "u").unwrap();
        assert_eq!(
            nav_item_key(&items, updates),
            Some(crossterm::event::KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE))
        );
        let focus = items.iter().position(|i| i.glyph == "←→").unwrap();
        // "←→" collapses to Right (forward) per key_for_glyph.
        assert_eq!(
            nav_item_key(&items, focus),
            Some(crossterm::event::KeyEvent::new(KeyCode::Right, KeyModifiers::NONE))
        );
    }
}
