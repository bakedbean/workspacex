# Themes

wsx has two layers of theming. The `theme` setting picks a base palette for the whole UI. The optional bar theme file, `theme.toml`, controls what the dashboard and attached-view bars show and how they look, using a subset of Starship's format grammar.

## Quick start

```bash
wsx theme init                  # write the bundled default to ~/.config/wsx/theme.toml
$EDITOR "$(wsx theme path)"
wsx theme check                 # print every error; exit 1 on any
wsx config set bar_theme on
```

- `bar_theme` is off by default. While it's off, wsx draws its stock bars and never reads the file. The `wsx theme` commands still work, so you can prepare a file first.
- Switching `bar_theme` either way, and saving the file, take effect within a second without a restart.
- If a save has errors, the bars keep their last good look and the dashboard footer shows the first error for five seconds. An invalid file at startup falls back to the bundled default.
- The file is optional, and anything you leave out keeps the bundled default.
- The path is `$XDG_CONFIG_HOME/wsx/theme.toml` when `XDG_CONFIG_HOME` is an absolute path, and `~/.config/wsx/theme.toml` otherwise.
- `wsx theme check <path>` validates another file before you install it.

See [`wsx theme`](../cli-reference/other.md#wsx-theme-check) for the command syntax.

## Base palette

```bash
wsx config set theme nord       # wsx (default), default, dracula, jellybeans, nord
```

The base palette colors repo headers, the selected row, status dots, modals and markdown. `default` uses your terminal's ANSI colors. Restart wsx after changing it. The bar theme file can use the palette's colors as [theme tokens](#styles-and-colors), but can't change them.

## Example themes

Ready-made files live in the repo under `docs/examples/`:

| File | Look | Pair with `theme` |
|---|---|---|
| `theme-starship.toml` | Powerline blocks in stepped greys with orange accents. | `wsx` |
| `theme-rose-pine.toml` | Rosé Pine in an airline layout. | `wsx` |
| `theme-rose-pine-moon.toml` | Rosé Pine Moon, same layout. | `wsx` |
| `theme-nord.toml` | Nord, same layout. | `nord` |
| `theme-nord0.toml` | Nord one step darker, blending into a Nord terminal. | `nord` |
| `theme-jellybeans.toml` | Jellybeans, same layout. | `jellybeans` |
| `theme-orange.toml` | Dark orange, from a vim-airline theme. | `jellybeans` |

```bash
cp docs/examples/theme-starship.toml ~/.config/wsx/theme.toml
wsx config set bar_theme on
```

They need a Nerd Font and a truecolor terminal. The powerline caps (U+E0B0, U+E0B2) are private-use characters, so copy the files as bytes (`cp`, `scp`, git). Pasting through a chat or editor can strip them: `wsx theme check` still passes, but the blocks lose their edges.

The examples place `$statuses`, which wsx builds before the segment existed don't recognize: `wsx theme check` fails, and the running app keeps its last good theme. Use the examples from the release you run.

To switch between several themes, make `theme.toml` a symlink and re-point it:

```bash
mkdir -p ~/.config/wsx/themes
cp docs/examples/theme-*.toml ~/.config/wsx/themes/
ln -sfn themes/theme-nord.toml ~/.config/wsx/theme.toml
```

The running app reloads within a second. If a switch doesn't show, `touch` the new target.

## Bars

The bundled default:

```toml
[dashboard_header]
format       = "$brand(  $filter)    ($statuses)"
right_format = "$counts"
fill         = " "

[dashboard_footer]
format       = "$keys"
right_format = "$funnel"
fill         = " "

[attached_top]
format = "($agent_bar )$workspace(  $workspace_tags)(   $attention)"

[attached_bottom]
format       = "$keys  ($pins  )($tags  )"
right_format = "( ($agents   )($model_tokens )($procs )($diff )$pr)"
fill         = "─"
fill_style   = "fg:dim"

[dashboard_detail]
format     = "($pins  )"
fill       = "─"
fill_style = "fg:dim"

[dashboard_detail_header]
format = "$agent_bar $workspace  $branch(  $pr)(  $diff)(  $procs)  $status"
fill   = " "

[dashboard_detail_reply]
format       = "($pins  )$prompt "
right_format = "$keys"
fill         = " "
```

| Bar | Where |
|---|---|
| `dashboard_header` | The dashboard's top line: wordmark, filter echo, status tally, counts. Only the tally's items are clickable. When its formats don't place `$statuses`, or `[statuses]` sets `disabled`, the tally gets its own row below it instead. That depends only on placement: a `priority` that lets `$statuses` drop on a narrow terminal doesn't bring the row back. |
| `dashboard_footer` | The dashboard's bottom line: key hints and the [`funnel` module](#modules). |
| `attached_top` | The attached view's top bar. |
| `attached_bottom` | The attached view's bottom bar (chip row). |
| `dashboard_detail` | The detail pane's pinned-command row. Drawn only when the reply row leaves out `$pins` or this bar places a module. |
| `dashboard_detail_header` | The detail pane's top row. |
| `dashboard_detail_reply` | The detail pane's reply row, laid out like a shell prompt. |

Each bar takes these keys:

| Key | Meaning |
|---|---|
| `format` | The left side, clipped at the right edge if too long. |
| `right_format` | Flush right. |
| `style` | Base style for literal text, segment content and the fill. |
| `fill` | Character repeated across the unused gap. |
| `fill_style` | Style for the fill, merged over `style`. |

When both sides are non-empty, at least one blank column separates them.

### Overflow

When a bar is too narrow, segments with a `priority` below 100 drop, lowest first, from either side, until the bar fits. Segments at the default priority of 100 never drop. If it still doesn't fit, wsx omits the right side and clips the left.

The bundled priorities:

| Bar | Priorities |
|---|---|
| `attached_bottom` | `model_tokens` 10, `agents` 20, `procs` 30, `diff` 40, `tags` 40, `pr` 50 |
| `dashboard_footer` | `hosts` 45, `version` 50, `tokens` 55, `funnel` 60, `usage` 60 |
| `dashboard_header` | `sort` 30 (when placed), `counts` 50 |

### The reply row

`[dashboard_detail_reply]`'s `format` is the prompt before your draft and `right_format` is a right prompt after it. The draft fills the gap and always keeps at least 12 cells. To make room, the right side drops first, then the prompt's droppable segments.

`$keys` holds the `↵ send` / `Esc cancel` hint and shows only while the row has focus (`Tab` from a selected workspace).

When `$pins` is on the reply row, on either side, the separate pinned-command row folds away and the chips stay clickable on the reply row. To get the separate row back, leave `$pins` out: `format = "$prompt "`. The example themes place the chips on a block:

```toml
[dashboard_detail_reply]
format       = "([ $pins ](bg:soot fg:orange)[\ue0b0](fg:soot) )$prompt "
right_format = "([\ue0b2](fg:ash)[ $keys ](bg:ash fg:orange))"
```

### Unplaced segments

The bundled default registers some segments without placing them:

- `$version`, the running wsx version: `right_format = "$version(  $funnel)"`.
- `$hosts`, one clickable chip per [shared host](../integrations/shared-workspaces.md): `right_format = "$hosts(  $funnel)"`. A chip opens that host's workspace list, like `H`.
- `$usage`, the activity sparkline; `usage_graph_window` sets its range.
- `$tokens`, a module showing context size per agent kind.

A theme that sets its own `[attached_bottom].format` must add `$tags` itself to show the prompt-tag chips.

### Differences from the stock bars

With `bar_theme` on:

- The attached view drops the dim `─` rule under its top bar, so the pane gains a row.
- A workspace with no PR leaves one blank cell at the chip row's right edge.
- A narrow dashboard footer drops the version, then the funnel, instead of overflowing.
- The dashboard header's filter echo is capped at 24 characters and never drops. A long filter drops the counts first.

## Grammar

| Syntax | Meaning |
|---|---|
| `$name`, `${name}` | Insert a segment (inside a segment's `format`, one of its variables). |
| `[text](style)` | Style a run. Inner runs inherit the outer background unless they set their own, so powerline blocks compose. |
| `( … )` | Render only if a `$name` inside produced output. Put separators inside the group so they vanish with the segment. |
| `$$`, `\[`, `\(`, `\x` | Literal `$`, `[`, `(`, or `x`. |

In TOML double-quoted strings, write a backslash as `\\`. Single-quoted literal strings take format escapes as-is: `'\[$workspace\]'`. Use double quotes when you need `\ue0b0` to decode to a glyph.

Bare parentheses are a conditional group, so escape a literal pair: `\($age\)`. An unescaped `($age)` drops the parentheses.

### Styles and colors

A style is space-separated tokens:

- `fg:<color>`, `bg:<color>`, or a bare `<color>` (foreground).
- `bold`, `dimmed`, `italic`, `underline`.
- `none`, accepted for Starship compatibility. It does nothing and doesn't reset anything.
- `$style`, the segment's resolved style.

A color is one of:

- `#rrggbb`, or a 0–255 index.
- An ANSI name: `red`, `bright-blue`, `white`.
- A `[palette]` name.
- A theme token: `dim row_dim path code bg_alt bg_soft ok warn err attention merged header_fg selected_fg selected_bg question stalled waiting thinking complete idle brand wordmark agent_claude agent_pi agent_hermes agent_codex agent_omp`. The `agent_*` tokens are each agent's fixed identity color.

Palette names shadow theme tokens, which shadow ANSI names. This only affects the bar theme. `fg:dim` is a color, while `dimmed` is a text modifier.

```toml
[palette]
rust = "#d75f00"
```

## Segments

Each segment has its own table, such as `[workspace]`, with these keys:

| Key | Meaning |
|---|---|
| `format` | The segment's layout, using its variables. |
| `style` | Merged over the segment's state-derived style and exposed as `$style`. |
| `symbol` | Substituted for `$symbol`. |
| `disabled` | Hide the segment. |
| `priority` | Overflow survival; higher lasts longer. Default 100 never drops. |
| `palette` | A sub-table of colors for this segment only. See [Recoloring one segment](#recoloring-one-segment). |
| `separator` | Multi-item segments: a format drawn between items. It takes no variables. |
| `styles` | Multi-item segments: per-position styles. See [Grading items by position](#grading-items-by-position). |
| `more_format` | `attention`: the tail when entries don't fit. `tags`: the manager chip. Its one variable is `$count`. |
| `more_style` | `attention` only: the tail's `$style`. |
| `symbols` | `agent_bar` only: one glyph per agent kind. |

| Segment | Variables | Notes |
|---|---|---|
| `brand` | `$symbol $name $mark $view` | The wordmark. `$view` is `dashboard`, not placed by default. Dashboard header only. |
| `group` | `$label $tabs` | The `group:` mode tabs; the active one is highlighted. Not placed by default. Dashboard header only. |
| `sort` | `$label $tabs` | The `sort:` mode tabs. Not placed by default. Dashboard header only. |
| `statuses` | `$glyph $count $label` | One item per [status](../dashboard/status-indicators.md#status-glyphs) with a nonzero count, most urgent first, counting filtered-out workspaces too. `$style` is the status color, reversed on the status the list is [filtered to](../dashboard/status-indicators.md#filtering-by-status) and dimmed on the rest. Dashboard header only. Clickable. Older wsx builds reject a theme that uses it. |
| `filter` | `$needle` | The live filter, capped at 24 characters. Dashboard header only. |
| `counts` | `$repos $workspaces` | Repo and workspace counts. Dashboard header only. |
| `keys` | `$key $label` | One pill per key hint. Clickable. |
| `version` | `$version` | |
| `usage` | `$label $spark` | The activity sparkline. Clickable. |
| `agent_bar` | `$symbol` | The agent's glyph in its identity color. |
| `workspace` | `$repo $name` | `$style` carries the PR-lifecycle tint (green open, purple merged, red closed). `$repo` is absent in the detail pane. |
| `workspace_tags` | `$tag` | One item per [workspace tag](../dashboard/tags-and-hiding.md), dimmed. Attached view only; empty over a remote attach. A custom `attached_top` must place it to show tags. |
| `branch` | `$symbol $branch` | The git branch. `symbol` defaults to `⎇`. |
| `prompt` | `$symbol $agent` | The reply row's prompt character. `symbol` defaults to `❯`. Bold in the agent's color while the row has focus. |
| `status` | `$glyph $label $ago` | The workspace's status as its row shows it. `$ago` is how long it has held. Detail header only. |
| `attention` | `$glyph $repo $name $age` | One item per workspace needing attention. Overflow folds into `more_format`. Clickable. |
| `pins` | `$index $label` | One chip per pinned command. Clickable. |
| `hosts` | `$index $name $dest` | One chip per shared host; `$name` is truncated to 14 columns. Dashboard footer only. Clickable. |
| `tags` | `$index $label` | The three most-used prompt tags, then the manager chip. Attached view only. Clickable. |
| `agents` | `$symbol $icon $label $key` | One pill per agent when there are two or more. `$symbol` shows which agent is focused, or a spinner while it works. Clickable. |
| `model_tokens` | `$model $tokens` | `$style` turns `warn` near the context limit. |
| `procs` | `$symbol $count` | Hidden at zero. Clickable. |
| `diff` | `$added $removed` | Hidden when clean. |
| `pr` | `$symbol $number $label $mark` | `$style` carries the lifecycle tint; `$mark_style` styles the review verdict. Clickable, except over a remote attach. |

Where segments produce output:

| Bar | Segments |
|---|---|
| Attached bars | All except the dashboard-header segments, `branch`, `prompt`, `status` and `hosts`. Either bar, either side. |
| `dashboard_footer` | `keys`, `version`, `usage`, `hosts` |
| `dashboard_header` | `brand`, `group`, `sort`, `statuses`, `filter`, `counts` |
| `dashboard_detail` | `pins` |
| `dashboard_detail_header` | `agent_bar`, `workspace`, `branch`, `pr`, `diff`, `procs`, `status` |
| `dashboard_detail_reply` | `prompt`, `agent_bar`, `workspace`, `branch`, `pins`, `keys` |

Other segments render empty there, and any segment renders empty when its data is absent. Modules work in every bar.

`pr`, `procs`, `usage`, `attention` and `tags` may appear only once per scope. The scopes are: the two attached bars together, the dashboard footer, the dashboard header, and each of the three detail-pane rows. `wsx theme check` rejects a duplicate.

Gotchas when overriding a segment:

- `style` reaches the output only through `$style`. The stock formats of `keys`, `pins`, `version`, `usage` and `diff` don't use `$style`, so `style` alone does nothing on them.
- Emptying `symbol` leaves the space after `$symbol` in the stock format. Remove it from `format` too.
- An item whose `format` renders empty is dropped, with no separator or click target.
- `[attention] format = "$items"` from older themes is rejected. Delete the table or use the item variables.
- A built-in segment name, such as `hosts`, can't be a module name.

### Overriding a segment's style

`style` merges over the segment's state-derived style attribute by attribute. `style = "bg:second"` keeps the PR's lifecycle foreground, while `style = "fg:rust"` replaces it.

```toml
[workspace]
format = "[($repo/)$name]($style)"
style  = "fg:rust bold"
```

An explicit style on an inner run still overrides `$style`. For the PR review mark, use `[$mark]($mark_style)` to keep its verdict color.

### Agent glyphs

`[agent_bar.symbols]` sets a glyph per agent kind, tried before `symbol`:

```toml
[agent_bar]
symbol = "\ue0b0"        # kinds without an entry

[agent_bar.symbols]
claude = "\uec82"
codex  = "\uec81"
```

Keys must be `claude`, `pi`, `hermes`, `codex` or `omp`. Entries merge over the bundled default. An empty entry (`pi = ""`) shows no glyph. The same glyphs appear as `$icon` in `agents` pills and as `$icon_<kind>` in modules.

### Grading items by position

A multi-item segment's `styles` is a list of styles. The first rendered item's `$style` is patched by `styles[0]`, the second by `styles[1]`, and items past the end take the last entry. A background-only grade keeps the item's state color in the foreground.

For powerline caps between graded blocks, a multi-item segment's formats can use six extra colors:

| Color | Meaning |
|---|---|
| `item_fg`, `item_bg` | This item's final `$style` colors. |
| `prev_fg`, `prev_bg` | The previous rendered item's colors. |
| `next_fg`, `next_bg` | The next rendered item's colors. |

`separator` sees `prev_*` and `next_*`, and `more_format` sees `prev_*`. A missing color, such as the last item's `next_bg`, sets nothing, so the bar's style shows through. That lets the last block's wedge blend into the bar.

`attention`'s `more_style` gives the fold tail its own colors, so the last entry's `next_*` points into the tail.

```toml
[attention]
styles      = ["bg:charcoal fg:orange", "bg:slate fg:cream", "bg:grey fg:cream"]
more_style  = "bg:orange fg:black"
format      = "[ $glyph $repo/$name \\($age\\) ]($style)[\ue0b0](fg:item_bg bg:next_bg)"
separator   = ""
more_format = "[ +$count more ]($style)[\ue0b0](fg:item_bg)"
```

Rules `wsx theme check` enforces:

- `styles` and `more_style` can't use the six neighbor colors.
- `styles` and the six colors are rejected on single-item segments.
- The six names can't be `[palette]` entries.

### Recoloring one segment

A `[<segment>.palette]` table overrides color names inside that segment only, including the tokens behind its state-derived `$style`. Use it to darken lifecycle tints on a light block:

```toml
[pr.palette]
ok     = "#008700"   # open
merged = "#870087"
err    = "#870000"   # closed
warn   = "#878700"   # conflict

[workspace.palette]
ok        = "#008700"
header_fg = "black"  # no PR
```

- Values may name global `[palette]` entries, but not each other.
- A name defined only in a segment palette is unknown elsewhere.
- It changes colors only, not attributes or which token a state uses.

The tokens each state uses:

| Segment | Tokens |
|---|---|
| `pr`, `workspace` | `ok` open, `merged`, `err` closed, `warn` conflict |
| `workspace` | `header_fg` without a PR or on a draft |
| `pr` | `dim` on a draft |
| `attention` `$glyph`, `statuses` | The six status tokens |
| `group`, `sort` | `selected_fg`, `selected_bg`, `path` |

## Modules

A module is a segment you compose from fleet-wide variables. Declare it as `[module.<name>]` and place it as `$<name>`:

```toml
[module.funnel]
format   = "([$working working](fg:ok)  )([$blocked blocked](fg:err)  )([$mergeable ready](fg:merged))"
priority = 60

[dashboard_footer]
right_format = "$funnel"
```

- A module takes `format`, `style`, `priority` and `disabled`. It has no click target.
- A count renders empty at zero, so wrap each item in `( … )` to drop it with its label. `$workspaces` and `$repos` always render.
- `tokens_*` values render abbreviated (`77k`, `1.2M`) and are empty at zero.
- The bundled default defines `funnel` (placed) and `tokens` (not placed). Set only the fields you want to change.

```toml
[dashboard_footer]
right_format = "$tokens(  $funnel)(  $usage)"
```

| Variable | Counts |
|---|---|
| `working` `waiting` `blocked` `done` | workspaces whose last `wsx status set` is that state |
| `busy` | workspaces parked on background work |
| `unreported` | workspaces with no reported status |
| `alerts` | workspaces with an unacknowledged attention alert |
| `awaiting` `stalled` `active` `idle` | workspaces by live transcript classification |
| `live_agents` | workspaces with a live primary session |
| `pr_none` `pr_draft` `pr_open` `pr_conflicted` `pr_merged` `pr_closed` | workspaces by PR lifecycle |
| `review_required` `changes_requested` `approved` | PRs by review verdict |
| `unresolved` | unresolved review threads |
| `mergeable` | PRs that are open and approved |
| `dirty` | workspaces with modified or untracked files |
| `msgs_queued` | agent-to-agent messages not yet delivered |
| `workspaces` `repos` | totals (always rendered) |
| `tokens_total` | sum of the latest context size across every agent, primary and peers |
| `tokens_claude` `tokens_pi` `tokens_hermes` `tokens_codex` `tokens_omp` | the same, per agent kind (`hermes` reports none) |
| `icon_claude` `icon_pi` `icon_hermes` `icon_codex` `icon_omp` | the kind's glyph from `[agent_bar.symbols]`; like literal text, it never keeps a `( … )` group alive alone |

## A powerline example

The rounded caps `\ue0b6` and `\ue0b4` need a Nerd Font. Conditional groups hide whole blocks, caps included, when the segment inside is empty.

```toml
[palette]
first  = "#121212"
second = "#3a3a3a"
rust   = "#d75f00"

[attached_top]
format = "[\ue0b6](fg:first)[ $workspace ](bg:first)[\ue0b4](fg:first)( [\ue0b6](fg:second)[ $attention ](bg:second)[\ue0b4](fg:second))"

[attached_bottom]
right_format = "([\ue0b6](fg:second)[ $agents ](bg:second)[\ue0b4](fg:second))(  [\ue0b6](fg:first)[ $pr ](bg:first)[\ue0b4](fg:first))"

[workspace]
format = "[($repo/)$name]($style)"
style  = "fg:rust bold"

[pr]
style = "fg:rust"
```
