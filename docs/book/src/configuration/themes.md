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
| `theme-rose-pine-moon.toml` | Rosé Pine Moon, the softer, slightly lighter dark variant, same layout. |
| `theme-nord.toml` | Nord, same layout. |
| `theme-nord0.toml` | Nord one step darker: base blocks on nord0, so the middle of each bar melts into a nord terminal background. |
| `theme-jellybeans.toml` | Jellybeans, same layout. |
| `theme-orange.toml` | Dark orange, converted from a vim-airline theme: an orange block at each edge, then the greys stepping up from near-black toward the middle. |

The Rosé Pine, Nord, and Jellybeans files share one arrangement and differ
only by palette: generally a bright "mode" block at each outer edge, a
mid-toned block beside it, and a base-toned block toward the middle. The
attached bottom bar instead starts with a dark Menu block. Orange keeps
the airline layout but splits more pieces into
their own blocks (view, repos and workspaces, and each item on the attached
bottom bar's right side). Across all seven example themes, the attached
bottom bar starts with Menu on the same dark background as the top agent
block, followed by separate pinned-command and Tags blocks. Orange keeps
its blank orange stub only on the dashboard footer.
In every file the dashboard header's wordmark stays flat on the bar in the
app's brand colours (the bundled default's blue bar and "x"), so the left
chain starts at the block beside it rather than on a mode block.
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
file the link resolves to by its modification time, size, and permission
bits, so re-pointing it reloads the bars within a second in the running
app. Two targets with identical metadata would not be told apart; if a
switch ever fails to show, `touch` the file the link now points at.

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
- When the dashboard footer is too narrow for its key hints plus the
  right side, it drops the version string first, then the `funnel`
  module, instead of overflowing the terminal width. Where that happens
  depends on how much the funnel has to say, since each stage renders
  only while its count is non-zero.
- A pinned chip clipped by the right edge keeps its visible portion
  clickable, rather than being dropped in full.
- The stock attached view draws a dim `─` rule under its top bar to set it
  off from the pane. With `bar_theme` on, the themed bar's own blocks do
  that job, so the rule row is dropped and the pane gains a row.
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
right_format = "$version(  $funnel)"

[attached_top]
format = "($agent_bar )$workspace(   $attention)"

[attached_bottom]
format       = "$keys  ($pins  )($tags  )"
right_format = "( ($agents   )($model_tokens )($procs )($diff )$pr)"
fill         = "─"
fill_style   = "fg:dim"

[dashboard_detail]
format     = "($pins  )"
fill       = "─"
fill_style = "fg:dim"
```

`[dashboard_footer]`'s right side is `$version` and the bundled `funnel`
module — see [Modules](#modules) below for what a module is and how to
replace or restyle it.

`[dashboard_header]` is the dashboard's top line: the wordmark, the `group:`
and `sort:` mode tabs, the live filter echo, and the repo/workspace counts
flush right. Its five segments are display only — nothing on that line is
clickable.

A theme that sets its own `[attached_bottom].format` keeps that layout
unchanged — add `$tags` to it yourself to get the prompt-tag chips (the
keyboard chord works either way).

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
idle brand wordmark agent_claude agent_pi agent_hermes agent_codex agent_omp`
(the `agent_*` tokens are each agent kind's fixed identity colour, the same
in every theme). Palette names shadow theme tokens, which shadow ANSI names.
Shadowing changes color lookup in the bar theme only; it never changes the
base `Theme` fields. `fg:dim` selects a color; `dimmed` is a text modifier.

### Segments

Each segment has its own table, such as `[workspace]`, with `format` (its
layout, using the variables below), `style`, `symbol`, `disabled`,
`priority` (overflow survival; higher lasts longer; unset defaults to 100,
which never drops), a `palette` sub-table (see
[Recolouring one segment](#recolouring-one-segment)), and, for multi-item
segments, `separator` and `styles`. A multi-item segment's format
describes one item; the items keep their existing order.
`separator` is a format too — `"[ │ ](fg:dim)"` draws a dim joiner — but
it sits between items rather than inside one, so it takes no variables and
no `$style`. Because it is parsed with the grammar above, a separator that
wants a literal `$`, `[`, `(`, or backslash must escape it (`$$`, `\[`,
`\(`, `\\`), and a bare `(x)` is a conditional group that renders nothing;
a plain run of spaces or box-drawing characters needs no change. `attention`
alone also takes `more_format`, the tail drawn when entries don't fit the
bar; its one variable is `$count`, the number of entries folded into it,
and setting it on any other segment is an error. `attention` also takes
`more_style`, the tail's own `$style` (see the caps below). Likewise `agent_bar`
alone takes a `symbols` sub-table, one glyph per agent kind, tried ahead
of `symbol`:

```toml
[agent_bar]
symbol = "\ue0b0"        # kinds without an entry below (Nerd Font chevron)

[agent_bar.symbols]
claude = "\uec82"
codex  = "\uec81"
```

Keys must be agent kind names; any other key, or the table on another
segment, is an error. Entries union per kind over the bundled default,
yours winning, like a segment palette. An empty entry (`pi = ""`) is an
override, not an absence: that kind shows no glyph rather than `symbol`.
The `agents` pills read the same table through their `$icon` variable,
and a `[module.<name>]` format through `$icon_<kind>`, so each harness's
glyph is drawn once and appears everywhere the theme names it.

An item whose `format` renders empty — an empty `format`, or one whose
variables are all absent for that item — is dropped as if it were never in
the list: no separator, no click target, no grade, and not counted in the
tail.

#### Grading items by position

A multi-item segment also takes `styles`, a list of style strings: the
first rendered item's `$style` is the segment's usual style patched by
`styles[0]`, the second's by `styles[1]`, and so on, with items past the
end of the list all taking its last entry. A bg-only grade keeps the
provider's state colour in the foreground (an attention entry's
PR-lifecycle tint, an agent pill's identity colour). Positions count
rendered items, so an empty item takes no grade with it, and an attention
entry folded into the tail does not either.

To draw powerline caps between graded blocks, the formats of a multi-item
segment may name six extra colours: `item_fg`/`item_bg` are the item's own
final `$style` colours, `prev_*` and `next_*` those of its rendered
neighbours. `separator` sees `prev_*` and `next_*` (the items on each side
of it); `more_format` sees `prev_*` (the last rendered entry). A colour
that does not exist — the first item's `prev`, the last rendered item's
`next`, or a grade that never set that colour — carries nothing: that
token sets nothing, whatever `$style` or an enclosing run already set
stays, and where nothing set it the bar's own style shows through. That
is what lets the last block's trailing wedge blend into the bar without
the theme knowing how many entries there are, provided the wedge sits
outside the graded background run (as below), not inside it.

`attention`'s fold tail joins the run through `more_style`: a style
string patched over the segment's `style`, like a grade. It is the tail's
`$style` and its `item_*` colours, and the last rendered entry's `next_*`
when the tail follows it — so that entry's trailing wedge points into the
tail, and into the bar only when the entry really is last. Without
`more_style` the tail has no colours of its own: `$style` there is empty,
and the last entry's `next` is absent even when a tail follows. Like
`styles`, it may not name the six colours.

```toml
[attention]
styles      = ["bg:charcoal fg:orange", "bg:slate fg:cream", "bg:grey fg:cream"]
more_style  = "bg:orange fg:black"
format      = "[ $glyph $repo/$name \\($age\\) ]($style)[\ue0b0](fg:item_bg bg:next_bg)"
separator   = ""
more_format = "[ +$count more ]($style)[\ue0b0](fg:item_bg)"
```

Here each block, entry or tail, carries its own trailing wedge, coloured
from its block into the next; the first block's leading cap belongs in
the bar format, where `styles[0]` is known. The formats are TOML double-quoted strings so
that `\ue0b0` decodes to the wedge glyph and `\\(` reaches the grammar as
`\(`; in a single-quoted literal string `\ue0b0` would stay as typed and
render as the five characters `ue0b0`. `styles` entries may not use the
six names themselves (a grade cannot depend on the neighbours that depend
on it), `wsx theme check` rejects `styles` and the six names on a
single-item segment, and the six names are reserved: a `[palette]` entry
by one of them is an error, since inside a multi-item segment it would be
shadowed by the per-item colour.
The bundled default sets `priority` on the segments that compete for room:
`model_tokens` 10, `agents` 20, `procs` 30, `diff` 40, `pr` 50 (the
attached chip row's right side); `tags` 40 (the same row's left side);
`version` 50, the `funnel` module 60 (the dashboard footer's right side,
and `usage` keeps its 60 for a theme that places it back); `sort` 30,
`counts` 50 (the dashboard header). Every other segment is the unset
default, 100, and so never drops.

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
| `agent_bar` | `$symbol` | `$style` includes the agent's identity color. `$symbol` is the focused agent's entry in `[agent_bar.symbols]` (keys `claude`, `pi`, `hermes`, `codex`, `omp`) when the theme sets one, else `symbol`. Attached only. |
| `workspace` | `$repo $name` | `$repo` is absent when there is no repo name. `$style` includes the PR-lifecycle tint (green open, purple merged, red closed), or the header style without a PR. Attached only. |
| `attention` | `$glyph $repo $name $age` | One item per workspace needing attention. `$glyph` is the entry's dashboard status glyph in its status color; `$style` is the name's PR-lifecycle tint (open, merged, …) or the muted `path` hue. Entries that don't fit fold into `more_format` (`$count`); the first entry always renders, and if it alone would push the tail off the bar its `$name` is shortened with an ellipsis (assuming one `$name` in the format; a format without `$name`, or a very long `$repo`, has nothing to yield and simply clips). Clickable: each entry, and the tail. |
| `pins` | `$index $label` | One chip per pinned command. Clickable. |
| `tags` | `$index $label` | The three most-used prompt tags as chips, then the manager chip from `more_format` (`$count` = saved tags). Attached only. Clickable: each chip, and the manager. |
| `agents` | `$symbol $icon $label $key` | One pill per agent (2+ agents). `$style` includes the agent color. `symbol` is ignored — the pill always uses a filled/hollow dot to show which agent is active. `$icon` is the pill's kind's entry in `[agent_bar.symbols]`, absent for a kind without one. Clickable. |
| `model_tokens` | `$model $tokens` | `$style` includes `ok`, or `warn` near the context limit. |
| `procs` | `$symbol $count` | Hidden at zero. Clickable. |
| `diff` | `$added $removed` | Hidden when clean. |
| `pr` | `$symbol $number $label $mark` | `$style` includes the lifecycle tint; `$mark_style` supplies the review verdict style. Clickable — except over a remote (ssh) attach, where the chip still renders but isn't clickable (opening a PR keys off a local workspace id a remote attach doesn't have). |

`pr`, `procs`, `usage`, `attention`, and `tags` may each be placed only
once: `pr`, `procs`, and `usage` carry exactly one click target; `attention`
and `tags`, though each records one hit per entry/chip like
`pins`/`agents`/`keys`, also carry a single tail target — `… +N more` for
`attention`, the manager chip (`more_format`) for `tags` — and each is
fitted to the one bar that places it. Put one of these five in more than
one place across the two attached bars' `format`/`right_format` (or twice
within the dashboard footer's own `format`/`right_format`, or twice within
the dashboard header's, or twice within the dashboard detail pane's) and
only the last-routed placement would be clickable, so `wsx theme check`
rejects it as a duplicate instead. These four scopes are independent: a
singleton segment may appear once in each without conflicting with the
others.

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
  `agent_bar`, `workspace`, `attention`, `agents`, `model_tokens`, `procs`,
  and `pr` bind it (`[…]($style)`), so setting `style` on those works as
  written. The stock formats of `keys`, `pins`, `version`, `usage`, and
  `diff` style their parts directly instead, so a bare `style = …` on one
  of those has no effect unless you also put `$style` in its `format`.
- Bare parentheses are the conditional-group syntax, so a literal pair
  must be escaped. The stock `attention` item format writes its age as
  `[ \($age\)](fg:dim)` in a TOML literal string for exactly this reason;
  an unescaped `($age)` renders the age without the parentheses.
- Upgrading a theme written before `attention` became multi-item: its
  old `[attention] format = "$items"` is now rejected as an unknown
  variable — delete the table to take the stock item format, or rewrite it
  with the variables above. There is no `$items` alias.
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

#### Recolouring one segment

`[palette]` names shadow theme tokens everywhere in the file. A segment
can carry its own `[<segment>.palette]` too, with the same value grammar
(plus: a value may name a global `[palette]` entry, so `ok = "green"` is
the theme's own green rather than ANSI's), that shadows both the global
palette and the theme tokens **inside that segment only** — for colour names in its `format`, `style`, `styles`,
`separator`, and `more_format`, and for the tokens behind its
state-derived `$style`. This is how a theme darkens the lifecycle tints
on a light block without changing them on a dark one: the same `ok` that
tints an open PR green on the dark `attention` run is too pale on a bright
"mode" block, so the segments that sit there take darker greens of their
own.

```toml
[palette]
orange = "#d75f00"

[attached_bottom]
right_format = "[$pr](bg:orange)"

[pr.palette]
ok     = "#008700"   # open, darker than the theme's `ok`
merged = "#870087"
err    = "#870000"
warn   = "#878700"   # conflict

[workspace.palette]
ok     = "#008700"
merged = "#870087"
err    = "#870000"
warn   = "#878700"
header_fg = "black"  # the no-PR fallback keeps the block's black text
```

The overlay is only a colour lookup: it cannot add attributes or change
which token a state uses (`ok` for open, `merged`, `err` for closed,
`warn` for conflict; `header_fg` for `workspace` without a PR or on a
draft, `dim` for the `pr` chip on a draft; the six status tokens for
`attention`'s `$glyph`; `selected_fg`/`selected_bg`/`path` for the
`group`/`sort` tabs). This is the one place a palette reaches a
state-derived `$style`: the global `[palette]` shadows tokens only where a
format names them (`fg:ok`), never the colour a segment derives for its
own state. A segment palette's values may reference `[palette]` names but
not each other, so there are no local aliases. A name defined only in a
segment's palette is unknown outside it, so `wsx theme check` reports a
bar format that uses one. The six per-item names are reserved here as in
`[palette]`.

### Modules

A module is a segment you compose yourself. Declare it as a
`[module.<name>]` table and place it in any bar as `$<name>`:

```toml
[module.funnel]
format   = "([$working working](fg:ok)  )([$blocked blocked](fg:err)  )([$mergeable ready](fg:merged))"
priority = 60

[dashboard_footer]
right_format = "$version(  $funnel)"
```

A module table takes `format`, `style` (patched over the bar style to form
`$style`), `priority`, and `disabled` — nothing else, since a module has no
items. Its `format` may reference only the fleet variables below, which
describe every workspace on the dashboard at once. A count renders empty
when it is zero, so wrap each item in a `( … )` group to drop it along with
its label and gap; `$workspaces` and `$repos` always render a number. The
`tokens_*` variables are sums of context size rather than counts; they render
abbreviated (`77k`, `1.2M`) and are likewise empty at zero.

A module's name may not be the name of a built-in segment (`keys`, `usage`,
`pr`, …). The bundled default defines two modules: `funnel`, placed where
the usage graph used to be, and `tokens` — context fill per agent kind
(`claude 1.2M  codex 340k`), defined but not placed. Set only the fields
you want to change to restyle either, or define your own and put that in
the bar instead. To show the token module, or bring the sparkline back,
place `$tokens` or `$usage`:

```toml
[dashboard_footer]
right_format = "$version(  $tokens)(  $funnel)(  $usage)"
```

Modules carry no click target.

| Variable | Counts |
|---|---|
| `working` `waiting` `blocked` `done` | workspaces whose last `wsx status set` is that state |
| `busy` | workspaces parked on background work (hook-inferred) |
| `unreported` | workspaces with no reported status |
| `alerts` | workspaces with an unacknowledged attention alert |
| `awaiting` `stalled` `active` `idle` | workspaces by live transcript classification |
| `live_agents` | workspaces with a live (thinking or waiting) primary session |
| `pr_none` `pr_draft` `pr_open` `pr_conflicted` `pr_merged` `pr_closed` | workspaces by PR lifecycle |
| `review_required` `changes_requested` `approved` | PRs by review verdict |
| `unresolved` | unresolved review threads across the fleet |
| `mergeable` | PRs that are open and approved |
| `dirty` | workspaces with modified or untracked files |
| `msgs_queued` | agent-to-agent messages not yet delivered |
| `workspaces` `repos` | totals (always rendered) |
| `tokens_total` | Σ latest reported context size (prompt-side tokens) across every agent instance whose transcript is still cached, primary and peers |
| `tokens_claude` `tokens_pi` `tokens_hermes` `tokens_codex` `tokens_omp` | the same, per agent kind (hermes reports no usage, so it is always empty) |
| `icon_claude` `icon_pi` `icon_hermes` `icon_codex` `icon_omp` | the kind's glyph from `[agent_bar.symbols]` — the theme's, not the fleet's; absent for a kind without an entry. A *label*: it renders beside a count but, like literal text, never keeps a `( … )` group alive by itself, so `([$icon_pi $tokens_pi])` drops with the count exactly as `[pi $tokens_pi]` does |

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
