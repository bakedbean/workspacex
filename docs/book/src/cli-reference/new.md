# wsx new

```
wsx new [--no-yolo] [--agent <kind>] [<repo> [task...]]
```

Creates a workspace, starts its agent in tmux, and gives it the task. Made for a phone: ssh in, run it, then switch to the Claude app.

With no repo it asks for one (most recently used first; Enter picks the first) and for an optional task. With a repo it asks nothing. Everything after the repo is the task, so flags go before it.

| Flag | Meaning |
| --- | --- |
| `--no-yolo` | Start restricted. The default is yolo, unlike `wsx workspace create`. Run inside a workspace, it follows that workspace's mode. |
| `--agent <kind>` | claude, pi, hermes, codex or omp. Defaults to the enclosing workspace's agent, else `coding_agent`. |

## What it does

The work runs in the background, so a dropped connection doesn't stop it. A running dashboard starts the agent; with none, `wsx new` starts it and the dashboard picks it up later. The task is sent only once the Claude app can reach the agent.

| Result | Exit |
| --- | --- |
| `✓ … agent running, open the Claude app` | 0 |
| `✓ … agent running` (not Claude, or `remote_control` is off) | 0 |
| Not visible to the Claude app within 60 seconds: prints the agent's screen and how to attach. The task is sent if it connects within 30 minutes. | 1 |
| Task not delivered: prints how to send it again, or that it is still queued | 1 |
| The agent exited or didn't start | 1 |

An undelivered task is kept in `logs/new/` under the wsx state directory.

## Termius

Save `wsx new` or `wsx new <repo>` as a snippet. The agent needs a trusted repo: in one Claude hasn't been trusted in, it stops at a prompt you can answer with the printed attach command.

## macOS over ssh

Claude's login is in the Keychain, which an ssh session can't read, so the background work runs as a one-off job in your desktop session. Someone must be logged in there; a locked screen is fine. If a tmux server was itself started over ssh, agents in it still have no login: run `tmux kill-server` when nothing in it is needed.
