# Starship-style bar theming

wsx's three chrome bars — the dashboard footer, the attached view's top
info line, and the attached view's bottom chip row — are hand-written
span builders that read colors straight off the active `Theme`. This
spec makes their content and styling user-configurable through a
`theme.toml` file whose grammar is a strict subset of
[starship](https://starship.rs)'s: a `format` string of `$segment`
placeholders, `[text](style)` styled literals, `( )` conditional groups,
per-segment tables, and a named palette.

## Goals

- Let users reorder, omit, restyle, and decorate the segments of the
  three bars, including a powerline look (colored background blocks with
  separator glyphs), using starship syntax.
- Every piece of today's bar content is a named segment, including the
  clickable ones (key hints, pinned chips, agent pills, PR chip, procs
  count, usage graph, attention items). Click targets travel with the
  segment wherever it is placed.
- A missing or invalid file at startup uses the bundled default, preserving
  today's bar content and styling. A bad live edit keeps the last good theme.
- Edits to the file are picked up while wsx runs, without a restart.
- Validation is available from the CLI so errors are found before save.

## Non-goals

- Per-repo overrides. The file is global. Layering a per-repo override
  the way `detail_bar_config` does can come later.
- Theming the split-pane title bars, the separator rule under the top
  bar, the detail bar header, modals, or markdown.
- Replacing the built-in `Theme` structs (`wsx`, `ansi`, `dracula`,
  `jellybeans`, `nord`). The `theme` setting still selects the base
  palette; the file layers bar formats and extra colors on top.
- Mutating built-in `Theme` fields (e.g. `selected_bg`) from the file.
  Palette names may shadow tokens during bar-style lookup only.
- starship's `$fill` and `add_newline`, and per-item format strings
  inside multi-item segments.
- A live-preview editor inside wsx.

## File

The whole feature is opt-in behind the `bar_theme` global setting (default
`off`, added after the initial evaluation). While off, wsx draws the
bundled default and never reads or watches the file; `on` enables the
loading and reload described below. The setting is re-read on the same
once-a-second check as the file fingerprint, so it toggles live.

`~/.config/wsx/theme.toml`, resolved through `XDG_CONFIG_HOME` the same
way `Dirs` resolves the state dir (`src/config/mod.rs`). `Dirs` gains a
`config_dir()` and `theme_path()`.

The bundled default (`src/ui/bar/default_theme.toml`, embedded with
`include_str!`) has the same shape and preserves today's content and styling. A
user file is merged over it per key: an unmentioned bar or segment
table keeps its default.

```toml
# Named colors. Style strings can use these, plus the built-in theme
# tokens: dim, path, code, bg_alt, bg_soft, ok, warn, err, attention,
# merged, header_fg, selected_fg, selected_bg, question, stalled,
# waiting, thinking, complete, idle, brand.
[palette]
first  = "#121212"
second = "#262626"
rust   = "#d75f00"

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

# A powerline top bar:
# [attached_top]
# format = "[](fg:first)[ $workspace ](bg:first)[](fg:first)( [](fg:second)[ $attention ](bg:second)[](fg:second))"

[workspace]
style  = "fg:rust bold"
format = "[($repo/)$name]($style)"

[pr]
symbol = ""
format = "[$symbol #$number $label]($style)( [$mark]($mark_style))"
```

### Grammar

- `$name` or `${name}` inserts a segment; the braced form disambiguates
  a name from following text. `$$` is a literal dollar sign; `\x`
  escapes any single character (`\[`, `\(`, `\$`, …). An unknown name
  is a load-time error.
- `[text](style)` styles a literal run. `$name` inside is allowed and
  inherits the group's style as its base.
- `( … )` is a conditional group: rendered only if at least one
  `$name` inside produced non-empty output. Groups nest.
- A style string is space-separated tokens: `fg:<c>`, `bg:<c>`, a bare
  `<c>` meaning `fg:<c>`, and the modifiers `bold`, `dimmed`, `italic`,
  `underline`, `none`. A color `<c>` is `#rrggbb`, a 0–255 index, an
  ANSI name (`red`, `bright-blue`, …), a palette name, or a theme
  token. Palette names shadow theme tokens; theme tokens shadow ANSI
  names. Unknown names are load-time errors.
- Styles inherit inward: an inner `[ ](…)` keeps the outer `bg` unless
  it sets its own. This is what makes powerline blocks composable.
- `right_format` is right-aligned against the bar's edge with at least
  one blank column between it and `format` when both sides are nonempty.
  This column counts toward the fit calculation and remains blank even
  with a visible fill. When the bar is too narrow the right side is
  dropped segment-by-segment (see Overflow). `format` is never dropped,
  only clipped at the right edge.
- Each `[bar]` table accepts `format`, `right_format`, `style`, `fill`,
  and `fill_style`. `style` supplies the inherited base style; the first
  character of `fill` repeats across the remaining gap, with
  `fill_style` merged over the base. Each `[segment]` table accepts
  `style`, `symbol`, `format`, `disabled`, `priority`, and (for multi-item
  segments) `separator`. Inside style expressions in a segment's
  `format`, `$style` is the segment's resolved style; the remaining
  variables are listed per segment below.

### Segments

| Segment | Variables in its `format` | Hit | Non-empty in |
|---|---|---|---|
| `keys` | one pill: `$key $label` | `Key`/`ArmLeader` per pill | all three bars |
| `version` | `$version` | — | all three bars |
| `usage` | `$label $spark` | `UsageGraph` | all three bars |
| `agent_bar` | `$symbol` (fg = agent identity color) | — | attached |
| `workspace` | `$repo $name` | — | attached |
| `attention` | `$items` | `Attention(i)` | attached |
| `pins` | one chip: `$index $label` | `PinnedChip(i)` | attached, dashboard detail |
| `agents` | one pill: `$symbol $label $key` | `Agent(id)` | attached |
| `model_tokens` | `$model $tokens` | — | attached |
| `procs` | `$symbol $count` | `Procs` | attached |
| `diff` | `$added $removed` | — | attached |
| `pr` | `$symbol $number $label $mark`, `$mark_style` in styles | `Pr` | attached |

All segments are available in either attached bar, in `format` or
`right_format`, with click targets following between bars. A segment
without applicable data renders empty. The dashboard footer supplies only
`keys`, `version`, and `usage`; the dashboard detail pane's row supplies
only `pins`; all others render empty in each.
`keys` in either attached bar is the `^x` leader pill plus the
leader-prefixed hints that exist today.

Multi-item segments (`keys`, `pins`, `agents`) keep their item order
fixed. Their `format` describes one item; the engine repeats it per
item with `separator` (default two spaces) between, recording a hit
per item.

`pr`, `procs`, `usage`, and `attention` each carry one click target that
isn't indexed by item (unlike the multi-item segments above, which
record a hit per item and so tolerate repeats). Placing one of these in
more than one of the four attached-bar format strings — or twice within
the dashboard footer's own `format`/`right_format`, or twice within the
dashboard detail pane's own `format`/`right_format` — would draw two
chips with only the last-routed one clickable, so `resolve` rejects it:
an error naming the segment and how many placements were found. These
three scopes (the attached pair, the dashboard footer, the dashboard
detail pane) are checked independently, so a singleton may appear once
in each. This also fixes `attention_width_budget`'s measurement, which
otherwise can't know which placement to measure from.

`[agents].symbol` is ignored: `$symbol` for that segment is always a
filled or hollow dot showing which agent is active, not a
user-configurable glyph. `[pr].symbol`, by contrast, does override the
lifecycle glyph when set.

Providers supply a segment's *default* style from state: the PR lifecycle
tint, the agent identity color, and the `ok`/`warn` model-token tint. A user
`style` merges over that default attribute by attribute, so
`style = "bg:second"` keeps the state-derived foreground. A segment format
applies the result with `[… ]($style)`; setting `style` does not forcibly
recolor every nested run. Explicit inner styles override inherited
attributes, including an enclosing bar-format style. The PR review mark's
separate verdict style is exposed as `$mark_style`, used by
`[$mark]($mark_style)`.

### Overflow

Each segment has an integer `priority` (unset defaults to 100); higher
survives longer. The bundled defaults reproduce the chip row's current
drop order: `model_tokens` 10, `agents` 20, `procs` 30, `diff` 40, `pr`
50; every other segment 100. When `right_format` does not fit beside
`format` and the required one-column blank gap, the renderer removes
the lowest-priority segment present in `right_format`, re-evaluates the
AST (so conditional groups drop their separators), and repeats. If
every variable is removed and the remaining literal text still doesn't
fit, the right side is omitted entirely rather than partially rendered.
`format` is never dropped this way, only clipped at the right edge.

## Architecture

New module `src/ui/bar/` with four units plus one config loader.

### `src/ui/bar/format.rs` — parser

`parse(&str) -> Result<Vec<Node>, ParseError>`.

```rust
pub enum Node {
    Text(String),
    Var(String),
    Styled(Vec<Node>, StyleSpec),
    Group(Vec<Node>),
}
pub struct ParseError { pub offset: usize, pub message: String }
```

Pure. Knows nothing about themes or segments. Unbalanced `[`, `(`, a
`]` not followed by `(`, or an unterminated `(style)` are errors with
the byte offset.

### `src/ui/bar/style.rs` — style grammar

`StyleSpec::parse(&str) -> Result<StyleSpec, StyleError>` produces
unresolved color names and modifier flags.
`StyleSpec::resolve(&self, &Palette, &Theme) -> Result<Style, StyleError>`
maps names to `ratatui::Color`. Parsing and resolution are separate so
an unknown name is reported at load time, never at draw time.
`Style::patch` provides the inherit-inward and user-over-default merges.

### `src/ui/bar/segment.rs` — provider contract

```rust
pub enum Hit {
    Key(KeyEvent),
    ArmLeader,
    PinnedChip(usize),
    Pr,
    Procs,
    Agent(AgentInstanceId),
    UsageGraph,
    Attention(WorkspaceId),
    AttentionMore,
}
pub struct HitSpan { pub start_col: u16, pub width: u16, pub hit: Hit }
pub struct Segment {
    pub spans: Vec<Span<'static>>,
    pub width: u16,          // cells, not chars
    pub hits: Vec<HitSpan>,  // columns relative to the segment start
}
pub struct SegmentConfig {
    pub style: StyleSpec,    // unresolved; merged over the provider's
                              // state-derived style and resolved as $style
    pub symbol: Option<String>,
    pub format: Vec<Node>,   // parsed
    pub disabled: bool,
    pub priority: u32,
    pub separator: String,
}
```

Providers are plain functions, one per segment, moved out of today's
builders: `pr_chip_parts`, `diff_chip_parts`, `procs_chip_parts`,
`model_tokens_chip_parts` (`src/ui/attached/chip_row.rs`),
`agent_pills_spans` (`agents_row.rs`), `key_pill_spans` and the
`footer` key loop (`src/ui/dashboard/layout.rs:164`), `info_line`
(`src/ui/attached/mod.rs:301`), and the sparkline tail. Each takes its
`SegmentConfig` plus its app data and returns `Option<Segment>`;
`None` means empty, which lets `( )` groups collapse.

`Hit` replaces the three differently shaped output bundles
(`FooterHintAction`, `ChipRowOutput`, the attention click list) at the
segment layer. Most of the existing output structs the input handlers
consume are kept and filled from the hit list, but `src/app/input` is
not fully untouched: since a pinned chip's position is no longer fixed
to its command index (a theme can reorder, hide, or move `$pins`),
`App.chip_rects` changes shape from `Vec<Rect>` to `Vec<(usize, Rect)>`,
carrying the pinned-command index alongside each rect, and the mouse
handler (`src/app/input/mouse.rs`) fires the chip at the carried index
instead of a positional one.

### `src/ui/bar/render.rs` — evaluator

```rust
pub struct Rendered { pub line: Line<'static>, pub hits: Vec<HitSpan> }
pub fn render(bar: &BarSpec, segments: &SegmentMap, width: u16, base: Style) -> Rendered;
```

Walks the AST, threading the inherited style through nested `Styled`
nodes, evaluating `Group` by whether any `Var` inside produced a
segment, tracking the running column in cells (via `Span::width`) so
hits land correctly after double-width glyphs, then lays out
`right_format` and applies the overflow rule. Callers convert
`HitSpan` columns to absolute `Rect`s with the bar's origin, using the
existing `footer_hint_rects` helper generalized over `Hit`.

### `src/config/theme_file.rs` — loader

```rust
pub struct ThemeFile { palette, bars: BTreeMap<String, BarTable>, segments: BTreeMap<String, SegmentTable> }  // serde
pub struct BarSpecs { dashboard_footer: BarSpec, attached_top: BarSpec, attached_bottom: BarSpec, segments: HashMap<String, SegmentConfig> }
pub fn load(path: &Path, theme: &Theme) -> Result<BarSpecs, Vec<ThemeError>>;
pub fn bundled_default(theme: &Theme) -> BarSpecs;
```

`load` reads the TOML, merges it over the bundled default per key,
parses every format string, parses and resolves every style against
the merged palette and the base `Theme`, checks every `$name` against
the segment table, and returns either a fully resolved `BarSpecs` or
every error found (not just the first). `BarSpecs` holds parsed ASTs
and resolved `Style`s, so drawing does no parsing.

### Renderer integration

`App` gains `bar_specs: BarSpecs` and `theme_file_mtime`. `App::new`
loads after choosing the base theme. `draw_attached`
(`src/app/render/attached.rs`) and `render_footer`
(`src/ui/dashboard/mod.rs:349`) stop building spans: they gather the
app data they already gather, call the providers to fill a
`SegmentMap`, call `render`, and translate hits into the rects the
input handlers already read (`footer_hint_rects`, `chip_rects`,
`pr_link_rect`, `procs_link_rect`, `agent_chip_rects`, attention
rects). `layout_chrome` and `layout_chip_row` become internal to the
providers or go away.

## Loading, reload, and errors

- **Precedence.** Valid user file merged over bundled default;
  otherwise bundled default.
- **Reload.** On the existing periodic tick, at most once a second,
  re-stat the file; reparse only when the mtime changes. No file
  watcher dependency.
- **Parse or validation error** (bad TOML, unknown segment, unknown
  color, unbalanced brackets): keep the last good `BarSpecs`, log every
  error (each carrying a `[table].field` location), and show a one-line
  `err`-styled notice in place of the dashboard footer for five seconds
  — the first error, plus `(+N more)` when there is more than one
  (e.g. `theme.toml: [attached_top].format: unknown color \`rusty\``).
  At startup with no last-good spec, fall back to the bundled default.
- **Runtime emptiness** (every referenced segment is empty right now)
  is not an error; the bar renders blank.
- **CLI.** `wsx theme check [path]` validates and prints every error,
  exit 1 on any. `wsx theme path` prints the resolved location.
  `wsx theme init` writes the bundled default there if absent (refuses
  to overwrite). The `theme` setting is unchanged in meaning.

## Testing

Unit tests per module:

- `format.rs`: each node type; nesting; `$$`; error offsets for
  unbalanced `[`, `(`, `]` without `(`, unterminated style.
- `style.rs`: every token kind; palette, theme-token, and ANSI
  resolution and their shadowing order; unknown-name error; `bg`
  inheritance when an inner group omits it; user-over-default merge
  keeps unset attributes.
- `render.rs`: conditional group collapse with and without nested
  vars; style inheritance through three levels; hit columns after a
  double-width glyph in the workspace name; right-alignment gap of at
  least one cell; overflow drops lowest priority first and
  re-evaluates groups; `format` clips rather than drops.
- `theme_file.rs`: bundled default parses and validates; partial file
  merges per key over default; invalid file returns all errors and no
  spec; unknown `$name` is an error.
- **Parity snapshots** (the gate for deleting old code): for each bar,
  a one-off test rendered the bundled default through the engine and
  the same fixture inputs through the still-live legacy builder and
  asserted the `Line` cells (symbol, foreground, background) and the
  produced hit rects matched, at more than one width. Once that parity
  run passed and the legacy builder was deleted, the comparison was
  replaced by literal snapshot assertions (the engine's own output,
  pinned) so the suite has no ongoing comparison target. Two
  differences from the legacy bars were accepted rather than matched
  exactly: a workspace with no PR (so `$pr` renders bare, with no
  trailing separator of its own) leaves one blank cell at the right
  edge of the chip row instead of hugging it; and on a terminal too
  narrow for the dashboard footer's right side, `version` (priority 50)
  drops before `usage` (default priority 100), so the version string
  disappears first rather than both overflowing off-screen together.
- Reload: mtime change reparses; unchanged mtime does not; invalid
  edit keeps last good and sets the notice.

Manual walkthrough in `docs/manual-tests/bar-theming.md`: `wsx theme
init`; set the powerline example; resize to 60 columns and confirm
`model_tokens` drops first, then `agents`; introduce a typo and confirm
the last-good bar stays with the notice; fix it and confirm reload;
click a moved PR chip and confirm it still opens the PR.

Docs: a section in `docs/book/src/configuration/themes.md` covering
the file location, grammar, segment table, and the three CLI commands.

## Delivery

Eleven tasks on this branch (`docs/superpowers/plans/2026-09-13-bar-theming.md`),
each landed as one or more green commits (`cargo test`, clippy,
`cargo fmt --check` under the CI toolchain) plus any review fix rounds:

1. `toml` dependency; `Dirs::config_dir`/`theme_path`. No rendering
   change.
2. `format.rs`: the format-string parser, with tests.
3. `style.rs`: the style grammar and resolver, with tests.
4. `segment.rs` (`Hit`, `Segment`, `SegmentConfig`) and `render.rs`
   (the evaluator, overflow, and hit geometry), with tests.
5. `theme_file.rs`: the bundled default and the file loader/merger,
   with tests; unknown keys in `[bar]`/`[segment]` tables rejected.
6. `wsx theme check|path|init`.
7. Dashboard footer on the engine; parity proven against the legacy
   builder, then pinned as literal snapshots; old `footer` builder
   deleted.
8. Attached top bar on the engine; `info_line` deleted.
9. Attached bottom bar on the engine; priority overflow replaces
   `DROP_ORDER`; parity proven, then pinned; old chip-row builder
   deleted. Pinned-chip clicks needed to carry their command index
   (see Architecture) since a themed `$pins` no longer sits at a fixed
   position.
10. mtime reload and the error notice.
11. This documentation, the manual test, and this spec sync.

Tasks 1–6 are additive (parsers, loader, CLI; no rendering change).
Tasks 7–9 are net-negative refactors that delete the three legacy span
builders once each is proven equivalent. The feature is opt-in behind
the `bar_theme` setting described in the File section above (default
off), so drawing from the engine only happens once a user turns it on;
the bundled default it draws from otherwise reproduces today's bar
content and styling exactly, with two accepted, narrow exceptions — at
the right edge of the chip row for a workspace with no PR, and in the
drop order at the narrow-terminal boundary — enumerated in Testing
above, plus: the required blank column can cause earlier right-side
overflow than the legacy fixed-width layout did, and a pinned chip
clipped by the right edge keeps its visible portion clickable instead
of being dropped in full.
