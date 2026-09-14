//! The single place a segment is declared. Adding a segment to the bar
//! engine means: an entry here (name, the variables its own `format` may
//! reference, its style variables, and its click-target arity), a
//! provider function in `providers.rs`, and wiring it into the composer
//! in `bars.rs` that builds the bar it belongs to. Nothing outside this
//! trio needs to change.

/// What a segment's own `format` may reference.
pub struct SegmentDef {
    pub name: &'static str,
    /// `$var` names the segment's `format` may use.
    pub vars: &'static [&'static str],
    /// Named styles the segment's `format` may reference in a `(...)` run,
    /// beyond the default `$style`.
    pub style_vars: &'static [&'static str],
    /// Carries one click target that isn't indexed by item (unlike
    /// `pins`/`agents`/`keys`, which record a hit per item and so tolerate
    /// repeats): placing one of these in two formats at once means the
    /// theme draws two chips but only the last-routed one is clickable.
    /// `check_singletons` in `config::theme_file` rejects a theme that
    /// does this.
    pub singleton: bool,
}

const STYLE: &[&str] = &["style"];

pub const SEGMENTS: &[SegmentDef] = &[
    SegmentDef {
        name: "keys",
        vars: &["key", "label"],
        style_vars: STYLE,
        singleton: false,
    },
    SegmentDef {
        name: "version",
        vars: &["version"],
        style_vars: STYLE,
        singleton: false,
    },
    SegmentDef {
        name: "usage",
        vars: &["label", "spark"],
        style_vars: STYLE,
        singleton: true,
    },
    SegmentDef {
        name: "agent_bar",
        vars: &["symbol"],
        style_vars: STYLE,
        singleton: false,
    },
    SegmentDef {
        name: "workspace",
        vars: &["repo", "name"],
        style_vars: STYLE,
        singleton: false,
    },
    SegmentDef {
        name: "attention",
        vars: &["items"],
        style_vars: STYLE,
        singleton: true,
    },
    SegmentDef {
        name: "pins",
        vars: &["index", "label"],
        style_vars: STYLE,
        singleton: false,
    },
    SegmentDef {
        name: "agents",
        vars: &["symbol", "label", "key"],
        style_vars: STYLE,
        singleton: false,
    },
    SegmentDef {
        name: "model_tokens",
        vars: &["model", "tokens"],
        style_vars: STYLE,
        singleton: false,
    },
    SegmentDef {
        name: "procs",
        vars: &["symbol", "count"],
        style_vars: STYLE,
        singleton: true,
    },
    SegmentDef {
        name: "diff",
        vars: &["added", "removed"],
        style_vars: STYLE,
        singleton: false,
    },
    SegmentDef {
        name: "pr",
        vars: &["symbol", "number", "label", "mark"],
        style_vars: &["style", "mark_style"],
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
