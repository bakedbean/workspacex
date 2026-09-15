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

/// A fleet-wide variable a `[module.<name>]` format may reference. Values
/// are derived once per frame by `crate::ui::bar::fleet::FleetStats`; the
/// loader validates module formats against this list exactly as it
/// validates a segment's `format` against its `SegmentDef::vars`.
pub struct FleetVar {
    pub name: &'static str,
    /// One line for the book's variable table and the loader's error hint.
    pub doc: &'static str,
}

/// Every fleet variable. Counts render empty at zero so a `( … )` group
/// around one drops; `workspaces` and `repos` always render a number.
pub const FLEET_VARS: &[FleetVar] = &[
    FleetVar {
        name: "working",
        doc: "workspaces whose last reported status is `working`",
    },
    FleetVar {
        name: "waiting",
        doc: "workspaces whose last reported status is `waiting`",
    },
    FleetVar {
        name: "blocked",
        doc: "workspaces whose last reported status is `blocked`",
    },
    FleetVar {
        name: "done",
        doc: "workspaces whose last reported status is `done`",
    },
    FleetVar {
        name: "busy",
        doc: "workspaces parked on background work (hook-inferred `busy`)",
    },
    FleetVar {
        name: "unreported",
        doc: "workspaces with no reported status",
    },
    FleetVar {
        name: "alerts",
        doc: "workspaces with an unacknowledged attention alert",
    },
    FleetVar {
        name: "awaiting",
        doc: "workspaces whose agent is awaiting an answer",
    },
    FleetVar {
        name: "stalled",
        doc: "workspaces whose agent has stalled",
    },
    FleetVar {
        name: "active",
        doc: "workspaces whose agent is actively working",
    },
    FleetVar {
        name: "idle",
        doc: "workspaces whose agent is idle",
    },
    FleetVar {
        name: "live_agents",
        doc: "workspaces with a live (thinking or waiting) primary session",
    },
    FleetVar {
        name: "pr_none",
        doc: "workspaces polled with no PR",
    },
    FleetVar {
        name: "pr_draft",
        doc: "workspaces with a draft PR",
    },
    FleetVar {
        name: "pr_open",
        doc: "workspaces with an open PR",
    },
    FleetVar {
        name: "pr_conflicted",
        doc: "workspaces with a conflicted PR",
    },
    FleetVar {
        name: "pr_merged",
        doc: "workspaces whose PR merged (not yet archived)",
    },
    FleetVar {
        name: "pr_closed",
        doc: "workspaces whose PR was closed unmerged",
    },
    FleetVar {
        name: "review_required",
        doc: "PRs still awaiting a review",
    },
    FleetVar {
        name: "changes_requested",
        doc: "PRs with changes requested",
    },
    FleetVar {
        name: "approved",
        doc: "PRs approved",
    },
    FleetVar {
        name: "unresolved",
        doc: "unresolved review threads across the fleet",
    },
    FleetVar {
        name: "mergeable",
        doc: "PRs that are open and approved",
    },
    FleetVar {
        name: "dirty",
        doc: "workspaces with modified or untracked files",
    },
    FleetVar {
        name: "msgs_queued",
        doc: "agent-to-agent messages not yet delivered",
    },
    FleetVar {
        name: "workspaces",
        doc: "total workspaces (always rendered)",
    },
    FleetVar {
        name: "repos",
        doc: "total repos (always rendered)",
    },
];

pub fn fleet_var(name: &str) -> Option<&'static FleetVar> {
    FLEET_VARS.iter().find(|v| v.name == name)
}

/// Every fleet variable name, in table order — the `allowed` list a module
/// format validates against.
pub fn fleet_var_names() -> Vec<&'static str> {
    FLEET_VARS.iter().map(|v| v.name).collect()
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

    #[test]
    fn fleet_vars_are_unique_snake_case_and_disjoint_from_segments() {
        let mut seen = std::collections::HashSet::new();
        for v in FLEET_VARS {
            assert!(seen.insert(v.name), "duplicate fleet var {}", v.name);
            assert!(
                v.name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit()),
                "fleet var {} is not snake_case",
                v.name
            );
            assert!(
                segment_def(v.name).is_none(),
                "fleet var {} collides with a segment",
                v.name
            );
            assert!(
                !ITEM_COLORS.contains(&v.name),
                "fleet var {} collides with an item colour",
                v.name
            );
            assert!(!v.doc.is_empty(), "fleet var {} has no doc", v.name);
        }
    }

    #[test]
    fn fleet_var_finds_by_name_and_misses_unknown_names() {
        assert_eq!(fleet_var("mergeable").map(|v| v.name), Some("mergeable"));
        assert!(
            fleet_var("attention").is_none(),
            "attention is a segment, not a fleet var"
        );
        assert!(fleet_var("nope").is_none());
        assert!(fleet_var_names().contains(&"workspaces"));
    }

    /// Every `FLEET_VARS` name must be documented in both places a user
    /// would look: the bundled default's modules comment block (as `$name`)
    /// and the book's fleet-variable table (as `` `name` ``). Catches a
    /// fleet var added to the registry but never wired into the docs.
    #[test]
    fn every_fleet_var_is_documented_in_the_default_toml_and_the_book() {
        const DEFAULT_TOML: &str = include_str!("default_theme.toml");
        let book = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/book/src/configuration/themes.md"
        ))
        .expect("book page exists");
        for v in FLEET_VARS {
            let in_toml = DEFAULT_TOML.contains(&format!("${}", v.name));
            assert!(
                in_toml,
                "{} missing from default_theme.toml's modules comment block",
                v.name
            );
            let in_book = book.contains(&format!("`{}`", v.name));
            assert!(
                in_book,
                "{} missing from docs/book/src/configuration/themes.md",
                v.name
            );
        }
    }
}
