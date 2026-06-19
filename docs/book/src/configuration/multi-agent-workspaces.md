A workspace isn't limited to a single agent. You can attach **additional agents — of the same kind or different kinds — to one workspace.** Every agent runs as its own session but they all share the same git worktree and branch, and they can message each other. This is useful for, say, running a second Claude as a dedicated reviewer alongside the one doing the work, or pitting `claude` and `codex` at the same problem in the same tree.

Every workspace starts with exactly one agent — the **primary**, chosen at creation time by `--agent` or the `coding_agent` setting (see [Coding agents](coding-agents.md)). Everything below is about adding more on top of that.

### Adding and removing agents

In the TUI, press `Ctrl-x a` while a workspace is selected to open the **agents panel**. It lists the agents already attached (the primary is tagged `(primary)`) and an "add" picker of the four kinds:

| Key      | Action                                                  |
| -------- | ------------------------------------------------------- |
| `↑`/`↓`  | Move through the add picker                             |
| `Enter`  | Add the highlighted kind                                |
| `a`      | Add one of every kind at once                           |
| `x`      | Remove the most-recently-added (non-primary) agent      |
| `Esc`    | Close the panel                                         |

Newly added agents spawn immediately with the workspace's context injected. The primary can't be removed from the panel — it lives for the life of the workspace.

From the CLI, the equivalent of the panel's "add" is:

```bash
wsx agent add <kind>     # kind = claude | pi | hermes | codex
```

This runs against the **current** workspace — the one whose worktree you're in, or the one named by `$WSX_WORKSPACE_ID` (see [identity](#agent-identity-and-labels) below). It prints the new agent's label, e.g. `added claude#2`.

### Switching focus between agents

When a workspace has more than one agent, the attached view grows a **footer agents row** listing each agent with a single-letter switch key:

```
agents:  ▎claude q   ▎codex w   ▎pi r
```

Press the key (`q`, `w`, `r`, …) to point the focused pane at that agent's session, or click the pill. The keys are drawn from a fixed pool — `q w r y i o p s h j` — assigned in display order (primary first). A workspace with more than ten agents renders the rest keyless, but they stay clickable. The row only appears once a second agent exists; a single-agent workspace looks exactly as before.

Because agents share the worktree, switching focus is just changing which session your keystrokes go to — there's no branch-swapping or checkout involved.

### Inter-agent messaging

Agents in the same workspace can send each other messages:

```bash
wsx agent send <label> <message…>
```

`<label>` is an agent's footer/list label (`claude`, `claude#2`, `codex`, …). The rest of the line is the message body. Delivery is **asynchronous**: the message is queued and injected into the target's session on the next tick, prefixed with a banner so the recipient knows where it came from:

```
[message from claude#2]
…your message body…
```

If the sender is the `wsx` CLI itself (not another agent — i.e. `$WSX_AGENT_INSTANCE_ID` is unset), the banner is just `[message]`. If the target agent isn't running yet, wsx spawns it first, then delivers. Sending to a label that doesn't exist in the workspace errors with a hint to run `wsx agent list`.

Since all agents write to the same files, prefer messaging to hand off work rather than editing the same paths in parallel.

### Listing agents

```bash
wsx agent list
```

Prints one agent per line — its instance id and label, with `(primary)` appended for the primary — for the current workspace:

```
1  claude  (primary)
2  claude#2
4  codex
```

The leading number is the agent's instance id — the same value wsx injects as `$WSX_AGENT_INSTANCE_ID` into that agent's session.

### Agent identity and labels

Each agent instance has a **label** derived from its kind and its ordinal within that kind: the first of a kind is the bare name (`claude`), and each subsequent one of the same kind gets a `#N` suffix (`claude#2`, `claude#3`). The same rule produces the labels shown in the footer row, in `wsx agent list`, and in message banners.

When wsx spawns an agent it injects two environment variables into that session, so the agent (or scripts it runs) can address the multi-agent CLI without guessing:

| Variable                 | Value                                              |
| ------------------------ | -------------------------------------------------- |
| `WSX_WORKSPACE_ID`       | The workspace this agent belongs to                |
| `WSX_AGENT_INSTANCE_ID`  | This specific agent instance                       |

`wsx agent` commands resolve the "current" workspace from `$WSX_WORKSPACE_ID` first, falling back to matching the current directory against known worktrees — so the commands work both from inside an agent session and from a plain shell in the worktree. `wsx agent send` uses `$WSX_AGENT_INSTANCE_ID` to stamp the `[message from …]` sender on outgoing messages.
