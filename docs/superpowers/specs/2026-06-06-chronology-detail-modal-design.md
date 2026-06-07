# Chronology Detail Modal — Design

**Date:** 2026-06-06
**Status:** Approved for planning
**Builds on / revises:** the Change Chronology bar, its keyboard navigation, and the detail line-number gutter.

## Problem

The detail view is an inline peek inside the narrow docked bar, capped at a few
lines. Seeing the *entire* change there is impractical — the vertical bar is the
wrong surface for a full diff.

## Goal

Keep the docked bar as the glanceable, navigable **timeline list**, but show a
selected change's **full** diff in a large, scrollable **modal overlay**.

- The bar becomes list-only: selecting a change opens the modal.
- The modal shows the entire change (untruncated), scrollable by keyboard
  (home-row + arrows + page/top-bottom) and mouse wheel.
- Opening the change's file in the editor moves into the modal (`e`).

## Scope

- **In scope:** a `Modal::ChangeDetail` overlay; opening it from the bar;
  full-change re-extraction on demand; the bar/keyboard/state simplification
  that the modal makes possible.
- **Superseded and removed** (the modal replaces them): the inline detail peek,
  expand/collapse, the two-level `Entry`/`Detail` cursor, the in-bar
  line-number gutter, and the in-bar detail click target. The bar's
  open-in-editor two-step is replaced by `e` inside the modal.
- **Out of scope:** horizontal scrolling in the modal (long lines clip);
  syntax highlighting; other agents (still the separate deferred follow-up).

## Decisions (from brainstorming)

- **A (chosen):** bar stays as the list; modal is the detail viewer (not B —
  replacing the bar with a modal chronology).
- **Re-extract the full change on demand** when the modal opens (vs storing
  every change's full text in memory). The in-memory `Timeline` keeps the
  existing 600-char clip for the list/line-resolution; the modal reads the full
  text for the one opened change.
- Modal opens on `Enter` (keyboard) / click (mouse) on a bar entry; `Esc`
  closes; `e` opens the editor at the change's line.

## Architecture

```
bar entry (Enter/click)
  → load_full_change(event)              [re-read the session-log line, un-clipped]
  → format_change_lines(full_detail, base_line)   [full diff + line-number gutter]
  → Modal::ChangeDetail { title, lines, scroll, worktree, file, line }
  → render overlay (scroll slice) ; keys/wheel adjust scroll ; Esc closes ; e → editor
```

## Components

### 1. Full-change re-extraction (`src/activity/chronology.rs`)

The `Timeline` holds every change in the whole-workspace history, so per-change
detail stays clipped (memory bound). The modal needs the full text for one
change, read on demand from the session log.

- **`ChangeSource`** added to `ChangeEvent`:
  ```rust
  pub struct ChangeSource {
      pub session_file: PathBuf, // the JSONL log this event came from
      pub line_index: usize,     // 0-based line number within that file
      pub index_in_line: usize,  // position among the events that line produced
  }
  ```
  `ChangeEvent` gains `pub source: ChangeSource`.
- **`extract_change_events` gains a clip bound:**
  `extract_change_events(v: &serde_json::Value, detail_max: usize) -> Vec<ChangeEvent>`.
  `clip` becomes `clip(s, max)`. It sets each emitted event's
  `source.index_in_line` to its position in the returned `Vec` and leaves
  `session_file`/`line_index` default (the caller fills them).
- **`parse_file`** passes `DETAIL_MAX_CHARS` and, for each parsed line (tracked
  via `enumerate`), sets `source.session_file = path` and
  `source.line_index = line_index` on every event that line produced.
- **`load_full_change(ev: &ChangeEvent) -> Option<ChangeDetail>`**: open
  `ev.source.session_file`, read the line at `line_index`, parse it, call
  `extract_change_events(&v, usize::MAX)`, and return
  `.get(ev.source.index_in_line)?.detail.clone()` (the full, un-clipped detail).
  Returns `None` when the source is empty/unreadable/line gone — callers fall
  back to the event's clipped `detail`.

  Because both the clipped build and the full re-extract use the *same*
  `extract_change_events` walk, `index_in_line` aligns by construction (no
  duplicated parsing logic to drift).

### 2. Full diff formatting (shared, pure)

Factor the gutter/diff formatting (currently inside `entry_lines`) into a
reusable pure function in `src/ui/chronology_bar.rs`:

```rust
/// Full change as display strings with a line-number gutter: removed (`-`)
/// lines (blank gutter) then added (`+`) lines numbered from `base_line`.
/// No line cap — the modal scrolls.
pub fn change_detail_lines(detail: &ChangeDetail, base_line: u32) -> Vec<String>
```

- `Edit { old, new }`: every `old` line → `"     - {l}"`; every `new` line →
  `"{n:>4} + {l}"` from `base_line` (saturating).
- `Write { head }`: every content line → `"{n:>4} + {l}"` from `base_line`.
- `None`: empty.
This is the same gutter scheme already shipped, minus the `take(2)`/`take(3)`
caps. The modal renderer owns horizontal clipping to its width.

### 3. The bar becomes list-only (`src/ui/chronology_bar.rs`, `src/ui/attached.rs`)

- `entry_lines` reduces to the single header line (`HH:MM <abbreviated path>`).
  Its signature becomes `entry_lines(ev, worktree, width, selected: bool)`:
  remove the `expanded` param, the peek block, the `base_line` param, and the
  `EntryHighlight` enum entirely — a row is just selected or not, so a
  `selected: bool` (reverses the header line when true) replaces `EntryHighlight`.
- `render_chronology_bar` removes the expand/peek/detail-rect logic; it renders
  one header line per entry with selection highlight and records per-entry click
  rects (to open the modal). `ChronologyHits` drops `detail`; keep `entries`
  and `visible_entries`.

### 4. Navigation reducer simplification (`src/ui/chronology_nav.rs`)

Single-level now:
- `ChronoSel` collapses to a plain selected index (`usize`); remove the
  `Entry`/`Detail` enum.
- `NavAction` becomes `{ None, Open(usize), Exit }` (remove `Expand`/`Collapse`).
- `nav(sel: usize, key: NavKey, len: usize) -> (usize, NavAction)`:
  `Up`/`Down` move with clamp; `Top`/`Bottom`; `Enter` → `Open(sel)`; `Esc` →
  `Exit`. `adjust_scroll` is unchanged (operates on the index).

### 5. App state (`src/app.rs`)

- Remove: `chronology_expanded`, `chronology_detail_rect`.
- Change: `chronology_sel: usize` (was `ChronoSel`).
- Keep: `chronology_focused`, `chronology_scroll`, `chronology_visible_entries`,
  `chronology_entry_rects`, `chronology_bar_rect`, `chronology_last_workspace`
  (the reset hook now resets `chronology_sel = 0`, `chronology_focused = false`).

### 6. The modal (`src/ui/modal.rs`, `src/app/render.rs`, `src/app/input.rs`)

- **Variant:**
  ```rust
  Modal::ChangeDetail {
      title: String,        // "HH:MM <relative path>"
      lines: Vec<String>,   // full diff, gutter-formatted (computed at open)
      scroll: usize,        // top visible line
      worktree: PathBuf,    // for the editor open
      file: PathBuf,        // absolute changed file
      line: u32,            // resolved change line (for `e`)
  }
  ```
- **Open** (`src/app/input.rs`): the bar's `NavAction::Open(i)` (keyboard
  `Enter`) and an entry click resolve the focused workspace + event `i`, then:
  `detail = load_full_change(ev).unwrap_or_else(|| ev.detail.clone())`;
  `line = resolve_line_in_file(&ev.file_path, &detail)`;
  `lines = change_detail_lines(&detail, line)`;
  `title = "{HH:MM} {relative path}"`; set `app.modal = Some(Modal::ChangeDetail
  { … scroll: 0 })`. (`open_focused_change` is repurposed from "open editor" to
  "open modal".)
- **Render** (`src/app/render.rs` modal dispatch): a bordered overlay sized to
  most of the screen (e.g. ~90% width/height, centered); header row = `title`;
  body = `lines[scroll .. scroll + body_height]`, each clipped to inner width;
  a footer hint (`↑/↓ j/k scroll · e editor · Esc close`) and a simple
  position indicator (`scroll+1`–`end` / `len`).
- **Input** (`src/app/input.rs` modal handler): add a `Modal::ChangeDetail` arm:
  `↓`/`j` → `scroll = (scroll+1).min(max)`, `↑`/`k` → `saturating_sub(1)`,
  `PgDn`/`PgUp` → ± a page, `g`/`G` → top/bottom (`max = lines.len().saturating_
  sub(body_height)`; since the handler may not know `body_height`, page = a fixed
  step e.g. 10, and `G` sets a large value clamped at render — store `scroll`
  and clamp in the renderer too). `Esc` → `app.modal = None`. `e` → open the
  editor via the existing `editor_open_decision` + `open_in_editor_at(&worktree,
  &file, line, …)`; on `NeedsConfig`/`Err` replace with `Modal::Error`.
  Mouse: wheel over the modal adjusts `scroll`; click outside the modal box
  closes it.

  Scroll clamping: the input handler clamps with a conservative `max =
  lines.len().saturating_sub(1)`; the renderer additionally clamps `scroll` so
  the last page isn't over-scrolled (it knows the body height). Keep a small
  pure `clamp_scroll(scroll, len, body) -> usize` helper, unit-tested.

## Interaction summary

| Surface | Key/action | Effect |
| --- | --- | --- |
| Bar | `j`/`k`, `↑`/`↓`, `g`/`G` | move selection (unchanged) |
| Bar | `Enter` / click entry | open the change in the modal |
| Bar | `Ctrl-x`+arrow | focus/exit the bar (unchanged) |
| Modal | `↑`/`↓`, `j`/`k`, `PgUp`/`PgDn`, `g`/`G`, wheel | scroll |
| Modal | `e` | open the file in the editor at the change line |
| Modal | `Esc` / click outside | close |

## Error handling / edge cases

- `load_full_change` fails (source missing/unreadable) → fall back to the
  clipped `detail`; the modal still opens with what's available.
- Empty timeline / no focused entry → `Enter` is a no-op.
- `e` with no `editor_cmd` → `Modal::Error` (existing config-required behavior).
- Over/under-scroll → clamped (helper + renderer).
- Modal open while attached renders over the attached view (modal block is after
  the view match) and is dismissible (existing handler).
- Workspace switch while a modal is open: existing modal lifecycle (the modal is
  independent of the bar's per-frame state).

## Testing

- **`change_detail_lines`** (pure): Edit → all `-` lines blank-gutter, all `+`
  lines numbered from `base_line`, no cap; Write → all content numbered;
  `None` → empty.
- **`extract_change_events`** with `detail_max`: clipped vs `usize::MAX` full;
  `index_in_line` assigned per emitted event (incl. MultiEdit → 0,1,…).
- **`load_full_change`** (round-trip): write a session line, build a
  `ChangeEvent` with a matching `ChangeSource`, assert the full (un-clipped)
  detail comes back; missing source → `None`.
- **`nav`** simplified: `Up`/`Down` clamp, `Top`/`Bottom`, `Enter` → `Open(sel)`,
  `Esc` → `Exit`; `len == 0` no-op.
- **`clamp_scroll`** pure: top, bottom, mid, len < body.
- `entry_lines` reduced tests (header only, selected highlight).
- Modal render/input glue verified by build + manual (open, scroll all ways,
  `e`, `Esc`).

## Files touched

- `src/activity/chronology.rs` — `ChangeSource`, `extract_change_events(detail_max)`,
  `parse_file` source population, `load_full_change`.
- `src/ui/chronology_bar.rs` — `change_detail_lines`; `entry_lines` reduced to
  header; `EntryHighlight` simplified.
- `src/ui/chronology_nav.rs` — single-level `nav`/`NavAction`; `ChronoSel`→index.
- `src/ui/attached.rs` — list-only bar render; `ChronologyHits` drops detail.
- `src/app.rs` — state changes (remove expanded/detail_rect; `sel: usize`).
- `src/ui/modal.rs` — `Modal::ChangeDetail` variant.
- `src/app/render.rs` — modal render; remove the bar's base_line/detail wiring.
- `src/app/input.rs` — bar `Open` → open modal; modal scroll/`e`/`Esc`/wheel.
- `README.md` — document the detail modal and its keys.
