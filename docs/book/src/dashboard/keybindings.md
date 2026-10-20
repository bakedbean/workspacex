# Keybindings

Every key in the wsx TUI, grouped by where you press it. Capital letters mean `Shift` plus the letter.

## Dashboard

### Moving around

| Key                     | Action                                                 |
| ----------------------- | ------------------------------------------------------ |
| `j` / `k` (or arrows)   | Move the selection                                     |
| `h` / `l`               | Fold / unfold the repo; `l` on a workspace attaches    |
| `z z`                   | Toggle the repo's fold                                 |
| `z a` (or `z r`)        | Unfold every repo                                      |
| `z M` (or `z m`)        | Fold every repo                                        |
| `/`                     | [Filter](tags-and-hiding.md#searching) the rows; `esc` clears |
| `o`                     | Sort by recency or by status (saved)                   |
| `G`                     | Group by repo or by attention                          |
| `Tab`                   | Focus the detail-bar reply, then the digest            |
| `q`                     | Quit; stops direct sessions (shared ones keep running)  |

`q` asks first if a workspace is still being created or archived.

### Workspaces

| Key              | Action                                                       |
| ---------------- | ------------------------------------------------------------ |
| `enter` / `i`    | Attach; on a repo header, create a workspace there           |
| `n`              | Create a workspace in the selected repo                      |
| `N`              | Create one in permissive mode (skips agent permission prompts) |
| `S`              | Create a [tmux-shared](../integrations/shared-workspaces.md) workspace |
| `T`              | Toggle the workspace between shared and direct               |
| `d`              | Archive                                                      |
| `e` / `t` / `v` / `g` | Open in [editor, terminal, diff, or lazygit](../integrations/editor-terminal-diff.md) |
| `?`              | Workspace actions card                                       |
| `#`              | Edit [tags](tags-and-hiding.md)                              |
| `C`              | Pick the row's name color                                    |
| `K`              | [Processes](process-tracking.md) under the worktree          |
| `Ctrl-x 1`–`9`   | Send a [pinned command](../attached-view/pinned-commands.md) to the workspace |
| `Ctrl-x a`       | [Agents panel](../agents/multi-agent-workspaces.md)          |

### Repos and panes

| Key                | Action                                                  |
| ------------------ | ------------------------------------------------------- |
| `K` / `J` on a repo | Move the repo up / down (saved)                        |
| `s`                | Repo settings                                           |
| `F`                | [Hide rules](tags-and-hiding.md#hiding-workspaces) for the repo |
| `H`                | [Remote hosts](../integrations/shared-workspaces.md)    |
| `p`                | Toggle the [digest](project-manager-digest.md)          |
| `r` (digest open)  | Refresh git and PR data                                 |
| `enter` / `l` on a hidden group | Show its workspaces                        |
| `h` on a hidden group | Collapse it                                          |

### Detail-bar reply

| Key        | Action                                             |
| ---------- | -------------------------------------------------- |
| `Tab`      | Focus the reply input                              |
| `enter`    | Send to the agent (starts it if needed)            |
| `esc`      | Discard and return to the list                     |

## Switching panels

Panels open in a drawer at the bottom of the screen.

- A panel's key opens it and replaces the open panel. Pressing the open panel's key again closes it. A key with nothing to open (`?` on a repo header) leaves the panel alone.
- While a panel takes text (tag editor, name-color filter, name fields, a `/` filter), keys type. In the process list `K` kills; in the updates panel `s` splits.
- `Ctrl-x` chords that open panels work from any panel. Progress cards for remote work ignore panel keys until they finish or you press `esc`.

## Dialogs

| Key        | New workspace                  | Confirm (archive, quit, share) |
| ---------- | ------------------------------ | ------------------------------ |
| `enter`    | Create (blank name generates one) | —                           |
| `y` / `n`  | —                              | Confirm / cancel               |
| `esc`      | Cancel                         | Cancel                         |
| `Tab`      | Cycle the agent                | —                              |
| `Ctrl-s`   | Toggle shared                  | —                              |

## Workspace actions card (`?`)

| Key                 | Action                                     |
| ------------------- | ------------------------------------------ |
| `r`                 | Rename the workspace and its branch        |
| `o`                 | Open the setup log                         |
| `x`                 | Cancel a create in progress                |
| `C` / `#`           | Name color / tags                          |
| `j` / `k`           | Move the selection, card stays open        |
| `e` `t` `v` `g` `enter` | Act on the workspace and close         |
| `?` / `esc`         | Close                                      |

## Setup log viewer (`?` then `o`)

Shows the [setup script](../configuration/setup-scripts.md)'s output: live while the workspace is being created, the saved log afterwards. Stderr lines are marked `!`. Only the last 2000 lines show.

| Key                     | Action                       |
| ----------------------- | ---------------------------- |
| `j` / `k` (or arrows)   | Scroll one line              |
| `PageUp` / `PageDown`   | Scroll ten lines             |
| `g` / `Home`            | Jump to the start            |
| `G` / `End`             | Jump to the end              |
| `esc` / `enter`         | Close; setup keeps running   |

## Panels opened from the dashboard

| Panel                  | Keys                                                                |
| ---------------------- | ------------------------------------------------------------------- |
| Tag editor (`#`)       | type + `enter` add; arrows + `space` toggle; `esc` close            |
| Hide rules (`F`)       | `j`/`k` move; `space` toggle; `esc` close                           |
| Name color (`C`)       | arrows move; type to filter; `enter` apply; `Delete` clear; `esc` close |
| Repo settings (`s`)    | `j`/`k` move; `enter` edit; `d` clear the repo's value; `esc` close       |
| Processes (`K`)        | arrows move; `r` run a command; `k` SIGTERM; `K` SIGKILL; `esc` close |
| Remote hosts (`H`)     | `j`/`k` move; `enter` open; `esc` close                             |
| Remote workspace list  | `enter` attach or share; `n`/`N` new; `r` refresh; `esc` close      |

## Digest (`p`, focused)

| Key                    | Action                              |
| ---------------------- | ----------------------------------- |
| `j` / `k` (or arrows)  | Move the selection                  |
| `enter`                | Attach to the workspace             |
| `/`                    | Filter by name                      |
| `r`                    | Refresh git and PR data             |
| `esc`                  | Clear the filter, or return focus   |
| `Tab`                  | Return focus to the list            |
| `p` / `q`              | Close the digest                    |

## Attached view

Keys go to the agent, except chords after `Ctrl-x`. Pressing `Ctrl-x` alone shows a menu; pick with arrows and `enter`, or press the letter. wsx drops `Ctrl-z` and `Ctrl-d` so a slip can't suspend or exit the agent.

| Key                  | Action                                                  |
| -------------------- | ------------------------------------------------------- |
| `Ctrl-x d`           | Close the pane; detach if it's the last one             |
| `Ctrl-x D`           | Save the split layout, then detach                      |
| `Ctrl-x` arrows      | Move focus between split panes                          |
| `Ctrl-x u`           | [Updates panel](../attached-view/updates-and-splits.md) |
| `Ctrl-x a`           | [Agents panel](../agents/multi-agent-workspaces.md)     |
| `Ctrl-x e` / `t` / `v` / `g` | Editor, terminal, diff, lazygit                 |
| `Ctrl-x k`           | Processes                                               |
| `Ctrl-x <`           | [Prompt tag](../attached-view/prompt-tags.md) picker    |
| `Ctrl-x #`           | [Workspace tags](tags-and-hiding.md) editor             |
| `Ctrl-x 1`–`9`       | Send a pinned command                                   |
| `Ctrl-x x`           | Send a literal `Ctrl-x` to the agent                    |
| `Ctrl-x esc`         | Dismiss the menu                                        |
| `Ctrl-x q` `w` `r` `y` `i` `o` `p` `s` `h` `j` | Switch the pane to another agent (multi-agent workspaces) |

A workspace attached from a remote host takes only `Ctrl-x d`, `Ctrl-x <`, and `Ctrl-x 1`–`9`.

## Panels opened from the attached view

| Panel                       | Keys                                                                |
| --------------------------- | ------------------------------------------------------------------- |
| Updates (`Ctrl-x u`)        | `j`/`k` move; `enter`/`l` attach; `v` side-by-side split; `s` stacked split; `o`/`G` sort and group; `/` filter; `esc` close |
| Agents (`Ctrl-x a`)         | `j`/`k` move; `enter` add the selected kind; `a` add one of each; `x` remove the newest; `esc` close |
| Prompt tag (`Ctrl-x <`)     | arrows move; `1`–`9` pick; type to filter or name a new tag; `enter` write the body; `Ctrl-d` delete the tag; `esc` close |
| Prompt tag body             | `Ctrl-s` insert; `esc` back                                         |
