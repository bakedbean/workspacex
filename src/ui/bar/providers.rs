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
use crate::ui::dashboard::layout::GroupMode;
use crate::ui::dashboard::sort::SortMode;
use crate::ui::dashboard::status::Status;
use crate::ui::detail_modules::session_summary::ChipModelTokens;
use crate::ui::text::{FILTER_ECHO_MAX, truncate};
use crate::ui::theme::Theme;
use crate::ui::updates_bar::{AttentionItems, format_age};
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
    let (separator, _) = eval(
        &cfg.separator,
        &SegmentMap::new(),
        resolver,
        Style::default(),
    );
    for (vars, default_style, hit) in items {
        let Some(seg) = eval_segment(cfg, vars, *default_style, &[], resolver) else {
            continue;
        };
        if !out.is_empty() {
            out.append(separator.clone());
        }
        let start = out.width;
        out.append(seg);
        if let Some(h) = hit {
            out.hit_from(start, *h);
        }
    }
    (!out.is_empty()).then_some(out)
}

/// The dashboard header's wordmark: the brand cursor block (the site's
/// blinking caret) marks the line as the app rather than a repo name, and
/// the two-tone wordmark keeps it distinct from `header_style`, which repo
/// headers also use. `$view` names the view the header belongs to.
pub fn brand(cfg: &SegmentConfig, view: &str, resolver: &Resolver) -> Option<Segment> {
    let symbol = cfg.symbol.clone().unwrap_or_else(|| "▌".to_string());
    eval_segment(
        cfg,
        &vars(vec![
            ("symbol", var(symbol)),
            ("name", var("workspace")),
            ("mark", var("x")),
            ("view", var(view)),
        ]),
        Style::default(),
        &[],
        resolver,
    )
}

/// One `group:`/`sort:` mode tab. The active tab is the one painted on the
/// selection background — that highlight is the only thing distinguishing
/// it, since every mode's label is always drawn.
fn tab_span(label: &str, active: bool, theme: &Theme) -> Span<'static> {
    if active {
        Span::styled(
            label.to_string(),
            Style::default()
                .fg(theme.selected_fg)
                .bg(theme.selected_bg)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(label.to_string(), Style::default().fg(theme.path))
    }
}

/// `$tabs`: an opaque run of mode labels joined by one space. Opaque
/// because the highlight is positional — a theme can restyle the `$label`
/// around it, but which tab is lit is state, not theming.
fn tabs(labels: &[(&str, bool)], theme: &Theme) -> Segment {
    let mut out = Segment::default();
    for (i, (label, active)) in labels.iter().enumerate() {
        if i > 0 {
            out.push(Span::raw(" "));
        }
        out.push(tab_span(label, *active, theme));
    }
    out
}

/// The dashboard's grouping tabs. Variables: `$label` `$tabs`.
pub fn group(
    cfg: &SegmentConfig,
    mode: GroupMode,
    theme: &Theme,
    resolver: &Resolver,
) -> Option<Segment> {
    let tabs = tabs(
        &[
            ("repo", mode == GroupMode::Repo),
            ("attention", mode == GroupMode::Attention),
        ],
        theme,
    );
    eval_segment(
        cfg,
        &vars(vec![("label", var("group:")), ("tabs", tabs)]),
        Style::default(),
        &[],
        resolver,
    )
}

/// The dashboard's ordering tabs. Variables: `$label` `$tabs`.
pub fn sort(
    cfg: &SegmentConfig,
    mode: SortMode,
    theme: &Theme,
    resolver: &Resolver,
) -> Option<Segment> {
    let tabs = tabs(
        &[
            ("recency", mode == SortMode::Recency),
            ("status", mode == SortMode::Status),
        ],
        theme,
    );
    eval_segment(
        cfg,
        &vars(vec![("label", var("sort:")), ("tabs", tabs)]),
        Style::default(),
        &[],
        resolver,
    )
}

/// The live filter echo. Absent when no filter is active — without the
/// echo, `/` looks inert and rows vanishing from the list have no visible
/// cause, so an active-but-empty needle still renders the bare `/`.
pub fn filter(
    cfg: &SegmentConfig,
    needle: Option<&str>,
    theme: &Theme,
    resolver: &Resolver,
) -> Option<Segment> {
    let needle = needle?;
    eval_segment(
        cfg,
        &vars(vec![("needle", var(truncate(needle, FILTER_ECHO_MAX)))]),
        Style::default().fg(theme.warn).add_modifier(Modifier::BOLD),
        &[],
        resolver,
    )
}

/// Registered repos and workspaces. Variables: `$repos` `$workspaces`.
pub fn counts(
    cfg: &SegmentConfig,
    repos: usize,
    workspaces: usize,
    resolver: &Resolver,
) -> Option<Segment> {
    eval_segment(
        cfg,
        &vars(vec![
            ("repos", var(repos.to_string())),
            ("workspaces", var(workspaces.to_string())),
        ]),
        Style::default(),
        &[],
        resolver,
    )
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

/// Cross-workspace attention entries: one item per entry, greedy-fitted
/// to `items.max_width` using the theme's own item, separator, and tail
/// widths. `$glyph` arrives pre-styled in the entry's status color;
/// `$style` is the name's PR-lifecycle tint, or the muted `path` hue when
/// the lifecycle has no color. Entries that don't fit fold into
/// `cfg.more_format` (`$count`); entries then give way from the tail end
/// until that tail fits too, since a clipped tail is both unreadable and,
/// as a click target, unreachable. The first entry always renders; when
/// it alone crowds out the tail its `$name` is shortened with an
/// ellipsis, and a lone entry with nothing behind it just clips. Each
/// rendered entry gets `Hit::Attention`, the tail `Hit::AttentionMore`.
pub fn attention(
    cfg: &SegmentConfig,
    items: Option<&AttentionItems>,
    theme: &Theme,
    resolver: &Resolver,
) -> Option<Segment> {
    let items = items?;
    let entries = &items.entries;
    if cfg.disabled || entries.is_empty() {
        return None;
    }
    let cell_width = |s: &str| Span::raw(s).width();
    let ages: Vec<String> = entries
        .iter()
        .map(|e| format_age(items.now_ms.saturating_sub(e.age_anchor_ms)))
        .collect();
    let item = |i: usize, name: &str| -> Segment {
        let e = &entries[i];
        let mut v = vars(vec![
            ("repo", var(e.repo_name.clone())),
            ("name", var(name)),
            ("age", var(ages[i].clone())),
        ]);
        v.insert(
            "glyph".to_string(),
            Segment::text(e.status.glyph().to_string(), theme.status_style(e.status)),
        );
        let name_style = theme
            .lifecycle_style(e.lifecycle)
            .unwrap_or_else(|| Style::default().fg(theme.path));
        eval_segment(cfg, &v, name_style, &[], resolver).unwrap_or_default()
    };
    let (separator, _) = eval(
        &cfg.separator,
        &SegmentMap::new(),
        resolver,
        Style::default(),
    );
    let sep_w = usize::from(separator.width);
    let tail = |remaining: usize| -> Segment {
        let v = vars(vec![("count", var(remaining.to_string()))]);
        eval(&cfg.more_format, &v, resolver, Style::default()).0
    };
    let width = |seg: &Segment| usize::from(seg.width);

    // An entry whose item renders empty is dropped as if it were never in
    // the list — no separator, no hit, not counted in the tail — exactly
    // as `eval_items` treats empty items. `rendered` pairs each surviving
    // item with its entry index.
    let mut rendered: Vec<(usize, Segment)> = (0..entries.len())
        .map(|i| (i, item(i, &entries[i].name)))
        .filter(|(_, seg)| !seg.is_empty())
        .collect();
    if rendered.is_empty() {
        return None;
    }
    let max_width = items.max_width;
    let mut included = 0usize;
    let mut total = 0usize;
    for (n, (_, seg)) in rendered.iter().enumerate() {
        let s = if n == 0 { 0 } else { sep_w };
        if total + s + width(seg) > max_width {
            break;
        }
        total += s + width(seg);
        included += 1;
    }
    while included > 1 && included < rendered.len() {
        if total + width(&tail(rendered.len() - included)) <= max_width {
            break;
        }
        included -= 1;
        total -= width(&rendered[included].1) + sep_w;
    }
    included = included.max(1);
    if included < rendered.len() {
        let budget = max_width.saturating_sub(width(&tail(rendered.len() - included)));
        let (first, seg) = &rendered[0];
        if width(seg) > budget {
            let name = &entries[*first].name;
            let fixed = width(seg).saturating_sub(cell_width(name));
            let name_budget = budget.saturating_sub(fixed);
            let mut kept = name.clone();
            while cell_width(&kept) + 1 > name_budget && kept.pop().is_some() {}
            kept.push('…');
            rendered[0].1 = item(*first, &kept);
        }
    }

    let remaining = rendered.len() - included;
    let mut out = Segment::default();
    for (n, (i, seg)) in rendered.into_iter().take(included).enumerate() {
        if n > 0 {
            out.append(separator.clone());
        }
        let start = out.width;
        out.append(seg);
        out.hit_from(start, Hit::Attention(entries[i].workspace_id));
    }
    if remaining > 0 {
        let start = out.width;
        out.append(tail(remaining));
        out.hit_from(start, Hit::AttentionMore);
    }
    (!out.is_empty()).then_some(out)
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
            separator: crate::ui::bar::format::parse(separator).unwrap(),
            more_format: Vec::new(),
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
    fn a_styled_separator_keeps_its_style_between_items() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let cfg = item_cfg("$label", "[ │ ](fg:dim)");
        let items: Vec<(SegmentMap, Style, Option<Hit>)> = vec![
            (vars(vec![("label", var("a"))]), Style::default(), None),
            (vars(vec![("label", var("b"))]), Style::default(), None),
        ];
        let out = eval_items(&cfg, &items, &resolver).unwrap();
        assert_eq!(out.plain_text(), "a │ b");
        let sep = out
            .spans
            .iter()
            .find(|s| s.content.as_ref() == " │ ")
            .expect("separator span");
        assert_eq!(sep.style.fg, Some(theme.dim));
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
