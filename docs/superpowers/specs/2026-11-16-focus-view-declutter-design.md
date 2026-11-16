# Repo focus view: PR-state sections and fixed-zone rows

## Intent

The repo focus view (`f` on the dashboard, #460, #465) is visually busy and
hard to step through. In a single-repo view the user wants three things, in
this order:

1. **Where each workspace stands** — no PR vs draft vs open.
2. **What it's about** — initial prompt, goal, now → next.
3. **The numbers** — PR #, diff, ahead count, process count, age.

Maximum content stays: every row keeps all of today's information. The fix
is structure and hierarchy, not hiding.

Today every row is a 3–6 line block with three side-by-side columns (facts |
PR chip | prose), all at equal weight, ordered by the list's sort with PR
states interleaved. `j`/`k` jump a variable, often large distance.

## Goals

- Group workspaces into sections by PR lifecycle so state reads from
  position, not by scanning a chip.
- Give each row three visual layers that map to the three questions: a
  section (state), a one-line numbers strip at fixed columns (data), a prose
  block below (what it's about).
- Make stepping predictable: row-to-row as today, plus section jumps.

## Non-goals

- Changing what data is collected or shown (no new fields, no removed ones).
- Changing the by-repo / by-attention list views.
- Changing the side panel, detail-bar reply row, hidden-group semantics,
  scroll model (#460's line-offset scrolling), or click behaviors.

## Anti-goals

- Hiding content behind expansion or a detail pane (user explicitly wants
  maximum display in this view).
- Truncating branch names (#460 decided: wrap, never shorten).
- Keys or layout that only work at one terminal width.

## Approaches considered

- **A. PR-state sections + fixed-zone rows (chosen).** Sections by
  lifecycle; each row = numbers strip, prose block, dim footer. Only option
  that gives each of the three questions its own visual layer. Cost: replaces
  the split/stacked geometries in `tall.rs`.
- **B. Sections on top of the current three-column layout.** Smallest
  change, but the facts column still stacks 4–6 short lines beside the
  prose, so most of the busyness remains.
- **C. Kanban columns (No PR | Draft | Open).** State reads instantly, but
  ~40-cell cards make prose wrap badly — fights "maximum content" — and
  adds two-axis navigation.

## Design

### Sections

Order follows the PR pipeline, matching how the user listed it:

| Section    | Lifecycles                       |
|------------|----------------------------------|
| `No PR`    | `NoPr`, and `None` (not yet known) |
| `Draft`    | `PrDraft`                        |
| `Open`     | `PrOpen`, `PrConflicted`         |
| `Done`     | `PrMerged`, `PrClosed`           |

- A section header is one line: `── Open · 3 ───…` to the full width, styled
  dim with the label and count in the theme's lifecycle hue where one exists
  (the same hue the PR chip uses).
- Empty sections are omitted.
- Within a section, rows keep the dashboard's current sort mode
  (`order_workspaces`), so sort still means something — except that the
  Recency mode's freshly-blocked pinning is off in the focus view (user
  decision: no pinning here; sections are strict, and blocked rows are
  spotted by their status glyph). The list views keep pinning.
- Conflicted rows stay in `Open`; their chip already marks the conflict.
- Hidden groups (hide rules) stay where they are today: after all sections,
  under their own summary rows, members drawn as tall rows when expanded.
  They are not split by lifecycle. A `/` filter that surfaces hidden
  workspaces places them in their lifecycle section, as it places them among
  the others today.
- Section headers are not selection targets. `j`/`k`, `visible_targets`, and
  click hit-testing skip them.

### Row: three zones

```
── Draft · 1 ───────────────────────────────────────────────────────────────
▍ ? bakedbean/detail-bar-scroll  ⏷ #459 draft  └ asking ● 1p  ↑2 vs main  +61 −8  14m
▍     prompt  The detail bar containers should scroll independently
▍     goal    Scroll detail-bar containers independently
▍     now     keep scroll offsets per workspace or global?
▍     ▎ claude  #ui
```

1. **Strip (line 1)** — status glyph (spinner while live), branch, PR chip
   (or dim `no PR`), status word with processes / badge / mail mark, ahead
   count, uncommitted counts (`3 mod`, `1 untr`; nothing when clean), diff,
   age flush right. Each field is a column whose width is computed once per
   frame across all rows in the view, so values line up down the whole view
   (same "decided once per frame" rule as today's `Geometry`). Empty fields
   pad to keep alignment.
   - Branch column: longest branch within today's bounds; a longer branch
     wraps onto continuation lines in the branch column (strip fields stay on
     line 1).
   - PR column: as today's `pr_cell` (natural width up to
     `row::MAX_PR_WIDTH`, click target sized to the painted chip).
2. **Prose block** — `prompt`, `goal`, `now` (or `last`), with today's
   `prose_items` rules unchanged (prompt and fallback capped at 2 lines,
   recap fields in full, prompt not repeated as `last`). Indented under the
   branch, wrapped at `min(available, PROSE_MAX)`.
3. **Footer** — agents (theme icons, primary first, active peers in agent
   hue) and tags, one dim line, always shown so every row has the same
   shape. Wraps onto further lines only if agents and tags overflow.

Gutter: the status-colored `▎` / `▍` bar continues on every line of a row as
today. Alternate-row tint stays; it restarts at each section header so the
first row of every section is untinted.

Selection: unchanged from today — the whole selected block is painted with
`theme.selected_bg`, plus the thicker `▍` gutter.

Agent strip: the existing `row::agent_strip_spans` column (one bar per
agent, spinner per active peer) stays at the start of the strip line, before
the gutter, as today; it is the only animated peer-activity signal. Other
lines pad by `widths.agent` as today. (Builder-check call; the mockup omits
it for brevity.)

### Narrow terminals

One layout, degrading by width rather than two geometries:

- If the strip's fields don't fit on one line, it breaks into two: line 1 =
  glyph, branch, PR chip, age; line 2 = status, ahead, uncommitted, diff.
  Columns still align across rows within each line.
- Prose and footer are unchanged except for wrap width.
- `SPLIT_MIN_WIDTH` and the stacked path (`row_lines`, `branch_width_for`,
  `Geometry::Stacked`) go away.

### Navigation

- `j` / `k`: next / previous row across sections (headers skipped), as
  today.
- `}` / `{`: jump to the first row of the next / previous section. From
  inside a section, `{` goes to that section's first row first, then to the
  previous section's first row (paragraph-motion feel, like vim). Hidden-group
  summary rows count as one trailing section for these keys. At the ends,
  no-op.
- `[` / `]` keep stepping repos.
- Scroll-into-view on selection change is unchanged, but a section jump also
  brings the section's header line into view when it fits.

### Header

The breadcrumb header (`◂ all repos / name …`) is unchanged.

### Code shape (guidance, not prescription)

- `src/ui/dashboard/mod.rs`: `focus_entries` gains a `Section` entry
  variant (lifecycle bucket + count) emitted before each non-empty bucket's
  rows; `FocusEntry::target` returns `None` for it and `visible_targets`
  filters it. A small `section_of(lifecycle)` helper owns the table above.
- `src/ui/dashboard/tall.rs`: `TallEntry` gains `Section`; `Geometry` is
  replaced by one strip-columns struct computed per frame; `split_layout` /
  `fact_lines` / `row_lines` are replaced by strip + prose + footer
  builders. `prose_items`, `pr_cell`, `wrap_capped`, `ahead_label` reuse.
- `src/app/input/dashboard.rs`: `{` / `}` handlers using the section
  boundaries from the same entry list.
- Docs: `docs/book/src/dashboard/repo-focus.md` (mockup and prose),
  `keybindings.md` (`{` / `}`).

### Tests

- Sectioning: lifecycles map to the right section, empty sections omitted,
  in-section order follows sort mode, hidden groups trail.
- Strip columns align across rows of differing content; a long branch
  wraps without moving strip fields; narrow width breaks the strip in two.
- `j`/`k` skip headers; `{`/`}` motion incl. the "first row of current
  section first" rule and end no-ops.
- Existing click-target tests (row, PR chip, group) still pass with the new
  layout.

## Left to the builder

- Exact strip field widths and separators, the threshold at which the strip
  breaks, and the precise header styling.
- Whether lifecycle hues exist for every bucket in the theme; fall back to
  dim where not.
- Splitting the work into logical commits (suggested: sections + nav; new
  row layout; narrow degradation; docs).
