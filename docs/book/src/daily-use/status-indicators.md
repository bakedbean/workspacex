| Symbol                 | Meaning                                                                         |
| ---------------------- | ------------------------------------------------------------------------------- |
| `●`                    | Session is running in this wsx process                                          |
| `◆`                    | Detached — a shared workspace's tmux session is alive on the server but has no client attached in this wsx process (normal right after a wsx restart; attach to reconnect) |
| `↻`                    | Resumable — a prior claude session exists for this worktree; attach to continue |
| `○`                    | No session ever started here                                                    |
| `✕`                    | Workspace state is `Failed` (worktree creation didn't succeed)                  |
| `[setup-failed]` badge | Setup script exited non-zero; workspace is otherwise usable                     |

Activity column for running sessions:

- `active` — output within the last 2 seconds
- `idle` — output within the last 30 seconds
- `waiting` — no output for over 30 seconds
- `off` — no current session
- `resumable` — prior session exists, not currently running

### Activity sub-line

Below each workspace row, wsx shows the most recent event from claude's
session log (tailed from `~/.claude/projects/<encoded-cwd>/`):

```
  ● fix-bug    bakedbean/fix-bug   ~3 ?1   active
    └ ran `cargo test --workspace` (3s ago)
```

The sub-line updates on the 2-second poll tick. Workspaces with no
claude session yet show no sub-line. Recognized events:

- User message → `user: <text>`
- Assistant text → `<text>`
- Assistant tool use (Bash) → ``ran `<command>` ``
- Assistant tool use (other) → `using <ToolName>`

Lines longer than ~70 characters are truncated with an ellipsis.

### Diff counts column

Compact summary of `git status` per workspace, refreshed every 2 seconds:

| Symbol (plain) | Symbol (nerd) | Meaning                                     |
| -------------- | ------------- | ------------------------------------------- |
| `~N`           | `N`           | Modified/staged/added/deleted tracked files |
| `?N`           | `N`           | Untracked files                             |
| `↑N`           | `N`           | Commits ahead of upstream                   |
| `↓N`           | `N`           | Commits behind upstream                     |

Zero values omitted. Clean workspaces show nothing in this column.

### Attention alerts

wsx watches each workspace for two distinct "user needs to act" signals:

- A `tool_use` event in the session log has been pending for ≥3 seconds —
  almost always means claude is showing a permission prompt for a tool.
  In this case the activity column reads `awaiting` and the sub-line shows
  `└ ⚠ awaiting permission: <tool> (<age>)`.
- The claude session has gone ≥30 seconds without producing PTY output
  (state flips from `active` or `idle` to `waiting`).

On either transition wsx considers the workspace to need attention:

- A terminal bell (`\x07`) is written to stdout. Your terminal config decides
  whether to beep, flash, or ignore.
- A `!` marker appears at the start of the workspace's row on the dashboard.

The marker clears the moment you attach to the workspace (Enter on the row).
The first observation of any workspace establishes a baseline; no bell rings
for workspaces that are already in `waiting` or `awaiting` state when wsx
launches.

Turn off both via `wsx config set notifications off`.
