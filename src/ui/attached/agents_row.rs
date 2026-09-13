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

/// Identity dot for an idle (non-focused) agent — a hollow circle in the
/// agent's kind colour — plus the space that separates it from the label.
const AGENT_DOT_IDLE: &str = "○ ";
/// Identity dot for the active (focused-pane) agent: a filled circle. Same
/// column width as [`AGENT_DOT_IDLE`] but visually heavier, so the agent
/// you're currently driving stands out without shifting any pill rects.
const AGENT_DOT_ACTIVE: &str = "● ";
/// Columns taken by [`AGENT_DOT_IDLE`] / [`AGENT_DOT_ACTIVE`] (`● claude`).
/// The width arithmetic and click-rect layout use this; the painter emits
/// the strings — `dot_prefixes_match_declared_width` keeps them in step.
const AGENT_DOT_WIDTH: u16 = 2;

/// Column width of one agent pill: the identity dot + space, `label `, and
/// (when the agent has a switch key) the 3-cell ` key ` pill.
fn agent_pill_width(label: &str, key: Option<char>) -> u16 {
    let mut width = AGENT_DOT_WIDTH + label.chars().count() as u16 + 1;
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

/// Spans for the agent pill group in the chip row: `● claude q   ○ codex w`.
/// Each agent entry renders as a colored identity dot, the agent label, and
/// (when present) a switch key in the footer's key-pill style. Agents past
/// the switch-key pool carry `None` and render keyless — they still show the
/// color dot + label and remain clickable.
///
/// `active` is the agent instance shown in the focused pane; its dot renders
/// with the filled [`AGENT_DOT_ACTIVE`] glyph (and a bold label) so it reads
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
        let dot = if is_active {
            AGENT_DOT_ACTIVE
        } else {
            AGENT_DOT_IDLE
        };
        spans.push(Span::styled(dot.to_string(), theme.agent_style(*kind)));
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
/// order, by walking pill widths from `x`. Each pill spans its color dot +
/// space + `label ` + optional ` key ` pill; the inter-pill gap is not included in
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
    fn agent_pills_spans_include_label_and_color_dot() {
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

        // Each pill opens with the dot span in its agent's kind colour.
        let dots: Vec<&Span<'static>> = spans
            .iter()
            .filter(|s| s.content.as_ref() == AGENT_DOT_IDLE)
            .collect();
        assert_eq!(dots.len(), 2, "one dot per agent: {text:?}");
        assert_eq!(dots[0].style.fg, theme.agent_style(AgentKind::Claude).fg);
        assert_eq!(dots[1].style.fg, theme.agent_style(AgentKind::Codex).fg);
    }

    #[test]
    fn dot_prefixes_match_declared_width() {
        // The painter emits the prefix strings; the width arithmetic and
        // click rects use `AGENT_DOT_WIDTH`. They must agree or rects drift.
        assert_eq!(AGENT_DOT_IDLE.chars().count(), AGENT_DOT_WIDTH as usize);
        assert_eq!(AGENT_DOT_ACTIVE.chars().count(), AGENT_DOT_WIDTH as usize);
    }

    #[test]
    fn agent_pills_spans_fill_the_focused_instance_among_same_kind_agents() {
        // Two Claude agents share a colour, so the filled dot is the only
        // per-pill cue for which instance is focused. Focus keys off the
        // instance id, not the kind: with `claude#2` active, its pill (and
        // only its pill) carries the filled dot, and both dots stay Claude-
        // coloured.
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
                AgentKind::Claude,
                "claude#2".to_string(),
                Some('w'),
            ),
        ];
        let spans = agent_pills_spans(&agents, Some(AgentInstanceId(2)), &theme);
        let text: String = spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(text, "○ claude  q    ● claude#2  w ");
        let claude_fg = theme.agent_style(AgentKind::Claude).fg;
        for dot in spans.iter().filter(|s| {
            s.content.as_ref() == AGENT_DOT_IDLE || s.content.as_ref() == AGENT_DOT_ACTIVE
        }) {
            assert_eq!(dot.style.fg, claude_fg);
        }
    }

    #[test]
    fn agent_pills_spans_fills_active_agent_dot() {
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

        // With the second agent active, exactly one filled dot is drawn and
        // the hollow dot still appears for the other agent.
        let spans = agent_pills_spans(&agents, Some(AgentInstanceId(2)), &theme);
        let text: String = spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(
            text.matches(AGENT_DOT_ACTIVE).count(),
            1,
            "active agent should get exactly one filled dot"
        );
        assert_eq!(
            text.matches(AGENT_DOT_IDLE).count(),
            1,
            "the non-active agent keeps the hollow dot"
        );

        // With no active agent (e.g. the PM pane), every dot is hollow.
        let spans = agent_pills_spans(&agents, None, &theme);
        let text: String = spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(text.matches(AGENT_DOT_ACTIVE).count(), 0);
        assert_eq!(text.matches(AGENT_DOT_IDLE).count(), 2);
    }

    #[test]
    fn layout_agent_pills_walks_from_the_given_start_column() {
        // Pills are laid out from an arbitrary start x (the flush-right
        // block's origin), not a fixed row prefix: dot + space (2) + `label `
        // + ` key ` (3) per pill, 3-col gaps between, clamped at `max_x`.
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
        let rects = layout_agent_pills(40, 7, 62, &agents);
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0], Rect::new(40, 7, 12, 1), "○ claude  q ");
        // 40 + 12 + 3 gap = 55; keyless pill is 2 + 5 + 1 = 8 wide but only 7
        // columns remain before max_x, so it clamps.
        assert_eq!(rects[1], Rect::new(55, 7, 7, 1));
        assert_eq!(agent_pills_width(&agents), 12 + 3 + 8);
    }
}
