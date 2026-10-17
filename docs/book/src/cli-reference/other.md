# Other commands

Settings, themes, remote shortcuts, shared workspaces, setup helpers and desktop widgets.

## wsx config get

```
wsx config get <key>
```

Prints a global setting's value, or `(unset)`. See [Settings](../configuration/settings.md) for every key.

## wsx config set

```
wsx config set <key> <value-or-@file>
```

Sets a global setting. `@path` reads the value from a file, and `""` clears the setting back to its default. An unknown key is an error.

## wsx config list

```
wsx config list
```

Lists every setting that has a value.

## wsx config edit

```
wsx config edit <key>
```

Opens a setting's value in `$EDITOR` and saves it when you exit. Use it for multi-line values such as `remotes` and `pinned_commands`.

## wsx theme check

```
wsx theme check [<path>]
```

Validates the bar theme file and prints every error. Without `<path>` it checks `~/.config/wsx/theme.toml`, and a missing file there passes. See [Themes](../configuration/themes.md).

- It also warns when the `bar_theme` setting is off, since the file then has no effect.

## wsx theme path

```
wsx theme path
```

Prints where wsx looks for `theme.toml`.

## wsx theme init

```
wsx theme init
```

Writes the bundled default `theme.toml` to that path. It refuses to overwrite an existing file.

## wsx remote

```
wsx remote [<name>]
```

Without a name, lists your saved remote shortcuts. With a name, replaces the `wsx` process with `sh -c <command>`. See [Remote access](../integrations/remote-access.md).

- Shortcuts live in the `remotes` setting as one `name=command` line each. Add one with `wsx config edit remotes`. There is no `wsx remote add`.

## wsx shared list

```
wsx shared list [--json] [--all]
```

Lists tmux-shared workspaces as tab-separated `repo  slug  session  alive` rows, one per agent. See [Shared workspaces](../integrations/shared-workspaces.md).

| Flag | Effect |
| --- | --- |
| `--all` | Also list every ready workspace that isn't shared. |
| `--json` | Print records with each workspace's PR status. Needs `gh` for the PR status. |

## wsx setup install-skill

```
wsx setup install-skill
```

Installs or updates the bundled `wsx`, `agent-review` and `handoff` skills for Claude Code, and for Codex and Hermes when they're installed. See [Agent skills](../agents/skills.md).

## wsx setup waybar

```
wsx setup waybar
```

Installs the waybar module into `~/.config/waybar`, plus the elephant menu and walker theme when those are installed. Linux only. See [Desktop widgets](../integrations/desktop-widgets.md).

## wsx setup plasma

```
wsx setup plasma
```

Installs the KDE Plasma 6 panel applet. Linux only. See [Desktop widgets](../integrations/desktop-widgets.md).

## wsx setup menubar

```
wsx setup menubar
```

Installs the SwiftBar plugin. macOS only. See [Desktop widgets](../integrations/desktop-widgets.md).

## wsx waybar status

```
wsx waybar status
```

Prints the JSON for the waybar `custom/wsx` module. Linux only, like every `wsx waybar` command.

## wsx waybar menu

```
wsx waybar menu
```

Opens a workspace picker and jumps to the one you choose. It uses walker unless `WSX_WAYBAR_MENU` names another menu command.

## wsx waybar jump

```
wsx waybar jump <repo> <slug>
```

Selects the workspace in a running dashboard, or opens a new one in a terminal (`terminal_cmd`) with it selected.

## wsx waybar menu-entries

```
wsx waybar menu-entries [--json]
```

Prints the workspace entries for the elephant menu as JSON.

## wsx waybar refresh-prs

```
wsx waybar refresh-prs
```

Refreshes the cached PR state for every workspace. Needs `gh`.

## wsx desktop status

```
wsx desktop status
```

Prints a JSON summary for panel widgets: `count`, `class` (the most urgent state), `tooltip`, and `rows` of `{repo, slug, state, message, pr_number}`.

## wsx desktop jump

```
wsx desktop jump <repo> <slug>
```

Selects the workspace in a running dashboard and raises its window, or opens a new one. Linux only.

## wsx menubar plugin

```
wsx menubar plugin
```

Prints the SwiftBar plugin document. macOS only, like every `wsx menubar` command.

## wsx menubar jump

```
wsx menubar jump <repo> <slug>
```

Selects the workspace in a running dashboard, or opens a new one in a terminal.

## wsx menubar copy-path

```
wsx menubar copy-path <repo> <slug>
```

Copies the workspace's worktree path to the clipboard.

## wsx menubar refresh

```
wsx menubar refresh
```

Refreshes the cached git and PR indicators for every workspace.
