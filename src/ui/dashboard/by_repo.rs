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
    /// Whether to paint the clickable "my open PRs" link on the header.
    /// Gated on the repo having a github.com remote — a repo GitHub can't
    /// serve gets no affordance rather than one that opens a dead tab.
    pub show_pr_link: bool,
    /// Per-view copy of the global nerd-fonts setting, mirroring
    /// `RowInputs::nerd_fonts`; selects the PR link's glyph.
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

/// Spaces flanking the filler rule on each side.
const RULE_PAD: usize = 2;

/// The clickable "my open PRs" link's glyph with nerd fonts on: the
/// git-pull-request octicon, the same one `branch_glyph` uses for an open
/// PR, so the two read as the same concept at two scales.
const PR_LINK_NERD: &str = "\u{f407}";
/// Fallback glyph. Plain text rather than a lookalike symbol — an icon
/// nobody can decode isn't an affordance.
const PR_LINK_PLAIN: &str = "PR";
/// Blank columns between the counts (or name) and the PR link. Excluded
/// from the hit span so a click on the gap doesn't open a browser.
const PR_LINK_PAD: usize = 2;

/// A repo header's clickable PR link: `(char offset in the line, width)`.
type PrLinkSpan = (u16, u16);

/// The PR link's glyph for a view, or `None` when it shouldn't be painted.
fn pr_link_glyph(view: &RepoView<'_>) -> Option<&'static str> {
    if !view.show_pr_link {
        return None;
    }
    Some(if view.nerd_fonts {
        PR_LINK_NERD
    } else {
        PR_LINK_PLAIN
    })
}

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

/// Build a repo header line, plus the span of its clickable PR link when
/// one was painted. The span is derived from the spans actually pushed, so
/// the paint and the click target can't drift — the same contract
/// `row::pr_chip_hit_span` keeps for workspace rows.
fn header_line(
    view: &RepoView<'_>,
    name_width: usize,
    width: usize,
    theme: &Theme,
) -> (Line<'static>, Option<PrLinkSpan>) {
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

    // The PR link closes the repo-identity cluster (name, counts, link),
    // before the filler rule hands the line over to the path.
    let pr_link = pr_link_glyph(view).map(|glyph| {
        // Measured in terminal cells, not Unicode scalars: mouse columns
        // and ratatui's layout both count cells, so a double-width repo
        // name would otherwise slide the painted glyph out from under its
        // click rect.
        let offset: usize = spans.iter().map(|s| s.width()).sum();
        spans.push(Span::raw(" ".repeat(PR_LINK_PAD)));
        // Open-PR colour, not the dim of the counts beside it: this names
        // the same thing a row's PR chip does, and a click target that
        // blends into its neighbours goes unnoticed.
        let style = theme
            .lifecycle_style(Some(crate::git::forge::BranchLifecycle::PrOpen))
            .unwrap_or_else(|| theme.dim_style());
        let glyph = Span::styled(glyph.to_string(), style);
        let glyph_width = glyph.width();
        spans.push(glyph);
        ((offset + PR_LINK_PAD) as u16, glyph_width as u16)
    });

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
    (Line::from(spans), pr_link)
}

/// A repo header's clickable PR link, positioned by flat list index:
/// `(repo id, flat item index, span)`. Mirrors the workspace rows'
/// `PrChipSpan`, which the caller resolves to screen rects the same way.
pub type RepoPrLinkSpan = (u64, usize, PrLinkSpan);

/// Emit the full sequence of `ListItem`s for the by-repo view, plus the PR
/// link span of every header that painted one.
pub fn render_list(
    repos: &[RepoView<'_>],
    widths: row::ColumnWidths,
    tick: u32,
    width: usize,
    theme: &Theme,
) -> (Vec<ListItem<'static>>, Vec<RepoPrLinkSpan>) {
    let mut items: Vec<ListItem<'static>> = Vec::new();
    let mut links: Vec<RepoPrLinkSpan> = Vec::new();
    let name_width = name_align_width(repos);
    for view in repos {
        let (line, pr_link) = header_line(view, name_width, width, theme);
        if let Some(span) = pr_link {
            links.push((view.id, items.len(), span));
        }
        items.push(ListItem::new(line));
        if !view.expanded {
            continue;
        }
        for w in &view.workspaces {
            items.push(ListItem::new(row::render(w, widths, tick, theme, width)));
        }
        items.push(ListItem::new(""));
    }
    (items, links)
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
                selected: i == 0,
                yolo: false,
                badge: None,
                undelivered_mail: false,
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
            show_pr_link: false,
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

        let (short_line, _) = header_line(&views[0], name_width, width, &theme);
        let (long_line, _) = header_line(&views[1], name_width, width, &theme);

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
        let short_t = header_text(&header_line(&views[0], name_width, 120, &theme).0);
        assert!(short_t.contains("─ wsx"), "left-fill rule: {short_t:?}");

        // The widest name has no left pad, so it hugs the glyph — no rule.
        let long_t = header_text(&header_line(&views[1], name_width, 120, &theme).0);
        assert!(long_t.starts_with("▾ scp-admin"), "no rule: {long_t:?}");
    }

    /// The header text sliced by a hit span, as the click target sees it.
    fn span_text(line: &Line<'_>, span: (u16, u16)) -> String {
        header_text(line)
            .chars()
            .skip(span.0 as usize)
            .take(span.1 as usize)
            .collect()
    }

    fn pr_link_view<'a>(
        r: &'a fixture::FixtureRepo,
        show_pr_link: bool,
        nerd_fonts: bool,
    ) -> RepoView<'a> {
        let mut view = make_view(r, 1, true);
        view.show_pr_link = show_pr_link;
        view.nerd_fonts = nerd_fonts;
        view
    }

    #[test]
    fn github_repo_header_carries_a_pr_link() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = pr_link_view(wsx, true, false);
        let (line, span) = header_line(
            &view,
            name_align_width(std::slice::from_ref(&view)),
            120,
            &theme,
        );
        let span = span.expect("github repo gets a clickable PR link");
        assert_eq!(span_text(&line, span), PR_LINK_PLAIN);
    }

    #[test]
    fn non_github_repo_header_has_no_pr_link() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = pr_link_view(wsx, false, false);
        let (line, span) = header_line(
            &view,
            name_align_width(std::slice::from_ref(&view)),
            120,
            &theme,
        );
        assert!(span.is_none(), "no click target without a GitHub remote");
        assert!(
            !header_text(&line).contains(PR_LINK_PLAIN),
            "and no glyph either: {:?}",
            header_text(&line)
        );
    }

    /// The link carries the open-PR colour rather than the dim of the
    /// counts beside it: it names the same concept as a row's PR chip, and
    /// a click target that blends into its neighbours isn't discoverable.
    #[test]
    fn pr_link_is_styled_like_an_open_pr() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = pr_link_view(wsx, true, false);
        let (line, _) = header_line(
            &view,
            name_align_width(std::slice::from_ref(&view)),
            120,
            &theme,
        );
        let link = line
            .spans
            .iter()
            .find(|s| s.content == PR_LINK_PLAIN)
            .expect("link span painted");
        assert_eq!(
            link.style,
            theme
                .lifecycle_style(Some(crate::git::forge::BranchLifecycle::PrOpen))
                .expect("open PRs have a colour")
        );
    }

    #[test]
    fn nerd_fonts_swap_the_pr_link_glyph() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = pr_link_view(wsx, true, true);
        let (line, span) = header_line(
            &view,
            name_align_width(std::slice::from_ref(&view)),
            120,
            &theme,
        );
        let span = span.expect("github repo gets a clickable PR link");
        assert_eq!(span_text(&line, span), PR_LINK_NERD);
    }

    /// The span must slice exactly the glyph out of the painted line — no
    /// leading separator, no trailing filler — so a click on blank space
    /// can't open a browser.
    #[test]
    fn pr_link_span_slices_exactly_the_glyph_at_every_width() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        for r in &repos {
            for nerd_fonts in [false, true] {
                let view = pr_link_view(r, true, nerd_fonts);
                let name_width = name_align_width(std::slice::from_ref(&view));
                let glyph = if nerd_fonts {
                    PR_LINK_NERD
                } else {
                    PR_LINK_PLAIN
                };
                for width in 0..=200 {
                    let (line, span) = header_line(&view, name_width, width, &theme);
                    let span = span.expect("link present regardless of width");
                    assert_eq!(
                        span_text(&line, span),
                        glyph,
                        "repo={} nerd_fonts={nerd_fonts} width={width}",
                        r.name
                    );
                }
            }
        }
    }

    #[test]
    fn render_list_reports_each_pr_link_at_its_flat_index() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let frontend = repos.iter().find(|r| r.name == "frontend").unwrap();
        // wsx is expanded with 4 workspaces (header + 4 rows + spacer = 6
        // items), so the second repo's header lands at flat index 6.
        let mut first = pr_link_view(wsx, true, false);
        first.id = 1;
        let mut second = make_view(frontend, 2, false);
        second.show_pr_link = true;
        let (_, links) = render_list(
            &[first, second],
            row::ColumnWidths::default(),
            0,
            120,
            &theme,
        );
        let indices: Vec<(u64, usize)> = links.iter().map(|(id, idx, _)| (*id, *idx)).collect();
        assert_eq!(indices, vec![(1, 0), (2, 6)]);
    }

    #[test]
    fn render_list_omits_links_for_non_github_repos() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = make_view(wsx, 1, true); // show_pr_link defaults to false
        let (_, links) = render_list(&[view], row::ColumnWidths::default(), 0, 120, &theme);
        assert!(links.is_empty());
    }

    #[test]
    fn collapsed_repo_emits_no_rows() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = make_view(wsx, 1, false);
        let (items, _) = render_list(&[view], row::ColumnWidths::default(), 0, 120, &theme);
        assert_eq!(items.len(), 1, "only the header for a collapsed repo");
    }

    #[test]
    fn expanded_repo_emits_header_then_rows_then_blank() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = make_view(wsx, 1, true);
        let (items, _) = render_list(&[view], row::ColumnWidths::default(), 0, 120, &theme);
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
        let names: Vec<&str> = view.workspaces.iter().map(|w| w.branch.as_str()).collect();
        assert_eq!(names[0], "bakedbean/theme-tokens", "stalled first");
        assert_eq!(names[1], "bakedbean/repo-overview", "question second");
    }
}
