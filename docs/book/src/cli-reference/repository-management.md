```
wsx repo add <path> [--name <name>] [--prefix <prefix>]
```

Registers a git repository. `<path>` must be an existing git working tree.

- `--name <name>` — display name on the dashboard. Defaults to the directory basename.
- `--prefix <prefix>` — per-repo branch prefix override. **Usually omit this** and use the global `branch_prefix` setting instead. Setting both means the per-repo value wins.

Where a `set-*` command below takes a value, it accepts `@/path/to/file` to load that value from a file and `""` to clear it (clearing falls back to the global setting where one exists).

```
wsx repo list
```

Lists registered repos with their paths.

```
wsx repo remove <name>
```

Removes a repo from the wsx registry. Does not delete the git repository on disk. Workspaces under the removed repo are also unregistered (but their worktrees remain on disk).

```
wsx repo set-name <name> <new-name>
```

Renames the repo in the wsx registry. The new name appears on the dashboard and is used in workspace references (e.g. `wsx workspace create <repo>`). Other commands like `wsx repo set-prefix <new-name> ...` must use the new name afterwards.

```
wsx repo set-prefix <name> <prefix>
```

Sets or changes the per-repo branch prefix override.

```
wsx repo set-instructions <name> <value-or-@file>
```

Sets per-repo custom instructions appended to claude's system prompt for sessions in this repo.

```
wsx repo set-pinned-commands <name> <value-or-@file>
wsx repo edit-pinned-commands <name>
```

Per-repo override of `pinned_commands`. Clearing falls back to the global setting.

```
wsx repo set-related-repos <name> <value-or-@file>
wsx repo edit-related-repos <name>
```

Per-repo list of other wsx-registered repos that workspaces in this repo should reference. Comma-separated names (e.g. `frontend,marketing`). At spawn time wsx looks each name up in the repo registry and passes `--add-dir <source-path>` to claude. Unknown names are silently skipped (logged at `warn` level — visible with `RUST_LOG=wsx=warn` or any less-specific filter).
