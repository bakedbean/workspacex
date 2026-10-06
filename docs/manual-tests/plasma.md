# Manual test: KDE Plasma applet

The automated test suite covers the installer (package layout, the baked
wsx path, idempotent re-runs), the `wsx desktop status` payload the applet
renders, the terminal a jump launches, and the KWin focus script with its
D-Bus clients. This procedure covers what tests can't: the applet loading
in plasmashell, live rendering, the popup, and KWin window focus.

## Setup

Prereqs: KDE Plasma 6 on Linux, at least one repo registered with wsx, and
the wsx binary you're testing installed where `wsx setup plasma` will find
it (`~/.local/bin/wsx` when present).

## Test 1: install

```
wsx setup plasma
```

Expected: reports `installed Plasma applet:
~/.local/share/plasma/plasmoids/io.github.bakedbean.wsx`, plus hints for
adding it to a panel and restarting plasmashell. The package holds
`metadata.json` and `contents/ui/main.qml`, and `main.qml`'s `wsx`
property names your wsx binary.

## Test 2: re-run is idempotent

Run `wsx setup plasma` again. Expected: same report, no error, no
`*.wsx-tmp.*` files left in the package directory.

## Test 3: add to a panel

Right-click a panel → *Add or Manage Widgets*, search "wsx", and drag it
onto the panel. Expected: a branch icon followed by N (N = total workspace
count across all repos), the same count the waybar module shows.

## Test 4: tooltip

Hover the applet. Expected: a "wsx" tooltip listing every repo, its
workspaces beneath, each with a status glyph and (if set) its status
message. A message containing `<b>` or `&` shows literally, not as markup.

## Test 5: status colors

```
wsx status set blocked --message "x"
```

in some workspace. Expected: within 5s the icon and count turn the color
scheme's negative (red) color. `done` → active (blue), `waiting` → neutral
(orange/yellow), `working` → positive (green); with no status set anywhere
they use the normal text color. `wsx status clear` reverts.

## Test 6: popup

Click the applet. Expected: a popup lists workspaces under a header per
repo, each row showing the status glyph (colored like the indicator), the
slug, the status message beneath it, and the PR number once wsx has
cached it. A message with line breaks
(`wsx status set working --message $'a\nb\nc'`) stays on one line.
Clicking the applet again closes it; with no workspaces it reads
"No workspaces".

## Test 7: jump into a running TUI

With a wsx TUI running in a terminal window, focus another window, open
the popup and click a workspace. Expected: the popup closes, the TUI's
window comes to the front with keyboard focus (KWin script over D-Bus), and
the workspace is attached, as if you had pressed Enter on it. Picking two
workspaces in quick succession raises the window both times. With
`dbus-send` off plasmashell's `PATH`, focus still works through `qdbus6`;
with no D-Bus client at all, the workspace is still attached and the popup
stays open with a "can't raise the TUI's window" warning.

## Test 8: jump launches a new TUI

Quit all wsx TUIs, then pick a workspace from the popup. Expected: a new
terminal opens running wsx already attached to that workspace: the
`terminal_cmd` template when it contains `{cmd}`, else `$TERMINAL`, else
konsole in a Plasma session that has it, else alacritty. With a terminal
that can't launch (`kquitapp6 plasmashell && TERMINAL=no-such-term kstart
plasmashell`, then pick a workspace), the popup stays open and shows wsx's
`failed to launch terminal 'no-such-term'` error; a `terminal_cmd` template
naming a missing terminal shows `exited with status 127, command not
found`.

## Test 9: no repos / unreadable database

With no repos registered (e.g. `XDG_STATE_HOME` pointed at an empty
directory before plasmashell starts), expected: the icon shows dimmed with
no count and the tooltip reads "No workspaces". When the database can't be
read for three polls in a row (about 10 seconds), the icon dims and the
tooltip and the popup show `Could not read wsx's status: …`; a database
that's busy for a moment keeps the last status instead of flashing the
error.

## Test 10: moved binary

Move the wsx binary named in `main.qml` away and wait 5s. Expected: the
icon dims and the tooltip reads "Could not run …" with the shell's error.
Move it back (or re-run `wsx setup plasma`) to recover.

## Test 11: vertical panel

Add the applet to a vertical panel. Expected: the count sits below the icon
instead of beside it, and nothing is clipped.

## Test 12: upgrade

Change something visible in the installed `main.qml`, re-run
`wsx setup plasma`, then restart plasmashell
(`kquitapp6 plasmashell && kstart plasmashell`).
Expected: the panel shows the freshly installed version (plasmashell keeps
the old QML until it restarts).

Runtime QML errors don't show in the panel; they go to plasmashell's log.
`plasmawindowed io.github.bakedbean.wsx` runs the applet in its own window
and prints them to the terminal instead.
