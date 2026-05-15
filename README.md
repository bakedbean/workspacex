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
| `editor_cmd` | Command to run for `[e] edit` on the dashboard. Worktree path appended as final arg unless the command contains `{path}` (substituted in place). Examples: `code`, `cursor`, `alacritty -e nvim`, `xdg-terminal-exec --dir={path} nvim`. |
| `terminal_cmd` | Command to run for `[t] terminal` on the dashboard. Spawned with cwd=worktree; `{path}` substituted in place if present. Examples: `alacritty`, `kitty`, `gnome-terminal`. |
| `notifications` | Ring the terminal bell and show a `!` marker when a workspace transitions to `waiting` (claude paused for ≥30s). Default ON; set to `off` / `false` / `0` / `no` to disable. |
| `theme` | Color theme. One of `default` (palette-adaptive ANSI), `dracula` (RGB), `jellybeans` (RGB), `nord` (RGB). Unknown values fall back to `default`. Restart wsx after changing. |
| `pm_enabled` | Enable the Project Manager pane (`p` keybind). Default ON; set to `off` / `false` / `0` / `no` to disable. |
| `pm_custom_instructions` | Free-text appended to the project manager's system prompt. Same `@file` / empty-clears semantics as `custom_instructions`. |

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
| `p` | Toggle the Project Manager pane (no-op when `pm_enabled` is off) |
| `Tab` | Swap focus between dashboard and the PM pane (when visible) |
| `r` (when PM focused) | Refresh `workspaces.json` and ask PM to re-summarize |
| `Ctrl-O` (when PM focused) | Expand PM to full screen (use `Ctrl-a d` to detach back) |

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
| `Ctrl-a u` | Open the floating updates panel (shows other workspaces' state) |
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

### Activity sub-line

Below each workspace row, wsx shows the most recent event from claude's
session log (tailed from `~/.claude/projects/<encoded-cwd>/`):

```
  ● fix-bug    bakedbean/fix-bug   ~3 ?1   active
    └ ran `cargo test --workspace` (3s ago)
```

The sub-line updates on the 2-second poll tick. Workspaces with no
claude session yet show no sub-line. Recognized events:

- User message → `user: <text>`
- Assistant text → `<text>`
- Assistant tool use (Bash) → ``ran `<command>` ``
- Assistant tool use (other) → `using <ToolName>`

Lines longer than ~70 characters are truncated with an ellipsis.

### Diff counts column

Compact summary of `git status` per workspace, refreshed every 2 seconds:

| Symbol (plain) | Symbol (nerd) | Meaning |
|---|---|---|
| `~N` |  `N` | Modified/staged/added/deleted tracked files |
| `?N` |  `N` | Untracked files |
| `↑N` | `N` | Commits ahead of upstream |
| `↓N` | `N` | Commits behind upstream |

Zero values omitted. Clean workspaces show nothing in this column.

### Attention alerts

wsx watches each workspace for two distinct "user needs to act" signals:

- A `tool_use` event in the session log has been pending for ≥3 seconds —
  almost always means claude is showing a permission prompt for a tool.
  In this case the activity column reads `awaiting` and the sub-line shows
  `└ ⚠ awaiting permission: <tool> (<age>)`.
- The claude session has gone ≥30 seconds without producing PTY output
  (state flips from `active` or `idle` to `waiting`).

On either transition wsx considers the workspace to need attention:

- A terminal bell (`\x07`) is written to stdout. Your terminal config decides
  whether to beep, flash, or ignore.
- A `!` marker appears at the start of the workspace's row on the dashboard.

The marker clears the moment you attach to the workspace (Enter on the row).
The first observation of any workspace establishes a baseline; no bell rings
for workspaces that are already in `waiting` or `awaiting` state when wsx
launches.

Turn off both via `wsx config set notifications off`.

## Workspace updates panel

When you're attached to a workspace (full-screen claude session) or the
project manager pane is expanded full-screen, wsx still tracks the other
workspaces in the background. Two affordances surface that:

- A single-row status indicator above the footer, shown only when another
  workspace needs attention or has produced output in the last 60 seconds.
  Format: `⚠ <name> awaiting permission: <tool> (<age>)` for attention,
  `● <name>: <event> (<age>)` for activity. The row collapses to nothing
  when there's nothing to surface, giving claude the row back.

- A floating panel via `Ctrl-a u` listing ALL workspaces grouped by repo,
  with their current state and latest event. Press `Esc` to close. The
  panel re-renders live, so ages count up and attention flags appear/clear
  in real time.

## Themes

Pick a color theme with:

```
wsx config set theme dracula
wsx config set theme jellybeans
wsx config set theme nord
wsx config set theme default
```

Themes affect repo headers, the selected row, sub-line dimming, and the
error modal. The state indicators (status dots, activity labels, attention
marks) are not yet per-state coloured — that's a planned follow-up.

The `default` theme uses ANSI-named colors that adapt to your terminal's
palette. `dracula`, `jellybeans`, and `nord` are fixed RGB palettes.

Restart wsx after changing — themes are loaded once at startup.

## Auto-rename modes

After your first prompt in a freshly-created workspace, wsx renames the workspace + git branch based on the conversation. Controlled by `WSX_RENAME_MODE`:

| Mode | Behavior |
|---|---|
| `claude` (default) | Claude itself runs `git branch -m` as the first action in its response, based on your first message. A background poller propagates the rename to the wsx store. Higher-quality slugs at the cost of ~80 tokens per session start. |
| `local` | wsx intercepts your first prompt's keystrokes locally and slugifies them. Zero tokens; literal text. |
| `off` | No auto-rename. Workspaces keep their generated `<adjective>-<plant>` name forever. |

The rename only fires on workspaces whose name still matches the generated `<adjective>-<plant>` pattern.

## Project manager pane

Press `p` on the dashboard to open a horizontal pane below the workspace list
hosting a dedicated Claude Code "project manager" session. PM's job is to
answer three questions about each of your active workspaces:

- What was this workspace created for?
- Where have things been left off?
- What's next to close it out?

`p` opens the pane and focuses it immediately — keystrokes go to PM (like
the attached view). `Tab` or `Esc` swaps focus back to the dashboard;
`Tab` from the dashboard swaps back into the PM pane. `r` (while PM is
focused) refreshes `workspaces.json` and asks PM to re-summarize. `Ctrl-O`
(while PM is focused) expands PM to a full-screen attached view so you
can scroll through claude's history naturally; `Ctrl-a d` detaches back
to the dashboard with the pane state preserved.

PM only summarizes workspaces where claude has been started at least once
(i.e., a session log exists under `~/.claude/projects/...`). Workspaces
you created but never opened are skipped — nothing for PM to report on.

PM lives at `$XDG_STATE_HOME/wsx/project-manager/` and persists across wsx
restarts via Claude Code's `--continue`. On the first `p` of a wsx run with
no prior PM session, wsx auto-sends a status-summary request (and submits
it for you). On subsequent runs (resuming via `--continue`), wsx stays
silent — type your own question or press `r` for a fresh summary.

PM only sees workspaces wsx knows about (registered repos and their `Ready`
workspaces). PM runs with `--dangerously-skip-permissions` so its tool
calls don't prompt you — convenient for an inspection-only sidekick, but
note that PM can technically write/edit files (the system prompt steers
it toward read-only inspection but doesn't enforce it). If you don't want
that, disable PM entirely.

Disable the feature with `wsx config set pm_enabled off`.
Customize PM's behavior with `wsx config set pm_custom_instructions @./pm.md`.

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
| `$XDG_STATE_HOME/wsx/project-manager/` | PM Claude Code session cwd; contains `workspaces.json` and PM's own git init. Auto-created on first `p`. |
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
