# Workspace-create inheritance: yolo and agent kind

## Problem

Agents hand work to sibling workspaces via `wsx workspace create` (per the
injected doctrine and the wsx skill). Today every such child defaults to
non-yolo and the claude agent, regardless of how the parent runs. A yolo
parent (spawned with `--dangerously-skip-permissions`) therefore creates
children whose agents stall on permission prompts nobody is watching, and a
pi parent creates claude children unless it remembers to pass `--agent pi`.
Relying on the agent to pass `--yolo` is not workable: agents have no way to
know their own workspace's yolo state.

## Decision

`wsx workspace create` inherits `yolo` and agent kind from the workspace the
command is invoked from — the "parent" — resolved with the existing
`resolve_current_workspace()` (WSX_WORKSPACE_ID env var, set for wsx-spawned
agents, with a cwd-prefix fallback covering a human in a worktree shell).

Rules:

- **yolo**: effective = `--yolo` flag OR parent's yolo. There is no
  `--no-yolo`; omitting the flag inherits. (Deliberate simplicity — a
  safer-than-parent child can be created from outside the workspace.)
- **agent**: `--agent <kind>` wins if passed; else the parent's agent kind;
  else the `coding_agent` setting (claude unless configured otherwise) — the
  same default the TUI's create modal already used, which the CLI previously
  ignored by hard-defaulting to claude.
- Inheritance is repo-agnostic: a yolo workspacex parent creating a sessionx
  child still passes both on (the cross-repo handoff case).
- No parent resolvable (create run outside any workspace): yolo behaves
  exactly as today (flag value); the agent falls back to the `coding_agent`
  setting rather than hard-defaulting to claude, aligning the CLI with the
  TUI and the book.
- The command's stdout names what was inherited and from where, so agents
  and humans see it happened.

## Implementation

- A pure helper in `cli.rs`,
  `effective_create_flags(explicit_yolo, explicit_agent, parent,
  default_agent) -> (bool, AgentKind)` (with `default_agent` resolved from
  the `coding_agent` setting), unit-tested without env/cwd manipulation
  (`resolve_current_workspace` reads process-global state, hostile to
  parallel tests).
- The `CliAction::WorkspaceCreate` handler resolves the parent best-effort
  (`.ok()`) and feeds it to the helper.
- Doctrine text (`src/agent/doctrine.rs`) and `skills/wsx/SKILL.md` gain a
  clause stating that yolo and agent kind inherit automatically, so agents
  don't cargo-cult `--yolo` into briefs.

## Not touched

The TUI new-workspace modal path (`create_with_app`) — it has no parent
notion and its own explicit controls.

## Testing

- Unit tests for `effective_create_flags`: explicit beats parent, parent
  beats the `coding_agent` setting, the setting applies when no parent,
  yolo flag ORs with parent yolo.
- Doctrine content test extended for the new clause.
