# Repo focus view

The focus view shows one repo on the dashboard and gives each of its workspaces several lines instead of one, laid out to be read at a glance. On a terminal at least 96 columns wide each workspace splits in two: its facts in a narrow column on the left, and what it's for and doing on the right, wrapped at a comfortable reading width instead of across the whole screen. Every other workspace has a faint background tint so each reads as one block.

```text
◂ all repos / workspacex  ~/src/workspacex  ──────────────────────────  ● 2  ? 1    5 ws

▎▍ ⠋ bakedbean/repo-focus-view          prompt  I want to design a new dashboard feature for how
 ▍   ⏺ #461 open ✓                  2m          workspaces are displayed. I want to be able to…
 ▍   ▸ working  ● 2p                    goal    Single-repo dashboard view with richer
 ▍   ↑4 base · 3 modified                       per-workspace detail
 ▍   1 untracked · +184 −22             now     mapping dashboard code → draft design options
 ▍    claude   codex
 ▍   #ui

▎▎ ? bakedbean/detail-bar-scroll        prompt  The detail bar containers should scroll
 ▎   ⏺ #459 draft                   14m         independently
 ▎   └ asking  ● 1p                     goal    Scroll detail-bar containers independently
 ▎   ↑2 base · clean · +61 −8           now     keep scroll offsets per workspace or global?
 ▎    claude
```

The left column, top to bottom:

- **The branch**, after the status glyph (a spinner while the agent works). A branch longer than the column wraps onto the next line; it is never shortened.
- **The PR chip** and how long ago the workspace was active.
- **The status word**, as on the dashboard row, with the running-process count and any badge.
- **Git**: commits ahead of the base branch, uncommitted changes, and the diff.
- **Agents**: every agent in the workspace, named with its icon from the theme's [`[agent_bar.symbols]`](../configuration/themes.md) table, or the `▎` bar when the theme has none.
- **Tags**.

The right column:

- **prompt**: the first prompt the workspace's agent was given, in italics and cut to two lines, so you can see what the workspace was started for. It appears once the agent's session log has been read.
- **goal**: the recap goal in full.
- **now**: the recap state, then the next step after `→`. Without a recap, a **last** line takes its place: the status text the dashboard row would have clipped, usually the agent's last message, cut to two lines. When that text is just the initial prompt, it isn't repeated.

Below 96 columns there's no room for two columns, so each workspace stacks instead: the dashboard row (its branch column widened to fit the longest branch, up to 80 cells), the prose lines under it, and a `git` line with the git counts, agents and tags.

In the focus view the [detail bar](detail-bar.md) shrinks to its reply row, so the workspaces get nearly the full height. Press `Tab` to type a message to the selected workspace's agent, as you would on the list.

## Opening and closing it

- Press `f` on a repo header, or on any workspace to focus that workspace's repo. Click a repo header's name to do the same.
- Press `f` again, press `esc`, or click `◂ all repos` to go back to every repo. While a `/` filter or status filter is set, `esc` clears that first.
- `[` and `]` move to the previous and next repo in the dashboard's repo order.

## Moving around

The focus view moves like the list: `j` / `k` (or the arrows) select the next or previous workspace, and `enter`, `i` or `l` attaches. Clicking anywhere on a workspace attaches to it, and clicking its PR chip opens the PR. `n` creates a workspace in the focused repo.

The view keeps the selected workspace on screen. To read a workspace taller than the screen, page with `PgDn` / `PgUp` or scroll with the mouse wheel; the view stays put until you select a different workspace. The other workspace keys (`d`, `e`, `#`, `?`, and so on) act on the selected workspace as usual.

## Filters and hiding

The `/` filter and the [status filter](status-indicators.md#filtering-by-status) narrow the view the same way they narrow the list. Workspaces a [hide rule](tags-and-hiding.md#hiding-workspaces) hides collapse into a summary row per rule after the other workspaces (`▸ #parked  ·  2 hidden`), as they do under the repo's header on the list. `enter` or `l` on the row, or a click, shows its workspaces under it; `h` on the row or on one of them hides them again. A group left open in one view is open in the other. A `/` filter shows hidden workspaces among the others, as it does on the list.

The focus view lasts until you leave it or quit wsx. Pressing `G` leaves it, since grouping has nothing to act on in a single repo.
