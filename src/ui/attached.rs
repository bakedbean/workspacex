use crate::commands::pinned::{PinnedCommand, truncate_label};
use crate::data::store::AgentInstanceId;
use crate::pty::render::render_screen;
use crate::pty::session::{AgentKind, Session};
use crate::ui::split::{Divider, SplitDirection};
use crate::ui::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::widgets::Paragraph;
use std::sync::Arc;

/// One pane in the attached view: a workspace's PTY plus its label,
/// the rect it occupies, and whether it's the focused pane (cursor + chip
/// chrome). For the single-pane case the slice has one entry; for vim-style
/// splits there's one entry per leaf.
pub struct PaneSpec<'a> {
    pub session: &'a Arc<Session>,
    pub label: &'a str,
    pub rect: Rect,
    pub focused: bool,
    /// The pane's coding agent, or `None` for the project-manager pane
    /// (which is not one of the four coding agents).
    pub agent: Option<AgentKind>,
}

/// What `render_panes` reports back to the caller for input hit-testing.
pub struct PanesDrawOutput {
    /// Clickable rects of the pinned-command chips (same as before).
    pub chip_rects: Vec<Rect>,
    /// `(session, terminal content rect)` for each rendered pane.
    pub pane_rects: Vec<(Arc<Session>, Rect)>,
    /// `(instance id, clickable rect)` for each agent pill in the footer
    /// agents row. Empty when the row isn't shown. Consumed by the input
    /// handler to retarget the focused pane on click.
    pub agent_chip_rects: Vec<(AgentInstanceId, Rect)>,
}

/// Render one or more attached panes plus the shared chrome (optional
/// chip row, optional attention line, footer). Returns a [`PanesDrawOutput`]:
/// the per-chip clickable rects plus each pane's `(session, content rect)`,
/// both consumed by the input handler for mouse hit-testing.
///
/// Layout (top to bottom):
///   - the pane area, subdivided per `panes[i].rect` (which the caller
///     pre-computed from `SplitTree::layout`),
///   - one row of pinned-command chips (only when `pinned` is non-empty),
///   - one row of cross-workspace attention status (only when `Some`),
///   - one row of footer hints.
///
/// When there are multiple panes, each pane also gets a 1-row title bar
/// at the top of its rect showing the workspace name and a focus marker.
/// Single-pane mode skips the title bar so it looks identical to the
/// previous single-attached view.
#[allow(clippy::too_many_arguments)]
pub fn render_panes(
    f: &mut Frame,
    panes: &[PaneSpec<'_>],
    dividers: &[Divider],
    chip_area: Rect,
    status_area: Rect,
    footer_area: Rect,
    agents_area: Rect,
    footer_label: &str,
    footer_agent: Option<AgentKind>,
    multi_pane_footer: bool,
    attention_line: Option<Line<'static>>,
    pinned: &[PinnedCommand],
    agents: &[(AgentInstanceId, AgentKind, String, Option<char>)],
    theme: &Theme,
) -> PanesDrawOutput {
    let show_titles = panes.len() > 1;

    let mut pane_rects = Vec::with_capacity(panes.len());
    for pane in panes {
        let term_area = render_one_pane(f, pane, show_titles, theme);
        pane_rects.push((Arc::clone(pane.session), term_area));
    }

    render_dividers(f, dividers, theme);

    if let Some(line) = attention_line {
        f.render_widget(Paragraph::new(line), status_area);
    }

    // Footer rect is 2 cells tall; the empty first line gives the keys
    // breathing room from the row above without doubling spacing
    // throughout the chrome stack.
    let footer_text = ratatui::text::Text::from(vec![
        Line::from(Vec::<Span<'static>>::new()),
        footer_line(footer_label, footer_agent, multi_pane_footer, theme),
    ]);
    f.render_widget(Paragraph::new(footer_text), footer_area);

    // Chips + inline rule filler. Always renders so the rule shows even
    // when there are no pinned commands.
    let chip_rects = render_chip_row(f, chip_area, pinned, theme);

    // Agents row: only rendered when the workspace has more than its primary
    // agent. Each pill's clickable rect is computed alongside the spans so the
    // input handler can retarget the focused pane on click.
    let agent_chip_rects: Vec<(AgentInstanceId, Rect)> = if agents.is_empty() {
        Vec::new()
    } else {
        let spans = agents_row_spans(agents, theme);
        f.render_widget(Paragraph::new(Line::from(spans)), agents_area);
        let rects = layout_agents_row(agents_area, agents);
        agents.iter().map(|(id, _, _, _)| *id).zip(rects).collect()
    };

    PanesDrawOutput {
        chip_rects,
        pane_rects,
        agent_chip_rects,
    }
}

fn render_one_pane(f: &mut Frame, pane: &PaneSpec<'_>, show_title: bool, theme: &Theme) -> Rect {
    let (title_area, term_area) = if show_title {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(pane.rect);
        (Some(chunks[0]), chunks[1])
    } else {
        (None, pane.rect)
    };

    if let Some(area) = title_area {
        // V5-style: ▎ gutter in accent color when focused, idle when not;
        // workspace name in bold. Focused row gets the selection bg fill
        // so the focus indicator is unmistakable even at a glance.
        let row_bg = if pane.focused {
            Style::default().bg(theme.selected_bg)
        } else {
            Style::default()
        };
        let spans = title_bar_spans(pane.label, pane.agent, pane.focused, theme);
        f.render_widget(Paragraph::new(Line::from(spans)).style(row_bg), area);
    }

    let offset = pane
        .session
        .scrollback_offset
        .load(std::sync::atomic::Ordering::Relaxed);
    let mut parser = pane.session.parser.lock().unwrap();
    parser.set_scrollback(offset);
    let screen = parser.screen();
    render_screen(screen, f.buffer_mut(), term_area);
    if pane.focused {
        let (cy, cx) = screen.cursor_position();
        if !screen.hide_cursor() && offset == 0 {
            f.set_cursor_position((term_area.x + cx, term_area.y + cy));
        }
    }
    drop(parser);
    term_area
}

/// Draw subtle 1-cell dividers between adjacent split panes. Vertical
/// dividers (between side-by-side panes) use `│`, horizontal dividers
/// (between stacked panes) use `─`, both in the muted `path` color so
/// they read as chrome, not content.
fn render_dividers(f: &mut Frame, dividers: &[Divider], theme: &Theme) {
    if dividers.is_empty() {
        return;
    }
    let style = Style::default().fg(theme.path);
    let buf = f.buffer_mut();
    for div in dividers {
        let (glyph, w, h) = match div.direction {
            SplitDirection::Vertical => ("│", 1u16, div.rect.height),
            SplitDirection::Horizontal => ("─", div.rect.width, 1u16),
        };
        if w == 0 || h == 0 {
            continue;
        }
        match div.direction {
            SplitDirection::Vertical => {
                let x = div.rect.x;
                for y in div.rect.y..div.rect.y.saturating_add(h) {
                    if buf.area().contains((x, y).into()) {
                        buf[(x, y)].set_symbol(glyph).set_style(style);
                    }
                }
            }
            SplitDirection::Horizontal => {
                let y = div.rect.y;
                for x in div.rect.x..div.rect.x.saturating_add(w) {
                    if buf.area().contains((x, y).into()) {
                        buf[(x, y)].set_symbol(glyph).set_style(style);
                    }
                }
            }
        }
    }
}

/// Carve the attached view's full `area` into pane / chip / status /
/// footer / agents sub-areas. Chip and attention rows are 1 cell tall
/// (flush with each other — the chip row's inline `─` rule already
/// provides visual separation from above). The footer rect is 2 cells
/// tall so its leading blank line lifts the keys one cell away from the
/// rows above, regardless of whether the attention line is present.
/// When `agents_present` is true an additional 1-row agents strip is
/// appended below the footer.
///
/// The chip row carries either pinned-command chips followed by a `─`
/// rule filler, or just the rule when no chips are configured.
pub fn layout_chrome(
    area: Rect,
    attention_present: bool,
    _pinned_present: bool,
    agents_present: bool,
) -> (Rect, Rect, Rect, Rect, Rect) {
    let status_h = if attention_present { 1 } else { 0 };
    let agents_h = if agents_present { 1 } else { 0 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),        // chip row
            Constraint::Length(status_h), // attention row (0 when absent)
            Constraint::Length(2),        // footer keys with 1-cell spacer above
            Constraint::Length(agents_h), // agents row (0 when absent)
        ])
        .split(area);
    (chunks[0], chunks[1], chunks[2], chunks[3], chunks[4])
}

/// Resize a session's PTY to fill its pane area (minus a per-pane title
/// row when `multi_pane` is true).
pub fn resize_pane(session: &Arc<Session>, pane_rect: Rect, multi_pane: bool) {
    let title: u16 = if multi_pane { 1 } else { 0 };
    let _ = session.resize(pane_rect.width, pane_rect.height.saturating_sub(title));
}

/// Build the spans for a pane's title bar: an optional per-agent identity
/// bar, the focus gutter (accent when focused, idle otherwise), then the
/// bold workspace label. Pure so the agent-bar branch is unit-testable
/// without a live `Session`/`Frame` (see `render_one_pane`, which applies
/// the row background separately).
fn title_bar_spans(
    label: &str,
    agent: Option<AgentKind>,
    focused: bool,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let gutter_style = if focused {
        Style::default().fg(theme.waiting)
    } else {
        Style::default().fg(theme.idle)
    };
    let name_style = if focused {
        theme.selected_style().add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.dim).add_modifier(Modifier::BOLD)
    };
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(3);
    if let Some(agent) = agent {
        // Agent identity bar, left of the focus gutter → two-tone edge.
        spans.push(Span::styled("▎".to_string(), theme.agent_style(agent)));
    }
    spans.push(Span::styled("▎".to_string(), gutter_style));
    spans.push(Span::styled(format!(" {} ", label), name_style));
    spans
}

/// V5-styled footer: workspace label in `header_style`, then the `^x`
/// leader, then per-keybind chips (`<key>` in dim+bold, ` <label>` in
/// `path` color), separated by 2 spaces. Matches the dashboard footer's
/// chip pattern.
fn footer_line(
    label: &str,
    agent: Option<AgentKind>,
    multi_pane: bool,
    theme: &Theme,
) -> Line<'static> {
    let keys: &[(&str, &str)] = if multi_pane {
        &[
            ("d", "close-pane"),
            ("←→", "focus"),
            ("u", "updates"),
            ("e", "edit"),
            ("t", "term"),
            ("v", "diff"),
            ("g", "lazygit"),
            ("k", "procs"),
            ("x", "send-^x"),
        ]
    } else {
        &[
            ("d", "detach"),
            ("u", "updates"),
            ("e", "edit"),
            ("t", "term"),
            ("v", "diff"),
            ("g", "lazygit"),
            ("k", "procs"),
            ("x", "send-^x"),
        ]
    };
    let key_style = Style::default()
        .fg(theme.dim)
        .add_modifier(Modifier::BOLD)
        .bg(theme.bg_soft);
    let label_style = Style::default().fg(theme.path);
    let pad_style = theme.chip_bg_style();

    let mut spans: Vec<Span<'static>> = Vec::with_capacity(2 + keys.len() * 5 + 4);
    if let Some(a) = agent {
        spans.push(Span::styled("▎".to_string(), theme.agent_style(a)));
        spans.push(Span::raw(" ".to_string()));
    }
    spans.push(Span::styled(label.to_string(), theme.header_style()));
    spans.push(Span::raw("   ".to_string()));
    // ^x leader rendered as a standalone pill (no label tail).
    spans.push(Span::styled(" ".to_string(), pad_style));
    spans.push(Span::styled("^x".to_string(), key_style));
    spans.push(Span::styled(" ".to_string(), pad_style));
    for (key, lbl) in keys {
        spans.push(Span::raw("  ".to_string()));
        spans.push(Span::styled(" ".to_string(), pad_style));
        spans.push(Span::styled((*key).to_string(), key_style));
        spans.push(Span::styled(" ".to_string(), pad_style));
        spans.push(Span::styled(format!(" {lbl}"), label_style));
    }
    Line::from(spans)
}

/// Compute the clickable Rect for each chip that fits within `area`.
/// Returns one Rect per chip rendered left-to-right; chips that don't fit
/// are dropped from the end. The chip text is ` <N> <label> ` (V5 button
/// treatment: 1ch padding on each side of the `N <label>` core) joined
/// by 2-space gaps. Labels are individually truncated to 12 columns first.
pub fn layout_chip_row(area: Rect, pinned: &[PinnedCommand]) -> Vec<Rect> {
    let mut rects = Vec::new();
    let mut x = area.x;
    let max_x = area.x.saturating_add(area.width);
    const GAP: u16 = 2;
    for (i, cmd) in pinned.iter().enumerate().take(9) {
        let label = truncate_label(&cmd.label, 12);
        // Chip text: " N label "  (leading pad + N + " " + label + trailing pad)
        let chip_chars = 4 + label.chars().count() as u16;
        if i > 0 {
            x = x.saturating_add(GAP);
        }
        if x.saturating_add(chip_chars) > max_x {
            break;
        }
        rects.push(Rect {
            x,
            y: area.y,
            width: chip_chars,
            height: 1,
        });
        x = x.saturating_add(chip_chars);
    }
    rects
}

pub(crate) fn render_chip_row(
    f: &mut Frame,
    area: Rect,
    pinned: &[PinnedCommand],
    theme: &Theme,
) -> Vec<Rect> {
    let rects = layout_chip_row(area, pinned);
    let key_style = Style::default()
        .fg(theme.dim)
        .add_modifier(Modifier::BOLD)
        .bg(theme.bg_soft);
    let label_style = Style::default().fg(theme.path);
    let pad_style = theme.chip_bg_style();
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(rects.len() * 5 + 2);
    let mut used: usize = 0;
    for (i, (_rect, cmd)) in rects.iter().zip(pinned.iter()).enumerate() {
        if i > 0 {
            spans.push(Span::raw("  ".to_string()));
            used += 2;
        }
        let label = truncate_label(&cmd.label, 12);
        let chip_text = format!("{}", i + 1);
        spans.push(Span::styled(" ".to_string(), pad_style));
        used += 1;
        spans.push(Span::styled(chip_text, key_style));
        used += 1;
        spans.push(Span::styled(" ".to_string(), pad_style));
        used += 1;
        let label_with_lead = format!(" {label}");
        used += label_with_lead.chars().count();
        spans.push(Span::styled(label_with_lead, label_style));
    }
    // Inline rule filler matching the V5 dashboard repo-header style:
    // 2 spaces (or 0 when there are no chips), then `─` runs to the
    // right edge of the row.
    let width = area.width as usize;
    if width > used {
        let gap = if used == 0 { 0 } else { 2 };
        let rule_len = width.saturating_sub(used + gap);
        if gap > 0 && rule_len > 0 {
            spans.push(Span::raw(" ".repeat(gap)));
        }
        if rule_len > 0 {
            spans.push(Span::styled("─".repeat(rule_len), theme.dim_style()));
        }
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
    rects
}

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
    // (d, x, u, a, e, t, v, g, k) plus all digits (pinned chips 1-9).
    const POOL: &[char] = &['q', 'w', 'r', 'y', 'i', 'o', 'p', 's', 'h', 'j'];
    POOL.iter().copied().take(count).collect()
}

/// Leading label of the agents row; its width is shared by the renderer and
/// `layout_agents_row` so the computed pill rects align with what's drawn.
const AGENTS_ROW_PREFIX: &str = "agents:  ";
/// Inter-pill separator width (in columns) in the agents row.
const AGENTS_ROW_GAP: u16 = 3;

/// Spans for the footer agents row: `agents:  ▎claude q   ▎codex w`.
/// Each agent entry renders as a colored identity bar (`▎`), the agent
/// label, and (when present) a switch key in the footer's key-pill style.
/// Agents past the switch-key pool carry `None` and render keyless — they
/// still show the color bar + label and remain clickable.
pub fn agents_row_spans(
    agents: &[(AgentInstanceId, AgentKind, String, Option<char>)],
    theme: &Theme,
) -> Vec<Span<'static>> {
    let key_style = Style::default()
        .fg(theme.dim)
        .add_modifier(Modifier::BOLD)
        .bg(theme.bg_soft);
    let pad_style = theme.chip_bg_style();

    let mut spans: Vec<Span<'static>> = Vec::with_capacity(1 + agents.len() * 6);
    spans.push(Span::raw(AGENTS_ROW_PREFIX.to_string()));
    for (i, (_id, kind, label, key)) in agents.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" ".repeat(AGENTS_ROW_GAP as usize)));
        }
        spans.push(Span::styled("▎".to_string(), theme.agent_style(*kind)));
        spans.push(Span::raw(format!("{label} ")));
        if let Some(key) = key {
            spans.push(Span::styled(" ".to_string(), pad_style));
            spans.push(Span::styled(key.to_string(), key_style));
            spans.push(Span::styled(" ".to_string(), pad_style));
        }
    }
    spans
}

/// Compute the clickable Rect for each agent pill in the footer agents row,
/// mirroring [`layout_chip_row`]. Returns one rect per agent, in order, by
/// walking pill widths from the leading `agents:` label. Each pill spans its
/// color bar + `label ` + optional ` key ` pill; the inter-pill gap is not
/// included in any rect. Rects are clamped to the row area but never dropped,
/// so every agent (including keyless overflow) stays clickable.
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
    use crate::commands::pinned::PinnedCommand;

    fn cmds(specs: &[(&str, &str)]) -> Vec<PinnedCommand> {
        specs
            .iter()
            .map(|(l, c)| PinnedCommand {
                label: (*l).into(),
                command: (*c).into(),
            })
            .collect()
    }

    #[test]
    fn chip_row_layout_returns_rects_for_each_visible_chip() {
        let area = ratatui::layout::Rect::new(0, 0, 80, 1);
        let pinned = cmds(&[("PR", "/pr"), ("FB", "/fb"), ("UR", "/ur")]);
        let rects = layout_chip_row(area, &pinned);
        assert_eq!(rects.len(), 3);
        for r in &rects {
            assert!(r.width > 0);
            assert_eq!(r.y, 0);
        }
        // Chips render left-to-right with at least one column of gap.
        assert!(rects[1].x > rects[0].x + rects[0].width);
    }

    #[test]
    fn chip_row_drops_trailing_chips_when_too_narrow() {
        let area = ratatui::layout::Rect::new(0, 0, 12, 1);
        let pinned = cmds(&[("PR", "/pr"), ("FB", "/fb"), ("UR", "/ur")]);
        let rects = layout_chip_row(area, &pinned);
        // Exact count depends on chip widths; at width 12 we expect strictly
        // fewer than 3, with at least 1.
        assert!(!rects.is_empty(), "should render at least one chip");
        assert!(rects.len() < 3, "should drop trailing chips at width 12");
    }

    #[test]
    fn chip_row_empty_list_returns_no_rects() {
        let area = ratatui::layout::Rect::new(0, 0, 80, 1);
        assert!(layout_chip_row(area, &[]).is_empty());
    }

    #[test]
    fn footer_line_pill_wraps_key_only_not_label() {
        // The attached footer's ^x leader and each chord key get a pill;
        // the label following each chord is plain text on the bar bg. A
        // regression that re-extended bg_soft over the label would
        // visually merge key and label into one block.
        let theme = crate::ui::theme::Theme::wsx();
        let line = footer_line("ws", None, false, &theme);
        let leader = line
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "^x")
            .expect("expected ^x leader span");
        assert_eq!(leader.style.bg, Some(theme.bg_soft));
        let chord_key = line
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "d")
            .expect("expected `d` chord-key span");
        assert_eq!(chord_key.style.bg, Some(theme.bg_soft));
        let chord_label = line
            .spans
            .iter()
            .find(|s| s.content.as_ref() == " detach")
            .expect("expected ` detach` chord-label span (no chip padding)");
        assert_eq!(
            chord_label.style.bg, None,
            "label should not carry the chip bg"
        );
    }

    #[test]
    fn footer_line_prepends_agent_bar_when_present() {
        let theme = Theme::wsx();
        let line = footer_line("wsx/foo", Some(AgentKind::Codex), false, &theme);
        assert_eq!(line.spans[0].content.as_ref(), "▎");
        assert_eq!(
            line.spans[0].style.fg,
            theme.agent_style(AgentKind::Codex).fg
        );
        assert_eq!(
            line.spans[2].content.as_ref(),
            "wsx/foo",
            "label follows the bar and its trailing space"
        );
    }

    #[test]
    fn footer_line_omits_agent_bar_when_none() {
        let theme = Theme::wsx();
        let line = footer_line("project-manager", None, false, &theme);
        assert_eq!(
            line.spans[0].content.as_ref(),
            "project-manager",
            "no leading bar for the PM pane"
        );
    }

    #[test]
    fn title_bar_spans_prepend_agent_bar_when_present() {
        let theme = Theme::wsx();
        let spans = title_bar_spans("foo", Some(AgentKind::Pi), true, &theme);
        assert_eq!(spans[0].content.as_ref(), "▎", "agent bar first");
        assert_eq!(spans[0].style.fg, theme.agent_style(AgentKind::Pi).fg);
        assert_eq!(spans[1].content.as_ref(), "▎", "focus gutter second");
        assert_eq!(spans[2].content.as_ref(), " foo ", "label last");
        assert_ne!(
            spans[0].style.fg, spans[1].style.fg,
            "agent and gutter colors differ (two-tone edge)"
        );
    }

    #[test]
    fn title_bar_spans_omit_agent_bar_when_none() {
        let theme = Theme::wsx();
        let spans = title_bar_spans("project-manager", None, false, &theme);
        assert_eq!(spans[0].content.as_ref(), "▎", "only the focus gutter");
        assert_eq!(spans[1].content.as_ref(), " project-manager ");
        assert_eq!(spans.len(), 2, "no agent bar when None");
    }

    #[test]
    fn layout_chrome_places_spacer_above_footer_keys() {
        // The chip and attention rows sit flush with each other (the
        // chip's `─` rule does the visual separation from above). The
        // footer rect is 2 cells tall so its leading blank line provides
        // a single cell of breathing room just above the keys —
        // independent of whether the attention row is present.
        let area = ratatui::layout::Rect::new(0, 0, 80, 30);
        let (pane, chip, status, footer, agents) = layout_chrome(area, true, true, false);
        assert_eq!(chip.height, 1, "chip row is 1 tall (no spacer below)");
        assert_eq!(
            status.height, 1,
            "attention row is 1 tall (flush with chip)"
        );
        assert_eq!(
            footer.height, 2,
            "footer rect is 2 tall (spacer + keys row)"
        );
        assert_eq!(agents.height, 0, "agents row absent when not requested");
        assert_eq!(
            pane.height + chip.height + status.height + footer.height + agents.height,
            area.height,
            "chrome chunks should tile the full area without overlap"
        );

        let (_, chip2, status2, footer2, agents2) = layout_chrome(area, false, true, false);
        assert_eq!(chip2.height, 1);
        assert_eq!(
            status2.height, 0,
            "attention row collapses to 0 when absent"
        );
        assert_eq!(
            footer2.height, 2,
            "footer rect still has its leading spacer when attention absent"
        );
        assert_eq!(agents2.height, 0);

        let (_, _, _, _, agents3) = layout_chrome(area, false, true, true);
        assert_eq!(agents3.height, 1, "agents row is 1 tall when present");
    }

    #[test]
    fn switch_keys_skip_reserved_and_are_unique() {
        // Request the whole pool so the exclusion check covers every key.
        let keys = agent_switch_keys(64);
        // No reserved `^x`-leader letter may appear anywhere in the pool.
        for reserved in ['d', 'x', 'u', 'a', 'e', 't', 'v', 'g', 'k'] {
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
        let spans = agents_row_spans(&agents, &theme);
        let text: String = spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("claude"));
        assert!(text.contains("codex"));
        assert!(text.contains('q'));
        assert!(text.contains('w'));
    }

    #[test]
    fn layout_chip_row_uses_padded_chip_width() {
        // Each pinned chip renders as ` N label ` (number + space + label
        // with 1ch padding each side). The clickable rect must match the
        // rendered width so mouse hit-testing lands on the chip's visual
        // bounds, padding included.
        let area = ratatui::layout::Rect::new(0, 0, 80, 1);
        let pinned = cmds(&[("pr", "/pr"), ("feedback", "/fb")]);
        let rects = layout_chip_row(area, &pinned);
        assert_eq!(rects.len(), 2);
        // " 1 pr " = 6 cells
        assert_eq!(rects[0].width, 6);
        // " 2 feedback " = 12 cells
        assert_eq!(rects[1].width, 12);
        // 2-cell gap between chips
        assert_eq!(rects[1].x, rects[0].x + rects[0].width + 2);
    }
}
