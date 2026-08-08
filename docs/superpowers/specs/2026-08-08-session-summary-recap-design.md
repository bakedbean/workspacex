# SESSION SUMMARY renders the workspace recap

**Date:** 2026-08-08
**Status:** approved

## Problem

The detail bar's `session_summary` module opens with the workspace's *first
user prompt* — the text that kicked the session off. That was the best
available proxy for "what is this workspace doing" before agent-authored
recaps existed.

It no longer is. Agents now maintain a `goal` / `state` / `next` digest via
`wsx recap set`, stored in the `workspace_recap` table. The Project Manager
pane (`src/ui/pm_pane.rs`) and the dashboard row's flex column
(`src/ui/dashboard/column_content.rs`) both render it. The detail bar does
not, so the one surface with the most vertical room still shows the stalest
possible answer: a prompt from hours ago that the work has long since moved
past.

## Goal

`SESSION SUMMARY` leads with the workspace's recap — the same
goal/state/next the PM pane shows — and falls back to the first user prompt
when no recap exists yet.

## Non-goals

- No staleness signal. Commit `e4bf119` deliberately collapsed the dashboard
  row to one grey for every recap; the detail bar follows that precedent.
  Staleness stays a PM-pane concern (`recap stale` in its facts line).
- No change to the PM pane or the dashboard row.
- No new config surface. The module has no options today and gains none.

## Data flow

`App::recaps` (`src/app.rs:529`) is a
`HashMap<WorkspaceId, WorkspaceRecap>` refreshed from
`Store::all_workspace_recaps()` (`src/app.rs:764`). `src/app/render.rs:127`
already reads it when building the dashboard row's flex column.

The change threads the same borrow one level further:

1. `DetailInputs` (`src/ui/dashboard/detail.rs:22`) gains
   `pub recap: Option<&'a WorkspaceRecap>`.
2. `DetailContext` (`src/detail_modules/mod.rs:21`) gains the same field.
3. `detail.rs` copies it across when it builds the context (line ~265).
4. `src/app/render.rs:335` populates it with `app.recaps.get(&ws.id)`.

No new queries, no new app state, no allocation per draw — `DetailContext`'s
"zero allocations per draw, all fields borrowed or `Copy`" invariant holds.

`tests_helpers::stub_context()` defaults the field to `None`, so every
existing module test keeps its current behavior.

## Rendering

A new `recap_lines()` in `src/detail_modules/session_summary.rs` runs
**before** the `match events` block, not inside it.

That placement matters: the prompt lives inside the `Some(evt)` arm because
it is derived from the JSONL scan, so it cannot render until
`events_scanned` flips. The recap comes from SQLite instead, so hoisting it
above the match lets a workspace with a recap show goal/state/next during
the `loading…` window rather than nothing.

### Line shape

One line per present field, in the order goal → state → next:

```
▸ goal:  Audit all V2 invoices auto-issued
         today for the CV-04964 drift bug
▸ state: 3 of 12 checked, drift confirmed on 2
▸ next:  Fix the rounding in issue_v2()
```

- Leading `▸ ` prefix in the workspace's status color, matching every other
  line in the module.
- 7-char dim label (`goal:  `, `state: `, `next:  `) — the same width as the
  existing `model: ` label, so the values column-align down the module.
- Value wrapped to `inner_width - 7` via the module's existing `wrap_lines`,
  with continuation lines indented 9 cells so they align under the value's
  first character.
- Value styled dim, matching the PM pane's plain treatment. One grey for
  every recap.

### Field selection

Each of the three slots takes the **long** field, falling back to the short
form when the long one is absent or whitespace-only:

| slot  | first choice | fallback     |
| ----- | ------------ | ------------ |
| goal  | `goal`       | `goal_short` |
| state | `state`      | `state_short`|
| next  | `next`       | `next_short` |

This is a deliberate, approved deviation from the PM pane, which reads only
the long forms — a workspace whose agent ran `wsx recap set --goal-short`
alone renders blank there. Not worth reproducing on a second surface.

Whitespace-only values count as absent throughout.

### Fallback

- **Any** slot resolves to non-empty text → the recap renders and the prompt
  is not shown at all. A recap with only `goal` set shows one line, not a
  goal line plus the prompt.
- **No** slot resolves (no recap row, or every field null/blank) → the
  prompt renders exactly as it does today, italic, inside the `Some(evt)`
  arm.

### Narrow columns

When `inner_width <= 7` the label does not fit alongside a value. The line
degrades to a truncated dim label alone — the same failure mode
`label_value_line` already implements for `model:` and `context:`. No
underflow on the value width.

## Testing

Unit tests in `session_summary.rs`, alongside the existing render tests:

1. A recap replaces the prompt — recap text present, prompt text absent.
2. A partial recap (goal only) renders one line and still suppresses the
   prompt.
3. A recap whose fields are all whitespace falls back to the prompt.
4. No recap at all falls back to the prompt.
5. A recap renders while `events_scanned` is false (the `loading…` window).
6. A goal longer than the column wraps, and continuation lines carry the
   9-cell indent rather than the `▸ ` prefix.
7. A narrow column degrades to a label-only line.
8. A slot with only the short form set renders that short form.
9. Label spans use `dim_style`; value spans use `dim_style`.

## Commits

1. Plumb `recap` through `DetailInputs` and `DetailContext`.
2. Render recap lines in `session_summary`, with prompt fallback + tests.
3. Document the module's content in
   `docs/book/src/daily-use/detail-bar.md`.
