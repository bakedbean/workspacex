# Multi-agent workspaces

A workspace can run several agents, of the same kind or different kinds. They share one worktree and branch, each has its own session, and they can message each other. Use this to run a reviewer next to the agent doing the work, or to try `claude` and `codex` on the same problem.

Every workspace starts with one **primary** agent, chosen by `--agent` or the `coding_agent` setting. See [Coding agents](coding-agents.md). The primary lives as long as the workspace.

Agents write to the same files. Hand work off with messages rather than editing the same paths at once.

## Labels

Each agent's label is its kind, plus `#N` for the second and later agents of that kind: `claude`, `claude#2`, `codex`. Labels appear in the footer, in `wsx agent list`, and in message banners. The label `primary` always means the workspace's primary agent.

wsx sets two variables in each agent's session, which the `wsx agent` commands read:

| Variable                | Value                       |
| ----------------------- | --------------------------- |
| `WSX_WORKSPACE_ID`      | The agent's workspace       |
| `WSX_AGENT_INSTANCE_ID` | This agent instance         |

Without `WSX_WORKSPACE_ID`, wsx finds the workspace from the current directory, so the commands also work from a shell in the worktree.

## Add and remove agents

Press `Ctrl-x a` on the dashboard or in the attached view to open the agents panel. It lists the workspace's agents and a picker of the five kinds.

| Key                  | Action                                           |
| -------------------- | ------------------------------------------------ |
| `↑` / `↓`, `k` / `j` | Move through the picker                          |
| `Enter`              | Add the selected kind                            |
| `a`                  | Add one agent of every kind                      |
| `x`                  | Remove the most recently added non-primary agent |
| `Esc`                | Close the panel                                  |

A new agent starts at once with the workspace's context. When you remove an agent you're viewing, the pane switches to the primary.

From the CLI:

```bash
wsx agent add codex        # prints: added codex
wsx agent remove claude#2  # can't remove the primary
```

Removing an agent discards messages still queued for it. See [`wsx agent add`](../cli-reference/agent.md#wsx-agent-add) and [`wsx agent remove`](../cli-reference/agent.md#wsx-agent-remove).

## Switch focus between agents

With two or more agents, the attached view's chip row shows an **agent pill** for each, with a switch key:

```
 ^x  menu   1 pr   2 fb  ────────   ● claude q   ○ codex w   ○ pi r    opus 4.8 45k/200k
```

- Press `Ctrl-x` and the pill's key, or click the pill, to show that agent in the focused pane.
- Keys come from `q w r y i o p s h j`, primary first. Agents past the tenth have no key but stay clickable.
- `●` marks the agent in the focused pane. A spinner replaces the dot while an agent works.
- The model and token count follow the agent in the focused pane.
- On a narrow terminal the pills hide as a group. Their keys still work.

## Messages

Agents send each other messages with `wsx agent send`. The target is a label, `primary`, or a numeric instance id:

```bash
wsx agent send codex "review the parser changes"
wsx agent send primary --file brief.md
wsx agent send --workspace backend/add-widgets primary "start on the API"
```

`send` prints the message id, for example `queued message #561 to codex (2747 bytes)`. The dashboard delivers the message into the target's session with a banner that says how to answer:

```
[message #561 from claude#2; reply with: wsx agent reply 561 <message>]
…message body…
```

A sender in another workspace shows as `<repo>/<slug> <label>`. A message from a plain shell has the banner `[message #561]`.

- Delivery waits until the target is idle at its prompt. If the target isn't running, wsx starts it.
- Only a running dashboard delivers messages. Without one, `send` warns and the message waits in the queue.
- `send` refuses a target whose binary isn't installed.
- A message that fails to deliver several times shows a red `✉!` badge on the dashboard row. Restart wsx to retry.
- Delivery is at least once. A wsx crash mid-delivery can repeat a message.

To answer, read mail, or wait for a reply:

```bash
wsx agent reply 561 "tests pass"          # reply to message #561
wsx agent messages                        # your inbox
wsx agent wait --after 561 --done codex   # block until codex replies or reports done
wsx agent whoami                          # your label, instance id, and workspace
```

`wsx agent wait` only reads a message. The dashboard still delivers it later, so the agent sees the same `[message #561 …]` banner again and should treat it as a duplicate.

The [`wsx agent`](../cli-reference/agent.md) reference covers every flag: [`send`](../cli-reference/agent.md#wsx-agent-send), [`reply`](../cli-reference/agent.md#wsx-agent-reply), [`messages`](../cli-reference/agent.md#wsx-agent-messages), [`wait`](../cli-reference/agent.md#wsx-agent-wait), and [`whoami`](../cli-reference/agent.md#wsx-agent-whoami).

## List agents

```bash
wsx agent list
```

```
1  claude  (primary)  working — "running the test suite" (model, 3m ago)
2  claude#2  done (hook, 10m ago)
4  codex
```

The number is the instance id. Each agent's own status follows its label. See [Per-agent status](status-and-recap.md#per-agent-status) and [`wsx agent list`](../cli-reference/agent.md#wsx-agent-list).

## Sessions survive a restart

When you quit and restart wsx, each agent resumes its own conversation, even when several agents of one kind share the worktree.

| Agent    | Resumes with                                                                      |
| -------- | --------------------------------------------------------------------------------- |
| `claude` | `claude --resume <id>`, with the id from wsx's status hooks. Follows `/clear`.    |
| `codex`  | `codex resume <id>`, after the agent's first turn ends                            |
| `pi`     | `pi --session-id <id>`. Follows `/new`, `/resume`, and forks.                     |
| `omp`    | `omp --resume=<file>`. Follows `/new`.                                            |
| `hermes` | The primary resumes its newest session. Added agents start fresh.                 |

- Until wsx has an agent's session id, the primary uses its harness's "continue latest" and an added agent starts fresh.
- If an agent's recorded session no longer exists on disk, the agent starts fresh.
- Changing a workspace's agent kind forgets its sessions.
- wsx looks only in each harness's default session directory. A custom `CODEX_HOME` or `CLAUDE_CONFIG_DIR` isn't supported.

## Inspect other workspaces

The read commands take `--workspace <repo>/<slug>`, so an agent can check on a workspace it handed work to:

```bash
wsx status show  --workspace backend/add-widgets
wsx recap show   --workspace backend/add-widgets
wsx agent list   --workspace backend/add-widgets
wsx context show --workspace backend/add-widgets
```

For every workspace at once, use [`wsx workspace list --json`](../cli-reference/workspace.md#wsx-workspace-list).
