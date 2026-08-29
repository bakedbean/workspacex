Install wsx first. See [Installation](installation.md) for every method;
the short version on macOS and Linux is:

```bash
brew tap bakedbean/workspacex https://github.com/bakedbean/workspacex
brew install bakedbean/workspacex/wsx
```

Then point wsx at a repository and launch it:

```bash
wsx repo add /path/to/your/repo
wsx              # launch TUI
```

Press `n` (or SHIFT + N for permissive) to create your first workspace, then `enter` to attach. Claude Code spawns inside the worktree.
