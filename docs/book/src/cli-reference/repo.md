# wsx repo

Commands that register git repositories with wsx and set their per-repo settings. `<name>` is the repo's wsx name, as shown by `wsx repo list`. Values marked `<value-or-@file>` accept `@path` to read from a file and `""` to clear. The `edit-*` commands open the current value in `$EDITOR`.

## wsx repo add

```
wsx repo add <path> [--name <name>] [--prefix <prefix>]
```

Registers the git working tree at `<path>`.

| Flag | Effect |
| --- | --- |
| `--name <name>` | Name on the dashboard and in commands. Default: the directory name. |
| `--prefix <prefix>` | Branch prefix for this repo. Usually omit it and set the global `branch_prefix` instead. |

## wsx repo list

```
wsx repo list [--json]
```

Lists registered repos and their paths. `--json` prints `{name, path, branch_prefix}` objects.

## wsx repo remove

```
wsx repo remove <name>
```

Unregisters a repo and all its workspaces. Nothing is deleted on disk: the repo and its worktrees stay where they are.

## wsx repo set-name

```
wsx repo set-name <name> <new-name>
```

Renames the repo in wsx. Later commands, such as `wsx workspace create`, use the new name.

## wsx repo set-path

```
wsx repo set-path <name> <path>
```

Points the repo at a source checkout that moved, keeping its name, settings and workspaces. Use it instead of `remove` and `add`.

- New workspaces branch from the new path. Existing worktrees came from the old checkout, so recreate them if the old path is gone.

## wsx repo set-prefix

```
wsx repo set-prefix <name> <prefix>
```

Sets the repo's branch prefix. `""` clears it, falling back to the global `branch_prefix`.

## wsx repo set-base-branch

```
wsx repo set-base-branch <name> <ref>
```

Sets the ref new workspaces branch from, such as `origin/main`. wsx fetches it before each create. The dashboard's diff column also compares against it.

- `""` clears it. New workspaces then branch from the source checkout's current `HEAD`.

## wsx repo set-instructions

```
wsx repo set-instructions <name> <value-or-@file>
```

Sets instructions added to the agent's system prompt in this repo's workspaces, after the global `custom_instructions`.

## wsx repo set-setup

<a id="wsx-repo-edit-setup"></a>

```
wsx repo set-setup <name> <value-or-@file>
wsx repo edit-setup <name>
```

Sets the script that runs in each new worktree. See [Setup and archive scripts](../configuration/setup-scripts.md).

## wsx repo set-archive

<a id="wsx-repo-edit-archive"></a>

```
wsx repo set-archive <name> <value-or-@file>
wsx repo edit-archive <name>
```

Sets the script that runs before a worktree is archived. See [Setup and archive scripts](../configuration/setup-scripts.md).

## wsx repo set-pinned-commands

<a id="wsx-repo-edit-pinned-commands"></a>

```
wsx repo set-pinned-commands <name> <value-or-@file>
wsx repo edit-pinned-commands <name>
```

Overrides the global `pinned_commands` for this repo. `""` falls back to the global value. See [Pinned commands](../attached-view/pinned-commands.md).

## wsx repo set-related-repos

<a id="wsx-repo-edit-related-repos"></a>

```
wsx repo set-related-repos <name> <value-or-@file>
wsx repo edit-related-repos <name>
```

Sets a comma-separated list of other registered repos, such as `frontend,marketing`, that agents in this repo can also read. Unknown names are skipped. See [Related repos](../integrations/related-repos.md).
