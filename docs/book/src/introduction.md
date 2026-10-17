# wsx (WorkspaceX)

Terminal UI for managing Claude Code, Pi, Hermes, Codex, or oh-my-pi sessions in git worktrees.

wsx gives each piece of work its own isolated git worktree and coding-agent
session, and a dashboard to launch, monitor, and orchestrate many of them in
parallel — including multiple agents collaborating in a single workspace.

- New here? Start with the [Quick start](getting-started/quick-start.md).
- Browsing what it does? See [Key features](#key-features).
- Looking for a command or setting? Jump to the [CLI reference](cli-reference/index.md)
  or [Reference](reference/index.md).

> Demo videos of parallel and multi-agent sessions are on the
> [project README](https://github.com/bakedbean/workspacex#readme).

- **Parallel agent sessions in git worktrees**: every workspace is its own branch + worktree; switch with one key.
- **Multiple coding agents**: run Claude, Pi, Hermes, Codex, or oh-my-pi (`omp`) per workspace. Set a global default with `coding_agent` or override per workspace with `--agent`. See [Coding agents](agents/coding-agents.md).
- **Multi-agent workspaces**: attach several agents to one worktree, switch focus with a keypress, and have them message each other. See [Multi-agent workspaces](agents/multi-agent-workspaces.md).
- **Cross-session attention alerts**: terminal bell + `!` or `?` marker when a session is awaiting permission, has gone idle or has a question.
- **Activity sub-line per workspace**: see the latest tool call or message from each session at a glance.
- **Configurable Workspace Detail Bar**: Display up to four independent containers with built-in or custom modules. See [Workspace detail bar](dashboard/detail-bar.md).
- **Project manager digest**: an instant, native pane summarizing what every workspace is for, where it's at, and what's next — no agent session required.
- **Remote control**: attach from claude.ai/code or the mobile app; or run wsx in tmux+ssh for full-fidelity desktop access; store and access remote connection commands via the `remote` CLI.
- **Pinned commands**: define your `/pull-request`, `/feedback`, `/ultrareview` shortcuts once; fire them with `Ctrl-x <digit>` or a click while attached or from the workspace details bar.
- **Bundled agent skills**: `wsx` (drive the CLI), `agent-review` (spawn a peer reviewer), and `handoff` (continue a finished workspace's epic in a fresh one, briefed with a summary of the old session). Installed with `wsx setup install-skill`; pin them as chips. See [Agent skill](agents/skills.md).
- **Prompt tags**: wrap a body in an XML tag and insert it unsubmitted with one keystroke. See [Prompt tags](attached-view/prompt-tags.md).
- **Related repos**: declare related wsx repos per primary repo; workspaces spawn with `--add-dir` for each and a read-only system prompt so claude can read but won't edit them.  Agent is provided with the wsx skill to use the CLI to orchestrate between repos.
- **Keyboard first navigation**: comprehensive keybindings for every action, from workspace creation to process killing to digest refreshes.
- **Frictionless workflow**: auto-rename branches from your first prompt, per-repo setup/archive scripts, editor/terminal/diff hooks.
