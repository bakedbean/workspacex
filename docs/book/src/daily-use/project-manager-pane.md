Press `p` on the dashboard to open a horizontal pane below the workspace list
hosting a dedicated "project manager" session. The PM runs whichever coding
agent your global `coding_agent` setting selects — `claude` (the default),
`pi`, `hermes`, or `codex` (see [Coding agents](../configuration/coding-agents.md)) — so
`wsx config set coding_agent pi` switches the PM to Pi on its next open. PM's
job is to answer three questions about each of your active workspaces:

- What was this workspace created for?
- Where have things been left off?
- What's next to close it out?

`p` opens the pane and focuses it immediately — keystrokes go to PM (like
the attached view). `Tab` or `Esc` swaps focus back to the dashboard;
`Tab` from the dashboard swaps back into the PM pane. `r` (while PM is
focused) refreshes `workspaces.json` and asks PM to re-summarize. `Ctrl-O`
(while PM is focused) expands PM to a full-screen attached view so you
can scroll through the agent's history naturally; `Ctrl-x d` detaches back
to the dashboard with the pane state preserved.

PM only summarizes workspaces where claude has been started at least once
(i.e., a session log exists under `~/.claude/projects/...`). Workspaces
you created but never opened are skipped — nothing for PM to report on.

PM lives at `$XDG_STATE_HOME/wsx/project-manager/` and persists across wsx
restarts by resuming the agent's prior session (Claude Code's `--continue`,
or the equivalent for Pi/Hermes/Codex). On the first `p` of a wsx run with
no prior PM session, wsx auto-sends a status-summary request (and submits
it for you). On subsequent runs (resuming via `--continue`), wsx stays
silent — type your own question or press `r` for a fresh summary.

PM only sees workspaces wsx knows about (registered repos and their `Ready`
workspaces). PM runs with `--dangerously-skip-permissions` so its tool
calls don't prompt you — convenient for an inspection-only sidekick, but
note that PM can technically write/edit files (the system prompt steers
it toward read-only inspection but doesn't enforce it). If you don't want
that, disable PM entirely.

Disable the feature with `wsx config set pm_enabled off`.
Customize PM's behavior with `wsx config set pm_custom_instructions @./pm.md`.
