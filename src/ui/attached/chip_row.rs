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
