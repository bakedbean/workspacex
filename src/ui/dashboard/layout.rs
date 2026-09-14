//! Renders the dashboard's top chrome and status strip. The footer
//! (keybinds + sparkline) renders through the bar engine instead; see
//! `crate::ui::bar::dashboard_footer`.

use crate::ui::dashboard::sort::{SortMode, StatusCounts};
use crate::ui::dashboard::status::Status;
use crate::ui::text::{FILTER_ECHO_MAX, truncate};
use crate::ui::theme::{BRAND_ACCENT, BRAND_WORDMARK, Theme};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupMode {
    #[default]
    Repo,
    Attention,
}

/// The dashboard's title row: brand, the `group:` and `sort:` mode tabs, the
/// active filter echo, and the repo/workspace counts.
///
/// Everything after the brand and `group:` tabs is optional, and sheds as the
/// terminal narrows so ratatui never clips the line mid-word. The order is
/// fixed: the `sort:` tabs go first (the mode stays discoverable from `o` and
/// the footer hint), then the counts. The filter echo never sheds — it only
/// shrinks — because a needle with no visible cause is worse than a truncated
/// one: rows are missing from the list and nothing on screen says why.
pub fn top_chrome(
    group: GroupMode,
    sort: SortMode,
    repos: usize,
    workspaces: usize,
    filter: Option<&str>,
    width: usize,
    theme: &Theme,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = vec![
        // Brand cursor block (the site's blinking caret) marks this as the
        // app, not a repo name. The wordmark is two-tone blue — deep
        // "workspace" + bright "x" — so it reads as the brand on every theme
        // rather than borrowing `header_style`, which repo headers also use.
        Span::styled("▌", Style::default().fg(BRAND_ACCENT)),
        Span::raw(" "),
        Span::styled(
            "workspace",
            Style::default()
                .fg(BRAND_WORDMARK)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " x".to_string(),
            Style::default()
                .fg(BRAND_ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · dashboard".to_string(), theme.dim_style()),
        Span::raw(" ".repeat(6)),
        Span::styled("group: ".to_string(), Style::default().fg(theme.path)),
        tab_span("repo", group == GroupMode::Repo, theme),
        Span::raw(" ".to_string()),
        tab_span("attention", group == GroupMode::Attention, theme),
    ];
    let sort_tabs: Vec<Span<'static>> = vec![
        Span::raw("   ".to_string()),
        Span::styled("sort: ".to_string(), Style::default().fg(theme.path)),
        tab_span("recency", sort == SortMode::Recency, theme),
        Span::raw(" ".to_string()),
        tab_span("status", sort == SortMode::Status, theme),
    ];
    let counts = format!("{repos} repos · {workspaces} workspaces");

    let cols = |spans: &[Span<'static>]| -> usize {
        spans.iter().map(|s| s.content.chars().count()).sum()
    };
    let fixed = cols(&spans);
    let sort_cols = cols(&sort_tabs);
    // One blank column minimum before the flush-right counts, so they never
    // run into whatever precedes them.
    let counts_cols = counts.chars().count() + 1;

    // Echo the live needle: without it, `/` looks inert and rows vanishing
    // from the list have no visible cause. The needle is budgeted against
    // the room actually left on this line, not just capped at
    // `FILTER_ECHO_MAX`, so a long needle shrinks instead of pushing the
    // counts off the right edge.
    let echo = filter.map(|needle| {
        // Reserved alongside the needle itself: the 2-space separator and
        // the `/`, plus the counts it must not displace.
        const ECHO_CHROME_W: usize = 3;
        let room = width.saturating_sub(fixed + counts_cols + ECHO_CHROME_W);
        format!("  /{}", truncate(needle, FILTER_ECHO_MAX.min(room)))
    });
    let echo_cols = echo.as_ref().map(|e| e.chars().count()).unwrap_or(0);

    // Decide what fits before emitting anything, since the counts are flush
    // right but outrank the tabs that precede them.
    let show_counts = fixed + echo_cols + counts_cols <= width;
    let counts_reserve = if show_counts { counts_cols } else { 0 };
    if fixed + echo_cols + counts_reserve + sort_cols <= width {
        spans.extend(sort_tabs);
    }
    if let Some(echo) = echo {
        spans.push(Span::styled(
            echo,
            Style::default().fg(theme.warn).add_modifier(Modifier::BOLD),
        ));
    }
    if show_counts {
        let used: usize = cols(&spans);
        let gap = width.saturating_sub(used + counts.chars().count()).max(1);
        spans.push(Span::raw(" ".repeat(gap)));
        spans.push(Span::styled(counts, Style::default().fg(theme.path)));
    }
    Line::from(spans)
}

fn tab_span(label: &'static str, active: bool, theme: &Theme) -> Span<'static> {
    if active {
        Span::styled(
            label.to_string(),
            Style::default()
                .fg(theme.selected_fg)
                .bg(theme.selected_bg)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(label.to_string(), Style::default().fg(theme.path))
    }
}

pub fn status_strip(counts: StatusCounts, theme: &Theme) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let cells = [
        (Status::Question, counts.question),
        (Status::Stalled, counts.stalled),
        (Status::Waiting, counts.waiting),
        (Status::Thinking, counts.thinking),
        (Status::Complete, counts.complete),
        (Status::Idle, counts.idle),
    ];
    for (i, (status, n)) in cells.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("   ".to_string()));
        }
        let zero = *n == 0;
        let value_style = if zero {
            theme.dim_style()
        } else {
            theme.status_style(*status).add_modifier(Modifier::BOLD)
        };
        let label_style = if zero {
            theme.dim_style()
        } else {
            Style::default().fg(theme.path)
        };
        spans.push(Span::styled(status.glyph().to_string(), value_style));
        spans.push(Span::styled(format!(" {n}"), value_style));
        spans.push(Span::styled(format!(" {}", status.label()), label_style));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn top_chrome_shows_app_name_and_counts() {
        let theme = Theme::wsx();
        let line = top_chrome(GroupMode::Repo, SortMode::Recency, 9, 14, None, 100, &theme);
        let t = text(&line);
        assert!(t.starts_with("▌ workspace x · dashboard"), "{t:?}");
        assert!(t.contains("group: "));
        assert!(t.contains("repo"));
        assert!(t.contains("attention"));
        assert!(t.trim_end().ends_with("9 repos · 14 workspaces"), "{t:?}");
    }

    /// Without the echo, `/` gives no feedback and rows disappearing from
    /// the list have no visible cause.
    #[test]
    fn top_chrome_echoes_the_active_filter() {
        let theme = Theme::wsx();
        let line = top_chrome(
            GroupMode::Repo,
            SortMode::Recency,
            9,
            14,
            Some("auth"),
            100,
            &theme,
        );
        assert!(text(&line).contains("/auth"), "{:?}", text(&line));

        // Look for the echo's own prefix rather than a bare `/`, so the
        // assertion tracks the echo and not some unrelated span (wordmark,
        // tab labels, counts) that happens to grow a slash later.
        let bare = top_chrome(GroupMode::Repo, SortMode::Recency, 9, 14, None, 100, &theme);
        assert!(!text(&bare).contains("  /"), "{:?}", text(&bare));
    }

    /// `/` with an empty buffer still echoes, so the keypress registers
    /// before the first character is typed.
    #[test]
    fn top_chrome_echoes_an_empty_filter() {
        let theme = Theme::wsx();
        let line = top_chrome(
            GroupMode::Repo,
            SortMode::Recency,
            9,
            14,
            Some(""),
            100,
            &theme,
        );
        assert!(text(&line).contains('/'), "{:?}", text(&line));
    }

    /// A long needle is truncated so it cannot displace the right-hand
    /// counts. Asserting on the concatenated text alone can't catch this —
    /// the counts span is appended unconditionally, so it "ends with" the
    /// counts at every width, however far the line overflows. The rendered
    /// width is the property that actually matters: anything past `width`
    /// is clipped off-screen by ratatui.
    #[test]
    fn top_chrome_truncates_a_long_filter_and_keeps_counts() {
        let theme = Theme::wsx();
        let needle = "x".repeat(80);
        // 100 has room to spare; 80 forces the echo to shrink well below
        // FILTER_ECHO_MAX to keep the counts on screen.
        for width in [100, 90, 80] {
            let line = top_chrome(
                GroupMode::Repo,
                SortMode::Recency,
                9,
                14,
                Some(&needle),
                width,
                &theme,
            );
            let t = text(&line);
            assert!(
                line.width() <= width,
                "line is {} cols wide at width {width}: {t:?}",
                line.width()
            );
            assert!(t.contains("  /"), "echo present at width {width}: {t:?}");
            assert!(t.contains('…'), "needle truncates at width {width}: {t:?}");
            assert!(
                t.trim_end().ends_with("9 repos · 14 workspaces"),
                "counts kept at width {width}: {t:?}"
            );
        }
    }

    /// A terminal too narrow for chrome + counts is already lost (the base
    /// chrome alone needs 75 cols), but the echo must degrade to nothing
    /// rather than underflow or panic on the way there.
    #[test]
    fn top_chrome_filter_echo_degrades_in_a_tiny_terminal() {
        let theme = Theme::wsx();
        let needle = "x".repeat(80);
        for width in [0, 1, 40, 76] {
            let line = top_chrome(
                GroupMode::Repo,
                SortMode::Recency,
                9,
                14,
                Some(&needle),
                width,
                &theme,
            );
            let t = text(&line);
            assert!(
                !t.contains("/x"),
                "no needle chars survive at width {width}: {t:?}"
            );
        }
    }

    #[test]
    fn top_chrome_names_both_sort_modes() {
        let theme = Theme::wsx();
        let t = text(&top_chrome(
            GroupMode::Repo,
            SortMode::Recency,
            9,
            14,
            None,
            120,
            &theme,
        ));
        assert!(t.contains("sort: "), "{t:?}");
        assert!(t.contains("recency"), "{t:?}");
        assert!(t.contains("status"), "{t:?}");
    }

    #[test]
    fn top_chrome_never_overflows_a_narrow_terminal() {
        let theme = Theme::wsx();
        for w in [60usize, 80, 100, 120, 160] {
            for filter in [None, Some("auth")] {
                let t = text(&top_chrome(
                    GroupMode::Repo,
                    SortMode::Recency,
                    9,
                    14,
                    filter,
                    w,
                    &theme,
                ));
                assert!(
                    t.chars().count() <= w,
                    "width {w} filter {filter:?} overflowed to {}: {t:?}",
                    t.chars().count()
                );
            }
        }
    }

    #[test]
    fn top_chrome_sheds_the_sort_tabs_before_the_counts() {
        let theme = Theme::wsx();
        let at = |w| {
            text(&top_chrome(
                GroupMode::Repo,
                SortMode::Recency,
                9,
                14,
                None,
                w,
                &theme,
            ))
        };
        // 120 holds everything; 80 holds the counts but not the tabs on top
        // of them. The mode stays reachable via `o` and the footer hint,
        // whereas the counts have no other home on this line.
        assert!(at(120).contains("sort: "), "{:?}", at(120));
        assert!(at(120).contains("9 repos · 14 workspaces"), "{:?}", at(120));
        assert!(!at(80).contains("sort: "), "{:?}", at(80));
        assert!(at(80).contains("9 repos · 14 workspaces"), "{:?}", at(80));
        // The group tabs are load-bearing and survive both.
        assert!(at(80).contains("group: "), "{:?}", at(80));
    }

    #[test]
    fn top_chrome_highlights_the_active_sort_mode() {
        let theme = Theme::wsx();
        // The active tab is the one painted on the selection background;
        // reading the styles is what distinguishes it from the inactive one,
        // since both labels are always present.
        let active_label = |mode: SortMode| -> String {
            top_chrome(GroupMode::Repo, mode, 9, 14, None, 120, &theme)
                .spans
                .iter()
                .filter(|s| s.style.bg == Some(theme.selected_bg))
                .map(|s| s.content.to_string())
                .collect()
        };
        assert!(active_label(SortMode::Recency).contains("recency"));
        assert!(!active_label(SortMode::Recency).contains("status"));
        assert!(active_label(SortMode::Status).contains("status"));
        assert!(!active_label(SortMode::Status).contains("recency"));
    }

    #[test]
    fn status_strip_includes_all_six_cells_with_zero_counts() {
        let theme = Theme::wsx();
        let counts = StatusCounts {
            question: 2,
            stalled: 1,
            waiting: 2,
            thinking: 2,
            complete: 3,
            idle: 4,
        };
        let line = status_strip(counts, &theme);
        let t = text(&line);
        assert!(t.contains("? 2 question"));
        assert!(t.contains("! 1 stalled"));
        assert!(t.contains("… 2 waiting"));
        assert!(t.contains("⠋ 2 thinking"));
        assert!(t.contains("✓ 3 complete"));
        assert!(t.contains("· 4 idle"));
    }

    #[test]
    fn status_strip_renders_zero_cells_in_dim() {
        let theme = Theme::wsx();
        let counts = StatusCounts::default();
        let line = status_strip(counts, &theme);
        let t = text(&line);
        assert!(t.contains("? 0 question"));
        assert!(t.contains("· 0 idle"));
    }
}
