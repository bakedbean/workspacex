# wsx status, recap, context

Commands an agent uses to report what it's doing, and that you or another agent use to read it back. See [Status and recap](../agents/status-and-recap.md) for how the dashboard shows them.

## wsx status set

```
wsx status set <working|waiting|blocked|done> [--message <text>] [--goal <text>] [--state <text>] [--next <text>] [--goal-short <text>] [--state-short <text>] [--next-short <text>]
```

Sets the calling agent's status in the current workspace.

| Flag | Effect |
| --- | --- |
| `--message <text>`, `-m <text>` | A short note shown with the status. |
| `--goal`, `--state`, `--next` and their `-short` forms | Update the recap in the same call. See [`wsx recap set`](#wsx-recap-set). |

- From a shell without `$WSX_AGENT_INSTANCE_ID`, the status is recorded against the primary agent.
- `done`, `blocked` and `waiting` hold until the agent starts new work. `working` holds until the turn ends. See [Status and recap](../agents/status-and-recap.md) for the rules.

## wsx status show

```
wsx status [show] [--workspace <repo>/<slug>] [--json]
```

Prints the workspace's combined status, then each agent's. Bare `wsx status` does the same.

```
status: blocked — "need your call on the schema" (model, 1m ago)
agents:
  claude (primary): working — "implementing" (model, 4m ago)
  claude#2: blocked — "need your call on the schema" (model, 1m ago)
```

`--json` prints `{repo, slug, status, agents}`, with the same shapes as [`wsx agent list --json`](agent.md#wsx-agent-list).

## wsx status clear

```
wsx status clear
```

Clears status in the current workspace. Run by an agent, it clears only that agent's status. Run from a shell, it clears every agent's.

## wsx status from-hook

```
wsx status from-hook [--agent <kind>]
```

Reads an agent hook's JSON event on stdin and updates the status. wsx installs this as the agents' status hook, so you don't run it yourself. It always exits 0 and prints nothing. Codex uses `wsx status from-notify` the same way.

## wsx recap set

```
wsx recap set [--goal <text>] [--state <text>] [--next <text>] [--goal-short <text>] [--state-short <text>] [--next-short <text>]
```

Updates the current workspace's recap. Pass at least one flag. Fields you omit keep their value.

| Flag | Field |
| --- | --- |
| `--goal` | What the workspace is for. |
| `--state` | Where the work stands. |
| `--next` | The next step. |
| `--goal-short`, `--state-short`, `--next-short` | Keyword versions for the dashboard row: identifiers, ticket and PR numbers, no filler. |

## wsx recap show

```
wsx recap [show] [--workspace <repo>/<slug>] [--json]
```

Prints the recap. Bare `wsx recap` does the same.

## wsx recap clear

```
wsx recap clear
```

Deletes the current workspace's recap.

## wsx context show

```
wsx context show [--workspace <repo>/<slug>]
```

Prints a markdown digest of the workspace for an editor-hosted agent: branch, agents, status, recap, recent commits, the primary agent's last message, and instructions for the external agent. See [Editor-hosted agents](../integrations/editor-agent-context.md).

## wsx context write

```
wsx context write
```

Writes the current workspace's digest to `$XDG_STATE_HOME/wsx/context/<repo>/<slug>.md` (default `~/.local/state`) and prints the path.

- Outside a wsx worktree it exits non-zero, so a global editor hook stays inert elsewhere.
- The file can quote the agent's last message. Don't commit or share it.
- The path follows the workspace name. After a rename, the old file stays behind.
