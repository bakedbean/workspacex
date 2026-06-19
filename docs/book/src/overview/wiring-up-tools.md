Out of the box, wsx drives your agent sessions, but the three keys that make it
feel like a cockpit (`[e]` editor, `[v]` diff, `[t]` terminal) do nothing useful
until you tell wsx *which* tools to launch. These aren't configured by default,
and the payoff isn't obvious until you've set them: jump straight from a
workspace into your editor, a full branch diff, or a fresh shell, all rooted in
that workspace's worktree.

Set them once globally with `wsx config set`. Sample commands for a Neovim +
Alacritty setup:

```bash
# [e] — open the worktree in Neovim, running inside a new Alacritty window
wsx config set editor_cmd "alacritty --working-directory={path} -e nvim"

# [v] — view the branch diff in Neovim via diffview.nvim
wsx config set diff_cmd "alacritty --working-directory={path} -e nvim -c 'DiffviewOpen {base}...HEAD'"

# [t] — open a shell in the worktree in a new Alacritty window
wsx config set terminal_cmd "alacritty --working-directory={path}"
```

A few things worth knowing, all covered in detail under
[Editor, terminal, and diff integration](../integrations/editor-terminal-diff.md):

- **`{path}` and `{base}` placeholders.** `{path}` expands to the worktree path
  and `{base}` to the diff base ref (e.g. `origin/main`). If a command has no
  `{path}`, wsx *appends* the worktree path as a trailing argument. That's why
  Alacritty uses `--working-directory={path}` — passing the path positionally
  (`alacritty /some/path`) is an error, and setting its working directory also
  lets Neovim start in the worktree without opening the directory as a buffer.
- **TUI editors need a terminal wrapper.** vim/nvim/helix are launched detached
  from wsx and have no TTY of their own, so wrap them in a terminal command
  (`alacritty -e nvim`). GUI editors (`code`, `cursor`, `zed`) work directly.
- **Why `{base}...HEAD` (three dots).** Three dots anchor the diff at the merge
  base, so a stale local `main` doesn't pollute the view — the same diff `gh pr`
  shows.

`editor_cmd` and `terminal_cmd` fall back to `$VISUAL`/`$EDITOR` and `$TERMINAL`
respectively if unset; `diff_cmd` has no fallback and must be set explicitly.
Each can also be overridden per-repo — see the linked section.
