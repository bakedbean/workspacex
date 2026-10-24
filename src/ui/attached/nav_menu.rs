//! Single source of truth for the Ctrl-x navigation overlay: the ordered
//! action list (so the renderer and the Enter-dispatch can't drift) plus the
//! overlay renderer itself.

#[allow(unused_imports)]
use super::*;
use crate::ui::footer::key_for_glyph;
use ratatui::text::Text;

/// One row in the navigation overlay. `glyph` is the key as printed in the
/// pill (`"d"`, `"x"`, `"←→"`); it doubles as the Enter-dispatch key via
/// [`crate::ui::footer::key_for_glyph`]. `label` is the human description.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NavItem {
    pub glyph: &'static str,
    pub label: &'static str,
}

/// The attached coding-agent view's action list. Multi-pane mode swaps
/// `detach` → `close pane` and adds `←→ focus pane` and `D save layout & detach`
/// rows. Order here is the order rows render AND the order ↑↓ walks — it is the
/// contract the Enter handler relies on.
pub fn nav_menu_items(multi_pane: bool) -> Vec<NavItem> {
    let mut items = vec![NavItem {
        glyph: "d",
        label: if multi_pane { "close pane" } else { "detach" },
    }];
    if multi_pane {
        items.push(NavItem {
            glyph: "←→",
            label: "focus pane",
        });
        // Shift-D persists the current pane arrangement before detaching, so
        // re-attaching restores it. Only meaningful with more than one pane.
        items.push(NavItem {
            glyph: "D",
            label: "save layout & detach",
        });
    }
    items.extend([
        NavItem {
            glyph: "u",
            label: "updates",
        },
        NavItem {
            glyph: "a",
            label: "agents",
        },
        NavItem {
            glyph: "e",
            label: "edit",
        },
        NavItem {
            glyph: "t",
            label: "open terminal",
        },
        NavItem {
            glyph: "v",
            label: "diff",
        },
        NavItem {
            glyph: "g",
            label: "lazygit",
        },
        NavItem {
            glyph: "k",
            label: "processes",
        },
        NavItem {
            glyph: "<",
            label: "prompt tag",
        },
        NavItem {
            glyph: "#",
            label: "workspace tags",
        },
        NavItem {
            glyph: "x",
            label: "send literal ^x",
        },
    ]);
    items
}

/// The remote-attach action list. A remote workspace has no local worktree or
/// session tree, so only the chords `handle_key_attached_remote` handles show.
pub fn remote_nav_menu_items() -> Vec<NavItem> {
    vec![
        NavItem {
            glyph: "d",
            label: "detach",
        },
        NavItem {
            glyph: "<",
            label: "prompt tag",
        },
        NavItem {
            glyph: "x",
            label: "send literal ^x",
        },
    ]
}

/// Resolve a highlighted index to the key press it fires, reusing the footer's
/// glyph→key map (`"←→"` → Right, single chars → that char). `None` when the
/// index is out of range or the glyph has no single-key mapping.
pub fn nav_item_key(items: &[NavItem], selected: usize) -> Option<crossterm::event::KeyEvent> {
    items.get(selected).and_then(|i| key_for_glyph(i.glyph))
}

/// Visible column width of one overlay cell, mirroring the spans built in
/// `render_nav_overlay`'s loop: marker(2) + key pill(`2 + glyph_len`, per
/// `key_pill_spans`) + gap(3) + label. Measuring the pill with the real glyph
/// length keeps multi-cell glyphs like "←→" from being truncated.
fn nav_row_width(glyph_len: usize, label_len: usize) -> usize {
    2 + (2 + glyph_len) + 3 + label_len
}

/// Draw the Ctrl-x navigation overlay as a which-key style grid in the bottom
/// drawer: each cell is the key pill then its label, columns as wide as the
/// widest row, filled column-major so ↑↓ (which walk `items` in order) move
/// down a column and wrap into the next. The `selected` cell carries a `▌`
/// marker. `pinned_hint` adds a "1-9 pinned" hint when the workspace has
/// pinned commands.
pub fn render_nav_overlay(
    f: &mut Frame,
    area: Rect,
    items: &[NavItem],
    selected: usize,
    pinned_hint: bool,
    theme: &Theme,
) {
    const GAP: usize = 2;
    let cell_w = items
        .iter()
        .map(|i| nav_row_width(i.glyph.chars().count(), i.label.chars().count()))
        .max()
        .unwrap_or(20);
    // The drawer's body is the screen less a 1-cell gutter each side.
    let usable = usize::from(area.width).saturating_sub(2);
    let cols = ((usable + GAP) / (cell_w + GAP)).clamp(1, items.len().max(1));
    let rows = items.len().div_ceil(cols);

    let mut hints = vec![("↑↓", "move"), ("↵", "run")];
    if pinned_hint {
        hints.push(("1-9", "pinned"));
    }
    hints.push(("esc", "close"));
    let inner = crate::ui::drawer::frame(f, area, rows as u16, "actions", &hints, theme);

    // A short screen caps the drawer below `rows`; scroll so the selected
    // cell's row stays on screen, or ↓ could walk onto an invisible action
    // that Enter would still run.
    let visible = usize::from(inner.height).max(1);
    let top = (selected % rows.max(1)).saturating_sub(visible - 1);
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(visible);
    for row in top..rows.min(top + visible) {
        let mut spans: Vec<Span<'static>> = Vec::new();
        for col in 0..cols {
            let Some(item) = items.get(col * rows + row) else {
                break;
            };
            if col > 0 {
                spans.push(Span::raw(" ".repeat(GAP)));
            }
            let width = nav_row_width(item.glyph.chars().count(), item.label.chars().count());
            if col * rows + row == selected {
                spans.push(Span::styled(
                    "▌ ".to_string(),
                    Style::default().fg(theme.waiting),
                ));
            } else {
                spans.push(Span::raw("  ".to_string()));
            }
            spans.extend(crate::ui::footer::key_pill_spans(item.glyph, theme));
            spans.push(Span::styled(
                format!("   {}{}", item.label, " ".repeat(cell_w - width)),
                Style::default().fg(theme.path),
            ));
        }
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(Text::from(lines)), inner);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nav_row_width_uses_real_glyph_width() {
        // The "←→" focus-pane row has a 2-char glyph, so its pill is 4 cells
        // (2 + 2), not the 3 a single-char glyph gets. The box width must be
        // measured with the real glyph length or the row truncates on narrow
        // terminals (regression: the old formula hard-coded the pill at 3).
        let focus = nav_menu_items(true)
            .into_iter()
            .find(|i| i.glyph == "←→")
            .unwrap();
        assert_eq!(
            nav_row_width(focus.glyph.chars().count(), focus.label.chars().count()),
            2 + 4 + 3 + "focus pane".chars().count(),
        );
        // A single-char glyph still measures as before (pill width 3).
        assert_eq!(nav_row_width(1, "updates".chars().count()), 2 + 3 + 3 + 7);
    }

    #[test]
    fn single_pane_lists_detach_not_close_pane() {
        let items = nav_menu_items(false);
        assert_eq!(
            items[0],
            NavItem {
                glyph: "d",
                label: "detach"
            }
        );
        assert!(!items.iter().any(|i| i.label == "focus pane"));
        // Stable tail order the Enter handler depends on.
        assert_eq!(items.last().unwrap().glyph, "x");
        assert!(
            items
                .iter()
                .any(|i| i.glyph == "<" && i.label == "prompt tag"),
            "the nav overlay lists the prompt-tag row"
        );
        assert_eq!(
            crate::ui::footer::key_for_glyph("<").map(|k| k.code),
            Some(crossterm::event::KeyCode::Char('<')),
            "the overlay's Enter path resolves the row to the `<` key"
        );
    }

    #[test]
    fn multi_pane_swaps_close_pane_and_adds_focus() {
        let items = nav_menu_items(true);
        assert_eq!(
            items[0],
            NavItem {
                glyph: "d",
                label: "close pane"
            }
        );
        assert_eq!(
            items[1],
            NavItem {
                glyph: "←→",
                label: "focus pane"
            }
        );
        assert_eq!(
            items[2],
            NavItem {
                glyph: "D",
                label: "save layout & detach"
            }
        );
        // Multi-pane adds two rows over single-pane: focus-pane and save-layout.
        assert!(items.len() == nav_menu_items(false).len() + 2);
        // Stable tail order the Enter handler depends on is unchanged.
        assert_eq!(items.last().unwrap().glyph, "x");
    }

    #[test]
    fn nav_item_key_maps_glyph_to_keypress() {
        use crossterm::event::{KeyCode, KeyModifiers};
        let items = nav_menu_items(true);
        let updates = items.iter().position(|i| i.glyph == "u").unwrap();
        assert_eq!(
            nav_item_key(&items, updates),
            Some(crossterm::event::KeyEvent::new(
                KeyCode::Char('u'),
                KeyModifiers::NONE
            ))
        );
        let focus = items.iter().position(|i| i.glyph == "←→").unwrap();
        // "←→" collapses to Right (forward) per key_for_glyph.
        assert_eq!(
            nav_item_key(&items, focus),
            Some(crossterm::event::KeyEvent::new(
                KeyCode::Right,
                KeyModifiers::NONE
            ))
        );
    }

    #[test]
    fn a_capped_drawer_scrolls_to_keep_the_selection_visible() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let theme = Theme::wsx();
        let items = nav_menu_items(false);
        // 40x12: one column of ten actions in a drawer capped to nine rows.
        let last = items.len() - 1;
        let mut term = Terminal::new(TestBackend::new(40, 12)).unwrap();
        term.draw(|f| render_nav_overlay(f, f.area(), &items, last, false, &theme))
            .unwrap();
        let buf = term.backend().buffer();
        let text: String = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.contains("send literal ^x"),
            "selection visible:\n{text}"
        );
        assert!(text.contains("▌"), "with its marker:\n{text}");
    }

    #[test]
    fn overlay_renders_keybind_then_label_with_marker() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let theme = Theme::wsx();
        let items = nav_menu_items(false);
        let mut term = Terminal::new(TestBackend::new(60, 20)).unwrap();
        term.draw(|f| {
            render_nav_overlay(f, Rect::new(0, 0, 60, 20), &items, 1, true, &theme);
        })
        .unwrap();
        let buf = term.backend().buffer();
        let text: String = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("actions"), "titled actions:\n{text}");
        assert!(text.contains("detach"), "lists detach");
        assert!(text.contains("send literal ^x"), "lists send-^x");
        // Selected row (index 1 = updates) carries the ▌ marker.
        assert!(text.contains("▌"), "highlight marker present");
        assert!(text.contains("1-9  pinned"), "pinned hint shown");
    }
}
