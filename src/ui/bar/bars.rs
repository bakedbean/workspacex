//! The bar composers: build each concrete bar (dashboard header/footer,
//! attached top/bottom, detail pane) from its segment map, plus the shared
//! inputs types they take.

use super::format;
use super::providers;
use super::render::{Rendered, eval, render_bar};
use super::segment::{Hit, Segment, SegmentConfig, SegmentMap};
use super::style;
use crate::config::theme_file::BarSpecs;
use crate::ui::dashboard::layout::GroupMode;
use crate::ui::dashboard::sort::SortMode;
use crate::ui::theme::Theme;

/// The segment config by name. Every name in `SEGMENTS` is present because
/// the user file is merged over the bundled default.
pub fn cfg<'a>(specs: &'a BarSpecs, name: &str) -> &'a SegmentConfig {
    specs
        .segments
        .get(name)
        .unwrap_or_else(|| panic!("bundled default defines [{name}]"))
}

fn put(map: &mut SegmentMap, name: &str, seg: Option<Segment>) {
    if let Some(seg) = seg {
        map.insert(name.to_string(), seg);
    }
}

pub struct DashboardFooterInputs<'a> {
    pub activity: &'a [u32],
    pub version: &'a str,
    pub window_label: &'a str,
    pub workspace_selected: bool,
}

/// The dashboard footer: key hints left, version + usage graph right.
pub fn dashboard_footer(
    specs: &BarSpecs,
    theme: &Theme,
    inputs: &DashboardFooterInputs<'_>,
    width: u16,
) -> Rendered {
    let resolver = specs.resolver(theme);
    let mut keys: Vec<(&str, &str)> = vec![
        ("↑↓", "nav"),
        ("↵", "open"),
        ("n", "new"),
        ("G", "group"),
        ("o", "order"),
        ("/", "filter"),
    ];
    if inputs.workspace_selected {
        keys.push(("?", "actions"));
    }
    keys.push(("q", "quit"));
    let items: Vec<(&str, &str, Option<Hit>)> = keys
        .iter()
        .map(|(k, l)| (*k, *l, crate::ui::footer::key_for_glyph(k).map(Hit::Key)))
        .collect();
    let spark = crate::ui::dashboard::sparkline::render(inputs.activity, 24);

    let mut segments = SegmentMap::new();
    put(
        &mut segments,
        "keys",
        providers::keys(cfg(specs, "keys"), &items, &resolver),
    );
    put(
        &mut segments,
        "version",
        providers::version(cfg(specs, "version"), inputs.version, &resolver),
    );
    put(
        &mut segments,
        "usage",
        providers::usage(cfg(specs, "usage"), inputs.window_label, &spark, &resolver),
    );
    render_bar(
        &specs.dashboard_footer,
        &segments,
        &specs.segments,
        width,
        &resolver,
    )
}

pub struct DashboardHeaderInputs<'a> {
    pub group: GroupMode,
    pub sort: SortMode,
    pub repos: usize,
    pub workspaces: usize,
    /// The live filter needle, or `None` when no filter is active. `Some("")`
    /// is an armed-but-empty filter and still echoes a bare `/`.
    pub filter: Option<&'a str>,
    /// `$brand`'s `$view`: which view this header belongs to ("dashboard").
    pub view: &'a str,
}

/// The dashboard's top line: wordmark, group and sort tabs, the live
/// filter echo, and the repo/workspace counts flush right. Display only —
/// none of its segments carry a click hit.
pub fn dashboard_header(
    specs: &BarSpecs,
    theme: &Theme,
    inputs: &DashboardHeaderInputs<'_>,
    width: u16,
) -> Rendered {
    let resolver = specs.resolver(theme);
    let mut segments = SegmentMap::new();
    put(
        &mut segments,
        "brand",
        providers::brand(cfg(specs, "brand"), inputs.view, &resolver),
    );
    put(
        &mut segments,
        "group",
        providers::group(cfg(specs, "group"), inputs.group, theme, &resolver),
    );
    put(
        &mut segments,
        "sort",
        providers::sort(cfg(specs, "sort"), inputs.sort, theme, &resolver),
    );
    put(
        &mut segments,
        "filter",
        providers::filter(cfg(specs, "filter"), inputs.filter, theme, &resolver),
    );
    put(
        &mut segments,
        "counts",
        providers::counts(
            cfg(specs, "counts"),
            inputs.repos,
            inputs.workspaces,
            &resolver,
        ),
    );
    render_bar(
        &specs.dashboard_header,
        &segments,
        &specs.segments,
        width,
        &resolver,
    )
}

/// The dashboard detail pane's pinned-command row: chips, then a rule to
/// the edge. Only `$pins` is built; every other registered segment is
/// absent from the map and so renders empty wherever a theme references
/// it here.
pub fn dashboard_detail(
    specs: &BarSpecs,
    theme: &Theme,
    pinned: &[crate::commands::pinned::PinnedCommand],
    width: u16,
) -> Rendered {
    let resolver = specs.resolver(theme);
    let mut segments = SegmentMap::new();
    put(
        &mut segments,
        "pins",
        providers::pins(cfg(specs, "pins"), pinned, &resolver),
    );
    render_bar(
        &specs.dashboard_detail,
        &segments,
        &specs.segments,
        width,
        &resolver,
    )
}

/// Everything both attached bars need, so any attached segment can appear
/// in either bar's format and keep its click. `pub(crate)`, not `pub`: it
/// carries `ChipPr`/`ChipModelTokens`, which are themselves `pub(crate)`.
pub(crate) struct AttachedInputs<'a> {
    pub repo: &'a str,
    pub name: &'a str,
    /// `$version`'s value — `env!("CARGO_PKG_VERSION")` in the app.
    pub version: &'a str,
    /// `$usage`'s `$label`: the configured usage window (`24h`/`1w`/`1mo`).
    pub window_label: &'a str,
    /// `$usage`'s samples, the same 24-bucket vector the dashboard footer
    /// builds, so the sparkline reads identically in every bar.
    pub activity: &'a [u32],
    pub agent: Option<crate::pty::session::AgentKind>,
    pub attention: Option<crate::ui::updates_bar::AttentionItems>,
    pub pinned: &'a [crate::commands::pinned::PinnedCommand],
    pub procs: u32,
    pub diff: Option<crate::git::DiffStats>,
    pub pr: Option<crate::ui::attached::ChipPr>,
    pub model_tokens: Option<crate::ui::detail_modules::session_summary::ChipModelTokens>,
    pub agents: &'a [(
        crate::data::store::AgentInstanceId,
        crate::pty::session::AgentKind,
        String,
        Option<char>,
    )],
    pub active_agent: Option<crate::data::store::AgentInstanceId>,
}

/// Build every attached segment once, so a segment that appears in both
/// bars (or moves between them) renders identically and keeps its hit.
/// `pub(super)`, not private: `tests.rs`'s registry-drift test calls it
/// directly to confirm its output covers every `registry::SEGMENTS` name.
pub(super) fn attached_segments(
    specs: &BarSpecs,
    theme: &Theme,
    inputs: AttachedInputs<'_>,
    resolver: &style::Resolver<'_>,
) -> SegmentMap {
    let mut segments = SegmentMap::new();
    let keys_items: [(&str, &str, Option<Hit>); 1] = [("^x", "menu", Some(Hit::ArmLeader))];
    put(
        &mut segments,
        "keys",
        providers::keys(cfg(specs, "keys"), &keys_items, resolver),
    );
    put(
        &mut segments,
        "version",
        providers::version(cfg(specs, "version"), inputs.version, resolver),
    );
    // Same 24-bucket sparkline the dashboard footer draws — `$usage` is a
    // segment in all three bars, not a dashboard-only one.
    let spark = crate::ui::dashboard::sparkline::render(inputs.activity, 24);
    put(
        &mut segments,
        "usage",
        providers::usage(cfg(specs, "usage"), inputs.window_label, &spark, resolver),
    );
    put(
        &mut segments,
        "agent_bar",
        providers::agent_bar(cfg(specs, "agent_bar"), inputs.agent, theme, resolver),
    );
    put(
        &mut segments,
        "workspace",
        providers::workspace(
            cfg(specs, "workspace"),
            inputs.repo,
            inputs.name,
            theme,
            resolver,
        ),
    );
    put(
        &mut segments,
        "attention",
        providers::attention(
            cfg(specs, "attention"),
            inputs.attention.as_ref(),
            theme,
            resolver,
        ),
    );
    put(
        &mut segments,
        "pins",
        providers::pins(cfg(specs, "pins"), inputs.pinned, resolver),
    );
    put(
        &mut segments,
        "agents",
        providers::agents(
            cfg(specs, "agents"),
            inputs.agents,
            inputs.active_agent,
            theme,
            resolver,
        ),
    );
    put(
        &mut segments,
        "model_tokens",
        providers::model_tokens(
            cfg(specs, "model_tokens"),
            inputs.model_tokens,
            theme,
            resolver,
        ),
    );
    put(
        &mut segments,
        "procs",
        providers::procs(cfg(specs, "procs"), inputs.procs, theme, resolver),
    );
    put(
        &mut segments,
        "diff",
        providers::diff(cfg(specs, "diff"), inputs.diff, theme, resolver),
    );
    put(
        &mut segments,
        "pr",
        providers::pr(cfg(specs, "pr"), inputs.pr, theme, resolver),
    );
    segments
}

/// The attached view's rendered top and bottom bars.
pub(crate) struct AttachedBars {
    pub top: Rendered,
    pub bottom: Rendered,
}

/// Render the attached view's top and bottom bars from one shared segment
/// map.
pub(crate) fn attached_bars(
    specs: &BarSpecs,
    theme: &Theme,
    inputs: AttachedInputs<'_>,
    top_width: u16,
    bottom_width: u16,
) -> AttachedBars {
    let resolver = specs.resolver(theme);
    let segments = attached_segments(specs, theme, inputs, &resolver);
    AttachedBars {
        top: render_bar(
            &specs.attached_top,
            &segments,
            &specs.segments,
            top_width,
            &resolver,
        ),
        bottom: render_bar(
            &specs.attached_bottom,
            &segments,
            &specs.segments,
            bottom_width,
            &resolver,
        ),
    }
}

/// How many columns the attention line's items may occupy in whichever
/// attached bar actually places `$attention`.
///
/// Measured from the REAL format that places it, not from an assumed
/// `▎ label   ` prefix: under a custom format (the powerline example, say)
/// a hardcoded prefix is wrong, and the items then overrun the bar and are
/// clipped along with their click rects. The loader guarantees `$attention`
/// appears at most once across the two attached bars' four format strings
/// (`[attached_top]`/`[attached_bottom]` `format`/`right_format`), so
/// there's exactly one placement to measure — or none, in which case
/// nothing constrains the items and the full width is returned.
///
/// The measurement builds the shared segment map once with a ONE-cell
/// probe standing in for the attention items — one cell rather than none,
/// so the enclosing `( … $attention)` group and all of its literals
/// survive — then evaluates both sides of the bar that places it directly
/// (bypassing `render_bar`'s width-based overflow, which would otherwise
/// drop right-side content at a narrow probe width). `inputs` must carry
/// every other segment's real data (they share the bar) but need not set
/// `attention`; whatever it holds is replaced by the probe. A disabled
/// `[attention]` gets no probe, exactly as it gets no items.
///
/// The chrome subtracted is: the probe's own side minus its one probe
/// cell, plus — when the OTHER side of that same bar is nonempty — that
/// side's full width plus the mandatory blank column between them (mirrors
/// `render_bar`'s own accounting for a nonempty pair of sides).
pub(crate) fn attention_width_budget(
    specs: &BarSpecs,
    theme: &Theme,
    inputs: AttachedInputs<'_>,
    width: u16,
) -> usize {
    let inputs = AttachedInputs {
        attention: None,
        ..inputs
    };
    let resolver = specs.resolver(theme);
    let mut segments = attached_segments(specs, theme, inputs, &resolver);
    if !cfg(specs, "attention").disabled {
        segments.insert(
            "attention".to_string(),
            Segment::text("x", ratatui::style::Style::default()),
        );
    }

    let has_attention = |nodes: &[format::Node]| format::vars(nodes).contains(&"attention");
    let placement = if has_attention(&specs.attached_top.format) {
        Some((&specs.attached_top, true))
    } else if has_attention(&specs.attached_top.right_format) {
        Some((&specs.attached_top, false))
    } else if has_attention(&specs.attached_bottom.format) {
        Some((&specs.attached_bottom, true))
    } else if has_attention(&specs.attached_bottom.right_format) {
        Some((&specs.attached_bottom, false))
    } else {
        None
    };
    let Some((spec, in_format)) = placement else {
        return usize::from(width);
    };

    let base = resolver.resolve(&spec.style).unwrap_or_default();
    let (left, _) = eval(&spec.format, &segments, &resolver, base);
    let (right, _) = eval(&spec.right_format, &segments, &resolver, base);

    let chrome = if in_format {
        usize::from(left.width).saturating_sub(1)
            + if right.is_empty() {
                0
            } else {
                usize::from(right.width) + 1
            }
    } else {
        usize::from(right.width).saturating_sub(1)
            + if left.is_empty() {
                0
            } else {
                usize::from(left.width) + 1
            }
    };

    usize::from(width).saturating_sub(chrome)
}
