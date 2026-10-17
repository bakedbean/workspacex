# Desktop widgets

A desktop widget shows your live workspace count in the system bar, colored by
the most urgent status, and jumps to any workspace in one click. wsx ships one
for waybar, one for KDE Plasma 6, and one for the macOS menubar.

Every widget:

- shows a branch icon and the count of workspaces across every repo;
- takes its color from the most urgent status: blocked, done, waiting, or working;
- shows a per-workspace tooltip on hover;
- jumps to a workspace by opening it in the running TUI, attached, or by
  launching a new TUI in a terminal.

The setup command bakes in the path to `wsx`. Re-run it after you move or
reinstall wsx somewhere else.

## Waybar (Linux)

```bash
wsx setup waybar
```

Setup writes `wsx.jsonc` and `wsx.css` into `~/.config/waybar/` and adds the
`custom/wsx` module to `config.jsonc`, saving a timestamped backup first. If
it doesn't recognize your config's layout, it prints the lines to paste
instead.

Click the module to open a workspace picker. It uses
[walker](https://github.com/abenz1267/walker) by default. Set
`WSX_WAYBAR_MENU` to use another menu command.

When walker and elephant are installed, setup also adds an elephant menu and
a `wsx` walker theme, so the picker shows PR numbers, review marks, and diff
counts:

- The theme takes its colors from your current walker theme. Re-run
  `wsx setup waybar` after you switch walker themes.
- Setup restarts elephant for you. Restart walker yourself to load the theme.

To remove it, delete `"custom/wsx"` and the wsx `include` entry from
`config.jsonc`, or restore the backup.

See [`wsx setup waybar`](../cli-reference/other.md#wsx-setup-waybar) and
[`wsx waybar`](../cli-reference/other.md#wsx-waybar-status).

## KDE Plasma applet (Linux)

Requires KDE Plasma 6.

```bash
wsx setup plasma
```

Setup installs the applet into
`~/.local/share/plasma/plasmoids/io.github.bakedbean.wsx`. To add it to a
panel, right-click the panel, choose **Add or Manage Widgets**, and search for
"wsx".

Click the applet to list every repo's workspaces with their status glyph,
status message, and PR number. Pick one to jump to it and raise its window. If
the jump fails, the popup shows the error.

- After you re-run setup with the applet already on a panel, restart
  plasmashell to load the new version:

  ```bash
  kquitapp6 plasmashell && kstart plasmashell
  ```

- A terminal that serves several windows from one process can raise the wrong
  window of that terminal.

To remove it:

```bash
kpackagetool6 --type Plasma/Applet --remove io.github.bakedbean.wsx
```

See [`wsx setup plasma`](../cli-reference/other.md#wsx-setup-plasma) and
[`wsx desktop`](../cli-reference/other.md#wsx-desktop-status).

## macOS menubar (SwiftBar)

Requires [SwiftBar](https://github.com/swiftbar/SwiftBar). Launch SwiftBar
once and pick a plugin folder before you run setup.

```bash
brew install swiftbar
wsx setup menubar
```

Setup installs a `wsx-menubar.10s.sh` plugin into SwiftBar's plugin folder.

The dropdown lists every repo and its workspaces, with status glyph, dirty
marker, diff stats, and PR number. Each workspace has a submenu:

| Item | Action |
| --- | --- |
| Jump | Open the workspace in the TUI. |
| Open PR | Open the pull request in your browser. |
| Copy worktree path | Copy the path to the clipboard. |
| Reveal in Finder | Show the worktree in Finder. |

A **Project Manager** submenu lists each workspace's recap (goal, state, and
next step), blocked and waiting workspaces first. It mirrors the dashboard's
[project manager digest](../dashboard/project-manager-digest.md). Click a line
to jump to that workspace.

To remove it, delete `wsx-menubar.10s.sh` from the plugin folder.

See [`wsx setup menubar`](../cli-reference/other.md#wsx-setup-menubar) and
[`wsx menubar`](../cli-reference/other.md#wsx-menubar-plugin).

## Which terminal a jump opens

With no TUI running, a jump launches one in the first of these that applies:

| Order | Linux | macOS |
| --- | --- | --- |
| 1 | `terminal_cmd`, if it contains `{cmd}` | `terminal_cmd`, if it contains `{cmd}` |
| 2 | `$TERMINAL` | iTerm2 |
| 3 | konsole, in a Plasma session | Terminal |
| 4 | alacritty | |

`terminal_cmd` is also the dashboard's `[t]` terminal, which doesn't fill in
`{cmd}`. If you use `[t]`, leave `{cmd}` out of `terminal_cmd` and let the
jump pick a terminal from the rest of the list. See
[Editor, terminal, and diff](editor-terminal-diff.md).
