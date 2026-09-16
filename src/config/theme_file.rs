//! `~/.config/wsx/theme.toml`: serde model, merge over the bundled default,
//! validation of every format string and color name, and the resolved
//! [`BarSpecs`] the renderer draws from. Parsing happens here, once per
//! load; drawing never parses.
//!
//! See `docs/superpowers/specs/2026-09-13-bar-theming-design.md`.

use crate::ui::bar::format::{self, Node};
use crate::ui::bar::registry::{ITEM_COLORS, SEGMENTS, segment_def, singleton_names};
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
    /// The dashboard's top line: wordmark, group/sort tabs, filter echo,
    /// and counts. A named field, not part of the flattened `segments`
    /// map, like the other bars.
    #[serde(default)]
    pub dashboard_header: BarTable,
    #[serde(default)]
    pub attached_top: BarTable,
    #[serde(default)]
    pub attached_bottom: BarTable,
    /// The dashboard detail pane's pinned-command row. A named field, not
    /// part of the flattened `segments` map, like the other bars.
    #[serde(default)]
    pub dashboard_detail: BarTable,
    /// User-composed modules, `[module.<name>]`. Declared before the
    /// flattened `segments` map so serde routes the `module` table here
    /// rather than treating it as a segment named `module`.
    #[serde(default)]
    pub module: BTreeMap<String, ModuleTable>,
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
    pub more_format: Option<String>,
    pub styles: Option<Vec<String>>,
    /// `[<segment>.palette]`: colours that shadow `[palette]` and the theme
    /// tokens inside this segment only. Same value grammar as `[palette]`.
    #[serde(default)]
    pub palette: BTreeMap<String, String>,
}

/// A `[module.<name>]` table: a segment composed from fleet variables. No
/// items, so none of the multi-item keys (`separator`, `styles`,
/// `more_format`) and no `symbol`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleTable {
    pub format: Option<String>,
    pub style: Option<String>,
    pub priority: Option<u32>,
    pub disabled: Option<bool>,
}

impl ModuleTable {
    fn merge_over(self, base: ModuleTable) -> ModuleTable {
        ModuleTable {
            format: self.format.or(base.format),
            style: self.style.or(base.style),
            priority: self.priority.or(base.priority),
            disabled: self.disabled.or(base.disabled),
        }
    }
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
            more_format: self.more_format.or(base.more_format),
            styles: self.styles.or(base.styles),
            palette: {
                let mut palette = self.palette;
                for (k, v) in base.palette {
                    palette.entry(k).or_insert(v);
                }
                palette
            },
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
        self.dashboard_header = self.dashboard_header.merge_over(base.dashboard_header);
        self.attached_top = self.attached_top.merge_over(base.attached_top);
        self.attached_bottom = self.attached_bottom.merge_over(base.attached_bottom);
        self.dashboard_detail = self.dashboard_detail.merge_over(base.dashboard_detail);
        for (name, tbl) in base.segments {
            let mine = self.segments.remove(&name).unwrap_or_default();
            self.segments.insert(name, mine.merge_over(tbl));
        }
        for (name, tbl) in base.module {
            let mine = self.module.remove(&name).unwrap_or_default();
            self.module.insert(name, mine.merge_over(tbl));
        }
        self
    }
}

/// Fully parsed and validated bar theme. Owned by `App`; rebuilt on reload.
#[derive(Debug, Clone)]
pub struct BarSpecs {
    pub palette: HashMap<String, Color>,
    pub dashboard_footer: BarSpec,
    pub dashboard_header: BarSpec,
    pub attached_top: BarSpec,
    pub attached_bottom: BarSpec,
    pub dashboard_detail: BarSpec,
    pub segments: HashMap<String, SegmentConfig>,
    /// Names of every `[module.<name>]`, sorted by name (the `BTreeMap`
    /// this is built from yields keys in that order, not table order).
    /// Each has its `SegmentConfig` in `segments` under the same name.
    pub modules: Vec<String>,
}

impl BarSpecs {
    pub fn resolver<'a>(&'a self, theme: &'a Theme) -> Resolver<'a> {
        Resolver::new(&self.palette, theme)
    }

    /// Whether `bar`'s `format` or `right_format` places an enabled
    /// `[module.<name>]`. A layout that allocates a row only when the bar
    /// has something to draw asks this rather than rendering first: it
    /// depends on what the theme places, not on what the fleet currently
    /// counts, so the row does not come and go with the numbers.
    pub fn places_module(&self, bar: &BarSpec) -> bool {
        format::vars(&bar.format)
            .into_iter()
            .chain(format::vars(&bar.right_format))
            .any(|name| {
                self.modules.iter().any(|m| m == name)
                    && self.segments.get(name).is_some_and(|cfg| !cfg.disabled)
            })
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
            let hint = if allowed.is_empty() {
                "no variables are allowed here".to_string()
            } else {
                format!("allowed: {}", allowed.join(", "))
            };
            errors.push(error(loc, format!("unknown `${v}` ({hint})")));
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

/// Resolve every entry of a palette table (`[palette]`, or a segment's
/// `[<segment>.palette]`, named by `table`) to a concrete color: a literal
/// parses directly, a named reference resolves against `base` (the global
/// palette, for a segment's table; empty for the global one, whose entries
/// can't reference each other), then the theme's tokens, then ANSI names;
/// anything else is an error.
fn resolve_palette(
    table: &str,
    entries: &BTreeMap<String, String>,
    base: &HashMap<String, Color>,
    theme: &Theme,
    errors: &mut Vec<ThemeError>,
) -> HashMap<String, Color> {
    let mut palette = HashMap::new();
    for (name, value) in entries {
        let loc = format!("[{table}].{name}");
        // Inside a multi-item segment these names resolve per item, ahead
        // of the palette, so a palette entry by one of them would be
        // silently shadowed there (and by nothing at all for an absent
        // neighbour). Reserve them.
        if crate::ui::bar::registry::ITEM_COLORS.contains(&name.as_str()) {
            errors.push(error(
                loc,
                format!("`{name}` is reserved for a multi-item segment's per-item colours"),
            ));
            continue;
        }
        match style::color_ref(value) {
            Ok(ColorRef::Literal(c)) => {
                palette.insert(name.clone(), c);
            }
            Ok(ColorRef::Named(n)) => match base
                .get(&n)
                .copied()
                .or_else(|| theme.token(&n))
                .or_else(|| style::ansi(&n))
            {
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
    base_resolver: &Resolver,
    errors: &mut Vec<ThemeError>,
) -> Option<SegmentConfig> {
    let palette = resolve_palette(
        &format!("{name}.palette"),
        &tbl.palette,
        base_resolver.palette,
        base_resolver.theme,
        errors,
    );
    // Every format and style of this segment sees its own palette first.
    let base_resolver = &base_resolver.with_overlay(&palette);
    let resolver = base_resolver;
    let Some(def) = segment_def(name) else {
        errors.push(error(
            format!("[{name}]"),
            format!(
                "unknown segment (known: {}); user modules go under [module.<name>]",
                SEGMENTS
                    .iter()
                    .map(|d| d.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
        return None;
    };
    // A multi-item segment's formats may name the item and neighbour
    // colours; nothing else may.
    let item_colors: HashMap<String, Option<Color>> = if def.items {
        ITEM_COLORS
            .iter()
            .map(|n| (n.to_string(), Some(Color::Reset)))
            .collect()
    } else {
        HashMap::new()
    };
    let resolver = &resolver.with_colors(item_colors);
    let loc = format!("[{name}].format");
    let nodes = parse_format(&loc, tbl.format.as_deref().unwrap_or(""), errors);
    let seg_resolver = resolver.with_styles(placeholder_styles(def.style_vars));
    validate(&loc, &nodes, def.vars, &seg_resolver, errors);
    // The separator sits between items, so it has no item variables and no
    // item `$style`: validated against the bare resolver.
    let sep_loc = format!("[{name}].separator");
    let separator = parse_format(&sep_loc, tbl.separator.as_deref().unwrap_or("  "), errors);
    validate(&sep_loc, &separator, &[], resolver, errors);
    // Per-position grades: only a multi-item segment has positions, and a
    // grade can't depend on its neighbours (they depend on it), so entries
    // resolve against the bare resolver.
    let styles_loc = format!("[{name}].styles");
    let styles: Vec<StyleSpec> = match tbl.styles.as_deref() {
        Some(_) if !def.items => {
            errors.push(error(
                &styles_loc,
                "this segment has no items (only keys, pins, agents, attention take `styles`)",
            ));
            Vec::new()
        }
        Some(list) => list
            .iter()
            .enumerate()
            .map(|(i, src)| {
                styled(
                    &format!("{styles_loc}[{i}]"),
                    Some(src),
                    base_resolver,
                    errors,
                )
            })
            .collect(),
        None => Vec::new(),
    };
    // Likewise the overflow tail: no item, so no item `$style`. Only a
    // segment that folds entries has one at all; a tail on any other is
    // rejected outright rather than silently ignored.
    let more_loc = format!("[{name}].more_format");
    let more_format = match tbl.more_format.as_deref() {
        Some(_) if def.more_vars.is_empty() => {
            errors.push(error(
                &more_loc,
                "this segment has no overflow tail (only `attention` and `tags` take `more_format`)",
            ));
            Vec::new()
        }
        Some(src) => {
            let nodes = parse_format(&more_loc, src, errors);
            validate(&more_loc, &nodes, def.more_vars, resolver, errors);
            nodes
        }
        None => Vec::new(),
    };
    let style = styled(
        &format!("[{name}].style"),
        tbl.style.as_deref(),
        base_resolver,
        errors,
    );
    Some(SegmentConfig {
        style,
        symbol: tbl.symbol.clone(),
        format: nodes,
        disabled: tbl.disabled.unwrap_or(false),
        priority: tbl
            .priority
            .unwrap_or(crate::ui::bar::render::DEFAULT_PRIORITY),
        separator,
        more_format,
        styles,
        palette,
    })
}

/// Validate one `[module.<name>]` table: its `format` may reference only
/// `registry::FLEET_VARS` (plus `$style`), and its name may not shadow a
/// built-in segment. Returns the module's `SegmentConfig`.
fn resolve_module(
    name: &str,
    tbl: &ModuleTable,
    resolver: &Resolver,
    errors: &mut Vec<ThemeError>,
) -> Option<SegmentConfig> {
    if segment_def(name).is_some() {
        errors.push(error(
            format!("[module.{name}]"),
            "name collides with a built-in segment; pick another",
        ));
        return None;
    }
    if tbl.format.as_deref().unwrap_or("").trim().is_empty() {
        errors.push(error(format!("[module.{name}]"), "module has no `format`"));
        return None;
    }
    let loc = format!("[module.{name}].format");
    let nodes = parse_format(&loc, tbl.format.as_deref().unwrap_or(""), errors);
    let allowed = crate::ui::bar::registry::fleet_var_names();
    let seg_resolver = resolver.with_styles(placeholder_styles(&["style"]));
    validate(&loc, &nodes, &allowed, &seg_resolver, errors);
    let style = styled(
        &format!("[module.{name}].style"),
        tbl.style.as_deref(),
        resolver,
        errors,
    );
    Some(SegmentConfig {
        style,
        symbol: None,
        format: nodes,
        disabled: tbl.disabled.unwrap_or(false),
        priority: tbl
            .priority
            .unwrap_or(crate::ui::bar::render::DEFAULT_PRIORITY),
        separator: Vec::new(),
        more_format: Vec::new(),
        styles: Vec::new(),
        // A module has no `[module.<name>.palette]`: its format is
        // validated against the global resolver, so nothing to shadow.
        palette: HashMap::new(),
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

/// Within one scope (a group of bar sides that together route one set of
/// click targets), reject a singleton segment (see
/// [`crate::ui::bar::registry::SegmentDef::singleton`]) placed more than
/// once.
fn check_singleton_scope(loc: &str, nodes: &[&[Node]], verb: &str, errors: &mut Vec<ThemeError>) {
    let count_var = |name: &str| -> usize {
        nodes
            .iter()
            .flat_map(|n| format::vars(n))
            .filter(|v| *v == name)
            .count()
    };
    for name in singleton_names() {
        let count = count_var(name);
        if count > 1 {
            errors.push(error(
                loc,
                format!(
                    "segment `${name}` carries one click target and may appear only once {verb} (found {count}; a bar side you did not set keeps its bundled default, so set that `format`/`right_format` to \"\" to clear it)"
                ),
            ));
        }
    }
}

/// Reject a singleton segment placed more than once among the bars that
/// would each try to route its one click target. Four independent
/// scopes: the attached pair together, the dashboard footer's own two
/// sides, the dashboard header's own two sides, and the dashboard detail
/// pane's pinned-chip row on its own (a singleton may appear once in each
/// without conflicting with the other scopes).
fn check_singletons(
    dashboard: &BarSpec,
    header: &BarSpec,
    top: &BarSpec,
    bottom: &BarSpec,
    detail: &BarSpec,
    errors: &mut Vec<ThemeError>,
) {
    let attached_nodes: [&[Node]; 4] = [
        &top.format,
        &top.right_format,
        &bottom.format,
        &bottom.right_format,
    ];
    check_singleton_scope(
        "[attached_top]/[attached_bottom]",
        &attached_nodes,
        "across the attached bars",
        errors,
    );
    let footer_nodes: [&[Node]; 2] = [&dashboard.format, &dashboard.right_format];
    check_singleton_scope(
        "[dashboard_footer]",
        &footer_nodes,
        "in the dashboard footer",
        errors,
    );
    let header_nodes: [&[Node]; 2] = [&header.format, &header.right_format];
    check_singleton_scope(
        "[dashboard_header]",
        &header_nodes,
        "in the dashboard header",
        errors,
    );
    let detail_nodes: [&[Node]; 2] = [&detail.format, &detail.right_format];
    check_singleton_scope(
        "[dashboard_detail]",
        &detail_nodes,
        "in the dashboard detail pane's pinned-chip row",
        errors,
    );
}

/// Merge `file` over the bundled default and resolve it. Every problem is
/// reported, not just the first.
pub fn resolve(file: ThemeFile, theme: &Theme) -> Result<BarSpecs, Vec<ThemeError>> {
    let base = ThemeFile::parse(DEFAULT_TOML).expect("bundled default_theme.toml parses");
    let file = file.merge_over(base);
    let mut errors = Vec::new();

    let palette = resolve_palette(
        "palette",
        &file.palette,
        &HashMap::new(),
        theme,
        &mut errors,
    );
    let resolver = Resolver::new(&palette, theme);

    let mut segments = HashMap::new();
    for (name, tbl) in &file.segments {
        if let Some(cfg) = resolve_segment(name, tbl, &resolver, &mut errors) {
            segments.insert(name.clone(), cfg);
        }
    }

    let mut modules = Vec::new();
    for (name, tbl) in &file.module {
        if let Some(cfg) = resolve_module(name, tbl, &resolver, &mut errors) {
            segments.insert(name.clone(), cfg);
            modules.push(name.clone());
        }
    }

    // Bars may place any segment or any module.
    let mut allowed_names: Vec<&str> = SEGMENTS.iter().map(|d| d.name).collect();
    allowed_names.extend(modules.iter().map(String::as_str));
    let dashboard_footer = resolve_bar(
        "dashboard_footer",
        &file.dashboard_footer,
        &allowed_names,
        &resolver,
        &mut errors,
    );
    let dashboard_header = resolve_bar(
        "dashboard_header",
        &file.dashboard_header,
        &allowed_names,
        &resolver,
        &mut errors,
    );
    let attached_top = resolve_bar(
        "attached_top",
        &file.attached_top,
        &allowed_names,
        &resolver,
        &mut errors,
    );
    let attached_bottom = resolve_bar(
        "attached_bottom",
        &file.attached_bottom,
        &allowed_names,
        &resolver,
        &mut errors,
    );
    let dashboard_detail = resolve_bar(
        "dashboard_detail",
        &file.dashboard_detail,
        &allowed_names,
        &resolver,
        &mut errors,
    );

    check_singletons(
        &dashboard_footer,
        &dashboard_header,
        &attached_top,
        &attached_bottom,
        &dashboard_detail,
        &mut errors,
    );

    if errors.is_empty() {
        Ok(BarSpecs {
            palette,
            dashboard_footer,
            dashboard_header,
            attached_top,
            attached_bottom,
            dashboard_detail,
            segments,
            modules,
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
        assert_eq!(
            specs.dashboard_detail.format,
            format::parse("($pins  )").unwrap()
        );
        assert_eq!(
            specs.dashboard_header.format,
            format::parse("$brand      $group(   $sort)(  $filter)").unwrap()
        );
        assert_eq!(specs.segments["pr"].priority, 50);
        assert_eq!(specs.segments["sort"].priority, 30);
        assert_eq!(specs.segments["counts"].priority, 50);
        assert_eq!(specs.segments["usage"].priority, 60);
        assert_eq!(specs.segments["brand"].symbol.as_deref(), Some("▌"));
        assert_eq!(
            specs.segments["keys"].separator,
            format::parse("  ").unwrap()
        );
        assert_eq!(specs.segments["procs"].symbol.as_deref(), Some("●"));
        assert_eq!(specs.modules, vec!["funnel".to_string()]);
        assert_eq!(specs.segments["funnel"].priority, 60);
        assert_eq!(
            specs.dashboard_footer.right_format,
            format::parse("$version(  $funnel)").unwrap()
        );
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
    fn partial_dashboard_header_table_merges_over_the_default() {
        let specs = ok("[dashboard_header]\nformat = \"$brand\"\n");
        assert_eq!(
            specs.dashboard_header.format,
            format::parse("$brand").unwrap()
        );
        assert_eq!(
            specs.dashboard_header.right_format,
            format::parse("$counts").unwrap(),
            "unset fields keep the default"
        );
    }

    #[test]
    fn partial_dashboard_detail_table_merges_over_the_default() {
        let specs = ok("[dashboard_detail]\nformat = \"$pins\"\n");
        assert_eq!(
            specs.dashboard_detail.format,
            format::parse("$pins").unwrap()
        );
        assert_eq!(
            specs.dashboard_detail.fill, "─",
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

    /// `[<segment>.palette]` resolves like `[palette]` (literals, theme
    /// tokens, ANSI names) into that segment's own overlay, and stays out
    /// of the global palette.
    #[test]
    fn segment_palette_resolves_into_the_segment_alone() {
        let specs = ok(concat!(
            "[pr.palette]\nok = \"#005f00\"\nmerged = \"dim\"\nerr = \"bright-red\"\n",
            "[palette]\nglobal = \"#123456\"\n",
        ));
        let pr = &specs.segments["pr"];
        assert_eq!(pr.palette["ok"], Color::Rgb(0x00, 0x5f, 0x00));
        assert_eq!(pr.palette["merged"], Theme::wsx().dim);
        assert_eq!(pr.palette["err"], Color::LightRed);
        assert!(!specs.palette.contains_key("ok"));
        assert!(specs.segments["workspace"].palette.is_empty());
        assert_eq!(specs.palette["global"], Color::Rgb(0x12, 0x34, 0x56));
    }

    /// A segment palette value may name a global `[palette]` entry — the
    /// natural way to reuse a theme's own colours — ahead of theme tokens
    /// and ANSI names, so `ok = "green"` is the theme's green, not ANSI's.
    #[test]
    fn segment_palette_values_resolve_against_the_global_palette_first() {
        let specs = ok(concat!(
            "[palette]\ngreen = \"#008700\"\nplum = \"#870087\"\n",
            "[pr.palette]\nok = \"green\"\nmerged = \"plum\"\nerr = \"red\"\n",
        ));
        let pal = &specs.segments["pr"].palette;
        assert_eq!(pal["ok"], Color::Rgb(0x00, 0x87, 0x00));
        assert_eq!(pal["merged"], Color::Rgb(0x87, 0x00, 0x87));
        assert_eq!(pal["err"], Color::Red);
    }

    /// A segment palette entry validates the segment's own format: a name
    /// it defines is legal there, and only there.
    #[test]
    fn segment_palette_names_are_known_only_to_that_segment() {
        let specs = ok(concat!(
            "[pr.palette]\nink = \"#005f00\"\n",
            "[pr]\nformat = \"[$label](fg:ink)\"\n",
        ));
        assert_eq!(
            specs.segments["pr"].palette["ink"],
            Color::Rgb(0x00, 0x5f, 0x00)
        );
        let e = errs(concat!(
            "[pr.palette]\nink = \"#005f00\"\n",
            "[diff]\nformat = \"[$added](fg:ink)\"\n",
        ));
        assert_eq!(e.len(), 1, "{e:?}");
        assert_eq!(e[0].location, "[diff].format");
        assert!(e[0].message.contains("ink"), "{e:?}");
    }

    #[test]
    fn segment_palette_errors_carry_their_location() {
        let e = errs("[pr.palette]\nbad = \"#12\"\nnope = \"rusty\"\nitem_bg = \"red\"\n");
        let locs: Vec<&str> = e.iter().map(|e| e.location.as_str()).collect();
        assert!(locs.contains(&"[pr.palette].bad"), "{locs:?}");
        assert!(locs.contains(&"[pr.palette].nope"), "{locs:?}");
        assert!(locs.contains(&"[pr.palette].item_bg"), "{locs:?}");
        assert_eq!(e.len(), 3, "{e:?}");
    }

    /// A user file's segment palette unions with the base's, the user
    /// winning per name — the same rule as the global `[palette]`.
    #[test]
    fn segment_palette_merges_per_name() {
        let base = ThemeFile::parse("[pr.palette]\nok = \"#111111\"\nerr = \"#222222\"\n").unwrap();
        let mine = ThemeFile::parse("[pr.palette]\nok = \"#333333\"\n").unwrap();
        let merged = mine.merge_over(base);
        let pal = &merged.segments["pr"].palette;
        assert_eq!(pal["ok"], "#333333");
        assert_eq!(pal["err"], "#222222");
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

    /// `separator` is a format like `format` is — styled runs and escapes
    /// work — but it sits BETWEEN items, so it has no item variables and no
    /// item `$style` to borrow.
    #[test]
    fn a_separator_is_a_format_with_styles_but_no_variables() {
        let specs = ok("[pins]\nseparator = \"[ │ ](fg:dim)\"\n");
        assert_eq!(
            specs.segments["pins"].separator,
            format::parse("[ │ ](fg:dim)").unwrap()
        );
        let e = errs("[pins]\nseparator = \"$label\"\n");
        assert_eq!(e.len(), 1, "{e:?}");
        assert_eq!(e[0].location, "[pins].separator");
        assert_eq!(
            e[0].message,
            "unknown `$label` (no variables are allowed here)"
        );
        assert!(!errs("[pins]\nseparator = \"[x](fg:nope)\"\n").is_empty());
        assert!(!errs("[pins]\nseparator = \"[x]($style)\"\n").is_empty());
    }

    /// `more_format` renders a multi-item segment's overflow tail. Only
    /// `attention` has one, and its single variable is `$count`; on any
    /// other segment even `$count` is unknown.
    #[test]
    fn more_format_takes_count_on_attention_and_nothing_elsewhere() {
        let specs = ok("[attention]\nmore_format = \"[ +$count](fg:dim)\"\n");
        assert_eq!(
            specs.segments["attention"].more_format,
            format::parse("[ +$count](fg:dim)").unwrap()
        );
        let e = errs("[attention]\nmore_format = \"$nope\"\n");
        assert_eq!(e.len(), 1, "{e:?}");
        assert_eq!(e[0].location, "[attention].more_format");
        assert!(e[0].message.contains("nope"), "{}", e[0].message);
        assert!(!errs("[attention]\nmore_format = \"[$count]($style)\"\n").is_empty());
        // A tail on a segment that never folds is rejected outright, even
        // when it references nothing: "attention alone takes more_format".
        for src in [
            "[pins]\nmore_format = \"$count\"\n",
            "[pins]\nmore_format = \"x\"\n",
            "[pins]\nmore_format = \"\"\n",
        ] {
            let e = errs(src);
            assert_eq!(e.len(), 1, "{e:?}");
            assert_eq!(e[0].location, "[pins].more_format");
            assert!(
                e[0].message.contains("no overflow tail"),
                "{}",
                e[0].message
            );
        }
    }

    /// `tags` folds nothing, but its manager chip is a tail all the same:
    /// `more_format` renders once after the chips, with `$count` the
    /// number of saved tags, and item variables are unknown there.
    #[test]
    fn tags_takes_a_more_format_with_count() {
        let specs = ok("[tags]\nmore_format = \"[$count tags](fg:dim)\"\n");
        assert_eq!(
            specs.segments["tags"].more_format,
            format::parse("[$count tags](fg:dim)").unwrap()
        );
        assert!(!errs("[tags]\nmore_format = \"$label\"\n").is_empty());
    }

    /// `styles` grades a multi-item segment's items by position, and the
    /// six neighbour colours (`item_*`, `prev_*`, `next_*`) are legal only
    /// in a multi-item segment's `format`, `separator`, and `more_format`
    /// — never in `styles` itself (a grade can't depend on its neighbours)
    /// and never on a single-item segment.
    #[test]
    fn styles_grade_items_and_neighbour_colours_are_item_only() {
        let specs = ok(concat!(
            "[pins]\nstyles = [\"bg:red\", \"bg:blue bold\"]\n",
            "format = \"[$label]($style)[>](fg:item_bg bg:next_bg)\"\n",
            "separator = \"[ ](fg:prev_bg bg:next_bg)\"\n",
            "[attention]\nmore_format = \"[x](fg:prev_bg)\"\n",
        ));
        assert_eq!(
            specs.segments["pins"].styles,
            vec![
                StyleSpec::parse("bg:red").unwrap(),
                StyleSpec::parse("bg:blue bold").unwrap()
            ]
        );
        assert!(specs.segments["pr"].styles.is_empty());

        let e = errs("[pr]\nstyles = [\"bg:red\"]\n");
        assert_eq!(e.len(), 1, "{e:?}");
        assert_eq!(e[0].location, "[pr].styles");
        assert!(e[0].message.contains("no items"), "{}", e[0].message);

        let e = errs("[pr]\nformat = \"[$number](fg:next_bg)\"\n");
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].message.contains("next_bg"), "{}", e[0].message);

        let e = errs("[pins]\nstyles = [\"bg:red\", \"fg:nope\"]\n");
        assert_eq!(e.len(), 1, "{e:?}");
        assert_eq!(e[0].location, "[pins].styles[1]");

        assert!(!errs("[pins]\nstyles = [\"bg:next_bg\"]\n").is_empty());
    }

    /// The six per-item colour names resolve ahead of the palette inside a
    /// multi-item segment's formats, so a palette entry by one of those
    /// names would be silently shadowed there (and, for an absent
    /// neighbour, shadowed by nothing at all). Reserve them instead.
    #[test]
    fn palette_may_not_define_the_per_item_colour_names() {
        for name in crate::ui::bar::registry::ITEM_COLORS {
            let e = errs(&format!("[palette]\n{name} = \"red\"\n"));
            assert_eq!(e.len(), 1, "{name}: {e:?}");
            assert_eq!(e[0].location, format!("[palette].{name}"));
            assert!(
                e[0].message.contains("reserved"),
                "{name}: {}",
                e[0].message
            );
        }
        // A near miss is still an ordinary palette entry.
        assert!(
            ok("[palette]\nnext_bgx = \"red\"\n")
                .palette
                .contains_key("next_bgx")
        );
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

    /// The dashboard header is its own singleton scope: `$usage` placed
    /// once there does not collide with the footer's own placement, but
    /// twice within the header does.
    #[test]
    fn the_dashboard_header_is_its_own_singleton_scope() {
        assert!(
            ok("[dashboard_header]\nright_format = \"$counts $usage\"\n")
                .dashboard_header
                .right_format
                .len()
                > 1
        );
        let e = errs("[dashboard_header]\nformat = \"$usage\"\nright_format = \"$usage\"\n");
        assert!(
            e.iter()
                .any(|e| e.location == "[dashboard_header]" && e.message.contains("usage")),
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
        let e = ThemeFile::parse("[dashboard_detail]\nfromat = \"x\"\n").unwrap_err();
        assert!(e.message.contains("fromat"), "{}", e.message);
        let e = ThemeFile::parse("[dashboard_header]\nfromat = \"x\"\n").unwrap_err();
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

    #[test]
    fn module_table_resolves_into_segments_and_modules() {
        let specs = ok(
            "[module.pipe]\nformat = \"([$pr_open open](fg:ok) )($mergeable ready)\"\npriority = 40\n[dashboard_footer]\nright_format = \"$pipe\"\n",
        );
        assert_eq!(
            specs.modules,
            vec!["funnel".to_string(), "pipe".to_string()]
        );
        assert_eq!(specs.segments["pipe"].priority, 40);
        assert_eq!(
            specs.segments["pipe"].format,
            format::parse("([$pr_open open](fg:ok) )($mergeable ready)").unwrap()
        );
        assert_eq!(
            specs.dashboard_footer.right_format,
            format::parse("$pipe").unwrap()
        );
    }

    #[test]
    fn module_format_may_only_use_fleet_vars() {
        let e = errs("[module.pipe]\nformat = \"$label $pr_open\"\n");
        assert_eq!(e.len(), 1, "{e:?}");
        assert_eq!(e[0].location, "[module.pipe].format");
        assert!(
            e[0].message.contains("unknown `$label`"),
            "{}",
            e[0].message
        );
        assert!(
            e[0].message.contains("pr_open"),
            "hint lists fleet vars: {}",
            e[0].message
        );
    }

    #[test]
    fn module_name_may_not_collide_with_a_segment() {
        let e = errs("[module.keys]\nformat = \"$working\"\n");
        assert_eq!(e.len(), 1, "{e:?}");
        assert_eq!(e[0].location, "[module.keys]");
        assert!(
            e[0].message.contains("built-in segment"),
            "{}",
            e[0].message
        );
    }

    #[test]
    fn module_table_rejects_segment_only_keys() {
        let e = ThemeFile::parse("[module.pipe]\nseparator = \"x\"\n").unwrap_err();
        assert!(e.message.contains("separator"), "{}", e.message);
        let e = ThemeFile::parse("[module.pipe]\nstyles = [\"x\"]\n").unwrap_err();
        assert!(e.message.contains("styles"), "{}", e.message);
    }

    #[test]
    fn module_is_placeable_in_every_bar() {
        let specs = ok(
            "[module.pipe]\nformat = \"$working\"\n[attached_bottom]\nright_format = \"$pipe\"\n[dashboard_header]\nright_format = \"$pipe\"\n[dashboard_detail]\nformat = \"$pipe\"\n[attached_top]\nformat = \"$pipe\"\n",
        );
        assert_eq!(
            specs.attached_bottom.right_format,
            format::parse("$pipe").unwrap()
        );
        assert_eq!(
            specs.dashboard_detail.format,
            format::parse("$pipe").unwrap()
        );
    }

    #[test]
    fn bar_referencing_an_undefined_module_is_an_error_listing_modules() {
        let e = errs("[dashboard_footer]\nright_format = \"$nope\"\n");
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].message.contains("unknown `$nope`"), "{}", e[0].message);
        assert!(
            e[0].message.contains("funnel"),
            "hint lists modules: {}",
            e[0].message
        );
    }

    #[test]
    fn user_module_table_merges_per_field_over_the_default() {
        let specs = ok("[module.funnel]\npriority = 7\n");
        assert_eq!(specs.segments["funnel"].priority, 7);
        assert!(
            !specs.segments["funnel"].format.is_empty(),
            "unset format keeps the bundled default's"
        );
    }

    #[test]
    fn places_module_sees_enabled_modules_in_either_format() {
        let specs = ok(
            "[module.a]\nformat = \"$working\"\n[module.b]\nformat = \"$blocked\"\ndisabled = true\n\
             [dashboard_detail]\nformat = \"$pins\"\nright_format = \"$a\"\n\
             [dashboard_header]\nformat = \"$b\"\n",
        );
        assert!(
            specs.places_module(&specs.dashboard_detail),
            "enabled module in right_format"
        );
        assert!(
            !specs.places_module(&specs.dashboard_header),
            "disabled module is not content"
        );
        // The bundled default places $funnel in the footer and nothing in the detail bar.
        let stock = bundled_default(&Theme::wsx());
        assert!(stock.places_module(&stock.dashboard_footer));
        assert!(!stock.places_module(&stock.dashboard_detail));
    }

    #[test]
    fn module_style_forms_dollar_style() {
        let specs = ok("[module.pipe]\nformat = \"[$working]($style)\"\nstyle = \"fg:ok bold\"\n");
        assert!(specs.segments.contains_key("pipe"));
    }

    #[test]
    fn unknown_segment_hints_at_module_tables() {
        let e = errs("[bogus]\nformat = \"x\"\n");
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].message.contains("unknown segment"), "{}", e[0].message);
        assert!(
            e[0].message
                .contains("user modules go under [module.<name>]"),
            "{}",
            e[0].message
        );
    }

    #[test]
    fn module_without_format_is_an_error() {
        let e = errs("[module.pipe]\npriority = 7\n");
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(
            e[0].message.contains("module has no `format`"),
            "{}",
            e[0].message
        );
        assert!(e[0].location.contains("[module.pipe]"), "{}", e[0].location);
    }
}
