# wsx agent

Commands that add agents to a workspace and pass messages between them. See [Multi-agent workspaces](../agents/multi-agent-workspaces.md) for how agents share a worktree.

An agent's label is its kind, with `#N` for the second and later agents of that kind: `claude`, `claude#2`, `codex`. `primary` always names the workspace's primary agent. An all-digit target is an instance id, which is unique across workspaces. `send`, `reply`, `messages` and `wait` identify you by `$WSX_AGENT_INSTANCE_ID`, which wsx sets in each agent session.

## wsx agent list

```
wsx agent list [--workspace <repo>/<slug>] [--json]
```

Lists the workspace's agents with their instance ids and each agent's last status.

```
1  claude  (primary)  working — "running the test suite" (model, 3m ago)
2  claude#2  done (hook, 10m ago)
4  codex
```

`--json` prints `{id, label, kind, primary, status}` objects. `status` is `{state, message, source, reported_at}` or `null`.

## wsx agent add

```
wsx agent add <kind>
```

Adds an agent of `<kind>` (`claude`, `pi`, `hermes`, `codex` or `omp`) to the current workspace and prints its label. The dashboard starts it.

## wsx agent remove

```
wsx agent remove <label>
```

Removes an agent from the current workspace.

- You can't remove the primary agent. Archive the workspace instead.
- Messages still queued for the agent are discarded. The command reports how many.

## wsx agent send

```
wsx agent send [--workspace <repo>/<slug>] [--file <path>|-] <label|instance-id> [<message...>|-]
```

Queues a message for an agent and prints its id, for example `queued message #561 to claude#2 (2747 bytes)`.

| Flag | Effect |
| --- | --- |
| `--workspace <repo>/<slug>` | Look up the label in another workspace. |
| `--file <path>` | Read the body from a file. `--file -` or a body of `-` reads stdin. |

- The running dashboard delivers the message when the target is idle, starting the agent first if needed. With no dashboard running, `send` warns and the message waits.
- The recipient sees a banner with the message id, the sender and the reply command.
- Flags can go before or after the target. From the first message word on, everything is body text. To send text containing `--file` or `--workspace`, quote the whole message.
- An empty body is refused. So is a target whose agent binary isn't installed.

## wsx agent reply

```
wsx agent reply [--file <path>|-] [<msg-id>] [<message...>|-]
```

Queues a message to the sender of `<msg-id>` (`561` or `#561`), wherever it lives. Without an id it answers the latest message you received.

- The first word counts as an id only when a body follows, so `wsx agent reply 42` sends "42". Prefer the explicit id from the banner.
- Messages sent from a shell or an editor agent have no sender to reply to.

## wsx agent messages

```
wsx agent messages [--sent|--all] [--undelivered] [--limit <n>] [--id <msg-id>] [--json]
```

Lists your inbox as tab-separated rows, oldest first.

```
ID   FROM                           TO      BYTES  CREATED               DELIVERED
561  claude#2                       claude  2747   2026-09-25T14:03:11Z  2026-09-25T14:03:14Z
562  workspacex/parent-task claude  claude  812    2026-09-25T14:10:40Z  queued
```

| Flag | Effect |
| --- | --- |
| `--sent` | List messages you sent. |
| `--all` | List every message in the current workspace. Works from a plain shell. |
| `--undelivered` | Only `queued` and `dropped` messages. |
| `--limit <n>` | Show the newest `n`. Default: 20. |
| `--id <msg-id>` | Print one message's headers and full body. |
| `--json` | Print `{id, from_id, from, to_id, to, workspace, bytes, body, created_at, delivered_at, state, drop_reason}` objects. |

- `DELIVERED` reads `dropped` when wsx gave up on a message, for example because the agent's binary is missing. `--id` shows the reason.

## wsx agent wait

```
wsx agent wait [--from <sender>] [--after <msg-id>] [--timeout <secs>] [--done <agent>]
```

Blocks until a message for you arrives, then prints it like `messages --id`.

| Flag | Effect |
| --- | --- |
| `--from <sender>` | Only messages from this agent: a label, `primary`, an instance id, or `<repo>/<slug> <label>`. |
| `--after <msg-id>` | Only messages newer than this id. Without it, already-queued messages count. |
| `--timeout <secs>` | Give up after this long. Default: 110. `0` waits forever. |
| `--done <agent>` | Also return when this agent reports `done` or `blocked` after the wait (or the `--after` message) began. Works from a plain shell. |

- `wait` doesn't consume the message. The dashboard still injects it into your session, so you see it twice. Chain waits with `--after <last id>`.
- For a request and reply: `send` a task, then `wait --after <its id> --done <peer>`.

## wsx agent whoami

```
wsx agent whoami
```

Prints your agent label, instance id, workspace and whether you're the primary.
