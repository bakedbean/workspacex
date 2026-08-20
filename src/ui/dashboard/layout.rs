//! Renders the three chrome bars around the V5 dashboard list:
//! top chrome, status strip, footer (keybinds + sparkline).

use crate::ui::dashboard::sort::{SortMode, StatusCounts};
use crate::ui::dashboard::sparkline;
use crate::ui::dashboard::status::Status;
use crate::ui::footer::{FooterHintAction, FooterHintSpan, key_for_glyph};
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

pub fn footer(
    activity_samples: &[u32],
    version: &str,
    width: usize,
    theme: &Theme,
    window_label: &str,
    workspace_selected: bool,
) -> (Line<'static>, u16, Vec<FooterHintSpan>) {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut keys: Vec<(&str, &str)> = vec![
        ("↑↓", "nav"),
        ("↵", "open"),
        ("n", "new"),
        ("G", "group"),
        ("o", "order"),
        ("/", "filter"),
    ];
    if workspace_selected {
        keys.push(("?", "actions"));
    }
    keys.push(("q", "quit"));
    let key_style = Style::default()
        .fg(theme.dim)
        .add_modifier(Modifier::BOLD)
        .bg(theme.bg_soft);
    let label_style = Style::default().fg(theme.path);
    let pad_style = theme.chip_bg_style();
    // Pill wraps only the key glyph (` key `); the label is plain text on
    // the bar bg, with a single leading space separating it from the pill.
    // `col` tracks the running column so each pill+label run can be recorded
    // as a clickable hint (offsets relative to the line start).
    let mut hints: Vec<FooterHintSpan> = Vec::new();
    let mut col: u16 = 0;
    let push = |spans: &mut Vec<Span<'static>>, col: &mut u16, span: Span<'static>| {
        *col += span.content.chars().count() as u16;
        spans.push(span);
    };
    for (i, (key, label)) in keys.iter().enumerate() {
        if i > 0 {
            push(&mut spans, &mut col, Span::raw("  ".to_string()));
        }
        let start = col;
        push(
            &mut spans,
            &mut col,
            Span::styled(" ".to_string(), pad_style),
        );
        push(
            &mut spans,
            &mut col,
            Span::styled((*key).to_string(), key_style),
        );
        push(
            &mut spans,
            &mut col,
            Span::styled(" ".to_string(), pad_style),
        );
        push(
            &mut spans,
            &mut col,
            Span::styled(format!(" {label}"), label_style),
        );
        if let Some(key_event) = key_for_glyph(key) {
            hints.push(FooterHintSpan {
                start_col: start,
                width: col - start,
                action: FooterHintAction::Key(key_event),
            });
        }
    }

    let spark = sparkline::render(activity_samples, 24);
    let right = format!("{version}  {window_label} {spark}");
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let gap = width.saturating_sub(used + right.chars().count()).max(1);
    spans.push(Span::raw(" ".repeat(gap)));
    spans.push(Span::styled(right, Style::default().fg(theme.path)));
    // The clickable graph is the trailing "<label> <24-char sparkline>" run.
    let graph_w = (window_label.chars().count() + 1 + 24) as u16;
    (Line::from(spans), graph_w, hints)
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
    fn footer_offers_the_order_key() {
        let theme = Theme::wsx();
        let (line, _, hints) = footer(&[1, 2, 3], "0.1.0", 200, &theme, "24h", false);
        let t = text(&line);
        assert!(t.contains(" order"), "order label present: {t:?}");
        let order = hints
            .iter()
            .find(|h| h.action == FooterHintAction::Key(key_for_glyph("o").unwrap()))
            .expect("order hint present");
        let cells: Vec<char> = t.chars().collect();
        assert_eq!(
            cells[order.start_col as usize..(order.start_col + order.width) as usize]
                .iter()
                .collect::<String>(),
            " o  order"
        );
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

    #[test]
    fn footer_includes_keybinds_and_sparkline() {
        let theme = Theme::wsx();
        let samples = vec![1, 2, 3, 4, 5];
        let (line, _, _) = footer(&samples, "v0.5.0", 200, &theme, "24h", true);
        let t = text(&line);
        // After the V5 pill treatment, key and label are separated by the
        // pill's trailing pad + the label's leading space (2 cells total).
        assert!(t.contains("↑↓"), "key present: {t:?}");
        assert!(t.contains(" nav"), "nav label present: {t:?}");
        assert!(t.contains(" actions"), "actions hint present: {t:?}");
        assert!(!t.contains(" lazygit"), "lazygit hint removed: {t:?}");
        assert!(!t.contains(" edit"), "edit hint removed: {t:?}");
        assert!(!t.contains(" term"), "term hint removed: {t:?}");
        assert!(!t.contains(" diff"), "diff hint removed: {t:?}");
        assert!(t.contains(" group"));
        assert!(t.contains(" quit"));
        assert!(t.contains("24h "));
        assert!(t.contains("v0.5.0"));
    }

    #[test]
    fn footer_key_pill_wraps_key_only_not_label() {
        // V5 footer chips paint bg_soft behind only the key glyph (with
        // 1ch padding on each side). The label following the pill is plain
        // text on the bar bg — a regression that re-extended bg_soft over
        // the label would visually merge key and label into one block.
        let theme = Theme::wsx();
        let (line, _, _) = footer(&[1, 2, 3], "v0.5.0", 200, &theme, "24h", true);
        let key_span = line
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "↑↓")
            .expect("expected ↑↓ key span");
        assert_eq!(key_span.style.bg, Some(theme.bg_soft));
        let label_span = line
            .spans
            .iter()
            .find(|s| s.content.as_ref() == " nav")
            .expect("expected ` nav` label span (no chip padding)");
        assert_eq!(
            label_span.style.bg, None,
            "label should not carry the chip bg"
        );
    }

    #[test]
    fn footer_uses_provided_window_label_and_reports_graph_width() {
        let theme = Theme::wsx();
        let (line, graph_w, _) = footer(&[1, 2, 3], "9.9.9", 120, &theme, "1w", true);
        let rendered = text(&line);
        assert!(rendered.contains("1w"), "label should appear: {rendered}");
        assert!(!rendered.contains("24h"), "old hardcoded label gone");
        // graph segment = label chars + 1 space + 24 sparkline chars.
        assert_eq!(graph_w, ("1w".chars().count() + 1 + 24) as u16);
    }

    #[test]
    fn footer_hints_align_with_rendered_key_pills() {
        // Each hint's column run must cover exactly the pill+label it
        // describes, so a click lands on the same key the user sees. We
        // reconstruct the line's per-cell text and assert the first hint
        // (↑↓ nav → Down) and a single-letter hint (q quit → Char('q'))
        // sit over their glyphs.
        let theme = Theme::wsx();
        let (line, _, hints) = footer(&[1, 2, 3], "v0.5.0", 200, &theme, "24h", true);
        let cells: Vec<char> = text(&line).chars().collect();
        let slice = |h: &FooterHintSpan| -> String {
            cells[h.start_col as usize..(h.start_col + h.width) as usize]
                .iter()
                .collect()
        };
        let nav = hints
            .iter()
            .find(|h| h.action == FooterHintAction::Key(key_for_glyph("↑↓").unwrap()))
            .expect("nav hint present");
        assert_eq!(slice(nav), " ↑↓  nav", "nav hint covers pill + label");
        let quit = hints
            .iter()
            .find(|h| h.action == FooterHintAction::Key(key_for_glyph("q").unwrap()))
            .expect("quit hint present");
        assert_eq!(slice(quit), " q  quit", "quit hint covers pill + label");
        // Every printed keybind gets a hint (none drop out).
        assert_eq!(hints.len(), 8);
    }

    #[test]
    fn footer_omits_actions_pill_without_workspace() {
        let theme = Theme::wsx();
        let samples = vec![1, 2, 3, 4, 5];
        let (line, _, hints) = footer(&samples, "v0.5.0", 200, &theme, "24h", false);
        let t = text(&line);
        assert!(!t.contains(" actions"), "actions pill hidden: {t:?}");
        assert!(t.contains(" nav"), "nav still present: {t:?}");
        assert!(t.contains(" group"), "group still present: {t:?}");
        assert!(t.contains(" filter"), "filter still present: {t:?}");
        assert!(t.contains(" quit"), "quit still present: {t:?}");
        assert_eq!(hints.len(), 7, "7 hints when actions omitted");
    }
}
