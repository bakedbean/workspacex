# Status indicators

Each dashboard row fits a workspace's agent status, PR, processes, and changes on one line, so you can see which workspace needs you without attaching.

## Row columns

Left to right:

| Column       | Shows                                                                 |
| ------------ | --------------------------------------------------------------------- |
| Agent strip  | One colored bar per agent, primary rightmost                          |
| Status       | A colored bar and a status glyph (see below)                          |
| Branch       | `⎇` colored by PR state, the name, tags, and badges                   |
| PR           | `⏺ #241 open` plus a review mark                                      |
| Processes    | `● 2p` for two [tracked processes](process-tracking.md), `·` for none |
| Diff         | `+N −N` lines committed on the branch since its base                  |
| Message      | Status word, then the agent's recap or its latest activity            |
| Age          | Time since the last activity                                          |

The name is bold, and warn-colored for a workspace created in permissive mode unless you picked a name color with `C`. In the message column, `▸` means the agent reported its status with [`wsx status`](../agents/status-and-recap.md) and `└` means wsx inferred it.

## Status glyphs

| Glyph    | Word      | Meaning                                                 |
| -------- | --------- | ------------------------------------------------------- |
| `?`      | asking    | The agent asked a question or waits on a permission prompt |
| `!`      | stalled   | No output for 60 seconds mid-turn                       |
| spinner  | thinking  | The agent is working                                    |
| spinner  | waiting   | The session is running but quiet for over 30 seconds    |
| `✓`      | done      | The agent finished its turn                             |
| `·`      | idle      | No session, or a session you haven't prompted yet       |

In a workspace with several agents, a peer's bar in the agent strip turns into a spinner while that agent prints output. A `+` stands in for peers that don't fit.

## Branch badges

| Badge        | Meaning                                                     |
| ------------ | ----------------------------------------------------------- |
| spinner      | Being created or archived                                   |
| `⚙!`         | Setup script failed; the workspace still works. `?` then `o` shows the log |
| `⚙?`         | Setup was cancelled                                         |
| `✗`          | The worktree was never created. Archive it with `d`         |
| `✉!`         | A message to the agent couldn't be delivered                |
| `◇`          | tmux-shared: green while its session runs, red when it doesn't |
| columns icon | A saved split layout (nerd fonts only)                      |

## PR chip

The chip and the `⎇` glyph take the PR state's color: no PR, draft, open, conflicts, merged, or closed. Open PRs also carry a review mark:

| Mark | Meaning                                                 |
| ---- | ------------------------------------------------------- |
| `✓`  | Approved                                                |
| `✗`  | Changes requested; a number counts unresolved threads   |
| `◌`  | Review required                                         |

PR data needs [`gh`](https://cli.github.com), installed and authenticated.

## Alerts

When an agent asks a question, waits on a permission prompt, finishes, or stalls, wsx rings the terminal bell. The workspace also moves to the front of the attached view's [workspace strip](../attached-view/updates-and-splits.md) until you attach to it.

- Each state has its own bell pattern, set with the `notification_bell_*` settings.
- `wsx config set notifications off` turns off the bell and the flag.
- Workspaces that were already waiting when wsx started don't ring.
- While wsx runs, the bell doesn't ask the window manager for urgency, so on Hyprland it won't switch your desktop workspace.

See [Settings](../configuration/settings.md) for the allowed values.
