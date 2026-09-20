//! Extracted from ui/modal.rs.

use super::*;
use unicode_width::UnicodeWidthStr;

/// Render repo-local values, with global previews for inherited settings.
pub fn render_repo_settings(
    f: &mut Frame,
    area: Rect,
    store: &crate::data::store::Store,
    repo: &crate::data::store::Repo,
    selected: usize,
    theme: &Theme,
) {
    let w = area.width.clamp(40, 90);
    let h = area.height.clamp(12, 20);
    let inner = panel_frame(
        f,
        area,
        w,
        h,
        format!(" Repo settings — {} ", repo.name),
        theme,
    );

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let body_area = chunks[0];
    let footer_area = chunks[1];

    let rows: [(crate::app::RepoSettingField, Option<&str>); 9] = [
        (
            crate::app::RepoSettingField::RepoName,
            Some(repo.name.as_str()),
        ),
        (
            crate::app::RepoSettingField::BranchPrefix,
            if repo.branch_prefix.is_empty() {
                None
            } else {
                Some(repo.branch_prefix.as_str())
            },
        ),
        (
            crate::app::RepoSettingField::BaseBranch,
            repo.base_branch.as_deref(),
        ),
        (
            crate::app::RepoSettingField::CustomInstructions,
            repo.custom_instructions.as_deref(),
        ),
        (
            crate::app::RepoSettingField::SetupScript,
            repo.setup_script.as_deref(),
        ),
        (
            crate::app::RepoSettingField::ArchiveScript,
            repo.archive_script.as_deref(),
        ),
        (
            crate::app::RepoSettingField::PinnedCommands,
            repo.pinned_commands
                .as_deref()
                .filter(|value| !value.trim().is_empty()),
        ),
        (
            crate::app::RepoSettingField::RelatedRepos,
            repo.related_repos.as_deref(),
        ),
        (
            crate::app::RepoSettingField::DetailBarConfig,
            repo.detail_bar_config.as_deref(),
        ),
    ];

    let mut lines: Vec<Line> = Vec::new();
    for (i, (field, value)) in rows.iter().enumerate() {
        let mut label_pad = 22; // width of the longest label + breathing room
        let global = if value.is_none()
            && matches!(
                field,
                crate::app::RepoSettingField::BranchPrefix
                    | crate::app::RepoSettingField::CustomInstructions
                    | crate::app::RepoSettingField::PinnedCommands
                    | crate::app::RepoSettingField::DetailBarConfig
            ) {
            store.get_setting(field.label()).ok().flatten()
        } else {
            None
        };
        let inherited = global.as_deref().filter(|value| !value.trim().is_empty());
        let source = if inherited.is_some() {
            "(inherited) "
        } else {
            ""
        };
        let preview = if matches!(field, crate::app::RepoSettingField::PinnedCommands) {
            // Keep room for the overflow count even in a narrow inherited row.
            label_pad = label_pad.min(
                (body_area.width as usize)
                    .saturating_sub(3 + source.len() + 8)
                    .max(field.label().len()),
            );
            let width = (body_area.width as usize).saturating_sub(3 + label_pad + source.len());
            value
                .or(inherited)
                .map(|v| preview_pinned_commands(v, width))
                .unwrap_or_else(|| "(unset)".to_string())
        } else {
            value
                .or(inherited)
                .map(|v| preview_value(v, 60))
                .unwrap_or_else(|| "(unset)".to_string())
        };
        let body = format!(
            "  {:<width$} {source}{preview}",
            field.label(),
            width = label_pad
        );
        let style = if value.is_none() {
            theme.dim_style()
        } else {
            Style::default()
        };
        if i == selected {
            lines.push(Line::from(Span::styled(body, theme.selected_style())));
        } else {
            lines.push(Line::from(Span::styled(body, style)));
        }
    }
    if body_area.height > rows.len() as u16 {
        lines.push(Line::from(Span::styled(
            "  Inherited from global config; edit sets a repo value.",
            theme.dim_style(),
        )));
    }
    f.render_widget(Paragraph::new(lines), body_area);

    f.render_widget(
        Paragraph::new("[\u{2191}/\u{2193}] move   [enter] edit   [d] clear   [esc] close")
            .style(theme.dim_style()),
        footer_area,
    );
}

/// Show complete labels and count every command whose label does not fit.
fn preview_pinned_commands(value: &str, max_cols: usize) -> String {
    let commands = crate::commands::pinned::parse(value);
    if commands.is_empty() {
        return "(none)".to_string();
    }
    let full_width = commands
        .iter()
        .map(|command| command.label.width())
        .sum::<usize>()
        + 2 * (commands.len() - 1);
    let mut preview = String::new();
    let mut width = 0;
    let mut shown = 0;
    for command in &commands {
        let next_width = width + usize::from(shown > 0) * 2 + command.label.width();
        let remaining = commands.len() - shown - 1;
        // Reserve ", +N more" before admitting another complete label.
        let suffix_width = if remaining > 0 {
            8 + remaining.ilog10() as usize + 1
        } else {
            0
        };
        if full_width > max_cols && next_width + suffix_width > max_cols {
            break;
        }
        if shown > 0 {
            preview.push_str(", ");
        }
        preview.push_str(&command.label);
        width = next_width;
        shown += 1;
    }
    if shown < commands.len() {
        if shown > 0 {
            preview.push_str(", ");
        }
        preview.push_str(&format!("+{}", commands.len() - shown));
        // Very narrow rows may only have room for the count itself.
        if preview.width() + 5 <= max_cols {
            preview.push_str(" more");
        }
    }
    preview
}

/// First non-empty line, trimmed and truncated. Used by render_repo_settings.
fn preview_value(s: &str, max: usize) -> String {
    let first_line = s.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let trimmed = first_line.trim();
    if trimmed.chars().count() <= max {
        trimmed.to_string()
    } else {
        let mut out: String = trimmed.chars().take(max.saturating_sub(1)).collect();
        out.push('\u{2026}');
        out
    }
}

#[cfg(test)]
mod preview_tests {
    use super::*;

    fn pinned_row(global: Option<&str>, local: Option<&str>, width: u16) -> String {
        use crate::data::store::Store;
        use ratatui::{Terminal, backend::TestBackend};
        use std::path::Path;

        let store = Store::open_in_memory().unwrap();
        let id = store
            .add_repo(Path::new("/preview"), "preview", "")
            .unwrap();
        if let Some(global) = global {
            store.set_setting("pinned_commands", global).unwrap();
        }
        store.set_repo_pinned_commands(id, local).unwrap();
        let repo = store.repos().unwrap().remove(0);
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
        terminal
            .draw(|f| render_repo_settings(f, f.area(), &store, &repo, 6, &Theme::wsx()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .find(|line| line.contains("pinned_commands"))
            .unwrap()
    }

    #[test]
    fn pinned_preview_shows_multiline_labels_from_the_effective_source() {
        let global = "PR=/pr\nReview=/review\nTests=/test";
        let inherited = pinned_row(Some(global), None, 80);
        assert!(
            inherited.contains("(inherited) PR, Review, Tests"),
            "{inherited}"
        );
        let local = pinned_row(
            Some(global),
            Some("Build=cargo build\nLint=cargo clippy"),
            80,
        );
        assert!(local.contains("Build, Lint"), "{local}");
        assert!(!local.contains("(inherited)") && !local.contains("Review"));
        let whitespace = pinned_row(Some(global), Some(" \n\t"), 80);
        assert!(
            whitespace.contains("(inherited) PR, Review, Tests"),
            "{whitespace}"
        );
    }

    #[test]
    fn pinned_preview_keeps_overflow_count_inside_the_modal() {
        let commands = "PR=/pr\nReview=/review\nTests=/test\nDeploy=/deploy\n\
                        Logs=/logs\nFormat=/format\nStatus=/status\nRebase=/rebase";
        let inherited = pinned_row(Some(commands), None, 80);
        assert!(
            inherited.contains("(inherited) PR, Review, Tests, Deploy, Logs, +3 more"),
            "{inherited}"
        );
        let narrow = pinned_row(Some(commands), None, 40);
        assert!(narrow.contains("(inherited) +8 more"), "{narrow}");
        let wide_labels = pinned_row(
            None,
            Some("界界界界界界界界界界=/one\nReview=/two\nTests=/three"),
            60,
        );
        // TestBackend retains a blank continuation cell after each wide glyph.
        assert_eq!(wide_labels.matches('界').count(), 10, "{wide_labels}");
        assert!(wide_labels.contains(", +2 more"), "{wide_labels}");
        let many = "PR=/pr\n".repeat(100);
        let narrow_count = pinned_row(Some(&many), None, 40);
        assert_eq!(
            narrow_count
                .split("(inherited)")
                .nth(1)
                .unwrap()
                .trim_end_matches('│')
                .trim(),
            "+100",
            "{narrow_count}"
        );
    }

    #[test]
    fn preview_value_returns_first_nonempty_line() {
        assert_eq!(preview_value("\n  \nhello\nworld", 60), "hello");
    }

    #[test]
    fn preview_value_truncates_with_ellipsis() {
        let long = "x".repeat(100);
        let out = preview_value(&long, 60);
        assert!(out.ends_with('\u{2026}'));
        assert_eq!(out.chars().count(), 60);
    }

    #[test]
    fn preview_value_empty_returns_empty() {
        assert_eq!(preview_value("", 60), "");
    }
}
