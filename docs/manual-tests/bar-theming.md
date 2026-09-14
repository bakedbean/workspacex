# Manual test — Starship-style bar theming

Spec: `docs/superpowers/specs/2026-09-13-bar-theming-design.md`

Run everything against a scratch config and state directory so your real
configuration is untouched. Run this in a dedicated shell, and use the same
exported paths in the second terminal where you edit the file:

```bash
scratch=$(mktemp -d /tmp/wsx-theme-test.XXXXXX)
export XDG_CONFIG_HOME="$scratch/config"
export XDG_STATE_HOME="$scratch/state"
printf 'XDG_CONFIG_HOME=%s\nXDG_STATE_HOME=%s\n' "$XDG_CONFIG_HOME" "$XDG_STATE_HOME"
```

Register a scratch repo and create a workspace in this isolated state.
For the populated-segment checks, prepare a workspace with a PR, a dirty
diff, a running process, pinned commands, model/token data, and at least two
agents. Have another workspace needing attention. Use harmless pinned
commands. A missing data source legitimately hides its segment; do not
mistake that for overflow or a failed cross-bar move.

When editing an initialized file, replace keys in its existing tables;
do not append duplicate TOML tables. Unless a step says otherwise, keep
unmentioned defaults.

## 0. The switch

`bar_theme` defaults to off. With a valid `theme.toml` in place and the app
running, `wsx config set bar_theme on` changes the bars within a second;
`wsx config set bar_theme off` snaps them back to the stock look and clears
any error notice. `wsx theme check` prints a `note:` line while the setting
is off. Enable it before the steps below.

## 1. Stock look with no file

Start `wsx` without a theme file. Expected: the dashboard footer and, after
attaching, the top and bottom bars preserve the stock content and styling.
Compare at roomy widths first. At narrow widths, the new layout must reserve
one blank column between nonempty left and right sides; a partially clipped
pinned chip remains clickable on its visible portion.

## 2. Init and check

In the second terminal, with the same scratch paths:

```bash
wsx theme path     # prints $XDG_CONFIG_HOME/wsx/theme.toml
wsx theme init     # writes it; a second run refuses to overwrite
wsx theme check    # succeeds for the generated file
```

Confirm the second init leaves the file unchanged. Leave wsx running:
loading the initialized default must preserve its appearance. Also try
`wsx theme check /path/to/a/scratch/candidate.toml` with an existing candidate
to verify validation can target a file other than the installed theme.

## 3. Powerline top bar, live

With wsx attached to a workspace, edit these tables in the file:

```toml
[attached_top]
format = "[](fg:#3a3a3a)[ $workspace ](bg:#3a3a3a)[](fg:#3a3a3a)( $attention)"

[workspace]
format = "[($repo/)$name]($style)"
style = "fg:#d75f00 bold"
```

Expected on the next roughly one-second reload check: the top bar shows the
workspace name in orange on a grey block with rounded caps; attention items
follow only when present. The terminal font must contain the actual powerline
cap glyphs `` and `` used above. Check a workspace without a repo name: no
orphan slash should appear.

Change the workspace style to `bold`. Its state/default foreground should
return while the enclosing grey background stays. The custom format applies
the resolved segment style at `($style)`; an enclosing bar foreground does
not override an explicit foreground inside a segment.

## 4. Overflow order and minimum blank gap

Restore the default segment styles and the attached bottom formats:

```toml
[attached_bottom]
format       = "$keys  ($pins  )"
right_format = "( ($agents   )($model_tokens )($procs )($diff )$pr)"
fill         = "─"
fill_style   = "fg:dim"
```

With all right-side segments populated, resize the terminal narrower step
by step. Expected drop order: model + tokens first, then agent pills, then
the process count, then the diff, with the PR chip surviving longest. At
every width where both sides remain, at least one separating column is
**blank**, not a fill glyph. The left side is clipped rather than dropped.
Expand the terminal again; the removed right-side segments should return.

For a clearer gap check without default padding, temporarily use:

```toml
[attached_bottom]
format       = "L"
right_format = "$version"
fill         = "─"
fill_style   = "fg:dim"
```

Expected: `L` is at the left edge, the version is at the right edge, and the
gap contains fill plus at least one blank column. Near the fit boundary,
the version must be removed rather than touch `L`. Repeat with `fill = " "`.
Restore the defaults before continuing.

## 4b. The dashboard header

On the dashboard (not attached), restyle the wordmark:

```toml
[brand]
format = "[ $name $mark ](bg:brand fg:black bold)[ · $view](fg:dim)"
```

Expected on the next reload: the top line opens with `workspace x` in black
on the brand blue, the ` · dashboard` tail still dim, and the `group:` and
`sort:` tabs unmoved after it. The active tab in each pair keeps the
selection highlight. Press `G` and `o` and confirm the highlight follows the
mode.

Then narrow the terminal step by step with no filter active. Expected drop
order on that line: the `sort:` tabs first (priority 30), then the
repo/workspace counts (50); the wordmark, the `group:` tabs, and the filter
echo never drop. Press `/` and type a long needle: the echo appears after
the tabs, capped at 24 characters with a trailing `…`, and it costs the
counts before it costs itself. Very narrow plus a long needle clips the line
at the right edge rather than shrinking the needle further — an accepted
difference from the stock header. Widen again; everything returns.

Restore `[brand]` before continuing.

## 5. Broken edit

Change `$workspace` in `[attached_top].format` to `$nope` and save.
Expected: the bars keep their previous look. Detach promptly to see the
dashboard footer's first theme error in red; it expires after about five
seconds. The diagnostic should identify `[attached_top].format` and the
unknown `$nope` variable; do not depend on exact punctuation.

Run `wsx theme check`: expected exit status 1 and the same underlying error.
Introduce a second independent validation error, such as an unknown color,
and check that CLI output and the log include both, and that the footer
notice appends `(+1 more)` to the first error. Fix the file: the bars
update and the notice clears. Also try an invalid file at startup: wsx must
use the bundled default instead of failing to start.

## 6. Clicks follow segments within and between bars

First move `$pr` from `[attached_bottom].right_format` to the start of
`[attached_bottom].format`, keeping `$keys` after it. Expected: clicking the
PR chip on the left still opens the PR, and clicking the `^x menu` hint still
opens the leader menu.

Then move `$pr` to `[attached_top].right_format`, removing its old occurrence.
Expected: the same PR action works in the top bar, and its former bottom-bar
position no longer triggers a PR action. Move it back and check again.

Exercise each remaining clickable segment in **both** attached bars, one at
a time to avoid overflow hiding it. Move the segment rather than duplicating
it, and test both left and right placement:

| Segment | Expected action at its new position |
|---|---|
| `keys` | Each hint performs its displayed action; the leader-menu hint opens the menu. |
| `usage` | Opens the usage graph. |
| `attention` | Each item activates its corresponding attention target. |
| `pins` | Each chip runs its corresponding harmless pinned command. |
| `agents` | Each pill selects the corresponding agent. |
| `procs` | Opens the process view. |

Also place `agent_bar`, `workspace`, `version`, `model_tokens`, and `diff`
in each attached bar to check non-clickable content availability. Check that
conditional decorations disappear when a segment has no data. On the
dashboard, `keys`, `version`, and `usage` remain available and interactive
where applicable; referencing attached-only segments there renders empty.
The dashboard header's five segments (`brand`, `group`, `sort`, `filter`,
`counts`) are display only — clicking them does nothing — and render empty
in every other bar.

Use a workspace name containing a double-width character and repeat a click
on a segment following it. Resize until a clickable chip is partly clipped:
only its visible cells should respond, with no stale hit target beyond the
bar's visible content.

## 7. Palette and state-style isolation

Define a palette entry named `selected_bg` and use it in a bar style.
Expected: the bar uses that palette entry, but dashboard selection and other
base-theme UI are unchanged. Change a PR segment style to a background-only
style and keep its default format: its lifecycle foreground and separate
review-verdict mark must remain state-colored. Change it to `fg:#d75f00`:
the lifecycle foreground becomes orange, while the review mark retains
`$mark_style`. Confirm `dimmed` is accepted as a modifier and `fg:dim` as a
color token.

Restyle `[pins]` (e.g. `style = "bg:second"`, or change its `format`).
Expected: the attached bottom bar's pin chips pick up the change, and so do
the dashboard's own DETAIL pane's pinned-command chips — the pane shown when
a workspace row is selected, separate from the attached view — since that
row is built from `[pins]` too, through the `[dashboard_detail]` bar.

Exit the scratch wsx session when finished. Close the dedicated shells to
restore your normal environment; remove only the scratch directory you
created once it is no longer needed.
