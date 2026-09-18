If `pinned_commands` is configured (globally or per-repo), a one-row chip strip appears between the claude pane and the footer. Each chip shows `[N] Label`:

```
[1] PR   [2] FB   [3] /loop /baby…   [4] UR
```

Fire a chip with `Ctrl-x <digit>` (1-9) or by clicking on it. The chip's command + `\r` is written to claude exactly as if you'd typed and submitted it.

Configure via the standard config CLI:

```bash
wsx config edit pinned_commands               # opens $EDITOR on the current value
wsx config set pinned_commands @./pinned.txt  # load from a file
wsx config set pinned_commands ""             # clear
```

One entry per line:

```
PR=/pull-request
FB=/feedback
/loop /babysit-prs
UR=/ultrareview
```

`Label=command` shows the label as the chip; a bare line uses the command itself. Labels are truncated past 14 columns. Both sides of `=` are trimmed.

At narrow terminal widths trailing chips drop from view; their keyboard shortcuts still work.

Chips submit by default. To leave a command typed but unsubmitted — for one that takes an argument you want to choose each time — end it with `...` (or `…`):

```
review=/agent-review ...
```

Firing that chip writes `/agent-review ` (the marker is stripped; the space before it is kept) and leaves the cursor there, so you type the reviewer kind and press enter. Put the marker in the label too (`review…=/agent-review ...`) if you want the chip itself to show it won't submit; a bare `/agent-review ...` line does that automatically.

A command that needs input can also ask for it instead: the bundled [`handoff`](../integrations/agent-skill.md#bundled-skills) skill asks "what should the new workspace implement?" when fired with no argument, and [`agent-review`](../integrations/agent-skill.md#bundled-skills) asks which reviewer kind to spawn.
