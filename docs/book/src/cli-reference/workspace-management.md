```
wsx workspace create <repo> [--name <slug>] [--yolo] [--agent claude|pi|hermes|codex] [--prompt <text>]
```

Creates a workspace in `<repo>`, equivalent to the dashboard's `[n]` keybind. `<slug>` is a kebab-case workspace name; the resulting git branch is `<branch_prefix>/<slug>`. When `--name` is omitted, an adjective-noun slug like `merry-birch` is generated. `--yolo` skips the permission prompts in the spawned agent session. `--agent` overrides the `coding_agent` setting (see [Coding agents](../configuration/coding-agents.md)) for this workspace; when omitted, the setting applies (claude unless configured otherwise).

`--prompt` seeds the new workspace's agent with a starting task, equivalent to running [`wsx agent send`](../configuration/multi-agent-workspaces.md) against it immediately afterward. Like any queued message it is delivered by the dashboard, which spawns the agent on demand — so a workspace created with `--prompt` while no dashboard is running stays idle, and the command says so on stderr.

When `create` runs from inside a workspace — an agent handing work off, or a shell in a worktree — the new workspace inherits that workspace's yolo mode and agent kind: yolo is on if `--yolo` is passed *or* the parent is yolo, and the agent is `--agent` if passed, else the parent's agent, else the `coding_agent` setting. The command prints what it inherited and from where. Creates from outside any workspace fall back to the flags and settings alone.

## Starting work from a phone

`--prompt` exists so a whole workspace can be started over SSH in one line, with the prompt as the only thing typed:

```
wsx workspace create backend --prompt "Fix the flaky input PTY tests"
```

Omitting `--name` is deliberate here: the workspace gets a placeholder slug, and the agent renames both it and the git branch from the prompt on its first turn (see [Auto-rename modes](../configuration/auto-rename-modes.md)).

Two existing behaviors make this self-sufficient. Claude sessions are spawned with `--remote-control` by default (see [Remote control](../integrations/remote-control.md)), so the new session appears in the Claude app without any URL to copy off the terminal. And agent sessions run under `tmux`, so the session survives your SSH connection dropping — back at a real terminal, the dashboard reattaches to it with full scrollback.

This assumes a wsx dashboard is already running on the target machine, since nothing else delivers the prompt. Leaving `wsx` running in a tmux session is the usual arrangement.

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
