# Settings

Global settings live in the wsx state database. Read and change them with `wsx config`. Per-repo values override some of them.

```bash
wsx config list
wsx config get <key>
wsx config set <key> <value>
wsx config edit <key>        # opens $EDITOR (default: vi)
```

Value sources:

- A literal: `wsx config set branch_prefix bakedbean`
- A file, prefixed with `@`: `wsx config set custom_instructions @./instructions.md`
- An empty string clears the key and restores the default: `wsx config set custom_instructions ""`
- `wsx config edit <key>` opens the current value in `$EDITOR`. Use it for multi-line values.

`config set` rejects unknown keys. On/off keys accept `on`/`true`/`1`/`yes` and `off`/`false`/`0`/`no`. See [`wsx config`](../cli-reference/other.md#wsx-config-set) for the full syntax.

## General

| Key | Default | Effect |
|---|---|---|
| `branch_prefix` | unset | Names new branches `<prefix>/<workspace>`. Unset, the branch is the bare workspace name. |
| `coding_agent` | `claude` | Agent for new workspaces: `claude`, `pi`, `hermes`, `codex` or `omp`. See [Coding agents](../agents/coding-agents.md). |

## Agent sessions

| Key | Default | Effect |
|---|---|---|
| `custom_instructions` | unset | Text appended to the agent's system prompt on every spawn. |
| `process_doctrine` | built-in text | Replaces the working doctrine injected into every session. `off`, `none` or `disabled` turns it off; an empty value restores the default. See [What agents are told](../agents/status-and-recap.md#what-agents-are-told). |
| `process_doctrine_extra` | unset | Extra `- ...` bullet lines appended to the doctrine. |
| `mcp_mirror` | on | Copy the source repo's MCP servers into each worktree. See [MCP server inheritance](../integrations/mcp-inheritance.md). |

## Editor and tools

| Key | Default | Effect |
|---|---|---|
| `editor_cmd` | `$VISUAL`, then `$EDITOR` | Command for `e`. The worktree path is appended unless the command contains `{path}`. |
| `terminal_cmd` | `$TERMINAL` | Command for `t`, run in the worktree. `{cmd}` makes it usable for desktop-widget jumps. |
| `diff_cmd` | unset | Command for `v`. Accepts `{path}` and `{base}`. |
| `lazygit_cmd` | unset | Command for `g`, run in the worktree. It must open its own window, for example `wezterm start -- lazygit`. |

See [Editor, terminal, and diff](../integrations/editor-terminal-diff.md) for examples.

## Notifications

| Key | Default | Effect |
|---|---|---|
| `notifications` | on | Master switch for the terminal bell and the `!` attention marker. |
| `notification_bell_question` | `double` | Bell when the agent asks you a question. |
| `notification_bell_complete` | `single` | Bell when the agent finishes a turn. |
| `notification_bell_permission` | `single` | Bell when the agent waits on a permission prompt. |
| `notification_bell_stalled` | `triple` | Bell when the agent stalls. |

Bell values are `off`, `single`, `double` or `triple`. `off` silences the bell but keeps the `!` marker. See [Status indicators](../dashboard/status-indicators.md).

## Dashboard

| Key | Default | Effect |
|---|---|---|
| `dashboard_sort_mode` | `recency` | Workspace order within a repo: `recency` or `status`. `o` toggles it. |
| `dashboard_blocked_pin_max_age_secs` | `86400` | How long a workspace blocked on you stays pinned to the top in `recency` mode. |
| `dashboard_branch_width` | `28` | Width of the branch column, clamped to 10–80. |
| `dashboard_pr_width` | `16` | Width of the PR column, clamped to 8–24. |
| `detail_bar_config` | built-in | JSON for the detail pane. See [Detail bar](../dashboard/detail-bar.md) for the schema. |
| `usage_graph_window` | `24h` | Window of the `$usage` activity graph: `24h`, `1w` or `1mo`. Shown only when a [theme](themes.md) places `$usage`. |
| `pinned_commands` | unset | `Label=command` lines shown as chips. See [Pinned commands](../attached-view/pinned-commands.md). |
| `prompt_tags` | unset | `name=uses` lines, maintained by the prompt-tag picker. See [Prompt tags](../attached-view/prompt-tags.md). |

## Appearance

| Key | Default | Effect |
|---|---|---|
| `theme` | `wsx` | Base palette: `wsx`, `default` (terminal ANSI colors), `dracula`, `jellybeans` or `nord`. Restart wsx to apply. |
| `bar_theme` | off | Read bar layout and styling from `~/.config/wsx/theme.toml`. Applies within a second. See [Themes](themes.md). |
| `nerd_fonts` | on | Use Nerd Font glyphs on the dashboard. |

## Remote

| Key | Default | Effect |
|---|---|---|
| `remote_control` | on | Start claude with `--remote-control`, so claude.ai/code and the mobile app can reach it. See [Remote access](../integrations/remote-access.md). |
| `remote_control_sandbox` | off | Also pass `--sandbox` when `remote_control` is on. |
| `remotes` | unset | `name=command` lines run by `wsx remote <name>`. See [Remote access](../integrations/remote-access.md). |
| `shared_hosts` | unset | `name=ssh-destination` lines for browsing and creating workspaces on other machines. See [Shared workspaces](../integrations/shared-workspaces.md). |

## Per-repo settings

Each repo can override or add these values:

| Setting | CLI | Notes |
|---|---|---|
| Name | `wsx repo set-name` | |
| Branch prefix | `wsx repo set-prefix` | Overrides `branch_prefix`. |
| Base branch | `wsx repo set-base-branch` | |
| Custom instructions | `wsx repo set-instructions` | Added to the global `custom_instructions`. |
| Setup and archive scripts | `wsx repo set-setup`, `set-archive` | See [Setup and archive scripts](setup-scripts.md). |
| Pinned commands | `wsx repo set-pinned-commands` | Replaces the global `pinned_commands`. |
| Related repos | `wsx repo set-related-repos` | See [Related repos](../integrations/related-repos.md). |
| Detail bar | drawer only | Merged over the global `detail_bar_config`. |

You can also edit them in the repo settings drawer: press `s` on any dashboard row.

- An unset field shows the global value marked `(inherited)`, or `(unset)`.
- `↑`/`↓` selects a field. `Enter` opens the repo value in `$EDITOR` and saves it when the editor exits.
- `d` clears the repo value, which restores inheritance. `Esc` closes the drawer.

Use a terminal editor that blocks until you quit (vim, helix, nano). A GUI editor that returns immediately saves nothing.

See [`wsx repo`](../cli-reference/repo.md) for the full syntax.
