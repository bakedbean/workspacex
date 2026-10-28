//! The global settings drawer: every `wsx config` key, grouped, with the
//! selected key's description in the footer.

use super::repo_settings::preview_value;
use super::*;
use crate::config::settings::{SETTINGS, SettingKind};

pub fn render_global_settings(
    f: &mut Frame,
    area: Rect,
    store: &crate::data::store::Store,
    selected: usize,
    theme: &Theme,
) {
    let selected = selected.min(SETTINGS.len() - 1);
    let groups = SETTINGS
        .iter()
        .enumerate()
        .filter(|(i, d)| *i == 0 || SETTINGS[i - 1].group != d.group)
        .count();
    // One line per key and per group heading, plus description and hints.
    let inner = panel_frame(
        f,
        area,
        SETTINGS.len() + groups + 2,
        " Global settings ",
        theme,
    );
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(2)])
        .split(inner);
    let (body_area, footer_area) = (chunks[0], chunks[1]);

    let label_pad = SETTINGS.iter().map(|d| d.key.len()).max().unwrap_or(0) + 1;
    let value_width = (body_area.width as usize).saturating_sub(3 + label_pad);
    let mut lines: Vec<Line> = Vec::new();
    let mut selected_line = 0;
    for (i, def) in SETTINGS.iter().enumerate() {
        if i == 0 || SETTINGS[i - 1].group != def.group {
            lines.push(Line::from(Span::styled(
                format!(" {}", def.group),
                theme.header_style(),
            )));
        }
        let stored = store
            .get_setting(def.key)
            .ok()
            .flatten()
            .filter(|v| !v.trim().is_empty());
        let preview = match (&stored, def.default) {
            (Some(v), _) => preview_value(v, value_width),
            (None, "") => "(unset)".to_string(),
            (None, default) => format!("(default: {default})"),
        };
        let body = format!("  {:<label_pad$} {preview}", def.key);
        let style = if i == selected {
            selected_line = lines.len();
            theme.selected_style()
        } else if stored.is_none() {
            theme.dim_style()
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(body, style)));
    }
    // A capped drawer scrolls so the selected key stays visible.
    let top = selected_line.saturating_sub(usize::from(body_area.height.max(1)) - 1);
    f.render_widget(
        Paragraph::new(lines).scroll((u16::try_from(top).unwrap_or(u16::MAX), 0)),
        body_area,
    );

    let def = &SETTINGS[selected];
    let enter = match def.kind {
        SettingKind::Toggle { .. } => "toggle",
        SettingKind::Choice(_) => "next",
        SettingKind::Text { .. } => "edit",
    };
    let description = match def.kind {
        SettingKind::Choice(values) => format!("  {} ({})", def.description, values.join(", ")),
        _ => format!("  {}", def.description),
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(description, theme.dim_style())),
            crate::ui::drawer::hint_line(
                &[
                    ("↑↓", "move"),
                    ("↵", enter),
                    ("d", "reset"),
                    ("esc", "close"),
                ],
                theme,
            ),
        ]),
        footer_area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::store::Store;
    use ratatui::{Terminal, backend::TestBackend};

    fn screen(store: &Store, selected: usize, w: u16, h: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| render_global_settings(f, f.area(), store, selected, &Theme::wsx()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    fn row<'a>(lines: &'a [String], key: &str) -> &'a str {
        lines
            .iter()
            .find(|l| l.contains(&format!(" {key} ")))
            .unwrap_or_else(|| panic!("{key} not on screen"))
    }

    #[test]
    fn shows_stored_values_and_defaults() {
        let store = Store::open_in_memory().unwrap();
        store.set_setting("theme", "nord").unwrap();
        let lines = screen(&store, 0, 120, 60);
        assert!(row(&lines, "theme").contains("nord"));
        assert!(row(&lines, "nerd_fonts").contains("(default: on)"));
        assert!(row(&lines, "diff_cmd").contains("(unset)"));
        assert!(lines.iter().any(|l| l.contains("Notifications")));
    }

    #[test]
    fn footer_describes_the_selected_key() {
        let store = Store::open_in_memory().unwrap();
        let i = SETTINGS.iter().position(|d| d.key == "theme").unwrap();
        let lines = screen(&store, i, 120, 60);
        let text = lines.join("\n");
        assert!(text.contains("Base palette"), "{text}");
        assert!(text.contains("wsx, default, dracula, jellybeans, nord"));
        assert!(text.contains("next"));
    }

    #[test]
    fn a_capped_drawer_scrolls_to_the_selected_key() {
        let store = Store::open_in_memory().unwrap();
        let last = SETTINGS.len() - 1;
        let lines = screen(&store, last, 100, 16);
        let y = lines
            .iter()
            .position(|l| l.contains(SETTINGS[last].key))
            .expect("the last key is on screen");
        let mut terminal = Terminal::new(TestBackend::new(100, 16)).unwrap();
        terminal
            .draw(|f| render_global_settings(f, f.area(), &store, last, &Theme::wsx()))
            .unwrap();
        assert_eq!(
            terminal.backend().buffer()[(3, y as u16)].bg,
            Theme::wsx().selected_bg
        );
    }
}
