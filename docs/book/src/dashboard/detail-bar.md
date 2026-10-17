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

`session_summary` falls back to the session's first prompt when the agent hasn't written a recap. Scroll a container with the mouse wheel.

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

# Hide the bar
wsx config set detail_bar_config '{"visible": false}'
```

`wsx config set` rejects malformed JSON and keeps the old value. Out-of-range heights are clamped.

## Per-repo override

To change the bar for one repo, press `s` on the dashboard, select `detail_bar_config`, and press `enter`. `$EDITOR` opens on the repo's override. Write only the fields you want to change, for example `{ "visible": false }`. Press `d` on the row to clear the override.

`visible`, `height`, and `bottom_rule` override field by field. `containers` replaces the global list.
