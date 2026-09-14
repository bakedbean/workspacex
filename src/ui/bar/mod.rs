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
}
