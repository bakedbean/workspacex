# CLI reference

Every `wsx` command and flag, grouped by command. Run `wsx` with no arguments to open the dashboard. `wsx --help` lists the command groups, `wsx <group> --help` lists a group's commands, and `wsx --version` prints the version.

```
wsx                          # open the dashboard
wsx --select <repo>/<slug>   # open the dashboard with a workspace selected
wsx <group> --help
wsx --version
```

## Conventions

| Convention | Meaning |
| --- | --- |
| `<value-or-@file>` | A literal value, or `@path` to read the value from a file. |
| `""` as a value | Clears the setting. Per-repo values then fall back to the global setting. |
| Current workspace | Commands without `--workspace` act on `$WSX_WORKSPACE_ID`, else the worktree containing the current directory. |
| `--workspace <repo>/<slug>` | Targets another workspace instead of the current one. |
| `--json` | Prints machine-readable output. Timestamps are epoch milliseconds. |

wsx sets `$WSX_WORKSPACE_ID` and `$WSX_AGENT_INSTANCE_ID` in every agent session it spawns, so commands run by an agent know their workspace and sender. See [Environment variables](../reference/environment-variables.md).

## Command groups

| Group | Purpose | Page |
| --- | --- | --- |
| `wsx workspace` | Create, list, rename, share, tag and archive workspaces | [wsx workspace](workspace.md) |
| `wsx repo` | Register repositories and set per-repo settings | [wsx repo](repo.md) |
| `wsx agent` | Add, remove and message agents | [wsx agent](agent.md) |
| `wsx status` | Report and read agent status | [wsx status, recap, context](status-recap-context.md) |
| `wsx recap` | Maintain the workspace recap | [wsx status, recap, context](status-recap-context.md) |
| `wsx context` | Print a context digest for editor-hosted agents | [wsx status, recap, context](status-recap-context.md) |
| `wsx config` | Get and set global settings | [Other commands](other.md#wsx-config-get) |
| `wsx theme` | Validate and scaffold the bar theme file | [Other commands](other.md#wsx-theme-check) |
| `wsx remote` | Run saved remote shortcuts | [Other commands](other.md#wsx-remote) |
| `wsx shared` | List tmux-shared workspaces | [Other commands](other.md#wsx-shared-list) |
| `wsx setup` | Install the agent skills and desktop widgets | [Other commands](other.md#wsx-setup-install-skill) |
| `wsx waybar` | Linux waybar module and workspace jumper | [Other commands](other.md#wsx-waybar-status) |
| `wsx desktop` | Desktop-neutral status and jump for panel widgets | [Other commands](other.md#wsx-desktop-status) |
| `wsx menubar` | macOS SwiftBar module and workspace jumper | [Other commands](other.md#wsx-menubar-plugin) |
