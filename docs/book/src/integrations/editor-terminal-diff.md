# Editor, terminal, and diff

Open the selected workspace in your own tools without leaving wsx. Each key works on a dashboard row and, after `Ctrl-x`, in the attached view. The tool starts detached, so wsx keeps running.

| Key | Opens | Setting | Fallback |
| --- | --- | --- | --- |
| `e` | Your editor on the worktree | `editor_cmd` | `$VISUAL`, then `$EDITOR` |
| `t` | A terminal in the worktree | `terminal_cmd` | `$TERMINAL` |
| `v` | Your difftool on the branch | `diff_cmd` | none |
| `g` | lazygit or another git UI in the worktree | `lazygit_cmd` | none |

If no command resolves, wsx shows an error that names the setting to set. See [Settings](../configuration/settings.md) for each key.

## Placeholders

| Placeholder | Replaced with | Used by |
| --- | --- | --- |
| `{path}` | The worktree path | All four |
| `{base}` | The upstream base ref, such as `origin/main` | `diff_cmd` |
| `{cmd}` | The wsx command a desktop jump runs | `terminal_cmd`, from [desktop widgets](desktop-widgets.md) |
| `{file}` | The file's absolute path | `file_editor_cmd`, [opening one file](#opening-one-file) |
| `{line}` | The line to start on | `file_editor_cmd`, [opening one file](#opening-one-file) |

If `editor_cmd` or `diff_cmd` contains no placeholder, wsx appends the worktree path as the last argument. `terminal_cmd` and `lazygit_cmd` start with the worktree as their working directory instead.

## Opening one file

`o` on a file in the [side panel](../dashboard/side-panel.md#browsing-changed-files), or in its diff viewer, opens just that file with `file_editor_cmd`, or `editor_cmd` when that isn't set. `{path}` is still the worktree, and the editor starts in it. Unless the command uses `{file}`, wsx appends the file's path as the last argument, so a command set up for `e` opens the file the same way:

| Command | `o` runs |
| --- | --- |
| `code` | `code /wt/src/main.rs` |
| `xdg-terminal-exec --dir={path} nvim` | `xdg-terminal-exec --dir=/wt nvim /wt/src/main.rs` |

To start on the line at the top of the diff viewer, set `file_editor_cmd` and place `{file}` and `{line}` in it yourself. `e` never reads `file_editor_cmd`, so it keeps opening the worktree:

```bash
wsx config set file_editor_cmd "xdg-terminal-exec --dir={path} nvim +{line} {file}"
wsx config set file_editor_cmd "code --goto {file}:{line}"
```

From the file list, `{line}` is 1. A file the branch deleted has nothing to open, so wsx says so instead.

`{base}` comes from `origin/HEAD` and falls back to `main`. If it's wrong, run `git remote set-head origin --auto` in the repo.

## Examples

TUI editors have no terminal of their own, so wrap them in one:

```bash
wsx config set editor_cmd "xdg-terminal-exec --dir={path} nvim"
```

GUI editors work directly:

```bash
wsx config set editor_cmd "code"
```

A diff in a terminal pager. Use three dots so the diff starts at the merge base and a stale local `main` doesn't add noise:

```bash
wsx config set diff_cmd "alacritty -e sh -c 'cd {path} && git diff {base}...HEAD | delta'"
```

lazygit needs a terminal window, because wsx owns its own:

```bash
wsx config set lazygit_cmd "wezterm start -- lazygit"
```

If your editor hosts its own AI agent, see [Editor-hosted agents](editor-agent-context.md) to give it the workspace's context.
