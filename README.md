# wsx

Terminal UI for managing Claude Code sessions in git worktrees.

## Key features

- **Parallel Claude sessions in git worktrees** — every workspace is its own branch + worktree; switch with one key.
- **Cross-session attention alerts** — terminal bell + `!` marker when a session is awaiting permission or has gone idle.
- **Activity sub-line per workspace** — see the latest tool call or message from each session at a glance.
- **Project Manager pane** — a dedicated Claude session that summarizes what every workspace is for, where it's at, and what's next.
- **Remote control** — attach from claude.ai/code or the mobile app; or run wsx in tmux+ssh for full-fidelity desktop access.
- **Pinned commands** — define your `/pull-request`, `/feedback`, `/ultrareview` shortcuts once; fire them with `Ctrl-x <digit>` or a click while attached.
- **Related repos** — declare related wsx repos per primary repo; workspaces spawn with `--add-dir` for each and a read-only system prompt so claude can read but won't edit them.
- **Frictionless workflow** — auto-rename branches from your first prompt, per-repo setup/archive scripts, editor/terminal/diff hooks.

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

wsx repo set-name <name> <new-name>
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
wsx repo set-name <name> <new-name>
```

Renames the repo in the wsx registry. The new name appears on the dashboard and is used in workspace references (e.g. `wsx workspace create <repo>`). Other commands like `wsx repo set-prefix <new-name> ...` must use the new name afterwards.

```
wsx repo set-prefix <name> <prefix>
```

Sets or changes the per-repo branch prefix override. Pass an empty string (`""`) to clear and fall back to the global setting.

```
wsx repo set-instructions <name> <value-or-@file>
```

Sets per-repo custom instructions appended to claude's system prompt for sessions in this repo. Pass `""` to clear. Use `@/path/to/file.md` to read the value from a file.

```
wsx repo set-pinned-commands <name> <value-or-@file>
wsx repo edit-pinned-commands <name>
```

Per-repo override of `pinned_commands`. Empty value clears the override; resolution then falls back to the global setting.

```
wsx repo set-related-repos <name> <value-or-@file>
wsx repo edit-related-repos <name>
```

Per-repo list of other wsx-registered repos that workspaces in this repo should reference. Comma-separated names (e.g. `frontend,marketing`). At spawn time wsx looks each name up in the repo registry and passes `--add-dir <source-path>` to claude. Unknown names are silently skipped (logged at `warn` level — visible with `RUST_LOG=wsx=warn` or any less-specific filter).

### Workspace management

```
wsx workspace create <repo> [--name <slug>] [--yolo]
```

Creates a workspace in `<repo>`, equivalent to the dashboard's `[n]` keybind. `<slug>` is a kebab-case workspace name; the resulting git branch is `<branch_prefix>/<slug>`. When `--name` is omitted, an adjective-noun slug like `merry-birch` is generated. `--yolo` skips the permission prompts in the spawned Claude session.

```
wsx workspace list [<repo>]
```

Lists workspaces as tab-separated `repo<TAB>slug<TAB>branch<TAB>worktree_path` rows. Pass a repo name to filter.

```
wsx workspace path <repo> <slug>
```

Prints just the worktree path. Designed for `cd "$(wsx workspace path backend my-slug)"`.

```
wsx workspace rename <repo> <old-slug> <new-slug>
```

Renames the workspace slug AND its git branch in sync with the wsx database. Using `git branch -m` directly leaves wsx's DB stale.

```
wsx workspace archive <repo> <slug> [--keep-worktree] [--force-delete-branch]
```

Equivalent to the dashboard's archive action: runs the per-repo archive script, removes the worktree (unless `--keep-worktree`), deletes the branch (force if `--force-delete-branch`), and drops the workspace from the registry.

### Claude Code skill

```
wsx setup install-skill
```

Writes the bundled wsx Claude Code skill to `~/.claude/skills/wsx/SKILL.md`. The skill teaches Claude how to drive the wsx CLI — workspace operations, slug-vs-`branch_prefix` naming, and the cross-repo orchestration flow that pairs with [Related repos](#related-repos). The file is embedded in the binary at compile time, so installing wsx on a new machine is `cargo install` then `wsx setup install-skill`.

Idempotent: re-running when the installed copy already matches reports "already up to date" without writing. If the installed copy has drifted (you edited it locally, or you're upgrading wsx with skill changes), it's overwritten and reports "updated".

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
| `pm_fast_mode` | Launch the Project Manager session with Claude Code's fast mode enabled (`--settings '{"fastMode":true}'`). PM is a status-summary session, so fast output is usually the right tradeoff. Default OFF; set to `on` / `true` / `1` / `yes` to enable. |
| `mcp_mirror` | Inherit MCP servers from the source repo into worktrees (see [MCP server inheritance](#mcp-server-inheritance)). Default ON; set to `off` / `false` / `0` / `no` to disable. |
| `remote_control` | Pass `--remote-control` to claude on every spawn so the session is reachable via [claude.ai/code](https://claude.ai/code) and the Claude mobile app (see [Remote control](#remote-control)). Default ON; set to `off` / `false` / `0` / `no` to disable. |
| `remote_control_sandbox` | When `remote_control` is on, also pass `--sandbox` for an extra safety wrapper on remote-issued commands. Default OFF; set to `on` / `true` / `1` / `yes` to enable. |
| `pinned_commands` | Newline-separated list of `Label=command` (or bare `command`) entries. Each becomes a chip in the attached view, fired via `Ctrl-x <digit>` or click. Max 9 visible/keyable. Per-repo override available via `wsx repo set-pinned-commands`. |
| `dashboard_name_width` | Width (chars) of the workspace-name column on the dashboard. Default `24`. Clamped to `10..=60`. |
| `dashboard_branch_width` | Width (chars) of the `⎇ branch` column on the dashboard. Default `28`. Clamped to `10..=80`. |
| `detail_bar_config` | JSON blob controlling the per-workspace detail bar (visibility, height, and the container/module layout). See [Workspace detail bar](#workspace-detail-bar) for the schema, defaults, and per-repo override flow. Out-of-range values are clamped on save. |

Value sources:

- A literal string: `wsx config set branch_prefix bakedbean`
- A file (prefix with `@`): `wsx config set custom_instructions @./instructions.md`
- Empty (clears): `wsx config set custom_instructions ""`

`wsx config edit <key>` opens `$EDITOR` on a tempfile prepopulated with the current value; saving updates the setting. Useful for multi-line `custom_instructions`.

## Keybindings

### Dashboard

| Key | Action |
|---|---|
| `Up` / `Down` (or `k` / `j`) | Move selection through repo headers and workspaces |
| `h` / `l` | Fold / unfold the focused repo (idempotent; use `zz` to toggle) |
| `enter` (or `i`) on a workspace | Attach to its claude session (spawns or resumes) |
| `enter` (or `i`) on a repo header | Open the New Workspace modal targeting that repo |
| `n` | New workspace in the selected row's repo |
| `e` | Open the selected workspace in your editor (no-op on repo header) |
| `t` | Open the selected workspace in a terminal (no-op on repo header) |
| `v` | View diff of the selected workspace's branch vs the repo's base branch (auto-detected; no-op on repo header) |
| `K` | Show processes running under the selected workspace's worktree (no-op on repo header) |
| `s` | Open repo settings modal for the selected repo (or the parent repo when a workspace is selected) |
| `d` | Archive the selected workspace (no-op on repo header) |
| `q` | Quit (kills all running sessions) |
| `p` | Toggle the Project Manager pane (no-op when `pm_enabled` is off) |
| `Tab` | Swap focus between dashboard and the PM pane (when visible) |
| `z z` | Toggle fold on the focused repo |
| `z a` | Expand all repos (override default-fold heuristic) |
| `z M` | Fold all repos |
| `r` (when PM focused) | Refresh `workspaces.json` and ask PM to re-summarize |
| `Ctrl-O` (when PM focused) | Expand PM to full screen (use `Ctrl-x d` to detach back) |

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
| `Ctrl-x d` | Close the focused pane. When only one pane is open, detaches back to the dashboard (session keeps running). |
| `Ctrl-x ←/→/↑/↓` | Move focus between split panes in that direction (vim's `Ctrl-w` motions). |
| `Ctrl-x u` | Open the floating updates panel (shows other workspaces' state; supports `v`/`s` to open in a split) |
| `Ctrl-x e` | Open the attached workspace in your editor (same `editor_cmd` as `[e]` on the dashboard) |
| `Ctrl-x t` | Open the attached workspace in a terminal (same `terminal_cmd` as `[t]`) |
| `Ctrl-x v` | View diff of the attached workspace's branch vs the base branch (same `diff_cmd` as `[v]`) |
| `Ctrl-x k` | Show processes running under the attached workspace's worktree |
| `Ctrl-x x` | Send a literal `Ctrl-x` to claude |

#### Pinned commands

If `pinned_commands` is configured (globally or per-repo), a one-row chip strip appears between the claude pane and the footer. Each chip shows `[N] Label`:

```
[1] PR   [2] FB   [3] /loop /baby…   [4] UR
```

Fire a chip with `Ctrl-x <digit>` (1-9) or by clicking on it. The chip's command + `\r` is written to claude exactly as if you'd typed and submitted it.

Configure via the standard config CLI:

```bash
wsx config edit pinned_commands               # opens $EDITOR on the current value
wsx config set pinned_commands @./pinned.txt  # load from a file
wsx config set pinned_commands ""             # clear
```

One entry per line:

```
PR=/pull-request
FB=/feedback
/loop /babysit-prs
UR=/ultrareview
```

`Label=command` shows the label as the chip; a bare line uses the command itself (truncated past 12 columns). Both sides of `=` are trimmed.

At narrow terminal widths trailing chips drop from view; their keyboard shortcuts still work.

#### Mouse, scrollback, and text selection

wsx enables terminal mouse capture so the trackpad / wheel scrolls
through the session's history (instead of getting translated into
arrow keys that claude reads as prompt-history navigation). One
consequence: native click-and-drag selection no longer works by
default.

To select text from the claude pane, **hold Shift while
dragging** — most modern terminals (Alacritty, Kitty, WezTerm,
iTerm2, GNOME Terminal) bypass mouse capture under Shift and fall
back to OS-native selection. iTerm2 also supports right-click →
"Bypass mouse reporting", and macOS terminals often accept Option
as the modifier instead of Shift.

## Editor, terminal, and diff integration

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

Examples (note the **three dots** — this anchors the diff at the merge base, so stale local `main` doesn't bleed extra commits into the diff):

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

## Remote access

Running wsx on one machine (e.g. your desktop) and attaching from another (e.g. a laptop) works cleanly with tmux + ssh — no wsx-specific networking required.

**On the host machine:**

```
tmux new -As wsx 'wsx'
```

This starts wsx inside a tmux session named `wsx` (or reattaches to it if one already exists).

**From any other machine:**

```
ssh desktop -t tmux attach -t wsx
```

Workspaces — and the claude sessions running inside them — keep running while you're detached, so picking up where you left off from a different machine just works.

**Notes:**

- wsx's leader key is `Ctrl-x`, chosen specifically to not collide with tmux's default `Ctrl-b` prefix (or anyone's `Ctrl-a` customization). No tmux config needed.
- **Mosh** drops in cleanly if your network is flaky: `mosh desktop -- tmux attach -t wsx`.
- **Tailscale** (or any VPN) makes the host reachable from anywhere by a stable name without port-forwarding.

## Process tracking

`[k]` on the dashboard (or `Ctrl-x k` while attached) shows long-running
processes whose current working directory is inside the selected
workspace's worktree — dev servers, watchers, anything you started in
that worktree from a terminal. Workspaces with detected processes show
a `~N` count between the branch and activity columns on the dashboard.

The modal lists each process's PID, command, and full cwd:

    ─── Processes — fix-bug ──────
      PID    COMMAND          CWD
      12345  npm              /home/user/wt/fix-bug
      12389  pytest           /home/user/wt/fix-bug/tests
    ─────────────────────────────
    [↑/↓] move   [k] term   [K] kill   [esc] close

`k` sends `SIGTERM` to the highlighted process; `K` sends `SIGKILL`.
After either, wsx immediately re-scans so the list reflects the new
state.

**Notes:**

- Detection runs once every 10 seconds in the background via `lsof -d cwd`.
- Shells and editors (bash, zsh, nvim, code, etc.) are filtered out so the
  count surfaces what's interesting — your dev server, not the terminal
  hosting it.
- wsx never starts these processes itself. Launch them however you
  like (the `[t]` terminal keybind is one option). The feature is
  observability plus a kill hook, not lifecycle management.
- Requires `lsof` to be installed (standard on most Linux/macOS setups).
  If it's missing, the count stays at 0 and the modal shows "(no tracked
  processes)" — no errors.

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

## Workspace detail bar

When a workspace is selected on the dashboard, wsx renders a multi-column
detail bar across the bottom. The body is divided into 1–4 equal-width
**containers**; each container holds one or more **modules** stacked
vertically. Four built-in modules ship today: `session_summary`,
`recent_chat`, `processes`, `recent_files`. The bar's appearance is
controlled by the `detail_bar_config` setting — globally via `wsx config`,
with optional per-repo overrides.

### Schema and defaults

The global value is a full `DetailBarConfig` JSON blob. Every field is
optional; missing fields fall back to defaults. Out-of-range values are
clamped on save (see below).

```json
{
  "visible": true,
  "height": {
    "percent": 30,
    "min_rows": 8,
    "max_rows": 18
  },
  "containers": [
    ["session_summary"],
    ["recent_chat"],
    ["processes", "recent_files"]
  ]
}
```

| Field | Type | Default | Effect |
|---|---|---|---|
| `visible` | bool | `true` | Master toggle. When `false`, the bar is hidden entirely and `Tab` skips the reply input. |
| `height.percent` | u8 | `30` | Target height as a percent of the terminal's rows. Clamped to `[5, 80]`. |
| `height.min_rows` | u16 | `8` | Floor on the bar's height. Clamped to `[4, 40]`. |
| `height.max_rows` | u16 | `18` | Ceiling on the bar's height. Clamped to `[4, 60]`. If `min_rows > max_rows`, the two are swapped on save. |
| `containers` | list of lists | (see default above) | Outer length 1–4: one entry per equal-width column. Inner is a list of module IDs stacked vertically within the column. An empty inner list `[]` reserves an empty column. Empty outer list resets to default. Lengths > 4 are truncated to 4. |

**Built-in module IDs:** `session_summary`, `recent_chat`, `processes`,
`recent_files`. Unknown IDs render a `[unknown: <id>]` placeholder and
log a warning, so typos are visible but don't break the dashboard.

When every container is empty (`[[], [], []]`), the bar shrinks to its
4-row chrome (header + two rules + reply input) regardless of
`height.percent`. That's how you trim the bar to just the reply input.

### Setting the global value

```bash
wsx config edit detail_bar_config     # opens $EDITOR; seeded with the pretty-printed default
wsx config set  detail_bar_config '{"height": {"percent": 50}}'
wsx config get  detail_bar_config
wsx config set  detail_bar_config ""  # clear (reverts to baked-in defaults)
```

Partial JSON is fine — `{"visible": false}` is a complete, valid value.
Missing fields are filled in from defaults. Malformed JSON is rejected
with a non-zero exit and the previous value is preserved.

Examples:

```bash
# Make the bar taller on big monitors.
wsx config set detail_bar_config '{"height": {"percent": 45, "max_rows": 24}}'

# Single full-width chat column.
wsx config set detail_bar_config '{"containers": [["recent_chat"]]}'

# Four columns, processes and files in separate slots.
wsx config set detail_bar_config '{"containers": [["session_summary"], ["recent_chat"], ["processes"], ["recent_files"]]}'

# Hide the bar entirely.
wsx config set detail_bar_config '{"visible": false}'
```

### Per-repo override

Each repo can override any subset of the global config. The per-repo
value is a `DetailBarOverride` — `visible` and `height.*` merge
per-field; `containers` is whole-replace when present, fully-inherited
when absent. An empty `{}` inherits everything; you only specify what
you want to change.

Open the repo settings modal with `s` on the dashboard, select the
`detail_bar_config` row, and press Enter. `$EDITOR` opens on `{}\n`
(or the current override). Save to apply; press `d` on the row to
clear the override and fall back to the global value.

Override examples:

Hide the bar entirely for this repo (global value can stay on):

```json
{"visible": false}
```

Single chat column for this repo; keep `visible` and `height` inherited from global:

```json
{"containers": [["recent_chat"]]}
```

Taller bar for a repo where the session-summary text is usually long
(CLI tools with verbose tool-call traces):

```json
{"height": {"percent": 45, "max_rows": 28}}
```

Merge precedence: bake-in defaults → global `detail_bar_config` →
per-repo override. `visible` and `height.*` apply per-field; `containers`
whole-replaces when the override sets it. So a repo override that only
sets `containers` still picks up any global `height` changes you make
later.

### Behavior on bad input

- Malformed JSON at the global level — falls back to baked-in defaults at runtime, logged at `warn`.
- Malformed JSON in a repo override — the override is ignored; the global value applies, logged at `warn` with the repo name.
- Out-of-range `height.percent` / `min_rows` / `max_rows` — clamped to legal ranges on save (`wsx config set/edit`) and again at runtime as a defense-in-depth.
- `min_rows > max_rows` — swapped on save so the lower bound is the floor and the higher is the ceiling.

## Workspace updates panel

When you're attached to a workspace (full-screen claude session) or the
project manager pane is expanded full-screen, wsx still tracks the other
workspaces in the background. Two affordances surface that:

- A single-row status indicator above the footer, shown only when another
  workspace needs attention or has produced output in the last 60 seconds.
  Format: `⚠ <name> awaiting permission: <tool> (<age>)` for attention,
  `● <name>: <event> (<age>)` for activity. The row collapses to nothing
  when there's nothing to surface, giving claude the row back.

- A floating panel via `Ctrl-x u` listing ALL workspaces grouped by repo,
  with their current state and latest event. Press `Esc` to close. The
  panel re-renders live, so ages count up and attention flags appear/clear
  in real time.

  From the panel, the selected workspace can be opened three ways:

  | Key | Action |
  |---|---|
  | `Up` / `Down` (or `k` / `j`) | Move selection within the panel. |
  | `Enter` | Switch the current pane to the selected workspace (replaces it). |
  | `v` | Open the selected workspace in a vertical split (panes side by side, vim's `:vsplit`). |
  | `s` | Open the selected workspace in a horizontal split (panes stacked, vim's `:split`). |

## Split panes

Multiple workspace PTYs can be tiled in the attached view, vim-style. Any
pane can be split again — recursively — into a tree of vertical and
horizontal splits. Each pane shows a 1-line title bar with the workspace
name and a `●` marker on the focused pane (which receives keystrokes).

The flow:

1. Attach to a workspace as usual (`Enter` on the dashboard).
2. Press `Ctrl-x u` to open the updates panel.
3. Move to another workspace; press `v` (vertical) or `s` (horizontal) to
   add it as a new pane alongside the current one. Focus jumps to the
   new pane.
4. Navigate between panes with `Ctrl-x ←/→/↑/↓` — direction-aware
   walking up the split tree, like vim's `Ctrl-w` motions.
5. Close the focused pane with `Ctrl-x d`. The other panes keep
   running; when the last pane closes you detach back to the dashboard.

When you split the *focused* pane again in the same direction as its
parent, the new pane is inserted as a sibling instead of nesting deeper —
matches vim and keeps the tree shallow.

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
can scroll through claude's history naturally; `Ctrl-x d` detaches back
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

Each script is executed as `$SHELL -ilc "$value"` (interactive + login) with `cwd` set to the new worktree and two extra env vars: `WSX_REPO_ROOT` (the source repo) and `WSX_WORKTREE` (the new worktree). Running as a login + interactive shell means your `~/.zprofile` and `~/.zshrc` (or bash equivalents) are sourced first, so tools activated there — `mise`, `direnv`, `asdf`, aliases — are available to the script. If `$SHELL` is unset, empty, or points at a POSIX-only shell (`sh`, `dash`, `ash`) that doesn't support `-l`, wsx falls back to `/bin/bash`. Setup failure does not block the workspace from being usable; it's surfaced as a `[setup-failed]` badge on the dashboard. Passing an empty value clears the script.

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
- `detail_bar_config` (see [Workspace detail bar](#workspace-detail-bar))

`↑/↓` selects a field. Press `Enter` to edit — wsx temporarily leaves
the TUI, opens `$EDITOR` (or `vi` if unset) on a tempfile prepopulated
with the current value, and saves whatever you write when the editor
exits. Press `d` to clear the highlighted field. `Esc` closes.

The editor needs to be a terminal-native editor that returns when you
quit (vim, nvim, helix, micro, nano). GUI editors that return
immediately without a `--wait` flag will appear to "save nothing" —
keep `$EDITOR` pointed at a CLI editor for this flow.

## Related repos

When you work across multiple repos that need to know about each other (a backend, a frontend, a marketing site), declare related repos per primary repo:

```bash
wsx repo set-related-repos backend frontend,marketing
```

When you spawn a workspace in `backend`, wsx invokes claude with `--add-dir` pointing at each related repo's source path. Claude can read, grep, and reference files in those directories freely.

To prevent claude from accidentally editing files in the source paths of related repos (which would land changes on whatever branch the source is on), wsx also appends a system-prompt instruction telling claude:

- Treat those directories as read-only.
- If changes are needed there, drive `wsx workspace create <other-repo> --name <slug>` from this session, `cd` into the new worktree path (`wsx workspace path <other-repo> <slug>`), and make the changes there. Each repo gets its own branch and PR; cross-link them and merge in dependency order.

This is a soft guard, not a tool-level lock — it relies on claude following the instruction. The same trust model as `custom_instructions`. Installing the bundled wsx skill (`wsx setup install-skill`, see [Claude Code skill](#claude-code-skill)) reinforces this with the full CLI vocabulary and slug-naming rules.

Unknown names in the list (e.g. a repo you renamed or unregistered) are logged and skipped at spawn time; the spawn still proceeds with the recognized names.

## MCP server inheritance

Claude Code stores MCP server config in `~/.claude.json` under
`projects.<absolute_cwd_path>.mcpServers`. The lookup is keyed on the
literal cwd path at launch time. Because wsx launches claude inside a
worktree path (under `~/.local/state/wsx/worktrees/...`), the source
repo's MCP servers aren't visible by default — claude looks up the
worktree path, finds no entry, and runs without those servers.

wsx mirrors the source repo's `mcpServers` into the worktree's project
entry every time a workspace session spawns. New servers added to the
source repo via `claude mcp add ...` show up in workspaces on the next
attach.

On `wsx workspace archive`, wsx removes the worktree's
`projects[<worktree_path>]` entry from `~/.claude.json` to keep it
tidy.

**Secrets**: MCP server configs frequently include API tokens and
other credentials. Mirroring copies them verbatim into the worktree
entry. This is the same file with the same permissions, but it does
mean the same secret is now keyed under two paths.

**Toggle**: this behavior is on by default. Disable it with:

```bash
wsx config set mcp_mirror false
```

With it disabled, wsx never reads or writes `~/.claude.json`. You can
still configure MCP servers per-workspace by running `claude mcp add
...` while attached.

## Remote control

Claude Code's `--remote-control` flag exposes a running session to
[claude.ai/code](https://claude.ai/code) and the Claude iOS/Android
apps. The local PTY behavior is unchanged — claude prints a session
URL and a QR code at startup that you can scan from your phone or
open in a browser to attach remotely.

wsx passes `--remote-control` to every claude spawn (workspaces and
the PM pane) by default, so any session is reachable from your phone
without extra setup.

**Toggle**: disable with `wsx config set remote_control false`. With
it off, sessions are local-only and nothing is sent to Anthropic's
relay servers.

**Sandbox**: claude offers `--sandbox` as an extra safety wrapper for
remote-issued commands. Disabled by default in wsx; enable with
`wsx config set remote_control_sandbox true`.

**Auth**: the relay rides on your claude.ai account. If you're not
signed in or you're offline, the local session continues to work and
the remote relay just fails silently.

**Privacy**: enabling remote control routes session state through
Anthropic's relay infrastructure. The session URL emitted in the PTY
is also visible to anyone seeing your screen.

## Testing

```bash
cargo test -- --test-threads=1
```

The test suite substitutes `claude` with `cat` via `WSX_CLAUDE_BIN`, so it runs without Claude Code installed. `--test-threads=1` is required because several tests mutate `WSX_CLAUDE_BIN` and `HOME`.
