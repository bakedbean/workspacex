# Focus View PR-State Sections and Fixed-Zone Rows Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Group the repo focus view's workspaces into PR-lifecycle sections and redraw each row as a fixed-column strip, a prose block, and a dim footer, with `{`/`}` section jumps.

**Architecture:** `focus_entries` (src/ui/dashboard/mod.rs) becomes the single source of section order; it emits `FocusEntry::Section` headers that render but are never selection targets. Section start indices are published next to `app.selectable` for the `{`/`}` keys. `tall.rs` replaces its `Geometry::{Stacked, Split}` with one per-frame `StripColumns` layout that breaks the strip onto two lines when narrow.

**Tech Stack:** Rust, ratatui, crossterm, tokio tests (`cargo test`).

**Spec:** `docs/superpowers/specs/2026-11-16-focus-view-declutter-design.md`

## Global Constraints

- Section order and membership: `No PR` = `NoPr` + unknown (`None`); `Draft` = `PrDraft`; `Open` = `PrOpen` + `PrConflicted`; `Done` = `PrMerged` + `PrClosed`. Empty sections omitted.
- Focus view only: no freshly-blocked pinning (`order_workspaces(…, pin_max_age_secs = 0)`). List views unchanged.
- Hidden groups trail all sections, unsplit. A `/` filter that surfaces hidden workspaces puts them in their lifecycle section.
- Section headers are never selection targets or click targets.
- Branch names never truncate; they wrap within the branch column.
- Prose rules (`prose_items`: prompt/fallback capped at 2 lines, recap in full, prompt not repeated as `last`) unchanged; wrap at `min(available, PROSE_MAX)` (`PROSE_MAX = 92`).
- Whole selected block keeps `theme.selected_bg`; `▍`/`▎` gutter on every line; `row::agent_strip_spans` stays at the start of the strip line.
- Alternate-row tint restarts after each section header (first row of each section untinted).
- No change to: breadcrumb, side panel, reply row, line-offset scrolling, PR chip / row / group click semantics.
- Commits: conventional style, no Claude attribution trailers. Commit locally; never push.

## Review Focus

1. **A selected workspace changes PR state while selected** (draft → open after a poll): selection follows the workspace id into its new section; no panic, no jump to row 0. → Task 1 test `selection_follows_a_workspace_across_sections`.
2. **Only one section present / all workspaces hidden**: `{`/`}` are no-ops, no index panic; hidden-group trailing section still reachable. → Task 2 test `section_keys_with_one_section_are_noops`.
3. **Narrow terminal (80 cols) with a long branch and a PR chip with review mark**: strip breaks in two, PR click target equals painted chip width. → Task 4 test `narrow_strip_breaks_and_pr_target_matches_chip`.
4. **Status/`/` filter empties a section mid-view**: header disappears and section starts recompute; `{`/`}` don't land on stale indices. → Task 2 test `section_starts_follow_the_filter`.
5. **Unknown lifecycle (never polled) rows**: land in `No PR`, show dim `no PR`, not clickable. → Task 1 test `unpolled_rows_sit_under_no_pr`.

---

### Task 1: Sections in the focus view's entry list

**Files:**
- Modify: `src/ui/dashboard/mod.rs` (`focused_workspaces` ~490, `FocusEntry` ~517, `focus_entries` ~527, `FocusEntry::target` ~558, `render_focus` ~572, `visible_targets` ~819)
- Modify: `src/ui/dashboard/tall.rs` (`TallEntry` ~117, `render` ~722 — header line and tint restart only)
- Test: `src/app/input/tests/repo_focus.rs`, `src/ui/dashboard/tall.rs` tests

**Interfaces:**
- Produces (mod.rs, `pub(crate)`):
  - `enum FocusSection { NoPr, Draft, Open, Done }` deriving `Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord` (declaration order = display order), with `fn label(self) -> &'static str` returning `"No PR"`, `"Draft"`, `"Open"`, `"Done"`.
  - `fn section_of(lc: Option<BranchLifecycle>) -> FocusSection` (table in Global Constraints).
  - `FocusEntry::Section { section: FocusSection, count: usize }`; `FocusEntry::target(&self, repo) -> Option<SelectionTarget>` (now `Option`, `None` for sections).
  - `pub fn focus_section_starts(inputs: &DashboardInputs, state: &DashboardState) -> Vec<usize>`: index into `visible_targets(...)` of each section's first target, in display order; the hidden-groups tail counts as one more section when non-empty. Empty when not focused.
- Produces (tall.rs): `TallEntry::Section(Line<'static>)` — painted as one line, never a row/group target, resets the alternate-tint ordinal.

- [ ] **Step 1: Write failing tests** in `repo_focus.rs` (seed lifecycles via `app.pr_lifecycle.insert(id, BranchLifecycle::…)` as `hide_rules.rs` does):
  - `sections_follow_pr_lifecycle`: ids a=Open, b=None, c=Draft, d=Merged, e=Conflicted. After `f`, drawn text has lines containing `── No PR · 1`, `── Draft · 1`, `── Open · 2`, `── Done · 1` in that vertical order; branch `b` is below `No PR` and above `Draft`; `e` is below `Open`.
  - `empty_sections_are_omitted`: all five Open → only `── Open · 5` header drawn.
  - `headers_are_not_selectable`: `app.selectable.len() == 5` and all `Workspace`; `j` from the last row of `No PR` lands on the first row of the next section.
  - `focus_view_does_not_pin_blocked`: set sort Recency, make `c` freshly blocked (status Blocked, ago 5s) in `Done`; it is drawn under `── Done`, not first.
  - `selection_follows_a_workspace_across_sections` (Review Focus 1): select `c` (Draft), change it to Open, redraw → `selected_ws == Some(c)` and it's drawn under `Open`.
  - `unpolled_rows_sit_under_no_pr` (Review Focus 5): no lifecycle inserted → single `── No PR · 5` header.
  - `hidden_groups_trail_the_sections`: with a hide rule matching `d`, the group summary line is drawn below every section header.
- [ ] **Step 2:** Add tall.rs test `section_header_restarts_the_tint`: entries `[Section, Row, Row, Section, Row]`; rows 1 and 3 (first in their sections) have no `bg_alt`, row 2 does.
- [ ] **Step 3: Run** `cargo test repo_focus section` and `cargo test tall::tests::section` — expect FAIL (unknown variants / headers missing).
- [ ] **Step 4: Implement.** In `focused_workspaces` pass `0` as pin age. In `focus_entries`, bucket the shown workspaces by `section_of(w.row.lifecycle)` preserving sorted order, emit `Section { section, count }` then rows for each non-empty bucket in `FocusSection` order, then hidden groups as today. Update `target` callers (`visible_targets` uses `filter_map`; `render_focus` `selected` lookup compares `e.target(..) == selection` where `selection` is `Option`). In `render_focus` map `Section` to `TallEntry::Section(header_line(section, count, width, theme))` — a private fn in mod.rs producing `── {label} · {count} ` then `─` to `width`, dim, label+count in `theme.lifecycle_style(<representative lifecycle>)` falling back to dim. In `tall::render`, treat `Section` like `Group` for painting, skip it for targets, and reset `row_ordinal = 0` on it.
- [ ] **Step 5:** Implement `focus_section_starts`; call it in `src/app/render/dashboard.rs` next to `visible_targets` (~104) and store in a new `pub focus_section_starts: Vec<usize>` on `App` (`src/app/state.rs`, default empty), assigned where `app.selectable` is (~131).
- [ ] **Step 6: Run** `cargo test` — all pass, including existing repo_focus and hide_rules tests (fix any that assumed the old flat order by inserting lifecycles or updating expected positions, not by weakening assertions).
- [ ] **Step 7: Commit** `feat(dashboard): group the focus view by PR state`

### Task 2: `{` / `}` jump between sections

**Files:**
- Modify: `src/app/input/dashboard.rs` (key match ~546-571; new fn near `cycle_repo_focus` ~181)
- Test: `src/app/input/tests/repo_focus.rs`

**Interfaces:**
- Consumes: `app.focus_section_starts: Vec<usize>`, `app.selectable`.
- Produces: `fn jump_focus_section(app: &mut App, forward: bool)`.

- [ ] **Step 1: Write failing tests** (lifecycles as in Task 1's `sections_follow_pr_lifecycle`; sections No PR[b], Draft[c], Open[a,e], Done[d]):
  - `close_brace_jumps_to_next_section`: select `b`, `}` → `c`; `}` → first Open row; `}` → `d`; `}` at last section → still `d`.
  - `open_brace_goes_to_section_top_then_previous`: select second Open row, `{` → first Open row; `{` → `c`; at first row of first section `{` → unchanged.
  - `section_keys_with_one_section_are_noops` (Review Focus 2): all Open; `}` and `{` from the middle → `{` goes to top, `}` unchanged; no panic with zero workspaces (`app.selectable` empty).
  - `section_starts_follow_the_filter` (Review Focus 4): set a `/` filter that removes the only Draft row, redraw; from `b`, `}` lands on the first Open row.
  - `section_keys_do_nothing_outside_focus`: not focused, `}` leaves selection unchanged.
- [ ] **Step 2: Run** `cargo test repo_focus brace` — FAIL.
- [ ] **Step 3: Implement** `jump_focus_section`: only when `focused_repo.is_some()`; find current index in `app.selectable`; forward → smallest start `> cur`; backward → largest start `< cur`, but if `cur` is not itself a start, the current section's start (largest start `<= cur`). Set selection to that target. Bind `(KeyCode::Char('}'), _)` and `(KeyCode::Char('{'), _)` guarded on `focused_repo.is_some()`.
- [ ] **Step 4:** Make the section header visible on a jump: when the jump lands on a section start, set `state.focus_scroll` so the header line (one line above the row's first line) shows if it fits. Implement in `tall::render` with no new state: when the selected entry is directly preceded by a `Section` entry, pass `scroll_for` a selection span that starts at the header line (`(header_top, len + header_top_gap)`), so any follow-frame scroll that brings the row in also brings its header. Applies to `j`/`k` landing on a section's first row too, which is the same desirable behavior. Test `jump_reveals_the_section_header`: 120x20 terminal, many rows; after `}` the drawn text contains the target section's header line.
- [ ] **Step 5: Run** `cargo test` — pass.
- [ ] **Step 6: Commit** `feat(dashboard): jump between focus view sections with { and }`

### Task 3: Fixed-zone rows (wide layout)

**Files:**
- Modify: `src/ui/dashboard/tall.rs` (replace `Geometry::Split`, `split_layout`, `fact_lines`; keep `prose_items`, `pr_cell`, `wrap_capped`, `ahead_label`, `pack`)
- Test: `src/ui/dashboard/tall.rs` tests; `src/app/input/tests/repo_focus.rs` click tests

**Interfaces:**
- Produces:
  - `pub struct StripColumns { pub widths: ColumnWidths, pub branch: usize, pub pr: usize, pub status: usize, pub ahead: usize, pub dirty: usize, pub diff: usize, pub age: usize, pub two_line: bool, pub prose: usize }` — every field a cell width computed across all rows in the frame.
  - `pub fn strip_columns(rows: &[TallRow<'_>], widths: ColumnWidths, width: usize) -> StripColumns` (Task 3 always returns `two_line: false` when `width >= SPLIT_MIN_WIDTH`; Task 4 adds the narrow rule).
  - `pub fn row_layout(tall, glyphs, cols: StripColumns, tick, theme, width) -> RowLayout` (signature now takes `StripColumns` instead of `Geometry`; `RowLayout` unchanged).
  - Uncommitted counts use short labels `"{n} mod"` / `"{n} untr"`, joined by `" "`; omitted when clean.
- Row lines: (1) strip: agent strip, gutter, space, glyph, branch padded to `branch`, gap 2, PR cell (`pr_cell`), gap 2, status (token + `● Np` + badge + `✉!`), ahead (`ahead_label`), dirty, diff (`+a −r`), age right-aligned at the line end; each padded to its column. Branch continuation lines (wrap at `branch`) follow the strip with only the gutter. (2) prose lines indented to the branch's x, label column `LABEL_WIDTH`, wrapped at `cols.prose`. (3) footer: agents (icon + name, active peers in agent hue) then tags, dim, wrapped via `pack` at the prose width.

- [ ] **Step 1: Write failing tests** (tall.rs, width 140):
  - `strip_fields_line_up_across_rows`: two rows with different branch lengths, one with PR `#7 open` and diff, one with `no PR` and ahead=3 → the x of `#`/`no PR`, of the status token, and of the age are equal on both strip lines.
  - `long_branch_wraps_without_moving_the_strip`: branch longer than the branch column → strip line still has the PR cell at the shared x; next line holds the branch remainder; `display_width` of every line ≤ 140.
  - `row_has_strip_then_prose_then_footer`: recap goal + prompt + two agents + tag → line 0 contains branch and age; following lines start (after gutter) with `prompt`, `goal`, `now` labels in order; last line contains `claude` and `#tag`.
  - `dirty_counts_are_short_and_skip_clean`: modified=3 untracked=1 → strip contains `3 mod 1 untr`; clean → neither.
  - `pr_target_matches_the_painted_chip`: replaces `oversized_chip_target_ends_at_the_painted_text` against the new x.
- [ ] **Step 2: Run** `cargo test tall::tests` — FAIL.
- [ ] **Step 3: Implement** `StripColumns`, `strip_columns`, the strip/prose/footer builders, and the new `row_layout`; route `render` through `strip_columns` once per frame (wide path only; leave `Geometry::Stacked` for `width < SPLIT_MIN_WIDTH` until Task 4). Delete `split_layout`, `fact_lines`, `LEFT_MIN/LEFT_MAX/COLUMN_GAP` and tests that only covered them (`wide_rows_split_facts_from_prose`, `split_wraps_a_long_branch`, `pr_column_lines_up_across_rows` — superseded by Step 1 tests).
- [ ] **Step 4: Run** `cargo test` — pass, including `repo_focus` click tests (row click attaches, PR chip click opens PR). Run `cargo clippy --all-targets -- -D warnings`.
- [ ] **Step 5: Commit** `feat(dashboard): redraw focus view rows as a strip, prose and footer`

### Task 4: Narrow terminals break the strip, stacked layout removed

**Files:**
- Modify: `src/ui/dashboard/tall.rs` (`strip_columns`, strip builder; delete `Geometry`, `geometry`, `row_lines`, `branch_width_for`, `SPLIT_MIN_WIDTH`)
- Test: `src/ui/dashboard/tall.rs` tests, `src/app/input/tests/repo_focus.rs`

**Interfaces:**
- Consumes: Task 3's `StripColumns`, `row_layout`.
- `strip_columns` sets `two_line = true` when the one-line strip (prefix + all columns + gaps) exceeds `width`. Two-line strip: line 1 = agent strip, gutter, glyph, branch, PR cell, age right-aligned; line 2 = gutter, indent to branch x, status, ahead, dirty, diff. Branch column then sized to `min(longest, width - prefix - pr - age - gaps)`, never below 12.

- [ ] **Step 1: Write failing tests:**
  - `narrow_strip_breaks_in_two`: width 80, row with status, ahead, diff → line 0 has branch, PR cell, age; line 1 has status token and diff; columns align across two rows.
  - `narrow_strip_breaks_and_pr_target_matches_chip` (Review Focus 3): width 80, long branch, chip with review mark + unresolved count → PR target width == painted chip width; all lines ≤ 80.
  - `wide_strip_stays_on_one_line`: width 140, same rows → `two_line == false`.
  - repo_focus: `draw` at 80x60 after `f` shows section headers and every branch name in full (concatenating wrapped lines).
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement** the two-line rule; delete the stacked path and its constants/tests (`branch_column_grows_to_fit_the_longest_branch`, `first_line_drops_the_recap_but_keeps_the_token`, and any test calling `row_lines`), porting assertions that still apply (fallback cap, prompt-not-repeated, agent icons) to `row_layout`.
- [ ] **Step 4: Run** `cargo test` and `cargo clippy --all-targets -- -D warnings` — pass.
- [ ] **Step 5: Commit** `refactor(dashboard): one focus view layout that breaks its strip when narrow`

### Task 5: Docs

**Files:**
- Modify: `docs/book/src/dashboard/repo-focus.md`, `docs/book/src/dashboard/keybindings.md`

- [ ] **Step 1:** Rewrite repo-focus.md's layout section: sections (table from Global Constraints, empty omitted, no blocked pinning here, hidden groups trail), the row's three zones with a mockup copied from an actual 120-col `draw` in a test (print it once with `--nocapture`), the narrow two-line strip; "Moving around" gains `{`/`}`. Remove text about the facts column and the stacked layout.
- [ ] **Step 2:** keybindings.md: add `{` / `}` — "previous / next section (repo focus view)".
- [ ] **Step 3:** `mdbook build docs/book` if available, else check links by eye.
- [ ] **Step 4: Commit** `docs(dashboard): describe focus view sections and row layout`
