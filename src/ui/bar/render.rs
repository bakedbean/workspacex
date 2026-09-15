//! Format evaluation, style inheritance, right-side overflow, and click geometry.

use super::format::{self, Node};
use super::segment::{Hit, HitSpan, Segment, SegmentConfig, SegmentMap};
use super::style::{Resolver, StyleSpec};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use std::collections::HashMap;

/// One bar's parsed theme settings.
#[derive(Debug, Clone, PartialEq)]
pub struct BarSpec {
    pub format: Vec<Node>,
    pub right_format: Vec<Node>,
    pub style: StyleSpec,
    /// Repeat the first character by display cells; empty or zero-width
    /// characters use spaces. A partial wide character is padded with spaces.
    pub fill: String,
    pub fill_style: StyleSpec,
}

#[derive(Debug, Clone)]
pub struct Rendered {
    pub line: Line<'static>,
    /// Columns relative to the first cell of the line.
    pub hits: Vec<HitSpan>,
}

/// Inherit the enclosing style underneath a provider's state-derived style.
fn inherit(segment: &Segment, base: Style) -> Segment {
    Segment {
        spans: segment
            .spans
            .iter()
            .map(|span| Span::styled(span.content.clone(), base.patch(span.style)))
            .collect(),
        width: segment.width,
        hits: segment.hits.clone(),
    }
}

/// Evaluate a format. The bool reports whether a variable produced cells,
/// which determines whether enclosing conditional groups survive.
/// Invalid styles fall back to the inherited base; the loader validates them.
pub fn eval(
    nodes: &[Node],
    vars: &SegmentMap,
    resolver: &Resolver,
    base: Style,
) -> (Segment, bool) {
    eval_excluding(nodes, vars, resolver, base, &[])
}

fn eval_excluding(
    nodes: &[Node],
    vars: &SegmentMap,
    resolver: &Resolver,
    base: Style,
    excluded: &[&str],
) -> (Segment, bool) {
    let mut out = Segment::default();
    let mut produced = false;
    for node in nodes {
        match node {
            Node::Text(text) => out.push(Span::styled(text.clone(), base)),
            Node::Var(name) => {
                if let Some(segment) = vars
                    .get(name)
                    .filter(|segment| !segment.is_empty() && !excluded.contains(&name.as_str()))
                {
                    produced = true;
                    out.append(inherit(segment, base));
                }
            }
            Node::Styled(children, spec) => {
                let style = base.patch(resolver.resolve(spec).unwrap_or_default());
                let (inner, has_var) = eval_excluding(children, vars, resolver, style, excluded);
                produced |= has_var;
                out.append(inner);
            }
            Node::Group(children) => {
                let (inner, has_var) = eval_excluding(children, vars, resolver, base, excluded);
                if has_var {
                    produced = true;
                    out.append(inner);
                }
            }
        }
    }
    (out, produced)
}

fn fill_run(fill: &str, cells: u16, trailing_blank: bool) -> String {
    let ch = fill.chars().next().unwrap_or(' ');
    let mut buffer = [0; 4];
    let glyph = ch.encode_utf8(&mut buffer);
    let glyph_width = Span::raw(&*glyph).width();
    let blank_cells = usize::from(trailing_blank);
    let fill_cells = usize::from(cells) - blank_cells;
    if glyph_width == 0 {
        return " ".repeat(usize::from(cells));
    }
    let repeats = fill_cells / glyph_width;
    let spaces = fill_cells % glyph_width + blank_cells;
    let mut out = String::with_capacity(repeats * glyph.len() + spaces);
    for _ in 0..repeats {
        out.push_str(glyph);
    }
    for _ in 0..spaces {
        out.push(' ');
    }
    out
}

/// A segment's effective priority: the default is [`DEFAULT_PRIORITY`], and
/// only something explicitly ranked below it may be dropped on overflow.
fn priority(configs: &HashMap<String, SegmentConfig>, name: &str) -> u32 {
    configs.get(name).map_or(DEFAULT_PRIORITY, |c| c.priority)
}

/// The priority a segment has when its `[segment]` table sets none. Segments
/// at this rank never drop; only an explicitly lower number is droppable.
pub const DEFAULT_PRIORITY: u32 = 100;

/// Align the right side flush right, keeping a blank cell at the right edge
/// of the intervening fill whenever both sides are nonempty.
///
/// When the two sides don't fit, drop the lowest-priority DROPPABLE segment
/// — one whose effective `priority` is below [`DEFAULT_PRIORITY`] — from
/// whichever side it sits on, then reevaluate BOTH sides so conditional
/// groups shed their separators along with it, and repeat until the sides
/// fit or nothing droppable is left. Ties go to the lowest priority first
/// and then to the first occurrence, scanning `format` before
/// `right_format`, depth-first.
///
/// If the sides still don't fit after that (default-priority segments and
/// bare literals can't be thinned), the right side is omitted altogether
/// and the left side is left overlong for the caller to clip.
pub fn render_bar(
    spec: &BarSpec,
    segments: &SegmentMap,
    configs: &HashMap<String, SegmentConfig>,
    width: u16,
    resolver: &Resolver,
) -> Rendered {
    let base = resolver.resolve(&spec.style).unwrap_or_default();
    let (mut left, _) = eval(&spec.format, segments, resolver, base);
    let (mut right, _) = eval(&spec.right_format, segments, resolver, base);
    let fits = |left: &Segment, right: &Segment| {
        usize::from(left.width)
            + usize::from(right.width)
            + usize::from(!left.is_empty() && !right.is_empty())
            <= usize::from(width)
    };
    if !fits(&left, &right) {
        // `format` before `right_format` — the tie-break order.
        let mut names = format::vars(&spec.format);
        names.extend(format::vars(&spec.right_format));
        let mut excluded: Vec<&str> = Vec::new();
        while !fits(&left, &right) {
            let victim = names
                .iter()
                .copied()
                .filter(|name| {
                    !excluded.contains(name)
                        && priority(configs, name) < DEFAULT_PRIORITY
                        && segments
                            .get(*name)
                            .is_some_and(|segment| !segment.is_empty())
                })
                .min_by_key(|name| priority(configs, name));
            let Some(victim) = victim else { break };
            excluded.push(victim);
            left = eval_excluding(&spec.format, segments, resolver, base, &excluded).0;
            right = eval_excluding(&spec.right_format, segments, resolver, base, &excluded).0;
        }
        if !right.is_empty() && !fits(&left, &right) {
            right = Segment::default();
        }
    }

    let gap = width.saturating_sub(left.width.saturating_add(right.width));
    let needs_blank = !left.is_empty() && !right.is_empty();
    let mut out = left;
    if gap > 0 {
        let fill_style = base.patch(resolver.resolve(&spec.fill_style).unwrap_or_default());
        let fill = fill_run(&spec.fill, gap, needs_blank);
        out.push(Span::styled(fill, fill_style));
    }
    out.append(right);
    Rendered {
        line: Line::from(out.spans),
        hits: out.hits,
    }
}

/// Convert line-relative hits to absolute, nonempty screen rects within `area`.
pub fn hit_rects(area: Rect, hits: &[HitSpan]) -> Vec<(Rect, Hit)> {
    if area.height == 0 {
        return Vec::new();
    }
    let max_x = area.x.saturating_add(area.width);
    hits.iter()
        .filter_map(|hit| {
            let x = area.x.saturating_add(hit.start_col);
            let width = hit.width.min(max_x.saturating_sub(x));
            (width > 0).then_some((Rect::new(x, area.y, width, 1), hit.hit))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::Theme;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::{Color, Modifier};

    fn seg(s: &str) -> Segment {
        Segment::text(s, Style::default())
    }

    fn hit_seg(s: &str, hit: Hit) -> Segment {
        let mut segment = seg(s);
        segment.hit_from(0, hit);
        segment
    }

    fn map(entries: &[(&str, Segment)]) -> SegmentMap {
        entries
            .iter()
            .map(|(name, segment)| (name.to_string(), segment.clone()))
            .collect()
    }

    fn spec(left: &str, right: &str, fill: &str) -> BarSpec {
        BarSpec {
            format: format::parse(left).unwrap(),
            right_format: format::parse(right).unwrap(),
            style: StyleSpec::default(),
            fill: fill.to_string(),
            fill_style: StyleSpec::default(),
        }
    }

    fn cfg(priority: u32) -> SegmentConfig {
        SegmentConfig {
            style: StyleSpec::default(),
            symbol: None,
            format: vec![],
            disabled: false,
            priority,
            separator: Vec::new(),
            more_format: Vec::new(),
        }
    }

    fn text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    const KEY: Hit = Hit::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

    #[test]
    fn groups_require_variable_output_not_literal_output() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let nodes = format::parse("a( $b)(literal)c").unwrap();
        for vars in [map(&[]), map(&[("b", seg(""))])] {
            let (out, produced) = eval(&nodes, &vars, &resolver, Style::default());
            assert_eq!(out.plain_text(), "ac");
            assert!(!produced);
        }
        let (out, produced) = eval(
            &nodes,
            &map(&[("b", seg("B"))]),
            &resolver,
            Style::default(),
        );
        assert_eq!(out.plain_text(), "a Bc");
        assert!(produced);
    }

    #[test]
    fn nested_styled_groups_propagate_variable_output() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let nodes = format::parse("(x[($b)](bold))").unwrap();
        assert_eq!(
            eval(&nodes, &map(&[]), &resolver, Style::default())
                .0
                .plain_text(),
            ""
        );
        let (out, produced) = eval(
            &nodes,
            &map(&[("b", hit_seg("B", KEY))]),
            &resolver,
            Style::default(),
        );
        assert_eq!(out.plain_text(), "xB");
        assert!(produced);
        assert!(out.spans[1].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(
            out.hits,
            vec![HitSpan {
                start_col: 1,
                width: 1,
                hit: KEY
            }]
        );
    }

    #[test]
    fn styles_inherit_inward_without_overwriting_provider_foreground() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let nodes = format::parse("[a[b](fg:red)$v](bg:blue)").unwrap();
        let vars = map(&[("v", Segment::text("V", Style::default().fg(Color::Green)))]);
        let (out, _) = eval(&nodes, &vars, &resolver, Style::default());
        assert_eq!(out.spans[0].style, Style::default().bg(Color::Blue));
        assert_eq!(
            out.spans[1].style,
            Style::default().bg(Color::Blue).fg(Color::Red)
        );
        assert_eq!(
            out.spans[2].style,
            Style::default().bg(Color::Blue).fg(Color::Green)
        );
    }

    #[test]
    fn unresolved_style_keeps_inherited_base() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let nodes = format::parse("[x](fg:missing)").unwrap();
        let base = Style::default().bg(Color::Blue);
        let (out, _) = eval(&nodes, &map(&[]), &resolver, base);
        assert_eq!(out.spans[0].style, base);
    }

    #[test]
    fn wide_variable_offsets_are_terminal_cells() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let nodes = format::parse("$w $k").unwrap();
        let (out, _) = eval(
            &nodes,
            &map(&[("w", seg("日本")), ("k", hit_seg("go", KEY))]),
            &resolver,
            Style::default(),
        );
        assert_eq!(out.width, 7);
        assert_eq!(
            out.hits,
            vec![HitSpan {
                start_col: 5,
                width: 2,
                hit: KEY
            }]
        );
    }

    #[test]
    fn decorative_gap_ends_in_a_blank_before_flush_right_hits() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let out = render_bar(
            &spec("$a", "$b", "─"),
            &map(&[("a", seg("left")), ("b", hit_seg("right", Hit::Pr))]),
            &HashMap::new(),
            20,
            &resolver,
        );
        assert_eq!(text(&out.line), "left────────── right");
        assert_eq!(out.line.width(), 20);
        assert_eq!(
            out.hits,
            vec![HitSpan {
                start_col: 15,
                width: 5,
                hit: Hit::Pr
            }]
        );
    }

    #[test]
    fn one_sided_bars_fill_to_edge_without_reserved_blank() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let out = render_bar(
            &spec("$a", "$b", "─"),
            &map(&[("a", seg("left"))]),
            &HashMap::new(),
            10,
            &resolver,
        );
        assert_eq!(text(&out.line), "left──────");
        let out = render_bar(
            &spec("$a", "$b", "─"),
            &map(&[("b", hit_seg("right", Hit::Pr))]),
            &HashMap::new(),
            10,
            &resolver,
        );
        assert_eq!(text(&out.line), "─────right");
        assert_eq!(
            out.hits,
            vec![HitSpan {
                start_col: 5,
                width: 5,
                hit: Hit::Pr
            }]
        );
    }

    #[test]
    fn overflow_drops_lowest_priority_and_its_group_separator() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let configs = HashMap::from([("b".to_string(), cfg(10)), ("c".to_string(), cfg(50))]);
        let vars = map(&[
            ("a", seg("left")),
            ("b", hit_seg("bbbb", Hit::Procs)),
            ("c", hit_seg("cc", Hit::Pr)),
        ]);
        let bar = spec("$a", "($b )$c", " ");
        let out = render_bar(&bar, &vars, &configs, 10, &resolver);
        assert_eq!(text(&out.line), "left    cc");
        assert_eq!(
            out.hits,
            vec![HitSpan {
                start_col: 8,
                width: 2,
                hit: Hit::Pr
            }]
        );
        let out = render_bar(&bar, &vars, &configs, 12, &resolver);
        assert_eq!(text(&out.line), "left bbbb cc");
        assert_eq!(
            out.hits[0],
            HitSpan {
                start_col: 5,
                width: 4,
                hit: Hit::Procs
            }
        );
    }

    #[test]
    fn exact_touching_width_drops_right_but_one_more_cell_retains_it() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let vars = map(&[("a", seg("left")), ("b", hit_seg("cc", Hit::Pr))]);
        let bar = spec("$a", "$b", "─");
        let out = render_bar(&bar, &vars, &HashMap::new(), 6, &resolver);
        assert_eq!(text(&out.line), "left──");
        assert!(out.hits.is_empty());
        let out = render_bar(&bar, &vars, &HashMap::new(), 7, &resolver);
        assert_eq!(text(&out.line), "left cc");
        assert_eq!(
            out.hits,
            vec![HitSpan {
                start_col: 5,
                width: 2,
                hit: Hit::Pr
            }]
        );
    }

    /// Dropping a segment removes every occurrence of it, on either side.
    #[test]
    fn suppression_removes_all_repeated_occurrences() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let configs = HashMap::from([("a".to_string(), cfg(10))]);
        let vars = map(&[
            ("a", hit_seg("A", Hit::Pr)),
            ("b", hit_seg("B", Hit::Procs)),
        ]);
        let out = render_bar(
            &spec("x", "($a )($a )$b", " "),
            &vars,
            &configs,
            5,
            &resolver,
        );
        assert_eq!(text(&out.line), "x   B");
        assert_eq!(
            out.hits,
            vec![HitSpan {
                start_col: 4,
                width: 1,
                hit: Hit::Procs
            }]
        );
    }

    /// The default priority is 100, and 100 never drops: `$b` has no config
    /// at all, so it stays even though it is what overflows; the explicitly
    /// lower-priority `$a` goes instead.
    #[test]
    fn unspecified_priority_is_one_hundred_and_never_drops() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let vars = map(&[("a", seg("A")), ("b", seg("B"))]);
        let configs = HashMap::from([("a".to_string(), cfg(99))]);
        let out = render_bar(&spec("x", "$a$b", " "), &vars, &configs, 3, &resolver);
        assert_eq!(text(&out.line), "x B");
        // At the default priority neither may drop, so the whole right side
        // is omitted rather than thinned.
        let configs = HashMap::from([("a".to_string(), cfg(100))]);
        let out = render_bar(&spec("x", "$a$b", " "), &vars, &configs, 3, &resolver);
        assert_eq!(text(&out.line), "x  ");
    }

    /// A droppable segment drops from the `format` side too, and does so
    /// before a right-side segment that ranks above it.
    #[test]
    fn a_droppable_left_segment_drops_before_a_higher_priority_right_one() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let configs = HashMap::from([("a".to_string(), cfg(10)), ("b".to_string(), cfg(20))]);
        let vars = map(&[("a", seg("AAAA")), ("b", hit_seg("BB", Hit::Pr))]);
        let out = render_bar(&spec("L($a)", "$b", " "), &vars, &configs, 5, &resolver);
        assert_eq!(text(&out.line), "L  BB");
        assert_eq!(
            out.hits,
            vec![HitSpan {
                start_col: 3,
                width: 2,
                hit: Hit::Pr
            }]
        );
    }

    /// A left segment at the default priority overflows the bar (the caller
    /// clips it) rather than dropping, even after the droppable right-side
    /// segment that could have made room has already gone.
    #[test]
    fn a_default_priority_left_segment_is_clipped_not_dropped() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let configs = HashMap::from([("b".to_string(), cfg(20))]);
        let vars = map(&[("a", seg("llllllll")), ("b", seg("BB"))]);
        let out = render_bar(&spec("$a", "$b", " "), &vars, &configs, 4, &resolver);
        assert_eq!(text(&out.line), "llllllll");
        assert_eq!(out.line.width(), 8, "wider than the bar; the caller clips");
    }

    /// Equal priorities break toward the first occurrence, and `format`
    /// comes before `right_format`.
    #[test]
    fn equal_priorities_drop_the_left_occurrence_first() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let configs = HashMap::from([("a".to_string(), cfg(30)), ("b".to_string(), cfg(30))]);
        let vars = map(&[("a", seg("AA")), ("b", seg("BB"))]);
        let out = render_bar(&spec("L($a)", "$b", " "), &vars, &configs, 5, &resolver);
        assert_eq!(text(&out.line), "L  BB");
    }

    /// Default-priority left segments are never dropped, only clipped.
    #[test]
    fn default_priority_left_segments_are_never_dropped() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let vars = map(&[("a", hit_seg("left", Hit::Pr)), ("b", seg("cc"))]);
        for width in [0, 3] {
            let out = render_bar(
                &spec("$a", "$b", " "),
                &vars,
                &HashMap::new(),
                width,
                &resolver,
            );
            assert_eq!(text(&out.line), "left");
            assert_eq!(
                out.hits,
                vec![HitSpan {
                    start_col: 0,
                    width: 4,
                    hit: Hit::Pr
                }]
            );
        }
    }

    #[test]
    fn overflowing_literal_right_cannot_remove_the_required_gap() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let out = render_bar(
            &spec("left", "right", "─"),
            &map(&[]),
            &HashMap::new(),
            9,
            &resolver,
        );
        assert_eq!(text(&out.line), "left─────");
    }

    #[test]
    fn fill_and_mandatory_blank_inherit_bar_and_fill_styles() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let mut bar = spec("x", "y", "-");
        bar.style = StyleSpec::parse("bg:blue").unwrap();
        bar.fill_style = StyleSpec::parse("fg:red dimmed").unwrap();
        let out = render_bar(&bar, &map(&[]), &HashMap::new(), 4, &resolver);
        assert_eq!(text(&out.line), "x- y");
        assert_eq!(out.line.spans[0].style, Style::default().bg(Color::Blue));
        assert_eq!(
            out.line.spans[1].style,
            Style::default()
                .bg(Color::Blue)
                .fg(Color::Red)
                .add_modifier(Modifier::DIM)
        );
        assert_eq!(
            out.line.spans.last().unwrap().style,
            Style::default().bg(Color::Blue)
        );
    }

    #[test]
    fn wide_and_zero_width_fill_preserve_flush_right_cell_offsets() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let vars = map(&[("a", seg("L")), ("b", hit_seg("R", Hit::Pr))]);
        for (fill, expected) in [
            ("界", "L界  R"),
            ("\u{301}", "L    R"),
            ("", "L    R"),
            ("ab", "Laaa R"),
        ] {
            let out = render_bar(
                &spec("$a", "$b", fill),
                &vars,
                &HashMap::new(),
                6,
                &resolver,
            );
            assert_eq!(text(&out.line), expected);
            assert_eq!(out.line.width(), 6);
            assert_eq!(
                out.hits,
                vec![HitSpan {
                    start_col: 5,
                    width: 1,
                    hit: Hit::Pr
                }]
            );
        }
    }

    #[test]
    fn maximum_bar_width_does_not_saturate_the_overflow_comparison() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let vars = map(&[
            ("a", Segment::text("x".repeat(65_535), Style::default())),
            ("b", hit_seg("R", Hit::Pr)),
        ]);
        let out = render_bar(
            &spec("$a", "$b", " "),
            &vars,
            &HashMap::new(),
            u16::MAX,
            &resolver,
        );
        assert_eq!(out.line.width(), 65_535);
        assert!(out.hits.is_empty());
    }

    #[test]
    fn hit_rects_are_absolute_clipped_and_nonempty() {
        let area = Rect::new(10, 5, 8, 1);
        let hits = vec![
            HitSpan {
                start_col: 2,
                width: 3,
                hit: Hit::Pr,
            },
            HitSpan {
                start_col: 6,
                width: 5,
                hit: Hit::Procs,
            },
            HitSpan {
                start_col: 9,
                width: 1,
                hit: Hit::ArmLeader,
            },
            HitSpan {
                start_col: 0,
                width: 0,
                hit: Hit::AttentionMore,
            },
        ];
        assert_eq!(
            hit_rects(area, &hits),
            vec![
                (Rect::new(12, 5, 3, 1), Hit::Pr),
                (Rect::new(16, 5, 2, 1), Hit::Procs)
            ]
        );
        assert!(hit_rects(Rect::new(10, 5, 8, 0), &hits).is_empty());
    }
}
