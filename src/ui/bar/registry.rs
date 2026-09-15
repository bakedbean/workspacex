//! The single place a segment is declared. Adding a segment to the bar
//! engine means: an entry here (name, the variables its own `format` may
//! reference, its style variables, the variables of its overflow tail if
//! it has one, and its click-target arity), a provider function in
//! `providers.rs`, and wiring it into the composer in `bars.rs` that
//! builds the bar it belongs to. Nothing outside this trio needs to
//! change.

/// What a segment's own `format` may reference.
pub struct SegmentDef {
    pub name: &'static str,
    /// `$var` names the segment's `format` may use.
    pub vars: &'static [&'static str],
    /// Named styles the segment's `format` may reference in a `(...)` run,
    /// beyond the default `$style`.
    pub style_vars: &'static [&'static str],
    /// `$var` names the segment's `more_format` (its overflow tail) may
    /// use. Empty for segments without a tail, where setting `more_format`
    /// at all is a theme error.
    pub more_vars: &'static [&'static str],
    /// A multi-item segment: `format` describes one item, joined by
    /// `separator`, graded by `styles`, with the neighbour colours
    /// [`ITEM_COLORS`] available in its formats.
    pub items: bool,
    /// Carries one click target that isn't indexed by item (unlike
    /// `pins`/`agents`/`keys`, which record a hit per item and so tolerate
    /// repeats): placing one of these in two formats at once means the
    /// theme draws two chips but only the last-routed one is clickable.
    /// `check_singletons` in `config::theme_file` rejects a theme that
    /// does this.
    pub singleton: bool,
}

const STYLE: &[&str] = &["style"];
const NO_TAIL: &[&str] = &[];

/// Colour names a multi-item segment's `format`, `separator`, and
/// `more_format` may use, resolved per item from the final `$style` of the
/// item and its rendered neighbours. An absent neighbour (the first item's
/// `prev`, the last's `next`) or an unset colour carries no colour: the
/// token sets nothing, so `$style` or an enclosing run keeps what it set
/// and otherwise the bar's style shows through. Reserved: the loader
/// rejects a `[palette]` entry by any of these names.
pub const ITEM_COLORS: &[&str] = &[
    "item_fg", "item_bg", "prev_fg", "prev_bg", "next_fg", "next_bg",
];

pub const SEGMENTS: &[SegmentDef] = &[
    SegmentDef {
        name: "brand",
        vars: &["symbol", "name", "mark", "view"],
        style_vars: STYLE,
        more_vars: NO_TAIL,
        items: false,
        singleton: false,
    },
    SegmentDef {
        name: "group",
        vars: &["label", "tabs"],
        style_vars: STYLE,
        more_vars: NO_TAIL,
        items: false,
        singleton: false,
    },
    SegmentDef {
        name: "sort",
        vars: &["label", "tabs"],
        style_vars: STYLE,
        more_vars: NO_TAIL,
        items: false,
        singleton: false,
    },
    SegmentDef {
        name: "filter",
        vars: &["needle"],
        style_vars: STYLE,
        more_vars: NO_TAIL,
        items: false,
        singleton: false,
    },
    SegmentDef {
        name: "counts",
        vars: &["repos", "workspaces"],
        style_vars: STYLE,
        more_vars: NO_TAIL,
        items: false,
        singleton: false,
    },
    SegmentDef {
        name: "keys",
        vars: &["key", "label"],
        style_vars: STYLE,
        more_vars: NO_TAIL,
        items: true,
        singleton: false,
    },
    SegmentDef {
        name: "version",
        vars: &["version"],
        style_vars: STYLE,
        more_vars: NO_TAIL,
        items: false,
        singleton: false,
    },
    SegmentDef {
        name: "usage",
        vars: &["label", "spark"],
        style_vars: STYLE,
        more_vars: NO_TAIL,
        items: false,
        singleton: true,
    },
    SegmentDef {
        name: "agent_bar",
        vars: &["symbol"],
        style_vars: STYLE,
        more_vars: NO_TAIL,
        items: false,
        singleton: false,
    },
    SegmentDef {
        name: "workspace",
        vars: &["repo", "name"],
        style_vars: STYLE,
        more_vars: NO_TAIL,
        items: false,
        singleton: false,
    },
    SegmentDef {
        name: "attention",
        vars: &["glyph", "repo", "name", "age"],
        style_vars: STYLE,
        more_vars: &["count"],
        items: true,
        singleton: true,
    },
    SegmentDef {
        name: "pins",
        vars: &["index", "label"],
        style_vars: STYLE,
        more_vars: NO_TAIL,
        items: true,
        singleton: false,
    },
    SegmentDef {
        name: "agents",
        vars: &["symbol", "label", "key"],
        style_vars: STYLE,
        more_vars: NO_TAIL,
        items: true,
        singleton: false,
    },
    SegmentDef {
        name: "model_tokens",
        vars: &["model", "tokens"],
        style_vars: STYLE,
        more_vars: NO_TAIL,
        items: false,
        singleton: false,
    },
    SegmentDef {
        name: "procs",
        vars: &["symbol", "count"],
        style_vars: STYLE,
        more_vars: NO_TAIL,
        items: false,
        singleton: true,
    },
    SegmentDef {
        name: "diff",
        vars: &["added", "removed"],
        style_vars: STYLE,
        more_vars: NO_TAIL,
        items: false,
        singleton: false,
    },
    SegmentDef {
        name: "pr",
        vars: &["symbol", "number", "label", "mark"],
        style_vars: &["style", "mark_style"],
        more_vars: NO_TAIL,
        items: false,
        singleton: true,
    },
];

pub fn segment_def(name: &str) -> Option<&'static SegmentDef> {
    SEGMENTS.iter().find(|d| d.name == name)
}

/// Names of every singleton segment — see [`SegmentDef::singleton`].
pub fn singleton_names() -> impl Iterator<Item = &'static str> {
    SEGMENTS.iter().filter(|d| d.singleton).map(|d| d.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn singleton_names_matches_the_flagged_entries() {
        let names: Vec<&str> = singleton_names().collect();
        assert_eq!(names, vec!["usage", "attention", "procs", "pr"]);
    }

    #[test]
    fn segment_def_finds_by_name_and_misses_unknown_names() {
        assert_eq!(segment_def("pr").map(|d| d.name), Some("pr"));
        assert!(segment_def("nope").is_none());
    }
}
