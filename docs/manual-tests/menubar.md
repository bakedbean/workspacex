# Manual test: macOS menubar (SwiftBar)

The automated test suite covers the plugin document renderer, the refresh
sweep, the install shim, and the jump/copy-path commands. This procedure
covers what tests can't: live SwiftBar rendering, menubar color, and
terminal/app focus.

## Setup

Prereqs: SwiftBar installed and running, `wsx` at `~/.local/bin/wsx`, at
least one repo with workspaces, `gh` authed for PR indicators.

## Test 1: install

```
wsx setup menubar
```

Expected: reports the shim path (`wsx-menubar.10s.sh` in SwiftBar's plugin
folder). SwiftBar shows the wsx item (branch glyph + workspace count)
within ~10s.

## Test 2: header color

```
wsx status set blocked --message "x"
```

in some workspace. Expected: within a poll (~10s) the item turns red.
`wsx status clear` reverts it to the default color. Check both light and
dark system appearance.

## Test 3: menu contents

Click the item. Expected: repos as section headers (empty repos listed with
"(no workspaces)"), one row per workspace with a status glyph and slug in
monospace, aligned. A workspace with an open PR shows `#N` after the
background sweep (≤ ~2 min, or immediately if a TUI is running and has
polled).

## Test 4: dirty indicator

Touch a file in a worktree. Expected: a `●` (plus diff stats) appears
within ~70s (poll + sweep interval). Revert the change and it disappears.

## Test 5: submenu

Hover a row. Expected: a subtitle line (`branch — state: message`), then
Jump, Open PR (only when a PR is cached), Copy worktree path, and Reveal in
Finder.

## Test 6: jump into a running TUI

With a wsx TUI already running, choose Jump from a row's submenu. Expected:
the TUI selects and attaches that workspace, and the terminal app comes
frontmost.

## Test 7: jump launches a new TUI

Quit all wsx TUIs, then choose Jump. Expected: a new terminal window opens
(iTerm2 if installed, else Terminal; or your configured `terminal_cmd`
template) running `wsx --select repo/slug`.

## Test 8: error path

```
mv ~/.local/state/wsx/state.db{,.bak}
```

Expected: the item degrades to icon-only (no error text, no crash). Restore
the db (`mv ~/.local/state/wsx/state.db{.bak,}`) afterward.

## Test 9: unusual repo/slug/message content

A repo or slug with a space or `|` in it, and a status message with the
same, should render without breaking rows or `bash=`/`param=` values in the
menu.

## Test 10: Project Manager submenu

Prereq: at least one workspace with a recap. Set one with:

```
wsx recap set --goal "try the PM submenu" --state "checking rendering" --next "read the menu"
```

Click the menubar item and hover `Project Manager` (below the workspace
list, above `Refresh`).

Expected:

- The submenu opens and lists repos as headers, with each workspace's
  `goal:` / `state:` / `next:` lines beneath its name, and a facts line
  (`#12 draft · ● · +45 -12 · recap 2m`).
- Workspaces with no recap show `no recap yet`.
- Ordering differs from the list above: blocked first, then waiting, then
  the rest oldest-signal-first.
- Clicking a workspace's header line jumps to it, exactly like the
  top-level row.
- With many workspaces, the submenu scrolls rather than overflowing the
  screen.

Two rendering details cannot be unit-tested — check them explicitly:

1. **Separators.** The `-----` lines between workspaces must render as
   separator rules, not as literal `-----` text. If they render literally,
   change them in `pm_section_lines` to a `disabled=true` line containing a
   single NBSP.
2. **Indentation.** The recap and fact lines must sit indented under their
   workspace name. If they are flush left, SwiftBar trimmed the NBSPs —
   change `RECAP_INDENT` in `src/desktop/menubar/pm.rs` to `"\u{2502} "`.
