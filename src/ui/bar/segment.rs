//! Provider spans and click hits, measured in terminal cells relative to the segment.

use super::format::Node;
use super::style::StyleSpec;
use crate::data::store::{AgentInstanceId, WorkspaceId};
use crate::ui::theme::Theme;
use crossterm::event::KeyEvent;
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use std::collections::HashMap;

/// Every action carried by a bar's clickable spans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Key(KeyEvent),
    ArmLeader,
    PinnedChip(usize),
    /// A prompt-tag chip: index into the sorted tag cache
    /// (`App::prompt_tags_cache`), opening the body stage for that tag.
    TagChip(usize),
    /// The trailing `<>` chip: opens the prompt-tag picker.
    TagsManager,
    Pr,
    /// A repo bar's "my open PRs" link: opens the author-filtered PR list
    /// for the repo the bar belongs to.
    RepoPrs,
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
    /// `[repo_name].pad`: the character that fills the name's left pad
    /// (`$pad`). Only `repo_name` takes it; `None` on every other segment.
    pub pad: Option<char>,
    pub format: Vec<Node>,
    pub disabled: bool,
    /// Below 100 the segment is droppable on overflow, lowest first and
    /// from either side; 100 (the default) never drops. See `render_bar`.
    pub priority: u32,
    /// Separator between items of a multi-item segment: a format like
    /// `format`, so it can carry styled runs, but with no item variables
    /// (it sits between items, not inside one).
    pub separator: Vec<Node>,
    /// The overflow tail of a multi-item segment that folds items it can't
    /// fit (`attention`'s ` … +N more`). Its variables are the segment's
    /// `SegmentDef::more_vars`; empty on segments without a tail.
    pub more_format: Vec<Node>,
    /// The tail's own style, patched over `style` like a grade: its
    /// `$style` and `item_*` colours, and the last rendered entry's
    /// `next_*` when the tail follows it. `None` leaves the tail unstyled
    /// and the last entry's `next` absent even when a tail follows.
    pub more_style: Option<StyleSpec>,
    /// Per-position item styles for a multi-item segment: item `i` gets
    /// `styles[i]`, or the last entry once the list runs out, patched over
    /// the provider's default and `style`. Empty grades nothing.
    pub styles: Vec<StyleSpec>,
    /// This segment's own colours: shadows the global `[palette]` and the
    /// theme tokens while the segment renders — both for names in its
    /// format and for the tokens behind its state-derived `$style` — and
    /// nowhere else. Lets a theme darken the lifecycle tints on one light
    /// block without touching the same tints on a dark one.
    pub palette: HashMap<String, Color>,
    /// `[<segment>.symbols]`: a glyph per key from the segment's
    /// `SegmentDef::symbol_keys`, tried before `symbol`. `agent_bar` keys
    /// it by agent kind; `fold` by `expanded`/`folded`.
    pub symbols: Vec<(String, String)>,
}

impl SegmentConfig {
    /// The glyph for `key` in this segment's `symbols` table, if set. An
    /// empty entry is a deliberate override and comes back as `Some("")`.
    pub fn symbol_for(&self, key: &str) -> Option<&str> {
        self.symbols
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, glyph)| glyph.as_str())
    }

    /// The base theme with this segment's palette shadowing its tokens:
    /// what a provider derives its state colours from, so `[pr.palette]
    /// ok = …` reaches `$style` and not only the format's own `fg:ok`.
    pub fn theme(&self, base: &Theme) -> Theme {
        base.shadowed(&self.palette)
    }
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
