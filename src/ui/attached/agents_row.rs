//! Extracted from ui/attached.rs.

use super::*;

/// Switch keys for the footer agents row, drawn from a reserved-safe pool so
/// they never collide with the `^x` leader follow-ups already bound in the
/// attached view:
///   - `a` → agents panel
///   - `u` → updates panel
///   - `d` → detach / close-pane
///   - `x` → send literal Ctrl-x
///   - `e` → open editor
///   - `t` → open terminal
///   - `v` → open diff
///   - `g` → open lazygit
///   - `c` → open chronox
///   - `k` → process list
///   - `1-9` (digits) → pinned commands
///
/// This is the single source of truth: both the renderer and (in a later
/// task) the input dispatcher call this with the same count so the
/// displayed key always equals the bound key.
///
/// Returns AT MOST `POOL.len()` (10) keys: a workspace with more than 10
/// agents (only reachable via many same-kind duplicates) exhausts the pool.
/// Callers must NOT `zip` agents against the result in a way that silently
/// drops the overflow — agents past the pool should still render (just
/// without a keyboard switch key; they remain clickable).
pub fn agent_switch_keys(count: usize) -> Vec<char> {
    // Pool excludes every letter the attached `^x` leader already binds
    // (d, x, u, a, e, t, v, g, c, k) plus all digits (pinned chips 1-9).
    const POOL: &[char] = &['q', 'w', 'r', 'y', 'i', 'o', 'p', 's', 'h', 'j'];
    POOL.iter().copied().take(count).collect()
}

/// Leading label of the agents row; its width is shared by the renderer and
/// `layout_agents_row` so the computed pill rects align with what's drawn.
const AGENTS_ROW_PREFIX: &str = "agents:  ";
/// Inter-pill separator width (in columns) in the agents row.
const AGENTS_ROW_GAP: u16 = 3;

/// Identity-bar glyph for an idle (non-focused) agent: a 1-cell quarter block.
const AGENT_BAR_IDLE: &str = "▎";
/// Identity-bar glyph for the active (focused-pane) agent: a 1-cell half block.
/// Same column width as [`AGENT_BAR_IDLE`] but visually heavier, so the agent
/// you're currently driving stands out without shifting any pill rects.
const AGENT_BAR_ACTIVE: &str = "▌";

/// Spans for the footer agents row: `agents:  ▎claude q   ▎codex w`.
/// Each agent entry renders as a colored identity bar (`▎`), the agent
/// label, and (when present) a switch key in the footer's key-pill style.
/// Agents past the switch-key pool carry `None` and render keyless — they
/// still show the color bar + label and remain clickable.
///
/// `active` is the agent instance shown in the focused pane; its bar renders
/// with the heavier [`AGENT_BAR_ACTIVE`] glyph (and a bold label) so it reads
/// as "the one you're on" when several agents are attached.
pub fn agents_row_spans(
    agents: &[(AgentInstanceId, AgentKind, String, Option<char>)],
    active: Option<AgentInstanceId>,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(1 + agents.len() * 6);
    spans.push(Span::raw(AGENTS_ROW_PREFIX.to_string()));
    for (i, (id, kind, label, key)) in agents.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" ".repeat(AGENTS_ROW_GAP as usize)));
        }
        let is_active = active == Some(*id);
        let bar = if is_active {
            AGENT_BAR_ACTIVE
        } else {
            AGENT_BAR_IDLE
        };
        spans.push(Span::styled(bar.to_string(), theme.agent_style(*kind)));
        let label_span = if is_active {
            Span::styled(
                format!("{label} "),
                Style::default().add_modifier(Modifier::BOLD),
            )
        } else {
            Span::raw(format!("{label} "))
        };
        spans.push(label_span);
        if let Some(key) = key {
            spans.extend(key_pill_spans(&key.to_string(), theme));
        }
    }
    spans
}

/// Compute the clickable Rect for each agent pill in the footer agents row,
/// mirroring [`layout_chip_row`]. Returns one rect per agent, in order, by
/// walking pill widths from the leading `agents:` label. Each pill spans its
/// color bar + `label ` + optional ` key ` pill; the inter-pill gap is not
/// included in any rect. One rect is returned per agent (so indices stay
/// aligned with the agents slice), but each is clamped to the row width: a
/// pill that begins at or past the row's right edge collapses to width 0 and
/// is therefore not hit-testable. So agents whose pills overflow the visible
/// row width are rendered up to the edge but are not clickable — switch to
/// them by key instead, or widen the terminal.
pub fn layout_agents_row(
    area: Rect,
    agents: &[(AgentInstanceId, AgentKind, String, Option<char>)],
) -> Vec<Rect> {
    let mut rects = Vec::with_capacity(agents.len());
    let max_x = area.x.saturating_add(area.width);
    let mut x = area
        .x
        .saturating_add(AGENTS_ROW_PREFIX.chars().count() as u16);
    for (i, (_id, _kind, label, key)) in agents.iter().enumerate() {
        if i > 0 {
            x = x.saturating_add(AGENTS_ROW_GAP);
        }
        // "▎" (1) + "{label} " (label + 1) + optional " key " pill (3).
        let mut width = 1 + label.chars().count() as u16 + 1;
        if key.is_some() {
            width = width.saturating_add(3);
        }
        let clamped_width = width.min(max_x.saturating_sub(x));
        rects.push(Rect {
            x,
            y: area.y,
            width: clamped_width,
            height: 1,
        });
        x = x.saturating_add(width);
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch_keys_skip_reserved_and_are_unique() {
        // Request the whole pool so the exclusion check covers every key.
        let keys = agent_switch_keys(64);
        // No reserved `^x`-leader letter may appear anywhere in the pool.
        for reserved in ['d', 'x', 'u', 'a', 'e', 't', 'v', 'g', 'c', 'k'] {
            assert!(
                !keys.contains(&reserved),
                "pool must not contain reserved '{reserved}'"
            );
        }
        assert!(keys.iter().all(|c| !c.is_ascii_digit())); // digits are pinned chips
        let unique: std::collections::HashSet<_> = keys.iter().collect();
        assert_eq!(unique.len(), keys.len());
    }

    #[test]
    fn agents_row_spans_include_label_and_color_bar() {
        let theme = Theme::by_name("default");
        let agents = vec![
            (
                AgentInstanceId(1),
                AgentKind::Claude,
                "claude".to_string(),
                Some('q'),
            ),
            (
                AgentInstanceId(2),
                AgentKind::Codex,
                "codex".to_string(),
                Some('w'),
            ),
        ];
        let spans = agents_row_spans(&agents, None, &theme);
        let text: String = spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("claude"));
        assert!(text.contains("codex"));
        assert!(text.contains('q'));
        assert!(text.contains('w'));
    }

    #[test]
    fn agents_row_spans_thickens_active_agent_bar() {
        let theme = Theme::by_name("default");
        let agents = vec![
            (
                AgentInstanceId(1),
                AgentKind::Claude,
                "claude".to_string(),
                Some('q'),
            ),
            (
                AgentInstanceId(2),
                AgentKind::Codex,
                "codex".to_string(),
                Some('w'),
            ),
        ];

        // With the second agent active, exactly one heavier bar is drawn and
        // the idle bar still appears for the other agent.
        let spans = agents_row_spans(&agents, Some(AgentInstanceId(2)), &theme);
        let text: String = spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(
            text.matches(AGENT_BAR_ACTIVE).count(),
            1,
            "active agent should get exactly one heavier bar"
        );
        assert_eq!(
            text.matches(AGENT_BAR_IDLE).count(),
            1,
            "the non-active agent keeps the idle bar"
        );

        // With no active agent (e.g. the PM pane), every bar is idle.
        let spans = agents_row_spans(&agents, None, &theme);
        let text: String = spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(text.matches(AGENT_BAR_ACTIVE).count(), 0);
        assert_eq!(text.matches(AGENT_BAR_IDLE).count(), 2);
    }
}
