# Status and recap

Agents tell wsx what they're doing so you can see at a glance which workspaces need you. The **status** is a state word and a one-line message. The **recap** is the task's goal, current state, and next step.

## Status states

| State     | Meaning                                                    |
| --------- | ---------------------------------------------------------- |
| `working` | The agent is doing substantive work                        |
| `waiting` | The agent is parked on something external, such as CI      |
| `blocked` | The agent needs a decision or answer from you              |
| `done`    | The task is finished                                       |
| `busy`    | The turn ended, but background work is still running (hooks only) |

## Set status

An agent sets its own status:

```bash
wsx status set blocked --message "need your call on the schema"
```

`wsx status set` also accepts the recap flags, so one call can update both:

```bash
wsx status set working --message "fixing tests" --state "2 tests failing" --next "fix regex"
```

| Command            | Effect                                                              |
| ------------------ | ------------------------------------------------------------------- |
| `wsx status show`  | Print the workspace's status and each agent's (bare `wsx status` too) |
| `wsx status clear` | From an agent, clear its own status. From a shell, clear every agent's |

See [`wsx status set`](../cli-reference/status-recap-context.md#wsx-status-set) and [`wsx status show`](../cli-reference/status-recap-context.md#wsx-status-show) for all flags.

## Automatic status from hooks

Claude and Codex report status without the agent's help. wsx wires their lifecycle events to `wsx status from-hook` (Claude) or Codex's `notify`.

| Event                                              | Status    |
| -------------------------------------------------- | --------- |
| You submit a prompt (Claude)                       | `working` |
| Permission prompt, `AskUserQuestion`, or plan approval (Claude) | `blocked` |
| Claude's idle notification                         | `waiting` |
| Turn ends with a question (ends in `?`)            | `blocked` |
| Turn ends while subagents or shell tasks run (Claude) | `busy` |
| Turn ends otherwise                                | `done`    |

A status the agent sets itself holds through the end of that turn:

- The turn-end hook keeps an agent's own `done`, `blocked`, or `waiting` and its message. The next turn that ends without a new `wsx status set` replaces it.
- The idle notification never overrides an agent's own status.
- New work (`working` or `busy`) replaces it and drops the old message.
- If the agent said `working` and the turn ends, the hook's result replaces it.

A background monitor, such as the watcher Claude keeps after publishing an artifact, doesn't count as background work. A turn that ends with only a monitor running reads as `done`.

pi, Hermes, and omp have no status hooks. They rely on `wsx status set`, and the dashboard reads their session logs.

## Per-agent status

Each agent's status is recorded separately, keyed by its `WSX_AGENT_INSTANCE_ID`. A status set from a plain shell or an editor-hosted agent goes to the primary. `wsx agent list` and `wsx status show` print each agent's status:

```
status: blocked — "need your call on the schema" (model, 1m ago)
agents:
  claude (primary): working — "implementing" (model, 4m ago)
  claude#2: blocked — "need your call on the schema" (model, 1m ago)
```

The workspace status comes from its agents. The state that most needs you wins, in this order: `blocked`, `working`, `busy`, `waiting`, `done`. The most recent breaks ties. So one agent finishing can't mark the workspace done while another still works.

- The dashboard and project manager digest combine the workspace status with the primary agent's session. A peer's older status gives way to newer activity in the primary's transcript.
- On the dashboard, a reported status holds until the agent's session log shows newer activity. Then wsx reads the log instead. `busy` holds until the next hook.
- A status never expires. An agent whose session died keeps its last status until it reports again, is cleared, or is removed.
- Removing an agent drops its status.

## Recap

The recap is three one-line fields, each with a short form for narrow views:

| Field   | Short form      | Holds                    |
| ------- | --------------- | ------------------------ |
| `goal`  | `--goal-short`  | What the task is for (short form ≤40 chars) |
| `state` | `--state-short` | Where the work stands (short form ≤24 chars) |
| `next`  | `--next-short`  | The next step (short form ≤24 chars) |

```bash
wsx recap set --goal "Audit V2 invoices auto-issued today for the CV-04964 drift bug" \
              --goal-short "Audit V2 invoices, CV-04964"
wsx recap set --state "tests failing" --next "debug the regex"
```

`wsx recap set` updates only the fields you pass. Write short forms in telegraphic style: identifiers and ticket numbers, no articles. `wsx recap show` prints the recap, and `wsx recap clear` deletes it. See [`wsx recap set`](../cli-reference/status-recap-context.md#wsx-recap-set).

## Where they show up

| Place                                                          | Shows                                                       |
| -------------------------------------------------------------- | ----------------------------------------------------------- |
| [Dashboard row](../dashboard/status-indicators.md)             | Status word, then the recap's short forms                   |
| [Detail bar](../dashboard/detail-bar.md) SESSION SUMMARY       | Full recap, then status                                     |
| [Project manager digest](../dashboard/project-manager-digest.md) | Full recap and status for every workspace                 |
| [Waybar, Plasma, and menu bar](../integrations/desktop-widgets.md) | Workspace status. The menu bar also shows recaps        |

When a recap field has no short form, the dashboard row shows the full field with articles removed.

## What agents are told

wsx's process doctrine, which it adds to every agent's instructions, tells agents to:

- set status as they go: `working` when they start, `blocked` when they need you, `waiting` when parked, `done` when finished;
- set the recap goal once they understand the task, and update state and next whenever they set status or end a turn unfinished;
- keep the short forms filled in.

Change or turn off the doctrine with the `process_doctrine` and `process_doctrine_extra` settings. See [Settings](../configuration/settings.md).
