# Repo focus view

The focus view shows one repo on the dashboard and gives each of its workspaces several lines instead of one, laid out to be read at a glance. Workspaces are grouped into sections by pull request state, each row starts with a strip of fixed columns that line up down the view, and what the workspace is for and doing sits under its branch. Every other workspace has a faint background tint so each reads as one block.

```text
◂ all repos / workspacex  ~/src/workspacex  ───────────────────────────────────────────────────────────  ● 1  ? 1    4 ws

── No PR · 1 ───────────────────────────────────────────────────────────────────────────────────────────────────────────

▎▎ ⠋ bakedbean/try-new-parser     no PR          ▸ working  ● 2p  ↑1 vs main                           2m
 ▎   prompt  Spike a faster parser for agent recaps; measure it against the current one on a few
 ▎           real session logs before changing anything.
 ▎   goal    Spike a faster recap parser
 ▎   now     benchmarking the streaming approach → compare against the regex version
 ▎   ▎ claude  #spike

── Draft · 1 ───────────────────────────────────────────────────────────────────────────────────────────────────────────

▎▎ ? bakedbean/detail-bar-scroll  ⏷ #459 draft   └ asking         ↑2 vs main                +61 −8    14m
 ▎   prompt  The detail bar containers should scroll independently
 ▎   last    Should the scroll offsets be kept per workspace or shared across the whole detail
 ▎           bar?
 ▎   ▎ claude

── Open · 1 ────────────────────────────────────────────────────────────────────────────────────────────────────────────

▎▎ · bakedbean/repo-focus-view    ⏺ #461 open ✓  ▸ idle           ↑4 vs main  3 mod 1 untr  +184 −22   3h
 ▎   goal    Single-repo dashboard view with richer per-workspace detail
 ▎   now     implemented sections and the strip → write the docs
 ▎   ▎ claude  #ui

── Done · 1 ────────────────────────────────────────────────────────────────────────────────────────────────────────────

▎▎ ✓ bakedbean/fix-flaky-poller   ⏺ #455 merged  ▸ done           ↑0 vs main                          48h
 ▎   ▎ claude
```

## Sections

A header line (`── Open · 2 ───…`, with the workspace count) starts each section:

| Section | Holds |
| --- | --- |
| **No PR** | Workspaces with no PR, or whose PR state isn't known yet |
| **Draft** | Draft PRs |
| **Open** | Open PRs, including those with merge conflicts |
| **Done** | Merged and closed PRs |

Sections with no workspaces are left out. Within a section the dashboard's sort applies, but the focus view doesn't pin workspaces that just started asking or stalled to the top, as the list does; the sections already order the view by PR state. Hidden groups (see [Filters and hiding](#filters-and-hiding)) come after all the sections, as they are on the list. Headers are not selectable: `j` and `k` step over them.

## The row

Each row has three zones.

**The strip** is line 1: columns that are as wide as the widest value in the view, so each field lines up down the screen. Left to right:

- the agent strip and the status glyph (a spinner while the agent works),
- the **branch**, which wraps onto the next line when it is too long and is never shortened,
- the **PR chip**, as on the dashboard list's PR column: number, state, and the review mark with its unresolved-thread count. A workspace without a PR shows a dim `no PR`,
- the **status word**, as on the dashboard row, with the running-process count and any badge,
- commits ahead of the repo's base branch (`↑4 vs main`, from the repo's **Base branch** setting; `↑4 base` when it isn't set),
- uncommitted changes (`3 mod 1 untr`; nothing when the tree is clean),
- the diff, and how long ago the workspace was active.

**The prose** sits under the branch:

- **prompt**: the first prompt the workspace's agent was given, in italics and cut to two lines, so you can see what the workspace was started for. It appears once the agent's session log has been read.
- **goal**: the recap goal in full.
- **now**: the recap state, then the next step after `→`. Without a recap, a **last** line takes its place: the status text the dashboard row would have clipped, usually the agent's last message, cut to two lines. When that text is just the initial prompt, it isn't repeated.

The prose is wrapped at a comfortable reading width instead of across the whole screen.

**The footer** is a dim line of the workspace's agents, each named with its icon from the theme's [`[agent_bar.symbols]`](../configuration/themes.md) table (or the `▎` bar when the theme has none), followed by its tags. It is always shown.

When the strip doesn't fit on one line, it breaks in two. Line 1 keeps the glyph, branch, PR chip and age; the status, commits ahead, uncommitted changes and diff move to line 2, under the branch. Every row breaks at the same point, so the columns still line up.

In the focus view the [detail bar](detail-bar.md) shrinks to its reply row, so the workspaces get nearly the full height. Press `Tab` to type a message to the selected workspace's agent, as you would on the list.

## Side panel

Press `c` to open the [side panel](side-panel.md) on the right: the selected workspace's commits and changed files against the repo's base branch. It works here as it does on the list; `esc` closes it before it leaves the focus view.

## Opening and closing it

- Press `f` on a repo header, or on any workspace to focus that workspace's repo. Click a repo header's name to do the same.
- Press `f` again, press `esc`, or click `◂ all repos` to go back to every repo. While a `/` filter or status filter is set, `esc` clears that first.
- `[` and `]` move to the previous and next repo in the dashboard's repo order.

## Moving around

The focus view moves like the list: `j` / `k` (or the arrows) select the next or previous workspace, and `enter`, `i` or `l` attaches. `}` jumps to the first workspace of the next section and `{` to the previous one's. Pressed in the middle of a section, `{` goes to the top of the current section first. At the first and last section they do nothing. Hidden groups count as a section of their own. A jump, or `j` / `k` onto a section's first workspace, scrolls its header into view. Clicking anywhere on a workspace attaches to it, and clicking its PR chip opens the PR. `n` creates a workspace in the focused repo.

The view keeps the selected workspace on screen. To read a workspace taller than the screen, page with `PgDn` / `PgUp` or scroll with the mouse wheel; the view stays put until you select a different workspace. The other workspace keys (`d`, `e`, `#`, `?`, and so on) act on the selected workspace as usual.

## Filters and hiding

The `/` filter and the [status filter](status-indicators.md#filtering-by-status) narrow the view the same way they narrow the list. Workspaces a [hide rule](tags-and-hiding.md#hiding-workspaces) hides collapse into a summary row per rule after the other workspaces (`▸ #parked  ·  2 hidden`), as they do under the repo's header on the list, rather than being sorted into the sections. `enter` or `l` on the row, or a click, shows its workspaces under it; `h` on the row or on one of them hides them again. A group left open in one view is open in the other. A `/` filter shows hidden workspaces among the others, as it does on the list.

The focus view lasts until you leave it or quit wsx. Pressing `G` leaves it, since grouping has nothing to act on in a single repo.
