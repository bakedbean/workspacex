# wsx

Terminal UI for managing Claude Code sessions in git worktrees.

## Quick start

```bash
cargo build --release
./target/release/wsx repo add /path/to/your/repo
./target/release/wsx              # launch TUI
```

Press `n` to create your first workspace, then `enter` to attach. Claude Code spawns inside the worktree.

## CLI reference

### Launch the TUI

```
wsx
```

Running with no arguments opens the dashboard.

### Repository management

```
wsx repo add <path> [--name <name>] [--prefix <prefix>]
```

Registers a git repository. `<path>` must be an existing git working tree.

- `--name <name>` — display name on the dashboard. Defaults to the directory basename.
- `--prefix <prefix>` — per-repo branch prefix override. **Usually omit this** and use the global `branch_prefix` setting instead. Setting both means the per-repo value wins.

```
wsx repo list
```

Lists registered repos with their paths.

```
wsx repo remove <name>
```

Removes a repo from the wsx registry. Does not delete the git repository on disk. Workspaces under the removed repo are also unregistered (but their worktrees remain on disk).

```
wsx repo set-prefix <name> <prefix>
```

Sets or changes the per-repo branch prefix override. Pass an empty string (`""`) to clear and fall back to the global setting.

```
wsx repo set-instructions <name> <value-or-@file>
```

Sets per-repo custom instructions appended to claude's system prompt for sessions in this repo. Pass `""` to clear. Use `@/path/to/file.md` to read the value from a file.

### Global settings

```
wsx config get <key>
wsx config set <key> <value-or-@file>
wsx config list
wsx config edit <key>          # opens $EDITOR (default: vi)
```

Known keys:

| Key | Effect |
|---|---|
| `branch_prefix` | Default branch prefix for repos with no per-repo override. Branches are named `<prefix>/<workspace>`. |
| `custom_instructions` | Free-text appended to claude's system prompt on every workspace spawn. |
| `nerd_fonts` | Render nerd-font glyphs in the dashboard. Default ON; set to `false` / `0` / `off` to disable. |
| `editor_cmd` | Command to run for `[e] edit` on the dashboard. Worktree path appended as final arg. Examples: `code`, `cursor`, `alacritty -e nvim`. |
| `terminal_cmd` | Command to run for `[t] terminal` on the dashboard. Spawned with cwd=worktree, no extra args. Examples: `alacritty`, `kitty`, `gnome-terminal`. |

Value sources:

- A literal string: `wsx config set branch_prefix bakedbean`
- A file (prefix with `@`): `wsx config set custom_instructions @./instructions.md`
- Empty (clears): `wsx config set custom_instructions ""`

`wsx config edit <key>` opens `$EDITOR` on a tempfile prepopulated with the current value; saving updates the setting. Useful for multi-line `custom_instructions`.

## Keybindings

### Dashboard

| Key | Action |
|---|---|
| `Up` / `Down` | Move selection through repo headers and workspaces |
| `enter` on a workspace | Attach to its claude session (spawns or resumes) |
| `enter` on a repo header | Open the New Workspace modal targeting that repo |
| `n` | New workspace in the selected row's repo |
| `e` | Open the selected workspace in your editor (no-op on repo header) |
| `t` | Open the selected workspace in a terminal (no-op on repo header) |
| `d` | Archive the selected workspace (no-op on repo header) |
| `q` | Quit (kills all running sessions) |

### New Workspace / Confirm Archive / Setup Running modals

| Key | Action |
|---|---|
| `enter` | Confirm |
| `esc` | Cancel |
| `y` / `n` | Confirm/cancel on ConfirmArchive |
| Printable chars / `backspace` | Edit the name field on NewWorkspace |

### Attached workspace

Keystrokes are forwarded to the running `claude` session, except:

| Key | Action |
|---|---|
| `Ctrl-a d` | Detach back to the dashboard (session keeps running) |
| `Ctrl-a a` | Send a literal `Ctrl-a` to claude |

## Editor and terminal integration

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

If neither the setting nor the env-var fallback is set, an error modal explains how to configure.

## Dashboard status indicators

| Symbol | Meaning |
|---|---|
| `●` | Session is running in this wsx process |
| `↻` | Resumable — a prior claude session exists for this worktree; attach to continue |
| `○` | No session ever started here |
| `✕` | Workspace state is `Failed` (worktree creation didn't succeed) |
| `[setup-failed]` badge | Setup script exited non-zero; workspace is otherwise usable |

Activity column for running sessions:

- `active` — output within the last 2 seconds
- `idle` — output within the last 30 seconds
- `waiting` — no output for over 30 seconds
- `off` — no current session
- `resumable` — prior session exists, not currently running

### Diff counts column

Compact summary of `git status` per workspace, refreshed every 2 seconds:

| Symbol (plain) | Symbol (nerd) | Meaning |
|---|---|---|
| `~N` |  `N` | Modified/staged/added/deleted tracked files |
| `?N` |  `N` | Untracked files |
| `↑N` | `N` | Commits ahead of upstream |
| `↓N` | `N` | Commits behind upstream |

Zero values omitted. Clean workspaces show nothing in this column.

## Auto-rename modes

After your first prompt in a freshly-created workspace, wsx renames the workspace + git branch based on the conversation. Controlled by `WSX_RENAME_MODE`:

| Mode | Behavior |
|---|---|
| `claude` (default) | Claude itself runs `git branch -m` as the first action in its response, based on your first message. A background poller propagates the rename to the wsx store. Higher-quality slugs at the cost of ~80 tokens per session start. |
| `local` | wsx intercepts your first prompt's keystrokes locally and slugifies them. Zero tokens; literal text. |
| `off` | No auto-rename. Workspaces keep their generated `<adjective>-<plant>` name forever. |

The rename only fires on workspaces whose name still matches the generated `<adjective>-<plant>` pattern.

## Environment variables

| Variable | Purpose |
|---|---|
| `WSX_RENAME_MODE` | Auto-rename mode: `claude` (default) / `local` / `off` |
| `WSX_CLAUDE_BIN` | Path to the `claude` binary (default: looked up via `PATH`). Used by tests to substitute `cat`. |
| `EDITOR` | Editor invoked by `wsx config edit` (default: `vi`) |
| `VISUAL` / `EDITOR` | Fallback when `editor_cmd` is unset |
| `TERMINAL` | Fallback when `terminal_cmd` is unset |
| `XDG_STATE_HOME` | Base for the wsx state directory (default: `~/.local/state`) |
| `RUST_LOG` | `tracing` filter (default: `info`); set `wsx=debug` for verbose logs |
| `HOME` | Fallback for resolving the state directory |

## Storage and configuration files

| Path | Contents |
|---|---|
| `$XDG_STATE_HOME/wsx/state.db` | SQLite database: repos, workspaces, settings |
| `$XDG_STATE_HOME/wsx/worktrees/<repo>/<workspace>/` | Worktree directories created by `wsx` |
| `$XDG_STATE_HOME/wsx/logs/wsx.log` | Daily-rotated `tracing` logs |
| `~/.claude/projects/<encoded-cwd>/<session>.jsonl` | Claude Code's own session files (wsx probes these to detect resumable workspaces) |

## Per-repo setup scripts

A `.claudette.json` file in the repo root is honored for setup and archive scripts that run when a workspace is created or removed:

```json
{
  "setup":   { "command": "bun",  "args": ["install"] },
  "archive": { "command": "rm",   "args": ["-rf", "node_modules"] }
}
```

The script runs with `cwd` set to the new worktree path and two extra env vars: `WSX_REPO_ROOT` (the source repo) and `WSX_WORKTREE` (the new worktree). Setup failure does not block the workspace from being usable; it's surfaced as a `[setup-failed]` badge on the dashboard.

## Testing

```bash
cargo test -- --test-threads=1
```

The test suite substitutes `claude` with `cat` via `WSX_CLAUDE_BIN`, so it runs without Claude Code installed. `--test-threads=1` is required because several tests mutate `WSX_CLAUDE_BIN` and `HOME`.
