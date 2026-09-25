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

Removing an agent with `x` closes the panel and drops that agent's pane from the attached view. If you were focused on it, focus moves to another pane in the same workspace; if it was the only pane, the view re-attaches to the workspace's primary. Either way you stay attached rather than being returned to the dashboard.

From the CLI, the equivalent of the panel's "add" is:

```bash
wsx agent add <kind>     # kind = claude | pi | hermes | codex | omp
```

This runs against the **current** workspace — the one whose worktree you're in, or the one named by `$WSX_WORKSPACE_ID` (see [identity](#agent-identity-and-labels) below). It prints the new agent's label, e.g. `added claude#2`.

### Sessions survive a restart

Quit wsx and come back, and each Claude, Codex, pi and omp agent in the workspace gets its own conversation back — not a blank chat, and not each other's (Hermes is the exception, see below). A harness's own "continue" flag can't do that: it resumes the *most recent* conversation in the directory, and once two agents share a worktree that is whichever one spoke last. So wsx tracks a session id **per agent instance** and respawns with it. How the id is obtained depends on the harness:

| Agent    | Where the id comes from                                                                                   | Respawn                 |
| -------- | --------------------------------------------------------------------------------------------------------- | ----------------------- |
| `claude` | Reported by the status hooks wsx injects (every payload carries it; each hook runs with that instance's `$WSX_AGENT_INSTANCE_ID`). A `/clear` moves the recorded id along with it. | `claude --resume <id>`  |
| `codex`  | The `thread-id` in the `notify` payload wsx already receives after each turn.                             | `codex resume <id>`     |
| `pi`     | Minted by wsx at the instance's first spawn and passed as `--session-id`, which pi creates-or-resumes (pi prints a one-line "creating a new session with that id" notice on that first spawn). From then on pi reports every session start itself: wsx loads a small extension (`pi -e <wsx state dir>/pi-session-report.ts`) that calls back into wsx on `session_start`, so `/new`, `/resume` and forks all move the recorded id. A pi primary from before ids were tracked adopts its newest session on its next spawn, skipping any session a pi peer owns or a previous occupant of the path left behind. If another registered pi instance has no recorded session id, ownership is ambiguous and adoption is skipped; agents of other kinds do not block adoption. With ambiguous ownership or nothing eligible, the primary starts fresh with a new pin rather than pi's cwd-wide continue. | `pi --session-id <id>`  |
| `omp`    | Read from omp's own per-terminal breadcrumb (`~/.omp/agent/terminal-sessions/<pts-N>`), which names the session file each terminal last opened. wsx created the terminal (or, in a shared workspace, asks tmux which terminal the pane is), so the file maps to exactly one instance. Device numbers get reused, so a crumb that already existed when wsx created the terminal is never taken for this agent's, however recent; under tmux, crumbs older than the tmux session are ignored instead. wsx polls every ~2s while the agent runs and once more on quit and share/unshare, so a `/new` is followed. | `omp --resume=<file>`   |
| `hermes` | Not available: the id only exists inside the Hermes process.                                              | added agents start fresh |

Except for pi's adoption-or-fresh behaviour above, until an instance has an id — a workspace created before this was tracked, an agent that has not completed a turn yet (Codex) or written its session file yet (omp), or a `hermes` peer — the old behaviour applies: the primary falls back to its harness's cwd-wide continue, an added agent starts fresh with its handoff note.

An instance whose recorded session is no longer on disk starts **fresh**, primary or not, never with the cwd-wide continue: its own conversation is gone (deleted, or a `/new` that omp had not yet written out), so the directory's most recent session is by definition someone else's. Changing a workspace's agent kind clears the recorded identity, since a session belongs to one harness.

Known limits: the session roots are the harnesses' defaults (`~/.claude/projects`, `~/.codex/sessions`, `~/.pi/agent/sessions`, `~/.omp/agent`); a non-default `CODEX_HOME`, `CLAUDE_CONFIG_DIR` or pi/omp session directory is not consulted. An omp `/new` followed by exiting omp within the same ~2s poll is not captured. The tmux lookup reads the session's active pane, so splitting the shared pane before wsx first looks can point it at the wrong terminal.

### Switching focus between agents

When a workspace has more than one agent, the attached view's bottom row (the one with the pinned-command chips) gains a set of **agent pills**, right-justified ahead of the workspace stats, listing each agent with a single-letter switch key:

```
 ^x  menu   1 pr   2 fb  ────────   ● claude q   ○ codex w   ○ pi r    opus 4.8 45k/200k ● 2p +12 −3 ⏺ #152 open
```

Press `Ctrl-x` then the key (`q`, `w`, `r`, …) to point the focused pane at that agent's session, or click the pill. Each pill opens with a dot in the agent's kind colour; the pill whose agent is in the focused pane carries a filled dot (`●`), the others a hollow one (`○`). The keys are drawn from a fixed pool — `q w r y i o p s h j` — assigned in display order (primary first). A workspace with more than ten agents renders the rest keyless; they stay clickable whenever the pills are shown. The pills only appear once a second agent exists; a single-agent workspace looks exactly as before. On a terminal too narrow for everything, the model/token stat is dropped first, then the pills as a whole group (never partially). The `Ctrl-x` switch keys keep working with the pills hidden; a keyless agent needs a wider terminal to be clicked.

Because agents share the worktree, switching focus is just changing which session your keystrokes go to — there's no branch-swapping or checkout involved.

The model and context-token usage follow the agent in the **focused pane**, including when focus moves between split panes. Each agent's recorded session is tracked independently. If its transcript or usage is unavailable—or an unrecorded session cannot be distinguished from another agent of the same kind—the usage chip is omitted rather than showing another agent's numbers.

### Inter-agent messaging

Agents can send each other messages — a peer in the same workspace by default, or any agent in another workspace with `--workspace`:

```bash
wsx agent send [--workspace <repo>/<slug>] [--file <path>|-] <label|instance-id> [<message…>|-]
```

`<label>` is an agent's footer/list label (`claude`, `claude#2`, `codex`, …),
or the reserved label `primary` for the workspace's primary agent. A target of
all digits is an agent's instance id (from `wsx agent whoami` or
`wsx agent list`), which is unique across workspaces and so needs no
`--workspace`. Without `--workspace` a label is looked up in the current
workspace; with it, in any workspace — which is how one agent hands a task to a
freshly created workspace's agent.

The rest of the line is the message body. For long or code-heavy bodies use
`--file <path>`, or `-` (as the body, or `--file -`) to read it from stdin; the
text is sent verbatim, with trailing whitespace dropped. Options go before the
target — everything after it is body, so a message may itself start with `--`.
An empty body is refused.

`send` prints the new message's id and size:

```
queued message #561 to claude#2 (2747 bytes)
```

Delivery is **asynchronous**: the message is queued and injected into the
target's session on the next tick, prefixed with a banner carrying its id,
where it came from, and how to answer it:

```
[message #561 from claude#2; reply with: wsx agent reply 561 <message>]
…your message body…
```

A sender in a *different* workspace is qualified with its `<repo>/<slug>`, so
the recipient can see which workspace the work came from:

```
[message #562 from workspacex/parent-task claude; reply with: wsx agent reply 562 <message>]
…your message body…
```

If the sender is the `wsx` CLI itself (not another agent — i.e. `$WSX_AGENT_INSTANCE_ID` is unset), the banner is just `[message #<id>]`. If the target agent isn't running yet, wsx spawns it first, then delivers. Sending to a label that doesn't exist in the target workspace errors with that workspace's agent labels listed inline (`wsx agent list` only reports the current workspace, so it can't describe another one).

Queued messages are injected by the running `wsx` TUI, so `wsx agent send`
warns on stderr when no dashboard is running — the message stays queued and is
delivered when one starts.

Delivery waits for the target agent to be ready to accept input: its TUI must
be up (not still booting) and its output quiet. A cold agent takes a second or
two to get there, and one that's midway through a turn takes as long as the
turn does — the message lands at its prompt rather than mid-work. A message is
only marked delivered once it has actually been written to the agent's
terminal; if the write doesn't happen, it stays queued and is retried. After
several failed attempts wsx stops retrying and the workspace's dashboard row
shows a red `✉!` badge, so an undeliverable message is visible rather than
silently dropped. Restarting `wsx` clears the attempt counts and retries.

Mail queued while no dashboard was running is delivered when one starts. Only
one message at a time is injected into any given agent — messages that arrive
while a delivery is in flight wait their turn, so they can't interleave in the
agent's terminal.

A message counts as delivered only when the terminal write for it is
acknowledged, so a queued write that never reaches the agent is retried rather
than recorded as sent.

Delivery is *at-least-once* across a crash: if `wsx` dies after writing a
message into an agent's terminal but before recording it as delivered, the
message is still queued and will be injected again when `wsx` restarts. The
in-flight bookkeeping that prevents duplicates is in memory, so it does not
survive the process.

Since all agents write to the same files, prefer messaging to hand off work rather than editing the same paths in parallel.

### Replying, reading, and waiting

```bash
wsx agent reply [--file <path>|-] [<msg-id>] [<message…>|-]
wsx agent messages [--sent|--all] [--undelivered] [--limit <n>] [--id <msg-id>]
wsx agent wait [--from <sender>] [--after <msg-id>] [--timeout <secs>]
wsx agent whoami
```

These act as the calling agent (`$WSX_AGENT_INSTANCE_ID`).

- **`reply`** queues a message to the sender of message `<msg-id>` (`561` or
  `#561`), wherever that sender lives; without an id it answers the latest
  message you received. A lone number is treated as the body, so
  `wsx agent reply 42` sends "42". Messages from the CLI or an editor agent have
  no sender to reply to.
- **`messages`** lists your inbox (`--sent`: what you sent; `--all`: every
  message queued in, or sent from, the current workspace — this one also works
  from a plain shell) as TSV, newest `--limit` rows (default 20), oldest first:

  ```
  ID	FROM	TO	BYTES	CREATED	DELIVERED
  561	claude#2	claude	2747	2026-09-25T14:03:11Z	2026-09-25T14:03:14Z
  562	workspacex/parent-task claude	claude	812	2026-09-25T14:10:40Z	queued
  ```

  `DELIVERED` stays `queued` until the dashboard has written the message into
  the agent's terminal. `--undelivered` shows only those; `--id <msg-id>` prints
  one message's headers and full body.
- **`wait`** blocks until a message for you is recorded, then prints it like
  `messages --id`. Messages already queued for you when it starts count (they
  haven't reached you yet); `--after <msg-id>` instead counts only newer ids,
  and `--from` accepts a label, `primary`, an instance id, or
  `<repo>/<slug> <label>` as shown in banners. It gives up after `--timeout`
  seconds (default 110, under common agent tool timeouts; `0` waits forever).
- **`whoami`** prints your label, instance id, workspace, and whether you are
  the primary.

`wait` is a **read**, not a delivery. The dashboard remains the only thing that
marks a message delivered, and it still injects the message into your session
afterwards — so an agent that handled message `#561` via `wait` will later see
the same `[message #561 …]` banner and should treat it as a duplicate. The
alternative, letting `wait` consume the message, would make two processes
race to deliver it (the dashboard may already have an injection in flight that
is waiting for the agent to go idle), trading a recognisable duplicate for a
possible lost or doubly-injected one. Delivery stays at-least-once either way.

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

`wsx agent` commands resolve the "current" workspace from `$WSX_WORKSPACE_ID` first, falling back to matching the current directory against known worktrees — so the commands work both from inside an agent session and from a plain shell in the worktree. `wsx agent send` uses `$WSX_AGENT_INSTANCE_ID` to stamp the `[message #<id> from …]` sender on outgoing messages.

`--workspace <repo>/<slug>` overrides that resolution for the *target*;
`$WSX_AGENT_INSTANCE_ID` still identifies the sender, which is how a
cross-workspace message gets its `<repo>/<slug>`-qualified banner.
