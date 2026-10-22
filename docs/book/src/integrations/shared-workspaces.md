# Shared workspaces

A shared workspace runs its agents inside tmux instead of as children of wsx. Quitting wsx or dropping your ssh connection doesn't stop them, and you can attach to an agent from another terminal or another machine.

Requires tmux 3.2 or later.

## Create or convert

```bash
wsx workspace create <repo> --shared
wsx workspace share <repo> <slug>
wsx workspace unshare <repo> <slug>
```

On the dashboard:

| Key | Action |
| --- | --- |
| `S` | New shared workspace. In the new-workspace form, `Ctrl-s` toggles shared. |
| `T` | Toggle the selected workspace between shared and direct, after a confirmation. |

A running agent can't move into or out of tmux, so it must restart:

- `T` restarts running agents right away and resumes their conversations. Stopped agents start in tmux.
- `share` and `unshare` change only the flag. Running agents keep their old mode until they restart.
- `share --restart` asks the dashboard on that host to do what `T` does, then waits and prints the tmux session names. It needs wsx running on the host, and fails without changes if no dashboard answers within a few seconds.

See [`wsx workspace share`](../cli-reference/workspace.md#wsx-workspace-share).

## On the dashboard

A shared workspace shows a badge left of its branch: a network icon with nerd fonts, `◇` without.

| Badge color | Meaning |
| --- | --- |
| Green | The tmux session is alive, even if this wsx isn't attached to it. |
| Red | No live session backs it, so nobody can attach. |

After you restart wsx, a shared workspace's agent is still running. Press `Enter` on the row to reattach with its history intact.

## Attach from another terminal

Each agent runs in a tmux session you can attach to directly:

```bash
tmux attach -t wsx-<repo>-<workspace>
```

Extra agents get `wsx-<repo>-<workspace>-<agent><n>`, for example `wsx-myrepo-fix-bug-codex2`. Characters other than letters, digits, `_` and `-` become `-`. The name is fixed when the session starts, so renaming the workspace doesn't change it.

List shared workspaces and their sessions:

```bash
wsx shared list            # repo, workspace, session, alive/(dead)
wsx shared list --all      # also every ready workspace that isn't shared
wsx shared list --json
```

See [`wsx shared list`](../cli-reference/other.md#wsx-shared-list).

## Browse another machine

List the hosts in the `shared_hosts` setting, one `name=ssh-destination` per line:

```bash
wsx config edit shared_hosts
```

```
mini=eben@ebenmini.local
lab=user@lab.example.com
```

Press `H` on the dashboard to pick a host. For one-click access, add the `$hosts` segment to your dashboard footer (see [Themes](../configuration/themes.md)).

wsx lists the host's workspaces: live agents first (agent, branch with PR number and tags, repo/workspace), then dimmed rows marked `not shared` or `stopped`.

| Key | Action |
| --- | --- |
| `j` / `k` | Move. |
| `Enter` | Attach to a live agent. On a `not shared` or `stopped` row, share and restart it on the host after a confirmation, then attach. |
| `n` / `N` | Create a workspace on the host, normal or yolo. |
| `r` | Refresh the list. |
| `Esc` | Close. |

- Sharing from the list restarts the agent, which cuts off what it was doing. Its conversation resumes.
- `Ctrl-x d` detaches from a remote agent and returns to the dashboard. The agent keeps running on the host.

### Create a workspace on the host

`n` opens a form preset to the selected row's repo:

| Key | Action |
| --- | --- |
| `↑` / `↓` | Change repo. |
| Type | Set the name. Leave it empty for a generated name. |
| `Tab` | Cycle the agent, starting from the host's default. |
| `Ctrl-y` | Toggle yolo. |
| `Enter` | Create it on the host as shared, run its setup script, start its agents, and attach. |

- If the workspace can't be created (name taken, unknown repo), the form returns with the host's reason.
- If it's created but its agents don't start, it appears in the list with the reason. Press `Enter` on it once the host is fixed.
- If its setup script fails, wsx doesn't attach. The list shows the exit code and the setup log path on the host.
- Creating gives up after 15 minutes. The host may still finish, so press `r` before you retry.

Names that would need shell quoting can't be shared or created from the list. A host running an older wsx shows only its live shared sessions, with no sharing or creating.

### Requirements for remote hosts

- Key-based ssh access. The background list fetch can't answer a password prompt.
- `wsx` and `tmux` on the PATH of a login `sh` on the host. Check with `ssh <host> "sh -lc 'which wsx tmux'"`. On macOS, PATH set only in `~/.zshrc` or `~/.zprofile` (including Homebrew's) is invisible to `sh -l`, so add it to `~/.profile` too.
- The wsx dashboard running on the host to share or create from the list. Attaching to an already-shared workspace doesn't need it.
- Locally, only `ssh`.

## Scrollback

Reattaching shows only the session's current screen. wsx's scrollback starts fresh with each attach. tmux keeps its own scrollback, which you can reach with tmux copy mode (`Ctrl-b [`).
