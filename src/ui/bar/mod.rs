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

    // The footer never sheds a segment on overflow (unlike the attached
    // bars, which lean on priority): the legacy builder never did either,
    // it just let the line run past the terminal edge and relied on the
    // renderer to clip it. Render at whatever width the content actually
    // needs so the engine never drops anything here; a too-narrow terminal
    // then clips the same way the legacy line did.
    let base = resolver
        .resolve(&specs.dashboard_footer.style)
        .unwrap_or_default();
    let (left, _) = eval(&specs.dashboard_footer.format, &segments, &resolver, base);
    let (right, _) = eval(
        &specs.dashboard_footer.right_format,
        &segments,
        &resolver,
        base,
    );
    let needed = left
        .width
        .saturating_add(right.width)
        .saturating_add(u16::from(!left.is_empty() && !right.is_empty()));
    let mut rendered = render_bar(
        &specs.dashboard_footer,
        &segments,
        &specs.segments,
        width.max(needed),
        &resolver,
    );

    // The legacy builder always anchored the usage graph's click target to
    // the terminal's right edge (`area.width - graph_w`), never to where
    // the (possibly overflowing) text actually sits. Match that so a click
    // near the edge still opens the usage picker on a narrow terminal; this
    // is a no-op whenever the bar isn't overflowing, since the segment
    // already sits flush against the right edge in that case.
    if let Some(hit) = rendered.hits.iter_mut().find(|h| h.hit == Hit::UsageGraph) {
        hit.start_col = width.saturating_sub(hit.width);
    }
    rendered
}
