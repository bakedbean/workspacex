//! Builds the lightweight context note injected into an *added* agent so it
//! can orient itself in a workspace already in progress. The note is fed into
//! the existing `--append-system-prompt` seam (see `SpawnMode::Fresh`).

use crate::pty::session::AgentKind;

pub struct HandoffContext<'a> {
    pub primary_label: &'a str,
    pub branch: &'a str,
    pub base_ref: &'a str,
    pub workspace_name: &'a str,
}

/// The injected note. Uniform across all agent types: it points the new agent
/// at the shared worktree + git rather than exporting any transcript. The
/// `added` kind is accepted for future per-agent tailoring but is not yet used.
pub fn context_note(_added: AgentKind, ctx: &HandoffContext) -> String {
    format!(
        "You are joining an existing wsx workspace \"{name}\" as an additional agent, \
         alongside `{primary}` (the primary agent). You share the same git worktree \
         and branch (`{branch}`) with the other agents here.\n\n\
         To see the work already in progress, inspect the working tree and run \
         `git diff {base}...HEAD`. The primary agent has been working on this branch; \
         review the current state before acting.\n\n\
         You can communicate with the other agents in this workspace. Run \
         `wsx agent list` to see them, and `wsx agent send <label> \"<message>\"` to \
         send one a prompt. Your own identity is in the `$WSX_AGENT_INSTANCE_ID` \
         environment variable.",
        name = ctx.workspace_name,
        primary = ctx.primary_label,
        branch = ctx.branch,
        base = ctx.base_ref,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_mentions_primary_branch_and_messaging() {
        let note = context_note(
            AgentKind::Codex,
            &HandoffContext {
                primary_label: "claude",
                branch: "wsx/feat",
                base_ref: "main",
                workspace_name: "feat",
            },
        );
        assert!(note.contains("claude"));
        assert!(note.contains("wsx/feat"));
        assert!(note.contains("git diff main...HEAD"));
        assert!(note.contains("wsx agent send"));
    }
}
