# wsx waybar indicator — design

Date: 2026-07-26
Status: approved pending spec review

## Goal

A waybar module for Linux (omarchy/Hyprland) that shows all registered wsx
repos and their workspaces at a glance, and lets the user jump to a workspace
by clicking. Shipped as part of wsx, but strictly isolated as a Linux/waybar
add-on: wsx is cross-platform and no waybar code may bleed into core modules.

## Non-goals

- No dropdowns rendered by waybar itself (not supported); a picker menu is used.
- No push/streaming updates; waybar polls every 5 seconds.
- No support for bars other than waybar or compositors other than Hyprland
  (jump degrades gracefully elsewhere on Linux).

## Architecture

Four pieces, all inside one Linux-only module tree:

| Piece | Command | Role |
|---|---|---|
| Status emitter | `wsx waybar status` | Print one waybar JSON object; run by waybar every 5s |
| Menu | `wsx waybar menu` | Walker picker over all workspaces; on pick, runs jump |
| Jump | `wsx waybar jump <repo> <slug>` | Select workspace in a running TUI + focus its window, or launch one |
| Installer | `wsx setup waybar` | Wire up `~/.config/waybar` |

### Isolation (hard requirement)

- All waybar logic lives in a new top-level `src/waybar/` module, declared as
  `#[cfg(target_os = "linux")] mod waybar;`. Suggested files: `mod.rs`
  (dispatch), `status.rs`, `menu.rs`, `jump.rs`, `ipc.rs`, `install.rs`.
- `cli.rs` parses the `waybar` group on all platforms (so help/usage stay
  uniform) but dispatch on non-Linux returns a clear error:
  `wsx waybar is only available on Linux (waybar integration)`.
- Core modules get exactly three narrow, platform-neutral seams; none of them
  mention waybar:
  1. A public `App::select_workspace_by_name(repo, slug) -> bool` method
     (reusable by any future automation). The TUI has no event channel —
     background tasks lock the shared `Arc<Mutex<App>>` and mutate directly,
     so the listener follows that established pattern instead of an event.
  2. A `--select <repo>/<slug>` launch flag that opens the TUI attached to
     that workspace.
  3. One `#[cfg(target_os = "linux")]` call at TUI startup that starts the
     IPC listener thread (the thread itself lives in `src/waybar/ipc.rs` and
     talks to the app only through the existing event channel).
- `src/waybar/` depends on core (store/data layer); core never depends on
  `src/waybar/`.

## `wsx waybar status`

Reads state.db through the existing data layer (same source as
`wsx workspace list`, `wsx status`, `wsx recap`). Prints a single JSON object:

- `text`: icon + count of workspaces, e.g. `󰙅 4`.
- `class`: highest-attention status across all workspaces, priority
  `blocked > done > waiting > working > idle`. Waybar applies it as a CSS
  class so the module turns e.g. red when anything is blocked, blue when
  something is done and awaiting review.
- `tooltip`: tree of **all registered repos** (including ones with zero
  workspaces) → workspaces, each line: status glyph, `repo/slug`, and the
  agent-set status message. Newline-separated plain text, markup escaped.

Errors (missing/locked db, no repos): print `{"text": ""}` and exit 0 so the
module hides instead of flashing errors in the bar.

## `wsx waybar menu`

- Lines: `repo/slug — <status message>` (message omitted when unset), sorted
  by repo then slug.
- Piped to `walker --dmenu` by default; the menu command is overridable via
  `WSX_WAYBAR_MENU` (any dmenu-compatible command). Walker missing and no
  override → error message via `notify-send` if available, else stderr.
- On selection, parse `repo/slug` back out and run the jump logic in-process.
- Escape/no selection → exit 0 silently.

## Jump

`wsx waybar jump <repo> <slug>` (also invoked by menu):

1. **Running TUI**: each TUI instance listens on a unix socket at
   `$XDG_RUNTIME_DIR/wsx/tui-<pid>.sock` (fallback `~/.local/state/wsx/run/`
   if `XDG_RUNTIME_DIR` unset). Socket removed on TUI exit; connect failure →
   treat as stale and skip/unlink. Jump connects to the first live socket and
   sends one line: `select <repo> <slug>\n`. The listener locks the shared
   app and calls `App::open_workspace_by_name`, which selects the workspace
   and attaches to it (same code path as pressing Enter on its row).
2. **Focus**: find the terminal window hosting that TUI — walk the `/proc`
   ppid chain upward from the TUI pid, match pids against `hyprctl clients
   -j`, then `hyprctl dispatch focuswindow pid:<terminal-pid>`. No hyprctl /
   not Hyprland → selection still happened; skip focus silently.
3. **No running TUI**: spawn `$TERMINAL` (fallback `alacritty`) detached,
   running `wsx --select <repo>/<slug>`.

Multiple live TUIs: use the most recently modified socket.

## `wsx setup waybar`

Follows the `wsx setup install-skill` precedent. Steps:

1. Write `~/.config/waybar/wsx.jsonc`: module definition
   (`exec: wsx waybar status`, `interval: 5`, `return-type: json`,
   `on-click: wsx waybar menu`, `tooltip: true`).
2. Write `~/.config/waybar/wsx.css`: classes for `blocked/done/waiting/
   working/idle` using omarchy theme variables where possible.
3. Patch `~/.config/waybar/config.jsonc` after making a timestamped `.bak`:
   add `"include": [".../wsx.jsonc"]` (or append to an existing include
   array) and insert `"custom/wsx"` as the first entry of `modules-right`
   (falling back to the last entry of `modules-left`), so the indicator
   leads the bar's right-side group. The patch is targeted text editing of
   jsonc; if the structure isn't confidently recognized, change nothing and
   print paste-ready snippets instead.
4. Print a note to `@import "wsx.css";` from `style.css` (CSS imports can't
   be injected safely) and to reload waybar (`omarchy-restart-waybar` if
   present, else `pkill -SIGUSR2 waybar`).

Idempotent: re-running overwrites `wsx.jsonc`/`wsx.css` and skips config
edits already present.

## Testing

- Unit (in `src/waybar/`, Linux-gated like the module): status JSON
  rendering, class priority mapping, tooltip escaping, menu line
  build/parse round-trip, IPC message parse, socket-path resolution.
- App level: `SelectWorkspace` event selects the right workspace (existing
  TUI test harness), `--select` flag parsing.
- Installer: patching logic against fixture jsonc files (recognized,
  unrecognized, already-installed).
- CI note: repo gates rustfmt, clippy, and tests separately — run
  `cargo fmt --check` too. Non-Linux builds must stay green: the module is
  cfg-gated, so `cargo check --target x86_64-pc-windows-msvc` (or at minimum
  reviewing cfg coverage) guards against leaks.
- Manual: run installer on this machine, verify live bar, tooltip, menu,
  jump-to-running-TUI, and jump-with-no-TUI.

## Open items deliberately deferred

- No waybar refresh signal on wsx mutations (5s poll is accepted staleness).
- No non-Hyprland window focusing.
- No packaging for bars other than waybar.
