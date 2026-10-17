# Quick start

This walkthrough takes you from install to a running agent session, then
connects wsx to your editor, terminal, and diff tool.

1. Install wsx. On macOS or Linux with Homebrew:

   ```bash
   brew tap bakedbean/workspacex https://github.com/bakedbean/workspacex
   brew install bakedbean/workspacex/wsx
   ```

   For other methods, see [Installation](installation.md).

2. Register a repository:

   ```bash
   wsx repo add /path/to/your/repo
   ```

3. Launch the dashboard:

   ```bash
   wsx
   ```

4. Press `n` to create a workspace: a new branch and worktree with its own
   agent session. Press `N` (`Shift-N`) instead to start the agent in
   permissive mode, which skips permission prompts.

5. Press `enter` to attach. The agent (Claude Code by default) runs inside
   the worktree.

6. Press `Ctrl-x d` to detach back to the dashboard. The session keeps
   running.

7. Tell wsx which tools the `[e]` editor, `[t]` terminal, and `[v]` diff keys
   launch. For example, Neovim in Alacritty:

   ```bash
   wsx config set editor_cmd   "alacritty --working-directory={path} -e nvim"
   wsx config set terminal_cmd "alacritty --working-directory={path}"
   wsx config set diff_cmd     "alacritty --working-directory={path} -e nvim -c 'DiffviewOpen {base}...HEAD'"
   ```

   `{path}` is the worktree and `{base}` is the diff base ref. Wrap terminal
   editors like `nvim` in a terminal command; GUI editors like `code` work
   directly. See [Editor, terminal, and diff](../integrations/editor-terminal-diff.md)
   for placeholders and fallbacks.

## Next steps

- [Dashboard](../dashboard/index.md) and [Keybindings](../dashboard/keybindings.md)
- [Attached view](../attached-view/index.md)
- [Coding agents](../agents/coding-agents.md), to switch from Claude Code
- [Settings](../configuration/settings.md)
