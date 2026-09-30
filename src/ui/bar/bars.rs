//! The bar composers: build each concrete bar (dashboard header/footer,
//! attached top/bottom, detail pane) from its segment map, plus the shared
//! inputs types they take.

use super::format;
use super::providers;
use super::render::{Rendered, eval, render_bar};
use super::segment::{Hit, HitSpan, Segment, SegmentConfig, SegmentMap};
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

/// Insert every `[module.<name>]` from `specs.modules`, so `$<name>` works
/// in whichever bar the theme places it. Called by every composer. The
/// per-kind glyph table rides along for `$icon_<kind>`.
pub(super) fn put_modules(
    segments: &mut SegmentMap,
    specs: &BarSpecs,
    fleet: &SegmentMap,
    resolver: &style::Resolver<'_>,
) {
    let vars = providers::module_vars(fleet, &cfg(specs, "agent_bar").symbols);
    for name in &specs.modules {
        let module = cfg(specs, name);
        if module.disabled {
            continue;
        }
        put(segments, name, providers::module(module, &vars, resolver));
    }
}

pub struct DashboardFooterInputs<'a> {
    pub activity: &'a [u32],
    pub version: &'a str,
    pub window_label: &'a str,
    pub workspace_selected: bool,
    /// The selected workspace carries a lifecycle badge that points at a
    /// setup log (`LifecycleBadge::offers_setup_log`). The badge itself is
    /// two cells with no room to say so, so the footer carries the hint.
    pub setup_log_available: bool,
    /// Fleet variables for `[module.*]` segments — `fleet::FleetStats::to_vars()`.
    pub fleet: &'a SegmentMap,
}

/// The dashboard footer: key hints left, the funnel module right.
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
    // Informational, not clickable: `key_for_glyph` only resolves a single
    // glyph, and this is a two-key path. It is what turns the `⚙!` badge
    // from a symptom into something the user can act on.
    if inputs.setup_log_available {
        keys.push(("? o", "setup log"));
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
    put_modules(&mut segments, specs, inputs.fleet, &resolver);
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
    pub fleet: &'a SegmentMap,
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
    put_modules(&mut segments, specs, inputs.fleet, &resolver);
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
    fleet: &SegmentMap,
    width: u16,
) -> Rendered {
    let resolver = specs.resolver(theme);
    let mut segments = SegmentMap::new();
    put(
        &mut segments,
        "pins",
        providers::pins(cfg(specs, "pins"), pinned, &resolver),
    );
    put_modules(&mut segments, specs, fleet, &resolver);
    render_bar(
        &specs.dashboard_detail,
        &segments,
        &specs.segments,
        width,
        &resolver,
    )
}

/// What the dashboard detail pane's header row shows: the selected
/// workspace's identity, PR, and activity. `pub(crate)` because `ChipPr`
/// is.
pub(crate) struct DetailHeaderInputs<'a> {
    pub agent: crate::pty::session::AgentKind,
    pub name: &'a str,
    pub branch: &'a str,
    pub pr: Option<crate::ui::attached::ChipPr>,
    pub diff: Option<crate::git::DiffStats>,
    pub procs: u32,
    pub status: crate::ui::dashboard::status::Status,
    pub ago_secs: Option<u64>,
    pub fleet: &'a SegmentMap,
}

/// Build the detail header's segments. `pub(super)` for the drift test,
/// like `attached_segments`.
pub(super) fn detail_header_segments(
    specs: &BarSpecs,
    theme: &Theme,
    inputs: &DetailHeaderInputs<'_>,
    resolver: &style::Resolver<'_>,
) -> SegmentMap {
    let mut segments = SegmentMap::new();
    put(
        &mut segments,
        "agent_bar",
        providers::agent_bar(cfg(specs, "agent_bar"), Some(inputs.agent), theme, resolver),
    );
    // No repo: the dashboard already shows which repo the selection is in.
    put(
        &mut segments,
        "workspace",
        providers::workspace(
            cfg(specs, "workspace"),
            "",
            inputs.name,
            inputs.pr.map(|p| p.lifecycle),
            theme,
            resolver,
        ),
    );
    put(
        &mut segments,
        "branch",
        providers::branch(cfg(specs, "branch"), inputs.branch, theme, resolver),
    );
    put(
        &mut segments,
        "pr",
        providers::pr(cfg(specs, "pr"), inputs.pr, theme, resolver),
    );
    put(
        &mut segments,
        "diff",
        providers::diff(cfg(specs, "diff"), inputs.diff, theme, resolver),
    );
    put(
        &mut segments,
        "procs",
        providers::procs(cfg(specs, "procs"), inputs.procs, theme, resolver),
    );
    put(
        &mut segments,
        "status",
        providers::status(
            cfg(specs, "status"),
            inputs.status,
            inputs.ago_secs,
            theme,
            resolver,
        ),
    );
    put_modules(&mut segments, specs, inputs.fleet, resolver);
    segments
}

/// The dashboard detail pane's header row: agent, name, branch, PR chip,
/// diff, procs, and status. Its `$pr` carries `Hit::Pr`, which the pane
/// turns into the PR link.
pub(crate) fn dashboard_detail_header(
    specs: &BarSpecs,
    theme: &Theme,
    inputs: &DetailHeaderInputs<'_>,
    width: u16,
) -> Rendered {
    let resolver = specs.resolver(theme);
    let segments = detail_header_segments(specs, theme, inputs, &resolver);
    render_bar(
        &specs.dashboard_detail_header,
        &segments,
        &specs.segments,
        width,
        &resolver,
    )
}

/// What the dashboard detail pane's reply row shows: the selected
/// workspace's identity for the prompt, and the draft being typed.
pub(crate) struct DetailReplyInputs<'a> {
    pub agent: crate::pty::session::AgentKind,
    pub name: &'a str,
    pub branch: &'a str,
    pub draft: &'a str,
    pub focused: bool,
    /// `$pins`' chips, so a theme can lead the prompt with them and fold
    /// the pinned-chip row into this one.
    pub pinned: &'a [crate::commands::pinned::PinnedCommand],
    pub fleet: &'a SegmentMap,
}

/// The reply row, plus the cursor column (relative to the line's first
/// cell) the caller hands `set_cursor_position` while the row has focus.
pub(crate) struct ReplyRendered {
    pub line: ratatui::text::Line<'static>,
    pub cursor_x: u16,
    /// Columns relative to the first cell of the line, as `Rendered::hits`.
    pub hits: Vec<HitSpan>,
}

/// The draft field's ghost text while it is empty.
pub const REPLY_PLACEHOLDER: &str = "Reply to agent";

/// The right prompt yields rather than squeeze the draft below this many
/// cells.
const REPLY_MIN_FIELD: u16 = 12;

/// Build the reply row's segments. `pub(super)` for the drift test, like
/// `detail_header_segments`.
pub(super) fn detail_reply_segments(
    specs: &BarSpecs,
    theme: &Theme,
    inputs: &DetailReplyInputs<'_>,
    resolver: &style::Resolver<'_>,
) -> SegmentMap {
    let mut segments = SegmentMap::new();
    put(
        &mut segments,
        "prompt",
        providers::prompt(
            cfg(specs, "prompt"),
            inputs.agent,
            inputs.focused,
            theme,
            resolver,
        ),
    );
    put(
        &mut segments,
        "agent_bar",
        providers::agent_bar(cfg(specs, "agent_bar"), Some(inputs.agent), theme, resolver),
    );
    put(
        &mut segments,
        "workspace",
        providers::workspace(
            cfg(specs, "workspace"),
            "",
            inputs.name,
            None,
            theme,
            resolver,
        ),
    );
    put(
        &mut segments,
        "branch",
        providers::branch(cfg(specs, "branch"), inputs.branch, theme, resolver),
    );
    put(
        &mut segments,
        "pins",
        providers::pins(cfg(specs, "pins"), inputs.pinned, resolver),
    );
    // The hint is for a row being typed into; an idle row has nothing to send.
    if inputs.focused {
        put(
            &mut segments,
            "keys",
            providers::keys(
                cfg(specs, "keys"),
                &[("↵", "send", None), ("Esc", "cancel", None)],
                resolver,
            ),
        );
    }
    put_modules(&mut segments, specs, inputs.fleet, resolver);
    segments
}

/// The dashboard detail pane's reply row, drawn like a shell prompt:
/// `format` (the prompt), then the draft — its tail, so the cursor end
/// stays in view — then `fill` padding and `right_format` flush right. An
/// empty draft shows `REPLY_PLACEHOLDER` dimmed, the cursor on its first
/// cell.
///
/// The draft keeps at least `REPLY_MIN_FIELD` cells (or the whole row, when
/// narrower). To make that room the right side goes first, whole; then
/// the prompt's droppable segments (`priority` below the default), lowest
/// first; and a prompt still too long is clipped. The right side keeps one
/// blank cell before it, like every bar. Widths are measured per grapheme,
/// as the terminal draws them.
pub(crate) fn dashboard_detail_reply(
    specs: &BarSpecs,
    theme: &Theme,
    inputs: &DetailReplyInputs<'_>,
    width: u16,
) -> ReplyRendered {
    use ratatui::text::Span;
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;

    let resolver = specs.resolver(theme);
    let segments = detail_reply_segments(specs, theme, inputs, &resolver);
    let spec = &specs.dashboard_detail_reply;
    let base = resolver.resolve(&spec.style).unwrap_or_default();
    let reserve = REPLY_MIN_FIELD.min(width);
    let prompt_room = width - reserve;
    let prompt = super::render::eval_fitted(
        &spec.format,
        &segments,
        &specs.segments,
        prompt_room,
        &resolver,
        base,
    );
    let prompt = super::render::clip(prompt, prompt_room);
    let (mut right, _) = eval(&spec.right_format, &segments, &resolver, base);
    let field = |right: &Segment| {
        width
            .saturating_sub(prompt.width)
            .saturating_sub(right.width)
            .saturating_sub(u16::from(!right.is_empty()))
    };
    if !right.is_empty() && field(&right) < REPLY_MIN_FIELD {
        right = Segment::default();
    }
    let field = field(&right);

    // Graphemes from the end while they fit, so a wide one never overhangs
    // the field and a cluster (ZWJ emoji, a base and its combining marks)
    // is never split. Focus reserves a cell for the cursor past the last.
    let budget = field.saturating_sub(u16::from(inputs.focused));
    let tail = |text: &str, budget: u16| -> (String, u16) {
        let mut used = 0u16;
        let mut kept: Vec<&str> = Vec::new();
        for g in text.graphemes(true).rev() {
            let w = u16::try_from(g.width()).unwrap_or(u16::MAX);
            if used.saturating_add(w) > budget {
                break;
            }
            used += w;
            kept.push(g);
        }
        kept.reverse();
        (kept.concat(), used)
    };
    let (text, text_style, cursor_x) = if inputs.draft.is_empty() {
        // ASCII, so a cell per grapheme; keep its start when narrow.
        let ghost: String = REPLY_PLACEHOLDER
            .graphemes(true)
            .take(usize::from(field))
            .collect();
        (ghost, base.patch(theme.dim_style()), prompt.width)
    } else {
        let (visible, used) = tail(inputs.draft, budget);
        (visible, base, prompt.width.saturating_add(used))
    };

    let mut out = prompt;
    out.push(Span::styled(text, text_style));
    let gap = width.saturating_sub(out.width.saturating_add(right.width));
    if gap > 0 {
        let fill_style = base.patch(resolver.resolve(&spec.fill_style).unwrap_or_default());
        let fill = super::render::fill_run(&spec.fill, gap, !right.is_empty());
        out.push(Span::styled(fill, fill_style));
    }
    out.append(right);
    ReplyRendered {
        line: ratatui::text::Line::from(out.spans),
        cursor_x: cursor_x.min(width.saturating_sub(1)),
        hits: out.hits,
    }
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
    /// `$tags`' chips, most-used first; the provider takes the first
    /// `CHIP_COUNT` and counts the rest into the manager chip.
    pub tags: &'a [crate::commands::tags::PromptTag],
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
    pub fleet: &'a SegmentMap,
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
            inputs.pr.map(|p| p.lifecycle),
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
        "tags",
        providers::tags(cfg(specs, "tags"), inputs.tags, resolver),
    );
    put(
        &mut segments,
        "agents",
        providers::agents(
            cfg(specs, "agents"),
            inputs.agents,
            inputs.active_agent,
            &cfg(specs, "agent_bar").symbols,
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
    put_modules(&mut segments, specs, inputs.fleet, resolver);
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
