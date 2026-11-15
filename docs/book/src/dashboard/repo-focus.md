# Repo focus view

The focus view shows one repo on the dashboard and gives each of its workspaces several lines instead of one. Nothing is cut short: the branch column widens to fit the longest branch name, and the recap goal and state wrap onto as many lines as they need.

```text
◂ all repos / workspacex  ~/src/workspacex  ──────────────────────────  ● 2  ? 1    5 ws

▎▍├  ⠋ ⎇ bakedbean/repo-focus-view          ⏺ #461 open ✓   ● 2p  +184 −22    ▸ working   2m ago
 ▍  goal  Single-repo dashboard view with richer per-workspace detail, wrapping every recap
 ▍        field across as many lines as it needs
 ▍  now   mapping dashboard code → draft design options
 ▍  git   ↑4 base · 3 modified 1 untracked · +184 −22 · ▎ claude  ▎ codex   ui

▎▎├  ? ⎇ bakedbean/detail-bar-scroll        ⏺ #459 draft     ● 1p  +61 −8      └ asking   14m ago
 ▎  goal  Scroll detail-bar containers independently
 ▎  now   keep scroll offsets per workspace or global?
 ▎  git   ↑2 base · clean · +61 −8 · ▎ claude
```

Each workspace shows:

- **The dashboard row**, with the status word but without the squeezed recap.
- **goal**: the recap goal in full.
- **now**: the recap state, then the next step after `→`. Without a recap, this line shows the status text the dashboard row would have clipped.
- **git**: commits ahead of the base branch, uncommitted changes, the diff, and every agent in the workspace, followed by its tags. Each agent is named with its icon from the theme's [`[agent_bar.symbols]`](../configuration/themes.md) table, or the `▎` bar when the theme has none.

The [detail bar](detail-bar.md) is not shown in the focus view, so the rows get the full height.

## Opening and closing it

- Press `f` on a repo header, or on any workspace to focus that workspace's repo. Click a repo header's name to do the same.
- Press `f` again, press `esc`, or click `◂ all repos` to go back to every repo.
- `[` and `]` move to the previous and next repo in the dashboard's repo order.

## Moving around

The focus view moves like the list: `j` / `k` (or the arrows) select the next or previous workspace, and `enter`, `i` or `l` attaches. Clicking anywhere on a workspace attaches to it, and clicking its PR chip opens the PR. `n` creates a workspace in the focused repo. The other workspace keys (`d`, `e`, `#`, `?`, and so on) act on the selected workspace as usual.

## Filters and hiding

The `/` filter and the [status filter](status-indicators.md#filtering-by-status) narrow the view the same way they narrow the list. Workspaces a [hide rule](tags-and-hiding.md#hiding-workspaces) hides are left out; the header counts them as `+N hidden`. A `/` filter shows them again, as it does on the list.

The focus view lasts until you leave it or quit wsx. Pressing `G` leaves it, since grouping has nothing to act on in a single repo.
