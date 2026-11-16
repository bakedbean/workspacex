# Mouse and text selection

wsx captures the mouse so the wheel scrolls the agent's history and parts of the screen are clickable. As a result, a plain click-and-drag no longer selects text.

## Selecting text

Hold `Shift` while you drag. Most terminals (Alacritty, Kitty, WezTerm, iTerm2, GNOME Terminal) pass a Shift-drag through to their own selection. On macOS, some terminals use `Option` instead. In iTerm2 you can also right-click and choose **Bypass mouse reporting**.

## Scrolling

- The wheel scrolls the attached pane's history. If the agent handles the mouse itself, the wheel goes to the agent; hold `Shift` to scroll wsx's history instead. Over the [side panel](side-panel.md#in-the-attached-view) it scrolls the panel.
- On the dashboard, the wheel over a [detail bar](detail-bar.md) container scrolls that container, and over the [side panel](side-panel.md) scrolls the panel. Elsewhere in the [focus view](repo-focus.md) it scrolls the workspaces.
- In the side panel's [diff viewer](side-panel.md#the-diff-viewer), the wheel scrolls the diff, and a click or drag on the minimap jumps there.

## Clickable targets

| Click                        | Does                                                     |
| ---------------------------- | -------------------------------------------------------- |
| A key hint in the footer     | Presses that key                                         |
| A status in the header tally (`? 2 question`) | [Filters the list](status-indicators.md#filtering-by-status) to that status; click it again, or press `esc`, to clear |
| A row's PR chip (`⏺ #123 open`) | Opens the PR in your browser                          |
| A repo header's PR link      | Opens your open PRs in that repo on GitHub               |
| A repo header's name         | Shows only that repo, in the [focus view](repo-focus.md) |
| A workspace in the focus view | Attaches to it                                          |
| `◂ all repos` in the focus view | Goes back to every repo                               |
| A [side-panel](side-panel.md) tab, on the dashboard or attached | Shows that tab        |
| A file in the side panel's Files tab | Opens its [diff](side-panel.md#the-diff-viewer) full screen |
| A row's process count (`● 2p`) | Opens the [process list](process-tracking.md)          |
| A pinned-command chip        | Sends that [pinned command](../attached-view/pinned-commands.md) |
| A prompt-tag chip            | Opens that [prompt tag](../attached-view/prompt-tags.md)'s body box |
| A workspace in the attached view's strip | Attaches to it                               |
| The strip's `+N more`        | Opens the updates panel                                  |
| An agent pill                | Switches the pane to that agent                          |
| A `$hosts` chip              | Opens that host's [shared workspaces](../integrations/shared-workspaces.md) |
| The footer activity graph    | Picks the graph's time window                            |

The repo PR link shows as `PR`, or as a pull-request icon with [`nerd_fonts`](../configuration/settings.md) on. It turns green when one of the repo's workspaces has an open PR. It appears only for repos whose `origin` is on github.com.

Opening PRs needs [`gh`](https://cli.github.com), installed and authenticated.
