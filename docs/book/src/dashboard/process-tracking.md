# Process tracking

wsx lists the long-running processes working inside each workspace's worktree, such as dev servers and watchers. Use it to find and stop what a workspace left running, or to start a command there without opening a terminal.

A workspace with tracked processes shows a count like `● 2p` on its dashboard row. Press `K` on the row, `Ctrl-x k` while attached, or click the count to open the list:

```
  PID    COMMAND          CWD
  12345  npm              /home/user/wt/fix-bug
  12389  pytest           /home/user/wt/fix-bug/tests
[↑/↓] move   [r] run   [k] term   [K] kill   [esc] close
```

| Key        | Action                                        |
| ---------- | --------------------------------------------- |
| `↑` / `↓`  | Move the selection                            |
| `k`        | Send `SIGTERM` to the process                 |
| `K`        | Send `SIGKILL` to the process                 |
| `r`        | Run a command in the worktree                 |
| `esc`      | Close                                         |

`j` and `k` don't move the selection here, because `k` stops a process.

## Running a command

`r` runs a command with `sh -c` in the worktree, in the background. Output goes to a log file under `~/.local/state/wsx/logs/`, and wsx shows the path. The command keeps running after you quit wsx, and it appears in the list so you can stop it with `K`.

## What's listed

- Processes whose working directory is inside the worktree.
- Not shells and editors (bash, zsh, nvim, code, and so on).
- Not helpers that Claude Code and editors start, such as MCP and language servers, unless they listen on a TCP port. A dev server started from inside Claude Code still shows.

## Archiving stops processes

Archiving a workspace, with `d` or [`wsx workspace archive`](../cli-reference/workspace.md#wsx-workspace-archive), stops every listed process before it removes the worktree. Each gets `SIGTERM`, then `SIGKILL` after two seconds. `--keep-worktree` skips this. A process wsx can't signal, such as one owned by another user, is reported and the archive continues.

## Requirements

Process tracking needs `lsof`. Without it, the count stays at 0 and the list is empty.
