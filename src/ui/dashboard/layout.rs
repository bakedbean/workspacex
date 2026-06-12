//! Renders the three chrome bars around the V5 dashboard list:
//! top chrome, status strip, footer (keybinds + sparkline).

use crate::ui::dashboard::sort::StatusCounts;
use crate::ui::dashboard::sparkline;
use crate::ui::dashboard::status::Status;
use crate::ui::footer::{FooterHintAction, FooterHintSpan, key_for_glyph};
use crate::ui::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupMode {
    #[default]
    Repo,
    Attention,
}

pub fn top_chrome(
    group: GroupMode,
    repos: usize,
    workspaces: usize,
    width: usize,
    theme: &Theme,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = vec![
        Span::styled("wsx", theme.header_style()),
        Span::styled(" · dashboard".to_string(), theme.dim_style()),
        Span::raw(" ".repeat(6)),
        Span::styled("group: ".to_string(), Style::default().fg(theme.path)),
        tab_span("repo", group == GroupMode::Repo, theme),
        Span::raw(" ".to_string()),
        tab_span("attention", group == GroupMode::Attention, theme),
    ];

    let right = format!("{repos} repos · {workspaces} workspaces");
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let gap = width.saturating_sub(used + right.chars().count()).max(1);
    spans.push(Span::raw(" ".repeat(gap)));
    spans.push(Span::styled(right, Style::default().fg(theme.path)));
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
) -> (Line<'static>, u16, Vec<FooterHintSpan>) {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let keys = [
        ("↑↓", "nav"),
        ("↵", "open"),
        ("n", "new"),
        ("e", "edit"),
        ("t", "term"),
        ("v", "diff"),
        ("g", "lazygit"),
        ("G", "group"),
        ("/", "filter"),
        ("q", "quit"),
    ];
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
        let line = top_chrome(GroupMode::Repo, 9, 14, 100, &theme);
        let t = text(&line);
        assert!(t.starts_with("wsx · dashboard"), "{t:?}");
        assert!(t.contains("group: "));
        assert!(t.contains("repo"));
        assert!(t.contains("attention"));
        assert!(t.trim_end().ends_with("9 repos · 14 workspaces"), "{t:?}");
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
        let (line, _, _) = footer(&samples, "v0.5.0", 200, &theme, "24h");
        let t = text(&line);
        // After the V5 pill treatment, key and label are separated by the
        // pill's trailing pad + the label's leading space (2 cells total).
        assert!(t.contains("↑↓"), "key present: {t:?}");
        assert!(t.contains(" nav"), "nav label present: {t:?}");
        assert!(t.contains(" lazygit"));
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
        let (line, _, _) = footer(&[1, 2, 3], "v0.5.0", 200, &theme, "24h");
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
        let (line, graph_w, _) = footer(&[1, 2, 3], "9.9.9", 120, &theme, "1w");
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
        let (line, _, hints) = footer(&[1, 2, 3], "v0.5.0", 200, &theme, "24h");
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
        assert_eq!(hints.len(), 10);
    }
}
