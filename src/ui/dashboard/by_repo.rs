//! By-repo view: renders one section per repo, with a header that pairs the
//! repo name with its path and embeds per-status counts on a horizontal rule,
//! and a nested list of workspace rows underneath when expanded.
//!
//! The header reads `▾ ── name  PR  /path/to/repo  ────  ? 1  ✓ 2    3 ws`:
//! names right-justified to a shared column, then the PR link in a gutter
//! reserved across every repo, then each path left-justified in the column
//! that opens up after it, counts flush-right, and the rule filling between.
//! It is drawn through the bar engine's `[dashboard_repo]` bar (see
//! `docs/superpowers/specs/2026-09-17-dashboard-repo-bar-theming-design.md`);
//! this module computes the cross-repo alignment the engine, which renders
//! one line at a time, cannot.

use crate::config::theme_file::BarSpecs;
use crate::ui::bar::segment::{Hit, SegmentMap};
use crate::ui::bar::{DashboardRepoInputs, FoldState, PrLink, dashboard_repo};
use crate::ui::dashboard::row::{self, RowInputs};
use crate::ui::dashboard::sort::StatusCounts;
use crate::ui::theme::Theme;
use ratatui::text::Line;
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

/// The clickable "my open PRs" link's glyph with nerd fonts on:
/// `nf-oct-git_pull_request`, the same glyph a workspace row uses for an
/// open PR — so the header and the rows below it name the same concept at
/// two scales, the way the shared open-PR green already does.
const PR_LINK_NERD: &str = "\u{f407}";
/// Fallback glyph. Plain text rather than a lookalike symbol — an icon
/// nobody can decode isn't an affordance.
const PR_LINK_PLAIN: &str = "PR";

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

/// The PR link's glyph for the nerd-font setting.
fn link_glyph(nerd_fonts: bool) -> &'static str {
    if nerd_fonts {
        PR_LINK_NERD
    } else {
        PR_LINK_PLAIN
    }
}

/// The glyph the list's PR links draw — the widest among the repos that
/// paint one — or `None` when no repo does. When one does, every repo's
/// `$pr_link` renders, a blank of this glyph's width for the others, so
/// every path starts in the same column; when none does, the segment is
/// absent everywhere and its group drops.
fn list_link_glyph(repos: &[RepoView<'_>]) -> Option<&'static str> {
    repos
        .iter()
        .filter(|v| v.show_pr_link)
        .map(|v| link_glyph(v.nerd_fonts))
        .max_by_key(|g| ratatui::text::Span::raw(*g).width())
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

/// Build a repo header line through the bar engine's `[dashboard_repo]`
/// bar, plus the span of its clickable PR link when one was painted. The
/// span comes from the engine's own hit list, so the paint and the click
/// target can't drift — the same contract `row::pr_chip_hit_span` keeps
/// for workspace rows. `name_width` and `list_glyph` are the cross-repo
/// alignment inputs (`name_align_width`, `list_link_glyph`).
fn header_line(
    view: &RepoView<'_>,
    name_width: usize,
    list_glyph: Option<&str>,
    width: usize,
    theme: &Theme,
    specs: &BarSpecs,
    fleet: &SegmentMap,
) -> (Line<'static>, Option<PrLinkSpan>) {
    let fold = if view.counts.total() == 0 {
        FoldState::Empty
    } else if view.expanded {
        FoldState::Expanded
    } else {
        FoldState::Folded
    };
    // A linked repo draws its own glyph; an unlinked one holds the gutter
    // open with a blank the width of the list's.
    let pr_link = list_glyph.map(|list_glyph| PrLink {
        glyph: if view.show_pr_link {
            link_glyph(view.nerd_fonts)
        } else {
            list_glyph
        },
        linked: view.show_pr_link,
        open: has_open_pr(view),
    });
    let rendered = dashboard_repo(
        specs,
        theme,
        &DashboardRepoInputs {
            fold,
            name: view.name,
            pad_cells: name_width.saturating_sub(view.name.chars().count()),
            path: &view.path,
            pr_link,
            counts: view.counts,
            fleet,
        },
        u16::try_from(width).unwrap_or(u16::MAX),
    );
    let span = rendered
        .hits
        .iter()
        .find(|h| h.hit == Hit::RepoPrs)
        .map(|h| (h.start_col, h.width));
    (rendered.line, span)
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
    specs: &BarSpecs,
    fleet: &SegmentMap,
) -> (Vec<ListItem<'static>>, Vec<RepoPrLinkSpan>) {
    let mut items: Vec<ListItem<'static>> = Vec::new();
    let mut links: Vec<RepoPrLinkSpan> = Vec::new();
    let name_width = name_align_width(repos);
    let list_glyph = list_link_glyph(repos);
    for view in repos {
        let (line, pr_link) = header_line(view, name_width, list_glyph, width, theme, specs, fleet);
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
                review: None,
                unresolved: None,
                nerd_fonts: false,
                name_color: None,
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

    /// `header_line` through the bundled default, with the alignment inputs
    /// computed over `views` the way `render_list` does.
    fn line(
        view: &RepoView<'_>,
        views: &[RepoView<'_>],
        width: usize,
    ) -> (Line<'static>, Option<PrLinkSpan>) {
        let theme = Theme::wsx();
        let specs = crate::config::theme_file::bundled_default(&theme);
        header_line(
            view,
            name_align_width(views),
            list_link_glyph(views),
            width,
            &theme,
            &specs,
            crate::ui::bar::fleet::empty(),
        )
    }

    fn render_list_default(
        repos: &[RepoView<'_>],
        width: usize,
    ) -> (Vec<ListItem<'static>>, Vec<RepoPrLinkSpan>) {
        let theme = Theme::wsx();
        let specs = crate::config::theme_file::bundled_default(&theme);
        render_list(
            repos,
            row::ColumnWidths::default(),
            0,
            width,
            &theme,
            &specs,
            crate::ui::bar::fleet::empty(),
        )
    }

    #[test]
    fn header_shows_fold_glyph_and_counts() {
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = make_view(wsx, 1, true);
        let (line, _) = line(&view, std::slice::from_ref(&view), 120);
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
        let repos = fixture::repos();
        let frontend = repos.iter().find(|r| r.name == "frontend").unwrap();
        let view = make_view(frontend, 2, false);
        let (line, _) = line(&view, std::slice::from_ref(&view), 120);
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
        let width = 120;
        let repos = fixture::repos();
        // Two repos with different name lengths and different path lengths.
        let short = repos.iter().find(|r| r.name == "wsx").unwrap();
        let long = repos.iter().find(|r| r.name == "scp-admin").unwrap();
        let views = [make_view(short, 1, true), make_view(long, 2, false)];

        let (short_line, _) = line(&views[0], &views, width);
        let (long_line, _) = line(&views[1], &views, width);

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
            assert!(
                list_link_glyph(&views).is_some(),
                "a linked repo in the list opens a gutter"
            );

            let (linked_line, span) = line(&views[0], &views, 120);
            let (bare_line, none) = line(&views[1], &views, 120);
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
        // Once the line fits, it is exactly `width` wide and the counts end
        // at the right edge, at every width; below that the engine omits
        // the right side (rather than pushing the counts past the edge)
        // and the left side stays at its own minimum. Swept with and
        // without the PR link, which adds cells left of the rule.
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        for (label, view) in [
            ("no link", make_view(wsx, 1, true)),
            ("plain link", pr_link_view(wsx, true, false)),
            ("nerd link", pr_link_view(wsx, true, true)),
        ] {
            let views = std::slice::from_ref(&view);
            let left_min = header_text(&line(&view, views, 0).0).chars().count();
            let mut fits_from = None;
            for width in 0..=200 {
                let l = line(&view, views, width).0;
                let t = header_text(&l);
                let len = t.chars().count();
                if t.contains("4 ws") {
                    fits_from.get_or_insert(width);
                    assert_eq!(
                        len, width,
                        "{label} width={width}: exactly `width` once it fits"
                    );
                    assert_eq!(substr_end_col(&l, "4 ws"), width, "{label} width={width}");
                } else {
                    assert!(
                        fits_from.is_none(),
                        "{label} width={width}: counts vanished after fitting"
                    );
                    assert_eq!(len, width.max(left_min), "{label} width={width}");
                }
            }
            assert!(
                fits_from.is_some(),
                "{label}: counts never fit by 200 columns"
            );
        }
    }

    #[test]
    fn short_names_get_a_left_fill_rule() {
        let repos = fixture::repos();
        let short = repos.iter().find(|r| r.name == "wsx").unwrap();
        let long = repos.iter().find(|r| r.name == "scp-admin").unwrap();
        let views = [make_view(short, 1, true), make_view(long, 2, true)];

        // The shorter name's left-pad is filled with a rule (one space before
        // the name), matching the pinned-command row's filler.
        let short_t = header_text(&line(&views[0], &views, 120).0);
        assert!(short_t.contains("─ wsx"), "left-fill rule: {short_t:?}");

        // The widest name has no left pad, so it hugs the glyph — no rule.
        let long_t = header_text(&line(&views[1], &views, 120).0);
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
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = pr_link_view(wsx, true, false);
        let (line, span) = line(&view, std::slice::from_ref(&view), 120);
        let span = span.expect("github repo gets a clickable PR link");
        assert_eq!(span_text(&line, span), PR_LINK_PLAIN);
    }

    #[test]
    fn non_github_repo_header_has_no_pr_link() {
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = pr_link_view(wsx, false, false);
        let (line, span) = line(&view, std::slice::from_ref(&view), 120);
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
    fn pr_link_style(view: &RepoView<'_>) -> ratatui::style::Style {
        let (l, _) = line(view, std::slice::from_ref(view), 120);
        style_of(&l, PR_LINK_PLAIN)
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
        let (line, _) = line(&view, std::slice::from_ref(&view), 120);
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
                pr_link_style(&view),
                green,
                "{lc:?} is open on GitHub, so the link should be green"
            );
        }

        // States that no longer appear in that list leave it dim.
        for lc in [NoPr, PrMerged, PrClosed] {
            let mut view = pr_link_view(wsx, true, false);
            view.workspaces[0].lifecycle = Some(lc);
            assert_eq!(
                pr_link_style(&view),
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
        assert_eq!(pr_link_style(&view), green, "expanded repo");

        // Folding hides the rows but must not hide the signal.
        view.expanded = false;
        assert_eq!(pr_link_style(&view), green, "folded repo");
    }

    #[test]
    fn nerd_fonts_swap_the_pr_link_glyph() {
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = pr_link_view(wsx, true, true);
        let (line, span) = line(&view, std::slice::from_ref(&view), 120);
        let span = span.expect("github repo gets a clickable PR link");
        assert_eq!(span_text(&line, span), PR_LINK_NERD);
    }

    /// The header link deliberately reuses the glyph a workspace row shows
    /// for an open PR, so the two name the same concept at two scales —
    /// mirroring the open-PR green they already share. Pinned here because
    /// the two live in different modules and are easy to drift apart.
    #[test]
    fn pr_link_glyph_matches_the_row_open_pr_glyph() {
        use crate::git::forge::BranchLifecycle::PrOpen;
        assert_eq!(
            PR_LINK_NERD,
            crate::ui::theme::branch_glyph(Some(PrOpen), true)
        );
    }

    /// The span must slice exactly the glyph out of the painted line — no
    /// leading separator, no trailing filler — so a click on blank space
    /// can't open a browser.
    #[test]
    fn pr_link_span_slices_exactly_the_glyph_at_every_width() {
        let repos = fixture::repos();
        for r in &repos {
            for nerd_fonts in [false, true] {
                let view = pr_link_view(r, true, nerd_fonts);
                let glyph = if nerd_fonts {
                    PR_LINK_NERD
                } else {
                    PR_LINK_PLAIN
                };
                for width in 0..=200 {
                    let (line, span) = line(&view, std::slice::from_ref(&view), width);
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
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let frontend = repos.iter().find(|r| r.name == "frontend").unwrap();
        // wsx is expanded with 4 workspaces (header + 4 rows + spacer = 6
        // items), so the second repo's header lands at flat index 6.
        let mut first = pr_link_view(wsx, true, false);
        first.id = 1;
        let mut second = make_view(frontend, 2, false);
        second.show_pr_link = true;
        let (_, links) = render_list_default(&[first, second], 120);
        let indices: Vec<(u64, usize)> = links.iter().map(|(id, idx, _)| (*id, *idx)).collect();
        assert_eq!(indices, vec![(1, 0), (2, 6)]);
    }

    #[test]
    fn render_list_omits_links_for_non_github_repos() {
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = make_view(wsx, 1, true); // show_pr_link defaults to false
        let (_, links) = render_list_default(&[view], 120);
        assert!(links.is_empty());
    }

    #[test]
    fn collapsed_repo_emits_no_rows() {
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = make_view(wsx, 1, false);
        let (items, _) = render_list_default(&[view], 120);
        assert_eq!(items.len(), 1, "only the header for a collapsed repo");
    }

    #[test]
    fn expanded_repo_emits_header_then_rows_then_blank() {
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        let view = make_view(wsx, 1, true);
        let (items, _) = render_list_default(&[view], 120);
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
