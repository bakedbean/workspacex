`[e]` and `[t]` on the dashboard launch your editor or terminal in the selected workspace's worktree directory. Both spawn detached so wsx keeps running.

Resolution chain (first non-empty wins):

- Editor: `editor_cmd` setting → `$VISUAL` → `$EDITOR`
- Terminal: `terminal_cmd` setting → `$TERMINAL`

**TUI editors (vim, nvim, helix, emacs -nw) need to be wrapped in a terminal command** because the spawned editor has no controlling TTY of its own. Example:

```
wsx config set editor_cmd "alacritty -e nvim"
```

GUI editors (VS Code, Cursor, Zed) work directly:

```
wsx config set editor_cmd "code"
```

### `{path}` placeholder

If your command contains `{path}`, the worktree path is substituted there
instead of being appended. Useful when the editor expects the path as a flag
value, or when launching a TUI editor inside a terminal where you want the
terminal's cwd to be set rather than passing the path to the editor:

```
wsx config set editor_cmd "xdg-terminal-exec --dir={path} nvim"
```

Result: `xdg-terminal-exec --dir=/path/to/worktree nvim` (nvim starts in the
worktree directory with no file argument — avoids triggering netrw / tree
plugins on a directory open).

For terminal commands the same substitution applies, though most terminals
honor the spawned process's cwd already so you typically don't need it.

### Diff command

`[v]` spawns the configured difftool with the selected workspace's worktree path as `{path}` and the repo's main branch as `{base}`. Unlike editor/terminal, there's no env-var fallback — set `diff_cmd` explicitly.

Examples (note the **three dots** — explained under "Why three dots?" below):

```
# Terminal pager with delta-prettified diff
wsx config set diff_cmd "alacritty -e sh -c 'cd {path} && git diff {base}...HEAD | delta'"

# Neovim with diffview.nvim (set alacritty's cwd so nvim doesn't open {path} as a buffer)
wsx config set diff_cmd "alacritty --working-directory={path} -e nvim -c 'DiffviewOpen {base}...HEAD'"

# VS Code (opens the workspace; user navigates to Source Control panel)
wsx config set diff_cmd "code {path}"
```

The base ref is auto-detected from `origin/HEAD` and substituted as the **upstream** tracking ref (e.g. `origin/main`) — using the upstream means a stale local `main` doesn't poison the diff. Falls back to `main` if your repo doesn't have `origin/HEAD` set. (Tip: `git remote set-head origin --auto` after cloning fixes that for the wsx repo metadata too.)

**Why three dots?** `git diff A..B` (two dots) lists every commit on `B` that isn't on `A`'s current tip. If your local `main` is behind `origin/main`, those upstream commits show up as "extra changes" in your branch diff. `A...B` (three dots) anchors at the merge base — the commit where your branch diverged — so stale local refs don't pollute the view. This is what `gh pr` and most code-review tools use.

For `editor_cmd` and `terminal_cmd`, if neither the setting nor the env-var fallback is set, an error modal explains how to configure. `diff_cmd` has no env-var fallback and errors directly if unset.
