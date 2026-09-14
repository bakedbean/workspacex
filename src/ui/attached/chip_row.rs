//! Extracted from ui/attached.rs.

use super::*;

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

/// Compute the clickable Rect for each chip that fits within `area`.
/// Returns one Rect per chip rendered left-to-right; chips that don't fit
/// are dropped from the end. The chip text is ` <N> <label> ` (V5 button
/// treatment: 1ch padding on each side of the `N <label>` core) joined
/// by 2-space gaps. Labels are individually truncated to `CHIP_LABEL_COLS` first.
///
/// Still needed by the dashboard detail pane's pinned-chip row
/// (`super::render_pinned_chip_row`) — the attached view's own chip row
/// renders through the bar engine instead.
pub(crate) fn layout_chip_row(area: Rect, pinned: &[PinnedCommand]) -> Vec<Rect> {
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
