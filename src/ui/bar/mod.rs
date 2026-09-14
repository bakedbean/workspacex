//! The themeable bar engine: a starship-style format grammar, segment
//! providers that carry click hits, and one evaluator shared by the
//! dashboard footer and the attached view's top and bottom bars.
//!
//! See `docs/superpowers/specs/2026-09-13-bar-theming-design.md`.

pub mod format;
pub mod providers;
pub mod render;
pub mod segment;
pub mod style;
#[cfg(test)]
pub mod test_util;

use crate::config::theme_file::BarSpecs;
use crate::ui::theme::Theme;
use render::{Rendered, eval, render_bar};
use segment::{Hit, Segment, SegmentConfig, SegmentMap};

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
    pub attention: Option<crate::ui::updates_bar::AttentionLine>,
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
fn attached_segments(
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
        providers::attention(cfg(specs, "attention"), inputs.attention, resolver),
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

/// Render the attached view's top and bottom bars from one shared segment
/// map. Returns `(top, bottom)`.
pub(crate) fn attached_bars(
    specs: &BarSpecs,
    theme: &Theme,
    inputs: AttachedInputs<'_>,
    top_width: u16,
    bottom_width: u16,
) -> (Rendered, Rendered) {
    let resolver = specs.resolver(theme);
    let segments = attached_segments(specs, theme, inputs, &resolver);
    (
        render_bar(
            &specs.attached_top,
            &segments,
            &specs.segments,
            top_width,
            &resolver,
        ),
        render_bar(
            &specs.attached_bottom,
            &segments,
            &specs.segments,
            bottom_width,
            &resolver,
        ),
    )
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
/// probe standing in for the attention line — one cell rather than none,
/// so the enclosing `( … $attention)` group and all of its literals
/// survive — then evaluates both sides of the bar that places it directly
/// (bypassing `render_bar`'s width-based overflow, which would otherwise
/// drop right-side content at a narrow probe width). `inputs` must carry
/// every other segment's real data (they share the bar) but need not set
/// `attention`; whatever it holds is replaced by the probe.
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
    let probe = crate::ui::updates_bar::AttentionLine {
        line: ratatui::text::Line::from("x"),
        segments: Vec::new(),
        more: None,
    };
    let inputs = AttachedInputs {
        attention: Some(probe),
        ..inputs
    };
    let resolver = specs.resolver(theme);
    let segments = attached_segments(specs, theme, inputs, &resolver);

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

#[cfg(test)]
mod attention_budget_tests {
    use super::*;
    use crate::config::theme_file::bundled_default;
    use crate::pty::session::AgentKind;

    fn inputs<'a>(repo: &'a str, name: &'a str, agent: Option<AgentKind>) -> AttachedInputs<'a> {
        AttachedInputs {
            repo,
            name,
            version: "0.1.0",
            window_label: "24h",
            activity: &[],
            agent,
            attention: None,
            pinned: &[],
            procs: 0,
            diff: None,
            pr: None,
            model_tokens: None,
            agents: &[],
            active_agent: None,
        }
    }

    /// The prefix the deleted `info_line_prefix_width` hardcoded: the
    /// agent bar (`▎` + a space) when present, the label in CELLS, and the
    /// stock format's 3-column gap before the attention items.
    fn stock_prefix(label: &str, agent: Option<AgentKind>) -> usize {
        let bar = if agent.is_some() { 2 } else { 0 };
        bar + ratatui::text::Span::raw(label).width() + 3
    }

    /// Parity with the stock prefix: for the bundled default the probe
    /// measures exactly `▎ ` + label + the 3-column gap, with and without
    /// an agent bar. (The old call site then subtracted a further 3 as an
    /// unexplained right margin; the probe measures what the bar actually
    /// draws, so those 3 columns are now available to the items.)
    #[test]
    fn bundled_default_budget_matches_the_stock_prefix() {
        let theme = Theme::wsx();
        let specs = bundled_default(&theme);
        for agent in [Some(AgentKind::Claude), None] {
            let budget = attention_width_budget(&specs, &theme, inputs("wsx", "foo", agent), 80);
            assert_eq!(
                budget,
                80 - stock_prefix("wsx/foo", agent),
                "agent={agent:?}"
            );
        }
    }

    /// Cells, not chars: "日本" is 2 chars but 4 cells, so a wide label
    /// must cost the budget 2 more columns than a 2-cell one. (Moved from
    /// `info_line_prefix_width_counts_cells_not_chars`.)
    #[test]
    fn budget_counts_label_cells_not_chars() {
        let theme = Theme::wsx();
        let specs = bundled_default(&theme);
        let wide = attention_width_budget(
            &specs,
            &theme,
            inputs("r", "日本", Some(AgentKind::Claude)),
            80,
        );
        let narrow = attention_width_budget(
            &specs,
            &theme,
            inputs("r", "ab", Some(AgentKind::Claude)),
            80,
        );
        assert_eq!(wide, narrow - 2);
    }

    /// A custom format gets its own measurement: `[ $workspace ](bg:blue)`
    /// is `label + 2` cells and `( $attention)` contributes its leading
    /// space, so the items get `width - (label + 2) - 1`. The old
    /// hardcoded prefix would have claimed 3 columns that this format
    /// never draws, overrunning the bar.
    #[test]
    fn custom_format_budget_follows_that_format() {
        let theme = Theme::wsx();
        let mut specs = bundled_default(&theme);
        specs.attached_top.format = format::parse("[ $workspace ](bg:blue)( $attention)").unwrap();
        let label_width = "wsx/foo".len();
        let budget = attention_width_budget(
            &specs,
            &theme,
            inputs("wsx", "foo", Some(AgentKind::Claude)),
            80,
        );
        assert_eq!(budget, 80 - (label_width + 2) - 1);
    }

    /// A bar wider than the terminal leaves no room at all rather than
    /// underflowing.
    #[test]
    fn budget_saturates_at_zero_on_a_narrow_bar() {
        let theme = Theme::wsx();
        let specs = bundled_default(&theme);
        assert_eq!(
            attention_width_budget(&specs, &theme, inputs("wsx", "foo", None), 4),
            0
        );
    }

    /// `$attention` in `attached_top.right_format`, with a nonempty left
    /// side (`$workspace`): the chrome is the probe side's own width minus
    /// its one probe cell, plus the OTHER (left) side's full width plus the
    /// mandatory blank between them — mirroring `render_bar`'s accounting
    /// for a nonempty pair of sides, just on the right-side probe instead
    /// of the left.
    #[test]
    fn attention_in_a_right_format_accounts_for_the_other_sides_width_too() {
        let theme = Theme::wsx();
        let mut specs = bundled_default(&theme);
        specs.attached_top.format = format::parse("$workspace").unwrap();
        specs.attached_top.right_format = format::parse("( $attention)").unwrap();
        let label_width = "wsx/foo".len();
        let budget = attention_width_budget(
            &specs,
            &theme,
            inputs("wsx", "foo", Some(AgentKind::Claude)),
            80,
        );
        // Probe side (right): "( $attention)" -> " x", 2 cells, minus the
        // 1 probe cell = 1. Other side (left): "wsx/foo" (label_width) + 1
        // mandatory blank.
        assert_eq!(budget, 80 - 1 - (label_width + 1));
    }

    /// `$attention` placed in `attached_bottom.format` (not the top bar)
    /// measures the BOTTOM bar's own right side — here the stock
    /// `right_format`, rendering a real PR chip — proving the budget
    /// follows placement across bars, not just within `attached_top`.
    #[test]
    fn attention_in_the_bottom_bars_format_uses_the_bottom_bars_own_right_side() {
        use crate::git::forge::BranchLifecycle;
        use crate::ui::attached::ChipPr;

        fn pr_input(base: AttachedInputs<'_>) -> AttachedInputs<'_> {
            AttachedInputs {
                pr: Some(ChipPr {
                    lifecycle: BranchLifecycle::PrOpen,
                    number: 42,
                    review: None,
                    unresolved: None,
                }),
                ..base
            }
        }

        let theme = Theme::wsx();
        let mut specs = bundled_default(&theme);
        // No placement in the top bar at all.
        specs.attached_top.format = format::parse("$workspace").unwrap();
        // Attention lives in the bottom bar's `format`; `right_format`
        // stays the stock one, which draws a PR chip when `pr` is set.
        specs.attached_bottom.format = format::parse("( $attention)").unwrap();

        // Measure the PR chip's own rendered width independently, via a
        // wide, non-dropping render of the bottom bar's hits.
        let (_, bottom) = attached_bars(
            &specs,
            &theme,
            pr_input(inputs("wsx", "foo", None)),
            200,
            200,
        );
        let pr_hit = bottom
            .hits
            .iter()
            .find(|h| h.hit == Hit::Pr)
            .expect("pr chip rendered");
        let pr_width = usize::from(pr_hit.width);

        let budget =
            attention_width_budget(&specs, &theme, pr_input(inputs("wsx", "foo", None)), 80);

        // Probe side (left, `format`): "( $attention)" -> " x", 2 cells,
        // minus the 1 probe cell = 1. Other side (right): the stock
        // `right_format`'s leading literal space plus the PR chip's own
        // width, plus 1 mandatory blank.
        assert_eq!(budget, 80 - 1 - (1 + pr_width + 1));
    }

    /// `$attention` absent from all four attached-bar format strings:
    /// nothing constrains the items, so the full width is available.
    #[test]
    fn attention_absent_everywhere_leaves_the_full_width() {
        let theme = Theme::wsx();
        let mut specs = bundled_default(&theme);
        specs.attached_top.format = format::parse("$workspace").unwrap();
        let budget = attention_width_budget(&specs, &theme, inputs("wsx", "foo", None), 80);
        assert_eq!(budget, 80);
    }
}

#[cfg(test)]
mod attached_bars_tests {
    use super::*;
    use crate::config::theme_file::bundled_default;

    /// A segment normally in the bottom bar (`keys`, the `^x menu` pill)
    /// renders — and keeps its hit — when the theme puts it in the top
    /// bar's format instead: proof the two bars share one segment map.
    #[test]
    fn bottom_bar_segment_renders_in_top_bar_with_its_hit() {
        let theme = Theme::wsx();
        let mut specs = bundled_default(&theme);
        specs.attached_top.format = format::parse("$keys").unwrap();
        let (top, _bottom) = attached_bars(
            &specs,
            &theme,
            AttachedInputs {
                repo: "wsx",
                name: "foo",
                version: "0.1.0",
                window_label: "24h",
                activity: &[],
                agent: None,
                attention: None,
                pinned: &[],
                procs: 0,
                diff: None,
                pr: None,
                model_tokens: None,
                agents: &[],
                active_agent: None,
            },
            60,
            60,
        );
        assert!(
            test_util::plain(&top.line).starts_with(" ^x  menu"),
            "{:?}",
            test_util::plain(&top.line)
        );
        assert_eq!(top.hits[0].start_col, 0);
        assert_eq!(top.hits[0].hit, Hit::ArmLeader);
    }
}

#[cfg(test)]
mod footer_tests {
    use super::*;
    use crate::config::theme_file::bundled_default;
    use crate::ui::bar::test_util::{plain, render_line};
    use crossterm::event::KeyCode;

    fn footer(selected: bool, label: &str, width: u16) -> Rendered {
        let theme = Theme::wsx();
        let specs = bundled_default(&theme);
        let activity: Vec<u32> = (0..24).collect();
        dashboard_footer(
            &specs,
            &theme,
            &DashboardFooterInputs {
                activity: &activity,
                version: "0.1.0",
                window_label: label,
                workspace_selected: selected,
            },
            width,
        )
    }

    #[test]
    fn default_footer_snapshot() {
        let out = footer(true, "24h", 120);
        let text = plain(&out.line);
        assert!(
            text.starts_with(
                " ↑↓  nav   ↵  open   n  new   G  group   o  order   /  filter   ?  actions   q  quit"
            ),
            "{text:?}"
        );
        let spark = crate::ui::dashboard::sparkline::render(&(0..24).collect::<Vec<u32>>(), 24);
        assert!(text.ends_with(&format!("0.1.0  24h {spark}")), "{text:?}");
        assert_eq!(out.line.width(), 120);
        assert_eq!(
            out.hits
                .iter()
                .filter(|h| matches!(h.hit, Hit::Key(_)))
                .count(),
            8
        );
    }

    #[test]
    fn footer_omits_actions_pill_without_workspace() {
        assert!(!plain(&footer(false, "24h", 120).line).contains("actions"));
        assert!(plain(&footer(true, "24h", 120).line).contains("actions"));
    }

    #[test]
    fn footer_key_pill_wraps_key_only_not_label() {
        let theme = Theme::wsx();
        let out = footer(true, "24h", 120);
        let buf = render_line(&out.line, 120);
        // " ↑↓ " is cols 0..4 on the chip bg; " nav" follows on the bar bg.
        assert_eq!(buf[(1, 0)].bg, theme.bg_soft);
        assert_eq!(buf[(5, 0)].symbol(), "n");
        assert_ne!(buf[(5, 0)].bg, theme.bg_soft);
    }

    #[test]
    fn footer_hints_align_with_rendered_key_pills() {
        let out = footer(true, "24h", 120);
        let buf = render_line(&out.line, 120);
        let order = out
            .hits
            .iter()
            .find(|h| matches!(h.hit, Hit::Key(k) if k.code == KeyCode::Char('o')))
            .expect("order hint");
        let cells: String = (order.start_col..order.start_col + order.width)
            .map(|x| buf[(x, 0)].symbol().to_string())
            .collect();
        assert_eq!(cells, " o  order");
    }

    #[test]
    fn footer_usage_hit_covers_label_and_sparkline() {
        let out = footer(true, "1w", 120);
        let usage = out.hits.iter().find(|h| h.hit == Hit::UsageGraph).unwrap();
        assert_eq!(usage.width, 2 + 1 + 24);
        assert_eq!(usage.start_col + usage.width, 120);
    }

    // At 100 columns the full content (keys 71 + gap 1 + version 5 + "  " 2
    // + usage 28 = 107, without the actions pill) doesn't fit. `version`'s
    // lower priority drops it first: keys 71 + gap 1 + usage 28 = 100 fits
    // exactly, so the usage graph survives and lands flush against the
    // right edge. `workspace_selected: false` (no `actions` pill) — with
    // it, keys alone are already 84 wide, leaving no room for usage either,
    // so this specifically exercises version-drops-before-usage rather than
    // everything-drops.
    #[test]
    fn narrow_footer_drops_version_before_usage() {
        let out = footer(false, "24h", 100);
        let text = plain(&out.line);
        assert!(!text.contains("0.1.0"), "{text:?}");
        let spark = crate::ui::dashboard::sparkline::render(&(0..24).collect::<Vec<u32>>(), 24);
        assert!(text.ends_with(&format!("24h {spark}")), "{text:?}");
        assert_eq!(out.line.width(), 100);
        let usage = out.hits.iter().find(|h| h.hit == Hit::UsageGraph).unwrap();
        assert_eq!(usage.start_col + usage.width, 100);

        // Narrower still (60): even usage alone no longer fits, so both
        // right-side segments are gone — but the left side is never
        // dropped, only clipped by whatever renders the (now wider than
        // requested) line.
        let out = footer(false, "24h", 60);
        let text = plain(&out.line);
        assert!(!text.contains("0.1.0"), "{text:?}");
        assert!(!text.contains("24h"), "{text:?}");
        assert_eq!(
            out.hits
                .iter()
                .filter(|h| matches!(h.hit, Hit::Key(_)))
                .count(),
            7
        );
    }

    /// Durable evidence for fix round 2: these two strings were verified
    /// byte-for-byte against the pre-Task-7 legacy footer builder
    /// (temporarily restored in git history for that one check, then
    /// removed again — see the `engine_footer_matches_legacy_footer` commit
    /// history) before this test was written. Pinning them here means a
    /// future change to the bundled default's overflow priorities gets
    /// caught without needing to resurrect the legacy code again.
    ///
    /// `workspace_selected: false` fits its full content at 110 (keys 71 +
    /// gap 4 + version 5 + "  " 2 + usage 28 = 110) without dropping
    /// anything, so it matches legacy exactly. `workspace_selected: true`
    /// does NOT: with the `actions` pill, keys alone are 84 wide, leaving
    /// only 110 - 84 - 1 = 25 cells for the right side — 3 short of even
    /// `usage` alone (28) — so both `version` and `usage` drop and legacy
    /// parity does not apply (legacy has no such drop and would overflow
    /// to 120 cells instead); this asserts the engine's own, intentional
    /// behavior at that width.
    #[test]
    fn default_footer_snapshot_at_110() {
        let spark = crate::ui::dashboard::sparkline::render(&(0..24).collect::<Vec<u32>>(), 24);

        let out = footer(false, "24h", 110);
        let expected = format!(
            "{}{}0.1.0  24h {spark}",
            " ↑↓  nav   ↵  open   n  new   G  group   o  order   /  filter   q  quit",
            " ".repeat(4),
        );
        assert_eq!(plain(&out.line), expected);
        assert_eq!(out.line.width(), 110);
        let usage = out.hits.iter().find(|h| h.hit == Hit::UsageGraph).unwrap();
        assert_eq!(usage.start_col + usage.width, 110);
        assert_eq!(
            out.hits
                .iter()
                .filter(|h| matches!(h.hit, Hit::Key(_)))
                .count(),
            7
        );

        let out = footer(true, "24h", 110);
        let expected = format!(
            "{}{}",
            " ↑↓  nav   ↵  open   n  new   G  group   o  order   /  filter   ?  actions   q  quit",
            " ".repeat(26)
        );
        assert_eq!(plain(&out.line), expected);
        assert_eq!(out.line.width(), 110);
        assert!(out.hits.iter().all(|h| h.hit != Hit::UsageGraph));
        assert_eq!(
            out.hits
                .iter()
                .filter(|h| matches!(h.hit, Hit::Key(_)))
                .count(),
            8
        );
    }
}

#[cfg(test)]
mod bottom_tests {
    use super::*;
    use crate::commands::pinned::PinnedCommand;
    use crate::config::theme_file::bundled_default;
    use crate::data::store::AgentInstanceId;
    use crate::git::forge::{BranchLifecycle, ReviewDecision};
    use crate::pty::session::AgentKind;
    use crate::ui::attached::ChipPr;
    use crate::ui::bar::test_util::{plain, render_line};
    use crate::ui::detail_modules::session_summary::ChipModelTokens;

    fn cmds(specs: &[(&str, &str)]) -> Vec<PinnedCommand> {
        specs
            .iter()
            .map(|(l, c)| PinnedCommand {
                label: (*l).into(),
                command: (*c).into(),
            })
            .collect()
    }
    fn pr(review: Option<ReviewDecision>) -> Option<ChipPr> {
        Some(ChipPr {
            lifecycle: BranchLifecycle::PrOpen,
            number: 42,
            review,
            unresolved: None,
        })
    }
    fn mt() -> Option<ChipModelTokens> {
        Some(ChipModelTokens {
            model: Some("opus 4.8".into()),
            tokens: "45k/200k".into(),
            warn: false,
        })
    }
    fn agents() -> Vec<(AgentInstanceId, AgentKind, String, Option<char>)> {
        vec![
            (
                AgentInstanceId(1),
                AgentKind::Claude,
                "claude".into(),
                Some('q'),
            ),
            (
                AgentInstanceId(2),
                AgentKind::Codex,
                "codex".into(),
                Some('w'),
            ),
        ]
    }
    fn render(inputs: AttachedInputs<'_>, width: u16) -> Rendered {
        let theme = Theme::wsx();
        let specs = bundled_default(&theme);
        attached_bars(&specs, &theme, inputs, width, width).1
    }
    fn full<'a>(
        pinned: &'a [PinnedCommand],
        agents: &'a [(AgentInstanceId, AgentKind, String, Option<char>)],
    ) -> AttachedInputs<'a> {
        AttachedInputs {
            repo: "wsx",
            name: "foo",
            version: "0.1.0",
            window_label: "24h",
            activity: &[],
            agent: None,
            attention: None,
            pinned,
            procs: 3,
            diff: Some(crate::git::DiffStats {
                added: 12,
                removed: 3,
            }),
            pr: pr(Some(ReviewDecision::Approved)),
            model_tokens: mt(),
            agents,
            active_agent: Some(AgentInstanceId(1)),
        }
    }
    fn present(out: &Rendered) -> Vec<&'static str> {
        let t = plain(&out.line);
        let mut v = Vec::new();
        if t.contains("claude") {
            v.push("agents");
        }
        if t.contains("45k/200k") {
            v.push("model_tokens");
        }
        if t.contains("3p") {
            v.push("procs");
        }
        if t.contains("+12") {
            v.push("diff");
        }
        if t.contains("#42") {
            v.push("pr");
        }
        v
    }

    #[test]
    fn default_bottom_snapshot_and_hits() {
        let pinned = cmds(&[("PR", "/pr"), ("feedback", "/fb")]);
        let agents = agents();
        let out = render(full(&pinned, &agents), 120);
        let t = plain(&out.line);
        // keys ` ^x  menu`, two literal spaces, chips ` 1  PR` and ` 2  feedback`
        // (pill pad + `index` pad + space-led label), two spaces, then the rule.
        assert!(
            t.starts_with(" ^x  menu   1  PR   2  feedback  ──"),
            "{t:?}"
        );
        // Each agent pill ends with its ` q ` key pill (trailing pad), then the
        // 3-cell separator / group gap, so four spaces precede `○` and `opus`.
        assert!(
            t.ends_with("● claude  q    ○ codex  w    opus 4.8 45k/200k ● 3p +12 −3 ⏺ #42 open ✓"),
            "{t:?}"
        );
        assert_eq!(out.line.width(), 120);
        let chips: Vec<_> = out
            .hits
            .iter()
            .filter(|h| matches!(h.hit, Hit::PinnedChip(_)))
            .collect();
        assert_eq!(chips.len(), 2);
        assert_eq!(chips[0].width, 6, "` 1  PR` is 6 cells");
        assert_eq!(chips[1].width, 12, "` 2  feedback` is 12 cells");
        assert_eq!(chips[1].start_col, chips[0].start_col + 6 + 2, "2-cell gap");
        let leader = out.hits.iter().find(|h| h.hit == Hit::ArmLeader).unwrap();
        assert_eq!((leader.start_col, leader.width), (0, 9));
        let pr = out.hits.iter().find(|h| h.hit == Hit::Pr).unwrap();
        assert_eq!(pr.start_col + pr.width, 120, "PR chip hugs the right edge");
        let ids: Vec<_> = out
            .hits
            .iter()
            .filter_map(|h| match h.hit {
                Hit::Agent(id) => Some(id),
                _ => None,
            })
            .collect();
        assert_eq!(ids, vec![AgentInstanceId(1), AgentInstanceId(2)]);
    }

    #[test]
    fn zero_procs_and_clean_diff_render_nothing() {
        let pinned = cmds(&[]);
        let agents = vec![];
        let mut inputs = full(&pinned, &agents);
        inputs.procs = 0;
        inputs.diff = Some(crate::git::DiffStats {
            added: 0,
            removed: 0,
        });
        let out = render(inputs, 120);
        assert_eq!(present(&out), vec!["model_tokens", "pr"]);
        assert!(out.hits.iter().all(|h| h.hit != Hit::Procs));
    }

    #[test]
    fn pr_mark_is_absent_without_a_verdict() {
        let pinned = cmds(&[]);
        let agents = vec![];
        let mut inputs = full(&pinned, &agents);
        inputs.pr = pr(None);
        let t = plain(&render(inputs, 120).line);
        assert!(t.ends_with("⏺ #42 open"), "{t:?}");
    }

    #[test]
    fn narrow_rows_drop_model_tokens_then_agents_then_procs_then_diff() {
        let pinned = cmds(&[("PR", "/pr")]);
        let agents = agents();
        // Left side is 19 cells (` ^x  menu` + 2 + ` 1  PR` + 2). Right side at
        // full strength is 73: 2 + agents 26 + 3 + model 17 + 1 + procs 4 + 1
        // + diff 6 + 1 + pr 12. Dropping model removes 18, agents 29, procs 5,
        // diff 7.
        let widths_and_expect: [(u16, &[&str]); 5] = [
            (120, &["agents", "model_tokens", "procs", "diff", "pr"]),
            (80, &["agents", "procs", "diff", "pr"]),
            (50, &["procs", "diff", "pr"]),
            (40, &["diff", "pr"]),
            (35, &["pr"]),
        ];
        for (w, expect) in widths_and_expect {
            let out = render(full(&pinned, &agents), w);
            assert_eq!(
                present(&out),
                expect.to_vec(),
                "width {w}: {:?}",
                plain(&out.line)
            );
        }
    }

    #[test]
    fn model_tokens_warn_style_is_the_warn_color() {
        let theme = Theme::wsx();
        let pinned = cmds(&[]);
        let agents = vec![];
        let mut inputs = full(&pinned, &agents);
        inputs.model_tokens = Some(ChipModelTokens {
            model: None,
            tokens: "190k/200k".into(),
            warn: true,
        });
        inputs.pr = None;
        inputs.diff = None;
        inputs.procs = 0;
        let out = render(inputs, 60);
        let buf = render_line(&out.line, 60);
        // No PR: the accepted trailing-blank defect (see the theme comment)
        // lands at the very last cell; the tokens' own last character sits
        // one cell in from it.
        assert_eq!(buf[(58, 0)].symbol(), "k");
        assert_eq!(buf[(58, 0)].fg, theme.warn);
        assert_eq!(buf[(59, 0)].symbol(), " ");
    }

    /// A single-agent workspace renders no agent pills (`agents` needs 2+ to
    /// show any), so `model_tokens` becomes the first element of the
    /// flush-right block. With trailing per-element separators, a missing
    /// element takes its own gap with it — the rule is followed by exactly
    /// the stock two-cell gap, not the 4-cell gap that would land after
    /// agents specifically (3-cell group gap + 1-cell model_tokens gap).
    #[test]
    fn single_agent_stats_block_sits_two_cells_after_the_rule() {
        let pinned = cmds(&[]);
        let agents: Vec<(AgentInstanceId, AgentKind, String, Option<char>)> = vec![];
        let mut inputs = full(&pinned, &agents);
        inputs.agents = &[];
        inputs.active_agent = None;
        let out = render(inputs, 120);
        let t = plain(&out.line);
        assert!(t.contains("──  opus 4.8 45k/200k"), "{t:?}");
        assert!(!t.contains("   opus"), "{t:?}");
    }

    /// The accepted tradeoff of trailing separators: `$pr` is bare (no
    /// trailing separator of its own), so a workspace with no PR leaves the
    /// diff count's own trailing gap as one dangling blank cell at the very
    /// right edge, instead of hugging it exactly.
    #[test]
    fn no_pr_leaves_only_a_trailing_blank() {
        let pinned = cmds(&[]);
        let agents: Vec<(AgentInstanceId, AgentKind, String, Option<char>)> = vec![];
        let mut inputs = full(&pinned, &agents);
        inputs.agents = &[];
        inputs.active_agent = None;
        inputs.pr = None;
        let out = render(inputs, 120);
        let t = plain(&out.line);
        assert!(t.ends_with("+12 −3 "), "{t:?}");
        assert_eq!(out.line.width(), 120);
    }

    /// `[pr].symbol` overrides the lifecycle glyph the provider would
    /// otherwise use — the amendment to Task 8/9's shared-segment-map plan.
    #[test]
    fn pr_symbol_overrides_lifecycle_glyph() {
        let theme = Theme::wsx();
        let mut specs = bundled_default(&theme);
        specs.segments.get_mut("pr").unwrap().symbol = Some("X".into());
        let pinned = cmds(&[]);
        let agents = vec![];
        let inputs = AttachedInputs {
            repo: "wsx",
            name: "foo",
            version: "0.1.0",
            window_label: "24h",
            activity: &[],
            agent: None,
            attention: None,
            pinned: &pinned,
            procs: 0,
            diff: None,
            pr: pr(None),
            model_tokens: None,
            agents: &agents,
            active_agent: None,
        };
        let out = attached_bars(&specs, &theme, inputs, 120, 120).1;
        let t = plain(&out.line);
        assert!(t.ends_with("X #42 open"), "{t:?}");
    }
}
