//! By-repo view: renders one section per repo, with a header that pairs the
//! repo name with its path and embeds per-status counts on a horizontal rule,
//! and a nested list of workspace rows underneath when expanded.
//!
//! The header reads `▾ ── name  PR  /path/to/repo  ────  ? 1  ✓ 2    3 ws`:
//! names right-justified to a shared column, then the PR link in a gutter
//! reserved across every repo, then each path left-justified in the column
//! that opens up after it, counts flush-right, and the rule filling between.

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

/// The clickable "my open PRs" link's glyph with nerd fonts on:
/// `nf-oct-git_merge_queue`. It reads as a queue of pull requests, which is
/// what the link opens and what the `PR` fallback below says in words.
const PR_LINK_NERD: &str = "\u{f4db}";
/// Fallback glyph. Plain text rather than a lookalike symbol — an icon
/// nobody can decode isn't an affordance.
const PR_LINK_PLAIN: &str = "PR";
/// Blank columns between the PR link and the path that follows it. Excluded
/// from the hit span so a click on the gap doesn't open a browser.
const PR_LINK_PAD: usize = 2;

/// A repo header's clickable PR link: `(char offset in the line, width)`.
type PrLinkSpan = (u16, u16);

/// Whether any of a repo's workspaces has a pull request that GitHub still
/// counts as open — including drafts and conflicted ones, since both are
/// listed by the `is:pr is:open author:@me` query the link opens. So the
/// colour predicts whether the link leads anywhere; merged and closed PRs
/// have dropped out of that list and leave it dim.
fn has_open_pr(view: &RepoView<'_>) -> bool {
    use crate::git::forge::BranchLifecycle::*;
    view.workspaces
        .iter()
        .any(|w| matches!(w.lifecycle, Some(PrOpen | PrDraft | PrConflicted)))
}

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

/// Columns reserved between the name and the path for the PR link, so that
/// every path starts in the same column whether or not its repo has one —
/// the alignment the left-justified path column depends on. Zero when no
/// repo in the list has a link, so a non-GitHub setup pays nothing for a
/// gutter it would never fill.
fn pr_link_gutter(repos: &[RepoView<'_>]) -> usize {
    repos
        .iter()
        .filter_map(pr_link_glyph)
        .map(|glyph| Span::raw(glyph).width())
        .max()
        .map(|w| w + PR_LINK_PAD)
        .unwrap_or(0)
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
    gutter: usize,
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

    // Then the PR link, then the path. The link takes the same dim as the path
    // it introduces: the two read as one quiet cluster identifying the repo,
    // rather than the link competing with the status counts for attention.
    //
    // It occupies a gutter reserved across every repo, so a repo without a
    // link leaves those columns blank instead of sliding its path left out of
    // the shared column. Because the names are right-justified to a shared
    // column and the gutter is a constant, every path starts in the same
    // column for free — no second alignment pass needed.
    spans.push(Span::raw("  ".to_string()));
    let pr_link = pr_link_glyph(view).map(|glyph| {
        // Measured in terminal cells, not Unicode scalars: mouse columns
        // and ratatui's layout both count cells, so a double-width repo
        // name would otherwise slide the painted glyph out from under its
        // click rect.
        let offset: usize = spans.iter().map(|s| s.width()).sum();
        // Dim by default so the link and the path read as one quiet cluster,
        // but lit in the open-PR green — the same one a row's PR chip takes —
        // when this repo actually has something waiting behind it.
        let style = if has_open_pr(view) {
            theme
                .lifecycle_style(Some(crate::git::forge::BranchLifecycle::PrOpen))
                .unwrap_or_else(|| theme.dim_style())
        } else {
            theme.dim_style()
        };
        let glyph = Span::styled(glyph.to_string(), style);
        let glyph_width = glyph.width();
        spans.push(glyph);
        // Pad out the rest of the gutter, keeping the pad outside the hit
        // span so a click on the gap doesn't open a browser.
        spans.push(Span::raw(" ".repeat(gutter.saturating_sub(glyph_width))));
        (offset as u16, glyph_width as u16)
    });
    if pr_link.is_none() {
        spans.push(Span::raw(" ".repeat(gutter)));
    }
    spans.push(Span::styled(view.path.to_string(), theme.dim_style()));

    // Status counts are flush-right, built separately so the rule between the
    // path and the counts can be sized from the gap they leave. Empty repos
    // show nothing — the absence of workspace rows is self-explanatory, no
    // label needed — and the rule then runs to the right edge on its own.
    let mut right: Vec<Span<'static>> = Vec::new();
    if view.counts.total() > 0 {
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
        right.push(Span::raw("    ".to_string()));
        right.push(Span::styled(
            format!("{} ws", view.counts.total()),
            theme.dim_style(),
        ));
    }

    // The rule fills the gap between the path and the flush-right counts,
    // flanked by RULE_PAD spaces. Size it from the *actual* gap so the counts'
    // right edge lands exactly at `width` — never force a minimum rule, which
    // would push the line one column past `width` and clip them. With no counts
    // there is nothing to separate on the right, so the trailing pad is dropped
    // and the rule runs to the edge. When the gap is too small for a padded
    // rule, fall back to plain spaces; if the left content plus the counts
    // already overflow, the gap is zero.
    let trail = if right.is_empty() { 0 } else { RULE_PAD };
    let used_left: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let used_right: usize = right.iter().map(|s| s.content.chars().count()).sum();
    let gap = width.saturating_sub(used_left + used_right);
    if gap > RULE_PAD + trail {
        let rule = "─".repeat(gap - RULE_PAD - trail);
        spans.push(Span::raw(" ".repeat(RULE_PAD)));
        spans.push(Span::styled(rule, theme.dim_style()));
        if trail > 0 {
            spans.push(Span::raw(" ".repeat(trail)));
        }
    } else {
        spans.push(Span::raw(" ".repeat(gap)));
    }
    spans.extend(right);
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
    let gutter = pr_link_gutter(repos);
    for view in repos {
        let (line, pr_link) = header_line(view, name_width, gutter, width, theme);
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
        let gutter = pr_link_gutter(std::slice::from_ref(&view));
        let (line, _) = header_line(&view, align, gutter, 120, &theme);
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
        let gutter = pr_link_gutter(std::slice::from_ref(&view));
        let (line, _) = header_line(&view, align, gutter, 120, &theme);
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
        // Path still follows the name, and with no counts to separate on the
        // right the rule runs all the way to the edge.
        assert!(
            t.starts_with("  frontend  /home/eben/meals/frontend  ─"),
            "path then rule to the edge: {t:?}"
        );
        assert!(t.ends_with('─'), "no trailing pad without counts: {t:?}");
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
        let gutter = pr_link_gutter(&views);

        let (short_line, _) = header_line(&views[0], name_width, gutter, width, &theme);
        let (long_line, _) = header_line(&views[1], name_width, gutter, width, &theme);

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

    /// The PR link sits between the name and the path, so a repo without one
    /// has to hold those columns open — otherwise a non-GitHub repo's path
    /// slides left out of the shared column and the alignment above breaks.
    #[test]
    fn paths_align_whether_or_not_a_repo_has_a_pr_link() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let admin = repos.iter().find(|r| r.name == "scp-admin").unwrap();
        for nerd_fonts in [false, true] {
            // Same name length either side, so only the link can move the path.
            let mut linked = pr_link_view(wsx, true, nerd_fonts);
            linked.name = "aaa";
            let mut bare = make_view(admin, 2, false);
            bare.name = "bbb";
            let views = [linked, bare];
            let name_width = name_align_width(&views);
            let gutter = pr_link_gutter(&views);
            assert!(gutter > 0, "a linked repo in the list opens a gutter");

            let (linked_line, span) = header_line(&views[0], name_width, gutter, 120, &theme);
            let (bare_line, none) = header_line(&views[1], name_width, gutter, 120, &theme);
            assert!(none.is_none(), "no link, no click target");
            assert_eq!(
                substr_start_col(&linked_line, &views[0].path),
                substr_start_col(&bare_line, &views[1].path),
                "the gutter must hold the path column open (nerd_fonts={nerd_fonts})"
            );
            // And the reserved columns are blank on the bare header, not
            // silently swallowed by shifting the path.
            let span = span.expect("linked repo gets a click target");
            assert_eq!(
                span_text(&linked_line, span),
                if nerd_fonts {
                    PR_LINK_NERD
                } else {
                    PR_LINK_PLAIN
                },
                "hit span lands on the glyph (nerd_fonts={nerd_fonts})"
            );
        }
    }

    #[test]
    fn counts_stay_flush_right_without_overflow() {
        // Across every width, the rendered line is exactly `width` once the
        // content fits, and never longer (which would clip the flush-right
        // counts). Below the fit threshold it stays pinned at the minimum
        // content width. Regression for forcing a >=1 rule that overshot by one
        // column at the boundary.
        //
        // Swept with and without the PR link, since the link adds columns to
        // the left of the rule and so has to be absorbed by the gap: a link
        // left out of the sizing would push the counts past the right edge.
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        for (label, view) in [
            ("no link", make_view(wsx, 1, true)),
            ("plain link", pr_link_view(wsx, true, false)),
            ("nerd link", pr_link_view(wsx, true, true)),
        ] {
            let name_width = name_align_width(std::slice::from_ref(&view));
            let gutter = pr_link_gutter(std::slice::from_ref(&view));
            // Minimum content width = the line with a zero gap (width 0).
            let min_content = header_text(&header_line(&view, name_width, gutter, 0, &theme).0)
                .chars()
                .count();
            for width in 0..=200 {
                let (line, _) = header_line(&view, name_width, gutter, width, &theme);
                let len = header_text(&line).chars().count();
                assert_eq!(
                    len,
                    width.max(min_content),
                    "line width must be exactly `width` when it fits (never +1): \
                     {label} width={width}"
                );
                if width >= min_content {
                    assert_eq!(
                        substr_end_col(&line, "4 ws"),
                        width,
                        "counts stay flush to the right edge: {label} width={width}"
                    );
                }
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
        let gutter = pr_link_gutter(&views);

        // The shorter name's left-pad is filled with a rule (one space before
        // the name), matching the pinned-command row's filler.
        let short_t = header_text(&header_line(&views[0], name_width, gutter, 120, &theme).0);
        assert!(short_t.contains("─ wsx"), "left-fill rule: {short_t:?}");

        // The widest name has no left pad, so it hugs the glyph — no rule.
        let long_t = header_text(&header_line(&views[1], name_width, gutter, 120, &theme).0);
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
            pr_link_gutter(std::slice::from_ref(&view)),
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
            pr_link_gutter(std::slice::from_ref(&view)),
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

    /// Style of the first span whose content is exactly `needle`.
    fn style_of(line: &Line<'_>, needle: &str) -> ratatui::style::Style {
        line.spans
            .iter()
            .find(|s| s.content == needle)
            .unwrap_or_else(|| panic!("{needle} span painted"))
            .style
    }

    /// Render `view`'s header and return the PR link's style.
    fn pr_link_style(view: &RepoView<'_>, theme: &Theme) -> ratatui::style::Style {
        let (line, _) = header_line(
            view,
            name_align_width(std::slice::from_ref(view)),
            pr_link_gutter(std::slice::from_ref(view)),
            120,
            theme,
        );
        style_of(&line, PR_LINK_PLAIN)
    }

    /// With nothing open behind it the link takes the same dim as the path it
    /// introduces, so the two read as one quiet cluster identifying the repo
    /// rather than the link competing with the status counts for attention.
    #[test]
    fn pr_link_without_open_prs_is_dimmed_like_the_path() {
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = pr_link_view(wsx, true, false);
        let (line, _) = header_line(
            &view,
            name_align_width(std::slice::from_ref(&view)),
            pr_link_gutter(std::slice::from_ref(&view)),
            120,
            &theme,
        );
        assert_eq!(style_of(&line, PR_LINK_PLAIN), theme.dim_style());
        // Not merely equal to a constant — equal to the path beside it.
        assert_eq!(style_of(&line, PR_LINK_PLAIN), style_of(&line, &view.path));
    }

    /// A repo with something waiting behind the link lights it up, in the same
    /// green a workspace row's open-PR chip uses.
    #[test]
    fn pr_link_goes_green_when_a_workspace_has_an_open_pr() {
        use crate::git::forge::BranchLifecycle::*;
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let green = theme.lifecycle_style(Some(PrOpen)).expect("open-PR style");
        assert_ne!(green, theme.dim_style(), "fixture theme must distinguish");

        // Every state still listed by `is:pr is:open author:@me` lights it up.
        for lc in [PrOpen, PrDraft, PrConflicted] {
            let mut view = pr_link_view(wsx, true, false);
            view.workspaces[0].lifecycle = Some(lc);
            assert_eq!(
                pr_link_style(&view, &theme),
                green,
                "{lc:?} is open on GitHub, so the link should be green"
            );
        }

        // States that no longer appear in that list leave it dim.
        for lc in [NoPr, PrMerged, PrClosed] {
            let mut view = pr_link_view(wsx, true, false);
            view.workspaces[0].lifecycle = Some(lc);
            assert_eq!(
                pr_link_style(&view, &theme),
                theme.dim_style(),
                "{lc:?} is not open, so the link should stay dim"
            );
        }
    }

    /// The signal is about the repo, not the selected row: any one workspace
    /// with an open PR is enough, and a folded repo still reports it.
    #[test]
    fn any_single_workspace_with_an_open_pr_lights_the_link() {
        use crate::git::forge::BranchLifecycle::*;
        let theme = Theme::wsx();
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let green = theme.lifecycle_style(Some(PrOpen)).expect("open-PR style");

        let mut view = pr_link_view(wsx, true, false);
        // Bury the only open PR at the end, behind several closed ones.
        for w in view.workspaces.iter_mut() {
            w.lifecycle = Some(PrClosed);
        }
        *view.workspaces.last_mut().unwrap() = {
            let mut w = view.workspaces.last().unwrap().clone();
            w.lifecycle = Some(PrOpen);
            w
        };
        assert_eq!(pr_link_style(&view, &theme), green, "expanded repo");

        // Folding hides the rows but must not hide the signal.
        view.expanded = false;
        assert_eq!(pr_link_style(&view, &theme), green, "folded repo");
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
            pr_link_gutter(std::slice::from_ref(&view)),
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
                let gutter = pr_link_gutter(std::slice::from_ref(&view));
                let glyph = if nerd_fonts {
                    PR_LINK_NERD
                } else {
                    PR_LINK_PLAIN
                };
                for width in 0..=200 {
                    let (line, span) = header_line(&view, name_width, gutter, width, &theme);
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
