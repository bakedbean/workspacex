//! The attached view's recap panel: the workspace's `wsx recap set` digest.

use super::*;
use crate::data::store::WorkspaceRecap;

/// Render the recap panel in the bottom drawer from the live `App::recaps`
/// entry, so a `wsx recap set` shows once the cache next refreshes. Returns
/// `scroll` (rows from the top) clamped to what fits under the drawer's cap.
pub fn render_recap_panel(
    f: &mut Frame,
    area: Rect,
    workspace_name: &str,
    recap: Option<&WorkspaceRecap>,
    scroll: usize,
    now_ms: i64,
    theme: &Theme,
) -> usize {
    // The drawer's 1-cell gutters, then the 2-cell `▸ ` prefix.
    let inner_width = usize::from(area.width).saturating_sub(4).max(1);
    let prefix = Span::styled("▸ ".to_string(), theme.dim_style());
    let mut lines = recap
        .map(|r| {
            crate::ui::detail_modules::session_summary::recap_lines(
                r,
                &prefix,
                Style::default(),
                theme,
                inner_width,
            )
        })
        .unwrap_or_default();
    if lines.is_empty() {
        lines.push(Line::styled(
            "no recap yet — the agent sets one with `wsx recap set`",
            theme.dim_style(),
        ));
    } else if let Some(r) = recap {
        let age = crate::util::time::format_age(now_ms - r.updated_at);
        lines.push(Line::from(""));
        lines.push(Line::styled(
            format!("updated {age} ago"),
            theme.dim_style(),
        ));
    }
    let body = crate::ui::drawer::frame(
        f,
        area,
        u16::try_from(lines.len()).unwrap_or(u16::MAX),
        format!(" recap — {workspace_name} "),
        &[("↑↓", "scroll"), ("esc", "close")],
        theme,
    );
    let scroll = scroll.min(lines.len().saturating_sub(usize::from(body.height)));
    let visible: Vec<Line> = lines.into_iter().skip(scroll).collect();
    f.render_widget(Paragraph::new(visible), body);
    scroll
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn render_to_text(recap: Option<&WorkspaceRecap>, w: u16) -> String {
        render_scrolled(recap, w, 20, 0).0
    }

    fn render_scrolled(
        recap: Option<&WorkspaceRecap>,
        w: u16,
        h: u16,
        scroll: usize,
    ) -> (String, usize) {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        let mut clamped = 0;
        term.draw(|f| {
            clamped =
                render_recap_panel(f, f.area(), "alpha", recap, scroll, 120_000, &Theme::wsx());
        })
        .unwrap();
        let buf = term.backend().buffer();
        let text = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        (text, clamped)
    }

    #[test]
    fn renders_recap_slots_age_and_close_hint() {
        let recap = WorkspaceRecap {
            goal: Some("ship the recap panel".into()),
            state: None,
            next: None,
            goal_short: None,
            state_short: None,
            next_short: Some("write tests".into()),
            updated_at: 0,
        };
        let text = render_to_text(Some(&recap), 80);
        assert!(text.contains("recap — alpha"), "{text}");
        assert!(text.contains("goal:  ship the recap panel"), "{text}");
        // The short form stands in for a missing long field, as in the detail bar.
        assert!(text.contains("next:  write tests"), "{text}");
        assert!(!text.contains("state:"), "{text}");
        assert!(text.contains("updated 2m ago"), "{text}");
        assert!(text.contains("esc"), "{text}");
    }

    #[test]
    fn renders_placeholder_without_a_recap() {
        let text = render_to_text(None, 80);
        assert!(text.contains("no recap yet"), "{text}");
        assert!(!text.contains("updated"), "{text}");
    }

    #[test]
    fn wraps_long_values_inside_the_drawer() {
        let recap = WorkspaceRecap {
            goal: Some("one two three four five six seven eight nine ten".into()),
            state: None,
            next: None,
            goal_short: None,
            state_short: None,
            next_short: None,
            updated_at: 0,
        };
        let text = render_to_text(Some(&recap), 30);
        assert!(text.contains("ten"), "value must wrap, not clip:\n{text}");
    }

    #[test]
    fn scrolls_to_fields_past_the_drawer_cap() {
        let recap = WorkspaceRecap {
            goal: Some("word ".repeat(80)),
            state: None,
            next: Some("the final step".into()),
            goal_short: None,
            state_short: None,
            next_short: None,
            updated_at: 0,
        };
        let (top, _) = render_scrolled(Some(&recap), 30, 20, 0);
        assert!(
            !top.contains("final step"),
            "goal must overflow the cap:\n{top}"
        );
        // Overshooting clamps to the last screenful, which reaches `next`.
        let (bottom, clamped) = render_scrolled(Some(&recap), 30, 20, usize::MAX);
        assert!(bottom.contains("the final step"), "{bottom}");
        assert!(bottom.contains("updated 2m ago"), "{bottom}");
        assert!(clamped > 0 && clamped < usize::MAX);
    }
}
