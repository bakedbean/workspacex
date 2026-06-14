//! Extracted from ui/modal.rs.

use super::*;

/// Render the floating repo-settings modal. Live state — reads
/// current values from the borrowed `Repo` struct.
pub fn render_repo_settings(
    f: &mut Frame,
    area: Rect,
    repo_name: &str,
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
        format!(" Repo settings — {repo_name} "),
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
            repo.pinned_commands.as_deref(),
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
        let label_pad = 22; // width of the longest label + breathing room
        let preview = value
            .map(|v| preview_value(v, 60))
            .unwrap_or_else(|| "(unset)".to_string());
        let body = format!("  {:<width$} {}", field.label(), preview, width = label_pad);
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
    f.render_widget(Paragraph::new(lines), body_area);

    f.render_widget(
        Paragraph::new("[\u{2191}/\u{2193}] move   [enter] edit   [d] clear   [esc] close")
            .style(theme.dim_style()),
        footer_area,
    );
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
