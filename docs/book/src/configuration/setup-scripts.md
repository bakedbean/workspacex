# Setup and archive scripts

Each repo can have a setup script, which runs when a workspace is created, and an archive script, which runs when a workspace is removed. Use them to install dependencies or clean up build output.

```bash
wsx repo set-setup   <repo> 'bun install'
wsx repo set-archive <repo> 'rm -rf node_modules'
wsx repo set-setup   <repo> @./scripts/setup.sh   # from a file
wsx repo edit-setup  <repo>                       # in $EDITOR
wsx repo set-setup   <repo> ""                    # clear
```

You can also edit both scripts in the repo settings drawer (`s` on the dashboard). See [Per-repo settings](settings.md#per-repo-settings) and [`wsx repo`](../cli-reference/repo.md#wsx-repo-set-setup).

## How scripts run

- wsx runs each script as `$SHELL -ilc "<script>"`, so your shell profile loads first and tools like `mise`, `direnv` and `asdf` work.
- If `$SHELL` is unset or a POSIX-only shell (`sh`, `dash`, `ash`), wsx uses `/bin/bash`.
- The working directory is the worktree.
- Two extra environment variables are set:

| Variable | Value |
|---|---|
| `WSX_REPO_ROOT` | The source repo |
| `WSX_WORKTREE` | The workspace's worktree |

## Setup failures and logs

A failed setup doesn't block the workspace. Its dashboard row shows a `⚙!` badge, and a cancelled setup shows `⚙?`.

Setup output goes to `~/.local/state/wsx/logs/setup-<repo>-<workspace>.log`. Each run overwrites it, and a rename moves it. wsx doesn't keep archive script output.

To read the log in the TUI, or to cancel a setup that's still running, open the workspace card with `?` on the dashboard. `o` opens the log and `x` cancels the setup. See [Keybindings](../dashboard/keybindings.md).

The archive script runs before wsx removes the worktree, and you can't cancel it.
