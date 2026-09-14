//! One provider per segment. A provider turns app data plus the segment's
//! `[segment]` config into a [`Segment`] by evaluating the segment's own
//! `format` against its variables, with `$style` bound to the provider's
//! state-derived default patched by the user's `style`.

use super::render::eval;
use super::segment::{Hit, Segment, SegmentConfig, SegmentMap};
use super::style::Resolver;
use ratatui::style::Style;
use ratatui::text::Span;
use std::collections::HashMap;

/// A plain variable value.
pub fn var(s: impl Into<String>) -> Segment {
    Segment::text(s, Style::default())
}

pub fn vars(entries: Vec<(&str, Segment)>) -> SegmentMap {
    entries
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

/// Evaluate `cfg.format` against `vars`. `$style` is `default_style` with
/// the user's `cfg.style` patched over it; `extra` adds more named styles
/// (`$mark_style`). `None` when the segment is disabled or renders empty.
pub fn eval_segment(
    cfg: &SegmentConfig,
    vars: &SegmentMap,
    default_style: Style,
    extra: &[(&str, Style)],
    resolver: &Resolver,
) -> Option<Segment> {
    if cfg.disabled {
        return None;
    }
    let user = resolver.resolve(&cfg.style).unwrap_or_default();
    let mut styles: HashMap<String, Style> =
        extra.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    styles.insert("style".to_string(), default_style.patch(user));
    let r = resolver.with_styles(styles);
    let (seg, _) = eval(&cfg.format, vars, &r, Style::default());
    (!seg.is_empty()).then_some(seg)
}

/// Multi-item segments (`keys`, `pins`, `agents`): `cfg.format` describes
/// one item; items are joined by `cfg.separator` and each gets `hit` over
/// its own cells.
pub fn eval_items(
    cfg: &SegmentConfig,
    items: &[(SegmentMap, Style, Option<Hit>)],
    resolver: &Resolver,
) -> Option<Segment> {
    if cfg.disabled || items.is_empty() {
        return None;
    }
    let mut out = Segment::default();
    for (i, (vars, default_style, hit)) in items.iter().enumerate() {
        if i > 0 {
            out.push(Span::raw(cfg.separator.clone()));
        }
        let start = out.width;
        if let Some(seg) = eval_segment(cfg, vars, *default_style, &[], resolver) {
            out.append(seg);
        }
        if let Some(h) = hit {
            if out.width > start {
                out.hit_from(start, *h);
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

/// `(key glyph, label, hit)` per pill.
pub fn keys(
    cfg: &SegmentConfig,
    items: &[(&str, &str, Option<Hit>)],
    resolver: &Resolver,
) -> Option<Segment> {
    let items: Vec<(SegmentMap, Style, Option<Hit>)> = items
        .iter()
        .map(|(k, l, h)| {
            (
                vars(vec![("key", var(*k)), ("label", var(*l))]),
                Style::default(),
                *h,
            )
        })
        .collect();
    eval_items(cfg, &items, resolver)
}

pub fn version(cfg: &SegmentConfig, version: &str, resolver: &Resolver) -> Option<Segment> {
    eval_segment(
        cfg,
        &vars(vec![("version", var(version))]),
        Style::default(),
        &[],
        resolver,
    )
}

/// The whole segment is the usage-graph click target.
pub fn usage(
    cfg: &SegmentConfig,
    label: &str,
    spark: &str,
    resolver: &Resolver,
) -> Option<Segment> {
    let mut seg = eval_segment(
        cfg,
        &vars(vec![("label", var(label)), ("spark", var(spark))]),
        Style::default(),
        &[],
        resolver,
    )?;
    seg.hit_from(0, Hit::UsageGraph);
    Some(seg)
}
