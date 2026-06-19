```
wsx workspace create <repo> [--name <slug>] [--yolo] [--agent claude|pi|hermes|codex]
```

Creates a workspace in `<repo>`, equivalent to the dashboard's `[n]` keybind. `<slug>` is a kebab-case workspace name; the resulting git branch is `<branch_prefix>/<slug>`. When `--name` is omitted, an adjective-noun slug like `merry-birch` is generated. `--yolo` skips the permission prompts in the spawned agent session. `--agent` overrides the `coding_agent` setting (see [Coding agents](../configuration/coding-agents.md)) for this workspace. Default: `claude`.

```
wsx workspace list [<repo>]
```

Lists workspaces as tab-separated `repo<TAB>slug<TAB>branch<TAB>worktree_path` rows. Pass a repo name to filter.

```
wsx workspace path <repo> <slug>
```

Prints just the worktree path. Designed for `cd "$(wsx workspace path backend my-slug)"`.

```
wsx workspace rename <repo> <old-slug> <new-slug>
```

Renames the workspace slug AND its git branch in sync with the wsx database. Using `git branch -m` directly leaves wsx's DB stale.

```
wsx workspace archive <repo> <slug> [--keep-worktree] [--force-delete-branch]
```

Equivalent to the dashboard's archive action: runs the per-repo archive script, removes the worktree (unless `--keep-worktree`), deletes the branch (force if `--force-delete-branch`), and drops the workspace from the registry.
