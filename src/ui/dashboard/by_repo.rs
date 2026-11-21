//! By-repo view: renders one section per repo, with a header that pairs the
//! repo name with its path and per-status counts, underlined by a horizontal
//! rule, and a nested list of workspace rows underneath when expanded.
//!
//! The header reads `▾    name  /path/to/repo        ? 1  ✓ 2    3 ws`:
//! names right-justified to a shared column, then each path left-justified
//! in the column that opens up after it, counts flush-right, and a full-width
//! rule on the line beneath.

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
    /// Summary rows for workspaces the repo's hide rules keep off the list.
    pub hidden: Vec<crate::ui::dashboard::hidden::HiddenGroupView>,
    /// Per-view copy of the global nerd-fonts setting, mirroring
    /// `RowInputs::nerd_fonts`; selects the hidden-group line's glyphs.
    pub nerd_fonts: bool,
}

/// Order repos by their persisted manual `sort_order`, ascending, with the
/// immutable repo `id` as a tiebreaker so the order is total and deterministic
/// even if two repos ever share a `sort_order`. `visible_targets` (the nav
/// index builder) must use the identical key to stay in lockstep. This is
/// stable: workspace activity never changes a repo's position.
pub fn order_repos(repos: &mut [RepoView<'_>]) {
    repos.sort_by_key(|r| (r.sort_order, r.id));
}

/// A clickable span on a repo header: `(cell offset in the line, width)`.
type HeaderSpan = (u16, u16);

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

/// Build a repo header line, plus the span of its clickable repo name. The
/// span is derived from the spans actually pushed, so the paint and the
/// click target can't drift — the same contract `row::pr_chip_hit_span`
/// keeps for workspace rows.
fn header_line(
    view: &RepoView<'_>,
    name_width: usize,
    width: usize,
    theme: &Theme,
) -> (Line<'static>, HeaderSpan) {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let fold_glyph = if view.counts.total() == 0 && view.hidden.is_empty() {
        ' '
    } else if view.expanded {
        '▾'
    } else {
        '▸'
    };
    spans.push(Span::styled(fold_glyph.to_string(), theme.dim_style()));
    spans.push(Span::raw(" ".to_string()));
    // Right-justify the name with blank left-padding; the rule lives on its
    // own line beneath the header.
    let name_len = view.name.chars().count();
    let pad = name_width.saturating_sub(name_len);
    if pad > 0 {
        spans.push(Span::raw(" ".repeat(pad)));
    }
    let name_offset: usize = spans.iter().map(|s| s.width()).sum();
    let name_span = Span::styled(view.name.to_string(), theme.header_style());
    let name_hit = (name_offset as u16, name_span.width() as u16);
    spans.push(name_span);

    // Then the path. Because the names are right-justified to a shared
    // column, every path starts in the same column for free.
    spans.push(Span::raw("  ".to_string()));
    spans.push(Span::styled(view.path.to_string(), theme.dim_style()));

    // Status counts are flush-right, built separately so the rule between the
    // path and the counts can be sized from the gap they leave. Empty repos
    // show nothing — the absence of workspace rows is self-explanatory, no
    // label needed — and the rule then runs to the right edge on its own.
    let right = count_spans(view.counts, theme);

    // Plain spaces push the counts flush-right; if the left content plus
    // the counts already overflow, the gap is zero.
    let used_left: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let used_right: usize = right.iter().map(|s| s.content.chars().count()).sum();
    let gap = width.saturating_sub(used_left + used_right);
    spans.push(Span::raw(" ".repeat(gap)));
    spans.extend(right);
    (Line::from(spans), name_hit)
}

/// The flush-right status tally of a repo header (`? 1  ● 2    3 ws`), or
/// nothing for an empty repo. Shared by the by-repo header and the focus
/// view's breadcrumb so both count the same way.
pub(crate) fn count_spans(counts: StatusCounts, theme: &Theme) -> Vec<Span<'static>> {
    let mut right: Vec<Span<'static>> = Vec::new();
    if counts.total() > 0 {
        let cells = [
            (Status::Question, counts.question, true),
            (Status::Stalled, counts.stalled, true),
            (Status::Waiting, counts.waiting, false),
            (Status::Thinking, counts.thinking, false),
            (Status::Complete, counts.complete, false),
            (Status::Idle, counts.idle, false),
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
        right.push(Span::raw("    ".to_string()));
        right.push(Span::styled(
            format!("{} ws", counts.total()),
            theme.dim_style(),
        ));
    }

    right
}

/// A clickable span on a repo header, positioned by flat list index:
/// `(repo id, flat item index, span)`. Mirrors the workspace rows'
/// `PrChipSpan`, which the caller resolves to screen rects the same way.
pub type RepoHeaderSpan = (u64, usize, HeaderSpan);

/// What `render_list` emits: the list items plus the click spans on the
/// repo headers.
pub struct RenderedList {
    pub items: Vec<ListItem<'static>>,
    /// Each header's repo name — clicking it focuses the repo.
    pub names: Vec<RepoHeaderSpan>,
}

/// Emit the full sequence of `ListItem`s for the by-repo view, plus the
/// click spans on every header.
pub fn render_list(
    repos: &[RepoView<'_>],
    widths: row::ColumnWidths,
    tick: u32,
    width: usize,
    theme: &Theme,
) -> RenderedList {
    let mut items: Vec<ListItem<'static>> = Vec::new();
    let mut names: Vec<RepoHeaderSpan> = Vec::new();
    let name_width = name_align_width(repos);
    let mut stripe = row::RowStripe::default();
    for view in repos {
        stripe.reset();
        let (line, name) = header_line(view, name_width, width, theme);
        names.push((view.id, items.len(), name));
        // Empty repos have nothing under them to set off, so they skip the
        // rule and stay a single row.
        if view.counts.total() == 0 && view.hidden.is_empty() {
            items.push(ListItem::new(line));
        } else {
            let rule = Line::from(Span::styled("─".repeat(width), theme.rule_style()));
            items.push(ListItem::new(vec![line, rule]));
        }
        if !view.expanded {
            continue;
        }
        for w in &view.workspaces {
            items.push(stripe.item(row::render(w, widths, tick, theme, width), theme));
        }
        for g in &view.hidden {
            items.push(ListItem::new(
                crate::ui::dashboard::hidden::hidden_group_line(g, None, view.nerd_fonts, theme),
            ));
            if g.expanded {
                for w in &g.workspaces {
                    items.push(stripe.item(row::render(w, widths, tick, theme, width), theme));
                }
            }
        }
        items.push(ListItem::new(""));
    }
    RenderedList { items, names }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::dashboard::column_content::{ColumnBody, ColumnEmphasis, RowColumn};
    use crate::ui::dashboard::fixture;

    fn make_view<'a>(r: &'a fixture::FixtureRepo, id: u64, expanded: bool) -> RepoView<'a> {
        let mut workspaces: Vec<RowInputs> = r
            .workspaces
            .iter()
            .enumerate()
            .map(|(i, w)| RowInputs {
                agent: crate::pty::session::AgentKind::Claude,
                peers: Vec::new(),
                status: w.status,
                branch: w.branch.clone(),
                pr_number: None,
                procs: w.procs,
                diff: Some(crate::git::DiffStats {
                    added: w.diff_added,
                    removed: w.diff_removed,
                }),
                column: w.last_message.clone().map(|t| RowColumn {
                    token: "idle".to_string(),
                    reported: false,
                    body: ColumnBody::Fallback {
                        text: t,
                        emphasis: ColumnEmphasis::Dim,
                    },
                }),
                ago_secs: w.ago_secs,
                last_active_ms: None,
                selected: i == 0,
                yolo: false,
                badge: None,
                undelivered_mail: false,
                shared: false,
                shared_active: false,
                lifecycle: None,
                review: None,
                unresolved: None,
                comments: None,
                nerd_fonts: false,
                name_color: None,
                workspace_id: crate::data::store::WorkspaceId(i as i64),
                has_multi_pane_layout: false,
                tags: Vec::new(),
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
            hidden: Vec::new(),
            nerd_fonts: false,
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
        let (line, _) = header_line(&view, align, 120, &theme);
        let t = header_text(&line);
        assert!(t.starts_with("▾ wsx"), "expanded fold + name: {t:?}");
        assert!(t.contains("? 1"));
        assert!(t.contains("! 1"));
        assert!(t.contains("… 1"));
        assert!(t.contains("✓ 1"));
        assert!(t.contains("4 ws"));
        // Path sits immediately after the name; the counts are flush-right, so
        // they — not the path — land at the end of the line.
        assert!(
            t.starts_with("▾ wsx  /home/eben/workspace/wsx  "),
            "path follows the name: {t:?}"
        );
        assert!(t.trim_end().ends_with("4 ws"), "counts flush-right: {t:?}");
    }

    #[test]
    fn header_for_empty_repo_omits_count_label() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let frontend = repos.iter().find(|r| r.name == "frontend").unwrap();
        let view = make_view(frontend, 2, false);
        let align = name_align_width(std::slice::from_ref(&view));
        let (line, _) = header_line(&view, align, 120, &theme);
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
        // Path still follows the name; the rule lives on its own line.
        assert!(
            t.starts_with("  frontend  /home/eben/meals/frontend"),
            "path follows name: {t:?}"
        );
        assert!(!t.contains('─'), "no inline rule: {t:?}");
    }

    /// Char column where the first occurrence of `needle` ends in the text.
    fn substr_end_col(line: &Line<'_>, needle: &str) -> usize {
        substr_start_col(line, needle) + needle.chars().count()
    }

    /// Char column where the first occurrence of `needle` starts in the text.
    fn substr_start_col(line: &Line<'_>, needle: &str) -> usize {
        let text = header_text(line);
        let byte_idx = text.find(needle).expect("substring present in header");
        text[..byte_idx].chars().count()
    }

    #[test]
    fn names_right_justified_and_paths_left_justified() {
        let theme = Theme::wsx();
        let width = 120;
        let repos = fixture::repos();
        // Two repos with different name lengths and different path lengths.
        let short = repos.iter().find(|r| r.name == "wsx").unwrap();
        let long = repos.iter().find(|r| r.name == "scp-admin").unwrap();
        let views = [make_view(short, 1, true), make_view(long, 2, false)];
        let name_width = name_align_width(&views);

        let (short_line, _) = header_line(&views[0], name_width, width, &theme);
        let (long_line, _) = header_line(&views[1], name_width, width, &theme);

        // Names are right-justified: both end in the same column.
        assert_eq!(
            substr_end_col(&short_line, views[0].name),
            substr_end_col(&long_line, views[1].name),
            "right-justified names must end in the same column"
        );
        // Which puts every path — of whatever length — in the same left-
        // justified start column, right after the name.
        assert_eq!(
            substr_start_col(&short_line, &views[0].path),
            substr_start_col(&long_line, &views[1].path),
            "left-justified paths must start in the same column"
        );
        // The counts are what's flush to the terminal's right edge now.
        assert_eq!(substr_end_col(&short_line, "4 ws"), width);
        assert_eq!(substr_end_col(&long_line, "1 ws"), width);
    }

    #[test]
    fn counts_stay_flush_right_without_overflow() {
        // Across every width, the rendered line is exactly `width` once the
        // content fits, and never longer (which would clip the flush-right
        // counts). Below the fit threshold it stays pinned at the minimum
        // content width. Regression for forcing a >=1 rule that overshot by one
        // column at the boundary.
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = make_view(wsx, 1, true);
        let name_width = name_align_width(std::slice::from_ref(&view));
        // Minimum content width = the line with a zero gap (width 0).
        let min_content = header_text(&header_line(&view, name_width, 0, &theme).0)
            .chars()
            .count();
        for width in 0..=200 {
            let (line, _) = header_line(&view, name_width, width, &theme);
            let len = header_text(&line).chars().count();
            assert_eq!(
                len,
                width.max(min_content),
                "line width must be exactly `width` when it fits (never +1): width={width}"
            );
            if width >= min_content {
                assert_eq!(
                    substr_end_col(&line, "4 ws"),
                    width,
                    "counts stay flush to the right edge: width={width}"
                );
            }
        }
    }

    #[test]
    fn short_names_are_space_padded() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let short = repos.iter().find(|r| r.name == "wsx").unwrap();
        let long = repos.iter().find(|r| r.name == "scp-admin").unwrap();
        let views = [make_view(short, 1, true), make_view(long, 2, true)];
        let name_width = name_align_width(&views);

        // The shorter name's left-pad is blank — the rule sits below the header.
        let short_t = header_text(&header_line(&views[0], name_width, 120, &theme).0);
        assert!(short_t.contains("      wsx  /"), "space pad: {short_t:?}");
        assert!(!short_t.contains('─'), "no inline rule: {short_t:?}");

        // The widest name has no left pad, so it hugs the glyph.
        let long_t = header_text(&header_line(&views[1], name_width, 120, &theme).0);
        assert!(long_t.starts_with("▾ scp-admin"), "no rule: {long_t:?}");
    }

    #[test]
    fn collapsed_repo_emits_no_rows() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = make_view(wsx, 1, false);
        let items = render_list(&[view], row::ColumnWidths::default(), 0, 120, &theme).items;
        assert_eq!(items.len(), 1, "only the header for a collapsed repo");
    }

    #[test]
    fn expanded_repo_emits_header_then_rows_then_blank() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = make_view(wsx, 1, true);
        let items = render_list(&[view], row::ColumnWidths::default(), 0, 120, &theme).items;
        // 1 header + 4 workspaces + 1 spacer
        assert_eq!(items.len(), 6);
    }

    #[test]
    fn rule_sits_under_non_empty_headers_only() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let frontend = repos.iter().find(|r| r.name == "frontend").unwrap();
        let views = [make_view(wsx, 1, false), make_view(frontend, 2, false)];
        let items = render_list(&views, row::ColumnWidths::default(), 0, 120, &theme).items;
        assert_eq!(
            items[0].height(),
            2,
            "header + rule for a repo with workspaces"
        );
        assert_eq!(items[1].height(), 1, "empty repo: header only");
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
        let names: Vec<&str> = view.workspaces.iter().map(|w| w.branch.as_str()).collect();
        assert_eq!(names[0], "bakedbean/theme-tokens", "stalled first");
        assert_eq!(names[1], "bakedbean/repo-overview", "question second");
    }

    /// Every other workspace under a repo takes the alternate background,
    /// as in the focus view; each repo header restarts the pattern.
    #[test]
    fn alternate_rows_are_tinted_per_repo() {
        use ratatui::{
            buffer::Buffer,
            layout::Rect,
            widgets::{List, Widget},
        };
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let views = [make_view(wsx, 1, true), make_view(wsx, 2, true)];
        let n = views[0].workspaces.len();
        assert!(n >= 3, "fixture needs a few rows");
        let list = render_list(&views, row::ColumnWidths::default(), 0, 120, &theme);
        let rows: usize = list.items.iter().map(|i| i.height()).sum();
        let area = Rect::new(0, 0, 120, rows as u16);
        let mut buf = Buffer::empty(area);
        List::new(list.items).render(area, &mut buf);
        let bg = |y: usize| buf[(119, y as u16)].bg;
        let alt = theme.bg_alt;
        assert_ne!(bg(0), alt, "repo header plain");
        assert_ne!(bg(1), alt, "header rule plain");
        for i in 0..n {
            let want = i % 2 == 1;
            assert_eq!(bg(2 + i) == alt, want, "row {i} of first repo");
        }
        // Header + rule, n rows, spacer, then the second repo's header and rows.
        let second = 2 + n + 1;
        assert_ne!(bg(second), alt, "second header plain");
        assert_ne!(bg(second + 2), alt, "first row under the second repo plain");
        assert_eq!(
            bg(second + 3),
            alt,
            "second row under the second repo tinted"
        );
    }

    /// Rows under an expanded hidden group continue the stripe: the group
    /// line itself neither takes the tint nor flips the pattern.
    #[test]
    fn hidden_group_rows_continue_the_stripe() {
        use crate::data::hide_rules::{HideRule, PrCriterion};
        use crate::ui::dashboard::hidden::HiddenGroupView;
        use ratatui::{
            buffer::Buffer,
            layout::Rect,
            widgets::{List, Widget},
        };
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let mut view = make_view(wsx, 1, true);
        view.workspaces.truncate(1);
        let hidden_rows = vec![view.workspaces[0].clone(), view.workspaces[0].clone()];
        view.hidden = vec![HiddenGroupView {
            rule: HideRule::Pr(PrCriterion::Merged),
            expanded: true,
            workspaces: hidden_rows,
        }];
        let list = render_list(&[view], row::ColumnWidths::default(), 0, 120, &theme);
        let rows: usize = list.items.iter().map(|i| i.height()).sum();
        let area = Rect::new(0, 0, 120, rows as u16);
        let mut buf = Buffer::empty(area);
        List::new(list.items).render(area, &mut buf);
        let bg = |y: u16| buf[(119, y)].bg;
        let alt = theme.bg_alt;
        // header + rule, row, group line, hidden row, hidden row, spacer
        assert_ne!(bg(2), alt, "first row plain");
        assert_ne!(bg(3), alt, "group line plain");
        assert_eq!(bg(4), alt, "first hidden row tinted, continuing the count");
        assert_ne!(bg(5), alt, "second hidden row plain");
    }

    /// The selection highlight paints over the stripe, so a selected row
    /// that would be tinted still reads as selected.
    #[test]
    fn selection_highlight_wins_over_the_stripe() {
        use ratatui::{
            buffer::Buffer,
            layout::Rect,
            widgets::{List, ListState, StatefulWidget},
        };
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let views = [make_view(wsx, 1, true)];
        let list = render_list(&views, row::ColumnWidths::default(), 0, 120, &theme);
        let rows: usize = list.items.iter().map(|i| i.height()).sum();
        let area = Rect::new(0, 0, 120, rows as u16);
        let mut buf = Buffer::empty(area);
        let mut state = ListState::default();
        state.select(Some(2)); // second workspace row: a tinted one
        StatefulWidget::render(
            List::new(list.items).highlight_style(theme.selected_bg_style()),
            area,
            &mut buf,
            &mut state,
        );
        // The header item is two rows tall (text + rule), so item N sits at y N+1.
        assert_eq!(buf[(119, 3)].bg, theme.selected_bg, "highlight over stripe");
        assert_eq!(buf[(119, 5)].bg, theme.bg_alt, "other tinted rows keep it");
    }
}
