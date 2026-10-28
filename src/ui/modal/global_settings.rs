//! The global settings drawer: every `wsx config` key, grouped, with the
//! selected key's description in the footer.

use super::repo_settings::preview_value;
use super::*;
use crate::config::settings::{SETTINGS, SettingKind};
use unicode_width::UnicodeWidthStr;

pub fn render_global_settings(
    f: &mut Frame,
    area: Rect,
    store: &crate::data::store::Store,
    selected: usize,
    theme: &Theme,
) {
    let selected = selected.min(SETTINGS.len() - 1);
    let def = &SETTINGS[selected];
    let description = match def.kind {
        SettingKind::Choice(values) => format!("{} ({})", def.description, values.join(", ")),
        _ => def.description.to_string(),
    };
    let groups = SETTINGS
        .iter()
        .enumerate()
        .filter(|(i, d)| *i == 0 || SETTINGS[i - 1].group != d.group)
        .count();
    // Wrapped, up to four lines, so a narrow drawer still shows the choices.
    let desc_lines = |width: u16| {
        let width = usize::from(width).saturating_sub(4).max(1);
        description.width().div_ceil(width).clamp(1, 4)
    };
    // One line per key and per group heading, plus description and hints.
    let inner = panel_frame(
        f,
        area,
        SETTINGS.len() + groups + desc_lines(area.width) + 1,
        " Global settings ",
        theme,
    );
    let footer_h = desc_lines(inner.width) as u16 + 1;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(footer_h)])
        .split(inner);
    let (body_area, footer_area) = (chunks[0], chunks[1]);

    // Keep at least MIN_VALUE columns for the value; long keys truncate instead.
    const MIN_VALUE: usize = 16;
    let longest = SETTINGS.iter().map(|d| d.key.len()).max().unwrap_or(0) + 1;
    let label_pad = longest
        .min((body_area.width as usize).saturating_sub(3 + MIN_VALUE))
        .max(1);
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
            (None, default) => preview_value(&format!("(default: {default})"), value_width),
        };
        let body = format!(
            "  {:<label_pad$} {preview}",
            truncate_key(def.key, label_pad - 1)
        );
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

    let enter = match def.kind {
        SettingKind::Toggle { .. } => "toggle",
        SettingKind::Choice(_) => "next",
        SettingKind::Text { .. } => "edit",
    };
    let footer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(footer_area);
    f.render_widget(
        Paragraph::new(Span::styled(description, theme.dim_style()))
            .wrap(ratatui::widgets::Wrap { trim: true })
            .block(Block::default().padding(ratatui::widgets::Padding::horizontal(2))),
        footer[0],
    );
    f.render_widget(
        Paragraph::new(vec![crate::ui::drawer::hint_line(
            &[
                ("↑↓", "move"),
                ("↵", enter),
                ("d", "reset"),
                ("esc", "close"),
            ],
            theme,
        )]),
        footer[1],
    );
}

/// `key` cut to `max` columns with an ellipsis; keys are ASCII.
fn truncate_key(key: &str, max: usize) -> std::borrow::Cow<'_, str> {
    if key.len() <= max {
        key.into()
    } else {
        format!("{}…", &key[..max.saturating_sub(1)]).into()
    }
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
    fn a_narrow_drawer_keeps_values_and_wraps_the_description() {
        let store = Store::open_in_memory().unwrap();
        store.set_setting("theme", "nord").unwrap();
        let i = SETTINGS.iter().position(|d| d.key == "theme").unwrap();
        let lines = screen(&store, i, 40, 60);
        assert!(row(&lines, "theme").contains("nord"), "{lines:#?}");
        assert!(
            lines.iter().any(|l| l.contains("dashboard_blocked…")),
            "{lines:#?}"
        );
        let text: String = lines.iter().map(|l| l.trim()).collect::<Vec<_>>().join(" ");
        assert!(text.contains("jellybeans, nord)"), "{text}");
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
