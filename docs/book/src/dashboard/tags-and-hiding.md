# Workspace tags

Tags are short labels you put on workspaces, like `parked` or `spike`. They
show on the dashboard row, narrow the `/` filter, and can
[hide workspaces](tags-and-hiding.md#by-tag).

## Adding and removing tags

Press `#` on a workspace (or `#` on its `?` card) to open the tag editor.
Type a tag and press `enter` to add it, or move to a listed tag with the
arrow keys and press `space` to toggle it. Typing filters the list; once you
arrow to a row, `space` or `enter` acts on that row instead of the typed
text. The list offers every tag used anywhere in wsx. `esc`, or `enter` with nothing typed, closes the editor.
Changes save as you make them.

Tags are lowercased and may use `a-z`, `0-9`, `-` and `_`, up to 32
characters. They must start with a letter or digit.

From a shell or an agent:

    wsx workspace tag add parked
    wsx workspace tag rm parked --workspace backend/old-spike
    wsx workspace tag list --all

`add` and `rm` act on the current workspace unless `--workspace
<repo>/<slug>` is given, and print its tags afterwards. `list --all` prints
every tag in use with how many workspaces carry it. `wsx workspace list
--json` includes each workspace's `tags`.

## On the dashboard

Tags appear dimmed after the branch name: `⎇ wsx/my-slug #parked #ui`. They
only use the room left in the branch column, so the name is never cut for
them; tags that don't fit collapse to a `+2`-style count. If your branch
names fill the column, widen it with
`wsx config set dashboard_branch_width 40` (see
[Global settings](../configuration/settings.md)).

## Searching

`/parked` matches tags along with names and recaps. `/#parked` matches only
workspaces tagged exactly `parked`.

Workspace tags are unrelated to [prompt tags](../attached-view/prompt-tags.md).

# Hiding workspaces

Long-lived workspaces can crowd a repo's section once their PRs merge or if
they never get one. Each repo can hide workspaces by PR state or by
[tag](tags-and-hiding.md).

Press `F` on a repo header (or any of its rows) and toggle states with
`space`: no PR, draft, open, conflicted, merged, closed. The choice is saved
per repo.

"No PR" only hides workspaces that look abandoned. A workspace without a PR
stays visible if any of these is true:

- it has commits ahead of its base branch, or uncommitted changes;
- an agent session is running, or its agent reports working, waiting, or
  blocked;
- it was created, or its agent was active, in the last 3 days.

The other states hide every matching workspace.

Hidden workspaces collapse into one dimmed row per state under their repo:

    ▸ ⏺ merged  ·  4 hidden

Press `enter` or `l` on that row to show its workspaces inline; `h` collapses
it again. Expansion lasts until wsx restarts.

A few things stay predictable:

- Typing a `/` filter searches every workspace, hidden or not.
- A workspace whose PR state hasn't loaded yet is never hidden.
- If a selected workspace becomes hidden (say its PR merges), the selection
  moves to its group's row.
- Status bar, waybar, and menubar counts still include hidden workspaces.
- The updates panel (`Ctrl-x u`) leaves hidden workspaces out.
- The by-attention view (`G`) lists hidden groups in a HIDDEN section.

## By tag

The `F` picker also lists every [workspace tag](tags-and-hiding.md) in use,
above the PR states. Turning one on hides that repo's workspaces carrying
the tag, whatever they're doing; there is no exemption for work in progress.

    ▸ #parked  ·  2 hidden

A workspace matching both a tag rule and a PR rule is grouped under the
tag. A rule for a tag no workspace carries any more stays in the picker,
with a count of 0, so you can turn it off.
