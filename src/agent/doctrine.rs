//! The standing "process doctrine" wsx injects into developer sessions.
//!
//! These are non-negotiable defaults; an agent may stand them down only if a
//! task plainly does not warrant the planning. An install may replace the text
//! via the `process_doctrine` setting, append its own clauses via
//! `process_doctrine_extra`, or disable injection entirely by setting
//! `process_doctrine` to a disable sentinel (`off` / `none` / `disabled`).

use crate::pty::session::AgentKind;

const DOCTRINE_HEADER: &str = "## wsx workspace operating doctrine\n\n\
    This is a wsx-managed workspace, and the work here is rarely trivial. Unless \
    the task is plainly simple, treat the following as your default, \
    non-negotiable operating mode. You may stand a practice down only if, after \
    evaluating, the task clearly does not warrant it.";

const CLAUSE_PLAN: &str = "- Think and plan before acting. Determine scope first, \
    applying maximum effort and explicit planning until the scope is clear. Do not \
    start editing code before you understand what you are building.";

const CLAUSE_COMMITS: &str = "- Break the work into logical commits on this branch. \
    A workspace that ends with a single commit should be the exception, reserved \
    for the simplest tasks — not the norm.";

const CLAUSE_WSX_SKILL: &str = "- Load and follow the wsx skill. It is authoritative \
    for workspace and cross-repo operations in this environment; consult it before \
    running wsx commands.";

const CLAUSE_HANDOFF_OUT: &str = "- Start a new workspace instead of a new branch. \
    When the work ahead needs a new branch, or shifts to a concern independent \
    enough that this session's history would be noise, do not branch here — \
    create a workspace and hand the task to its own agent: \
    `wsx workspace create <repo> --name <slug>`, then \
    `wsx agent send --workspace <repo>/<slug> primary \"<brief>\"`. Always pass \
    `--name`; an unnamed workspace forces the new agent to rename it before it \
    can start. The new workspace inherits this workspace's yolo mode and agent \
    kind automatically — do not pass `--yolo`, and pass `--agent` only to \
    deliberately pick a different agent. The brief is the receiving agent's ONLY context: state the task \
    and what done looks like, why it exists, the decisions and file:line \
    pointers it needs, the constraints, and the first concrete step — write it \
    so it still makes sense if this session were deleted. Then tell the user \
    which workspace is now working on what and return to your own task; do NOT \
    `cd` into the new worktree and work there yourself.";

const CLAUSE_HANDOFF_IN: &str = "- If your first input is a handoff brief from \
    another workspace's agent (banner: `[message from <repo>/<slug> <label>]`), \
    that brief is your task. Set `wsx recap set --goal` from it before you start.";

const CLAUSE_STATUS: &str = "- Report your status as you go with `wsx status set \
    <working|waiting|blocked|done> --message \"<one line>\"`: `working` when you start \
    substantive work, `blocked` when you need a decision or answer from the user, \
    `waiting` when parked on something external (a build, CI, a long command), and \
    `done` when the task is finished. This keeps the wsx dashboard accurate.";

const CLAUSE_RECAP: &str = "- Maintain the workspace recap with `wsx recap set`: run \
    `wsx recap set --goal \"<one line>\"` once you understand the task's scope, and \
    update `--state \"<one line>\"` and `--next \"<one line>\"` whenever you set status \
    and whenever you end a turn with the task unfinished. Alongside each full field, \
    keep keyword short forms for the dashboard row: `--goal-short` (≤40 chars), \
    `--state-short` and `--next-short` (≤24 chars) — telegraphic style: identifiers \
    and ticket/PR numbers only, no articles (a/an/the), no filler verbs. \
    Example: --goal \"Audit all V2 invoices auto-issued today for the \
    CV-04964 amount-drift bug fixed in PR #2835\" --goal-short \"Audit V2 invoices, \
    CV-04964, bug #2835\". The project-manager digest renders the full lines; the \
    dashboard row renders the short forms.";

/// Values for the `process_doctrine` setting that disable doctrine injection
/// entirely (matched case-insensitively against the trimmed value).
const DISABLE_SENTINELS: [&str; 3] = ["off", "none", "disabled"];

/// The effective doctrine for a spawn, or `None` when injection is disabled.
///
/// Resolution of the `process_doctrine` setting:
/// - unset, or blank/whitespace-only → the built-in default.
/// - a disable sentinel (`off` / `none` / `disabled`, case-insensitive) →
///   `None`, suppressing injection for that install.
/// - any other value → that value verbatim, for every agent.
///
/// Then, unless injection is disabled, a non-blank `process_doctrine_extra`
/// setting is appended verbatim — neither string is trimmed; a separating
/// newline is inserted only when the base does not already end in one. This
/// is the additive hook: it lets an install add clauses (e.g. "use the
/// superpowers skills by default") without freezing a copy of the whole
/// default text.
pub fn resolve_effective_doctrine(
    store: &crate::data::store::Store,
    agent: AgentKind,
) -> Option<String> {
    let base = match store.get_setting("process_doctrine") {
        Ok(Some(v)) => {
            let trimmed = v.trim();
            if trimmed.is_empty() {
                process_doctrine(agent)
            } else if DISABLE_SENTINELS.contains(&trimmed.to_lowercase().as_str()) {
                return None;
            } else {
                v
            }
        }
        _ => process_doctrine(agent),
    };
    let extra = match store.get_setting("process_doctrine_extra") {
        Ok(Some(v)) if !v.trim().is_empty() => v,
        _ => return Some(base),
    };
    let sep = if base.ends_with('\n') { "" } else { "\n" };
    Some(format!("{base}{sep}{extra}"))
}

/// The default doctrine. Every agent kind receives the same text: the
/// doctrine deliberately names no third-party skill bundle (which skills a
/// developer runs is a per-install choice — see `process_doctrine_extra`).
pub fn process_doctrine(_agent: AgentKind) -> String {
    let clauses = [
        CLAUSE_PLAN,
        CLAUSE_COMMITS,
        CLAUSE_WSX_SKILL,
        CLAUSE_HANDOFF_OUT,
        CLAUSE_HANDOFF_IN,
        CLAUSE_STATUS,
        CLAUSE_RECAP,
    ];
    format!("{DOCTRINE_HEADER}\n\n{}", clauses.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::session::AgentKind;

    #[test]
    fn doctrine_covers_core_practices_for_every_agent() {
        for agent in AgentKind::ALL {
            let d = process_doctrine(agent).to_lowercase();
            assert!(
                d.contains("plan"),
                "{agent:?} must get planning clause: {d}"
            );
            assert!(
                d.contains("commit"),
                "{agent:?} must get commits clause: {d}"
            );
            assert!(
                d.contains("wsx skill"),
                "{agent:?} must get wsx skill clause: {d}"
            );
        }
    }

    /// Which skill bundles a developer runs is a per-install choice, so the
    /// built-in doctrine names none. Installs that want one opt in through
    /// `process_doctrine_extra`.
    #[test]
    fn doctrine_names_no_third_party_skill_bundle() {
        for agent in AgentKind::ALL {
            let d = process_doctrine(agent).to_lowercase();
            assert!(
                !d.contains("superpowers"),
                "{agent:?} doctrine must not hardcode superpowers: {d}"
            );
        }
    }

    #[test]
    fn doctrine_mentions_status_reporting() {
        let d = process_doctrine(AgentKind::Claude).to_lowercase();
        assert!(
            d.contains("wsx status"),
            "doctrine must tell the agent to report status: {d}"
        );
    }

    #[test]
    fn doctrine_mentions_recap_maintenance() {
        for agent in [
            AgentKind::Claude,
            AgentKind::Pi,
            AgentKind::Hermes,
            AgentKind::Codex,
        ] {
            let d = process_doctrine(agent).to_lowercase();
            assert!(
                d.contains("wsx recap set"),
                "doctrine must tell {agent:?} to maintain the recap: {d}"
            );
        }
    }

    #[test]
    fn doctrine_mentions_recap_short_forms() {
        for agent in [
            AgentKind::Claude,
            AgentKind::Pi,
            AgentKind::Hermes,
            AgentKind::Codex,
        ] {
            let d = process_doctrine(agent).to_lowercase();
            assert!(
                d.contains("--goal-short"),
                "doctrine must teach {agent:?} the short-form flags: {d}"
            );
            assert!(
                d.contains("no articles"),
                "doctrine must teach {agent:?} the telegraphic style: {d}"
            );
        }
    }

    #[test]
    fn resolve_returns_default_when_unset() {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        assert_eq!(
            resolve_effective_doctrine(&store, AgentKind::Claude),
            Some(process_doctrine(AgentKind::Claude))
        );
    }

    #[test]
    fn resolve_override_replaces_default_verbatim() {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        store
            .set_setting("process_doctrine", "CUSTOM DOCTRINE")
            .unwrap();
        assert_eq!(
            resolve_effective_doctrine(&store, AgentKind::Hermes),
            Some("CUSTOM DOCTRINE".to_string())
        );
    }

    #[test]
    fn resolve_treats_blank_override_as_unset() {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        store.set_setting("process_doctrine", "   ").unwrap();
        assert_eq!(
            resolve_effective_doctrine(&store, AgentKind::Pi),
            Some(process_doctrine(AgentKind::Pi))
        );
    }

    #[test]
    fn resolve_disable_sentinel_suppresses_doctrine() {
        for sentinel in ["off", "none", "disabled", "OFF", "None", " disabled "] {
            let store = crate::data::store::Store::open_in_memory().unwrap();
            store.set_setting("process_doctrine", sentinel).unwrap();
            assert_eq!(
                resolve_effective_doctrine(&store, AgentKind::Claude),
                None,
                "sentinel {sentinel:?} should disable the doctrine"
            );
        }
    }

    #[test]
    fn resolve_appends_extra_to_default() {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        // Indentation and the trailing newline are the operator's, and must
        // survive verbatim (`@file` content routinely carries both).
        let extra = "- Use the superpowers skills.\n  - Prefer brainstorming first.\n";
        store.set_setting("process_doctrine_extra", extra).unwrap();
        let d = resolve_effective_doctrine(&store, AgentKind::Claude).unwrap();
        let base = process_doctrine(AgentKind::Claude);
        assert_eq!(d, format!("{base}\n{extra}"));
    }

    #[test]
    fn resolve_does_not_double_newline_when_base_already_ends_with_one() {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        store
            .set_setting("process_doctrine", "CUSTOM DOCTRINE\n")
            .unwrap();
        store
            .set_setting("process_doctrine_extra", "EXTRA")
            .unwrap();
        assert_eq!(
            resolve_effective_doctrine(&store, AgentKind::Hermes),
            Some("CUSTOM DOCTRINE\nEXTRA".to_string())
        );
    }

    #[test]
    fn resolve_appends_extra_to_custom_override() {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        store
            .set_setting("process_doctrine", "CUSTOM DOCTRINE")
            .unwrap();
        store
            .set_setting("process_doctrine_extra", "EXTRA")
            .unwrap();
        assert_eq!(
            resolve_effective_doctrine(&store, AgentKind::Codex),
            Some("CUSTOM DOCTRINE\nEXTRA".to_string())
        );
    }

    #[test]
    fn resolve_ignores_blank_extra() {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        store
            .set_setting("process_doctrine_extra", "  \n ")
            .unwrap();
        assert_eq!(
            resolve_effective_doctrine(&store, AgentKind::Pi),
            Some(process_doctrine(AgentKind::Pi))
        );
    }

    #[test]
    fn resolve_disabled_doctrine_drops_extra_too() {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        store.set_setting("process_doctrine", "off").unwrap();
        store
            .set_setting("process_doctrine_extra", "EXTRA")
            .unwrap();
        assert_eq!(
            resolve_effective_doctrine(&store, AgentKind::Claude),
            None,
            "a disabled doctrine must not leak the extra clauses"
        );
    }

    #[test]
    fn doctrine_teaches_handoff_to_every_agent() {
        for agent in [
            AgentKind::Claude,
            AgentKind::Pi,
            AgentKind::Hermes,
            AgentKind::Codex,
        ] {
            let d = process_doctrine(agent);
            assert!(
                d.contains("wsx agent send --workspace"),
                "{agent:?} must learn the cross-workspace send: {d}"
            );
            assert!(
                d.contains("wsx workspace create <repo> --name <slug>"),
                "{agent:?} must learn to name the new workspace: {d}"
            );
            assert!(
                d.to_lowercase()
                    .contains("do not `cd` into the new worktree"),
                "{agent:?} must be told not to drive the workspace it created: {d}"
            );
            assert!(
                d.contains("inherits this workspace's yolo mode and agent kind"),
                "{agent:?} must learn that yolo/agent inherit on create: {d}"
            );
        }
    }

    #[test]
    fn doctrine_tells_the_receiver_a_brief_is_its_task() {
        let d = process_doctrine(AgentKind::Claude);
        assert!(d.contains("handoff brief"), "receiving side missing: {d}");
        assert!(
            d.contains("Set `wsx recap set --goal` from it before you start"),
            "receiver must seed its recap goal from the brief: {d}"
        );
    }
}
