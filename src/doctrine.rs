//! The standing "process doctrine" wsx injects into developer sessions.
//!
//! These are non-negotiable defaults; an agent may stand them down only if a
//! task plainly does not warrant the planning.

use crate::pty::session::AgentKind;

const DOCTRINE_HEADER: &str = "## wsx workspace operating doctrine\n\n\
    This is a wsx-managed workspace, and the work here is rarely trivial. Unless \
    the task is plainly simple, treat the following as your default, \
    non-negotiable operating mode. You may stand a practice down only if, after \
    evaluating, the task clearly does not warrant it.";

const CLAUSE_PLAN: &str = "- Think and plan before acting. Determine scope first, \
    applying maximum effort and explicit planning until the scope is clear. Do not \
    start editing code before you understand what you are building.";

const CLAUSE_SUPERPOWERS: &str = "- Use the superpowers skills by default when \
    evaluating the initial request. If the task turns out not to need that level \
    of planning, you may discard them and proceed.";

const CLAUSE_COMMITS: &str = "- Break the work into logical commits on this branch. \
    A workspace that ends with a single commit should be the exception, reserved \
    for the simplest tasks — not the norm.";

const CLAUSE_WSX_SKILL: &str = "- Load and follow the wsx skill. It is authoritative \
    for workspace and cross-repo operations in this environment; consult it before \
    running wsx commands.";

pub fn process_doctrine(agent: AgentKind) -> String {
    let include_superpowers = matches!(agent, AgentKind::Claude | AgentKind::Pi);
    let mut clauses = vec![CLAUSE_PLAN];
    if include_superpowers {
        clauses.push(CLAUSE_SUPERPOWERS);
    }
    clauses.push(CLAUSE_COMMITS);
    clauses.push(CLAUSE_WSX_SKILL);
    format!("{DOCTRINE_HEADER}\n\n{}", clauses.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::session::AgentKind;

    #[test]
    fn doctrine_covers_all_practices_for_claude() {
        let d = process_doctrine(AgentKind::Claude).to_lowercase();
        assert!(d.contains("plan"), "must mention planning: {d}");
        assert!(d.contains("superpowers"), "claude must get superpowers clause: {d}");
        assert!(d.contains("commit"), "must mention commits: {d}");
        assert!(d.contains("wsx skill"), "must mention the wsx skill: {d}");
    }

    #[test]
    fn pi_also_gets_superpowers() {
        let d = process_doctrine(AgentKind::Pi).to_lowercase();
        assert!(d.contains("superpowers"), "pi must get superpowers clause: {d}");
    }

    #[test]
    fn hermes_omits_superpowers_but_keeps_the_rest() {
        let d = process_doctrine(AgentKind::Hermes).to_lowercase();
        assert!(!d.contains("superpowers"), "hermes must NOT get superpowers clause: {d}");
        assert!(d.contains("plan"), "hermes must still get planning clause: {d}");
        assert!(d.contains("commit"), "hermes must still get commits clause: {d}");
        assert!(d.contains("wsx skill"), "hermes must still get wsx skill clause: {d}");
    }
}
