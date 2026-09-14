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
use render::{Rendered, render_bar};
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
///
/// `#[allow(dead_code)]`: `attached_segments` only reads `agent`, `repo`,
/// `name`, and `attention` for now (`keys`/`agent_bar`/`workspace`/
/// `attention`, the Task 8 segments); the rest feed `pins`/`agents`/
/// `model_tokens`/`procs`/`diff`/`pr` once Task 9 adds those segments to
/// the same function.
#[allow(dead_code)]
pub(crate) struct AttachedInputs<'a> {
    pub repo: &'a str,
    pub name: &'a str,
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
