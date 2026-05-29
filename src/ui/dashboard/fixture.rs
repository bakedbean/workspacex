//! Synthetic data mirroring `~/Desktop/design_handoff_wsx_dashboard/data.js`.
//! Used by render tests so the V5 fixture stays close to the design spec.

#![cfg(test)]

use crate::ui::dashboard::status::Status;

#[derive(Debug, Clone)]
pub struct FixtureWorkspace {
    pub name: String,
    pub branch: String,
    pub procs: u32,
    pub status: Status,
    pub last_message: Option<String>,
    pub diff_added: u32,
    pub diff_removed: u32,
    pub ago_secs: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct FixtureRepo {
    pub name: String,
    pub path: String,
    pub workspaces: Vec<FixtureWorkspace>,
}

pub fn repos() -> Vec<FixtureRepo> {
    use Status::*;
    // This test fixture builder takes one arg per workspace field on purpose; a
    // params struct would not improve clarity here.
    #[allow(clippy::too_many_arguments)]
    fn ws(
        name: &str,
        branch: &str,
        procs: u32,
        status: Status,
        last: Option<&str>,
        added: u32,
        removed: u32,
        ago: Option<u64>,
    ) -> FixtureWorkspace {
        FixtureWorkspace {
            name: name.into(),
            branch: branch.into(),
            procs,
            status,
            last_message: last.map(str::to_string),
            diff_added: added,
            diff_removed: removed,
            ago_secs: ago,
        }
    }
    vec![
        FixtureRepo {
            name: "ssk".into(),
            path: "/home/eben/ssk/ssk-web".into(),
            workspaces: vec![
                ws(
                    "wobbly-peony",
                    "eben/wobbly-peony",
                    0,
                    Idle,
                    None,
                    0,
                    0,
                    None,
                ),
                ws(
                    "woven-parsley",
                    "eben/woven-parsley",
                    0,
                    Idle,
                    None,
                    0,
                    0,
                    None,
                ),
                ws("eager-ivy", "eben/eager-ivy", 0, Idle, None, 0, 0, None),
                ws(
                    "quiet-fennel",
                    "eben/quiet-fennel",
                    2,
                    Thinking,
                    Some("Reading src/cli/dashboard.rs to understand the current layout system…"),
                    184,
                    62,
                    Some(4),
                ),
                ws(
                    "brave-cedar",
                    "eben/brave-cedar",
                    1,
                    Complete,
                    Some("Done. Tests pass (47 ok). Ready for review on PR #214."),
                    612,
                    211,
                    Some(8 * 60),
                ),
            ],
        },
        FixtureRepo {
            name: "wsx".into(),
            path: "/home/eben/workspace/wsx".into(),
            workspaces: vec![
                ws(
                    "tech-stack-question",
                    "bakedbean/tech-stack-question",
                    1,
                    Complete,
                    Some("* Insight ─── `wsx` is a Rust binary using ratatui + crossterm…"),
                    0,
                    0,
                    Some(34),
                ),
                ws(
                    "repo-overview",
                    "bakedbean/repo-overview",
                    2,
                    Question,
                    Some("I have enough to give you a grounded tour. ## wsx — a TUI for…"),
                    12,
                    3,
                    Some(29),
                ),
                ws(
                    "list-virtualization",
                    "bakedbean/list-virt",
                    2,
                    Waiting,
                    Some("Running cargo test --package wsx-tui list_virtualization::scroll…"),
                    318,
                    44,
                    Some(2 * 60),
                ),
                ws(
                    "theme-tokens",
                    "bakedbean/theme-tokens",
                    1,
                    Stalled,
                    Some("Hit ambiguous dependency: ratatui 0.26 vs 0.27 across two crates."),
                    88,
                    12,
                    Some(17 * 60),
                ),
            ],
        },
        FixtureRepo {
            name: "backend".into(),
            path: "/home/eben/meals/backend".into(),
            workspaces: vec![ws(
                "recipe-importer",
                "eben/recipe-importer",
                2,
                Thinking,
                Some("Scaffolding ImportJob model and worker. About to wire it up…"),
                241,
                0,
                Some(11),
            )],
        },
        FixtureRepo {
            name: "frontend".into(),
            path: "/home/eben/meals/frontend".into(),
            workspaces: vec![],
        },
        FixtureRepo {
            name: "api".into(),
            path: "/home/eben/ridesnridesnrides/api".into(),
            workspaces: vec![
                ws(
                    "rate-limit",
                    "eben/rate-limit",
                    0,
                    Complete,
                    Some("All limits in place; benchmark shows P99 at 38ms under 5k rps."),
                    419,
                    83,
                    Some(3600),
                ),
                ws(
                    "webhook-retry",
                    "eben/webhook-retry",
                    0,
                    Idle,
                    None,
                    0,
                    0,
                    None,
                ),
            ],
        },
        FixtureRepo {
            name: "ui".into(),
            path: "/home/eben/ridesnridesnrides/ui".into(),
            workspaces: vec![ws(
                "driver-map-v2",
                "eben/driver-map-v2",
                1,
                Question,
                Some("Should the heatmap render driver positions as discrete pins or…"),
                507,
                192,
                Some(3 * 60),
            )],
        },
        FixtureRepo {
            name: "scp-admin".into(),
            path: "/home/eben/cci/scp-admin".into(),
            workspaces: vec![ws(
                "auth-refactor",
                "eben/auth-refactor",
                1,
                Waiting,
                Some("cargo build --release running… (61%)"),
                88,
                104,
                Some(46),
            )],
        },
        FixtureRepo {
            name: "scp-api".into(),
            path: "/home/eben/cci/scp-api".into(),
            workspaces: vec![],
        },
    ]
}
