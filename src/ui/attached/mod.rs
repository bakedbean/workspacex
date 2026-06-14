use crate::commands::pinned::{PinnedCommand, truncate_label};
use crate::data::store::AgentInstanceId;
use crate::git::forge::BranchLifecycle;
use crate::pty::render::render_screen;
use crate::pty::session::{AgentKind, Session};
use crate::ui::footer::{FooterHintAction, FooterHintSpan, key_for_glyph};
use crate::ui::split::{Divider, SplitDirection};
use crate::ui::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::widgets::Paragraph;
use std::sync::Arc;

mod agents_row;
mod chip_row;
mod footer;

// render_panes (below) draws the chrome rows from these submodules.
use agents_row::{agents_row_spans, layout_agents_row};
use footer::footer_line;
// Re-exported for app::render / app::input via `crate::ui::attached::*`.
pub use agents_row::agent_switch_keys;
pub(crate) use chip_row::render_chip_row;

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
    /// Clickable rect of the right-justified PR chip on the chip row, or `None`
    /// when the focused workspace has no PR (or the chip didn't fit). Consumed
    /// by the input handler to open the PR in the browser on click.
    pub pr_link_rect: Option<Rect>,
    /// `(session, terminal content rect)` for each rendered pane.
    pub pane_rects: Vec<(Arc<Session>, Rect)>,
    /// `(instance id, clickable rect)` for each agent pill in the footer
    /// agents row. Empty when the row isn't shown. Consumed by the input
    /// handler to retarget the focused pane on click.
    pub agent_chip_rects: Vec<(AgentInstanceId, Rect)>,
    /// `(clickable_rect, action)` for each footer keybind hint (including the
    /// `^x` leader pill). Consumed by the input handler to fire the matching
    /// key on click.
    pub footer_hint_rects: Vec<(Rect, crate::ui::footer::FooterHintAction)>,
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
    diff: Option<crate::git::DiffStats>,
    pr: Option<(BranchLifecycle, u32)>,
    agents: &[(AgentInstanceId, AgentKind, String, Option<char>)],
    active_agent: Option<AgentInstanceId>,
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
    let (footer_keys_line, footer_hints) =
        footer_line(footer_label, footer_agent, multi_pane_footer, theme);
    // Keys render on the footer's second row, so hint rects are anchored there.
    let footer_keys_row = footer_area.y.saturating_add(1);
    let footer_hint_rects =
        crate::ui::dashboard::footer_hint_rects(footer_area, footer_keys_row, &footer_hints);
    let footer_text = ratatui::text::Text::from(vec![
        Line::from(Vec::<Span<'static>>::new()),
        footer_keys_line,
    ]);
    f.render_widget(Paragraph::new(footer_text), footer_area);

    // Chips + inline rule filler + right-justified info block (diff count and,
    // when present, the PR chip). Always renders so the rule shows even when
    // there are no pinned commands.
    let (chip_rects, pr_link_rect) = render_chip_row(f, chip_area, pinned, diff, pr, theme);

    // Agents row: only rendered when the workspace has more than its primary
    // agent. Each pill's clickable rect is computed alongside the spans so the
    // input handler can retarget the focused pane on click.
    let agent_chip_rects: Vec<(AgentInstanceId, Rect)> = if agents.is_empty() {
        Vec::new()
    } else {
        let spans = agents_row_spans(agents, active_agent, theme);
        f.render_widget(Paragraph::new(Line::from(spans)), agents_area);
        let rects = layout_agents_row(agents_area, agents);
        agents.iter().map(|(id, _, _, _)| *id).zip(rects).collect()
    };

    PanesDrawOutput {
        chip_rects,
        pr_link_rect,
        pane_rects,
        agent_chip_rects,
        footer_hint_rects,
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

/// The footer/chip "key pill" style: a dim, bold glyph on the soft chip
/// background. Shared by the footer keybinds, the pinned-chip row, and the
/// agents row so every pill reads identically.
fn key_pill_style(theme: &Theme) -> Style {
    Style::default()
        .fg(theme.dim)
        .add_modifier(Modifier::BOLD)
        .bg(theme.bg_soft)
}

/// The three spans forming one key pill: a 1-cell pad, the `key` glyph in
/// [`key_pill_style`], and a trailing 1-cell pad — all on the chip background.
/// Width is always `2 + key.chars().count()`. Callers append any label tail
/// themselves (the agents row has none; the footer/chip rows do).
fn key_pill_spans(key: &str, theme: &Theme) -> [Span<'static>; 3] {
    let pad_style = theme.chip_bg_style();
    [
        Span::styled(" ".to_string(), pad_style),
        Span::styled(key.to_string(), key_pill_style(theme)),
        Span::styled(" ".to_string(), pad_style),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
