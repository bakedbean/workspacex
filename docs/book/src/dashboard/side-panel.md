# Side panel

The side panel shows the selected workspace's branch in more depth, on the right of the screen, one tab at a time. It's available on the dashboard list and in the [repo focus view](repo-focus.md).

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

Press `1` for Commits and `2` for Files, or click a tab; either opens the panel if it's closed. The mouse wheel over the panel scrolls it. `c` or `esc` closes it, and it remembers its tab. With a repo header or a hidden group selected, the panel asks you to select a workspace.

The panel needs the repo's **Base branch** setting (repo settings, `s`) to know what to compare against. After you change that setting, the panel shows `loading…` until the lists against the new base arrive (within about ten seconds); it never shows the old base's lists in their place. The panel stays hidden on terminals under 100 columns wide.
