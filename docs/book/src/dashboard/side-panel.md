# Side panel

The side panel shows the selected workspace's branch in more depth, on the right of the screen, one tab at a time. It's available on the dashboard list, in the [repo focus view](repo-focus.md), and in the [attached view](#in-the-attached-view).

On the dashboard, press `c` to open it for the selected workspace:

- **Commits**: the commits on the branch that the repo's base branch doesn't have, newest first, as a short hash and its full subject. The tab counts them all, but the list stops at the newest 200 and says how many older ones it left out.
- **Files**: each file the branch changed against its base, with its `+N −M` line counts (`binary` for a binary file), under a total. Paths wrap rather than being shortened. These are committed changes; uncommitted ones aren't listed.

```text
│  Commits 2   Files 7
│ bakedbean/repo-focus-view vs main
│ ──────────────────────────────────
│ cda7649  fix(dashboard): name the base
│          branch in the focus view's
│          ahead count
│ 392b182  fix(dashboard): drop "clean"
```

Press `1` for Commits and `2` for Files, or click a tab; either opens the panel if it's closed. The mouse wheel over the panel scrolls it. Drag the panel's left edge to resize it; the dashboard and the attached view share the width, and wsx remembers it across restarts. `c` or `esc` closes it, and it remembers its tab. With a repo header or a hidden group selected, the panel asks you to select a workspace.

The panel needs the repo's **Base branch** setting (repo settings, `s`) to know what to compare against. After you change that setting, the panel shows `loading…` until the lists against the new base arrive (within about ten seconds); it never shows the old base's lists in their place. The panel stays hidden on terminals under 100 columns wide.

## Browsing changed files

Press `→` to move the keyboard into the Files list, opening the panel on Files if need be. The file under the cursor is highlighted:

| Key           | Action                                   |
|---------------|------------------------------------------|
| `j` / `k`     | Move the cursor (`↓` / `↑` also work)    |
| `g` / `G`     | First / last file                        |
| `enter`       | Open the file's [diff](#the-diff-viewer) |
| `o`           | Open the file in your [editor](../integrations/editor-terminal-diff.md#opening-one-file) |
| `←` / `esc`   | Give the keys back to the workspace list |

Any other key also gives focus back and then does what it normally does, so `n` still makes a new workspace and `c` still closes the panel. `l` still opens the selected workspace. Clicking a file opens its diff without moving focus.

### The diff viewer

The diff viewer fills the screen with one file's changes against the base branch. When the branch still has the file, the viewer shows all of it with the deleted lines (red) and added lines (green) in place, numbered as the branch has them. For a deleted file it shows the diff's hunks. On the right, a minimap shows the whole file as dots, four lines per row, with the changes colored and the part on screen shaded.

```text
╭ src/ui/dashboard/tall.rs  3/7  +412 −63 ─────────╮╭ minimap ────────╮
│  12  use crate::ui::text;                        ││⣿⣿⣿⡇  ⣤⣤⣤       │
│      -use crate::ui::old;                        ││⣿⣿⣿⣿⣿⣿⣷        │
│  13 +use crate::ui::new;                         ││⠿⠿⠿⠇           │
╰──────────────────────────────────────────────────╯╰─────────────────╯
```

| Key                         | Action                              |
|-----------------------------|-------------------------------------|
| `j` / `k`, `↓` / `↑`        | Scroll a line                       |
| `J` / `K`, `PgDn` / `PgUp`  | Scroll a page                       |
| `Ctrl-d` / `Ctrl-u`         | Scroll half a page                  |
| `g` / `G`                   | Top / bottom                        |
| `[` / `]`, `←` / `→`        | Previous / next changed file        |
| `o`                         | Open the file in your [editor](../integrations/editor-terminal-diff.md#opening-one-file), at the top line on screen |
| `m`                         | Hide or show the minimap            |
| `esc` / `q`                 | Close                               |

The mouse wheel over the diff or the minimap scrolls the diff. Click or drag on the minimap to jump there. The minimap hides itself on terminals under 60 columns wide.

## In the attached view

Press `Ctrl-x c` to open the panel beside the agent, for the focused pane's workspace; `Ctrl-x c` again closes it. `Ctrl-x C` switches between Commits and Files, opening the panel if it's closed, and clicking a tab works too. The wheel over the panel scrolls it.

`Ctrl-x f` opens the panel on Files and puts the keyboard in the file list, as `→` does on the dashboard. The [same keys](#browsing-changed-files) then move through the files instead of going to the agent, until `←`, `esc`, or another key gives them back. Clicking a file opens its diff here too.

The agent's pane narrows to make room, and gets the width back when the panel closes. The attached view's panel opens and closes on its own, apart from the dashboard's, but follows you between workspaces and remembers its tab. A workspace attached from a remote host has no panel.
