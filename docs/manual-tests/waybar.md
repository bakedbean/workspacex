# Manual test: waybar indicator

The automated test suite covers the status payload, the jsonc patcher, and
the socket protocol. This procedure covers what tests can't: live glyph
rendering, the walker menu, and Hyprland window focus.

## Setup

Prereqs: omarchy/Hyprland, waybar running, walker installed.

## Test 1: install

```
wsx setup waybar
```

Expected: reports written files (`wsx.jsonc`, `wsx.css`) plus a patched
`config.jsonc` (backup path shown).

## Test 2: enable stylesheet + reload

Add `@import "wsx.css";` to `~/.config/waybar/style.css` if not already
present, then run `omarchy-restart-waybar`.

## Test 3: bar text + tooltip

Expected: the bar shows ` N` (N = total workspace count across all repos).
Hovering shows a tooltip listing every repo, its workspaces beneath, each
with a status glyph and (if set) its status message.

## Test 4: status class transitions

```
wsx status set blocked --message "x"
```

in some workspace. Expected: within 5s the module's class turns `blocked`
(color change per the stylesheet). `wsx status clear` reverts it.

## Test 5: click opens menu

Left-click the module. Expected: walker opens listing `repo/slug — message`
lines. Pressing Escape dismisses it with no side effects.

## Test 6: jump into a running TUI

With a wsx TUI already running, pick an entry from the menu. Expected: the
TUI's window is focused and the workspace is attached (opened, as if you
had pressed Enter on it — spawning its agent session if missing).

## Test 7: jump launches a new TUI

Quit all wsx TUIs, then pick an entry from the menu. Expected: a new
terminal opens running wsx already attached to that workspace.

## Test 8: stale socket fallback

Kill a running TUI with `SIGKILL` (leaving a stale socket behind), then
click the module and pick an entry. Expected: the jump still works — it
falls back to launching a new TUI and removes the stale socket.

## Test 9: idempotent install

```
wsx setup waybar
```

again. Expected: "already" messages for each file/config; no second backup
is created. The elephant menu line is the exception: it always reports
`installed elephant menu: …` (never "already"), by design — the file is
deliberately overwritten every run to refresh the baked wsx binary path.

## Elephant menu (walker rich picker)

Prereqs: omarchy host with walker + elephant running; `wsx` installed to
`~/.local/bin` (rebuild ≠ install — copy the binary).

- [ ] `wsx setup waybar` prints `installed elephant menu: …/elephant/menus/wsx.lua`,
      `restarted elephant …` (or the restart hint), and
      `installed walker theme: …/walker/themes/wsx`
- [ ] `elephant listproviders` shows `menus:wsx`; a `journalctl --user -u
      elephant` query line for `menus:wsx` reports results in ms, not µs
      (µs-fast empty results = dead registration, restart elephant)
- [ ] `wsx waybar menu-entries --json | jq .` shows one object per workspace
      with text/subtext/icon/action; repo/slug sorted; branch in subtext
- [ ] Click the waybar module: walker opens the rich menu — two-line rows
      (branch/status on the dimmer second line), wide window with no
      ellipsized rows at normal lengths, status glyph icons (a blocked
      workspace shows a glyph, never the red missing-icon symbol)
- [ ] Rows show dirty ● and +N −N immediately after touching a worktree
- [ ] With a PR open on a branch: PR indicator appears (immediately if the
      TUI is running; otherwise by the second menu open, after the sweep)
- [ ] Enter on a row jumps/attaches the workspace (same as dmenu behavior)
- [ ] Esc closes without jumping; no fallback dmenu double-opens
- [ ] `WSX_WAYBAR_MENU="wofi --dmenu" wsx waybar menu` still uses the pipe
- [ ] `mv ~/.config/elephant/menus/wsx.lua{,.off} && wsx waybar menu` falls
      back to the plain dmenu pipe (restore the file after)
