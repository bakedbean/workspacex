//! One provider per segment. A provider turns app data plus the segment's
//! `[segment]` config into a [`Segment`] by evaluating the segment's own
//! `format` against its variables, with `$style` bound to the provider's
//! state-derived default patched by the user's `style`.

use super::registry;
use super::render::{eval, eval_with_labels};
use super::segment::{Hit, Segment, SegmentConfig, SegmentMap};
use super::style::Resolver;
use crate::commands::pinned::{PinnedCommand, truncate_label};
use crate::commands::tags::{CHIP_COUNT, PromptTag};
use crate::data::store::AgentInstanceId;
use crate::git::DiffStats;
use crate::git::forge::BranchLifecycle;
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
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use std::borrow::Cow;
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

/// `$style` for a whole segment: the provider's state-derived
/// `default_style` with the user's `cfg.style` patched over it.
fn segment_style(cfg: &SegmentConfig, default_style: Style, resolver: &Resolver) -> Style {
    default_style.patch(resolver.resolve(&cfg.style).unwrap_or_default())
}

/// `$style` for the item at rendered position `i` of a multi-item
/// segment: the segment style patched by `cfg.styles[i]`, clamped to the
/// last grade once the list runs out. No grades: the segment style.
fn item_style(cfg: &SegmentConfig, i: usize, default_style: Style, resolver: &Resolver) -> Style {
    let base = segment_style(cfg, default_style, resolver);
    match cfg.styles.get(i).or(cfg.styles.last()) {
        Some(spec) => base.patch(resolver.resolve(spec).unwrap_or_default()),
        None => base,
    }
}

/// The neighbour colours (`registry::ITEM_COLORS`) for one evaluation:
/// the item's own final `$style` plus its rendered neighbours'. `None`
/// for an absent neighbour, or a style with that colour unset, carries no
/// colour, so the token drops and the run inherits.
fn item_colors(
    prev: Option<Style>,
    item: Option<Style>,
    next: Option<Style>,
) -> HashMap<String, Option<Color>> {
    let mut colors = HashMap::new();
    for (name, style) in [("item", item), ("prev", prev), ("next", next)] {
        colors.insert(format!("{name}_fg"), style.and_then(|s| s.fg));
        colors.insert(format!("{name}_bg"), style.and_then(|s| s.bg));
    }
    colors
}

/// Evaluate `cfg.format` against `vars` with `$style` bound to `style`,
/// `extra` adding more named styles (`$mark_style`), `colors` the
/// per-evaluation colour names, and `labels` the variables that render
/// without keeping a group alive (`render::eval_with_labels`). `None`
/// when the segment is disabled or renders empty.
fn eval_format(
    cfg: &SegmentConfig,
    vars: &SegmentMap,
    style: Style,
    extra: &[(&str, Style)],
    colors: HashMap<String, Option<Color>>,
    labels: &[&str],
    resolver: &Resolver,
) -> Option<Segment> {
    if cfg.disabled {
        return None;
    }
    let mut styles: HashMap<String, Style> =
        extra.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    styles.insert("style".to_string(), style);
    let r = resolver.with_colors(colors).with_styles(styles);
    let (seg, _) = eval_with_labels(&cfg.format, vars, &r, Style::default(), labels);
    (!seg.is_empty()).then_some(seg)
}

/// Evaluate `cfg.format` against `vars`. `$style` is `default_style` with
/// the user's `cfg.style` patched over it; `extra` adds more named styles
/// (`$mark_style`). Colour names resolve under the segment's own palette.
/// `None` when the segment is disabled or renders empty.
pub fn eval_segment(
    cfg: &SegmentConfig,
    vars: &SegmentMap,
    default_style: Style,
    extra: &[(&str, Style)],
    resolver: &Resolver,
) -> Option<Segment> {
    let resolver = &resolver.with_overlay(&cfg.palette);
    let style = segment_style(cfg, default_style, resolver);
    eval_format(cfg, vars, style, extra, HashMap::new(), &[], resolver)
}

/// Multi-item segments (`keys`, `pins`, `agents`): `cfg.format` describes
/// one item; items are joined by `cfg.separator`, graded by `cfg.styles`,
/// and each gets `hit` over its own cells. An item whose `format` renders
/// empty (an empty `format`, or one whose variables are all absent)
/// contributes neither text nor a separator nor a grade — it's as if it
/// were never in the list — so it can't leave a dangling separator
/// behind, or in front of, the items that did render.
///
/// Two passes: a measure pass with no neighbour colours learns which
/// items render (colours never change that), then a paint pass gives each
/// survivor its position's grade and its RENDERED neighbours' colours.
pub fn eval_items(
    cfg: &SegmentConfig,
    items: &[(SegmentMap, Style, Option<Hit>)],
    resolver: &Resolver,
) -> Option<Segment> {
    if cfg.disabled || items.is_empty() {
        return None;
    }
    let resolver = &resolver.with_overlay(&cfg.palette);
    let rendered: Vec<usize> = (0..items.len())
        .filter(|&i| {
            let (vars, default_style, _) = &items[i];
            let none = item_colors(None, None, None);
            eval_format(cfg, vars, *default_style, &[], none, &[], resolver).is_some()
        })
        .collect();
    let styles: Vec<Style> = rendered
        .iter()
        .enumerate()
        .map(|(n, &i)| item_style(cfg, n, items[i].1, resolver))
        .collect();
    let mut out = Segment::default();
    for (n, &i) in rendered.iter().enumerate() {
        let (vars, _, hit) = &items[i];
        let prev = (n > 0).then(|| styles[n - 1]);
        let next = styles.get(n + 1).copied();
        if n > 0 {
            let r = resolver.with_colors(item_colors(prev, None, Some(styles[n])));
            out.append(eval(&cfg.separator, &SegmentMap::new(), &r, Style::default()).0);
        }
        let colors = item_colors(prev, Some(styles[n]), next);
        let Some(seg) = eval_format(cfg, vars, styles[n], &[], colors, &[], resolver) else {
            continue;
        };
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
    let theme = &cfg.theme(theme);
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
    let theme = &cfg.theme(theme);
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
    let theme = &cfg.theme(theme);
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

/// The agent identity bar; `$style` is the agent's fixed color. `$symbol`
/// is the kind's `[agent_bar.symbols]` entry when the theme has one, else
/// `symbol` — so a theme can give each harness its own icon and keep one
/// glyph for kinds it hasn't drawn.
pub fn agent_bar(
    cfg: &SegmentConfig,
    agent: Option<AgentKind>,
    theme: &Theme,
    resolver: &Resolver,
) -> Option<Segment> {
    let theme = &cfg.theme(theme);
    let agent = agent?;
    let symbol = cfg
        .symbols
        .iter()
        .find(|(kind, _)| *kind == agent)
        .map(|(_, glyph)| glyph.clone())
        .or_else(|| cfg.symbol.clone())
        .unwrap_or_else(|| "▎".to_string());
    eval_segment(
        cfg,
        &vars(vec![("symbol", var(symbol))]),
        theme.agent_style(agent),
        &[],
        resolver,
    )
}

/// `$repo` is absent (so `($repo/)` collapses) when the repo name is empty.
/// `$style` is the branch's PR-lifecycle tint — the same one the dashboard
/// row and the `pr` chip use — or the header style when there is no PR or
/// the lifecycle has no tint of its own (draft).
pub fn workspace(
    cfg: &SegmentConfig,
    repo: &str,
    name: &str,
    lifecycle: Option<BranchLifecycle>,
    theme: &Theme,
    resolver: &Resolver,
) -> Option<Segment> {
    let theme = &cfg.theme(theme);
    let mut v = vars(vec![("name", var(name))]);
    if !repo.is_empty() {
        v.insert("repo".to_string(), var(repo));
    }
    let style = theme
        .lifecycle_style(lifecycle)
        .map(|s| s.add_modifier(Modifier::BOLD))
        .unwrap_or_else(|| theme.header_style());
    eval_segment(cfg, &v, style, &[], resolver)
}

/// Cross-workspace attention entries: one item per entry, greedy-fitted
/// to `items.max_width` using the theme's own item, separator, and tail
/// widths. `$glyph` arrives pre-styled in the entry's status color;
/// `$style` is the name's PR-lifecycle tint, or the muted `path` hue when
/// the lifecycle has no color, graded by `cfg.styles` per rendered
/// position. Entries that don't fit fold into `cfg.more_format`
/// (`$count`); entries then give way from the tail end until that tail
/// fits too, since a clipped tail is both unreadable and, as a click
/// target, unreachable. The first entry always renders; when it alone
/// crowds out the tail its `$name` is shortened with an ellipsis, and a
/// lone entry with nothing behind it just clips. Each rendered entry gets
/// `Hit::Attention`, the tail `Hit::AttentionMore`.
///
/// Like `eval_items`, two passes: measure (no neighbour colours, ungraded
/// style — neither changes a width) to drop empty items and fit, then
/// paint with each survivor's grade and rendered neighbours. The tail's
/// `prev` is the last rendered entry. With `cfg.more_style` the tail is a
/// graded block too: that style is its `$style` and `item_*`, and the
/// last rendered entry's `next` when the tail follows it. Without one the
/// tail is unstyled and that entry's `next` is absent even when a tail
/// follows.
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
    let theme = &cfg.theme(theme);
    let resolver = &resolver.with_overlay(&cfg.palette);
    let cell_width = |s: &str| Span::raw(s).width();
    let ages: Vec<String> = entries
        .iter()
        .map(|e| format_age(items.now_ms.saturating_sub(e.age_anchor_ms)))
        .collect();
    let name_style = |i: usize| {
        theme
            .lifecycle_style(entries[i].lifecycle)
            .unwrap_or_else(|| Style::default().fg(theme.path))
    };
    let item_vars = |i: usize, name: &str| -> SegmentMap {
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
        v
    };
    let measure = |i: usize, name: &str| -> Segment {
        let none = item_colors(None, None, None);
        eval_format(
            cfg,
            &item_vars(i, name),
            name_style(i),
            &[],
            none,
            &[],
            resolver,
        )
        .unwrap_or_default()
    };
    let separator = |colors: HashMap<String, Option<Color>>| -> Segment {
        let r = resolver.with_colors(colors);
        eval(&cfg.separator, &SegmentMap::new(), &r, Style::default()).0
    };
    let tail_style = cfg.more_style.as_ref().map(|spec| {
        segment_style(cfg, Style::default(), resolver)
            .patch(resolver.resolve(spec).unwrap_or_default())
    });
    let tail = |remaining: usize, colors: HashMap<String, Option<Color>>| -> Segment {
        let v = vars(vec![("count", var(remaining.to_string()))]);
        let r = resolver.with_colors(colors).with_styles(HashMap::from([(
            "style".to_string(),
            tail_style.unwrap_or_default(),
        )]));
        eval(&cfg.more_format, &v, &r, Style::default()).0
    };
    let none = || item_colors(None, None, None);
    let width = |seg: &Segment| usize::from(seg.width);

    // Measure pass. An entry whose item renders empty is dropped as if it
    // were never in the list — no separator, no hit, not counted in the
    // tail — exactly as `eval_items` treats empty items. `rendered` pairs
    // each surviving entry index with its (possibly shortened) name and
    // its measured item.
    let mut rendered: Vec<(usize, String, Segment)> = (0..entries.len())
        .map(|i| (i, entries[i].name.clone(), measure(i, &entries[i].name)))
        .filter(|(_, _, seg)| !seg.is_empty())
        .collect();
    if rendered.is_empty() {
        return None;
    }
    let sep_w = width(&separator(none()));
    let max_width = items.max_width;
    let mut included = 0usize;
    let mut total = 0usize;
    for (n, (_, _, seg)) in rendered.iter().enumerate() {
        let s = if n == 0 { 0 } else { sep_w };
        if total + s + width(seg) > max_width {
            break;
        }
        total += s + width(seg);
        included += 1;
    }
    while included > 1 && included < rendered.len() {
        if total + width(&tail(rendered.len() - included, none())) <= max_width {
            break;
        }
        included -= 1;
        total -= width(&rendered[included].2) + sep_w;
    }
    included = included.max(1);
    if included < rendered.len() {
        let budget = max_width.saturating_sub(width(&tail(rendered.len() - included, none())));
        let (first, name, seg) = &rendered[0];
        if width(seg) > budget {
            let fixed = width(seg).saturating_sub(cell_width(name));
            let name_budget = budget.saturating_sub(fixed);
            let mut kept = name.clone();
            while cell_width(&kept) + 1 > name_budget && kept.pop().is_some() {}
            kept.push('…');
            rendered[0].2 = measure(*first, &kept);
            rendered[0].1 = kept;
        }
    }

    // Paint pass.
    let remaining = rendered.len() - included;
    let styles: Vec<Style> = rendered
        .iter()
        .take(included)
        .enumerate()
        .map(|(n, (i, _, _))| item_style(cfg, n, name_style(*i), resolver))
        .collect();
    let mut out = Segment::default();
    for (n, (i, name, _)) in rendered.into_iter().take(included).enumerate() {
        let prev = (n > 0).then(|| styles[n - 1]);
        let next = styles
            .get(n + 1)
            .copied()
            .or_else(|| (remaining > 0).then_some(tail_style).flatten());
        if n > 0 {
            out.append(separator(item_colors(prev, None, Some(styles[n]))));
        }
        let colors = item_colors(prev, Some(styles[n]), next);
        let seg = eval_format(
            cfg,
            &item_vars(i, &name),
            styles[n],
            &[],
            colors,
            &[],
            resolver,
        )
        .unwrap_or_default();
        let start = out.width;
        out.append(seg);
        out.hit_from(start, Hit::Attention(entries[i].workspace_id));
    }
    if remaining > 0 {
        let start = out.width;
        out.append(tail(
            remaining,
            item_colors(styles.last().copied(), tail_style, None),
        ));
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

/// The variable map every module evaluates against: the fleet map plus
/// `$icon_<kind>`, each kind's glyph from `icons` — `[agent_bar.symbols]`,
/// the table the top bar and the pills read — and absent for a kind
/// without one. Borrowed as-is when the theme draws no glyphs, so the
/// bundled default clones nothing; built once per bar, not per module.
pub fn module_vars<'a>(
    fleet: &'a SegmentMap,
    icons: &[(AgentKind, String)],
) -> Cow<'a, SegmentMap> {
    if icons.is_empty() {
        return Cow::Borrowed(fleet);
    }
    let mut v = fleet.clone();
    for (kind, icon) in icons {
        v.insert(format!("icon_{}", kind.display_name()), var(icon.clone()));
    }
    Cow::Owned(v)
}

/// A `[module.<name>]`: the user's `format` evaluated against
/// `module_vars`. The icon variables are labels — they render beside a
/// count but never keep a `( … )` group alive on their own, so a kind with
/// nothing to report drops out glyph and all, as it does under the
/// bundled preset's literal words. No state colour of its own and no
/// click target.
pub fn module(cfg: &SegmentConfig, vars: &SegmentMap, resolver: &Resolver) -> Option<Segment> {
    let resolver = &resolver.with_overlay(&cfg.palette);
    let style = segment_style(cfg, Style::default(), resolver);
    let labels: Vec<&str> = registry::fleet_label_names().collect();
    eval_format(cfg, vars, style, &[], HashMap::new(), &labels, resolver)
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

/// Prompt-tag chips: the first `CHIP_COUNT` tags (the caller passes them
/// most-used first), then the manager chip from `more_format`. The manager
/// chip always renders — with no tags it is the only way to discover the
/// feature from the footer — so this segment is never empty unless disabled.
pub fn tags(cfg: &SegmentConfig, tags: &[PromptTag], resolver: &Resolver) -> Option<Segment> {
    if cfg.disabled {
        return None;
    }
    let items: Vec<(SegmentMap, Style, Option<Hit>)> = tags
        .iter()
        .take(CHIP_COUNT)
        .enumerate()
        .map(|(i, t)| {
            (
                vars(vec![
                    ("index", var((i + 1).to_string())),
                    ("label", var(truncate_label(&t.name, CHIP_LABEL_COLS))),
                ]),
                Style::default(),
                Some(Hit::TagChip(i)),
            )
        })
        .collect();
    let mut out = eval_items(cfg, &items, resolver).unwrap_or_default();
    let resolver = &resolver.with_overlay(&cfg.palette);
    if !out.is_empty() {
        out.append(
            eval(
                &cfg.separator,
                &SegmentMap::new(),
                resolver,
                Style::default(),
            )
            .0,
        );
    }
    let start = out.width;
    let v = vars(vec![("count", var(tags.len().to_string()))]);
    out.append(eval(&cfg.more_format, &v, resolver, Style::default()).0);
    out.hit_from(start, Hit::TagsManager);
    (!out.is_empty()).then_some(out)
}

/// Agent pills: `● claude q   ○ codex w`. The active instance gets the
/// filled dot and a bold label. `$symbol` is the dot plus its space (the
/// `[agents].symbol` field is not used; the dot encodes active/idle).
/// `$icon` is the pill's kind's glyph from `icons` — `[agent_bar.symbols]`,
/// so a theme draws each harness once — and absent for a kind without one.
pub fn agents(
    cfg: &SegmentConfig,
    agents: &[(AgentInstanceId, AgentKind, String, Option<char>)],
    active: Option<AgentInstanceId>,
    icons: &[(AgentKind, String)],
    theme: &Theme,
    resolver: &Resolver,
) -> Option<Segment> {
    let theme = &cfg.theme(theme);
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
            if let Some((_, icon)) = icons.iter().find(|(k, _)| k == kind) {
                v.insert("icon".to_string(), var(icon.clone()));
            }
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
    let theme = &cfg.theme(theme);
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
    let theme = &cfg.theme(theme);
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
    let theme = &cfg.theme(theme);
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
    let theme = &cfg.theme(theme);
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
            more_style: None,
            styles: Vec::new(),
            palette: HashMap::new(),
            symbols: Vec::new(),
        }
    }

    fn chip_pr(lifecycle: BranchLifecycle) -> ChipPr {
        ChipPr {
            lifecycle,
            number: 7,
            review: None,
            unresolved: None,
        }
    }

    fn graded_cfg(format_src: &str, separator: &str, styles: &[&str]) -> SegmentConfig {
        let mut cfg = item_cfg(format_src, separator);
        cfg.styles = styles
            .iter()
            .map(|s| crate::ui::bar::style::StyleSpec::parse(s).unwrap())
            .collect();
        cfg
    }

    fn labelled(labels: &[&str]) -> Vec<(SegmentMap, Style, Option<Hit>)> {
        labels
            .iter()
            .map(|l| {
                let v = if l.is_empty() {
                    vars(vec![])
                } else {
                    vars(vec![("label", var(*l))])
                };
                (v, Style::default(), None)
            })
            .collect()
    }

    fn span_style(out: &Segment, text: &str) -> Style {
        out.spans
            .iter()
            .find(|s| s.content.as_ref() == text)
            .unwrap_or_else(|| panic!("span {text:?} in {:?}", out.plain_text()))
            .style
    }

    /// `$symbol` is the kind's entry in `[agent_bar.symbols]` when it has
    /// one, else the segment's `symbol`; `$style` stays the agent colour
    /// either way, so a per-kind icon wears its kind's identity.
    #[test]
    fn agent_bar_symbol_prefers_the_kinds_entry_and_falls_back_to_symbol() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let mut cfg = item_cfg("[$symbol]($style)", "");
        cfg.symbol = Some(">".to_string());
        cfg.symbols = vec![(AgentKind::Codex, "X".to_string())];
        let out = agent_bar(&cfg, Some(AgentKind::Codex), &theme, &resolver).unwrap();
        assert_eq!(out.plain_text(), "X");
        assert_eq!(
            span_style(&out, "X").fg,
            theme.agent_style(AgentKind::Codex).fg
        );
        let out = agent_bar(&cfg, Some(AgentKind::Claude), &theme, &resolver).unwrap();
        assert_eq!(out.plain_text(), ">", "no entry: the plain symbol");
        assert_eq!(
            span_style(&out, ">").fg,
            theme.agent_style(AgentKind::Claude).fg
        );
    }

    /// An empty per-kind glyph is an override, not an absence: the segment
    /// drops (so a `($agent_bar )` group collapses) rather than falling
    /// through to `symbol`. With neither a per-kind entry nor `symbol`,
    /// the bundled bar glyph stands in.
    #[test]
    fn agent_bar_empty_entry_suppresses_and_no_symbol_uses_the_bar() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let mut cfg = item_cfg("[$symbol]($style)", "");
        cfg.symbol = Some(">".to_string());
        cfg.symbols = vec![(AgentKind::Pi, String::new())];
        assert!(agent_bar(&cfg, Some(AgentKind::Pi), &theme, &resolver).is_none());
        cfg.symbol = None;
        let out = agent_bar(&cfg, Some(AgentKind::Claude), &theme, &resolver).unwrap();
        assert_eq!(out.plain_text(), "▎");
    }

    /// has a PR, so a theme can colour the focused name like the
    /// dashboard row and the `pr` chip; without one it stays the header
    /// style.
    #[test]
    fn workspace_style_is_the_lifecycle_tint_or_header_style() {
        use crate::git::forge::BranchLifecycle::*;
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let cfg = item_cfg("[$name]($style)", "");
        let out = workspace(&cfg, "", "ws", Some(PrMerged), &theme, &resolver).unwrap();
        assert_eq!(span_style(&out, "ws").fg, Some(theme.merged));
        let out = workspace(&cfg, "", "ws", None, &theme, &resolver).unwrap();
        assert_eq!(span_style(&out, "ws").fg, Some(theme.header_fg));
        assert!(span_style(&out, "ws").add_modifier.contains(Modifier::BOLD));
        // Lifecycles with no tint of their own (draft) fall back the same way.
        let out = workspace(&cfg, "", "ws", Some(PrDraft), &theme, &resolver).unwrap();
        assert_eq!(span_style(&out, "ws").fg, Some(theme.header_fg));
    }

    /// A segment's own palette shadows the theme tokens behind its
    /// state-derived `$style`: `[pr.palette] ok = …` retints an open PR in
    /// that segment alone, so a theme can darken the lifecycle colours on a
    /// light block without touching the same colours elsewhere.
    #[test]
    fn segment_palette_shadows_the_tokens_behind_style() {
        use crate::git::forge::BranchLifecycle::*;
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let mut cfg = item_cfg("[$label]($style)", "");
        cfg.palette.insert("ok".to_string(), Color::Red);
        let out = pr(&cfg, Some(chip_pr(PrOpen)), &theme, &resolver).unwrap();
        assert_eq!(span_style(&out, "open").fg, Some(Color::Red));
        // Tokens the overlay doesn't name keep the theme's colour.
        let out = pr(&cfg, Some(chip_pr(PrMerged)), &theme, &resolver).unwrap();
        assert_eq!(span_style(&out, "merged").fg, Some(theme.merged));
        // The no-PR fallback of `workspace` is a token too (`header_fg`).
        cfg.palette.insert("header_fg".to_string(), Color::Black);
        let cfg_ws = SegmentConfig {
            format: crate::ui::bar::format::parse("[$name]($style)").unwrap(),
            ..cfg.clone()
        };
        let out = workspace(&cfg_ws, "", "ws", None, &theme, &resolver).unwrap();
        assert_eq!(span_style(&out, "ws").fg, Some(Color::Black));
    }

    /// Every provider that derives a colour from the theme derives it from
    /// the segment's shadowed theme — the dashboard header's `filter`
    /// (warn), `group`/`sort` tabs (selected_fg/bg, path) included, not
    /// only the lifecycle-tinted attached segments.
    #[test]
    fn dashboard_header_segments_honour_the_overlay_too() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let mut cfg = item_cfg("[$needle]($style)", "");
        cfg.palette.insert("warn".to_string(), Color::Red);
        let out = filter(&cfg, Some("auth"), &theme, &resolver).unwrap();
        assert_eq!(span_style(&out, "auth").fg, Some(Color::Red));

        let mut cfg = item_cfg("$tabs", "");
        cfg.palette.insert("selected_bg".to_string(), Color::Red);
        cfg.palette.insert("path".to_string(), Color::Blue);
        let out = group(&cfg, GroupMode::Repo, &theme, &resolver).unwrap();
        assert_eq!(span_style(&out, "repo").bg, Some(Color::Red));
        assert_eq!(span_style(&out, "attention").fg, Some(Color::Blue));
        let out = sort(&cfg, SortMode::Recency, &theme, &resolver).unwrap();
        assert_eq!(span_style(&out, "recency").bg, Some(Color::Red));
    }

    /// The overlay also shadows colour names used directly in the
    /// segment's format, ahead of the global `[palette]`, and is scoped to
    /// the segment: the resolver handed in is not changed.
    #[test]
    fn segment_palette_shadows_format_colors_ahead_of_the_global_palette() {
        let theme = Theme::wsx();
        let mut palette = HashMap::new();
        palette.insert("ok".to_string(), Color::Blue);
        let resolver = Resolver::new(&palette, &theme);
        let mut cfg = item_cfg("[$name](fg:ok)", "");
        cfg.palette.insert("ok".to_string(), Color::Red);
        let out = workspace(&cfg, "", "ws", None, &theme, &resolver).unwrap();
        assert_eq!(span_style(&out, "ws").fg, Some(Color::Red));
        assert_eq!(resolver.color("ok"), Some(Color::Blue));
        // Multi-item segments get the same overlay for their items.
        let mut cfg = item_cfg("[$label](fg:ok)", "-");
        cfg.palette.insert("ok".to_string(), Color::Red);
        let out = eval_items(&cfg, &labelled(&["a", "b"]), &resolver).unwrap();
        assert_eq!(span_style(&out, "a").fg, Some(Color::Red));
        assert_eq!(span_style(&out, "b").fg, Some(Color::Red));
    }

    /// `styles` grades by RENDERED position — an empty item takes no
    /// grade with it — and clamps to the last entry past the end.
    #[test]
    fn styles_grade_items_by_rendered_position_and_clamp() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let cfg = graded_cfg("[$label]($style)", "-", &["bg:red", "bg:blue bold"]);
        let out = eval_items(&cfg, &labelled(&["a", "", "b", "c"]), &resolver).unwrap();
        assert_eq!(out.plain_text(), "a-b-c");
        assert_eq!(span_style(&out, "a").bg, Some(ratatui::style::Color::Red));
        assert_eq!(span_style(&out, "b").bg, Some(ratatui::style::Color::Blue));
        assert_eq!(span_style(&out, "c").bg, Some(ratatui::style::Color::Blue));
        assert!(span_style(&out, "c").add_modifier.contains(Modifier::BOLD));
    }

    /// The grade patches over the provider's default and the user's
    /// `style`, so a bg-only grade keeps the state colour in the fg.
    #[test]
    fn a_grade_patches_over_the_default_and_user_style() {
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        let mut cfg = graded_cfg("[$label]($style)", "-", &["bg:red"]);
        cfg.style = crate::ui::bar::style::StyleSpec::parse("bold").unwrap();
        let items = vec![(
            vars(vec![("label", var("a"))]),
            Style::default().fg(ratatui::style::Color::Green),
            None,
        )];
        let out = eval_items(&cfg, &items, &resolver).unwrap();
        let s = span_style(&out, "a");
        assert_eq!(s.fg, Some(ratatui::style::Color::Green));
        assert_eq!(s.bg, Some(ratatui::style::Color::Red));
        assert!(s.add_modifier.contains(Modifier::BOLD));
    }

    /// Neighbour colours follow the RENDERED neighbours (an empty item is
    /// skipped over), and an absent neighbour at either end carries no
    /// colour, so the token drops and the run inherits.
    #[test]
    fn neighbour_colours_follow_rendered_neighbours_and_drop_at_the_ends() {
        use ratatui::style::Color;
        let theme = Theme::wsx();
        let palette = HashMap::new();
        let resolver = Resolver::new(&palette, &theme);
        // The whole item is a conditional group, so an item with no
        // `$label` renders nothing at all — including its wedges.
        let cfg = graded_cfg(
            "([<](fg:prev_bg bg:item_bg)[$label]($style)[>](fg:item_bg bg:next_bg))",
            "[|](fg:prev_bg bg:next_bg)",
            &["bg:red", "bg:blue", "bg:green"],
        );
        let out = eval_items(&cfg, &labelled(&["a", "", "b"]), &resolver).unwrap();
        assert_eq!(out.plain_text(), "<a>|<b>");
        let styles: Vec<(Option<Color>, Option<Color>)> =
            out.spans.iter().map(|s| (s.style.fg, s.style.bg)).collect();
        assert_eq!(
            styles,
            vec![
                (None, Some(Color::Red)),              // "<": no prev, so no fg
                (None, Some(Color::Red)),              // "a"
                (Some(Color::Red), Some(Color::Blue)), // ">": item -> next
                (Some(Color::Red), Some(Color::Blue)), // "|": prev -> next
                (Some(Color::Red), Some(Color::Blue)), // "<": prev (a) -> item
                (None, Some(Color::Blue)),             // "b"
                (Some(Color::Blue), None),             // ">": no next, so no bg
            ]
        );
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

    #[test]
    fn tags_renders_at_most_three_chips_then_the_manager_chip() {
        let theme = Theme::wsx();
        let specs = crate::config::theme_file::bundled_default(&theme);
        let resolver = specs.resolver(&theme);
        let cfg = &specs.segments["tags"];
        let four: Vec<PromptTag> = ["context", "task", "constraints", "examples"]
            .iter()
            .map(|n| PromptTag {
                name: (*n).into(),
                uses: 1,
            })
            .collect();
        let seg = tags(cfg, &four, &resolver).unwrap();
        assert_eq!(seg.plain_text(), "<context>  <task>  <constraints>   <> ");
        let hits: Vec<_> = seg.hits.iter().map(|h| h.hit).collect();
        assert_eq!(
            hits,
            vec![
                Hit::TagChip(0),
                Hit::TagChip(1),
                Hit::TagChip(2),
                Hit::TagsManager
            ]
        );
        let manager = seg.hits.iter().find(|h| h.hit == Hit::TagsManager).unwrap();
        assert_eq!(manager.width, 4, "` <> ` is the manager pill");

        // No tags at all still gives the manager chip, and nothing else.
        let seg = tags(cfg, &[], &resolver).unwrap();
        assert_eq!(seg.plain_text(), " <> ");
        assert_eq!(seg.hits.len(), 1);
        assert_eq!(seg.hits[0].hit, Hit::TagsManager);
    }
}
