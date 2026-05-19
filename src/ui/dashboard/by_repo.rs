//! By-repo view: renders one section per repo, with a header that
//! embeds per-status counts on a horizontal rule, and a nested list of
//! workspace rows underneath when expanded.

use crate::ui::dashboard::row::{self, RowInputs};
use crate::ui::dashboard::sort::{StatusCounts, noise_score};
use crate::ui::dashboard::status::Status;
use crate::ui::theme::Theme;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::ListItem;

#[derive(Debug, Clone)]
pub struct RepoView<'a> {
    pub id: u64,
    pub name: &'a str,
    pub path: &'a str,
    pub counts: StatusCounts,
    pub expanded: bool,
    /// Already sorted by Status priority (Stalled first).
    pub workspaces: Vec<RowInputs>,
}

/// Order repos by descending noise score; empty repos to the end.
pub fn order_repos(repos: &mut [RepoView<'_>]) {
    repos.sort_by(|a, b| {
        let a_empty = a.counts.total() == 0;
        let b_empty = b.counts.total() == 0;
        match (a_empty, b_empty) {
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            _ => noise_score(b.counts).cmp(&noise_score(a.counts)),
        }
    });
}

pub fn header_line(view: &RepoView<'_>, width: usize, theme: &Theme) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let fold_glyph = if view.counts.total() == 0 {
        ' '
    } else if view.expanded {
        '▾'
    } else {
        '▸'
    };
    spans.push(Span::styled(fold_glyph.to_string(), theme.dim_style()));
    spans.push(Span::raw(" ".to_string()));
    spans.push(Span::styled(view.name.to_string(), theme.header_style()));
    spans.push(Span::raw("  ".to_string()));
    spans.push(Span::styled(view.path.to_string(), theme.dim_style()));
    spans.push(Span::raw("  ".to_string()));

    let mut right: Vec<Span<'static>> = Vec::new();
    let cells = [
        (Status::Question, view.counts.question, true),
        (Status::Stalled, view.counts.stalled, true),
        (Status::Waiting, view.counts.waiting, false),
        (Status::Thinking, view.counts.thinking, false),
        (Status::Complete, view.counts.complete, false),
        (Status::Idle, view.counts.idle, false),
    ];
    let mut first = true;
    for (status, n, bold) in cells {
        if n == 0 {
            continue;
        }
        if !first {
            right.push(Span::raw("  ".to_string()));
        }
        first = false;
        let mut style = theme.status_style(status);
        if bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        if matches!(status, Status::Idle) {
            style = theme.dim_style();
        }
        right.push(Span::styled(format!("{} {}", status.glyph(), n), style));
    }

    let suffix = if view.counts.total() == 0 {
        "no workspaces".to_string()
    } else {
        format!("{} ws", view.counts.total())
    };
    right.push(Span::raw("    ".to_string()));
    right.push(Span::styled(suffix, theme.dim_style()));

    let used_left: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let used_right: usize = right.iter().map(|s| s.content.chars().count()).sum();
    let rule_len = width.saturating_sub(used_left + used_right + 2).max(1);
    spans.push(Span::styled("─".repeat(rule_len), theme.dim_style()));
    spans.push(Span::raw("  ".to_string()));
    spans.extend(right);
    Line::from(spans)
}

/// Emit the full sequence of `ListItem`s for the by-repo view.
pub fn render_list(
    repos: &[RepoView<'_>],
    widths: row::ColumnWidths,
    tick: u32,
    width: usize,
    theme: &Theme,
) -> Vec<ListItem<'static>> {
    let mut items: Vec<ListItem<'static>> = Vec::new();
    for view in repos {
        items.push(ListItem::new(header_line(view, width, theme)));
        if !view.expanded {
            continue;
        }
        for w in &view.workspaces {
            items.push(ListItem::new(row::render(w, widths, tick, theme, width)));
        }
        items.push(ListItem::new(""));
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::dashboard::fixture;

    fn make_view<'a>(r: &'a fixture::FixtureRepo, id: u64, expanded: bool) -> RepoView<'a> {
        let mut workspaces: Vec<RowInputs> = r
            .workspaces
            .iter()
            .enumerate()
            .map(|(i, w)| RowInputs {
                status: w.status,
                name: w.name.clone(),
                branch: w.branch.clone(),
                procs: w.procs,
                diff: Some(crate::git::DiffStats {
                    added: w.diff_added,
                    removed: w.diff_removed,
                }),
                last_message: w.last_message.clone(),
                ago_secs: w.ago_secs,
                selected: i == 0,
                yolo: false,
                setup_failed: false,
                lifecycle: None,
                nerd_fonts: false,
                workspace_id: crate::store::WorkspaceId(i as i64),
            })
            .collect();
        workspaces.sort_by(|a, b| b.status.priority().cmp(&a.status.priority()));
        let counts = StatusCounts::from_iter(workspaces.iter().map(|w| w.status));
        RepoView {
            id,
            name: r.name.as_str(),
            path: r.path.as_str(),
            counts,
            expanded,
            workspaces,
        }
    }

    fn header_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn header_shows_fold_glyph_and_counts() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = make_view(wsx, 1, true);
        let line = header_line(&view, 120, &theme);
        let t = header_text(&line);
        assert!(t.starts_with("▾ wsx"), "expanded fold + name: {t:?}");
        assert!(t.contains("/home/eben/workspace/wsx"));
        assert!(t.contains("? 1"));
        assert!(t.contains("! 1"));
        assert!(t.contains("… 1"));
        assert!(t.contains("✓ 1"));
        assert!(t.trim_end().ends_with("4 ws"));
    }

    #[test]
    fn header_for_empty_repo_shows_no_workspaces() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let frontend = repos.iter().find(|r| r.name == "frontend").unwrap();
        let view = make_view(frontend, 2, false);
        let line = header_line(&view, 120, &theme);
        let t = header_text(&line);
        assert!(
            t.starts_with("  frontend"),
            "no fold glyph for empty: {t:?}"
        );
        assert!(t.contains("no workspaces"));
    }

    #[test]
    fn collapsed_repo_emits_no_rows() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = make_view(wsx, 1, false);
        let items = render_list(&[view], row::ColumnWidths::default(), 0, 120, &theme);
        assert_eq!(items.len(), 1, "only the header for a collapsed repo");
    }

    #[test]
    fn expanded_repo_emits_header_then_rows_then_blank() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = make_view(wsx, 1, true);
        let items = render_list(&[view], row::ColumnWidths::default(), 0, 120, &theme);
        // 1 header + 4 workspaces + 1 spacer
        assert_eq!(items.len(), 6);
    }

    #[test]
    fn order_repos_puts_noisy_first_and_empty_last() {
        let theme = Theme::wsx();
        let _ = theme;
        let repos = fixture::repos();
        let mut views: Vec<RepoView<'_>> = repos
            .iter()
            .enumerate()
            .map(|(i, r)| make_view(r, i as u64, true))
            .collect();
        order_repos(&mut views);
        let names: Vec<&str> = views.iter().map(|v| v.name).collect();
        let wsx_pos = names.iter().position(|n| *n == "wsx").unwrap();
        let ssk_pos = names.iter().position(|n| *n == "ssk").unwrap();
        assert!(wsx_pos < ssk_pos, "wsx before ssk: {names:?}");
        let frontend_pos = names.iter().position(|n| *n == "frontend").unwrap();
        let scp_api_pos = names.iter().position(|n| *n == "scp-api").unwrap();
        assert!(frontend_pos >= names.len() - 2);
        assert!(scp_api_pos >= names.len() - 2);
    }

    #[test]
    fn within_repo_workspaces_are_priority_sorted() {
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = make_view(wsx, 1, true);
        let names: Vec<&str> = view.workspaces.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names[0], "theme-tokens", "stalled first");
        assert_eq!(names[1], "repo-overview", "question second");
    }
}
