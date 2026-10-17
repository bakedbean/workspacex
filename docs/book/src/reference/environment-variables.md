# Environment variables

Set these in the shell that starts wsx. wsx passes its environment on to the agents it starts.

## Agent binaries and models

| Variable | Purpose |
| --- | --- |
| `WSX_CLAUDE_BIN` | Path to `claude`. Default: `claude` on `PATH`. |
| `WSX_PI_BIN` | Path to `pi`. Default: `pi` on `PATH`. |
| `WSX_PI_MODEL` | Model pattern passed to pi as `--model`. |
| `WSX_PI_PROVIDER` | Limits pi to one provider's models. |
| `WSX_HERMES_BIN` | Path to `hermes`. Default: `hermes` on `PATH`. |
| `WSX_HERMES_MODEL` | Model for Hermes, overriding `~/.hermes/config.yaml`. |
| `WSX_HERMES_PROVIDER` | Provider passed to Hermes as `--provider`. Hermes's default REPL mode may ignore it in favour of `~/.hermes/config.yaml`. |
| `WSX_CODEX_BIN` | Path to `codex`. Default: `codex` on `PATH`. |
| `WSX_CODEX_MODEL` | Model passed to Codex as `-m`. |
| `WSX_OMP_BIN` | Path to `omp` (oh-my-pi). Default: `omp` on `PATH`. |
| `WSX_OMP_MODEL` | Model passed to omp as `--model`. |

See [Coding agents](../agents/coding-agents.md).

## Other tools

| Variable | Purpose |
| --- | --- |
| `WSX_TMUX_BIN` | Path to `tmux`, for [shared workspaces](../integrations/shared-workspaces.md). |
| `WSX_SSH_BIN` | Path to `ssh`, for browsing [remote hosts](../integrations/shared-workspaces.md#browse-another-machine). |
| `WSX_WAYBAR_MENU` | Menu command for the Waybar workspace picker. Default: `walker --dmenu`. |
| `VISUAL`, `EDITOR` | Editor for `e` when `editor_cmd` is unset. |
| `EDITOR` | Editor for `wsx config edit`, the `edit-*` commands, and repo settings. Default: `vi`. |
| `TERMINAL` | Terminal for `t` when `terminal_cmd` is unset. |
| `SHELL` | Shell for setup and archive scripts. Falls back to `bash` for `sh`, `dash` and `ash`. |

## Behavior and diagnostics

| Variable | Purpose |
| --- | --- |
| `WSX_RENAME_MODE` | Auto-rename mode: `claude` (default), `local`, or `off`. See [Auto-rename](../configuration/auto-rename.md). |
| `RUST_LOG` | Log filter. Default: `info`. Use `wsx=debug` for verbose logs. |
| `WSX_INPUT_TRACE` | Set to `1` to log every key and mouse event to `wsx.log`. |

## Paths

| Variable | Purpose |
| --- | --- |
| `XDG_STATE_HOME` | Base of the state directory. Default: `~/.local/state`. |
| `XDG_CONFIG_HOME` | Base of the config directory. Default: `~/.config`. |
| `XDG_RUNTIME_DIR` | Base of the directory for the dashboard's control sockets. |

## Set by wsx

wsx sets these for the processes it starts.

| Variable | Set for | Purpose |
| --- | --- | --- |
| `WSX_WORKSPACE_ID` | Agent sessions | The workspace the session belongs to. |
| `WSX_AGENT_INSTANCE_ID` | Agent sessions | The agent instance, used to sign and receive messages. |
| `WSX_REPO_ROOT` | Setup and archive scripts | The source repo's path. |
| `WSX_WORKTREE` | Setup and archive scripts | The workspace's worktree path. |

See [Multi-agent workspaces](../agents/multi-agent-workspaces.md) and [Setup and archive scripts](../configuration/setup-scripts.md).
