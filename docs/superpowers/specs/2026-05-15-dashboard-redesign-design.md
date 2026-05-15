# Dashboard redesign — fixed-column tabular layout

## Background

The current workspaces dashboard (`src/ui/dashboard.rs`) renders each
workspace as 1–2 lines with no consistent column alignment. Names,
branches, git-status counts, and activity labels all float at row-dependent
x positions. As repo and workspace counts grow, scanning the dashboard
becomes visually burdensome — there's no anchor for the eye.

Issue #19 asks for a redesign applying UX/TUI best practices.

## Goal

Replace the current loose, mixed-density layout with a tabular layout
where every workspace row has the same column structure. Make the top row
useful (state summary instead of decorative title). Make repo groups
read as proper sections. Move the optional `[setup-failed]` indicator
into the name column as a small glyph so the right side is reserved for
activity/age.

## Non-goals

- No changes to which workspaces appear or their sort order.
- No new theme colors. Reuses existing `ok` / `warn` / `err` / `dim` /
  `merged` accessors.
- No key-binding changes beyond a `[↑/↓] move` prefix in the footer.
- No responsive reflow for very narrow terminals (< 80 cols). At narrow
  widths, ratatui's normal right-edge clipping applies.

## Visual reference

```
wsx · 7 workspaces · 1 awaiting · 2 stopped

ssk · /home/eben/ssk/ssk-web · 4
────────────────────────────────────────────────────────────────────────────────
  ! ● fix-login              feat/fix-login    pr  ~3 ↑2          awaiting  5s
      └ Hello, can I run this Bash command?                            5s ago
    ● streaming-bug          debug/streaming         ~1                stopped 12s
      └ Made changes to src/streaming.rs                              12s ago
    ✕ failed-setup           exp/failed                                   off  —
    ↻ resumable-old          feat/foo                              resumable  1h
      └ user: try a different approach to mocking                      1h ago

wsx · /home/eben/workspace/wsx · 2
────────────────────────────────────────────────────────────────────────────────
    ● dashboard-redo         feat/dashboard                           active  2s
      └ ran `cargo test --lib`                                         2s ago
    ↻ updates-modal          feat/updates                          resumable  4h

[↑/↓] move  [⏎] attach  [n] new  [e] edit  [t] terminal  [d] archive  [q] quit
```

## Components

### Top summary line

Replaces the current `wsx — Workspaces` banner. Same vertical budget
(1 row), real information instead of decoration.

```
wsx · <N> workspaces[ · <K> awaiting][ · <M> stopped]
```

Counts are computed by the renderer from the supplied `Item` slice:
- `N` = total workspace rows
- `K` = workspaces whose `awaiting_tool.is_some()`
- `M` = workspaces whose `stopped == true`

Each alertable suffix only appears when its count is non-zero. So a
quiet dashboard reads `wsx · 7 workspaces`.

**Coloring:** `wsx` uses `theme.header_style()`. Static text (`·`, `N`,
` workspaces`) uses `theme.dim_style()`. The numeric `K` / `M` for the
alertable suffixes uses `theme.warn_style()`; the surrounding text
(`· `, ` awaiting`, etc.) stays dim.

### Repo group header

Two consecutive list items per repo:

```
<repo.name> · <repo.path> · <count>
────────────────────────────────────────────────────────────────────────────────
```

Header line:
- `repo.name` in `theme.header_style()`
- ` · `, path, ` · `, count in `theme.dim_style()`
- `count` is the number of workspaces belonging to this repo in the
  current item slice. (When the item slice contains
  `Item::EmptyHint` for an empty repo, count is `0`.)

Rule line:
- A single `─` repeated to span `inner_width`
- `theme.dim_style()`

A blank `Item::Spacer` continues to follow the last workspace in a repo,
giving repo groups breathing room. (No structural change to the items
the renderer receives.)

### Workspace row — main

Fixed column structure:

| Col block | Width | Content |
|---|---|---|
| indent | 2 | `"  "` constant |
| attn | 1 | `!` when `needs_attention`, else space |
| sep | 1 | space |
| glyph | 1 | session-state glyph: `●` running, `↻` resumable, `✕` failed, `○` off |
| sep | 1 | space |
| **name** | 20 | `truncate_pad(name, 20)` — truncates to 19 chars + `…` if longer; left-padded with spaces if shorter |
| (setup badge) | 0 or 3 | when `setup_status == Failed`: ` ⚙!` immediately after the name, eating into the name's trailing pad if needed |
| gutter | 3 | `"   "` |
| **branch block** | 28 | branch glyph + space + branch_name + optional ASCII suffix (` draft` / ` pr` / ` merged` / ` closed` / ` conflict`). Whole block truncated with `…` at 28 chars. PR-lifecycle color applies. |
| gutter | ≥1 | elastic — collapses to 1 if the row is overflowing, expands otherwise |
| **git status** | 0+ | `~N ?N ↑N ↓N` (only non-zero counts). Empty string when clean. `theme.dim_style()`. |
| gutter | ≥1 | elastic |
| **activity** | varies | activity word — colored per the table below |
| sep | 1 | space |
| **age** | 2–4 | compact `5s` / `12s` / `5m` / `1h`. Source: `max(awaiting_tool.first_seen_ms, latest_event.timestamp_ms)` if either is present; otherwise `—` (em-dash) — typical for `off` and `resumable` workspaces with no live events. `theme.dim_style()`. |

The leading 6 chars (indent + attn + sep + glyph + sep) place the name
column always starting at x = 6. This is the alignment anchor for both
the name column and the sub-line `└` glyph.

**Setup-failed glyph:** the old `[setup-failed]` right-side badge is
replaced by an inline `⚙!` styled with `theme.err_style()` immediately
following the name. This frees the right side for activity/age and
makes the issue visually adjacent to the workspace it affects. The
glyph is 2 chars + 1 leading space = 3 chars, consuming pad space from
the name column (so a name of length 17 + glyph = 20 total fits exactly).

**Activity color map:**

| Activity word | Style |
|---|---|
| `awaiting` | `theme.warn_style()` |
| `stopped` | `theme.warn_style()` |
| `active` | `theme.ok_style()` |
| `idle` | default fg |
| `waiting` | `theme.dim_style()` |
| `resumable` | `theme.dim_style()` |
| `off` | `theme.dim_style()` |

### Workspace row — sub-line (always when applicable)

Rendered as a separate list item below the main row, in
`theme.dim_style()`:

```
      └ <event display, truncated to fit>                                <age> ago
```

- 6 leading spaces (aligned with name column start)
- `└` + space
- `awaiting_tool` if present: `└ ⚠ awaiting permission: {tool}` with
  `theme.warn_style()` on the `⚠` glyph
- otherwise `latest_event.display` (already truncated to `MAX_DISPLAY_CHARS = 70` upstream)
- right-justified `{age} ago` at the right edge

When neither `awaiting_tool` nor `latest_event` is present, the
sub-line is omitted entirely (a single-line workspace row).

### Outer border

Dropped. The current `Block::default().borders(Borders::ALL)` around the
list area is removed. This reclaims 2 cols horizontal and 2 rows
vertical. The selection highlight (background color from
`theme.selected_style()`) remains the visual anchor for the cursor.

### Footer

```
[↑/↓] move  [⏎] attach  [n] new  [e] edit  [t] terminal  [d] archive  [q] quit
```

`theme.dim_style()`. Only change from today is the `[↑/↓] move` prefix
making navigation discoverable.

## Helpers (new private functions in `dashboard.rs`)

- `fn truncate_pad(s: &str, target: usize) -> String`
  Truncates with `…` to `target-1` chars if longer than `target`; pads
  with trailing spaces to exactly `target` if shorter; returns
  unchanged if exactly `target`.

- `fn truncate_with_ellipsis(s: &str, max: usize) -> String`
  Like the existing `truncate_display` (already in `events.rs`) but
  for use on the sub-line. May reuse the existing util if visible.

- `fn format_age_compact(timestamp_ms: i64) -> String`
  Like the existing `format_age` but produces `5s` / `12s` / `5m` /
  `1h` (no trailing ` ago`). Used in the right-side age column. The
  existing `format_age` stays in place for the sub-line.

- `fn top_summary_line(items: &[Item], theme: &Theme) -> Line<'static>`
  Counts and builds the styled top row.

- `fn repo_header_lines(repo: &Repo, count: usize, inner_width: usize, theme: &Theme) -> (Line<'static>, Line<'static>)`
  Returns the header line and the rule line.

- `fn workspace_main_row(...) -> Line<'static>`
  Composes the main row spans.

- `fn workspace_sub_line(...) -> Option<Line<'static>>`
  Returns the sub-line if applicable.

- `fn activity_style(label: &str, theme: &Theme) -> Style`
  Maps activity word → style per the table above.

## Data flow

No new fields on `Item::Workspace`. The renderer derives:
- top summary counts: iterates `items`, counts workspaces and matches
  on `awaiting_tool.is_some()` / `stopped`
- repo counts: counts contiguous `Item::Workspace` runs between
  `Item::Header` rows

`Item::EmptyHint` (used for empty repos) still produces its current
"(no workspaces — press n to create one)" indented line in dim style.

## Error handling

None added. No new failure modes — pure rendering changes.

## Tests

All new tests live in `src/ui/dashboard.rs`. Existing render-tests
update their string assertions (e.g. `"wsx — Workspaces"` →
`"wsx · "`).

Coverage matrix:

| Test | Verifies |
|---|---|
| `top_summary_shows_total_and_alertable_counts` | Buffer for a 3-workspace dashboard with 1 awaiting + 1 stopped contains `wsx · 3 workspaces · 1 awaiting · 1 stopped`. |
| `top_summary_omits_zero_alertable_counts` | A quiet dashboard's top row reads `wsx · 7 workspaces` and nothing else. |
| `repo_header_renders_with_rule` | Repo header is followed by a row that is all `─` chars (matching inner width). |
| `repo_header_shows_workspace_count` | The header includes ` · <count>` matching the number of workspaces in the section. |
| `workspace_row_name_padded_to_fixed_width` | Two workspaces with names of length 6 and 14 have their branch glyphs at the same column. |
| `workspace_row_branch_truncated_with_ellipsis` | A branch name longer than 26 chars renders as `<glyph> <first chars>…` within the 28-col block. |
| `setup_failed_glyph_appears_after_name` | Workspace with `setup_status = Failed` renders `⚙!` after the name (within the name column's allotted 20 chars). |
| `activity_word_uses_warn_color_for_stopped_and_awaiting` | Spans for `stopped` and `awaiting` rows include `theme.warn` fg on the activity word. |
| `activity_word_uses_ok_color_for_active` | Spans for an `active` row include `theme.ok` fg on the activity word. |
| `sub_line_indent_aligns_with_name_column` | Sub-line `└` glyph sits at column index 6 (matches name column start). |
| `outer_border_is_absent` | Render buffer's leftmost and rightmost columns contain workspace content, not border chars. |
| `footer_includes_arrow_nav_hint` | Footer text contains `[↑/↓] move`. |

Existing tests to revise (not delete):
- `dashboard_renders_full_area_when_pm_hidden`: string assertion
  updates.
- `attached_view_shows_status_row_for_other_workspace_needing_attention`:
  unaffected (attached view, not dashboard).

## Commit sequence on the branch

1. `chore(ui): cut dashboard-redesign branch (no code yet)` — trivial commit, or omitted in favor of starting with commit 2.
2. `refactor(ui): drop outer dashboard border, replace banner with summary line`
3. `refactor(ui): repo header with horizontal rule + count suffix`
4. `feat(ui): fixed-column workspace row with colored activity word`
5. `feat(ui): inline setup-failed glyph beside name`
6. `chore(ui): footer hint includes [↑/↓] move`

Each commit ships with the corresponding test(s) added or updated.

## Branch & integration

Branch name: `feat/dashboard-redesign`, cut off `main` before any
edits. Work in isolation. The user reviews the result in the running
TUI; if approved we merge with a fast-forward, if not approved we
delete the branch and lose nothing.

## Risks

- **Test assertions that match full rendered strings** become brittle.
  Mitigation: assertions use `.contains(...)` on substrings, not full
  equality.
- **Branch-block truncation edge case**: very long branch names with
  nerd-font glyphs may misalign by one column due to multi-byte glyph
  width vs. char count. Mitigation: tests assert column positions
  using ratatui's TestBackend `buf[(x, y)]`, not raw byte offsets.
- **Setup-failed glyph collision with long names**: a name of length
  18+ leaves no room for ` ⚙!`. Mitigation: truncate the name to 17
  chars (with `…`) before appending the badge so the name column
  always stays within its 20-char allotment.
