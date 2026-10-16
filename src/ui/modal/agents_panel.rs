//! Extracted from ui/modal.rs.

use super::*;

/// Rows pinned to the bottom of the panel: a blank separator, the "Add:"
/// label, the add picker, and the key hints.
const PINNED_ROWS: u16 = 4;

/// Render the agents panel. Lists all instances attached to the workspace and
/// lets the user add / add-all / remove agents. Called directly from
/// `render.rs` with live app state — never goes through the generic
/// `render()` function.
///
/// The add picker and key hints are pinned to the bottom so they survive the
/// drawer's height cap. When the attached list does not fit above them it
/// shows its tail: `x` removes the most recently added instance, so the row
/// it acts on stays on screen, and the header counts the rows hidden above.
pub fn render_agents_panel(
    f: &mut Frame,
    area: Rect,
    agents: &[crate::data::agents::AgentInstance],
    selected: usize,
    theme: &Theme,
) {
    // The "Attached:" header, one row per instance, then the pinned rows.
    let want = 1 + agents.len() + usize::from(PINNED_ROWS);
    let inner = panel_frame(f, area, want, " agents ", theme);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(PINNED_ROWS)])
        .split(inner);
    let (list_area, pinned_area) = (chunks[0], chunks[1]);

    let room = usize::from(list_area.height).saturating_sub(1);
    let hidden = agents.len().saturating_sub(room);
    let mut lines: Vec<Line> = Vec::new();
    lines.push(if hidden > 0 {
        Line::from(vec![
            Span::raw(format!("Attached ({}):", agents.len())),
            Span::styled(format!("  {hidden} more above"), theme.dim_style()),
        ])
    } else {
        Line::from("Attached:")
    });
    for a in &agents[hidden..] {
        let tag = if a.is_primary { "  (primary)" } else { "" };
        lines.push(Line::from(vec![
            Span::styled("▎", theme.agent_style(a.agent)),
            Span::raw(format!(" {}{}", a.label(), tag)),
        ]));
    }
    f.render_widget(Paragraph::new(lines), list_area);

    let add: Vec<Span> = crate::pty::session::AgentKind::ALL
        .iter()
        .enumerate()
        .flat_map(|(i, k)| {
            let marker = if i == selected { "> " } else { "  " };
            vec![
                Span::raw(marker.to_string()),
                Span::styled("▎", theme.agent_style(*k)),
                Span::raw(format!("{}   ", k.display_name())),
            ]
        })
        .collect();
    let pinned = vec![
        Line::from(""),
        Line::from("Add:"),
        Line::from(add),
        crate::ui::drawer::hint_line(
            &[
                ("↵", "add"),
                ("a", "add all"),
                ("x", "remove"),
                ("↑↓", "move"),
                ("esc", "close"),
            ],
            theme,
        ),
    ];
    f.render_widget(Paragraph::new(pinned), pinned_area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::agents::AgentInstance;
    use crate::data::store::{AgentInstanceId, WorkspaceId};
    use crate::pty::session::AgentKind;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn agents(n: i64) -> Vec<AgentInstance> {
        (1..=n)
            .map(|ordinal| AgentInstance {
                id: AgentInstanceId(ordinal),
                workspace_id: WorkspaceId(1),
                agent: AgentKind::Claude,
                ordinal,
                is_primary: ordinal == 1,
                session_ref: None,
                agent_session_id: None,
                created_at: 0,
            })
            .collect()
    }

    fn render(agents: &[AgentInstance], h: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(80, h)).unwrap();
        term.draw(|f| render_agents_panel(f, f.area(), agents, 0, &Theme::wsx()))
            .unwrap();
        let buf = term.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn a_short_list_shows_every_agent_and_no_overflow_note() {
        let text = render(&agents(2), 30);
        assert!(text.contains("claude  (primary)"), "{text}");
        assert!(text.contains("claude#2"), "{text}");
        assert!(!text.contains("more above"), "{text}");
    }

    #[test]
    fn a_long_list_keeps_the_controls_and_the_newest_agent_visible() {
        // 20 agents on a 16-row screen: the drawer caps at 12 rows.
        let text = render(&agents(20), 16);
        assert!(text.contains("Add:"), "add picker pinned:\n{text}");
        assert!(text.contains("esc  close"), "hints pinned:\n{text}");
        assert!(
            text.contains("claude#20"),
            "newest (what x removes) shown:\n{text}"
        );
        assert!(text.contains("Attached (20):"), "{text}");
        assert!(text.contains("more above"), "{text}");
    }
}
