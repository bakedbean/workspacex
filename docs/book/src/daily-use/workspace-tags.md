# Workspace tags

Tags are short labels you put on workspaces, like `parked` or `spike`. They
show on the dashboard row, narrow the `/` filter, and can
[hide workspaces](hiding-workspaces.md#by-tag).

## Adding and removing tags

Press `#` on a workspace (or `#` on its `?` card) to open the tag editor.
Type a tag and press `enter` to add it, or move to a listed tag with the
arrow keys and press `space` to toggle it. The list offers every tag used
anywhere in wsx. `esc`, or `enter` with nothing typed, closes the editor.
Changes save as you make them.

Tags are lowercased and may use `a-z`, `0-9`, `-` and `_`, up to 32
characters.

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
[Global settings](../configuration/global-settings.md)).

## Searching

`/parked` matches tags along with names and recaps. `/#parked` matches only
workspaces tagged exactly `parked`.

Workspace tags are unrelated to [prompt tags](prompt-tags.md).
