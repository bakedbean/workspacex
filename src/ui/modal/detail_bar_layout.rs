//! The `b` detail-bar layout editor: the bar's containers side by side,
//! a cursor on one module, and a menu of every module to add.

use super::*;
use crate::app::detail_bar_layout::LayoutScope;
use crate::config::detail_bar_layout::{Containers, Pos};
use crate::ui::detail_modules::Registry;
use unicode_width::UnicodeWidthStr;

/// The editor's state. `containers` mirrors what is stored: every edit is
/// saved as it is made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutEditor {
    pub scope: LayoutScope,
    /// Shown in the title: `global`, or the repo whose override this is.
    pub scope_label: String,
    pub containers: Containers,
    pub cursor: Pos,
    /// `Some(i)` while the module menu is open, highlighting the `i`th
    /// registered module.
    pub menu: Option<usize>,
    /// Feedback for the last key (a refused edit, a save error).
    pub notice: Option<String>,
}

impl LayoutEditor {
    pub fn new(scope: LayoutScope, scope_label: String, containers: Containers) -> Self {
        Self {
            scope,
            scope_label,
            containers,
            cursor: (0, 0),
            menu: None,
            notice: None,
        }
    }
}

const COLUMN_HINTS: &[Hint] = &[
    ("←→↑↓", "move"),
    ("a", "add"),
    ("d", "remove"),
    ("HJKL", "shift"),
    ("<>", "swap column"),
    ("n/X", "add/del column"),
    ("r", "reset"),
    ("esc", "close"),
];
const MENU_HINTS: &[Hint] = &[
    ("↑↓", "move"),
    ("↵", "add/remove here"),
    ("d", "remove"),
    ("esc", "back"),
];

/// Render the editor in the bottom drawer.
pub fn render_detail_bar_layout(
    f: &mut Frame,
    area: Rect,
    ed: &LayoutEditor,
    registry: &Registry,
    theme: &Theme,
) {
    let tallest = ed.containers.iter().map(Vec::len).max().unwrap_or(0).max(1);
    let content_h = match ed.menu {
        Some(_) => 1 + registry.ids().count(),
        None => 1 + tallest,
    };
    let inner = panel_frame(
        f,
        area,
        content_h + 2,
        format!(" Detail bar layout — {} ", ed.scope_label),
        theme,
    );
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);
    let lines = match ed.menu {
        Some(sel) => menu_lines(ed, sel, registry, theme, chunks[0].width),
        None => column_lines(ed, registry, theme, chunks[0].width),
    };
    // Keep the cursor row on screen when the drawer is capped short.
    let cursor_line = match ed.menu {
        Some(sel) => 1 + sel,
        None => 1 + ed.cursor.1,
    };
    let top = cursor_line.saturating_sub(usize::from(chunks[0].height.max(1)) - 1);
    f.render_widget(
        Paragraph::new(lines).scroll((u16::try_from(top).unwrap_or(u16::MAX), 0)),
        chunks[0],
    );
    if let Some(n) = &ed.notice {
        f.render_widget(
            Paragraph::new(Span::styled(format!("  {n}"), theme.warn_style())),
            chunks[1],
        );
    }
    let hints = if ed.menu.is_some() {
        MENU_HINTS
    } else {
        COLUMN_HINTS
    };
    f.render_widget(
        Paragraph::new(crate::ui::drawer::hint_line(hints, theme)),
        chunks[2],
    );
}

/// The heading the bar draws for `id`, or the raw id for one no module
/// registers (a typo in hand-edited JSON), so it can still be removed.
fn label(registry: &Registry, id: &str) -> (String, bool) {
    match registry.get(id) {
        Some(m) => (m.title().to_string(), true),
        None => (format!("? {id}"), false),
    }
}

/// The containers side by side, a header row over each.
fn column_lines(
    ed: &LayoutEditor,
    registry: &Registry,
    theme: &Theme,
    width: u16,
) -> Vec<Line<'static>> {
    let n = ed.containers.len().max(1);
    let sep = " │ ";
    let col_w = (usize::from(width).saturating_sub(2 + sep.len() * (n - 1)) / n).max(4);
    let tallest = ed.containers.iter().map(Vec::len).max().unwrap_or(0).max(1);
    let mut lines = Vec::with_capacity(tallest + 1);

    let mut header = vec![Span::raw("  ")];
    for (c, mods) in ed.containers.iter().enumerate() {
        if c > 0 {
            header.push(Span::styled(sep, theme.rule_style()));
        }
        let text = format!("CONTAINER {}  ({})", c + 1, mods.len());
        let style = if c == ed.cursor.0 {
            theme.selected_style()
        } else {
            theme.dim_style()
        };
        header.push(Span::styled(fit(&text, col_w), style));
    }
    lines.push(Line::from(header));

    for row in 0..tallest {
        let mut spans = vec![Span::raw("  ")];
        for (c, mods) in ed.containers.iter().enumerate() {
            if c > 0 {
                spans.push(Span::styled(sep, theme.rule_style()));
            }
            let at_cursor = (c, row) == ed.cursor;
            let (text, style) = match mods.get(row) {
                Some(id) => {
                    let (text, known) = label(registry, id);
                    let style = if known {
                        Style::default()
                    } else {
                        theme.warn_style()
                    };
                    (text, style)
                }
                None if row == 0 => ("(empty)".to_string(), theme.dim_style()),
                None => (String::new(), Style::default()),
            };
            let style = if at_cursor {
                theme.selected_bg_style()
            } else {
                style
            };
            spans.push(Span::styled(fit(&text, col_w), style));
        }
        lines.push(Line::from(spans));
    }
    lines
}

/// Every registered module, marked by where it sits.
fn menu_lines(
    ed: &LayoutEditor,
    sel: usize,
    registry: &Registry,
    theme: &Theme,
    width: u16,
) -> Vec<Line<'static>> {
    let col = ed.cursor.0;
    let mut lines = vec![Line::from(Span::styled(
        format!("  Modules for container {}", col + 1),
        theme.header_style(),
    ))];
    let title_w = registry
        .modules()
        .map(|m| m.title().width())
        .max()
        .unwrap_or(0);
    for (i, m) in registry.modules().enumerate() {
        let placed = crate::config::detail_bar_layout::find(&ed.containers, m.id());
        let (mark, place) = match placed {
            Some((c, _)) if c == col => ("[x]", String::new()),
            Some((c, _)) => ("[ ]", format!("in {}", c + 1)),
            None => ("[ ]", String::new()),
        };
        let head = format!("  {mark} {:<title_w$}  ", m.title());
        let tail_w = place.width() + 2;
        let desc_w = usize::from(width).saturating_sub(head.width() + tail_w);
        let body = format!("{head}{}  {place}", fit(m.description(), desc_w));
        let style = if i == sel {
            theme.selected_bg_style()
        } else if placed.is_some() {
            Style::default()
        } else {
            theme.dim_style()
        };
        lines.push(Line::from(Span::styled(body, style)));
    }
    lines
}

/// `s` truncated with `…` or padded with spaces to exactly `w` columns.
fn fit(s: &str, w: usize) -> String {
    if s.width() <= w {
        return format!("{s}{}", " ".repeat(w - s.width()));
    }
    let mut out = String::new();
    for ch in s.chars() {
        if out.width() + 1 >= w {
            break;
        }
        out.push(ch);
    }
    out.push('…');
    let pad = w.saturating_sub(out.width());
    out.push_str(&" ".repeat(pad));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn registry() -> Registry {
        let mut reg = Registry::new();
        crate::ui::detail_modules::register_builtins(&mut reg);
        reg
    }

    fn draw(ed: &LayoutEditor) -> String {
        let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
        let reg = registry();
        term.draw(|f| {
            render_detail_bar_layout(f, f.area(), ed, &reg, &Theme::default());
        })
        .unwrap();
        let buf = term.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn editor(containers: &[&[&str]]) -> LayoutEditor {
        LayoutEditor::new(
            LayoutScope::Global,
            "global".into(),
            containers
                .iter()
                .map(|c| c.iter().map(|s| s.to_string()).collect())
                .collect(),
        )
    }

    #[test]
    fn columns_show_each_container_and_its_modules() {
        let out = draw(&editor(&[
            &["session_summary"],
            &["recent_chat", "processes"],
            &[],
        ]));
        assert!(out.contains("Detail bar layout — global"), "{out}");
        assert!(out.contains("CONTAINER 1  (1)"), "{out}");
        assert!(out.contains("CONTAINER 2  (2)"), "{out}");
        assert!(out.contains("SESSION SUMMARY"), "{out}");
        assert!(out.contains("PROCESSES"), "{out}");
        assert!(out.contains("(empty)"), "{out}");
    }

    #[test]
    fn unknown_module_ids_stay_visible() {
        let out = draw(&editor(&[&["seshun_summary"]]));
        assert!(out.contains("? seshun_summary"), "{out}");
    }

    #[test]
    fn menu_lists_every_module_with_its_place() {
        let mut ed = editor(&[&["session_summary"], &["recent_chat"]]);
        ed.cursor = (1, 0);
        ed.menu = Some(0);
        let out = draw(&ed);
        assert!(out.contains("Modules for container 2"), "{out}");
        for title in [
            "SESSION SUMMARY",
            "RECENT CHAT",
            "PROCESSES",
            "RECENT FILES",
            "AGENTS",
        ] {
            assert!(out.contains(title), "{title} missing:\n{out}");
        }
        let line = |needle: &str| {
            out.lines()
                .find(|l| l.contains(needle))
                .unwrap()
                .to_string()
        };
        assert!(line("SESSION SUMMARY").contains("in 1"));
        assert!(line("RECENT CHAT").contains("[x]"));
        assert!(line("AGENTS").contains("[ ]"));
    }

    #[test]
    fn fit_pads_and_truncates() {
        assert_eq!(fit("abc", 5), "abc  ");
        assert_eq!(fit("abcdef", 4), "abc…");
    }
}
