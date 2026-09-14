//! Provider spans and click hits, measured in terminal cells relative to the segment.

use super::format::Node;
use super::style::StyleSpec;
use crate::data::store::{AgentInstanceId, WorkspaceId};
use crossterm::event::KeyEvent;
use ratatui::style::Style;
use ratatui::text::Span;
use std::collections::HashMap;

/// Every action carried by a bar's clickable spans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Key(KeyEvent),
    ArmLeader,
    PinnedChip(usize),
    Pr,
    Procs,
    Agent(AgentInstanceId),
    UsageGraph,
    Attention(WorkspaceId),
    AttentionMore,
}

impl Hit {
    /// This hit's footer key-hint action — the two variants a footer's
    /// key-hint row can dispatch (a synthesized key press, or arming the
    /// attached-view leader); `None` for every other hit.
    pub fn footer_action(self) -> Option<crate::ui::footer::FooterHintAction> {
        match self {
            Hit::Key(k) => Some(crate::ui::footer::FooterHintAction::Key(k)),
            Hit::ArmLeader => Some(crate::ui::footer::FooterHintAction::ArmLeader),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitSpan {
    pub start_col: u16,
    pub width: u16,
    pub hit: Hit,
}

#[derive(Debug, Clone, Default)]
pub struct Segment {
    pub spans: Vec<Span<'static>>,
    /// Display cells, saturated at the largest representable terminal width.
    pub width: u16,
    pub hits: Vec<HitSpan>,
}

impl Segment {
    pub fn text(s: impl Into<String>, style: Style) -> Self {
        let mut segment = Self::default();
        segment.push(Span::styled(s.into(), style));
        segment
    }

    pub fn push(&mut self, span: Span<'static>) {
        let width = u16::try_from(span.width()).unwrap_or(u16::MAX);
        self.width = self.width.saturating_add(width);
        self.spans.push(span);
    }

    /// Move another segment's spans and shift its hits to their new columns.
    pub fn append(&mut self, other: Segment) {
        let offset = self.width;
        for hit in other.hits {
            let start_col = offset.saturating_add(hit.start_col);
            let width = hit.width.min(u16::MAX - start_col);
            if width > 0 {
                self.hits.push(HitSpan {
                    start_col,
                    width,
                    hit: hit.hit,
                });
            }
        }
        self.width = self.width.saturating_add(other.width);
        self.spans.extend(other.spans);
    }

    /// Record a hit covering the cells pushed since `start_col`.
    pub fn hit_from(&mut self, start_col: u16, hit: Hit) {
        let width = self.width.saturating_sub(start_col);
        if width > 0 {
            self.hits.push(HitSpan {
                start_col,
                width,
                hit,
            });
        }
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0
    }

    pub fn plain_text(&self) -> String {
        self.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }
}

/// A segment's theme settings, merged over the bundled defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentConfig {
    /// Patched over the provider's state-derived style to form `$style`.
    pub style: StyleSpec,
    pub symbol: Option<String>,
    pub format: Vec<Node>,
    pub disabled: bool,
    /// Lower values are dropped first from the right side on overflow.
    pub priority: u32,
    /// Separator between items of a multi-item segment.
    pub separator: String,
}

pub type SegmentMap = HashMap<String, Segment>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appending_moves_hits_by_display_cells() {
        let mut left = Segment::text("日本", Style::default());
        left.push(Span::raw(" "));
        let mut right = Segment::text("go", Style::default());
        right.hit_from(0, Hit::Pr);
        left.append(right);
        assert_eq!(left.plain_text(), "日本 go");
        assert_eq!(left.width, 7);
        assert_eq!(
            left.hits,
            vec![HitSpan {
                start_col: 5,
                width: 2,
                hit: Hit::Pr
            }]
        );
    }

    #[test]
    fn oversized_spans_saturate_instead_of_wrapping() {
        let mut segment = Segment::text("x".repeat(65_536), Style::default());
        assert_eq!(segment.width, u16::MAX);
        assert!(!segment.is_empty());
        segment.push(Span::raw("日本"));
        assert_eq!(segment.width, u16::MAX);
        segment.hit_from(65_530, Hit::Procs);
        assert_eq!(
            segment.hits,
            vec![HitSpan {
                start_col: 65_530,
                width: 5,
                hit: Hit::Procs
            }]
        );
    }

    #[test]
    fn appended_hits_stop_at_representable_edge() {
        let mut left = Segment::text("x".repeat(65_534), Style::default());
        let mut right = Segment::text("go", Style::default());
        right.hit_from(0, Hit::Pr);
        left.append(right);
        assert_eq!(left.width, u16::MAX);
        assert_eq!(
            left.hits,
            vec![HitSpan {
                start_col: 65_534,
                width: 1,
                hit: Hit::Pr
            }]
        );
    }

    #[test]
    fn empty_or_backwards_hit_ranges_are_not_clickable() {
        let mut segment = Segment::text("x", Style::default());
        segment.hit_from(1, Hit::Pr);
        segment.hit_from(2, Hit::Procs);
        assert!(segment.hits.is_empty());
    }
}
