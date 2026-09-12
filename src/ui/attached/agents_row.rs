//! Extracted from ui/attached.rs.

use super::*;

/// Switch keys for the chip row's agent pills, drawn from a reserved-safe pool so
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

/// Inter-pill separator width (in columns) between agent pills, and between
/// the pill group and the next element of the chip row's flush-right block.
pub(super) const AGENT_PILL_GAP: u16 = 3;

/// Identity-bar glyph for an idle (non-focused) agent: a 1-cell quarter block.
const AGENT_BAR_IDLE: &str = "▎";
/// Identity-bar glyph for the active (focused-pane) agent: a 1-cell half block.
/// Same column width as [`AGENT_BAR_IDLE`] but visually heavier, so the agent
/// you're currently driving stands out without shifting any pill rects.
const AGENT_BAR_ACTIVE: &str = "▌";

/// Column width of one agent pill: the identity bar, `label `, and (when the
/// agent has a switch key) the 3-cell ` key ` pill.
fn agent_pill_width(label: &str, key: Option<char>) -> u16 {
    let mut width = 1 + label.chars().count() as u16 + 1;
    if key.is_some() {
        width = width.saturating_add(3);
    }
    width
}

/// Total column width of the agent pill group as painted by
/// [`agent_pills_spans`]: every pill plus the [`AGENT_PILL_GAP`] between
/// neighbours. Zero when there are no agents.
pub fn agent_pills_width(agents: &[(AgentInstanceId, AgentKind, String, Option<char>)]) -> usize {
    let pills: usize = agents
        .iter()
        .map(|(_, _, label, key)| agent_pill_width(label, *key) as usize)
        .sum();
    let gaps = agents.len().saturating_sub(1) * AGENT_PILL_GAP as usize;
    pills + gaps
}

/// Spans for the agent pill group in the chip row: `▎claude q   ▎codex w`.
/// Each agent entry renders as a colored identity bar (`▎`), the agent
/// label, and (when present) a switch key in the footer's key-pill style.
/// Agents past the switch-key pool carry `None` and render keyless — they
/// still show the color bar + label and remain clickable.
///
/// `active` is the agent instance shown in the focused pane; its bar renders
/// with the heavier [`AGENT_BAR_ACTIVE`] glyph (and a bold label) so it reads
/// as "the one you're on" when several agents are attached.
pub fn agent_pills_spans(
    agents: &[(AgentInstanceId, AgentKind, String, Option<char>)],
    active: Option<AgentInstanceId>,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(agents.len() * 6);
    for (i, (id, kind, label, key)) in agents.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" ".repeat(AGENT_PILL_GAP as usize)));
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

/// Compute the clickable Rect for each agent pill painted from column `x` on
/// row `y`, mirroring [`agent_pills_spans`]. Returns one rect per agent, in
/// order, by walking pill widths from `x`. Each pill spans its color bar +
/// `label ` + optional ` key ` pill; the inter-pill gap is not included in
/// any rect. One rect is returned per agent (so indices stay aligned with the
/// agents slice), but each is clamped at `max_x`: a pill that begins at or
/// past it collapses to width 0 and is therefore not hit-testable.
pub fn layout_agent_pills(
    x: u16,
    y: u16,
    max_x: u16,
    agents: &[(AgentInstanceId, AgentKind, String, Option<char>)],
) -> Vec<Rect> {
    let mut rects = Vec::with_capacity(agents.len());
    let mut x = x;
    for (i, (_id, _kind, label, key)) in agents.iter().enumerate() {
        if i > 0 {
            x = x.saturating_add(AGENT_PILL_GAP);
        }
        let width = agent_pill_width(label, *key);
        let clamped_width = width.min(max_x.saturating_sub(x));
        rects.push(Rect {
            x,
            y,
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
    fn agent_pills_spans_include_label_and_color_bar() {
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
        let spans = agent_pills_spans(&agents, None, &theme);
        let text: String = spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("claude"));
        assert!(text.contains("codex"));
        assert!(text.contains('q'));
        assert!(text.contains('w'));
    }

    #[test]
    fn agent_pills_spans_thickens_active_agent_bar() {
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
        let spans = agent_pills_spans(&agents, Some(AgentInstanceId(2)), &theme);
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
        let spans = agent_pills_spans(&agents, None, &theme);
        let text: String = spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(text.matches(AGENT_BAR_ACTIVE).count(), 0);
        assert_eq!(text.matches(AGENT_BAR_IDLE).count(), 2);
    }

    #[test]
    fn layout_agent_pills_walks_from_the_given_start_column() {
        // Pills are laid out from an arbitrary start x (the flush-right
        // block's origin), not a fixed row prefix: bar (1) + `label ` +
        // ` key ` (3) per pill, 3-col gaps between, clamped at `max_x`.
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
                None,
            ),
        ];
        let rects = layout_agent_pills(40, 7, 60, &agents);
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0], Rect::new(40, 7, 11, 1), "▎claude  q ");
        // 40 + 11 + 3 gap = 54; keyless pill is 1 + 6 = 7 wide but only 6
        // columns remain before max_x, so it clamps.
        assert_eq!(rects[1], Rect::new(54, 7, 6, 1));
        assert_eq!(agent_pills_width(&agents), 11 + 3 + 7);
    }
}
