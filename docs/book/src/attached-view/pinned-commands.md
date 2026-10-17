# Pinned commands

Pinned commands put the prompts you send often, such as `/pull-request`, on a chip row below the agent pane. Fire one with a key instead of typing it.

```
[1] PR   [2] FB   [3] /loop /baby…   [4] UR
```

| Key                  | Action                     |
| -------------------- | -------------------------- |
| `Ctrl-x 1` … `Ctrl-x 9` | Fire chip 1–9           |
| Click a chip         | Fire that chip             |

A chip types its command into the agent and presses `Enter`. `Ctrl-x <digit>` also works on the dashboard, where it sends the command to the selected workspace's agent and starts its session if needed.

## Configure chips

Set the `pinned_commands` setting, one entry per line:

```
PR=/pull-request
FB=/feedback
/loop /babysit-prs
review=/agent-review ...
```

- `Label=command` shows the label on the chip. A bare line shows the command itself.
- Labels longer than 14 columns are truncated.
- Only the first 9 chips get keys. At narrow widths trailing chips are hidden, but their keys still work.

```bash
wsx config edit pinned_commands               # edit in $EDITOR
wsx config set pinned_commands @./pinned.txt  # load from a file
wsx config set pinned_commands ""             # clear
```

`wsx config set` replaces the whole list. Use `wsx config edit` to add one chip. A repo's own list, set with [`wsx repo set-pinned-commands`](../cli-reference/repo.md#wsx-repo-set-pinned-commands), replaces the global list for that repo. See [Settings](../configuration/settings.md).

## Chips that type without submitting

End a command with `...` or `…` to type it without pressing `Enter`. Use this for a command that takes an argument you choose each time:

```
review=/agent-review ...
```

- Attached, the chip types `/agent-review ` and leaves the cursor there.
- On the dashboard, the text goes into the detail bar's reply input. Finish it there and press `Enter`.
- A command that really ends in `...` can't be pinned as a submitting chip.

Some commands ask for missing input on their own. The bundled [`handoff`](../agents/skills.md#bundled-skills) skill asks what to implement, and [`agent-review`](../agents/skills.md#bundled-skills) asks which reviewer to spawn.
