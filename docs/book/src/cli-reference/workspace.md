# wsx workspace

Commands that create, inspect and retire workspaces. Each workspace is a git worktree on its own branch, with one or more agents.

## wsx workspace create

```
wsx workspace create <repo> [--name <slug>] [--yolo] [--shared] [--start [--json]] [--agent <kind>] [--prompt <text>]
```

Creates a branch and worktree in `<repo>`, like `n` on the dashboard.

| Flag | Effect |
| --- | --- |
| `--name <slug>` | Workspace name. The branch is `<branch_prefix>/<slug>`. Default: a generated name like `merry-birch`. |
| `--yolo` | Skip permission prompts in the agent session. |
| `--shared` | Run the workspace's agents under tmux. See [Shared workspaces](../integrations/shared-workspaces.md). |
| `--start` | Have the running dashboard start the agents in tmux, wait until they're live, and print the sessions to attach to. Implies `--shared`. |
| `--json` | With `--start` only: print the workspace record on stdout, even when the start fails. |
| `--agent <kind>` | `claude`, `pi`, `hermes`, `codex` or `omp`. Overrides the `coding_agent` setting. See [Coding agents](../agents/coding-agents.md). |
| `--prompt <text>` | Queue a first task for the primary agent, like an immediate [`wsx agent send`](agent.md#wsx-agent-send). |

- Run from inside a workspace, `create` inherits that workspace's yolo mode and agent kind unless you pass `--yolo` or `--agent`. It prints what it inherited.
- wsx refuses a slug that git can't use in a branch name, or whose branch already exists or clashes with an existing branch path (`wsx/foo` blocks `wsx/foo/bar`).
- A failing setup script prints a warning and its log path. The workspace is still created.
- `--start` and `--prompt` need a running dashboard. Without one, `--start` fails after creating the workspace, and a `--prompt` stays queued until a dashboard starts.

Starting work from a phone over SSH, with a dashboard already running in tmux on the host:

```
wsx workspace create backend --prompt "Fix the flaky input PTY tests"
```

Without `--name`, the agent renames the workspace and branch from the prompt. See [Auto-rename](../configuration/auto-rename.md).

## wsx workspace list

```
wsx workspace list [<repo>] [--json]
```

Lists workspaces as tab-separated `repo  slug  branch  worktree_path` rows, optionally for one repo.

`--json` prints one object per workspace with everything its dashboard row shows:

```json
{
  "repo": "backend",
  "slug": "add-widgets",
  "branch": "eg/add-widgets",
  "path": "/home/me/.local/state/wsx/worktrees/backend/add-widgets",
  "status": {"state": "working", "message": "running tests", "source": "model", "reported_at": 1790342366562},
  "recap": {"goal_short": "POST /widgets", "state_short": "tests failing", "next_short": "fix validation"},
  "agents": [{"id": 12, "label": "claude", "kind": "claude", "primary": true, "status": {"state": "working", "...": "..."}}],
  "pr": {"state": "open", "number": 241, "url": "https://github.com/acme/backend/pull/241", "review": "approved", "unresolved": 0, "fetched_at": 1790342836000},
  "tags": ["spike"]
}
```

- `status` is the workspace's combined status. See [Status and recap](../agents/status-and-recap.md).
- `pr` is the last PR state the dashboard or a desktop widget cached. `list` makes no network calls. `pr.state` is one of `no_pr`, `draft`, `open`, `conflicted`, `merged` or `closed`.
- A field that was never set is `null`.

## wsx workspace path

```
wsx workspace path <repo> <slug>
```

Prints the workspace's worktree path, for `cd "$(wsx workspace path backend my-slug)"`.

## wsx workspace rename

```
wsx workspace rename <repo> <old-slug> <new-slug>
```

Renames the workspace and its git branch together. Use this instead of `git branch -m`, which leaves wsx out of sync.

- The worktree directory doesn't move. It keeps the old slug, and the command prints the unchanged path.

## wsx workspace archive

```
wsx workspace archive <repo> <slug> [--keep-worktree] [--force-delete-branch]
```

Retires a workspace, like the dashboard's archive action. In order, it:

1. stops the workspace's tmux agent sessions;
2. stops [tracked processes](../dashboard/process-tracking.md) under the worktree (`SIGTERM`, then `SIGKILL` after two seconds), unless `--keep-worktree`;
3. runs the repo's [archive script](../configuration/setup-scripts.md);
4. force-removes the worktree, unless `--keep-worktree`. **Uncommitted changes are lost.**
5. deletes the branch, then unregisters the workspace.

| Flag | Effect |
| --- | --- |
| `--keep-worktree` | Keep the worktree directory and leave its processes running. |
| `--force-delete-branch` | Delete the branch even if it isn't merged. Without it, an unmerged branch is kept. |

## wsx workspace share

```
wsx workspace share <repo> <slug> [--restart [--json]]
```

Switches a workspace to tmux-backed agent sessions, so other terminals or machines can attach. See [Shared workspaces](../integrations/shared-workspaces.md).

| Flag | Effect |
| --- | --- |
| `--restart` | Have the running dashboard restart the agents in tmux now, and print the sessions to attach to. |
| `--json` | With `--restart` only: print the workspace record as JSON. |

- Without `--restart`, running sessions keep their current backend until they restart.

## wsx workspace unshare

```
wsx workspace unshare <repo> <slug>
```

Switches a workspace back to direct (non-tmux) agent sessions. Running sessions change on their next restart.

## wsx workspace tag

```
wsx workspace tag add|rm <tag>... [--workspace <repo>/<slug>]
wsx workspace tag list [--workspace <repo>/<slug> | --all] [--json]
```

Adds, removes or lists [workspace tags](../dashboard/tags-and-hiding.md) on the current workspace, then prints its tags.

| Flag | Effect |
| --- | --- |
| `--workspace <repo>/<slug>` | Act on another workspace. |
| `--all` | `list` only: print `tag<TAB>count` for every tag in use. |
| `--json` | `list` only: print an array of tags, or `{"tag", "workspaces"}` objects with `--all`. |

- Tags are lowercased. Each must be 1–32 characters of `a-z 0-9 - _`, starting with a letter or digit. If any tag is invalid, nothing is written.
