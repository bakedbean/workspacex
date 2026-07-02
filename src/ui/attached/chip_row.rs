//! Extracted from ui/attached.rs.

use super::*;
use crate::ui::dashboard::status::Status;

/// Build the right-justified PR chip's display text and style for the chip
/// row, mirroring the dashboard detail header (`{glyph} #{n} {label}`).
/// `None` when there's no PR or the lifecycle has no glyph (e.g. `NoPr`).
fn pr_chip_parts(pr: Option<(BranchLifecycle, u32)>, theme: &Theme) -> Option<(String, Style)> {
    let (lc, number) = pr?;
    let (glyph, label) = crate::ui::dashboard::detail::lifecycle_chip(lc);
    if glyph.is_empty() {
        return None;
    }
    let style = theme
        .lifecycle_style(Some(lc))
        .unwrap_or_else(|| theme.dim_style());
    Some((format!("{glyph} #{number} {label}"), style))
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

/// Render the pinned-command chip row, returning each chip's clickable rect.
///
/// A right-justified info block — the running-process count (`● Np`), the
/// `diff` count (`+A −R`), then the PR chip (`{glyph} #{n} {label}`, mirroring
/// the dashboard detail header) — is painted flush to the row's right edge with
/// the inline rule stopping short of it. Every element is optional: the procs
/// count and diff each render on their own, and a zero procs / clean-or-absent
/// diff renders nothing. On rows too narrow for the whole block, elements are
/// dropped from the left (procs first, then diff) so the PR — the strongest
/// signal — stays visible longest; the whole block drops when the pinned chips
/// leave no room for it.
///
/// The returned `Rect` is the PR chip's screen rect (for mouse hit-testing),
/// or `None` when no PR chip was painted — the procs and diff counts are not
/// clickable.
pub(crate) fn render_chip_row(
    f: &mut Frame,
    area: Rect,
    pinned: &[PinnedCommand],
    procs: u32,
    diff: Option<crate::git::DiffStats>,
    pr: Option<(BranchLifecycle, u32)>,
    theme: &Theme,
) -> (Vec<Rect>, Option<Rect>) {
    let rects = layout_chip_row(area, pinned);
    let label_style = Style::default().fg(theme.path);
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(rects.len() * 5 + 2);
    let mut used: usize = 0;
    for (i, (_rect, cmd)) in rects.iter().zip(pinned.iter()).enumerate() {
        if i > 0 {
            spans.push(Span::raw("  ".to_string()));
            used += 2;
        }
        let label = truncate_label(&cmd.label, 12);
        let chip_text = format!("{}", i + 1);
        used += 2 + chip_text.chars().count();
        spans.extend(key_pill_spans(&chip_text, theme));
        let label_with_lead = format!(" {label}");
        used += label_with_lead.chars().count();
        spans.push(Span::styled(label_with_lead, label_style));
    }
    // Right-justified info block: procs count (`● Np`), diff count (`+A −R`),
    // then the PR chip, in that left-to-right order, flush to the row's right
    // edge. The PR chip is the rightmost element; each present element is
    // separated from its neighbour by one space. The inline rule below stops a
    // 2-cell gap short of the whole block. When the row is too narrow for the
    // whole block, elements are dropped from the LEFT (procs first, then diff)
    // so the PR — the most important signal — stays visible longest; the block
    // is dropped entirely when the pinned chips leave less than the 2-cell gap,
    // so it never overlaps.
    let width = area.width as usize;
    let pr_parts = pr_chip_parts(pr, theme);
    let pr_width = pr_parts
        .as_ref()
        .map(|(text, _)| text.chars().count())
        .unwrap_or(0);

    // The optional elements in left-to-right order. Each is `(spans, width)`.
    let mut elements: Vec<(Vec<Span<'static>>, usize)> = Vec::with_capacity(3);
    if let Some(parts) = procs_chip_parts(procs, theme) {
        elements.push(parts);
    }
    if let Some(parts) = diff_chip_parts(diff, theme) {
        elements.push(parts);
    }
    if let Some((text, style)) = pr_parts {
        elements.push((vec![Span::styled(text, style)], pr_width));
    }

    // Width of the elements from `start` onward, joined by single-space gaps.
    let block_width_from = |els: &[(Vec<Span<'static>>, usize)]| -> usize {
        if els.is_empty() {
            0
        } else {
            els.iter().map(|(_, w)| w).sum::<usize>() + (els.len() - 1)
        }
    };
    // Drop leftmost elements until the block plus its 2-cell rule gap fits.
    let mut start = 0;
    while start < elements.len() && used + 2 + block_width_from(&elements[start..]) > width {
        start += 1;
    }
    let block_width = block_width_from(&elements[start..]);
    // The PR chip, when present, is the rightmost element and is only dropped
    // once the whole block collapses — so its flush-right click rect is live
    // exactly when a non-empty block remains.
    let pr_rect = if pr_width > 0 && block_width > 0 {
        pr_chip_rect(area, pr_width as u16)
    } else {
        None
    };

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
    // (procs, diff, PR) left-to-right, one space between adjacent ones.
    if block_width > 0 {
        let pad = width.saturating_sub(used + block_width);
        if pad > 0 {
            spans.push(Span::raw(" ".repeat(pad)));
        }
        for (i, (el_spans, _)) in elements.into_iter().skip(start).enumerate() {
            if i > 0 {
                spans.push(Span::raw(" ".to_string()));
            }
            spans.extend(el_spans);
        }
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
    (rects, pr_rect)
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
                let (_chips, r) = render_chip_row(
                    f,
                    area,
                    &pinned,
                    0,
                    None,
                    Some((BranchLifecycle::PrOpen, 152)),
                    &theme,
                );
                pr_rect = r;
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
                let (_chips, r) = render_chip_row(
                    f,
                    area,
                    &pinned,
                    0,
                    None,
                    Some((BranchLifecycle::PrOpen, 152)),
                    &theme,
                );
                pr_rect = r;
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
                let (_chips, r) = render_chip_row(
                    f,
                    area,
                    &pinned,
                    0,
                    Some(crate::git::DiffStats {
                        added: 12,
                        removed: 3,
                    }),
                    Some((BranchLifecycle::PrOpen, 152)),
                    &theme,
                );
                pr_rect = r;
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
                let (_chips, r) = render_chip_row(
                    f,
                    area,
                    &pinned,
                    3,
                    Some(crate::git::DiffStats {
                        added: 12,
                        removed: 3,
                    }),
                    Some((BranchLifecycle::PrOpen, 152)),
                    &theme,
                );
                pr_rect = r;
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
                render_chip_row(f, area, &pinned, 2, None, None, &theme);
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
                render_chip_row(f, area, &pinned, 0, None, None, &theme);
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
                let (_chips, r) = render_chip_row(
                    f,
                    area,
                    &pinned,
                    9,
                    None,
                    Some((BranchLifecycle::PrOpen, 9)),
                    &theme,
                );
                pr_rect = r;
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
}
