//! One provider per segment. A provider turns app data plus the segment's
//! `[segment]` config into a [`Segment`] by evaluating the segment's own
//! `format` against its variables, with `$style` bound to the provider's
//! state-derived default patched by the user's `style`.

use super::render::eval;
use super::segment::{Hit, Segment, SegmentConfig, SegmentMap};
use super::style::Resolver;
use crate::commands::pinned::{PinnedCommand, truncate_label};
use crate::data::store::AgentInstanceId;
use crate::git::DiffStats;
use crate::pty::session::AgentKind;
use crate::ui::attached::ChipPr;
use crate::ui::attached::chip_row::CHIP_LABEL_COLS;
use crate::ui::dashboard::status::Status;
use crate::ui::detail_modules::session_summary::ChipModelTokens;
use crate::ui::theme::Theme;
use crate::ui::updates_bar::AttentionLine;
use ratatui::style::Modifier;
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
/// its own cells. An item whose `format` renders empty (an empty `format`,
/// or one whose variables are all absent) contributes neither text nor a
/// separator — it's as if it were never in the list — so it can't leave a
/// dangling separator behind, or in front of, the items that did render.
pub fn eval_items(
    cfg: &SegmentConfig,
    items: &[(SegmentMap, Style, Option<Hit>)],
    resolver: &Resolver,
) -> Option<Segment> {
    if cfg.disabled || items.is_empty() {
        return None;
    }
    let mut out = Segment::default();
    for (vars, default_style, hit) in items {
        let Some(seg) = eval_segment(cfg, vars, *default_style, &[], resolver) else {
            continue;
        };
        if !out.is_empty() {
            out.push(Span::raw(cfg.separator.clone()));
        }
        let start = out.width;
        out.append(seg);
        if let Some(h) = hit {
            out.hit_from(start, *h);
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

/// The agent identity bar; `$style` is the agent's fixed color.
pub fn agent_bar(
    cfg: &SegmentConfig,
    agent: Option<AgentKind>,
    theme: &Theme,
    resolver: &Resolver,
) -> Option<Segment> {
    let agent = agent?;
    let symbol = cfg.symbol.clone().unwrap_or_else(|| "▎".to_string());
    eval_segment(
        cfg,
        &vars(vec![("symbol", var(symbol))]),
        theme.agent_style(agent),
        &[],
        resolver,
    )
}

/// `$repo` is absent (so `($repo/)` collapses) when the repo name is empty.
pub fn workspace(
    cfg: &SegmentConfig,
    repo: &str,
    name: &str,
    theme: &Theme,
    resolver: &Resolver,
) -> Option<Segment> {
    let mut v = vars(vec![("name", var(name))]);
    if !repo.is_empty() {
        v.insert("repo".to_string(), var(repo));
    }
    eval_segment(cfg, &v, theme.header_style(), &[], resolver)
}

/// The pre-built attention line as one opaque `$items` variable, its entry
/// and `… +N more` click extents carried as hits.
pub fn attention(
    cfg: &SegmentConfig,
    line: Option<AttentionLine>,
    resolver: &Resolver,
) -> Option<Segment> {
    let line = line?;
    let mut items = Segment::default();
    for span in line.line.spans {
        items.push(span);
    }
    for s in &line.segments {
        items.hits.push(super::segment::HitSpan {
            start_col: s.start_col,
            width: s.width,
            hit: Hit::Attention(s.workspace_id),
        });
    }
    if let Some(m) = line.more {
        items.hits.push(super::segment::HitSpan {
            start_col: m.start_col,
            width: m.width,
            hit: Hit::AttentionMore,
        });
    }
    eval_segment(
        cfg,
        &vars(vec![("items", items)]),
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

/// Pinned-command chips, at most nine (they are keyed `1`–`9`).
pub fn pins(cfg: &SegmentConfig, pinned: &[PinnedCommand], resolver: &Resolver) -> Option<Segment> {
    let items: Vec<(SegmentMap, Style, Option<Hit>)> = pinned
        .iter()
        .take(9)
        .enumerate()
        .map(|(i, cmd)| {
            let label = truncate_label(&cmd.label, CHIP_LABEL_COLS);
            (
                vars(vec![
                    ("index", var((i + 1).to_string())),
                    ("label", var(label)),
                ]),
                Style::default(),
                Some(Hit::PinnedChip(i)),
            )
        })
        .collect();
    eval_items(cfg, &items, resolver)
}

/// Agent pills: `● claude q   ○ codex w`. The active instance gets the
/// filled dot and a bold label. `$symbol` is the dot plus its space (the
/// `[agents].symbol` field is not used; the dot encodes active/idle).
pub fn agents(
    cfg: &SegmentConfig,
    agents: &[(AgentInstanceId, AgentKind, String, Option<char>)],
    active: Option<AgentInstanceId>,
    theme: &Theme,
    resolver: &Resolver,
) -> Option<Segment> {
    let items: Vec<(SegmentMap, Style, Option<Hit>)> = agents
        .iter()
        .map(|(id, kind, label, key)| {
            let is_active = active == Some(*id);
            let dot = if is_active { "● " } else { "○ " };
            let label_style = if is_active {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let mut v = vars(vec![("symbol", var(dot))]);
            v.insert(
                "label".to_string(),
                Segment::text(label.clone(), label_style),
            );
            if let Some(k) = key {
                v.insert("key".to_string(), var(k.to_string()));
            }
            (v, theme.agent_style(*kind), Some(Hit::Agent(*id)))
        })
        .collect();
    eval_items(cfg, &items, resolver)
}

/// `$style` is `ok`, or `warn` when the context window is nearly full.
pub(crate) fn model_tokens(
    cfg: &SegmentConfig,
    mt: Option<ChipModelTokens>,
    theme: &Theme,
    resolver: &Resolver,
) -> Option<Segment> {
    let mt = mt?;
    let style = if mt.warn {
        theme.warn_style()
    } else {
        theme.ok_style()
    };
    let mut v = vars(vec![("tokens", var(mt.tokens))]);
    if let Some(model) = mt.model {
        v.insert("model".to_string(), var(model));
    }
    eval_segment(cfg, &v, style, &[], resolver)
}

/// Hidden at zero, like the dashboard row's process dot.
pub fn procs(
    cfg: &SegmentConfig,
    procs: u32,
    theme: &Theme,
    resolver: &Resolver,
) -> Option<Segment> {
    if procs == 0 {
        return None;
    }
    let symbol = cfg.symbol.clone().unwrap_or_else(|| "●".to_string());
    let mut seg = eval_segment(
        cfg,
        &vars(vec![
            ("symbol", var(symbol)),
            ("count", var(procs.to_string())),
        ]),
        theme.status_style(Status::Thinking),
        &[],
        resolver,
    )?;
    seg.hit_from(0, Hit::Procs);
    Some(seg)
}

/// Hidden for a clean or unknown worktree.
pub fn diff(
    cfg: &SegmentConfig,
    diff: Option<DiffStats>,
    theme: &Theme,
    resolver: &Resolver,
) -> Option<Segment> {
    let d = diff?;
    if d.added == 0 && d.removed == 0 {
        return None;
    }
    eval_segment(
        cfg,
        &vars(vec![
            ("added", var(d.added.to_string())),
            ("removed", var(d.removed.to_string())),
        ]),
        theme.dim_style(),
        &[],
        resolver,
    )
}

/// `$style` is the lifecycle tint, `$mark_style` the review verdict's.
/// `$mark` is absent (so `( [$mark]($mark_style))` collapses) without a
/// verdict or on lifecycles that don't show one. `[pr].symbol` overrides
/// the lifecycle glyph when set.
pub(crate) fn pr(
    cfg: &SegmentConfig,
    pr: Option<ChipPr>,
    theme: &Theme,
    resolver: &Resolver,
) -> Option<Segment> {
    use crate::ui::theme::{lifecycle_chip, lifecycle_shows_review, review_mark};
    let pr = pr?;
    let (glyph, label) = lifecycle_chip(pr.lifecycle);
    if glyph.is_empty() {
        return None;
    }
    let glyph = cfg.symbol.as_deref().unwrap_or(glyph);
    let review = pr.review.filter(|_| lifecycle_shows_review(pr.lifecycle));
    let style = theme
        .lifecycle_style(Some(pr.lifecycle))
        .unwrap_or_else(|| theme.dim_style());
    let mark_style = review.map(|d| theme.review_style(d)).unwrap_or_default();
    let mut v = vars(vec![
        ("symbol", var(glyph)),
        ("number", var(pr.number.to_string())),
        ("label", var(label)),
    ]);
    if let Some(d) = review {
        v.insert("mark".to_string(), var(review_mark(d, pr.unresolved)));
    }
    let mut seg = eval_segment(cfg, &v, style, &[("mark_style", mark_style)], resolver)?;
    seg.hit_from(0, Hit::Pr);
    Some(seg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item_cfg(format_src: &str, separator: &str) -> SegmentConfig {
        SegmentConfig {
            style: crate::ui::bar::style::StyleSpec::default(),
            symbol: None,
            format: crate::ui::bar::format::parse(format_src).unwrap(),
            disabled: false,
            priority: 100,
            separator: separator.to_string(),
        }
    }

    #[test]
    fn all_items_rendering_empty_yields_none() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        // An empty `format` never produces anything, whatever the vars.
        let cfg = item_cfg("", "  ");
        let items: Vec<(SegmentMap, Style, Option<Hit>)> = vec![
            (vars(vec![("label", var("a"))]), Style::default(), None),
            (vars(vec![("label", var("b"))]), Style::default(), None),
        ];
        assert!(eval_items(&cfg, &items, &resolver).is_none());
    }

    #[test]
    fn a_leading_empty_item_leaves_no_leading_separator_and_keeps_the_survivors_hit() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let cfg = item_cfg("$label", "  ");
        let items: Vec<(SegmentMap, Style, Option<Hit>)> = vec![
            // No `label` var at all, so `$label` produces nothing.
            (vars(vec![]), Style::default(), Some(Hit::Procs)),
            (
                vars(vec![("label", var("b"))]),
                Style::default(),
                Some(Hit::Pr),
            ),
        ];
        let out = eval_items(&cfg, &items, &resolver).unwrap();
        assert_eq!(out.plain_text(), "b");
        assert_eq!(out.hits.len(), 1, "the empty item emits no hit");
        assert_eq!(out.hits[0].hit, Hit::Pr);
        assert_eq!(out.hits[0].start_col, 0);
    }

    #[test]
    fn two_non_empty_items_get_exactly_one_separator_between_them() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let cfg = item_cfg("$label", "-");
        let items: Vec<(SegmentMap, Style, Option<Hit>)> = vec![
            (vars(vec![("label", var("a"))]), Style::default(), None),
            (vars(vec![("label", var("b"))]), Style::default(), None),
        ];
        let out = eval_items(&cfg, &items, &resolver).unwrap();
        assert_eq!(out.plain_text(), "a-b");
    }
}
