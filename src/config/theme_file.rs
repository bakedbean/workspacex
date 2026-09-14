//! `~/.config/wsx/theme.toml`: serde model, merge over the bundled default,
//! validation of every format string and color name, and the resolved
//! [`BarSpecs`] the renderer draws from. Parsing happens here, once per
//! load; drawing never parses.
//!
//! See `docs/superpowers/specs/2026-09-13-bar-theming-design.md`.

use crate::ui::bar::format::{self, Node};
use crate::ui::bar::registry::{SEGMENTS, segment_def, singleton_names};
use crate::ui::bar::render::BarSpec;
use crate::ui::bar::segment::SegmentConfig;
use crate::ui::bar::style::{self, ColorRef, Resolver, StyleSpec};
use crate::ui::theme::Theme;
use ratatui::style::{Color, Style};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

pub const DEFAULT_TOML: &str = include_str!("../ui/bar/default_theme.toml");

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ThemeFile {
    #[serde(default)]
    pub palette: BTreeMap<String, String>,
    #[serde(default)]
    pub dashboard_footer: BarTable,
    #[serde(default)]
    pub attached_top: BarTable,
    #[serde(default)]
    pub attached_bottom: BarTable,
    /// Every other top-level table is a `[segment]`.
    #[serde(flatten)]
    pub segments: BTreeMap<String, SegmentTable>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BarTable {
    pub format: Option<String>,
    pub right_format: Option<String>,
    pub style: Option<String>,
    pub fill: Option<String>,
    pub fill_style: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SegmentTable {
    pub format: Option<String>,
    pub style: Option<String>,
    pub symbol: Option<String>,
    pub disabled: Option<bool>,
    pub priority: Option<u32>,
    pub separator: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeError {
    /// `[table].field`, `[table]`, `toml`, or a path.
    pub location: String,
    pub message: String,
}

impl std::fmt::Display for ThemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

fn error(location: impl Into<String>, message: impl Into<String>) -> ThemeError {
    ThemeError {
        location: location.into(),
        message: message.into(),
    }
}

impl BarTable {
    fn merge_over(self, base: BarTable) -> BarTable {
        BarTable {
            format: self.format.or(base.format),
            right_format: self.right_format.or(base.right_format),
            style: self.style.or(base.style),
            fill: self.fill.or(base.fill),
            fill_style: self.fill_style.or(base.fill_style),
        }
    }
}

impl SegmentTable {
    fn merge_over(self, base: SegmentTable) -> SegmentTable {
        SegmentTable {
            format: self.format.or(base.format),
            style: self.style.or(base.style),
            symbol: self.symbol.or(base.symbol),
            disabled: self.disabled.or(base.disabled),
            priority: self.priority.or(base.priority),
            separator: self.separator.or(base.separator),
        }
    }
}

impl ThemeFile {
    pub fn parse(src: &str) -> Result<Self, ThemeError> {
        toml::from_str(src).map_err(|e| error("toml", e.to_string()))
    }

    /// Fill every unset field from `base`; palette entries and segment
    /// tables are unioned, with `self` winning per field.
    pub fn merge_over(mut self, base: ThemeFile) -> ThemeFile {
        for (k, v) in base.palette {
            self.palette.entry(k).or_insert(v);
        }
        self.dashboard_footer = self.dashboard_footer.merge_over(base.dashboard_footer);
        self.attached_top = self.attached_top.merge_over(base.attached_top);
        self.attached_bottom = self.attached_bottom.merge_over(base.attached_bottom);
        for (name, tbl) in base.segments {
            let mine = self.segments.remove(&name).unwrap_or_default();
            self.segments.insert(name, mine.merge_over(tbl));
        }
        self
    }
}

/// Fully parsed and validated bar theme. Owned by `App`; rebuilt on reload.
#[derive(Debug, Clone)]
pub struct BarSpecs {
    pub palette: HashMap<String, Color>,
    pub dashboard_footer: BarSpec,
    pub attached_top: BarSpec,
    pub attached_bottom: BarSpec,
    pub segments: HashMap<String, SegmentConfig>,
}

impl BarSpecs {
    pub fn resolver<'a>(&'a self, theme: &'a Theme) -> Resolver<'a> {
        Resolver::new(&self.palette, theme)
    }
}

fn parse_format(loc: &str, src: &str, errors: &mut Vec<ThemeError>) -> Vec<Node> {
    match format::parse(src) {
        Ok(nodes) => nodes,
        Err(e) => {
            errors.push(error(loc, e.to_string()));
            Vec::new()
        }
    }
}

fn parse_style(loc: &str, src: Option<&str>, errors: &mut Vec<ThemeError>) -> StyleSpec {
    match StyleSpec::parse(src.unwrap_or("")) {
        Ok(s) => s,
        Err(e) => {
            errors.push(error(loc, e.to_string()));
            StyleSpec::default()
        }
    }
}

/// Check every `$var` in `nodes` is in `allowed`, and every style resolves
/// against `resolver` (which carries the allowed `$style` names).
fn validate(
    loc: &str,
    nodes: &[Node],
    allowed: &[&str],
    resolver: &Resolver,
    errors: &mut Vec<ThemeError>,
) {
    for v in format::vars(nodes) {
        if !allowed.contains(&v) {
            errors.push(error(
                loc,
                format!("unknown `${v}` (allowed: {})", allowed.join(", ")),
            ));
        }
    }
    for spec in format::styles(nodes) {
        if let Err(e) = resolver.resolve(spec) {
            errors.push(error(loc, e.to_string()));
        }
    }
}

fn placeholder_styles(names: &[&str]) -> HashMap<String, Style> {
    names
        .iter()
        .map(|n| (n.to_string(), Style::default()))
        .collect()
}

/// Parse a style string and confirm it resolves, in one step — the shared
/// tail of segment style, bar style, and bar `fill_style` validation.
fn styled(
    loc: &str,
    src: Option<&str>,
    resolver: &Resolver,
    errors: &mut Vec<ThemeError>,
) -> StyleSpec {
    let style = parse_style(loc, src, errors);
    if let Err(e) = resolver.resolve(&style) {
        errors.push(error(loc, e.to_string()));
    }
    style
}

/// Resolve every `[palette]` entry to a concrete color: a literal parses
/// directly, a named reference resolves against the theme's tokens, then
/// ANSI names; anything else is an error.
fn resolve_palette(
    file: &ThemeFile,
    theme: &Theme,
    errors: &mut Vec<ThemeError>,
) -> HashMap<String, Color> {
    let mut palette = HashMap::new();
    for (name, value) in &file.palette {
        let loc = format!("[palette].{name}");
        match style::color_ref(value) {
            Ok(ColorRef::Literal(c)) => {
                palette.insert(name.clone(), c);
            }
            Ok(ColorRef::Named(n)) => match theme.token(&n).or_else(|| style::ansi(&n)) {
                Some(c) => {
                    palette.insert(name.clone(), c);
                }
                None => errors.push(error(loc, format!("unknown color `{value}`"))),
            },
            Err(e) => errors.push(error(loc, e.to_string())),
        }
    }
    palette
}

/// Validate one `[segment]` table's `format` (only its own `SegmentDef`
/// vars) and `style`, and build its `SegmentConfig`; `None` (after pushing
/// an error) for a name with no known definition.
fn resolve_segment(
    name: &str,
    tbl: &SegmentTable,
    resolver: &Resolver,
    errors: &mut Vec<ThemeError>,
) -> Option<SegmentConfig> {
    let Some(def) = segment_def(name) else {
        errors.push(error(
            format!("[{name}]"),
            format!(
                "unknown segment (known: {})",
                SEGMENTS
                    .iter()
                    .map(|d| d.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
        return None;
    };
    let loc = format!("[{name}].format");
    let nodes = parse_format(&loc, tbl.format.as_deref().unwrap_or(""), errors);
    let seg_resolver = resolver.with_styles(placeholder_styles(def.style_vars));
    validate(&loc, &nodes, def.vars, &seg_resolver, errors);
    let style = styled(
        &format!("[{name}].style"),
        tbl.style.as_deref(),
        resolver,
        errors,
    );
    Some(SegmentConfig {
        style,
        symbol: tbl.symbol.clone(),
        format: nodes,
        disabled: tbl.disabled.unwrap_or(false),
        priority: tbl.priority.unwrap_or(100),
        separator: tbl.separator.clone().unwrap_or_else(|| "  ".to_string()),
    })
}

/// Validate one bar's `format`/`right_format` (may reference only segment
/// names) plus its `style` and `fill_style`, and build its `BarSpec`.
fn resolve_bar(
    name: &str,
    tbl: &BarTable,
    segment_names: &[&str],
    resolver: &Resolver,
    errors: &mut Vec<ThemeError>,
) -> BarSpec {
    let f_loc = format!("[{name}].format");
    let format_nodes = parse_format(&f_loc, tbl.format.as_deref().unwrap_or(""), errors);
    validate(&f_loc, &format_nodes, segment_names, resolver, errors);
    let r_loc = format!("[{name}].right_format");
    let right_nodes = parse_format(&r_loc, tbl.right_format.as_deref().unwrap_or(""), errors);
    validate(&r_loc, &right_nodes, segment_names, resolver, errors);
    let style = styled(
        &format!("[{name}].style"),
        tbl.style.as_deref(),
        resolver,
        errors,
    );
    let fill_style = styled(
        &format!("[{name}].fill_style"),
        tbl.fill_style.as_deref(),
        resolver,
        errors,
    );
    BarSpec {
        format: format_nodes,
        right_format: right_nodes,
        style,
        fill: tbl.fill.clone().unwrap_or_else(|| " ".to_string()),
        fill_style,
    }
}

/// Reject a singleton segment (see [`crate::ui::bar::registry::SegmentDef::singleton`])
/// placed more than once among the bars that would each try to route its
/// one click target — the attached pair together, and the dashboard
/// footer's own two sides.
fn check_singletons(
    dashboard: &BarSpec,
    top: &BarSpec,
    bottom: &BarSpec,
    errors: &mut Vec<ThemeError>,
) {
    let count_var = |groups: &[&[Node]], name: &str| -> usize {
        groups
            .iter()
            .flat_map(|nodes| format::vars(nodes))
            .filter(|v| *v == name)
            .count()
    };
    let attached_nodes: [&[Node]; 4] = [
        &top.format,
        &top.right_format,
        &bottom.format,
        &bottom.right_format,
    ];
    for name in singleton_names() {
        let count = count_var(&attached_nodes, name);
        if count > 1 {
            errors.push(error(
                "[attached_top]/[attached_bottom]",
                format!(
                    "segment `${name}` carries one click target and may appear only once across the attached bars (found {count}; a bar side you did not set keeps its bundled default, so set that `format`/`right_format` to \"\" to clear it)"
                ),
            ));
        }
    }
    let footer_nodes: [&[Node]; 2] = [&dashboard.format, &dashboard.right_format];
    for name in singleton_names() {
        let count = count_var(&footer_nodes, name);
        if count > 1 {
            errors.push(error(
                "[dashboard_footer]",
                format!(
                    "segment `${name}` carries one click target and may appear only once in the dashboard footer (found {count}; a bar side you did not set keeps its bundled default, so set that `format`/`right_format` to \"\" to clear it)"
                ),
            ));
        }
    }
}

/// Merge `file` over the bundled default and resolve it. Every problem is
/// reported, not just the first.
pub fn resolve(file: ThemeFile, theme: &Theme) -> Result<BarSpecs, Vec<ThemeError>> {
    let base = ThemeFile::parse(DEFAULT_TOML).expect("bundled default_theme.toml parses");
    let file = file.merge_over(base);
    let mut errors = Vec::new();

    let palette = resolve_palette(&file, theme, &mut errors);
    let resolver = Resolver::new(&palette, theme);

    let mut segments = HashMap::new();
    for (name, tbl) in &file.segments {
        if let Some(cfg) = resolve_segment(name, tbl, &resolver, &mut errors) {
            segments.insert(name.clone(), cfg);
        }
    }

    let segment_names: Vec<&str> = SEGMENTS.iter().map(|d| d.name).collect();
    let dashboard_footer = resolve_bar(
        "dashboard_footer",
        &file.dashboard_footer,
        &segment_names,
        &resolver,
        &mut errors,
    );
    let attached_top = resolve_bar(
        "attached_top",
        &file.attached_top,
        &segment_names,
        &resolver,
        &mut errors,
    );
    let attached_bottom = resolve_bar(
        "attached_bottom",
        &file.attached_bottom,
        &segment_names,
        &resolver,
        &mut errors,
    );

    check_singletons(
        &dashboard_footer,
        &attached_top,
        &attached_bottom,
        &mut errors,
    );

    if errors.is_empty() {
        Ok(BarSpecs {
            palette,
            dashboard_footer,
            attached_top,
            attached_bottom,
            segments,
        })
    } else {
        Err(errors)
    }
}

/// The bundled default as resolved specs. Panics only if the embedded
/// TOML is broken, which `bundled_default_parses_and_validates` guards.
pub fn bundled_default(theme: &Theme) -> BarSpecs {
    resolve(ThemeFile::default(), theme).expect("bundled default_theme.toml validates")
}

/// Load `path` merged over the bundled default. A missing file is the
/// bundled default; any other read error, parse error, or validation
/// error is returned.
pub fn load(path: &Path, theme: &Theme) -> Result<BarSpecs, Vec<ThemeError>> {
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(bundled_default(theme)),
        Err(e) => return Err(vec![error(path.display().to_string(), e.to_string())]),
    };
    let file = ThemeFile::parse(&src).map_err(|e| vec![e])?;
    resolve(file, theme)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn errs(src: &str) -> Vec<ThemeError> {
        resolve(ThemeFile::parse(src).unwrap(), &Theme::wsx()).unwrap_err()
    }
    fn ok(src: &str) -> BarSpecs {
        resolve(ThemeFile::parse(src).unwrap(), &Theme::wsx()).unwrap()
    }

    #[test]
    fn bundled_default_parses_and_validates() {
        let specs = bundled_default(&Theme::wsx());
        assert_eq!(
            specs.dashboard_footer.format,
            format::parse("$keys").unwrap()
        );
        assert_eq!(specs.attached_bottom.fill, "─");
        assert_eq!(specs.segments["pr"].priority, 50);
        assert_eq!(specs.segments["keys"].separator, "  ");
        assert_eq!(specs.segments["procs"].symbol.as_deref(), Some("●"));
        for def in SEGMENTS {
            assert!(
                specs.segments.contains_key(def.name),
                "default lacks [{}]",
                def.name
            );
        }
    }

    #[test]
    fn partial_file_merges_per_field_over_the_default() {
        let specs = ok("[attached_top]\nformat = \"$workspace\"\n[pr]\npriority = 7\n");
        assert_eq!(
            specs.attached_top.format,
            format::parse("$workspace").unwrap()
        );
        assert_eq!(
            specs.dashboard_footer.format,
            format::parse("$keys").unwrap()
        );
        assert_eq!(specs.segments["pr"].priority, 7);
        assert_eq!(
            specs.segments["pr"].format,
            format::parse("[$symbol #$number $label]($style)( [$mark]($mark_style))").unwrap(),
            "unset fields keep the default"
        );
    }

    #[test]
    fn palette_accepts_literals_theme_tokens_and_ansi() {
        let specs =
            ok("[palette]\nfirst = \"#123456\"\nsecond = \"dim\"\nthird = \"bright-red\"\n");
        assert_eq!(specs.palette["first"], Color::Rgb(0x12, 0x34, 0x56));
        assert_eq!(specs.palette["second"], Theme::wsx().dim);
        assert_eq!(specs.palette["third"], Color::LightRed);
    }

    #[test]
    fn palette_names_resolve_in_styles() {
        let specs = ok(
            "[palette]\nfirst = \"#123456\"\n[attached_top]\nformat = \"[$workspace](bg:first)\"\n",
        );
        let theme = Theme::wsx();
        let r = specs.resolver(&theme);
        let Node::Styled(_, spec) = &specs.attached_top.format[0] else {
            panic!()
        };
        assert_eq!(
            r.resolve(spec).unwrap().bg,
            Some(Color::Rgb(0x12, 0x34, 0x56))
        );
    }

    #[test]
    fn every_error_is_reported_with_a_location() {
        let e = errs(concat!(
            "[palette]\nbad = \"#12\"\n",
            "[bogus]\nformat = \"x\"\n",
            "[dashboard_footer]\nformat = \"$nope [x](fg:rusty)\"\n",
            "[diff]\nformat = \"$foo [x]($mark_style)\"\n",
        ));
        let locs: Vec<&str> = e.iter().map(|e| e.location.as_str()).collect();
        assert!(locs.contains(&"[palette].bad"), "{locs:?}");
        assert!(locs.contains(&"[bogus]"), "{locs:?}");
        assert!(
            locs.iter()
                .filter(|l| **l == "[dashboard_footer].format")
                .count()
                >= 2,
            "{locs:?}"
        );
        assert!(
            locs.iter().filter(|l| **l == "[diff].format").count() >= 2,
            "{locs:?}"
        );
        let msgs: Vec<&str> = e.iter().map(|e| e.message.as_str()).collect();
        assert!(msgs.iter().any(|m| m.contains("nope")), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("rusty")), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("foo")), "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("mark_style")), "{msgs:?}");
    }

    #[test]
    fn segment_style_vars_are_per_segment() {
        assert!(
            resolve(
                ThemeFile::parse("[pr]\nformat = \"[$mark]($mark_style)\"\n").unwrap(),
                &Theme::wsx()
            )
            .is_ok()
        );
        assert!(!errs("[procs]\nformat = \"[$count]($mark_style)\"\n").is_empty());
    }

    #[test]
    fn bar_formats_may_only_reference_segments() {
        let e = errs("[attached_top]\nformat = \"$symbol\"\n");
        assert_eq!(e.len(), 1);
        assert!(e[0].message.contains("symbol"));
    }

    #[test]
    fn segment_style_and_bar_style_are_validated() {
        assert!(!errs("[pr]\nstyle = \"fg:nope\"\n").is_empty());
        assert!(!errs("[attached_top]\nstyle = \"bg:\"\n").is_empty());
        assert!(!errs("[attached_top]\nfill_style = \"fg:zzz\"\n").is_empty());
    }

    /// `$pr` in `attached_top.format` plus the default bottom bar's own
    /// `$pr` (in `right_format`) is two placements, so `resolve` rejects it
    /// naming `pr`.
    #[test]
    fn a_singleton_segment_placed_in_both_attached_bars_is_an_error() {
        let e = errs("[attached_top]\nformat = \"$pr\"\n");
        assert!(
            e.iter()
                .any(|e| e.location == "[attached_top]/[attached_bottom]"
                    && e.message.contains('`')
                    && e.message.contains("pr")),
            "{e:?}"
        );
    }

    /// `$usage` twice within the dashboard footer's own two format strings
    /// is also a duplicate placement, even though only one bar is involved.
    #[test]
    fn a_singleton_segment_placed_twice_in_the_dashboard_footer_is_an_error() {
        let e = errs("[dashboard_footer]\nformat = \"$usage\"\nright_format = \"$usage\"\n");
        assert!(
            e.iter()
                .any(|e| e.location == "[dashboard_footer]" && e.message.contains("usage")),
            "{e:?}"
        );
    }

    /// Moving `$pr` to the top bar while removing it from the bottom bar's
    /// `right_format` leaves exactly one placement, which is fine.
    #[test]
    fn moving_a_singleton_segment_between_bars_is_fine() {
        let specs = ok(concat!(
            "[attached_top]\nformat = \"$pr\"\n",
            "[attached_bottom]\nright_format = \"( ($agents   )($model_tokens )($procs )($diff ))\"\n",
        ));
        assert_eq!(specs.attached_top.format, format::parse("$pr").unwrap());
    }

    /// Multi-item segments (`pins`, `agents`, `keys`) record one hit per
    /// item, so placing `$pins` in both attached bars is still allowed.
    #[test]
    fn multi_item_segments_may_still_appear_in_both_bars() {
        let specs = ok("[attached_top]\nformat = \"$pins\"\n");
        assert_eq!(specs.attached_top.format, format::parse("$pins").unwrap());
    }

    #[test]
    fn bad_toml_is_one_error() {
        let e = ThemeFile::parse("[pr\nformat = 1").unwrap_err();
        assert_eq!(e.location, "toml");
    }

    #[test]
    fn unknown_table_keys_are_errors() {
        let e = ThemeFile::parse("[pr]\npriorty = 7\n").unwrap_err();
        assert!(e.message.contains("priorty"), "{}", e.message);
        let e = ThemeFile::parse("[attached_top]\nfromat = \"x\"\n").unwrap_err();
        assert!(e.message.contains("fromat"), "{}", e.message);
    }

    #[test]
    fn load_missing_file_is_the_bundled_default() {
        let dir = tempfile::tempdir().unwrap();
        let specs = load(&dir.path().join("theme.toml"), &Theme::wsx()).unwrap();
        assert_eq!(
            specs.dashboard_footer.format,
            format::parse("$keys").unwrap()
        );
    }

    #[test]
    fn load_reads_and_validates_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("theme.toml");
        std::fs::write(&path, "[dashboard_footer]\nformat = \"$version\"\n").unwrap();
        let specs = load(&path, &Theme::wsx()).unwrap();
        assert_eq!(
            specs.dashboard_footer.format,
            format::parse("$version").unwrap()
        );
        std::fs::write(&path, "[dashboard_footer]\nformat = \"$nope\"\n").unwrap();
        assert!(load(&path, &Theme::wsx()).is_err());
    }
}
