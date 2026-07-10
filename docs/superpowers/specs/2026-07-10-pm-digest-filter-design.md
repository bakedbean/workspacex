# PM digest filter — design

2026-07-10

## Goal

Let the user narrow the Project Manager digest pane to matching workspaces by
typing. Activation and editing mirror the dashboard's existing `/` filter so
there is one filter mental model across the app. Matching is against the
**workspace name only** (explicit user requirement).

## State

- New `pm_filter: Option<String>` field on `App`, next to
  `pm_digest_selected`. `None` = no filter; `Some(buf)` = filter mode with
  `buf` as the live needle. Mirrors `dashboard.filter`.
- The filter is cleared whenever the PM pane closes (any path: `q`/`p` from
  PM focus, `p` from dashboard focus), so reopening never shows an invisibly
  filtered list.

## Input (PM-focus block, `src/app/input.rs`)

Filter **inactive** (`pm_filter == None`):

- `/` sets `pm_filter = Some(String::new())`.
- All existing bindings unchanged: `j`/`k`/arrows navigate, `Enter` attaches,
  `Tab`/`Esc` return focus to the dashboard, `q`/`p` close the pane,
  `r` nudges refresh.

Filter **active** (`pm_filter == Some(_)`), checked before the single-key
bindings:

- Printable chars (no CTRL/ALT modifier) append to the buffer — including
  `j`, `k`, `q`, `p`, `r`, which become filter text while typing.
- `Backspace` pops the last char.
- `Esc` clears the filter (sets `None`) and stays in the pane. A second
  `Esc` then leaves the pane as today.
- `Up`/`Down` arrows, `Enter`, and `Tab` keep their navigation/attach/unfocus
  meanings while the filter is active.
- After every filter edit, `pm_digest_selected` is clamped to
  `card_count - 1` (0 when the list is empty) so the selection marker never
  points past the filtered list.

## Filtering (`src/ui/pm_pane.rs`)

- `DigestInputs` gains `filter: Option<&'a str>`.
- `build_digest` drops cards whose workspace name does not contain the
  needle, case-insensitive substring — same semantics as the dashboard's
  `matches_filter`, but on the name only. An empty or `None` needle matches
  everything.
- Repos left with zero cards are omitted by the existing empty-repo rule.
- Because both the renderer and the input handler consume
  `app.build_pm_digest()`, `card_count`, `card_at`, selection, and
  Enter-attach automatically agree with what is on screen.

## Rendering (`src/ui/pm_pane.rs`)

- `render_digest` gains the active filter needle so the title can echo it.
- Title line, PM-focused, filter active:
  `Project Manager [/<needle> · Esc clear · Enter attach]`.
- Title line, PM-focused, no filter: current hints plus `/ filter`:
  `Project Manager [j/k select · / filter · Enter attach · Esc/Tab back]`.
- Unfocused title unchanged.
- Zero cards **with a filter active** renders `no matching workspaces`;
  zero cards with no filter keeps the existing `no active workspaces`.

## Error handling

No fallible paths: the filter is a pure in-memory string over already-cached
digest inputs. Degenerate cases (empty needle, zero matches, selection past
the end) are covered above.

## Testing

- `digest_tests`: name filtering keeps only matching cards; repos with no
  matching cards are omitted; matching is case-insensitive; `None`/empty
  needle is a no-op.
- `render_tests`: filter-active title echoes `/<needle>`; zero-match
  placeholder text; no-filter title advertises `/ filter`.
- `input_tests`: `/` enters filter mode; printable chars (including `q`)
  edit the buffer instead of triggering bindings; Backspace pops; first Esc
  clears the filter, second Esc leaves the pane; selection clamps when the
  filtered count shrinks; closing the pane clears the filter.
