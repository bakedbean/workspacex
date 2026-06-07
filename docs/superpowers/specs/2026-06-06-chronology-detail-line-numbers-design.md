# Chronology Detail Line Numbers — Design

**Date:** 2026-06-06
**Status:** Approved for planning
**Builds on:** the Change Chronology bar (expandable detail "diff peek").

## Problem

When a chronology entry is expanded, its detail peek shows the change as removed
(`- old`) and added (`+ new`) lines, but with no line numbers. The reader can't
tell *where* in the file the change lives without opening the editor.

## Goal

Show a line-number gutter on the detail peek. Only the **added** lines have a
current-file line number (the removed lines no longer exist in the file), so:

- `+` added lines (and `Write` content lines) are numbered with their
  current-file line, starting at the change's resolved line and incrementing.
- `-` removed lines render with a blank gutter (no current-file line).

This reuses the line wsx already computes for "open at line," so the gutter and
the editor jump agree.

## Decisions (from brainstorming)

- **Option A:** keep the `- removed` lines (blank gutter) alongside the numbered
  `+ added` lines — preserves before/after context. (Chosen over dropping the
  removed lines.)
- The starting line comes from the existing `resolve_line_in_file(file, detail)`
  (which anchors on the first non-blank line of the post-edit `new` text).
- `entry_lines` stays pure: the renderer does the file IO to compute the base
  line and passes it in.

## Current behavior (for reference)

`src/ui/chronology_bar.rs::entry_lines(ev, worktree, expanded, width, highlight)`
builds the peek (only when `expanded`):
- `ChangeDetail::Edit { old, new }` → up to 2 `- {old_line}` then up to 2
  `+ {new_line}`.
- `ChangeDetail::Write { head }` → up to 3 `+ {content_line}`.
- `ChangeDetail::None` → no peek.
Each peek line is dimmed and clipped to `width`.

`resolve_line_in_file(path, detail) -> u32` (in `src/activity/chronology.rs`)
reads the file and returns the 1-based line of the first non-blank `new` line,
or 1 when not found / Write / unreadable.

## Design

### Gutter format

A fixed-width, right-aligned line-number gutter precedes the existing `±`
marker and text. Gutter width: `GUTTER = 4` columns (line number, right-aligned)
plus one trailing space, so the body starts at column 5.

- Added line at file line `n`: `"{n:>4} + {text}"` → e.g. `"  42 + let x = 2;"`.
- Removed line: blank gutter → `"     - {text}"` (4 spaces + space, then marker).
- Line numbers ≥ 10000 simply widen the field (Rust `{:>4}` does not truncate);
  acceptable for a glance.

The whole composed line is then clipped to `width` (as today), so the gutter is
preserved and the text tail is what gets cut on a narrow bar.

### Numbering

For the expanded entry, the renderer computes `base_line =
resolve_line_in_file(&ev.file_path, &ev.detail)` and passes it to `entry_lines`.

- `Edit { old, new }`: removed lines (`old.lines().take(2)`) get a blank gutter;
  added lines (`new.lines().take(2)`) get `base_line`, `base_line + 1`.
- `Write { head }`: content lines (`head.lines().take(3)`) get `base_line`,
  `base_line + 1`, `base_line + 2` (`base_line` is 1 for a Write).
- Numbering is best-effort (consistent with the editor-open line): it assumes the
  shown added lines are contiguous from `base_line`. If a later edit moved the
  text, the gutter may be approximate — acceptable for a glanceable peek.

### Components / files

- **`src/ui/chronology_bar.rs`**
  - `entry_lines` gains a `base_line: u32` parameter (after `width`).
  - Refactor the peek construction so each peek line carries its gutter:
    build `(gutter: Option<u32>, marker: char, text: &str)` tuples — `Some(n)`
    for added lines (numbered from `base_line`), `None` for removed lines — then
    format `"{:>4} {} {}"` / `"     {} {}"` and clip to `width`.
  - The `EntryHighlight::Detail` highlight still reverses the peek lines (now
    including the gutter), unchanged in index range (peek lines remain
    everything after the header).
- **`src/ui/attached.rs` (`render_chronology_bar`)**
  - For the expanded entry, compute `base_line =
    crate::activity::chronology::resolve_line_in_file(&ev.file_path, &ev.detail)`
    and pass it into the `entry_lines(...)` call. For non-expanded entries pass
    any value (e.g. `1`) — the peek isn't rendered, so it's unused. (Compute it
    only when `expanded` to avoid needless file reads.)
- No change to `resolve_line_in_file` or the editor-open path.

### Error handling / edge cases

- File unreadable / change not found → `resolve_line_in_file` returns 1; the
  gutter still numbers from 1 (best-effort, no error).
- `Write` → `base_line` is 1, so content numbers 1, 2, 3.
- Narrow bar → the composed `"{gutter} {marker} {text}"` is clipped to `width`;
  the gutter stays, the text tail is trimmed (gutter is the high-value part).
- Empty `new`/`old` → fewer peek lines, no panic (existing `take(2)`/`take(3)`).

### Testing

- **`entry_lines` (pure):**
  - Edit at `base_line = 42`: the added (`+`) peek lines carry `42`, `43` in a
    right-aligned gutter; the removed (`-`) peek lines have a blank (spaces)
    gutter. Assert the rendered line strings contain `"42"`/`"43"` on the `+`
    lines and that the `-` lines' gutter region is spaces.
  - Write at `base_line = 1`: content lines carry `1`, `2`, `3`.
  - Collapsed entry: unchanged (single header line; `base_line` ignored).
  - Existing highlight tests updated for the new `entry_lines` arity; `Detail`
    highlight still reverses peek lines.
- The renderer's `base_line` computation is the existing tested
  `resolve_line_in_file`; the wiring is verified by build + manual.

## Files touched

- `src/ui/chronology_bar.rs` — `entry_lines` gutter + `base_line` param + tests.
- `src/ui/attached.rs` — compute and pass `base_line` for the expanded entry.
- `README.md` — note the detail peek shows line numbers on added lines.
