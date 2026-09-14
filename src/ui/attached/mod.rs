use crate::commands::pinned::{PinnedCommand, truncate_label};
use crate::data::store::AgentInstanceId;
use crate::git::forge::BranchLifecycle;
use crate::pty::render::render_screen;
use crate::pty::session::{AgentKind, Session};
use crate::ui::split::{Divider, SplitDirection};
use crate::ui::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::widgets::Paragraph;
use std::sync::Arc;

mod agents_row;
pub(crate) mod chip_row;
mod nav_menu;

// Re-exported for app::render / app::input via `crate::ui::attached::*`.
pub use agents_row::agent_switch_keys;
pub(crate) use chip_row::ChipPr;
pub use nav_menu::{NavItem, nav_item_key, nav_menu_items, render_nav_overlay};

/// One pane in the attached view: a workspace's PTY plus its label,
/// the rect it occupies, and whether it's the focused pane (cursor + chip
/// chrome). For the single-pane case the slice has one entry; for vim-style
/// splits there's one entry per leaf.
pub struct PaneSpec<'a> {
    pub session: &'a Arc<Session>,
    pub label: &'a str,
    pub rect: Rect,
    pub focused: bool,
    /// The pane's coding agent, or `None` for a label-only pane (no agent
    /// kind).
    pub agent: Option<AgentKind>,
}

/// What `render_panes` reports back to the caller for input hit-testing.
#[derive(Default)]
pub struct PanesDrawOutput {
    /// Clickable rects of the pinned-command chips (same as before).
    pub chip_rects: Vec<Rect>,
    /// Clickable rect of the right-justified PR chip on the chip row, or `None`
    /// when the focused workspace has no PR (or the chip didn't fit). Consumed
    /// by the input handler to open the PR in the browser on click.
    pub pr_link_rect: Option<Rect>,
    /// Clickable rect of the running-process count (`● Np`) on the chip row, or
    /// `None` when the focused workspace has no running processes (or the count
    /// was dropped on a narrow row). Consumed by the input handler to open the
    /// process-list modal on click.
    pub procs_link_rect: Option<Rect>,
    /// `(session, terminal content rect)` for each rendered pane.
    pub pane_rects: Vec<(Arc<Session>, Rect)>,
    /// `(instance id, clickable rect)` for each agent pill in the chip row's
    /// flush-right block. Empty when no pills are shown. Consumed by the input
    /// handler to retarget the focused pane on click.
    pub agent_chip_rects: Vec<(AgentInstanceId, Rect)>,
    /// `(clickable_rect, action)` for each footer keybind hint (including the
    /// `^x` leader pill). Consumed by the input handler to fire the matching
    /// key on click.
    pub footer_hint_rects: Vec<(Rect, crate::ui::footer::FooterHintAction)>,
    /// `(workspace, rect)` per attention entry on the info line.
    pub attention_rects: Vec<(crate::data::store::WorkspaceId, Rect)>,
    /// Rect of the `… +N more` tail, when present.
    pub attention_more_rect: Option<Rect>,
}

/// Route one bar's hits into the output the input handlers read. Called for
/// the top bar and (from Task 9 on) the bottom bar, so a segment moved
/// between bars keeps its click.
fn route_hits(area: Rect, hits: &[crate::ui::bar::segment::HitSpan], out: &mut PanesDrawOutput) {
    use crate::ui::bar::segment::Hit;
    for (rect, hit) in crate::ui::bar::render::hit_rects(area, hits) {
        match hit {
            Hit::PinnedChip(_) => out.chip_rects.push(rect),
            Hit::Pr => out.pr_link_rect = Some(rect),
            Hit::Procs => out.procs_link_rect = Some(rect),
            Hit::Agent(id) => out.agent_chip_rects.push((id, rect)),
            Hit::ArmLeader => out
                .footer_hint_rects
                .push((rect, crate::ui::footer::FooterHintAction::ArmLeader)),
            Hit::Key(k) => out
                .footer_hint_rects
                .push((rect, crate::ui::footer::FooterHintAction::Key(k))),
            Hit::Attention(id) => out.attention_rects.push((id, rect)),
            Hit::AttentionMore => out.attention_more_rect = Some(rect),
            Hit::UsageGraph => {}
        }
    }
}

/// Render one or more attached panes plus the shared chrome (info line,
/// separator, chip row). Returns a [`PanesDrawOutput`]:
/// the per-chip clickable rects plus each pane's `(session, content rect)`,
/// both consumed by the input handler for mouse hit-testing.
///
/// Layout (top to bottom):
///   - one row of focused workspace label + cross-workspace attention status,
///   - a `─` separator rule beneath it,
///   - the pane area, subdivided per `panes[i].rect` (which the caller
///     pre-computed from `SplitTree::layout`),
///   - one row of pinned-command chips / `^x` menu hint, with the agent
///     pills (only when the workspace has extra agents) and workspace stats
///     right-justified.
///
/// When there are multiple panes, each pane also gets a 1-row title bar
/// at the top of its rect showing the workspace name and a focus marker.
/// Single-pane mode skips the title bar so it looks identical to the
/// previous single-attached view.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_panes(
    f: &mut Frame,
    panes: &[PaneSpec<'_>],
    dividers: &[Divider],
    info_area: Rect,
    separator_area: Rect,
    chip_area: Rect,
    specs: &crate::config::theme_file::BarSpecs,
    repo: &str,
    name: &str,
    agent: Option<AgentKind>,
    attention: Option<crate::ui::updates_bar::AttentionLine>,
    pinned: &[PinnedCommand],
    procs: u32,
    diff: Option<crate::git::DiffStats>,
    pr: Option<ChipPr>,
    model_tokens: Option<crate::ui::detail_modules::session_summary::ChipModelTokens>,
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

    let mut out = PanesDrawOutput::default();

    // Both the info line (top) and the chip row (bottom) render from one
    // shared segment map, so a segment that appears in either bar's format
    // — or moves between them — renders identically and keeps its click.
    let (top, bottom) = crate::ui::bar::attached_bars(
        specs,
        theme,
        crate::ui::bar::AttachedInputs {
            repo,
            name,
            agent,
            attention,
            pinned,
            procs,
            diff,
            pr,
            model_tokens,
            agents,
            active_agent,
        },
        info_area.width,
        chip_area.width,
    );
    f.render_widget(Paragraph::new(top.line), info_area);
    route_hits(info_area, &top.hits, &mut out);
    if separator_area.width > 0 {
        let rule = "─".repeat(separator_area.width as usize);
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(rule, theme.dim_style()))),
            separator_area,
        );
    }

    // Chip row (bottom): `^x menu` hint + pinned chips left, the stats
    // block (agents, model+tokens, procs, diff, PR) flush right.
    f.render_widget(Paragraph::new(bottom.line), chip_area);
    route_hits(chip_area, &bottom.hits, &mut out);

    out.pane_rects = pane_rects;
    out
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

/// Carve the attached view's `area` into info-line / separator / pane / chip
/// sub-areas. The info line hosts the focused workspace label + attention
/// items and sits at the TOP, with a 1-cell `─` separator rule beneath it to
/// set it off from the pane content. The chip row is the bottom row; the agent
/// pills share it, so the chrome is always three rows.
/// Returns `(info, separator, pane, chip)`.
pub fn layout_chrome(area: Rect) -> (Rect, Rect, Rect, Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // info line (label + attention)
            Constraint::Length(1), // separator rule
            Constraint::Min(1),    // pane area
            Constraint::Length(1), // chip row
        ])
        .split(area);
    (chunks[0], chunks[1], chunks[2], chunks[3])
}

/// Resize a session's PTY to fill its pane area (minus a per-pane title
/// row when `multi_pane` is true).
pub fn resize_pane(session: &Arc<Session>, pane_rect: Rect, multi_pane: bool) {
    let title: u16 = if multi_pane { 1 } else { 0 };
    let _ = session.resize(pane_rect.width, pane_rect.height.saturating_sub(title));
}

/// Width in columns of the info line's leading `[agent-bar ]label   ` prefix,
/// before the attention items begin. Shared by `render.rs` (to shrink the
/// attention width budget and offset its click rects) and the engine's
/// `attached_top` bar (which renders the same prefix) so the two never
/// disagree.
pub fn info_line_prefix_width(label: &str, agent: Option<AgentKind>) -> u16 {
    let bar = if agent.is_some() { 2 } else { 0 }; // "▎" + " "
    // Cells, not chars: a double-width glyph in a workspace name would
    // otherwise shift every attention click rect one column left.
    bar + Span::raw(label).width() as u16 + 3 // 3-col gap before attention
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
/// background. Shared by the nav overlay and the pinned-chip row so every
/// pill reads identically.
fn key_pill_style(theme: &Theme) -> Style {
    Style::default()
        .fg(theme.dim)
        .add_modifier(Modifier::BOLD)
        .bg(theme.bg_soft)
}

/// The three spans forming one key pill: a 1-cell pad, the `key` glyph in
/// [`key_pill_style`], and a trailing 1-cell pad — all on the chip background.
/// Width is always `2 + key.chars().count()`. Callers append any label tail
/// themselves (the nav overlay has none; the pinned-chip row does).
fn key_pill_spans(key: &str, theme: &Theme) -> [Span<'static>; 3] {
    let pad_style = theme.chip_bg_style();
    [
        Span::styled(" ".to_string(), pad_style),
        Span::styled(key.to_string(), key_pill_style(theme)),
        Span::styled(" ".to_string(), pad_style),
    ]
}

/// Paint pinned-command chips only, with no right-justified stats block.
/// Used by the dashboard detail pane's chip row, which has no procs/diff/PR
/// data of its own to show (those live in the header strip instead) — the
/// attached view's own chip row renders through the bar engine instead (see
/// `crate::ui::bar::attached_bars`). Returns each chip's clickable rect.
pub(crate) fn render_pinned_chip_row(
    f: &mut Frame,
    area: Rect,
    pinned: &[PinnedCommand],
    theme: &Theme,
) -> Vec<Rect> {
    let rects = chip_row::layout_chip_row(area, pinned);
    let label_style = Style::default().fg(theme.path);
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(rects.len() * 5 + 2);
    let mut used: usize = 0;
    for (i, (_rect, cmd)) in rects.iter().zip(pinned.iter()).enumerate() {
        if i > 0 {
            spans.push(Span::raw("  ".to_string()));
            used += 2;
        }
        let label = truncate_label(&cmd.label, chip_row::CHIP_LABEL_COLS);
        let chip_text = format!("{}", i + 1);
        used += 2 + chip_text.chars().count();
        spans.extend(key_pill_spans(&chip_text, theme));
        let label_with_lead = format!(" {label}");
        used += label_with_lead.chars().count();
        spans.push(Span::styled(label_with_lead, label_style));
    }
    // Trailing dim rule, same treatment as the attached chip row's fill,
    // so the pinned chips read consistently in both places.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn attached_bars_top(
        specs: &crate::config::theme_file::BarSpecs,
        theme: &Theme,
        repo: &str,
        name: &str,
        agent: Option<AgentKind>,
        attention: Option<crate::ui::updates_bar::AttentionLine>,
        width: u16,
    ) -> crate::ui::bar::render::Rendered {
        crate::ui::bar::attached_bars(
            specs,
            theme,
            crate::ui::bar::AttachedInputs {
                repo,
                name,
                agent,
                attention,
                pinned: &[],
                procs: 0,
                diff: None,
                pr: None,
                model_tokens: None,
                agents: &[],
                active_agent: None,
            },
            width,
            width,
        )
        .0
    }

    /// The bottom row's fixture inputs: two pinned commands, a diff, a PR
    /// with a verdict, model+token usage, and two agents. Shared by the
    /// parity test and the cross-bar routing test.
    #[allow(clippy::type_complexity)]
    fn bottom_fixture() -> (
        Vec<crate::commands::pinned::PinnedCommand>,
        Option<crate::git::DiffStats>,
        Option<ChipPr>,
        Option<crate::ui::detail_modules::session_summary::ChipModelTokens>,
        Vec<(AgentInstanceId, AgentKind, String, Option<char>)>,
    ) {
        use crate::git::forge::{BranchLifecycle, ReviewDecision};
        let pinned = vec![
            crate::commands::pinned::PinnedCommand {
                label: "PR".into(),
                command: "/pr".into(),
            },
            crate::commands::pinned::PinnedCommand {
                label: "feedback".into(),
                command: "/fb".into(),
            },
        ];
        let diff = Some(crate::git::DiffStats {
            added: 12,
            removed: 3,
        });
        let pr = Some(ChipPr {
            lifecycle: BranchLifecycle::PrOpen,
            number: 42,
            review: Some(ReviewDecision::Approved),
            unresolved: Some(2),
        });
        let mt = Some(
            crate::ui::detail_modules::session_summary::ChipModelTokens {
                model: Some("opus 4.8".into()),
                tokens: "45k/200k".into(),
                warn: false,
            },
        );
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
        (pinned, diff, pr, mt, agents)
    }

    /// Proof that a segment routes to the right `PanesDrawOutput` field no
    /// matter which bar's format it appears in: with the PR chip moved to
    /// the top bar's format and dropped from the bottom bar's, its rect
    /// still lands in `pr_link_rect` — now on the info row — while the
    /// pinned chips (left where they normally are) still land in
    /// `chip_rects` on the chip row. `route_hits` is called once per bar,
    /// so a segment moved between bars keeps its click rather than being
    /// silently dropped.
    #[test]
    fn cross_bar_click_routing_pr_moves_to_top_pins_stay_bottom() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let theme = Theme::wsx();
        let mut specs = crate::config::theme_file::bundled_default(&theme);
        specs.attached_top.format = crate::ui::bar::format::parse("$pr").unwrap();
        specs.attached_bottom.right_format = crate::ui::bar::format::parse("").unwrap();
        let (pinned, diff, pr, mt, agents) = bottom_fixture();
        let (w, h) = (120u16, 4u16);
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        let mut out_result = None;
        term.draw(|f| {
            let (info, sep, _pane, chip) = layout_chrome(Rect::new(0, 0, w, h));
            out_result = Some(render_panes(
                f,
                &[],
                &[],
                info,
                sep,
                chip,
                &specs,
                "wsx",
                "foo",
                None,
                None,
                &pinned,
                3,
                diff,
                pr,
                mt,
                &agents,
                Some(AgentInstanceId(1)),
                &theme,
            ));
        })
        .unwrap();
        let out = out_result.unwrap();

        let pr_rect = out.pr_link_rect.expect("pr chip moved to the top bar");
        assert_eq!(pr_rect.y, 0, "pr now renders on the info row");
        assert!(pr_rect.x + pr_rect.width <= w);

        assert_eq!(
            out.chip_rects.len(),
            2,
            "pins still render on the bottom row"
        );
        for rect in &out.chip_rects {
            assert_eq!(rect.y, 3, "pins stay on the chip row");
        }
    }

    /// Durable evidence for the engine cutover: this snapshot and its hit
    /// tuples were verified byte-for-byte against the legacy `info_line`
    /// builder (temporarily restored in git history for that one check,
    /// then removed again — see the `engine_top_bar_matches_legacy_info_line`
    /// commit history) before `info_line` was deleted.
    #[test]
    fn engine_top_bar_snapshot_with_attention() {
        use crate::ui::bar::segment::Hit;
        use crate::ui::updates_bar::{AttentionLine, AttentionMore, AttentionSegment};
        let theme = Theme::wsx();
        let specs = crate::config::theme_file::bundled_default(&theme);
        let attention = Some(AttentionLine {
            line: Line::from(vec![
                Span::styled("? foo".to_string(), theme.attention_style()),
                Span::raw("  ".to_string()),
                Span::styled("… +2 more".to_string(), theme.dim_style()),
            ]),
            segments: vec![AttentionSegment {
                workspace_id: crate::data::store::WorkspaceId(7),
                start_col: 0,
                width: 5,
            }],
            more: Some(AttentionMore {
                start_col: 7,
                width: 9,
            }),
        });
        let new = attached_bars_top(
            &specs,
            &theme,
            "wsx",
            "foo",
            Some(AgentKind::Claude),
            attention,
            60,
        );
        assert!(
            crate::ui::bar::test_util::plain(&new.line).starts_with("▎ wsx/foo   ? foo  … +2 more"),
            "{:?}",
            crate::ui::bar::test_util::plain(&new.line)
        );
        let prefix = info_line_prefix_width("wsx/foo", Some(AgentKind::Claude));
        let hits: Vec<_> = new
            .hits
            .iter()
            .map(|h| (h.start_col, h.width, h.hit))
            .collect();
        assert_eq!(
            hits,
            vec![
                (
                    prefix,
                    5,
                    Hit::Attention(crate::data::store::WorkspaceId(7))
                ),
                (prefix + 7, 9, Hit::AttentionMore),
            ]
        );
    }

    #[test]
    fn engine_top_bar_label_has_no_slash_without_repo() {
        let theme = Theme::wsx();
        let specs = crate::config::theme_file::bundled_default(&theme);
        let new = attached_bars_top(&specs, &theme, "", "solo", None, None, 20);
        assert_eq!(
            crate::ui::bar::test_util::plain(&new.line).trim_end(),
            "solo"
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
    fn layout_chrome_puts_info_line_on_top_with_separator() {
        let area = Rect::new(0, 0, 80, 24);
        let (info, separator, pane, chip) = layout_chrome(area);
        assert_eq!(info.y, 0);
        assert_eq!(info.height, 1);
        assert_eq!(separator.y, 1);
        assert_eq!(separator.height, 1);
        assert_eq!(pane.y, 2);
        assert_eq!(chip.height, 1);
        assert_eq!(chip.y, 23, "chip row is the bottom row");
        assert_eq!(
            info.height + separator.height + pane.height + chip.height,
            24
        );
    }

    #[test]
    fn render_panes_draws_info_on_top_and_full_width_separator() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let theme = Theme::wsx();
        let specs = crate::config::theme_file::bundled_default(&theme);
        let (w, h) = (40u16, 10u16);
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| {
            let area = Rect::new(0, 0, w, h);
            let (info, separator, _pane, chip) = layout_chrome(area);
            // Empty pane slice → renders only the chrome rows (no live Session).
            render_panes(
                f,
                &[],
                &[],
                info,
                separator,
                chip,
                &specs,
                "wsx",
                "foo",
                None,
                None,
                &[],
                0,
                None,
                None,
                None,
                &[],
                None,
                &theme,
            );
        })
        .unwrap();
        let buf = term.backend().buffer();
        // Row 0 starts with the workspace label.
        let row0: String = (0..w).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        assert!(row0.starts_with("wsx/foo"), "row0={row0:?}");
        // Row 1 is the full-width separator rule.
        let row1: String = (0..w).map(|x| buf[(x, 1)].symbol().to_string()).collect();
        assert_eq!(row1, "─".repeat(w as usize), "separator spans the width");
    }

    #[test]
    fn info_line_prefix_width_counts_cells_not_chars() {
        // "日本" is 2 chars but 4 cells; the attention click rects are
        // offset by this width, so it must be measured in cells.
        let wide = info_line_prefix_width("r/日本", Some(AgentKind::Claude));
        let narrow = info_line_prefix_width("r/ab", Some(AgentKind::Claude));
        assert_eq!(wide, narrow + 2);
    }

    #[test]
    fn prefix_width_matches_drawn_prefix() {
        use crate::ui::updates_bar::AttentionLine;
        let theme = Theme::wsx();
        let specs = crate::config::theme_file::bundled_default(&theme);
        let attention = Some(AttentionLine {
            line: Line::from(vec![Span::raw("ATTN".to_string())]),
            segments: vec![],
            more: None,
        });
        let prefix = info_line_prefix_width("wsx/foo", Some(AgentKind::Claude)) as usize;
        let out = attached_bars_top(
            &specs,
            &theme,
            "wsx",
            "foo",
            Some(AgentKind::Claude),
            attention,
            60,
        );
        let buf = crate::ui::bar::test_util::render_line(&out.line, 60);
        let cols: Vec<String> = (0..60).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        assert_eq!(cols[prefix..prefix + 4].concat(), "ATTN", "cols={cols:?}");
    }

    #[test]
    fn top_bar_is_label_only_without_attention() {
        let theme = Theme::wsx();
        let specs = crate::config::theme_file::bundled_default(&theme);
        let out = attached_bars_top(&specs, &theme, "wsx", "foo", None, None, 20);
        assert_eq!(
            crate::ui::bar::test_util::plain(&out.line).trim_end(),
            "wsx/foo"
        );
    }
}
