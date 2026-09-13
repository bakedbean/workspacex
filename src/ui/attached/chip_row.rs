//! Extracted from ui/attached.rs.

use super::agents_row::{AGENT_PILL_GAP, agent_pills_spans, agent_pills_width, layout_agent_pills};
use super::*;
use crate::ui::dashboard::status::Status;
use crate::ui::detail_modules::session_summary::ChipModelTokens;

/// Widest a pinned-command chip label gets before it is ellipsised. Wide
/// enough for a slash-command name like `/agent-review`.
pub(crate) const CHIP_LABEL_COLS: usize = 14;

/// The focused pane's PR, as the chip row needs it. A struct rather than a
/// tuple because the review verdict is a third, differently-shaped field and
/// the call sites read better named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChipPr {
    pub lifecycle: BranchLifecycle,
    pub number: u32,
    pub review: Option<crate::git::forge::ReviewDecision>,
    /// Unresolved review-thread count, drawn as digits after the mark.
    pub unresolved: Option<u32>,
}

impl ChipPr {
    /// A PR with no review verdict — the shape most tests want.
    #[cfg(test)]
    fn new(lifecycle: BranchLifecycle, number: u32) -> Self {
        Self {
            lifecycle,
            number,
            review: None,
            unresolved: None,
        }
    }
}

/// Build the right-justified PR chip's spans and column width for the chip
/// row, mirroring the dashboard detail header (`{glyph} #{n} {label} {mark}`).
/// The verdict mark is a separate span so it can carry its own traffic-light
/// color. `None` when there's no PR or the lifecycle has no glyph (e.g. `NoPr`).
fn pr_chip_parts(pr: Option<ChipPr>, theme: &Theme) -> Option<(Vec<Span<'static>>, usize)> {
    let pr = pr?;
    // Laid out inline against the whole row, so the lifecycle word never has
    // to yield to the mark here.
    let chip = crate::ui::theme::pr_chip(
        pr.lifecycle,
        Some(pr.number),
        pr.review,
        pr.unresolved,
        usize::MAX,
    )?;
    let style = theme
        .lifecycle_style(Some(pr.lifecycle))
        .unwrap_or_else(|| theme.dim_style());
    let mut spans = vec![Span::styled(chip.lifecycle_text.clone(), style)];
    if let Some((mark, d)) = chip.mark() {
        spans.push(Span::raw(" ".to_string()));
        spans.push(Span::styled(mark, theme.review_style(d)));
    }
    Some((spans, chip.width()))
}

/// Build the `+A −R` diff-count spans (dashboard colours: green adds, red
/// removes) plus their column width, or `None` when there's nothing to show —
/// no stats, or a clean worktree with zero added/removed lines. Mirrors the
/// dashboard row's diff cell so the two stay in lockstep.
fn diff_chip_parts(
    diff: Option<crate::git::DiffStats>,
    theme: &Theme,
) -> Option<(Vec<Span<'static>>, usize)> {
    let d = diff?;
    if d.added == 0 && d.removed == 0 {
        return None;
    }
    let added_text = format!("+{}", d.added);
    let removed_text = format!("−{}", d.removed);
    let width = added_text.chars().count() + 1 + removed_text.chars().count();
    let spans = vec![
        Span::styled(added_text, theme.ok_style()),
        Span::styled(" ".to_string(), theme.dim_style()),
        Span::styled(removed_text, theme.err_style()),
    ];
    Some((spans, width))
}

/// Build the `● Np` running-process count span plus its column width, or
/// `None` when the workspace has no running processes. Colour matches the
/// dashboard row / detail bar: the `Thinking` status colour when live, and a
/// zero count is hidden entirely (like the dashboard row's faint dot collapses
/// and the diff cell hides at zero), keeping the flush-right block compact.
fn procs_chip_parts(procs: u32, theme: &Theme) -> Option<(Vec<Span<'static>>, usize)> {
    if procs == 0 {
        return None;
    }
    let text = format!("● {procs}p");
    let width = text.chars().count();
    let spans = vec![Span::styled(text, theme.status_style(Status::Thinking))];
    Some((spans, width))
}

/// Build the combined `{model} {tokens}` element (e.g. `opus 4.8 45k/200k`)
/// plus its column width, or `None` when there's no token data. Colors
/// mirror the detail bar's SESSION SUMMARY lines: the model label takes the
/// model hue, and the token fill is a traffic light — ok with headroom,
/// warn near the limit. This is the lowest-priority element in the
/// flush-right block: the first to go on a narrow row.
fn model_tokens_chip_parts(
    model_tokens: Option<ChipModelTokens>,
    theme: &Theme,
) -> Option<(Vec<Span<'static>>, usize)> {
    let chip = model_tokens?;
    let tokens_style = if chip.warn {
        theme.warn_style()
    } else {
        theme.ok_style()
    };
    let mut width = chip.tokens.chars().count();
    let mut spans = Vec::with_capacity(3);
    if let Some(model) = chip.model {
        width += model.chars().count() + 1;
        spans.push(Span::styled(model, theme.model_style()));
        spans.push(Span::styled(" ".to_string(), theme.dim_style()));
    }
    spans.push(Span::styled(chip.tokens, tokens_style));
    Some((spans, width))
}

/// Screen rect where the right-justified PR chip lands within `area`: flush to
/// the row's right edge. `None` when there's no chip or it can't fit the row at
/// all. The caller additionally drops the chip when the pinned chips would
/// leave no gap before it.
fn pr_chip_rect(area: Rect, pr_width: u16) -> Option<Rect> {
    if pr_width == 0 || pr_width > area.width {
        return None;
    }
    Some(Rect {
        x: area.x + area.width - pr_width,
        y: area.y,
        width: pr_width,
        height: 1,
    })
}

/// Compute the clickable Rect for each chip that fits within `area`.
/// Returns one Rect per chip rendered left-to-right; chips that don't fit
/// are dropped from the end. The chip text is ` <N> <label> ` (V5 button
/// treatment: 1ch padding on each side of the `N <label>` core) joined
/// by 2-space gaps. Labels are individually truncated to `CHIP_LABEL_COLS` first.
pub fn layout_chip_row(area: Rect, pinned: &[PinnedCommand]) -> Vec<Rect> {
    let mut rects = Vec::new();
    let mut x = area.x;
    let max_x = area.x.saturating_add(area.width);
    const GAP: u16 = 2;
    for (i, cmd) in pinned.iter().enumerate().take(9) {
        let label = truncate_label(&cmd.label, CHIP_LABEL_COLS);
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

/// The optional elements of the chip row's flush-right block, in the order
/// they are painted left to right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockElement {
    /// The agent pills (`● claude q   ○ codex w`); only present with 2+ agents.
    Agents,
    /// `{model} {n}/{w}` token usage.
    ModelTokens,
    /// `● Np` running-process count.
    Procs,
    /// `+A −R` diff count.
    Diff,
    /// `{glyph} #{n} {label}` PR chip.
    Pr,
}

impl BlockElement {
    /// The order elements are dropped on rows too narrow for the whole block:
    /// first entry goes first. Distinct from the paint order — the agent pills
    /// are painted leftmost but outlive the model + tokens stat, since they're
    /// the row's only navigation affordance; the PR chip is the strongest
    /// signal and goes last.
    const DROP_ORDER: [BlockElement; 5] = [
        BlockElement::ModelTokens,
        BlockElement::Agents,
        BlockElement::Procs,
        BlockElement::Diff,
        BlockElement::Pr,
    ];

    /// Columns painted between this element and the one following it. The
    /// pill group keeps its own inter-pill gap after it so the pills read as
    /// a group distinct from the stats; everything else is one space apart.
    fn gap_after(self) -> usize {
        match self {
            BlockElement::Agents => AGENT_PILL_GAP as usize,
            _ => 1,
        }
    }
}

/// One element of the flush-right block: what it is, its spans, its width.
struct Element {
    kind: BlockElement,
    spans: Vec<Span<'static>>,
    width: usize,
}

/// Column width of `elements` painted left to right with their gaps.
fn block_width(elements: &[Element]) -> usize {
    elements
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let gap = if i + 1 < elements.len() {
                e.kind.gap_after()
            } else {
                0
            };
            e.width + gap
        })
        .sum()
}

/// What [`render_chip_row`] painted, for mouse hit-testing.
#[derive(Debug, Default)]
pub(crate) struct ChipRowOutput {
    /// Clickable rect of each pinned-command chip, left to right.
    pub chip_rects: Vec<Rect>,
    /// Screen rect of the PR chip, or `None` when no PR chip was painted.
    pub pr_rect: Option<Rect>,
    /// Screen rect of the `● Np` procs count, or `None` when not painted.
    pub procs_rect: Option<Rect>,
    /// `(instance id, rect)` per agent pill, in display order. Empty when the
    /// row has no pills (single-agent workspace) or they were dropped for
    /// width. The diff count and model stat are not clickable.
    pub agent_rects: Vec<(AgentInstanceId, Rect)>,
}

/// Render the pinned-command chip row, returning each chip's clickable rect.
///
/// A right-justified info block — the agent pills (`● claude q   ○ codex w`,
/// only when the workspace has more than one agent), the model + token usage
/// (`{model} {n}/{w}`), the running-process count (`● Np`), the `diff` count
/// (`+A −R`), then the PR chip (`{glyph} #{n} {label}`, mirroring the
/// dashboard detail header) — is painted flush to the row's right edge with the
/// inline rule stopping short of it. Every element is optional: each renders
/// on its own, and absent token data, zero procs, or a clean-or-absent diff
/// each render nothing. The model+tokens element shows whenever there's token
/// data — the `{model}` label is dropped when the model is unknown, leaving
/// just `{n}/{w}`. On rows too narrow for the whole block, elements are dropped
/// in [`BlockElement::DROP_ORDER`] (model+tokens, then the agent pills, procs,
/// diff) so the PR — the strongest signal — stays visible longest; the whole
/// block drops when the pinned chips leave no room for it.
///
/// `active_agent` is the instance in the focused pane; its pill gets the
/// filled identity dot.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_chip_row(
    f: &mut Frame,
    area: Rect,
    pinned: &[PinnedCommand],
    procs: u32,
    diff: Option<crate::git::DiffStats>,
    pr: Option<ChipPr>,
    model_tokens: Option<ChipModelTokens>,
    agents: &[(AgentInstanceId, AgentKind, String, Option<char>)],
    active_agent: Option<AgentInstanceId>,
    theme: &Theme,
) -> ChipRowOutput {
    let rects = layout_chip_row(area, pinned);
    let label_style = Style::default().fg(theme.path);
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(rects.len() * 5 + 2);
    let mut used: usize = 0;
    for (i, (_rect, cmd)) in rects.iter().zip(pinned.iter()).enumerate() {
        if i > 0 {
            spans.push(Span::raw("  ".to_string()));
            used += 2;
        }
        let label = truncate_label(&cmd.label, CHIP_LABEL_COLS);
        let chip_text = format!("{}", i + 1);
        used += 2 + chip_text.chars().count();
        spans.extend(key_pill_spans(&chip_text, theme));
        let label_with_lead = format!(" {label}");
        used += label_with_lead.chars().count();
        spans.push(Span::styled(label_with_lead, label_style));
    }
    // Right-justified info block, flush to the row's right edge. The inline
    // rule below stops a 2-cell gap short of the whole block; the block is
    // dropped entirely when the pinned chips leave less than that gap, so it
    // never overlaps them.
    let width = area.width as usize;
    let pr_parts = pr_chip_parts(pr, theme);
    let pr_width = pr_parts.as_ref().map(|(_, w)| *w).unwrap_or(0);

    // The optional elements in paint (left-to-right) order.
    let mut elements: Vec<Element> = Vec::with_capacity(5);
    if !agents.is_empty() {
        elements.push(Element {
            kind: BlockElement::Agents,
            spans: agent_pills_spans(agents, active_agent, theme),
            width: agent_pills_width(agents),
        });
    }
    if let Some((spans, width)) = model_tokens_chip_parts(model_tokens, theme) {
        elements.push(Element {
            kind: BlockElement::ModelTokens,
            spans,
            width,
        });
    }
    if let Some((spans, width)) = procs_chip_parts(procs, theme) {
        elements.push(Element {
            kind: BlockElement::Procs,
            spans,
            width,
        });
    }
    if let Some((spans, width)) = diff_chip_parts(diff, theme) {
        elements.push(Element {
            kind: BlockElement::Diff,
            spans,
            width,
        });
    }
    if let Some((spans, _)) = pr_parts {
        elements.push(Element {
            kind: BlockElement::Pr,
            spans,
            width: pr_width,
        });
    }

    // Drop elements in `DROP_ORDER` until the block plus its 2-cell rule gap
    // fits after the pinned chips.
    while !elements.is_empty() && used + 2 + block_width(&elements) > width {
        let victim = BlockElement::DROP_ORDER
            .iter()
            .copied()
            .find(|k| elements.iter().any(|e| e.kind == *k))
            .expect("a non-empty block has a droppable element");
        elements.retain(|e| e.kind != victim);
    }
    let block_width = block_width(&elements);

    // Click rects for the kept elements, walking the block from its
    // flush-right start column. The PR chip, when kept, is always the
    // rightmost element, so its rect hugs the right edge.
    let mut pr_rect = None;
    let mut procs_rect = None;
    let mut agent_rects = Vec::new();
    let max_x = area.x.saturating_add(area.width);
    let mut x = area.x as usize + width.saturating_sub(block_width);
    for el in &elements {
        match el.kind {
            BlockElement::Agents => {
                let rects = layout_agent_pills(x as u16, area.y, max_x, agents);
                agent_rects = agents.iter().map(|(id, _, _, _)| *id).zip(rects).collect();
            }
            BlockElement::Procs => {
                procs_rect = Some(Rect {
                    x: x as u16,
                    y: area.y,
                    width: el.width as u16,
                    height: 1,
                });
            }
            BlockElement::Pr => {
                pr_rect = pr_chip_rect(area, pr_width as u16);
            }
            BlockElement::ModelTokens | BlockElement::Diff => {}
        }
        x += el.width + el.kind.gap_after();
    }

    // Inline rule filler matching the V5 dashboard repo-header style:
    // 2 spaces (or 0 when there are no chips), then `─` runs to the right edge
    // of the row — or to the gap before the info block when one is present.
    let rule_end = if block_width > 0 {
        width - block_width - 2
    } else {
        width
    };
    if rule_end > used {
        let gap = if used == 0 { 0 } else { 2 };
        let rule_len = rule_end.saturating_sub(used + gap);
        if gap > 0 && rule_len > 0 {
            spans.push(Span::raw(" ".repeat(gap)));
            used += gap;
        }
        if rule_len > 0 {
            spans.push(Span::styled("─".repeat(rule_len), theme.dim_style()));
            used += rule_len;
        }
    }

    // Pad out to the block's flush-right start, then paint the kept elements
    // left-to-right with their gaps.
    if block_width > 0 {
        let pad = width.saturating_sub(used + block_width);
        if pad > 0 {
            spans.push(Span::raw(" ".repeat(pad)));
        }
        let last = elements.len() - 1;
        for (i, el) in elements.into_iter().enumerate() {
            spans.extend(el.spans);
            if i < last {
                spans.push(Span::raw(" ".repeat(el.kind.gap_after())));
            }
        }
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
    ChipRowOutput {
        chip_rects: rects,
        pr_rect,
        procs_rect,
        agent_rects,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::pinned::PinnedCommand;
    use crate::git::forge::ReviewDecision;

    fn cmds(specs: &[(&str, &str)]) -> Vec<PinnedCommand> {
        specs
            .iter()
            .map(|(l, c)| PinnedCommand {
                label: (*l).into(),
                command: (*c).into(),
            })
            .collect()
    }

    fn mt(model: &str, tokens: &str, warn: bool) -> ChipModelTokens {
        ChipModelTokens {
            model: Some(model.to_string()),
            tokens: tokens.to_string(),
            warn,
        }
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

    #[test]
    fn pr_chip_rect_is_flush_right() {
        // The PR chip's clickable rect hugs the right edge of the row so the
        // chip painted by `render_chip_row` (right-padded to the same column)
        // lines up with the mouse hit target.
        let area = ratatui::layout::Rect::new(4, 7, 80, 1);
        let rect = pr_chip_rect(area, 12).expect("chip fits in an 80-wide row");
        assert_eq!(rect.x, 4 + 80 - 12);
        assert_eq!(rect.y, 7);
        assert_eq!(rect.width, 12);
        assert_eq!(rect.height, 1);
    }

    #[test]
    fn pr_chip_rect_dropped_when_wider_than_row() {
        let area = ratatui::layout::Rect::new(0, 0, 10, 1);
        assert!(pr_chip_rect(area, 12).is_none());
        assert!(pr_chip_rect(area, 0).is_none());
    }

    #[test]
    fn render_chip_row_paints_pr_chip_at_its_click_rect() {
        // The painted PR chip must occupy exactly the rect returned for mouse
        // hit-testing — otherwise clicks land next to it. Render into a backend
        // and assert the chip text fills `pr_rect`, flush to the right edge.
        let theme = Theme::wsx();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 1)).unwrap();
        let pinned = cmds(&[("pr", "/pr")]);
        let mut pr_rect = None;
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 80, 1);
                let out = render_chip_row(
                    f,
                    area,
                    &pinned,
                    0,
                    None,
                    Some(ChipPr::new(BranchLifecycle::PrOpen, 152)),
                    None,
                    &[],
                    None,
                    &theme,
                );
                pr_rect = out.pr_rect;
            })
            .unwrap();
        let rect = pr_rect.expect("PR chip present and fits an 80-wide row");
        let buf = terminal.backend().buffer();
        let mut painted = String::new();
        for x in rect.x..rect.x + rect.width {
            painted.push_str(buf[(x, rect.y)].symbol());
        }
        assert_eq!(painted, "⏺ #152 open");
        assert_eq!(rect.x + rect.width, 80, "chip is flush to the right edge");
    }

    #[test]
    fn attached_pr_chip_carries_the_approval_mark() {
        let theme = Theme::wsx();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 1)).unwrap();
        let pinned = cmds(&[("pr", "/pr")]);
        let mut pr_rect = None;
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 80, 1);
                let out = render_chip_row(
                    f,
                    area,
                    &pinned,
                    0,
                    None,
                    Some(ChipPr {
                        lifecycle: BranchLifecycle::PrOpen,
                        number: 152,
                        review: Some(ReviewDecision::ChangesRequested),
                        unresolved: None,
                    }),
                    None,
                    &[],
                    None,
                    &theme,
                );
                pr_rect = out.pr_rect;
            })
            .unwrap();
        let rect = pr_rect.expect("PR chip present and fits an 80-wide row");
        let buf = terminal.backend().buffer();
        let mut painted = String::new();
        for x in rect.x..rect.x + rect.width {
            painted.push_str(buf[(x, rect.y)].symbol());
        }
        assert_eq!(painted, "⏺ #152 open ✗");
        assert_eq!(
            rect.x + rect.width,
            80,
            "chip stays flush to the right edge"
        );
    }

    #[test]
    fn render_chip_row_drops_pr_chip_when_pinned_fill_the_row() {
        // A narrow row whose pinned chips leave no gap must not paint a PR chip
        // (which would overlap them) — `pr_rect` comes back `None`.
        let theme = Theme::wsx();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(16, 1)).unwrap();
        let pinned = cmds(&[("first", "/a"), ("second", "/b")]);
        let mut pr_rect = Some(Rect::new(0, 0, 0, 0));
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 16, 1);
                let out = render_chip_row(
                    f,
                    area,
                    &pinned,
                    0,
                    None,
                    Some(ChipPr::new(BranchLifecycle::PrOpen, 152)),
                    None,
                    &[],
                    None,
                    &theme,
                );
                pr_rect = out.pr_rect;
            })
            .unwrap();
        assert!(pr_rect.is_none());
    }

    #[test]
    fn render_chip_row_paints_diff_just_left_of_pr_chip() {
        // The diff count (`+A −R`, dashboard colours) sits flush-right, one
        // space to the left of the PR chip, mirroring the dashboard's cell.
        let theme = Theme::wsx();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 1)).unwrap();
        let pinned = cmds(&[("pr", "/pr")]);
        let mut pr_rect = None;
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 80, 1);
                let out = render_chip_row(
                    f,
                    area,
                    &pinned,
                    0,
                    Some(crate::git::DiffStats {
                        added: 12,
                        removed: 3,
                    }),
                    Some(ChipPr::new(BranchLifecycle::PrOpen, 152)),
                    None,
                    &[],
                    None,
                    &theme,
                );
                pr_rect = out.pr_rect;
            })
            .unwrap();
        let rect = pr_rect.expect("PR chip present and fits an 80-wide row");
        let buf = terminal.backend().buffer();
        // PR chip stays flush-right, unchanged by the new diff count.
        let mut pr_painted = String::new();
        for x in rect.x..rect.x + rect.width {
            pr_painted.push_str(buf[(x, rect.y)].symbol());
        }
        assert_eq!(pr_painted, "⏺ #152 open");
        // The diff count sits one space left of the PR chip.
        let diff_text = "+12 −3";
        let diff_w = diff_text.chars().count() as u16;
        let diff_start = rect.x - 1 - diff_w;
        let mut diff_painted = String::new();
        for x in diff_start..diff_start + diff_w {
            diff_painted.push_str(buf[(x, rect.y)].symbol());
        }
        assert_eq!(diff_painted, diff_text);
    }

    #[test]
    fn render_chip_row_paints_diff_flush_right_without_pr() {
        // Before a PR exists, the diff count still shows — flush to the right
        // edge, where the PR chip would otherwise sit. This is what makes it
        // update as the agent commits, ahead of any PR.
        let theme = Theme::wsx();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 1)).unwrap();
        let pinned = cmds(&[("pr", "/pr")]);
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 80, 1);
                render_chip_row(
                    f,
                    area,
                    &pinned,
                    0,
                    Some(crate::git::DiffStats {
                        added: 5,
                        removed: 0,
                    }),
                    None,
                    None,
                    &[],
                    None,
                    &theme,
                );
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let diff_text = "+5 −0";
        let diff_w = diff_text.chars().count() as u16;
        let start = 80 - diff_w;
        let mut painted = String::new();
        for x in start..start + diff_w {
            painted.push_str(buf[(x, 0)].symbol());
        }
        assert_eq!(painted, diff_text);
    }

    #[test]
    fn render_chip_row_omits_zero_diff() {
        // A clean worktree (no added/removed lines) shows nothing — the right
        // edge stays blank, matching the dashboard which hides a zero diff.
        let theme = Theme::wsx();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 1)).unwrap();
        let pinned = cmds(&[("pr", "/pr")]);
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 80, 1);
                render_chip_row(
                    f,
                    area,
                    &pinned,
                    0,
                    Some(crate::git::DiffStats {
                        added: 0,
                        removed: 0,
                    }),
                    None,
                    None,
                    &[],
                    None,
                    &theme,
                );
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        // The far-right cell holds a rule dash or blank, never a digit/sign.
        let sym = buf[(79, 0)].symbol().to_string();
        assert!(sym == "─" || sym == " ", "got {sym:?}");
    }

    #[test]
    fn render_chip_row_paints_procs_left_of_diff_and_pr() {
        // The running-process count (`● Np`, dashboard "Thinking" colour) sits
        // leftmost in the flush-right block: procs, then diff, then the PR chip,
        // each separated by one space.
        let theme = Theme::wsx();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 1)).unwrap();
        let pinned = cmds(&[("pr", "/pr")]);
        let mut pr_rect = None;
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 80, 1);
                let out = render_chip_row(
                    f,
                    area,
                    &pinned,
                    3,
                    Some(crate::git::DiffStats {
                        added: 12,
                        removed: 3,
                    }),
                    Some(ChipPr::new(BranchLifecycle::PrOpen, 152)),
                    None,
                    &[],
                    None,
                    &theme,
                );
                pr_rect = out.pr_rect;
            })
            .unwrap();
        let rect = pr_rect.expect("PR chip present and fits an 80-wide row");
        let buf = terminal.backend().buffer();
        // Whole block reads `● 3p +12 −3 ⏺ #152 open`, flush to the right edge.
        let block = "● 3p +12 −3 ⏺ #152 open";
        let block_w = block.chars().count() as u16;
        let start = rect.x + rect.width - block_w;
        let mut painted = String::new();
        for x in start..start + block_w {
            painted.push_str(buf[(x, 0)].symbol());
        }
        assert_eq!(painted, block);
        assert_eq!(rect.x + rect.width, 80, "PR chip stays flush-right");
    }

    #[test]
    fn render_chip_row_shows_procs_without_diff_or_pr() {
        // Before any diff or PR, the procs count still shows on its own, flush
        // to the right edge — the chip row surfaces workspace liveness early.
        let theme = Theme::wsx();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 1)).unwrap();
        let pinned = cmds(&[("pr", "/pr")]);
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 80, 1);
                render_chip_row(f, area, &pinned, 2, None, None, None, &[], None, &theme);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let text = "● 2p";
        let w = text.chars().count() as u16;
        let start = 80 - w;
        let mut painted = String::new();
        for x in start..start + w {
            painted.push_str(buf[(x, 0)].symbol());
        }
        assert_eq!(painted, text);
    }

    #[test]
    fn render_chip_row_returns_procs_click_rect_at_its_paint() {
        // The procs count (`● Np`) must be clickable: `render_chip_row` returns
        // its screen rect, and the painted `● Np` fills exactly that rect so a
        // click lands on the visible glyphs. Here procs sits leftmost in the
        // block `● 3p +12 −3 ⏺ #152 open`, flush-right in an 80-wide row.
        let theme = Theme::wsx();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 1)).unwrap();
        let pinned = cmds(&[("pr", "/pr")]);
        let mut procs_rect = None;
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 80, 1);
                let out = render_chip_row(
                    f,
                    area,
                    &pinned,
                    3,
                    Some(crate::git::DiffStats {
                        added: 12,
                        removed: 3,
                    }),
                    Some(ChipPr::new(BranchLifecycle::PrOpen, 152)),
                    None,
                    &[],
                    None,
                    &theme,
                );
                procs_rect = out.procs_rect;
            })
            .unwrap();
        let rect = procs_rect.expect("procs count present and fits an 80-wide row");
        let buf = terminal.backend().buffer();
        let mut painted = String::new();
        for x in rect.x..rect.x + rect.width {
            painted.push_str(buf[(x, rect.y)].symbol());
        }
        assert_eq!(painted, "● 3p");
    }

    #[test]
    fn render_chip_row_returns_no_procs_rect_when_dropped() {
        // When the row is too narrow to keep the procs count, its click rect is
        // `None` — no phantom hit target survives after the element is dropped.
        let theme = Theme::wsx();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(20, 1)).unwrap();
        let pinned = cmds(&[("pr", "/pr")]);
        let mut procs_rect = Some(Rect::new(0, 0, 0, 0));
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 20, 1);
                let out = render_chip_row(
                    f,
                    area,
                    &pinned,
                    9,
                    None,
                    Some(ChipPr::new(BranchLifecycle::PrOpen, 9)),
                    None,
                    &[],
                    None,
                    &theme,
                );
                procs_rect = out.procs_rect;
            })
            .unwrap();
        assert!(procs_rect.is_none());
    }

    #[test]
    fn render_chip_row_omits_zero_procs() {
        // Zero running processes shows nothing — like the diff cell, the block
        // stays empty rather than painting a `● 0p`.
        let theme = Theme::wsx();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 1)).unwrap();
        let pinned = cmds(&[("pr", "/pr")]);
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 80, 1);
                render_chip_row(f, area, &pinned, 0, None, None, None, &[], None, &theme);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        // The far-right cell holds a rule dash or blank, never the procs dot.
        let sym = buf[(79, 0)].symbol().to_string();
        assert!(sym == "─" || sym == " ", "got {sym:?}");
    }

    #[test]
    fn render_chip_row_drops_procs_before_pr_when_narrow() {
        // On a row too narrow for the whole block, procs is dropped first so the
        // PR chip — the strongest signal — stays visible.
        let theme = Theme::wsx();
        // "⏺ #9 open" is 9 cells; add the "  " rule gap → 11. A ` 1 pr ` chip is
        // 6 cells. Width 20 fits the chip + gap + PR but not a leading `● 9p `.
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(20, 1)).unwrap();
        let pinned = cmds(&[("pr", "/pr")]);
        let mut pr_rect = None;
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 20, 1);
                let out = render_chip_row(
                    f,
                    area,
                    &pinned,
                    9,
                    None,
                    Some(ChipPr::new(BranchLifecycle::PrOpen, 9)),
                    None,
                    &[],
                    None,
                    &theme,
                );
                pr_rect = out.pr_rect;
            })
            .unwrap();
        let rect = pr_rect.expect("PR chip kept when procs is dropped");
        let buf = terminal.backend().buffer();
        let mut pr_painted = String::new();
        for x in rect.x..rect.x + rect.width {
            pr_painted.push_str(buf[(x, 0)].symbol());
        }
        assert_eq!(pr_painted, "⏺ #9 open");
        // No procs dot survived anywhere on the row.
        let row: String = (0..20).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        assert!(!row.contains("● 9p"), "procs should be dropped: {row:?}");
    }

    #[test]
    fn render_chip_row_paints_model_tokens_leftmost() {
        // Without agent pills, the combined model+token element sits leftmost in the flush-right
        // block: model+tokens, then procs, then diff, then the PR chip.
        let theme = Theme::wsx();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 1)).unwrap();
        let pinned = cmds(&[("pr", "/pr")]);
        let mut pr_rect = None;
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 80, 1);
                let out = render_chip_row(
                    f,
                    area,
                    &pinned,
                    3,
                    Some(crate::git::DiffStats {
                        added: 12,
                        removed: 3,
                    }),
                    Some(ChipPr::new(BranchLifecycle::PrOpen, 152)),
                    Some(mt("opus 4.8", "45k/200k", false)),
                    &[],
                    None,
                    &theme,
                );
                pr_rect = out.pr_rect;
            })
            .unwrap();
        let rect = pr_rect.expect("PR chip present and fits an 80-wide row");
        let buf = terminal.backend().buffer();
        let block = "opus 4.8 45k/200k ● 3p +12 −3 ⏺ #152 open";
        let block_w = block.chars().count() as u16;
        let start = rect.x + rect.width - block_w;
        let mut painted = String::new();
        for x in start..start + block_w {
            painted.push_str(buf[(x, 0)].symbol());
        }
        assert_eq!(painted, block);
        assert_eq!(rect.x + rect.width, 80, "PR chip stays flush-right");
    }

    #[test]
    fn render_chip_row_shows_model_tokens_without_others() {
        // Before any procs/diff/PR, the model+token element still shows on its
        // own, flush to the right edge.
        let theme = Theme::wsx();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 1)).unwrap();
        let pinned = cmds(&[("pr", "/pr")]);
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 80, 1);
                render_chip_row(
                    f,
                    area,
                    &pinned,
                    0,
                    None,
                    None,
                    Some(mt("opus 4.8", "45k/200k", false)),
                    &[],
                    None,
                    &theme,
                );
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let text = "opus 4.8 45k/200k";
        let w = text.chars().count() as u16;
        let start = 80 - w;
        let mut painted = String::new();
        for x in start..start + w {
            painted.push_str(buf[(x, 0)].symbol());
        }
        assert_eq!(painted, text);
    }

    #[test]
    fn render_chip_row_drops_model_tokens_first_when_narrow() {
        // On a row too narrow for the whole block, the model+token element is
        // dropped before the PR chip (it is lowest priority).
        let theme = Theme::wsx();
        // "⏺ #9 open" = 9 cells; + "  " rule gap → 11. " 1 pr " chip = 6 cells.
        // Width 20 fits chip + gap + PR but not a leading model+token element.
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(20, 1)).unwrap();
        let pinned = cmds(&[("pr", "/pr")]);
        let mut pr_rect = None;
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 20, 1);
                let out = render_chip_row(
                    f,
                    area,
                    &pinned,
                    0,
                    None,
                    Some(ChipPr::new(BranchLifecycle::PrOpen, 9)),
                    Some(mt("opus 4.8", "45k/200k", false)),
                    &[],
                    None,
                    &theme,
                );
                pr_rect = out.pr_rect;
            })
            .unwrap();
        let rect = pr_rect.expect("PR chip kept when model+tokens is dropped");
        let buf = terminal.backend().buffer();
        let mut pr_painted = String::new();
        for x in rect.x..rect.x + rect.width {
            pr_painted.push_str(buf[(x, 0)].symbol());
        }
        assert_eq!(pr_painted, "⏺ #9 open");
        let row: String = (0..20).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        assert!(
            !row.contains("opus"),
            "model+tokens should be dropped: {row:?}"
        );
    }

    #[test]
    fn render_chip_row_model_tokens_warn_style() {
        // The model part carries the model hue; with the warn flag set, the
        // token part paints in the theme warn color.
        let theme = Theme::wsx();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 1)).unwrap();
        let pinned = cmds(&[("pr", "/pr")]);
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 80, 1);
                render_chip_row(
                    f,
                    area,
                    &pinned,
                    0,
                    None,
                    None,
                    Some(mt("opus 4.8", "190k/200k", true)),
                    &[],
                    None,
                    &theme,
                );
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let text = "opus 4.8 190k/200k";
        let w = text.chars().count() as u16;
        let start = 80 - w;
        assert_eq!(buf[(start, 0)].fg, theme.model_style().fg.unwrap());
        let tokens_start = start + "opus 4.8 ".chars().count() as u16;
        assert_eq!(buf[(tokens_start, 0)].fg, theme.warn_style().fg.unwrap());
    }

    #[test]
    fn render_chip_row_model_tokens_ok_style_when_healthy() {
        // Without the warn flag, the token part paints in the ok color —
        // mirroring the detail bar's traffic-light context line.
        let theme = Theme::wsx();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 1)).unwrap();
        let pinned = cmds(&[("pr", "/pr")]);
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 80, 1);
                render_chip_row(
                    f,
                    area,
                    &pinned,
                    0,
                    None,
                    None,
                    Some(mt("opus 4.8", "45k/200k", false)),
                    &[],
                    None,
                    &theme,
                );
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let text = "opus 4.8 45k/200k";
        let w = text.chars().count() as u16;
        let start = 80 - w;
        assert_eq!(buf[(start, 0)].fg, theme.model_style().fg.unwrap());
        let tokens_start = start + "opus 4.8 ".chars().count() as u16;
        assert_eq!(buf[(tokens_start, 0)].fg, theme.ok_style().fg.unwrap());
    }

    /// Two agents in display order: the primary `claude` (key `q`) and a
    /// `codex` peer (key `w`).
    fn agents() -> Vec<(AgentInstanceId, AgentKind, String, Option<char>)> {
        vec![
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
        ]
    }

    /// The symbols painted on row 0 from `x` for `w` cells, joined.
    fn painted(buf: &ratatui::buffer::Buffer, x: u16, w: u16) -> String {
        (x..x + w)
            .map(|c| buf[(c, 0)].symbol().to_string())
            .collect()
    }

    /// The whole of row 0 as a string.
    fn row0(buf: &ratatui::buffer::Buffer, width: u16) -> String {
        painted(buf, 0, width)
    }

    #[test]
    fn render_chip_row_paints_agent_pills_first_in_the_flush_right_block() {
        // The agent pills open the flush-right block: left of the model +
        // tokens stat, separated from it by the pills' own 3-col gap, with no
        // `agents:` label. Each returned click rect covers exactly the painted
        // pill (dot + label + key pill), and the active agent's dot is filled.
        let theme = Theme::wsx();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 1)).unwrap();
        let pinned = cmds(&[("pr", "/pr")]);
        let mut rects = Vec::new();
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 120, 1);
                let out = render_chip_row(
                    f,
                    area,
                    &pinned,
                    0,
                    None,
                    Some(ChipPr::new(BranchLifecycle::PrOpen, 152)),
                    Some(mt("opus 4.8", "45k/200k", false)),
                    &agents(),
                    Some(AgentInstanceId(1)),
                    &theme,
                );
                rects = out.agent_rects;
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let row = row0(buf, 120);
        assert!(!row.contains("agents:"), "no label: {row:?}");
        assert_eq!(rects.len(), 2, "one rect per agent");
        let (id0, r0) = rects[0];
        let (id1, r1) = rects[1];
        assert_eq!(id0, AgentInstanceId(1));
        assert_eq!(id1, AgentInstanceId(2));
        assert_eq!(painted(buf, r0.x, r0.width), "● claude  q ");
        assert_eq!(painted(buf, r1.x, r1.width), "○ codex  w ");
        assert_eq!(r1.x, r0.x + r0.width + 3, "3-col gap between pills");
        // The model stat starts a 3-col gap after the last pill.
        // Byte offset → character column: the row holds multi-cell glyphs.
        let opus = row
            .find("opus 4.8")
            .map(|b| row[..b].chars().count())
            .expect("model stat painted") as u16;
        assert_eq!(
            opus,
            r1.x + r1.width + 3,
            "pills sit left of the model stat"
        );
        assert!(
            row.ends_with("⏺ #152 open"),
            "PR chip stays flush right: {row:?}"
        );
    }

    #[test]
    fn render_chip_row_drops_model_tokens_before_agent_pills_when_narrow() {
        // Pills outlive the model + tokens stat: on a row too narrow for the
        // whole block, the stat goes first even though the pills sit to its
        // left. Widths: chips 6 + gap 2, pills 24 + gap 3, model 17 + 1,
        // procs 4 + 1, diff 6 + 1, PR 11 → 76 needed; 70 fits only once the
        // model stat (17 + its gap) is gone.
        let theme = Theme::wsx();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(70, 1)).unwrap();
        let pinned = cmds(&[("pr", "/pr")]);
        let mut rects = Vec::new();
        let mut procs_rect = None;
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 70, 1);
                let out = render_chip_row(
                    f,
                    area,
                    &pinned,
                    2,
                    Some(crate::git::DiffStats {
                        added: 12,
                        removed: 3,
                    }),
                    Some(ChipPr::new(BranchLifecycle::PrOpen, 152)),
                    Some(mt("opus 4.8", "45k/200k", false)),
                    &agents(),
                    None,
                    &theme,
                );
                rects = out.agent_rects;
                procs_rect = out.procs_rect;
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let row = row0(buf, 70);
        assert!(!row.contains("opus"), "model stat dropped first: {row:?}");
        assert_eq!(rects.len(), 2, "pills survive: {row:?}");
        assert_eq!(painted(buf, rects[0].1.x, rects[0].1.width), "○ claude  q ");
        assert!(procs_rect.is_some(), "procs count kept");
        assert!(row.ends_with("● 2p +12 −3 ⏺ #152 open"), "{row:?}");
    }

    #[test]
    fn render_chip_row_drops_agent_pills_before_procs_when_narrow() {
        // Narrower still (50 cols): after the model stat, the pills are the
        // next to go, ahead of procs / diff / PR.
        let theme = Theme::wsx();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(50, 1)).unwrap();
        let pinned = cmds(&[("pr", "/pr")]);
        let mut rects = vec![(AgentInstanceId(0), Rect::default())];
        let mut procs_rect = None;
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 50, 1);
                let out = render_chip_row(
                    f,
                    area,
                    &pinned,
                    2,
                    Some(crate::git::DiffStats {
                        added: 12,
                        removed: 3,
                    }),
                    Some(ChipPr::new(BranchLifecycle::PrOpen, 152)),
                    Some(mt("opus 4.8", "45k/200k", false)),
                    &agents(),
                    None,
                    &theme,
                );
                rects = out.agent_rects;
                procs_rect = out.procs_rect;
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let row = row0(buf, 50);
        assert!(rects.is_empty(), "pills dropped: {row:?}");
        assert!(!row.contains("claude"), "no pill painted: {row:?}");
        assert!(procs_rect.is_some(), "procs count outlives the pills");
        assert!(row.ends_with("● 2p +12 −3 ⏺ #152 open"), "{row:?}");
    }

    #[test]
    fn render_chip_row_returns_no_agent_rects_without_agents() {
        let theme = Theme::wsx();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 1)).unwrap();
        let mut rects = vec![(AgentInstanceId(0), Rect::default())];
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 80, 1);
                let out = render_chip_row(
                    f,
                    area,
                    &[],
                    0,
                    None,
                    Some(ChipPr::new(BranchLifecycle::PrOpen, 152)),
                    None,
                    &[],
                    None,
                    &theme,
                );
                rects = out.agent_rects;
            })
            .unwrap();
        assert!(rects.is_empty());
    }

    #[test]
    fn render_chip_row_paints_keyless_pills_past_the_switch_key_pool() {
        // Eleven agents: the pool hands out ten keys, so the eleventh pill is
        // keyless — no ` key ` pill — yet still painted, with a click rect
        // covering exactly its dot + `label `.
        let keys = crate::ui::attached::agent_switch_keys(11);
        let roster: Vec<(AgentInstanceId, AgentKind, String, Option<char>)> = (1..=11)
            .map(|i| {
                let label = if i == 1 {
                    "claude".to_string()
                } else {
                    format!("claude#{i}")
                };
                (
                    AgentInstanceId(i),
                    AgentKind::Claude,
                    label,
                    keys.get(i as usize - 1).copied(),
                )
            })
            .collect();
        assert!(roster[10].3.is_none(), "eleventh agent is keyless");
        let theme = Theme::wsx();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(200, 1)).unwrap();
        let pinned = cmds(&[("pr", "/pr")]);
        let mut rects = Vec::new();
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, 200, 1);
                let out =
                    render_chip_row(f, area, &pinned, 0, None, None, None, &roster, None, &theme);
                rects = out.agent_rects;
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert_eq!(rects.len(), 11, "one rect per agent, keyless included");
        let (id, r) = rects[10];
        assert_eq!(id, AgentInstanceId(11));
        assert_eq!(painted(buf, r.x, r.width), "○ claude#11 ");
        assert_eq!(r.x + r.width, 200, "last pill is flush right");
    }

    #[test]
    fn render_chip_row_keeps_sole_pill_group_at_exact_fit_and_drops_it_one_short() {
        // With the pills as the block's only element: ` 1 pr ` (6) + the
        // 2-cell rule gap + the 26-col group = 34 fits exactly; one column
        // short drops the whole group (rects empty, nothing painted).
        let theme = Theme::wsx();
        let pinned = cmds(&[("pr", "/pr")]);
        for (width, kept) in [(34u16, true), (33u16, false)] {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, 1)).unwrap();
            let mut rects = Vec::new();
            terminal
                .draw(|f| {
                    let area = ratatui::layout::Rect::new(0, 0, width, 1);
                    let out = render_chip_row(
                        f,
                        area,
                        &pinned,
                        0,
                        None,
                        None,
                        None,
                        &agents(),
                        None,
                        &theme,
                    );
                    rects = out.agent_rects;
                })
                .unwrap();
            let row = row0(terminal.backend().buffer(), width);
            if kept {
                assert_eq!(row, " 1  pr  ○ claude  q    ○ codex  w ", "width {width}");
                assert_eq!(rects.len(), 2, "width {width}");
            } else {
                assert!(rects.is_empty(), "width {width}: {row:?}");
                assert!(!row.contains("claude"), "width {width}: {row:?}");
            }
        }
    }
}
