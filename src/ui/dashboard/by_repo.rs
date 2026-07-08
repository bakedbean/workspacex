//! By-repo view: renders one section per repo, with a header that
//! embeds per-status counts on a horizontal rule, and a nested list of
//! workspace rows underneath when expanded.

use crate::ui::dashboard::row::{self, RowInputs};
use crate::ui::dashboard::sort::StatusCounts;
use crate::ui::dashboard::status::Status;
use crate::ui::theme::Theme;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::ListItem;

#[derive(Debug, Clone)]
pub struct RepoView<'a> {
    pub id: u64,
    pub name: &'a str,
    /// Lossy-converted display path — `RepoView` owns the string so
    /// non-UTF8 path bytes survive the conversion (with U+FFFD
    /// substitution) instead of being dropped to an empty string.
    pub path: String,
    pub counts: StatusCounts,
    pub expanded: bool,
    /// Persisted manual order; repos render ascending by this. Stable across
    /// workspace add/remove/status changes.
    pub sort_order: i64,
    /// Already sorted by Status priority (Stalled first).
    pub workspaces: Vec<RowInputs>,
}

/// Order repos by their persisted manual `sort_order`, ascending, with the
/// immutable repo `id` as a tiebreaker so the order is total and deterministic
/// even if two repos ever share a `sort_order`. `visible_targets` (the nav
/// index builder) must use the identical key to stay in lockstep. This is
/// stable: workspace activity never changes a repo's position.
pub fn order_repos(repos: &mut [RepoView<'_>]) {
    repos.sort_by_key(|r| (r.sort_order, r.id));
}

/// Spaces flanking the filler rule on each side.
const RULE_PAD: usize = 2;

/// Width that right-justifies every repo's `name` to a shared right edge: the
/// widest repo name's character count. `header_line` left-pads each shorter
/// name up to this width so all names end in the same column.
fn name_align_width(repos: &[RepoView<'_>]) -> usize {
    repos
        .iter()
        .map(|r| r.name.chars().count())
        .max()
        .unwrap_or(0)
}

fn header_line(
    view: &RepoView<'_>,
    name_width: usize,
    width: usize,
    theme: &Theme,
) -> Line<'static> {
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
    // Right-justify the name, filling the blank space its left-pad opens up
    // with a rule (matching the pinned-command row's filler). A space on each
    // side keeps the rule from touching the glyph or the name.
    let name_len = view.name.chars().count();
    let pad = name_width.saturating_sub(name_len);
    if pad > 0 {
        if pad > 1 {
            spans.push(Span::styled("─".repeat(pad - 1), theme.dim_style()));
        }
        spans.push(Span::raw(" ".to_string()));
    }
    spans.push(Span::styled(view.name.to_string(), theme.header_style()));

    // Status counts immediately follow the name. Empty repos show nothing —
    // the absence of workspace rows is self-explanatory, no label needed.
    if view.counts.total() > 0 {
        spans.push(Span::raw("  ".to_string()));
        let cells = [
            (Status::Question, view.counts.question, true),
            (Status::Stalled, view.counts.stalled, true),
            (Status::Waiting, view.counts.waiting, false),
            (Status::Thinking, view.counts.thinking, false),
            (Status::Complete, view.counts.complete, false),
            (Status::Detached, view.counts.detached, false),
            (Status::Idle, view.counts.idle, false),
        ];
        let mut first = true;
        for (status, n, bold) in cells {
            if n == 0 {
                continue;
            }
            if !first {
                spans.push(Span::raw("  ".to_string()));
            }
            first = false;
            let mut style = theme.status_style(status);
            if bold {
                style = style.add_modifier(Modifier::BOLD);
            }
            if matches!(status, Status::Idle) {
                style = theme.dim_style();
            }
            spans.push(Span::styled(format!("{} {}", status.glyph(), n), style));
        }
        spans.push(Span::raw("    ".to_string()));
        spans.push(Span::styled(
            format!("{} ws", view.counts.total()),
            theme.dim_style(),
        ));
    }

    // Path is flush-right; the rule fills the gap between the counts and the
    // path, flanked by RULE_PAD spaces. Size the rule from the *actual* gap so
    // the path's right edge lands exactly at `width` — never force a minimum
    // rule, which would push the line one column past `width` and clip the
    // path. When the gap is too small for a padded rule, fall back to plain
    // spaces; if the left content + path already overflow, the gap is zero.
    let path_len = view.path.chars().count();
    let used_left: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let gap = width.saturating_sub(used_left + path_len);
    if gap > RULE_PAD * 2 {
        let rule = "─".repeat(gap - RULE_PAD * 2);
        spans.push(Span::raw(" ".repeat(RULE_PAD)));
        spans.push(Span::styled(rule, theme.dim_style()));
        spans.push(Span::raw(" ".repeat(RULE_PAD)));
    } else {
        spans.push(Span::raw(" ".repeat(gap)));
    }
    spans.push(Span::styled(view.path.to_string(), theme.dim_style()));
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
    let name_width = name_align_width(repos);
    for view in repos {
        items.push(ListItem::new(header_line(view, name_width, width, theme)));
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
    use crate::ui::dashboard::column_content::{ColumnEmphasis, RowColumn};
    use crate::ui::dashboard::fixture;

    fn make_view<'a>(r: &'a fixture::FixtureRepo, id: u64, expanded: bool) -> RepoView<'a> {
        let mut workspaces: Vec<RowInputs> = r
            .workspaces
            .iter()
            .enumerate()
            .map(|(i, w)| RowInputs {
                agent: crate::pty::session::AgentKind::Claude,
                status: w.status,
                name: w.name.clone(),
                branch: w.branch.clone(),
                procs: w.procs,
                diff: Some(crate::git::DiffStats {
                    added: w.diff_added,
                    removed: w.diff_removed,
                }),
                column: w.last_message.clone().map(|t| RowColumn {
                    text: t,
                    emphasis: ColumnEmphasis::Dim,
                }),
                ago_secs: w.ago_secs,
                selected: i == 0,
                yolo: false,
                setup_failed: false,
                shared: false,
                shared_active: false,
                lifecycle: None,
                nerd_fonts: false,
                workspace_id: crate::data::store::WorkspaceId(i as i64),
                has_multi_pane_layout: false,
            })
            .collect();
        workspaces.sort_by_key(|w| std::cmp::Reverse(w.status.priority()));
        let counts = StatusCounts::from_iter(workspaces.iter().map(|w| w.status));
        RepoView {
            id,
            name: r.name.as_str(),
            path: r.path.clone(),
            counts,
            expanded,
            sort_order: id as i64,
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
        let align = name_align_width(std::slice::from_ref(&view));
        let line = header_line(&view, align, 120, &theme);
        let t = header_text(&line);
        assert!(t.starts_with("▾ wsx"), "expanded fold + name: {t:?}");
        assert!(t.contains("? 1"));
        assert!(t.contains("! 1"));
        assert!(t.contains("… 1"));
        assert!(t.contains("✓ 1"));
        assert!(t.contains("4 ws"));
        // Path is now flush-right, so it lands at the end of the line.
        assert!(t.trim_end().ends_with("/home/eben/workspace/wsx"));
    }

    #[test]
    fn header_for_empty_repo_omits_count_label() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let frontend = repos.iter().find(|r| r.name == "frontend").unwrap();
        let view = make_view(frontend, 2, false);
        let align = name_align_width(std::slice::from_ref(&view));
        let line = header_line(&view, align, 120, &theme);
        let t = header_text(&line);
        assert!(
            t.starts_with("  frontend"),
            "no fold glyph for empty: {t:?}"
        );
        // Empty repos carry no count label — not even "no workspaces".
        assert!(
            !t.contains("no workspaces"),
            "empty repo label dropped: {t:?}"
        );
        assert!(!t.contains(" ws"), "no count suffix for empty repo: {t:?}");
        // Path still renders flush-right.
        assert!(t.trim_end().ends_with("/home/eben/meals/frontend"));
    }

    /// Char column where the first occurrence of `needle` ends in the text.
    fn substr_end_col(line: &Line<'_>, needle: &str) -> usize {
        let text = header_text(line);
        let byte_idx = text.find(needle).expect("substring present in header");
        text[..byte_idx].chars().count() + needle.chars().count()
    }

    #[test]
    fn names_right_justified_and_paths_flush_right() {
        let theme = Theme::wsx();
        let width = 120;
        let repos = fixture::repos();
        // Two repos with different name lengths and different path lengths.
        let short = repos.iter().find(|r| r.name == "wsx").unwrap();
        let long = repos.iter().find(|r| r.name == "scp-admin").unwrap();
        let views = [make_view(short, 1, true), make_view(long, 2, false)];
        let name_width = name_align_width(&views);

        let short_line = header_line(&views[0], name_width, width, &theme);
        let long_line = header_line(&views[1], name_width, width, &theme);

        // Names are right-justified: both end in the same column.
        assert_eq!(
            substr_end_col(&short_line, views[0].name),
            substr_end_col(&long_line, views[1].name),
            "right-justified names must end in the same column"
        );
        // Paths are flush to the terminal's right edge.
        assert_eq!(substr_end_col(&short_line, &views[0].path), width);
        assert_eq!(substr_end_col(&long_line, &views[1].path), width);
    }

    #[test]
    fn path_stays_flush_right_without_overflow() {
        // Across every width, the rendered line is exactly `width` once the
        // content fits, and never longer (which would clip the flush-right
        // path). Below the fit threshold it stays pinned at the minimum
        // content width. Regression for forcing a >=1 rule that overshot by one
        // column at the boundary.
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let view = make_view(repos.iter().find(|r| r.name == "wsx").unwrap(), 1, true);
        let name_width = name_align_width(std::slice::from_ref(&view));
        // Minimum content width = the line with a zero gap (rendered at width 0).
        let min_content = header_text(&header_line(&view, name_width, 0, &theme))
            .chars()
            .count();
        for width in 0..=200 {
            let line = header_line(&view, name_width, width, &theme);
            let len = header_text(&line).chars().count();
            assert_eq!(
                len,
                width.max(min_content),
                "line width must be exactly `width` when it fits (never +1): width={width}"
            );
            if width >= min_content {
                assert_eq!(
                    substr_end_col(&line, &view.path),
                    width,
                    "path stays flush to the right edge at width={width}"
                );
            }
        }
    }

    #[test]
    fn short_names_get_a_left_fill_rule() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let short = repos.iter().find(|r| r.name == "wsx").unwrap();
        let long = repos.iter().find(|r| r.name == "scp-admin").unwrap();
        let views = [make_view(short, 1, true), make_view(long, 2, true)];
        let name_width = name_align_width(&views);

        // The shorter name's left-pad is filled with a rule (one space before
        // the name), matching the pinned-command row's filler.
        let short_t = header_text(&header_line(&views[0], name_width, 120, &theme));
        assert!(short_t.contains("─ wsx"), "left-fill rule: {short_t:?}");

        // The widest name has no left pad, so it hugs the glyph — no rule.
        let long_t = header_text(&header_line(&views[1], name_width, 120, &theme));
        assert!(long_t.starts_with("▾ scp-admin"), "no rule: {long_t:?}");
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
    fn order_repos_sorts_by_sort_order_ascending() {
        let repos = fixture::repos();
        // Build views, then assign sort_order in REVERSE of fixture order so a
        // correct ascending sort visibly reorders them (id stays the identity).
        let mut views: Vec<RepoView<'_>> = repos
            .iter()
            .enumerate()
            .map(|(i, r)| make_view(r, i as u64, true))
            .collect();
        let n = views.len() as i64;
        for (i, v) in views.iter_mut().enumerate() {
            v.sort_order = n - 1 - i as i64;
        }
        order_repos(&mut views);
        let orders: Vec<i64> = views.iter().map(|v| v.sort_order).collect();
        let mut sorted = orders.clone();
        sorted.sort();
        assert_eq!(orders, sorted, "repos must be in ascending sort_order");
        // Activity/emptiness must NOT affect order anymore.
        assert_eq!(views.first().unwrap().sort_order, 0);
        assert_eq!(views.last().unwrap().sort_order, n - 1);
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
