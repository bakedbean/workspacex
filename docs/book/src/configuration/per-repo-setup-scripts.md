Each repo can have a `setup` script (run when a workspace is created) and an `archive` script (run when a workspace is removed). Both are stored in the wsx state database and configured per-repo via the CLI:

```bash
wsx repo set-setup    <repo-name> 'bun install'
wsx repo set-archive  <repo-name> 'rm -rf node_modules'
```

For multi-line scripts, pass a file with the `@` prefix or open `$EDITOR`:

```bash
wsx repo set-setup    <repo-name> @./scripts/setup.sh
wsx repo edit-setup   <repo-name>
wsx repo edit-archive <repo-name>
```

Each script is executed as `$SHELL -ilc "$value"` (interactive + login) with `cwd` set to the new worktree and two extra env vars: `WSX_REPO_ROOT` (the source repo) and `WSX_WORKTREE` (the new worktree). Running as a login + interactive shell means your `~/.zprofile` and `~/.zshrc` (or bash equivalents) are sourced first, so tools activated there — `mise`, `direnv`, `asdf`, aliases — are available to the script. If `$SHELL` is unset, empty, or points at a POSIX-only shell (`sh`, `dash`, `ash`) that doesn't support `-l`, wsx falls back to `/bin/bash`. Setup failure does not block the workspace from being usable; it's surfaced as a `[setup-failed]` badge on the dashboard. When you create a workspace from the dashboard, the script's output is captured to `~/.local/state/wsx/logs/setup-<repo>-<name>.log` (overwritten on each run) — check it when a workspace shows `[setup-failed]` to see what went wrong. Passing an empty value clears the script.

### Editing in the TUI

Press `s` on any dashboard row to open the Repo settings modal for that
row's repo. The modal lists the per-repo fields:

- `name`
- `branch_prefix`
- `base_branch`
- `custom_instructions`
- `setup_script`
- `archive_script`
- `pinned_commands`
- `related_repos`
- `detail_bar_config` (see [Workspace detail bar](../daily-use/detail-bar.md))
- `chronology_config` (see [Change chronology](change-chronology.md))

`↑/↓` selects a field. Press `Enter` to edit — wsx temporarily leaves
the TUI, opens `$EDITOR` (or `vi` if unset) on a tempfile prepopulated
with the current value, and saves whatever you write when the editor
exits. Press `d` to clear the highlighted field. `Esc` closes.

The editor needs to be a terminal-native editor that returns when you
quit (vim, nvim, helix, micro, nano). GUI editors that return
immediately without a `--wait` flag will appear to "save nothing" —
keep `$EDITOR` pointed at a CLI editor for this flow.
