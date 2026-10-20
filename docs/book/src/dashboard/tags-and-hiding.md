# Tags and hiding

Tags are short labels on workspaces, like `parked` or `spike`. Use them, and each PR's state, to search the dashboard and to hide workspaces you're not working on.

## Tagging workspaces

Press `#` on a workspace, or `Ctrl-x #` while attached to one, to open the tag editor:

| Key                 | Action                                  |
| ------------------- | --------------------------------------- |
| type + `enter`      | Add the tag                             |
| arrows + `space`    | Toggle a listed tag                     |
| `esc`               | Close                                   |

Typing filters the list, which offers every tag used in wsx. Changes save as you make them.

Tag rules:

- Lowercase `a-z`, `0-9`, `-`, and `_`, starting with a letter or digit.
- Up to 32 characters.

From a shell or an agent:

```bash
wsx workspace tag add parked
wsx workspace tag rm parked --workspace backend/old-spike
wsx workspace tag list --all
```

See [`wsx workspace tag`](../cli-reference/workspace.md#wsx-workspace-tag) for the full syntax.

Tags show dimmed after the branch name: `⎇ wsx/my-slug #parked #ui`. They use only the space left in the branch column, and tags that don't fit collapse to a count like `+2`. To make room, widen the column with the `dashboard_branch_width` [setting](../configuration/settings.md). Attached to a workspace, its tags show after its name in the top bar.

Workspace tags are unrelated to [prompt tags](../attached-view/prompt-tags.md).

## Hiding workspaces

Press `F` on a repo header, or any row in the repo, to choose what that repo hides. Move with `j`/`k` and toggle with `space`. wsx saves the choice per repo.

Hidden workspaces collapse into one dimmed row per rule:

```
▸ ⏺ merged  ·  4 hidden
▸ #parked  ·  2 hidden
```

Press `enter` or `l` on that row to show its workspaces, and `h` to collapse it. Expanded rows stay open until wsx restarts.

### By PR state

The states are no PR, draft, open, conflicted, merged, and closed. Each hides every matching workspace, except no PR, which keeps a workspace visible when any of these is true:

- It has commits ahead of its base branch, or uncommitted changes.
- An agent session is running, or its agent reports working, waiting, or blocked.
- It was created, or its agent was active, in the last 3 days.

A workspace whose PR state hasn't loaded yet stays visible.

### By tag

The `F` picker lists every tag in use above the PR states. A tag rule hides every workspace with that tag, even one with work in progress. A workspace matching a tag rule and a PR rule is grouped under the tag. A rule for a tag no longer in use stays in the picker with a count of 0, so you can turn it off.

### What still counts hidden workspaces

- A `/` search covers every workspace, hidden or not.
- Status bar, Waybar, and menu bar counts include hidden workspaces.
- The updates panel (`Ctrl-x u`) leaves them out.
- The by-attention view (`G`) lists them in a HIDDEN section.

If the selected workspace becomes hidden, for example when its PR merges, the selection moves to its group's row.

## Searching

Press `/` and type. The filter matches branch names, repo names, tags, status words, and recaps, ignoring case. Prefix a tag with `#` to match only that exact tag:

```
/parked      anything containing "parked"
/#parked     tagged exactly "parked"
```

`esc` clears the filter.
