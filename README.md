# wsx (WorkspaceX)

Terminal UI for managing Claude Code, Pi, Hermes, or Codex sessions in git worktrees.

> **A note on this project.** wsx approaches git worktree based agent development from a hands on perspective, choosing to delegate to other tools rather than be an all in one IDE. wsx does not attempt to deliver it's own agent interface.  Rather wsx simply loads the agent TUI of choice with a thin CLI wrapper around it.  Many of wsx's features are only accessible from the CLI and empower the user to define their own behaviors. If you want the power of multi agent worktree based development with an orchestration CLI, prefer working in the terminal, want to use your own tools like Neovim, Emacs and Lazygit, then this might just be something you’d enjoy using too.  Feedback, ideas, and contributions are welcome.


## Parallel Agent Sessions
### Deploy multiple workspaces at once all working in parallel with real time feedback 
https://github.com/user-attachments/assets/17962906-abde-4589-81e1-58737212645b

## Multi Agent Sessions
### Deploy multiple agents to the same workspace
https://github.com/user-attachments/assets/2023a76b-334b-415e-bc70-059ee8fee661

## Table of contents

- [Overview](#overview)
  - [Key features](#key-features)
  - [Quick start](#quick-start)
- [Daily use](#daily-use)
  - [Keybindings](#keybindings)
  - [Pinned commands](#pinned-commands)
  - [Mouse, scrollback, and text selection](#mouse-scrollback-and-text-selection)
  - [Dashboard status indicators](#dashboard-status-indicators)
  - [Process tracking](#process-tracking)
  - [Workspace detail bar](#workspace-detail-bar)
  - [Workspace updates panel](#workspace-updates-panel)
  - [Split panes](#split-panes)
  - [Project manager pane](#project-manager-pane)
- [Configuration and customization](#configuration-and-customization)
  - [Global settings](#global-settings)
  - [Themes](#themes)
  - [Auto-rename modes](#auto-rename-modes)
  - [Change chronology](#change-chronology)
  - [Coding agents](#coding-agents)
  - [Multi-agent workspaces](#multi-agent-workspaces)
  - [Per-repo setup scripts](#per-repo-setup-scripts)
- [Integrations and remote access](#integrations-and-remote-access)
  - [Editor, terminal, and diff integration](#editor-terminal-and-diff-integration)
  - [Remote access](#remote-access)
  - [Remote control](#remote-control)
  - [Named remote shortcuts](#named-remote-shortcuts)
  - [MCP server inheritance](#mcp-server-inheritance)
  - [Related repos](#related-repos)
  - [Agent skill](#agent-skill)
- [CLI reference](#cli-reference)
  - [Launch the TUI](#launch-the-tui)
  - [Repository management](#repository-management)
  - [Workspace management](#workspace-management)
- [Reference](#reference)
  - [Environment variables](#environment-variables)
  - [Storage and configuration files](#storage-and-configuration-files)
- [Development](#development)
  - [Testing](#testing)

## Overview

### Key features

- **Parallel agent sessions in git worktrees**: every workspace is its own branch + worktree; switch with one key.
- **Multiple coding agents**: run Claude, Pi, Hermes, or Codex per workspace. Set a global default with `coding_agent` or override per workspace with `--agent`. See [Coding agents](#coding-agents).
- **Multi-agent workspaces**: attach several agents to one worktree, switch focus with a keypress, and have them message each other. See [Multi-agent workspaces](#multi-agent-workspaces).
- **Cross-session attention alerts**: terminal bell + `!` or `?` marker when a session is awaiting permission, has gone idle or has a question.
- **Activity sub-line per workspace**: see the latest tool call or message from each session at a glance.
- **Configurable Workspace Detail Bar**: Display up to four independent containers with built-in or custom modules. See [Workspace detail bar](#workspace-detail-bar).
- **Project Manager pane**: a dedicated agent session that summarizes what every workspace is for, where it's at, and what's next.
- **Remote control**: attach from claude.ai/code or the mobile app; or run wsx in tmux+ssh for full-fidelity desktop access; store and access remote connection commands via the `remote` CLI.
- **Pinned commands**: define your `/pull-request`, `/feedback`, `/ultrareview` shortcuts once; fire them with `Ctrl-x <digit>` or a click while attached or from the workspace details bar.
- **Related repos**: declare related wsx repos per primary repo; workspaces spawn with `--add-dir` for each and a read-only system prompt so claude can read but won't edit them.  Agent is provided with the wsx skill to use the CLI to orchstrate between repos.
- **Keyboard first navigation**: comprehensive keybindings for every action, from workspace creation to process killing to project manager refreshes.
- **Frictionless workflow**: auto-rename branches from your first prompt, per-repo setup/archive scripts, editor/terminal/diff hooks.

### Quick start

```bash
cargo build --release
./target/release/wsx repo add /path/to/your/repo
./target/release/wsx              # launch TUI
```

Press `n` to create your first workspace, then `enter` to attach. Claude Code spawns inside the worktree.

## Daily use

### Keybindings

#### Dashboard

| Key                               | Action                                                                                                       |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| `Up` / `Down` (or `k` / `j`)      | Move selection through repo headers and workspaces                                                           |
| `h` / `l`                         | Fold / unfold the focused repo (idempotent; use `zz` to toggle)                                              |
| `enter` (or `i`) on a workspace   | Attach to its claude session (spawns or resumes)                                                             |
| `enter` (or `i`) on a repo header | Open the New Workspace modal targeting that repo                                                             |
| `n`                               | New workspace in the selected row's repo                                                                     |
| `Shift + N`                       | New workspace in permissive mode (claude launches with `--dangerously-skip-permissions`)                     |
| `e`                               | Open the selected workspace in your editor (no-op on repo header)                                            |
| `t`                               | Open the selected workspace in a terminal (no-op on repo header)                                             |
| `v`                               | View diff of the selected workspace's branch vs the repo's base branch (auto-detected; no-op on repo header) |
| `Shift + K`                       | On a workspace: show processes under its worktree. On a repo header: move the repo up one slot (persisted)   |
| `Shift + J`                       | On a repo header: move the repo down one slot (persisted). No-op on a workspace                              |
| `s`                               | Open repo settings modal for the selected repo (or the parent repo when a workspace is selected)             |
| `d`                               | Archive the selected workspace (no-op on repo header)                                                        |
| `q`                               | Quit (kills all running sessions)                                                                            |
| `p`                               | Toggle the Project Manager pane (no-op when `pm_enabled` is off)                                             |
| `Tab`                             | Swap focus between dashboard and the PM pane (when visible)                                                  |
| `z z`                             | Toggle fold on the focused repo                                                                              |
| `z a`                             | Expand all repos (override default-fold heuristic)                                                           |
| `z M`                             | Fold all repos                                                                                               |
| `r` (when PM focused)             | Refresh `workspaces.json` and ask PM to re-summarize                                                         |
| `Ctrl-O` (when PM focused)        | Expand PM to full screen (use `Ctrl-x d` to detach back)                                                     |

#### New Workspace / Confirm Archive / Setup Running modals

| Key                           | Action                              |
| ----------------------------- | ----------------------------------- |
| `enter`                       | Confirm                             |
| `esc`                         | Cancel                              |
| `y` / `n`                     | Confirm/cancel on ConfirmArchive    |
| Printable chars / `backspace` | Edit the name field on NewWorkspace |

#### Attached workspace

Keystrokes are forwarded to the running `claude` session, except:

| Key              | Action                                                                                                      |
| ---------------- | ----------------------------------------------------------------------------------------------------------- |
| `Ctrl-x d`       | Close the focused pane. When only one pane is open, detaches back to the dashboard (session keeps running). |
| `Ctrl-x Esc`     | Save the current split layout for this workspace, then detach to the dashboard. Restored on next attach.    |
| `Ctrl-x ←/→/↑/↓` | Move focus between split panes in that direction (vim's `Ctrl-w` motions).                                  |
| `Ctrl-x u`       | Open the floating updates panel (shows other workspaces' state; supports `v`/`s` to open in a split)        |
| `Ctrl-x a`       | Open the agents panel to add/remove agents in this workspace (see [Multi-agent workspaces](#multi-agent-workspaces)) |
| `Ctrl-x e`       | Open the attached workspace in your editor (same `editor_cmd` as `[e]` on the dashboard)                    |
| `Ctrl-x t`       | Open the attached workspace in a terminal (same `terminal_cmd` as `[t]`)                                    |
| `Ctrl-x v`       | View diff of the attached workspace's branch vs the base branch (same `diff_cmd` as `[v]`)                  |
| `Ctrl-x k`       | Show processes running under the attached workspace's worktree                                              |
| `Ctrl-x x`       | Send a literal `Ctrl-x` to claude                                                                           |
| `Ctrl-x c`       | Toggle the change chronology bar on/off                                                                     |
| `Ctrl-x C`       | Swap the chronology bar's side (left ↔ right)                                                               |
| `Ctrl-x →` (bar on right) / `Ctrl-x ←` (bar on left) | Move keyboard focus into the chronology bar (from the adjacent edge pane only) |
| `Ctrl-x ←` (bar on right) / `Ctrl-x →` (bar on left) | Return focus from the bar to the agent pane                                    |
| `↑` / `k` *(bar focused)* | Move selection up (toward newer entries)                                              |
| `↓` / `j` *(bar focused)* | Move selection down (toward older entries)                                            |
| `g` *(bar focused)*        | Jump to the top (newest entry)                                                        |
| `G` *(bar focused)*        | Jump to the bottom (oldest entry)                                                     |
| `Enter` *(bar focused)*    | Open the full-change detail modal for the selected entry                              |
| `Esc` *(bar focused)*      | Return focus to the agent pane                                                        |

When a workspace has more than one agent, the footer also binds bare keys `q w r y i o p s h j` (no leader) to switch the focused pane between agents — see [Multi-agent workspaces](#multi-agent-workspaces).

### Pinned commands

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

### Mouse, scrollback, and text selection

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

### Dashboard status indicators

| Symbol                 | Meaning                                                                         |
| ---------------------- | ------------------------------------------------------------------------------- |
| `●`                    | Session is running in this wsx process                                          |
| `↻`                    | Resumable — a prior claude session exists for this worktree; attach to continue |
| `○`                    | No session ever started here                                                    |
| `✕`                    | Workspace state is `Failed` (worktree creation didn't succeed)                  |
| `[setup-failed]` badge | Setup script exited non-zero; workspace is otherwise usable                     |

Activity column for running sessions:

- `active` — output within the last 2 seconds
- `idle` — output within the last 30 seconds
- `waiting` — no output for over 30 seconds
- `off` — no current session
- `resumable` — prior session exists, not currently running

#### Activity sub-line

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

#### Diff counts column

Compact summary of `git status` per workspace, refreshed every 2 seconds:

| Symbol (plain) | Symbol (nerd) | Meaning                                     |
| -------------- | ------------- | ------------------------------------------- |
| `~N`           | `N`           | Modified/staged/added/deleted tracked files |
| `?N`           | `N`           | Untracked files                             |
| `↑N`           | `N`           | Commits ahead of upstream                   |
| `↓N`           | `N`           | Commits behind upstream                     |

Zero values omitted. Clean workspaces show nothing in this column.

#### Attention alerts

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

### Process tracking

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
    [↑/↓] move   [r] run   [k] term   [K] kill   [esc] close

`k` sends `SIGTERM` to the highlighted process; `K` sends `SIGKILL`.
After either, wsx immediately re-scans so the list reflects the new
state.

`r` opens a prompt to run a command in the selected workspace's worktree —
handy for starting a dev server without opening a separate terminal. The
command runs via `sh -c` as a background process, with stdout and stderr
captured to a log file under `~/.local/state/wsx/logs/`; the path is shown
after launch. It runs detached (its own session, reparented away from wsx),
so it keeps running if you close the dashboard and survives until it exits or
you stop it. Because it runs in the worktree, it appears in this same list on
the next scan, where `K` stops it.

**Notes:**

- Detection runs once every 10 seconds in the background via `lsof -d cwd`.
- Shells and editors (bash, zsh, nvim, code, etc.) are filtered out so the
  count surfaces what's interesting — your dev server, not the terminal
  hosting it.
- Helper processes spawned by Claude Code and editors (MCP servers,
  language servers) are hidden too, since they inherit the worktree cwd
  but aren't work you launched. The exception is a process holding a
  listening TCP socket: a dev server started from inside Claude Code (e.g.
  a `pnpm dev` on `:3000`) still shows up and can be killed here, while the
  stdio-only helpers stay filtered.
- wsx never starts these processes itself. Launch them however you
  like (the `[t]` terminal keybind is one option). The feature is
  observability plus a kill hook, not lifecycle management.
- Requires `lsof` to be installed (standard on most Linux/macOS setups).
  If it's missing, the count stays at 0 and the modal shows "(no tracked
  processes)" — no errors.

### Workspace detail bar

When a workspace is selected on the dashboard, wsx renders a multi-column
detail bar across the bottom. The body is divided into 1–4 equal-width
**containers**; each container holds one or more **modules** stacked
vertically. Four built-in modules ship today: `session_summary`,
`recent_chat`, `processes`, `recent_files`. The bar's appearance is
controlled by the `detail_bar_config` setting — globally via `wsx config`,
with optional per-repo overrides.

#### Schema and defaults

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

| Field             | Type          | Default             | Effect                                                                                                                                                                                                                                         |
| ----------------- | ------------- | ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `visible`         | bool          | `true`              | Master toggle. When `false`, the bar is hidden entirely and `Tab` skips the reply input.                                                                                                                                                       |
| `height.percent`  | u8            | `30`                | Target height as a percent of the terminal's rows. Clamped to `[5, 80]`.                                                                                                                                                                       |
| `height.min_rows` | u16           | `8`                 | Floor on the bar's height. Clamped to `[4, 40]`.                                                                                                                                                                                               |
| `height.max_rows` | u16           | `18`                | Ceiling on the bar's height. Clamped to `[4, 60]`. If `min_rows > max_rows`, the two are swapped on save.                                                                                                                                      |
| `containers`      | list of lists | (see default above) | Outer length 1–4: one entry per equal-width column. Inner is a list of module IDs stacked vertically within the column. An empty inner list `[]` reserves an empty column. Empty outer list resets to default. Lengths > 4 are truncated to 4. |

**Built-in module IDs:** `session_summary`, `recent_chat`, `processes`,
`recent_files`. Unknown IDs render a `[unknown: <id>]` placeholder and
log a warning, so typos are visible but don't break the dashboard.

When every container is empty (`[[], [], []]`), the bar shrinks to its
4-row chrome (header + two rules + reply input) regardless of
`height.percent`. That's how you trim the bar to just the reply input.

#### Setting the global value

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

#### Per-repo override

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
{ "visible": false }
```

Single chat column for this repo; keep `visible` and `height` inherited from global:

```json
{ "containers": [["recent_chat"]] }
```

Taller bar for a repo where the session-summary text is usually long
(CLI tools with verbose tool-call traces):

```json
{ "height": { "percent": 45, "max_rows": 28 } }
```

Merge precedence: bake-in defaults → global `detail_bar_config` →
per-repo override. `visible` and `height.*` apply per-field; `containers`
whole-replaces when the override sets it. So a repo override that only
sets `containers` still picks up any global `height` changes you make
later.

#### Behavior on bad input

- Malformed JSON at the global level — falls back to baked-in defaults at runtime, logged at `warn`.
- Malformed JSON in a repo override — the override is ignored; the global value applies, logged at `warn` with the repo name.
- Out-of-range `height.percent` / `min_rows` / `max_rows` — clamped to legal ranges on save (`wsx config set/edit`) and again at runtime as a defense-in-depth.
- `min_rows > max_rows` — swapped on save so the lower bound is the floor and the higher is the ceiling.

### Workspace updates panel

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

  | Key                          | Action                                                                                 |
  | ---------------------------- | -------------------------------------------------------------------------------------- |
  | `Up` / `Down` (or `k` / `j`) | Move selection within the panel.                                                       |
  | `Enter`                      | Switch the current pane to the selected workspace (replaces it).                       |
  | `v`                          | Open the selected workspace in a vertical split (panes side by side, vim's `:vsplit`). |
  | `s`                          | Open the selected workspace in a horizontal split (panes stacked, vim's `:split`).     |

### Split panes

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

When you split the _focused_ pane again in the same direction as its
parent, the new pane is inserted as a sibling instead of nesting deeper —
matches vim and keeps the tree shallow.

**Saving a layout.** `Ctrl-x d` detaches without remembering how the panes
were arranged. To keep the arrangement, press `Ctrl-x Esc` instead: wsx
saves the split tree (and which pane was focused) against the _anchor_
workspace — the first pane you attached to — then detaches to the
dashboard. The next time you attach to that workspace, wsx restores the
layout and respawns the side panes' sessions. Panes whose workspaces no
longer exist are pruned on restore; if none survive you get a plain
single-pane view. Workspaces with a saved multi-pane layout show a columns
glyph next to their branch on the dashboard (nerd fonts only).

### Project manager pane

Press `p` on the dashboard to open a horizontal pane below the workspace list
hosting a dedicated "project manager" session. The PM runs whichever coding
agent your global `coding_agent` setting selects — `claude` (the default),
`pi`, `hermes`, or `codex` (see [Coding agents](#coding-agents)) — so
`wsx config set coding_agent pi` switches the PM to Pi on its next open. PM's
job is to answer three questions about each of your active workspaces:

- What was this workspace created for?
- Where have things been left off?
- What's next to close it out?

`p` opens the pane and focuses it immediately — keystrokes go to PM (like
the attached view). `Tab` or `Esc` swaps focus back to the dashboard;
`Tab` from the dashboard swaps back into the PM pane. `r` (while PM is
focused) refreshes `workspaces.json` and asks PM to re-summarize. `Ctrl-O`
(while PM is focused) expands PM to a full-screen attached view so you
can scroll through the agent's history naturally; `Ctrl-x d` detaches back
to the dashboard with the pane state preserved.

PM only summarizes workspaces where claude has been started at least once
(i.e., a session log exists under `~/.claude/projects/...`). Workspaces
you created but never opened are skipped — nothing for PM to report on.

PM lives at `$XDG_STATE_HOME/wsx/project-manager/` and persists across wsx
restarts by resuming the agent's prior session (Claude Code's `--continue`,
or the equivalent for Pi/Hermes/Codex). On the first `p` of a wsx run with
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

## Configuration and customization

### Global settings

```
wsx config get <key>
wsx config set <key> <value-or-@file>
wsx config list
wsx config edit <key>          # opens $EDITOR (default: vi)
```

Known keys:

| Key                      | Effect                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `branch_prefix`          | Default branch prefix for repos with no per-repo override. Branches are named `<prefix>/<workspace>`.                                                                                                                                                                                                                                                                                                                                                                                                            |
| `custom_instructions`    | Free-text appended to claude's system prompt on every workspace spawn.                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `process_doctrine`       | Standing "operating doctrine" injected into every developer session (new and resumed) across all agents: think and plan before scope is set, use the superpowers skills by default (Claude/Pi only), break work into logical commits, and load the wsx skill. Not applied to the Project Manager session. Set this to replace the default text verbatim (`@file` supported); set it to `off` / `none` / `disabled` to suppress injection entirely. A blank value restores the default (it is not an off switch). |
| `coding_agent`           | Default coding agent for new workspaces _and_ the Project Manager pane: `claude` (default) / `pi` / `hermes` / `codex`. Per-workspace override via `wsx workspace create <repo> --agent <agent>` (does not affect the PM). See [Coding agents](#coding-agents).                                                                                                                                                                                                                                                  |
| `nerd_fonts`             | Render nerd-font glyphs in the dashboard. Default ON; set to `false` / `0` / `off` to disable.                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `editor_cmd`             | Command to run for `[e] edit` on the dashboard. Worktree path appended as final arg unless the command contains `{path}` (substituted in place). Examples: `code`, `cursor`, `alacritty -e nvim`, `xdg-terminal-exec --dir={path} nvim`. Also required for the chronology bar's "open at changed line" action; see [Change chronology](#change-chronology) for the `{file}`/`{line}` injection details.                                                                                                          |
| `terminal_cmd`           | Command to run for `[t] terminal` on the dashboard. Spawned with cwd=worktree; `{path}` substituted in place if present. Examples: `alacritty`, `kitty`, `gnome-terminal`.                                                                                                                                                                                                                                                                                                                                       |
| `notifications`          | Ring the terminal bell and show a `!` marker when a workspace transitions to `waiting` (claude paused for ≥30s). Default ON; set to `off` / `false` / `0` / `no` to disable.                                                                                                                                                                                                                                                                                                                                     |
| `theme`                  | Color theme. One of `default` (palette-adaptive ANSI), `dracula` (RGB), `jellybeans` (RGB), `nord` (RGB). Unknown values fall back to `default`. Restart wsx after changing.                                                                                                                                                                                                                                                                                                                                     |
| `pm_enabled`             | Enable the Project Manager pane (`p` keybind). Default ON; set to `off` / `false` / `0` / `no` to disable.                                                                                                                                                                                                                                                                                                                                                                                                       |
| `pm_custom_instructions` | Free-text appended to the project manager's system prompt. Same `@file` / empty-clears semantics as `custom_instructions`.                                                                                                                                                                                                                                                                                                                                                                                       |
| `pm_fast_mode`           | Launch the Project Manager session with Claude Code's fast mode enabled (`--settings '{"fastMode":true}'`). PM is a status-summary session, so fast output is usually the right tradeoff. Only applies when the PM agent is `claude` (Pi/Hermes/Codex have no fast mode); ignored otherwise. Default OFF; set to `on` / `true` / `1` / `yes` to enable.                                                                                                                                                           |
| `mcp_mirror`             | Inherit MCP servers from the source repo into worktrees (see [MCP server inheritance](#mcp-server-inheritance)). Default ON; set to `off` / `false` / `0` / `no` to disable.                                                                                                                                                                                                                                                                                                                                     |
| `remote_control`         | Pass `--remote-control` to claude on every spawn so the session is reachable via [claude.ai/code](https://claude.ai/code) and the Claude mobile app (see [Remote control](#remote-control)). Default ON; set to `off` / `false` / `0` / `no` to disable.                                                                                                                                                                                                                                                         |
| `remote_control_sandbox` | When `remote_control` is on, also pass `--sandbox` for an extra safety wrapper on remote-issued commands. Default OFF; set to `on` / `true` / `1` / `yes` to enable.                                                                                                                                                                                                                                                                                                                                             |
| `pinned_commands`        | Newline-separated list of `Label=command` (or bare `command`) entries. Each becomes a chip in the attached view, fired via `Ctrl-x <digit>` or click. Max 9 visible/keyable. Per-repo override available via `wsx repo set-pinned-commands`.                                                                                                                                                                                                                                                                     |
| `remotes`                | Newline-separated list of `name=command` entries — named shell commands run by `wsx remote <name>`, typically `ssh -t host '…tmux attach…'` for reattaching a wsx session running on another machine. List with `wsx remote`; add or edit with `wsx config edit remotes`. See [Named remote shortcuts](#named-remote-shortcuts).                                                                                                                                                                                 |
| `dashboard_name_width`   | Width (chars) of the workspace-name column on the dashboard. Default `24`. Clamped to `10..=60`.                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `dashboard_branch_width` | Width (chars) of the `⎇ branch` column on the dashboard. Default `28`. Clamped to `10..=80`.                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `detail_bar_config`      | JSON blob controlling the per-workspace detail bar (visibility, height, and the container/module layout). See [Workspace detail bar](#workspace-detail-bar) for the schema, defaults, and per-repo override flow. Out-of-range values are clamped on save.                                                                                                                                                                                                                                                       |
| `chronology_config`      | JSON blob controlling the change chronology bar in the attached view (visibility, side, and width). See [Change chronology](#change-chronology) for the schema, defaults, and per-repo override flow.                                                                                                                                                                                                                                                                                                            |

Value sources:

- A literal string: `wsx config set branch_prefix bakedbean`
- A file (prefix with `@`): `wsx config set custom_instructions @./instructions.md`
- Empty (clears): `wsx config set custom_instructions ""`

`wsx config edit <key>` opens `$EDITOR` on a tempfile prepopulated with the current value; saving updates the setting. Useful for multi-line `custom_instructions`.

### Themes

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

### Auto-rename modes

After your first prompt in a freshly-created workspace, wsx renames the workspace + git branch based on the conversation. Controlled by `WSX_RENAME_MODE`:

| Mode               | Behavior                                                                                                                                                                                                                           |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `claude` (default) | Claude itself runs `git branch -m` as the first action in its response, based on your first message. A background poller propagates the rename to the wsx store. Higher-quality slugs at the cost of ~80 tokens per session start. |
| `local`            | wsx intercepts your first prompt's keystrokes locally and slugifies them. Zero tokens; literal text.                                                                                                                               |
| `off`              | No auto-rename. Workspaces keep their generated `<adjective>-<plant>` name forever.                                                                                                                                                |

The rename only fires on workspaces whose name still matches the generated `<adjective>-<plant>` pattern.

### Change chronology

When an agent is actively editing files, it's easy to lose track of what changed, where, and when — especially across a long session with many small edits. The change chronology bar is a toggleable vertical panel docked to the side of the **attached** view that rebuilds your spatial and temporal memory of what the agent touched.

The bar shows a newest-first, time-ordered list of individual file edits the agent made — one entry per change, not per commit. Each entry is a single line: the time and the file path. Long paths are abbreviated by collapsing the ancestor directories to their first letter, keeping the parent directory and filename readable (e.g. `docs/superpowers/specs/2026-06-05-foo.md` shows as `d/s/specs/2026-06-05-foo.md`). Press `Enter` on an entry (or click it) to open the **full-change detail modal**, a scrollable overlay showing the complete diff with a line-number gutter — added (`+`) lines are numbered with their current file line (the same line the editor opens to), while removed (`-`) lines show a blank gutter.

Currently the chronology is reconstructed from Claude Code's on-disk session logs. Support for other agents is added incrementally as those log formats are covered.

#### Keyboard navigation

The chronology bar is a focusable pane. While attached, press `Ctrl-x` then an arrow key **toward the bar's side** to move keyboard focus into it (bar on the right → `Ctrl-x →`; bar on the left → `Ctrl-x ←`). This only works from the edge pane adjacent to the bar; otherwise `Ctrl-x`+arrow keeps moving between agent split panes as normal.

While the bar is focused, keystrokes are captured by the bar and do **not** reach the agent:

- `↑` / `k` and `↓` / `j` move the selection; `g` jumps to the top (newest), `G` to the bottom.
- `Enter` on an entry opens the full-change detail modal for that entry.
- `Esc` (or `Ctrl-x` + arrow **away** from the bar's side) returns focus to the agent pane.

#### Detail modal

The modal is a scrollable overlay showing the full diff of the selected change:

- Scroll with `↑` / `↓`, `j` / `k`, `PgUp` / `PgDn`, `g` / `G`, or the mouse wheel.
- Press `e` to open the file in your editor at the changed line (requires `editor_cmd` — see below).
- Press `Esc` or click outside the modal to close it and return to the bar.

The diff is displayed with basic syntax highlighting for Rust, Python, Shell, and a generic C-like family (C/C++/JS/TS/Go/Java/JSON, and similar); other file types are shown plain. Added (`+`) lines are tinted green and removed (`-`) lines red; the line-number gutter stays dim. Highlighting is per-line — multi-line strings or block comments may not be perfectly colored.

#### Keybindings (attached view, under the `Ctrl-x` leader)

| Key        | Action                                          |
| ---------- | ----------------------------------------------- |
| `Ctrl-x c` | Toggle the chronology bar on/off                |
| `Ctrl-x C` | Swap the bar's side (left ↔ right)              |

Mouse wheel over the bar scrolls it. Click an entry to focus the bar, select the entry, and open the detail modal.

#### Opening a file at the changed line

Pressing `e` inside the detail modal opens the file in your editor, jumping directly to the modified line.

**`editor_cmd` is required for this action.** If `editor_cmd` is unset, wsx shows a dismissible prompt telling you to configure it. There is no silent fallback to `$VISUAL` or `$EDITOR` for this specific action — those env-var fallbacks still apply to the separate `[e]` / `Ctrl-x e` "open workspace in editor" actions, which are unchanged.

**File and line injection.** When `editor_cmd` is set, wsx injects the file path and line number at runtime using one of two strategies:

- **Placeholders**: if your command contains `{file}`, `{line}`, and/or `{path}`, they are substituted in place. `{path}` is the worktree root (the same value substituted by the `[e]` dir-open action), so a single `editor_cmd` works for both actions. Use placeholders for editors wsx doesn't recognize or when you need exact control over argument order.
- **Auto-detection**: if no `{file}` or `{line}` placeholders are present, wsx scans the command for a known editor name and appends the appropriate goto arguments (after substituting any `{path}` first):
  - `code`, `codium`, `cursor`, `zed` → `--goto <file>:<line>`
  - `vim`, `nvim`, `vi`, `nano`, `emacs`, `emacsclient` → `+<line> <file>`

Detection matches the editor name **anywhere** in the command, so a terminal wrapper works transparently. For example, `alacritty -e nvim` is detected as nvim and becomes `alacritty -e nvim +<line> <file>`, opening the file at the changed line in a new terminal window.

```bash
wsx config set editor_cmd 'alacritty -e nvim'
```

Commands with `{path}` also work — the worktree is substituted first, then the editor is auto-detected or `{file}`/`{line}` are substituted:

```bash
wsx config set editor_cmd 'xdg-terminal-exec --dir={path} nvim'
```

For an editor wsx doesn't recognize, add `{file}` and `{line}` placeholders to control the exact syntax:

```bash
wsx config set editor_cmd 'myed --line {line} {file}'
```

**Error visibility.** If the editor fails to launch, wsx surfaces the error in a dismissible prompt — failures are no longer silent.

#### Schema and defaults

`chronology_config` is a JSON blob set globally via `wsx config set` or overridden per-repo via the repo settings modal (`s` on the dashboard, select the `chronology_config` row). Every field is optional; missing fields fall back to defaults.

| Field            | Type                  | Default  | Effect                                                                 |
| ---------------- | --------------------- | -------- | ---------------------------------------------------------------------- |
| `visible`        | bool                  | `true`   | Master toggle. `false` hides the bar entirely (same as `Ctrl-x c`).   |
| `side`           | `"left"` / `"right"`  | `"right"` | Which side of the attach area the bar is docked to.                   |
| `width.percent`  | u8                    | `32`     | Target width as a percent of the attach area's columns.                |
| `width.min_cols` | u16                   | `24`     | Minimum width in columns.                                              |
| `width.max_cols` | u16                   | `60`     | Maximum width in columns.                                              |

#### Setting the global value

```bash
wsx config set chronology_config '{"side":"left","width":{"min_cols":30}}'
wsx config get chronology_config
wsx config set chronology_config ""   # clear (reverts to defaults)
```

Partial JSON is fine — unspecified fields inherit defaults. Malformed JSON is rejected with a non-zero exit and the previous value is preserved.

#### Per-repo override

Open the repo settings modal with `s` on the dashboard, select the `chronology_config` row, and press Enter. `$EDITOR` opens on `{}\n` (or the current override). Save to apply; press `d` to clear the override and fall back to the global value.

Example — pin the bar to the left for a repo with a wide main pane:

```json
{ "side": "left", "width": { "percent": 28 } }
```

### Coding agents

By default, wsx spawns Claude Code (`claude`) as the coding agent in every workspace. You can choose a different agent per-workspace or set a global default:

```bash
wsx config set coding_agent hermes           # new workspaces use hermes by default
wsx workspace create backend --agent pi      # override for a single workspace
```

The global `coding_agent` setting also selects the agent that powers the
[Project Manager pane](#project-manager-pane); there is no separate PM-only
setting. The per-workspace `--agent` override applies only to that
workspace, not the PM.

Supported agents:

| Agent              | CLI option       | Source                                                                    | Config                                    |
| ------------------ | ---------------- | ------------------------------------------------------------------------- | ----------------------------------------- |
| `claude` (default) | `--agent claude` | `claude` binary (override via `WSX_CLAUDE_BIN`)                           | Environment + `~/.claude.json` MCP        |
| `pi`               | `--agent pi`     | `pi` binary (override via `WSX_PI_BIN`)                                   | `~/.pi/`                                  |
| `hermes`           | `--agent hermes` | [nousresearch/hermes-agent](https://github.com/nousresearch/hermes-agent) | `~/.hermes/config.yaml` (provider, model) |
| `codex`            | `--agent codex`  | `codex` binary (override via `WSX_CODEX_BIN`)                             | `~/.codex/config.toml`                    |

#### Hermes integration

When a workspace uses `coding_agent: hermes`, wsx spawns `hermes` (or the path in `WSX_HERMES_BIN`) instead of `claude`. Hermes runs in classic REPL mode and receives wsx custom instructions and auto-rename directives.

**AGENTS.md management**: Because Hermes lacks a `--append-system-prompt` flag, wsx injects instructions into a fenced block at the end of `AGENTS.md` in the worktree's working directory:

```markdown
<!-- BEGIN wsx-managed -->

…injected instructions…

<!-- END wsx-managed -->
```

The block is rewritten every time Hermes spawns and automatically cleaned up when there's nothing to inject. This approach works whether or not the repository tracks `AGENTS.md` in git:

- **Untracked `AGENTS.md`**: wsx adds it to `.git/info/exclude` so it doesn't show up in `git status`.
- **Tracked `AGENTS.md`**: the worktree will show the file as modified during a Hermes spawn — this is expected and the modification disappears on subsequent spawns when there's no custom instructions to inject.

**Session detection**: On every Hermes spawn, wsx writes a timestamp marker at `<worktree>/.git/info/wsx-hermes-spawn-at` (per-worktree-local, never committed). To find the active Hermes session for a worktree, wsx queries `~/.hermes/state.db` for the most recent session started at or after that timestamp (with a 2-second look-back buffer to absorb clock skew). This drives both the prior-session indicator on the dashboard and the `--resume <id>` flag on Continue spawns. Note: if two worktrees both spawn Hermes within a few seconds of each other, the lookup is best-effort — the more-recent session could be attributed to either worktree depending on timing.

**Session-tail**: wsx tails `~/.hermes/state.db` (sqlite) to populate the dashboard's RECENT CHAT, SESSION SUMMARY, and last-message columns for Hermes workspaces. The following fields are populated: last assistant text, first user prompt, stop reason, tool-use counts, and per-event snapshots (user messages, assistant text, and tool calls — including `ran \`<cmd>\`` display for terminal/bash tool invocations). Tool-use counts treat all Hermes tool names as "other" for now — categorization into read/edit/write/bash buckets is a follow-up since Hermes uses lowercase tool names rather than Claude's capitalized convention. Still missing compared to Claude/Pi: edited-files tracking and pending-tool-use timing for permission-prompt detection.

**Environment overrides**: configure Hermes via `~/.hermes/config.yaml` (persistent settings), or set `WSX_HERMES_MODEL` and `WSX_HERMES_PROVIDER` to override per-workspace:

```bash
WSX_HERMES_MODEL=llama-3-70b-instruct WSX_HERMES_PROVIDER=together wsx workspace create backend --agent hermes
```

#### Codex integration

When a workspace uses `coding_agent: codex`, wsx spawns `codex` (or the path in `WSX_CODEX_BIN`) instead of `claude`. Codex receives wsx custom instructions and auto-rename directives.

**AGENTS.md management**: Because Codex has no `--append-system-prompt` flag, wsx injects the workspace doctrine, the auto-rename hint, and any custom instructions into a `wsx-managed` fenced block in the worktree's `AGENTS.md` — the same mechanism used for Hermes:

```markdown
<!-- BEGIN wsx-managed -->

…injected instructions…

<!-- END wsx-managed -->
```

The block is rewritten every time Codex spawns and automatically cleaned up when there's nothing to inject. The file is git-excluded via `.git/info/exclude` if untracked, or will show as modified during a spawn if already tracked. The superpowers-skills doctrine clause is omitted for Codex (those skills install under `~/.claude` and Codex can't load them).

**Claude slash commands**: before each Codex spawn, wsx mirrors Markdown files from `~/.claude/commands/` into a local Codex plugin at `~/plugins/wsx-claude-commands/commands/` and registers that plugin in the implicit personal marketplace at `~/.agents/plugins/marketplace.json`. The marketplace entry is marked `INSTALLED_BY_DEFAULT`, so commands such as `/pull-request` and `/commit-changes` are available in Codex without maintaining a second command set. Edits to the Claude command files are picked up on the next Codex spawn.

**Spawn**: fresh workspaces launch bare `codex`. Non-yolo sessions use Codex's built-in interactive approvals + workspace-write sandbox; `--yolo` workspaces add `--dangerously-bypass-approvals-and-sandbox`.

**Continue**: `codex resume --last`, which Codex filters to the current directory natively — so wsx resumes the worktree's own most-recent session.

**Activity**: the dashboard detail bar tails the worktree's rollout file under `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`. RECENT FILES is not yet populated for Codex (file edits are inferred-via-shell and not tracked).

**Model**: set `WSX_CODEX_MODEL` to pass `-m <model>` to Codex (e.g. `gpt-5.4`). Unset = Codex default.

### Multi-agent workspaces

A workspace isn't limited to a single agent. You can attach **additional agents — of the same kind or different kinds — to one workspace.** Every agent runs as its own session but they all share the same git worktree and branch, and they can message each other. This is useful for, say, running a second Claude as a dedicated reviewer alongside the one doing the work, or pitting `claude` and `codex` at the same problem in the same tree.

Every workspace starts with exactly one agent — the **primary**, chosen at creation time by `--agent` or the `coding_agent` setting (see [Coding agents](#coding-agents)). Everything below is about adding more on top of that.

#### Adding and removing agents

In the TUI, press `Ctrl-x a` while a workspace is selected to open the **agents panel**. It lists the agents already attached (the primary is tagged `(primary)`) and an "add" picker of the four kinds:

| Key      | Action                                                  |
| -------- | ------------------------------------------------------- |
| `↑`/`↓`  | Move through the add picker                             |
| `Enter`  | Add the highlighted kind                                |
| `a`      | Add one of every kind at once                           |
| `x`      | Remove the most-recently-added (non-primary) agent      |
| `Esc`    | Close the panel                                         |

Newly added agents spawn immediately with the workspace's context injected. The primary can't be removed from the panel — it lives for the life of the workspace.

From the CLI, the equivalent of the panel's "add" is:

```bash
wsx agent add <kind>     # kind = claude | pi | hermes | codex
```

This runs against the **current** workspace — the one whose worktree you're in, or the one named by `$WSX_WORKSPACE_ID` (see [identity](#agent-identity-and-labels) below). It prints the new agent's label, e.g. `added claude#2`.

#### Switching focus between agents

When a workspace has more than one agent, the attached view grows a **footer agents row** listing each agent with a single-letter switch key:

```
agents:  ▎claude q   ▎codex w   ▎pi r
```

Press the key (`q`, `w`, `r`, …) to point the focused pane at that agent's session, or click the pill. The keys are drawn from a fixed pool — `q w r y i o p s h j` — assigned in display order (primary first). A workspace with more than ten agents renders the rest keyless, but they stay clickable. The row only appears once a second agent exists; a single-agent workspace looks exactly as before.

Because agents share the worktree, switching focus is just changing which session your keystrokes go to — there's no branch-swapping or checkout involved.

#### Inter-agent messaging

Agents in the same workspace can send each other messages:

```bash
wsx agent send <label> <message…>
```

`<label>` is an agent's footer/list label (`claude`, `claude#2`, `codex`, …). The rest of the line is the message body. Delivery is **asynchronous**: the message is queued and injected into the target's session on the next tick, prefixed with a banner so the recipient knows where it came from:

```
[message from claude#2]
…your message body…
```

If the sender is the `wsx` CLI itself (not another agent — i.e. `$WSX_AGENT_INSTANCE_ID` is unset), the banner is just `[message]`. If the target agent isn't running yet, wsx spawns it first, then delivers. Sending to a label that doesn't exist in the workspace errors with a hint to run `wsx agent list`.

Since all agents write to the same files, prefer messaging to hand off work rather than editing the same paths in parallel.

#### Listing agents

```bash
wsx agent list
```

Prints one agent per line — its instance id and label, with `(primary)` appended for the primary — for the current workspace:

```
1  claude  (primary)
2  claude#2
4  codex
```

The leading number is the agent's instance id — the same value wsx injects as `$WSX_AGENT_INSTANCE_ID` into that agent's session.

#### Agent identity and labels

Each agent instance has a **label** derived from its kind and its ordinal within that kind: the first of a kind is the bare name (`claude`), and each subsequent one of the same kind gets a `#N` suffix (`claude#2`, `claude#3`). The same rule produces the labels shown in the footer row, in `wsx agent list`, and in message banners.

When wsx spawns an agent it injects two environment variables into that session, so the agent (or scripts it runs) can address the multi-agent CLI without guessing:

| Variable                 | Value                                              |
| ------------------------ | -------------------------------------------------- |
| `WSX_WORKSPACE_ID`       | The workspace this agent belongs to                |
| `WSX_AGENT_INSTANCE_ID`  | This specific agent instance                       |

`wsx agent` commands resolve the "current" workspace from `$WSX_WORKSPACE_ID` first, falling back to matching the current directory against known worktrees — so the commands work both from inside an agent session and from a plain shell in the worktree. `wsx agent send` uses `$WSX_AGENT_INSTANCE_ID` to stamp the `[message from …]` sender on outgoing messages.

### Per-repo setup scripts

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

#### Editing in the TUI

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
- `chronology_config` (see [Change chronology](#change-chronology))

`↑/↓` selects a field. Press `Enter` to edit — wsx temporarily leaves
the TUI, opens `$EDITOR` (or `vi` if unset) on a tempfile prepopulated
with the current value, and saves whatever you write when the editor
exits. Press `d` to clear the highlighted field. `Esc` closes.

The editor needs to be a terminal-native editor that returns when you
quit (vim, nvim, helix, micro, nano). GUI editors that return
immediately without a `--wait` flag will appear to "save nothing" —
keep `$EDITOR` pointed at a CLI editor for this flow.

## Integrations and remote access

### Editor, terminal, and diff integration

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

#### `{path}` placeholder

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

#### Diff command

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

### Remote access

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

**Saving the invocation**: once you've settled on a working `ssh … tmux attach …` command, save it as a named remote so reconnecting is just `wsx remote <name>`. See [Named remote shortcuts](#named-remote-shortcuts).

### Remote control

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

### Named remote shortcuts

```
wsx remote                 # list configured names (alphabetized), one per line
wsx remote <name>          # exec the stored command — process-replaces wsx
wsx config edit remotes    # opens $EDITOR on the blob
```

Stores frequently-used remote shell commands — typically `ssh -t host '…tmux attach…'` for reattaching a wsx session running on another machine (see [Remote access](#remote-access)) — under short names. The value is an arbitrary shell command run through `sh -c`, so nested quoting works as you'd type it at a terminal.

The `remotes` setting is a newline-separated blob, one `name=command` per line. **There is no `wsx remote add`** — `wsx config edit remotes` opens the existing blob in `$EDITOR`, and you add a remote by appending a new line. Clearing the buffer and typing only the new line replaces every other remote, so always keep the existing lines unless you mean to drop them. Example:

```
ebenmini=ssh -4 -t ebenmini.local "zsh -lc 'tmux attach'"
gpu=ssh gpu-box -t 'tmux -u attach -t main || tmux -u new -s main'
```

Parser rules: only the **first** `=` separates name from command (so `=` inside the command, e.g. an inline env-var, is preserved); whitespace around `=` is trimmed; blank lines are skipped; lines with an empty name or command are dropped; duplicate names take the last value.

`wsx remote <name>` `exec`-replaces the wsx process with `sh -c <command>`, so signals and TTY state flow straight through to the remote session; when it exits you're back at your local shell with no wsx parent process. Unknown names error out with the list of available names.

### MCP server inheritance

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

### Related repos

When you work across multiple repos that need to know about each other (a backend, a frontend, a marketing site), declare related repos per primary repo:

```bash
wsx repo set-related-repos backend frontend,marketing
```

When you spawn a workspace in `backend`, wsx invokes claude with `--add-dir` pointing at each related repo's source path. Claude can read, grep, and reference files in those directories freely.

To prevent claude from accidentally editing files in the source paths of related repos (which would land changes on whatever branch the source is on), wsx also appends a system-prompt instruction telling claude:

- Treat those directories as read-only.
- If changes are needed there, drive `wsx workspace create <other-repo> --name <slug>` from this session, `cd` into the new worktree path (`wsx workspace path <other-repo> <slug>`), and make the changes there. Each repo gets its own branch and PR; cross-link them and merge in dependency order.

This is a soft guard, not a tool-level lock — it relies on claude following the instruction. The same trust model as `custom_instructions`. Installing the bundled wsx skill (`wsx setup install-skill`, see [Agent skill](#agent-skill)) reinforces this with the full CLI vocabulary and slug-naming rules.

Unknown names in the list (e.g. a repo you renamed or unregistered) are logged and skipped at spawn time; the spawn still proceeds with the recognized names.

### Agent skill

```
wsx setup install-skill
```

Writes the bundled wsx skill to `~/.claude/skills/wsx/SKILL.md`. When Codex is installed, it also writes the same skill to `~/.codex/skills/wsx/SKILL.md`. The skill teaches coding agents how to drive the wsx CLI — workspace operations, slug-vs-`branch_prefix` naming, and the cross-repo orchestration flow that pairs with [Related repos](#related-repos). The file is embedded in the binary at compile time, so installing wsx on a new machine is `cargo install` then `wsx setup install-skill`.

Codex is considered installed when `WSX_CODEX_BIN` is set, `codex` is on `PATH`, or `~/.codex` already exists.

Idempotent: re-running when an installed copy already matches reports "already up to date" without writing. If an installed copy has drifted (you edited it locally, or you're upgrading wsx with skill changes), it's overwritten and reports "updated".

## CLI reference

Run `wsx --help` for the full command list, or `wsx <command> --help` (e.g. `wsx agent --help`) for a group's commands and arguments. `wsx --version` prints the version.

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

Where a `set-*` command below takes a value, it accepts `@/path/to/file` to load that value from a file and `""` to clear it (clearing falls back to the global setting where one exists).

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

Sets or changes the per-repo branch prefix override.

```
wsx repo set-instructions <name> <value-or-@file>
```

Sets per-repo custom instructions appended to claude's system prompt for sessions in this repo.

```
wsx repo set-pinned-commands <name> <value-or-@file>
wsx repo edit-pinned-commands <name>
```

Per-repo override of `pinned_commands`. Clearing falls back to the global setting.

```
wsx repo set-related-repos <name> <value-or-@file>
wsx repo edit-related-repos <name>
```

Per-repo list of other wsx-registered repos that workspaces in this repo should reference. Comma-separated names (e.g. `frontend,marketing`). At spawn time wsx looks each name up in the repo registry and passes `--add-dir <source-path>` to claude. Unknown names are silently skipped (logged at `warn` level — visible with `RUST_LOG=wsx=warn` or any less-specific filter).

### Workspace management

```
wsx workspace create <repo> [--name <slug>] [--yolo] [--agent claude|pi|hermes|codex]
```

Creates a workspace in `<repo>`, equivalent to the dashboard's `[n]` keybind. `<slug>` is a kebab-case workspace name; the resulting git branch is `<branch_prefix>/<slug>`. When `--name` is omitted, an adjective-noun slug like `merry-birch` is generated. `--yolo` skips the permission prompts in the spawned agent session. `--agent` overrides the `coding_agent` setting (see [Coding agents](#coding-agents)) for this workspace. Default: `claude`.

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

### Commands documented elsewhere

A few command families live in their feature sections rather than here:

- `wsx agent list | send | add` — see [Multi-agent workspaces](#multi-agent-workspaces)
- `wsx config get | set | list | edit <key>` — see [Global settings](#global-settings)
- `wsx remote [<name>]` — see [Named remote shortcuts](#named-remote-shortcuts)
- `wsx repo set-setup | set-archive | edit-setup | edit-archive <repo>` — see [Per-repo setup scripts](#per-repo-setup-scripts)
- `wsx setup install-skill` — see [Claude Code skill](#claude-code-skill)

## Reference

### Environment variables

| Variable              | Purpose                                                                                                                                                                                                                                           |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `WSX_RENAME_MODE`     | Auto-rename mode: `claude` (default) / `local` / `off`                                                                                                                                                                                            |
| `WSX_CLAUDE_BIN`      | Path to the `claude` binary (default: looked up via `PATH`). Used by tests to substitute `cat`.                                                                                                                                                   |
| `WSX_HERMES_BIN`      | Path to the `hermes` binary (default: looked up via `PATH`). Only used when `coding_agent` is `hermes`.                                                                                                                                           |
| `WSX_HERMES_MODEL`    | Model override for Hermes, passed as `HERMES_INFERENCE_MODEL` env var on the child Hermes process. When set, overrides the model in `~/.hermes/config.yaml`.                                                                                      |
| `WSX_HERMES_PROVIDER` | Provider override for Hermes, passed as `--provider` to the Hermes CLI. Note: in classic REPL mode (the default), Hermes uses the persistent provider from `~/.hermes/config.yaml`; this flag primarily affects `-z/--oneshot` and `--tui` modes. |
| `WSX_CODEX_BIN`       | Path to the `codex` binary (default: `codex` on `PATH`). Only used when `coding_agent` is `codex`.                                                                                                                                                |
| `WSX_CODEX_MODEL`     | Model passed to Codex as `-m` (e.g. `gpt-5.4`). Unset = Codex default.                                                                                                                                                                           |
| `WSX_WORKSPACE_ID`    | Injected into each agent session: the workspace it belongs to. `wsx agent` commands read it to resolve the current workspace. See [Multi-agent workspaces](#multi-agent-workspaces).                                                              |
| `WSX_AGENT_INSTANCE_ID` | Injected into each agent session: that specific agent instance. `wsx agent send` reads it to stamp the message sender. See [Multi-agent workspaces](#multi-agent-workspaces).                                                                   |
| `EDITOR`              | Editor invoked by `wsx config edit` (default: `vi`)                                                                                                                                                                                               |
| `VISUAL` / `EDITOR`   | Fallback when `editor_cmd` is unset                                                                                                                                                                                                               |
| `TERMINAL`            | Fallback when `terminal_cmd` is unset                                                                                                                                                                                                             |
| `XDG_STATE_HOME`      | Base for the wsx state directory (default: `~/.local/state`)                                                                                                                                                                                      |
| `RUST_LOG`            | `tracing` filter (default: `info`); set `wsx=debug` for verbose logs                                                                                                                                                                              |
| `HOME`                | Fallback for resolving the state directory                                                                                                                                                                                                        |

### Storage and configuration files

| Path                                                | Contents                                                                                                 |
| --------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `$XDG_STATE_HOME/wsx/state.db`                      | SQLite database: repos, workspaces, settings                                                             |
| `$XDG_STATE_HOME/wsx/worktrees/<repo>/<workspace>/` | Worktree directories created by `wsx`                                                                    |
| `$XDG_STATE_HOME/wsx/logs/wsx.log`                  | Daily-rotated `tracing` logs                                                                             |
| `$XDG_STATE_HOME/wsx/project-manager/`              | PM Claude Code session cwd; contains `workspaces.json` and PM's own git init. Auto-created on first `p`. |
| `~/.claude/projects/<encoded-cwd>/<session>.jsonl`  | Claude Code's own session files (wsx probes these to detect resumable workspaces)                        |

## Development

### Testing

```bash
cargo test -- --test-threads=1
```

The test suite substitutes `claude` with `cat` via `WSX_CLAUDE_BIN`, so it runs without Claude Code installed. `--test-threads=1` is required because several tests mutate `WSX_CLAUDE_BIN` and `HOME`.
