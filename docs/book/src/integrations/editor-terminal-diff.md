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

If `editor_cmd` or `diff_cmd` contains no placeholder, wsx appends the worktree path as the last argument. `terminal_cmd` and `lazygit_cmd` start with the worktree as their working directory instead.

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
