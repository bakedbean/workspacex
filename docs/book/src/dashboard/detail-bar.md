# Detail bar

The detail bar runs along the bottom of the dashboard and shows more about the selected workspace: its recap, recent chat, processes, and changed files. It ends in a reply input, so you can answer an agent without attaching.

## Layout

The bar holds 1–4 equal-width columns, called containers. Each container stacks one or more modules:

| Module            | Shows                                                         |
| ----------------- | ------------------------------------------------------------- |
| `session_summary` | The recap (`goal`, `state`, `next`), tool counts, status, model |
| `recent_chat`     | The latest messages between you and the agent                 |
| `processes`       | [Tracked processes](process-tracking.md) in the worktree      |
| `recent_files`    | Files the agent edited recently, with line counts             |
| `agents`          | Claude subagents running in the workspace, and what each is doing |

`session_summary` falls back to the session's first prompt when the agent hasn't written a recap. Scroll a container with the mouse wheel.

`agents` isn't in the default layout; add it to a container to show it. It counts the subagents (from Claude's Agent tool) running across the workspace's Claude sessions, the primary's and any peers', and lists each with its task, how long it has run, its type and its latest tool call, named the way Claude Code's footer names it:

```text
2 subagents running
● Find the poller                       1m
  Explore · Grep(fn poll)
● Review the diff                      12s
  general-purpose · Bash(Run the tests)
```

wsx reads this from the files Claude Code keeps beside each session's transcript, so it updates with the dashboard's 2-second poll and needs no setup. A subagent drops off when Claude marks it finished, or when the Claude session it belongs to exits. One whose transcript hasn't changed in 15 minutes shows `quiet 20m · ` (in the warn color) ahead of what it last did: it may be waiting on a slow build or command, or its session may have died without wsx seeing it exit. After 2 hours of silence it's taken for abandoned and dropped. Other agent kinds don't report subagents.

The activity line shows what the subagent last ran or looked up, such as a Bash command's description (or the command itself), a search pattern, a URL or a web search query, just as the dashboard already shows the main agent's current command.

## Arranging modules

Press `b` on the dashboard to open the layout editor. It shows the bar's containers side by side, with the modules each one stacks. Every change is saved as you make it, so the bar updates as you go.

| Key                | Action                                                         |
| ------------------ | -------------------------------------------------------------- |
| `←` `→` `↑` `↓` / `h` `l` `k` `j` | Move between containers and modules |
| `a` / `enter`      | Open the module menu for the focused container                 |
| `d` / `x`          | Remove the highlighted module                                  |
| `H` / `L`          | Move the module into the container to the left / right         |
| `K` / `J`          | Move the module up / down within its container                 |
| `<` / `>`          | Swap the focused container with the one to its left / right    |
| `n`                | Add an empty container to the right (up to 4)                  |
| `X`                | Delete the focused container and its modules (one always stays)|
| `r`                | Reset to the default layout                                    |
| `esc` / `b`        | Close                                                          |

The module menu lists every module with a description. `[x]` marks the modules already in the focused container, and `in 2` marks a module that sits in another container. Press `enter` to add the highlighted module to the focused container. If it sits in another container, it moves, since a module shows once. If it's already in the focused container, `enter` takes it out. `d` removes it from wherever it sits. The menu stays open so you can add several modules; `esc` goes back to the containers.

The editor changes the selected repo's override when that override sets `containers` (the title then reads `repo <name>`). Otherwise it changes the global layout. Adding, deleting, or swapping containers keeps the widths you dragged for the others.

## Replying

Press `Tab` to focus the reply input, type, and press `enter` to send it to the agent. wsx starts the agent first if it isn't running. `esc` discards the draft.

## Configuring

The `detail_bar_config` setting holds a JSON value. Every field is optional, and missing fields take the defaults:

```json
{
  "visible": true,
  "height": { "percent": 30, "min_rows": 8, "max_rows": 18 },
  "containers": [["session_summary"], ["recent_chat"], ["processes", "recent_files"]],
  "bottom_rule": true
}
```

| Field             | Default  | Effect                                                      |
| ----------------- | -------- | ----------------------------------------------------------- |
| `visible`         | `true`   | `false` hides the bar and its reply input                   |
| `height.percent`  | `30`     | Height as a percent of the terminal, 5–80                   |
| `height.min_rows` | `8`      | Minimum height in rows, 4–40                                |
| `height.max_rows` | `18`     | Maximum height in rows, 4–60                                |
| `containers`      | as above | Up to 4 columns of module IDs; `[]` leaves a column empty   |
| `bottom_rule`     | `true`   | Draw a rule under the reply row; `false` saves a row        |

To resize the bar by hand, drag its top edge (the header row or the rule under it) up or down. wsx remembers the dragged height across restarts in the `detail_bar_height` setting, which takes the place of `height` while it's set; unset it to go back to `height`. The bar never gets shorter than `min_rows`, and it always leaves the workspace list at least 10 rows. On a screen too short for both (counting the PM pane when it shows), the bar can't be dragged and takes its automatic size, `detail_bar_height` or not. A click on the edge without dragging changes nothing.

To resize the containers, drag the line between two of them left or right. Only the two containers beside that line change, and each keeps at least 12 columns, on any screen wide enough for every container to have 12. wsx remembers the widths across restarts in the `detail_bar_widths` setting, as proportions, so they keep the same split when the terminal is resized. Widths saved for a different number of containers are ignored, and the containers share the width evenly. Below 80 columns the bar shows only its first container, so there's nothing to drag. A click on the line without dragging changes nothing.

When every container is empty (`[[], [], []]`), the bar shrinks to its header and reply input. An unknown module ID shows a `[unknown: <id>]` placeholder.

Set or edit the global value:

```bash
wsx config edit detail_bar_config          # open in $EDITOR
wsx config set detail_bar_config ""        # back to the defaults
```

Examples:

```bash
# Taller bar on a big monitor
wsx config set detail_bar_config '{"height": {"percent": 45, "max_rows": 24}}'

# One full-width chat column
wsx config set detail_bar_config '{"containers": [["recent_chat"]]}'

# Running subagents above processes and files
wsx config set detail_bar_config '{"containers": [["session_summary"], ["recent_chat"], ["agents", "processes", "recent_files"]]}'

# Hide the bar
wsx config set detail_bar_config '{"visible": false}'
```

`wsx config set` rejects malformed JSON and keeps the old value. Out-of-range heights are clamped.

## Per-repo override

To change the bar for one repo, press `s` on the dashboard, select `detail_bar_config`, and press `enter`. `$EDITOR` opens on the repo's override. Write only the fields you want to change, for example `{ "visible": false }`. Press `d` on the row to clear the override.

`visible`, `height`, and `bottom_rule` override field by field. `containers` replaces the global list.
