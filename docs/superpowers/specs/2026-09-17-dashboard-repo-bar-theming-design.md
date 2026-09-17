# Dashboard repo bar theming — design

**Date:** 2026-09-17
**Status:** approved; plan at `docs/superpowers/plans/2026-09-17-dashboard-repo-bar-theming.md`

## Problem

The starship-style bar engine (`docs/superpowers/specs/2026-09-13-bar-theming-design.md`)
draws the dashboard header and footer, both attached bars, and the detail
pane's chip row from `theme.toml`. The by-repo view's per-repo header line —
the "repo bar" — is still a hand-written span builder
(`src/ui/dashboard/by_repo.rs` `header_line`), so a theme that gives every
other bar a powerline look leaves the repo bars in the stock style. The
example themes in `docs/examples/` visibly stop at the list.

Today's repo bar reads:

```
▾ ── name  PR  /path/to/repo  ────────────  ? 1  ✓ 2    3 ws
```

- a fold glyph (`▾` expanded, `▸` folded, blank for an empty repo);
- the repo name right-justified to a shared column across every repo, the
  pad filled with a dim rule and one space;
- a clickable PR link (`PR`, or the Nerd Font pull-request glyph) in a gutter
  reserved across every repo, so paths align whether or not a repo has one;
  lit in the open-PR green when the repo has an open PR, dim otherwise;
- the path, dim;
- a dim rule flanked by two spaces each side;
- status counts flush right — `? 1  ! 2  … 3  ⠋ 4  ✓ 5  · 6`, zero counts
  omitted, question and stalled bold, idle dim — then four spaces and
  `N ws`, dim. An empty repo has no right side, and the rule runs to the
  edge.

## Goal

Draw the repo bar through the bar engine from a `[dashboard_repo]` table,
with every piece of today's content as a named segment, so a theme can
reorder, omit, restyle, and block-decorate it exactly as it does the other
bars. The bundled default must reproduce today's bar, since the engine also
draws the bundled default when `bar_theme` is off.

## Non-goals

- The by-attention view's section headers (`◆ NEEDS ATTENTION …`), its
  quiet-repo lines, and the status strip under the dashboard header. Each is
  a separate builder; follow-ups.
- Theming the workspace rows under a repo bar.
- Making the fold glyph clickable. Clicking a header selects it as today.
- A per-repo theme. `[dashboard_repo]` is one table applied to every repo.

## Decisions

| Question | Decision |
|---|---|
| Table name | `[dashboard_repo]`, alongside `dashboard_header`, `dashboard_footer`, `dashboard_detail`. |
| One segment or several? | Five: `fold`, `repo_name`, `pr_link`, `repo_path`, `status_counts`. The PR link carries a click, so it must be its own segment, which forces name and path apart. |
| Cross-repo alignment | Stays in `by_repo.rs`: the shared name width and PR gutter are computed over the list as today and passed to the composer per repo. The engine stays line-local. |
| How the name pad is exposed | A `$pad` variable on `repo_name`, and a `pad` key on the segment naming the pad character (default `─`). A space gives plain spaces, which a block theme wants. |
| Zero counts | Empty, like fleet variables, so `( … )` groups drop them with their gaps. |
| Bold/dim on counts | Moves into the bundled format, where a theme can change it. |
| Fold glyphs | A `[fold.symbols]` table with keys `expanded` and `folded`. This generalises the `symbols` table, today hardwired to agent kinds: the registry names each segment's allowed keys. |
| Selection highlight | Unchanged: the list's `highlight_style` still patches the selected row, so a selected block bar flattens to the selection colour as any selected row does. |

## Config shape

Bundled default (`src/ui/bar/default_theme.toml`):

```toml
[dashboard_repo]
format       = "$fold $repo_name  ($pr_link  )$repo_path  "
right_format = "( $status_counts)"
fill         = "─"
fill_style   = "fg:dim"

[fold]
format = "[$symbol]($style)"
[fold.symbols]
expanded = "▾"
folded   = "▸"

[repo_name]
pad    = "─"
format = "[$pad](fg:dim)[$name]($style)"

[pr_link]
format = "[$symbol]($style)"

[repo_path]
format = "[$path](fg:dim)"

[status_counts]
format = "([? $question](fg:question bold)  )([! $stalled](fg:stalled bold)  )([… $waiting](fg:waiting)  )([⠋ $thinking](fg:thinking)  )([✓ $complete](fg:complete)  )([· $idle](fg:dim)  )  [$total ws](fg:dim)"
```

Parity with today's spacing: the two literal spaces at the end of `format`
plus the engine's fill reproduce `path  ────`; the mandatory blank between
sides plus the one leading space of `right_format` reproduce `────  counts`;
the right side is one conditional group, so for an empty repo
`status_counts` is empty, the group and its literal space drop, there is
no right side and no mandatory blank, and the rule runs to the edge. Each count item carries its
own trailing two spaces, and the last is followed by two more, so the four
spaces before `N ws` match today.

`[dashboard_repo]` takes the same keys as every other bar table:
`format`, `right_format`, `style`, `fill`, `fill_style`.

### Segments

| Segment | Variables | `$style` | Hit | Notes |
|---|---|---|---|---|
| `fold` | `$symbol` | dim | — | `expanded`/`folded` from `[fold.symbols]`; a blank the width of the expanded glyph for a repo with no workspaces, so columns stay aligned. |
| `repo_name` | `$pad $name` | header style | — | `$pad` right-justifies the name to the widest repo name: `pad` repeated `n-1` times then one space, for `n > 0` cells short; absent when the name is already the widest. `pad` is one character; the loader rejects anything else. |
| `pr_link` | `$symbol` | open-PR green when the repo has an open, draft, or conflicted PR; dim otherwise | `Hit::RepoPrs`, over the whole rendered segment | `symbol` overrides the glyph (`PR`, or the Nerd Font glyph when nerd fonts are on). Empty for a repo without a GitHub remote when no repo in the list has one; otherwise renders blanks of the glyph's width and no hit, so the paths of linked and unlinked repos stay aligned. |
| `repo_path` | `$path` | dim | — | The lossy display path. |
| `status_counts` | `$question $stalled $waiting $thinking $complete $idle $total` | none | — | Counts of this repo's visible workspaces by dashboard status. Each is empty at zero; the provider renders nothing at all for a repo with no workspaces, whatever the format says. |

All five are dashboard-repo-only: like the header's five, they render empty
in every other bar, and no other segment produces output in
`[dashboard_repo]` except modules (`[module.<name>]`), which are global and
place in any bar.

`pr_link` is a singleton in the `[dashboard_repo]` scope: placed twice
across `format`/`right_format` it is a theme error, as `pr` is in the
attached scope. The scope is independent of the four existing ones.

### `symbols` generalisation

`SegmentDef` gains `symbol_keys: &'static [&'static str]`: `agent_bar`
lists the agent kind names, `fold` lists `expanded` and `folded`, every
other segment lists none, and a `[<segment>.symbols]` table on a segment
with no keys, or with an unknown key, is rejected as today (the error
names the allowed keys). `SegmentConfig::symbols` becomes
`Vec<(String, String)>` with a `symbol_for(key) -> Option<&str>` lookup;
the agent-bar readers look up by `kind.display_name()`. Merge behaviour
(union per key, the user winning; an empty entry is an override) is
unchanged.

## Architecture

- `src/ui/bar/registry.rs`: five new `SegmentDef`s and `symbol_keys`.
- `src/ui/bar/providers.rs`: five providers. `status_counts` builds a
  variable map the way `fleet::FleetStats::to_vars` does (empty at zero).
- `src/ui/bar/segment.rs`: `Hit::RepoPrs`; `symbols` retyped.
- `src/ui/bar/bars.rs`: `DashboardRepoInputs { fold: FoldState, name,
  pad_cells, path, pr_link: Option<PrLink { glyph, linked, open }>,
  counts: StatusCounts }` and `dashboard_repo(specs, theme, &inputs, width)
  -> Rendered`. `FoldState` is `Expanded | Folded | Empty`.
- `src/config/theme_file.rs`: `dashboard_repo: BarTable` on `ThemeFile`
  and `BarSpec` on `BarSpecs`, merged and resolved like the others; `pad`
  on `SegmentTable` (rejected on any segment but `repo_name`); the fifth
  singleton scope; `resolve_symbols` driven by `symbol_keys`.
- `src/ui/dashboard/by_repo.rs`: `header_line` becomes a thin adapter —
  compute the shared name width and gutter as today, build
  `DashboardRepoInputs` per repo, call `dashboard_repo`, and turn the
  rendered `Hit::RepoPrs` span into the existing `RepoPrLinkSpan`. The
  span-building body is deleted. `render_list` takes `specs`, threaded from
  `render_without_footer` through `render_by_repo`.
- Input handling is untouched: `dashboard_repo_pr_rects` is filled from the
  same span list as before.

## Accepted differences from today

- At widths too narrow for a padded rule, the old builder drew plain spaces
  and let the counts overflow past the edge; the engine fills whatever gap
  remains and omits the right side entirely when it cannot fit. Narrow
  terminals only.
- A theme placing `$fold` after `$repo_name` or omitting `$pad` loses the
  shared name column; that is the theme's choice, not a regression in the
  bundled default.

## Example themes

Each of the seven files in `docs/examples/` gains a `[dashboard_repo]` block
and, where its style calls for it, `[repo_name] pad = " "`, `[fold.symbols]`,
and `[status_counts]` colours, so the repo bars match that theme's header:
the airline chain in the orange theme, the block chain in the starship
example, the flat Nord/Rosé Pine/Jellybeans styles. `wsx theme check` must
pass on every example.

## Testing

- `by_repo.rs`'s existing header tests (`header_shows_fold_glyph_and_counts`,
  alignment, PR-link gutter and span, empty-repo rule) run unchanged against
  the engine output; they are the parity gate, as the legacy-builder
  comparisons were for the other bars. A cell-for-cell snapshot of the
  bundled default at two widths is pinned in `src/ui/bar/tests.rs`.
- `registry` drift tests: the five new names are registered, produced by the
  `dashboard_repo` composer, and excluded from the attached composer's
  coverage set; every segment's variables are documented in the book table.
- `theme_file.rs`: `[dashboard_repo]` merges per key over the default;
  `pr_link` twice in the repo bar is an error; `pad` on another segment, or
  longer than one character, is an error; `[fold.symbols]` accepts
  `expanded`/`folded` and rejects other keys; `[agent_bar.symbols]` keeps its
  existing tests; `[pr.symbols]` is still rejected.
- Providers: `status_counts` empties zeros and the whole segment for an empty
  repo; `pr_link` renders blanks without a hit for an unlinked repo in a
  linked list and nothing in an unlinked list; `repo_name` pads with the
  configured character and a space.
- Examples: a test loads every `docs/examples/theme-*.toml` through the
  loader and asserts no errors (add if absent).
- Manual: `docs/manual-tests/bar-theming.md` gains a repo-bar walkthrough —
  copy the orange example, confirm the repo bars take the airline chain,
  fold and unfold a repo, click a PR link, resize narrow.

Docs: `docs/book/src/configuration/themes.md` gains the bar in the Bars
block, the five segments in the segment table, `pad` and `[fold.symbols]`
in the segment-keys paragraph, and the fifth singleton scope.

## Delivery

One branch, one commit per task, each green under `cargo test`, `cargo
clippy`, and `cargo fmt --check`:

1. `symbols` generalisation (registry `symbol_keys`, retyped config,
   loader validation), no behaviour change.
2. Loader: `[dashboard_repo]` bar table, `pad`, the singleton scope, the
   five segment registrations, tests.
3. Providers and composer: the five providers, `Hit::RepoPrs`,
   `dashboard_repo`, unit tests.
4. `by_repo.rs` on the engine: adapter, parity tests green, legacy span
   body deleted, snapshot pinned.
5. Example themes and the examples-load test.
6. Book, manual test, default-theme comment header, this spec's status
   line.
