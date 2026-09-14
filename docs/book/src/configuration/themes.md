# Themes

wsx has two layers of theming: a **base palette** chosen with the `theme`
setting, and a **bar theme file** that describes what the dashboard header
and footer, the attached view's top and bottom bars, and the dashboard
detail pane's pinned-command row contain and how each piece is styled,
using a subset of Starship's format grammar.

## Base palette

```bash
wsx config set theme dracula
wsx config set theme jellybeans
wsx config set theme nord
wsx config set theme wsx        # default
wsx config set theme default    # ANSI colors that follow your terminal
```

The base palette colors repo headers, the selected row, status dots, modals,
and markdown. Restart wsx after changing it. Its colors are also available
to the bar theme file as *theme tokens* (below). The bar theme file does not
change the base palette or the appearance of other UI elements.

## Bar theme file

Ready-to-use examples live in the repo under `docs/examples/`:

| File | Look |
|---|---|
| `theme-starship.toml` | Powerline blocks in six stepped greys with orange accents, in the style of a starship prompt. |
| `theme-rose-pine.toml` | Rosé Pine (main) in an airline layout. |
| `theme-rose-pine-moon.toml` | Rosé Pine Moon, the darker and more muted variant, same layout. |
| `theme-nord.toml` | Nord, same layout. |
| `theme-nord0.toml` | Nord one step darker: base blocks on nord0, so the middle of each bar melts into a nord terminal background. |
| `theme-jellybeans.toml` | Jellybeans, same layout. |
| `theme-orange.toml` | Dark orange, converted from a vim-airline theme: an orange block at each edge, then the greys stepping up from near-black toward the middle. |

The airline files share one arrangement and differ only by palette: a
bright "mode" block at each outer edge, a mid-toned block beside it, and a
base-toned block toward the middle, so the eye lands on the edges first.
The two Nord files pair with `wsx config set theme nord`, and Jellybeans and
Orange with `wsx config set theme jellybeans`, so the rest of the UI
matches; Rosé Pine has no built-in base palette, so leave the default `wsx`.

To use one, copy it to `~/.config/wsx/theme.toml`, turn the feature on, and
validate:

```
cp docs/examples/theme-starship.toml ~/.config/wsx/theme.toml
wsx config set bar_theme on
wsx theme check
```

To keep several on hand and switch between them, copy the files somewhere
stable and make `theme.toml` a symlink you re-point. wsx fingerprints the
file the link resolves to, so re-pointing it reloads the bars within a
second in the running app:

```
mkdir -p ~/.config/wsx/themes
cp docs/examples/theme-*.toml ~/.config/wsx/themes/
ln -sfn themes/theme-nord.toml ~/.config/wsx/theme.toml      # switch
ln -sfn themes/theme-rose-pine.toml ~/.config/wsx/theme.toml # switch again
```

They need a Nerd Font (for the `` / `` caps) and a truecolor terminal.
Those caps are private-use characters (U+E0B0 and U+E0B2), so copy the files
as bytes (`cp`, `scp`, a dotfiles repo) rather than pasting them through a
chat or editor that strips unknown glyphs; if they go missing, `wsx theme
check` still passes but the blocks render with flat edges.

The bar theme is opt-in. Turn it on with

```
wsx config set bar_theme on
```

and off again with `wsx config set bar_theme off` (the default). While off,
wsx draws its stock bars and never reads the file; the `wsx theme` commands
below still work, so you can prepare and validate a file before enabling it.
The setting is re-read once a second, so switching either way takes effect
in the running app without a restart.

```bash
wsx theme path     # where wsx looks: ~/.config/wsx/theme.toml
wsx theme init     # write the bundled default there (never overwrites)
wsx theme check    # validate and print every error; exit 1 on any
```

The path honors `XDG_CONFIG_HOME`: when set to an absolute path, the file is
`$XDG_CONFIG_HOME/wsx/theme.toml`; a relative or unset value falls back to
`~/.config/wsx/theme.toml`. `wsx theme check [path]` can also validate
another file before you install it.

The file is optional. Anything you leave out keeps the bundled default,
which preserves wsx's stock bar content and styling, so `wsx theme init`
gives you a commented starting point. wsx checks for changes once a second
and reloads edits while running. If a save has an error, the bars keep their
last good look; in its place, the dashboard footer shows the first error in
red for five seconds (`(+N more)` when there is more than one), and the
full list goes to the log. An invalid file at startup falls back to the
bundled default.

The loader reports every field-level problem it finds in one pass — an
unknown segment, a bad color, a variable a format isn't allowed to use, and
so on, each with its own location. A TOML syntax error or a format-string
parse error is different: it stops parsing right there, so only the first
such error in that string is reported, not every one that string might
contain.

### Differences from the stock bars

A handful of narrow, accepted gaps between the engine and the bars it
replaced:

- A workspace with no PR leaves one blank cell at the chip row's right
  edge instead of hugging it exactly (the stock chip row's `$pr` is bare,
  with no trailing separator of its own).
- Below roughly 107 columns, the dashboard footer drops the version string
  first, then the usage graph, instead of overflowing the terminal width.
- A pinned chip clipped by the right edge keeps its visible portion
  clickable, rather than being dropped in full.
- The dashboard header's filter echo is capped at 24 characters and never
  shrinks further. The stock header instead budgeted the needle against
  whatever room was left on the line, so the repo/workspace counts always
  survived. Now a long needle costs the counts (`priority` 50) first, and
  below roughly 80 columns the header runs long and is clipped at the right
  edge. Why the echo itself never drops is unchanged: a needle with no
  visible cause is worse than a truncated one — rows are missing from the
  list and nothing on screen says why.

### Bars

```toml
[dashboard_header]
format       = "$brand      $group(   $sort)(  $filter)"
right_format = "$counts"
fill         = " "

[dashboard_footer]
format       = "$keys"
right_format = "($version  )$usage"

[attached_top]
format = "($agent_bar )$workspace(   $attention)"

[attached_bottom]
format       = "$keys  ($pins  )"
right_format = "( ($agents   )($model_tokens )($procs )($diff )$pr)"
fill         = "─"
fill_style   = "fg:dim"

[dashboard_detail]
format     = "($pins  )"
fill       = "─"
fill_style = "fg:dim"
```

`[dashboard_header]` is the dashboard's top line: the wordmark, the `group:`
and `sort:` mode tabs, the live filter echo, and the repo/workspace counts
flush right. Its five segments are display only — nothing on that line is
clickable.

`[dashboard_detail]` is the dashboard's own DETAIL pane (the pane shown when
a workspace row is selected, distinct from the attached view): its
pinned-command chip row, followed by a rule to the edge. `$pins` is the only
data-bearing segment there — every other registered segment renders empty if
you put it in this bar's format.

The `($version  )` group around `$version` and its trailing two spaces means
that separator drops along with `$version` itself when the footer is too
narrow for both — the same "put separators inside the group" rule described
under Grammar below.

| Key | Meaning |
|---|---|
| `format` | The left side, clipped at the right edge if too long. |
| `right_format` | Flush right. |
| `style` | Base style inherited by literal text, segment content, and the fill; inner styles can override it. |
| `fill` | The first character repeated across the unused gap. |
| `fill_style` | Style for the fill, merged over the bar's base style. |

When both sides are nonempty, at least one column between them stays blank,
even with a visible `fill` character. This blank column counts when deciding
whether the right side fits. The fill occupies the remaining gap.

**Overflow.** When a bar is too narrow for both sides plus that blank
column, segments with a `priority` below 100 may drop — from either side,
lowest first, re-evaluating both sides after each removal so conditional
groups shed their separators with them — until the bar fits or nothing
droppable is left. Segments at the default priority (100) never drop. If
the sides still don't fit after that, the right side is omitted entirely
rather than partially rendered, and the left side is clipped at the right
edge.

### Grammar

| Syntax | Meaning |
|---|---|
| `$name`, `${name}` | Insert a segment (inside a segment's `format`: one of its variables). |
| `[text](style)` | Style a run. Inner runs inherit the outer background unless they set their own, so powerline blocks compose. |
| `( … )` | Render only if a `$name` inside produced output. Put separators inside the group so they vanish with the segment. |
| `$$`, `\[`, `\(`, `\x` | Literal characters: `$`, `[`, `(`, or the escaped character `x`. |

In TOML double-quoted strings, write a backslash as `\\`; TOML single-quoted
literal strings can contain format escapes directly, such as `'\[$workspace\]'`.

A **style** is space-separated tokens: `fg:<c>`, `bg:<c>`, a bare `<c>`
(foreground), `bold`, `dimmed`, `italic`, `underline`, `none`, and `$style`
(the segment's resolved style, see below). `none` is accepted for Starship
compatibility but does nothing: it is a no-op, not a reset, so it neither
clears inherited attributes nor cancels other tokens in the same style.
A **color** is `#rrggbb`, a 0–255
index, an ANSI name (`red`, `bright-blue`, `white`), a `[palette]` name, or
a theme token: `dim path code bg_alt bg_soft ok warn err attention merged
header_fg selected_fg selected_bg question stalled waiting thinking complete
idle brand wordmark`. Palette names shadow theme tokens, which shadow ANSI names.
Shadowing changes color lookup in the bar theme only; it never changes the
base `Theme` fields. `fg:dim` selects a color; `dimmed` is a text modifier.

### Segments

Each segment has its own table, such as `[workspace]`, with `format` (its
layout, using the variables below), `style`, `symbol`, `disabled`,
`priority` (overflow survival; higher lasts longer; unset defaults to 100,
which never drops), and, for multi-item segments, `separator`. A multi-item
segment's format describes one item; the items keep their existing order.
The bundled default sets `priority` on the segments that compete for room:
`model_tokens` 10, `agents` 20, `procs` 30, `diff` 40, `pr` 50 (the
attached chip row's right side); `version` 50, `usage` 60 (the dashboard
footer's right side); `sort` 30, `counts` 50 (the dashboard header). Every
other segment is the unset default, 100, and so never drops.

| Segment | Variables | Notes |
|---|---|---|
| `brand` | `$symbol $name $mark $view` | The wordmark. `$name` is `workspace`, `$mark` is `x`, `$view` names the view (`dashboard`). Dashboard header only. |
| `group` | `$label $tabs` | The `group:` mode tabs; `$tabs` is opaque, with the active mode highlighted. Dashboard header only. |
| `sort` | `$label $tabs` | The `sort:` mode tabs, same shape as `group`. Dashboard header only. |
| `filter` | `$needle` | The live filter echo, absent when no filter is active; `$needle` is capped at 24 characters. Dashboard header only. |
| `counts` | `$repos $workspaces` | Registered repo and workspace counts. Dashboard header only. |
| `keys` | `$key $label` | One pill per key hint. Clickable. |
| `version` | `$version` | |
| `usage` | `$label $spark` | The activity sparkline. Clickable. |
| `agent_bar` | `$symbol` | `$style` includes the agent's identity color. Attached only. |
| `workspace` | `$repo $name` | `$repo` is absent when there is no repo name. |
| `attention` | `$items` | Cross-workspace attention list. Clickable. |
| `pins` | `$index $label` | One chip per pinned command. Clickable. |
| `agents` | `$symbol $label $key` | One pill per agent (2+ agents). `$style` includes the agent color. `symbol` is ignored — the pill always uses a filled/hollow dot to show which agent is active. Clickable. |
| `model_tokens` | `$model $tokens` | `$style` includes `ok`, or `warn` near the context limit. |
| `procs` | `$symbol $count` | Hidden at zero. Clickable. |
| `diff` | `$added $removed` | Hidden when clean. |
| `pr` | `$symbol $number $label $mark` | `$style` includes the lifecycle tint; `$mark_style` supplies the review verdict style. Clickable — except over a remote (ssh) attach, where the chip still renders but isn't clickable (opening a PR keys off a local workspace id a remote attach doesn't have). |

`pr`, `procs`, `usage`, and `attention` each carry exactly one click
target, unlike `pins`/`agents`/`keys`, which record one hit per item. Put
one of these four in more than one place across the two attached bars'
`format`/`right_format` (or twice within the dashboard footer's own
`format`/`right_format`, or twice within the dashboard header's, or twice
within the dashboard detail pane's) and only the last-routed placement
would be clickable, so `wsx theme check` rejects it as a duplicate instead.
These four scopes are independent: a singleton segment may appear once in
each without conflicting with the others.

All segments are available in **either attached bar**, on either side;
click targets follow them between bars as well as within a bar. `version`
and `usage` work in all three bars, not just the dashboard footer — put
`$usage` in an attached bar and its sparkline is the same graph, clickable
the same way. `keys` uses the attached view's leader-key hints in both
attached bars. On the dashboard footer, only `keys`, `version`, and `usage`
produce output; on the dashboard header, only `brand`, `group`, `sort`,
`filter`, and `counts`; on the dashboard detail pane's row, only `pins`;
other segments render empty in each. Segments also render empty when their
underlying data is absent.

Two details of the **stock formats** are worth knowing before you override
them:

- `style` only reaches the output through `$style`. The stock formats of
  `agent_bar`, `workspace`, `agents`, `model_tokens`, `procs`, and `pr`
  bind it (`[…]($style)`), so setting `style` on those works as written.
  The stock formats of `keys`, `pins`, `version`, `usage`, `attention`, and
  `diff` style their parts directly instead (or, for `attention`, not at
  all), so a bare `style = …` on one of those has no effect unless you also
  put `$style` in its `format`.
- `symbol` is substituted into `format`, but the literal spacing around it
  stays. Emptying one (`[pr]` `symbol = ""`) leaves the space that follows
  `$symbol` in the stock format; delete that space in `format` too if you
  want the glyph gone entirely.

#### Overriding a segment's style

A segment's `style` is merged over its state-derived default attribute by
attribute and exposed as `$style` **inside style expressions in that
segment's format**. For example, `style = "bg:second"` preserves the PR's
state-derived foreground, while `style = "fg:rust"` replaces it. Use
`[$name]($style)` in a custom segment format to apply that resolved style:

```toml
[palette]
rust = "#d75f00"

[workspace]
format = "[($repo/)$name]($style)"
style = "fg:rust bold"
```

Here `rust` must be defined in `[palette]`, as in the example below. Setting
a segment's `style` does not forcibly recolor every nested run: an explicit
style on an inner run can override inherited attributes. Likewise, putting
`[$workspace](fg:rust)` in a bar format supplies an inherited foreground,
not an override of the segment's own foreground. For the PR review mark,
use `[$mark]($mark_style)` to retain its separate verdict color rather than
applying the lifecycle style to it.

### A powerline example

These examples use the real rounded powerline caps `` and ``; your
terminal font needs to include them. Conditional groups hide whole blocks,
including their caps and spaces, when the enclosed segment is empty.

```toml
[palette]
first  = "#121212"
second = "#3a3a3a"
rust   = "#d75f00"

[attached_top]
format = "[](fg:first)[ $workspace ](bg:first)[](fg:first)( [](fg:second)[ $attention ](bg:second)[](fg:second))"

[attached_bottom]
right_format = "([](fg:second)[ $agents ](bg:second)[](fg:second))(  [](fg:first)[ $pr ](bg:first)[](fg:first))"

[workspace]
format = "[($repo/)$name]($style)"
style = "fg:rust bold"

[pr]
style = "fg:rust"
```
