# Updates panel and split panes

While you're attached to one workspace, wsx keeps the others in view. Use the top-bar strip and the updates panel to jump between workspaces, and split the view to watch several agents at once.

## Workspace strip

The top bar lists every other workspace after the focused one's label, as `<glyph> <repo>/<name> (<age>)`:

- The glyph and color match the workspace's dashboard row.
- Workspaces that need attention come first. The rest follow the dashboard's order.
- Workspaces the dashboard [hides](../dashboard/tags-and-hiding.md#hiding-workspaces) are left out, even ones that need attention. To keep a hidden tag in the strip, list it in the `top_bar_show_tags` [setting](../configuration/settings.md). A workspace with a listed tag ignores every tag rule, but PR rules still hide it.
- Click an entry to switch to that workspace.
- When the strip runs out of room it ends in `… +N more`. Click it to open the updates panel.

## Updates panel

Press `Ctrl-x u` to open the updates panel, a compact dashboard in the bottom drawer. It lists every visible workspace in the dashboard's order and grouping, with nothing folded. Hidden workspaces fold into the dashboard's group rows: press `l` on a group to list its workspaces, `h` to fold it again, or `enter` to toggle it. A `/` search covers hidden workspaces too. Each row shows the workspace's state, latest event, PR chip, and `+N −N` diff. It updates live.

| Key                 | Action                                                         |
| ------------------- | -------------------------------------------------------------- |
| `↑` / `↓`, `k` / `j` | Move the selection (wraps at the ends)                        |
| `Enter` or `l`      | Switch the current pane to the selected workspace              |
| `l` / `h`           | Expand / fold a hidden group (`Enter` toggles it)              |
| `v`                 | Open the selected workspace in a vertical split (side by side) |
| `s`                 | Open the selected workspace in a horizontal split (stacked)    |
| `o`                 | Cycle the sort mode (recency ↔ status), same as the dashboard  |
| `G`                 | Toggle grouping (by repo ↔ by attention)                       |
| `/`                 | Filter by workspace name, repo, or status text                 |
| `Esc`               | Clear the filter, or close the panel                           |

While a filter is active, letter keys type into the filter. Use the arrow keys and `Enter` to pick a row.

## Split panes

You can tile several workspaces in the attached view, vim-style. Any pane can split again, so you can build any tree of vertical and horizontal splits. Each pane has a one-line title bar, and `●` marks the focused pane, which receives your keystrokes.

1. Attach to a workspace and press `Ctrl-x u`.
2. Select another workspace and press `v` or `s`. Focus moves to the new pane.
3. Repeat to add more panes.

| Key                   | Action                                                       |
| --------------------- | ------------------------------------------------------------ |
| `Ctrl-x ←/→/↑/↓`      | Move focus to the neighboring pane                           |
| `Ctrl-x d`            | Close the focused pane. Closing the last pane detaches       |
| `Ctrl-x Shift-D`      | Save the layout and detach to the dashboard                  |

Other panes keep running when you close one.

## Saving a layout

`Ctrl-x d` doesn't remember the arrangement. Press `Ctrl-x Shift-D` to save the split tree and the focused pane against the first workspace you attached to, then detach.

- The next time you attach to that workspace, wsx restores the layout and restarts the other panes' sessions.
- Panes whose workspaces no longer exist are dropped. If none remain, you get a single pane.
- A workspace with a saved layout shows a columns glyph next to its branch on the dashboard (Nerd Fonts only).
