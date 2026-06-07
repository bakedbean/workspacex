# Chronology Keyboard Navigation — Design

**Date:** 2026-06-05
**Status:** Approved for planning
**Builds on:** `2026-06-05-change-chronology-view-design.md` (the Change Chronology bar)

## Problem

The Change Chronology bar is glanceable but only weakly interactive: today a click expands an entry's detail and a second click on the same entry opens the editor, with no keyboard path at all. Two gaps:

1. **No keyboard navigation.** You can't move focus into the bar, walk the list, or open a change without the mouse.
2. **Opening is unclear.** The mouse "second-click-the-expanded-entry-to-open" gesture is undiscoverable; users expand an entry and have no obvious way to jump to the file.

## Goal

Keyboard-drive the chronology bar as a focusable pane within the attached view, and align the mouse with the same model so opening a change is obvious.

- `Ctrl-x + arrow` moves focus **into** the bar (arrow toward the bar's side) and **out** (arrow away, or `Esc`).
- While focused, arrow keys or home-row `j`/`k` walk the list; `g`/`G` jump to top/bottom.
- `Enter` on an entry expands its detail; arrowing **into** the detail and pressing `Enter` again opens the editor at the changed line (new file → top).
- The mouse mirrors this: click an entry to select + expand it; click the expanded detail to open the file at the line.

## Non-goals

- Not making the chronology bar a `SplitTree` leaf (its leaves are agent PTY targets; the bar is chrome).
- Not a modal/overlay list — navigation stays in-place so the agent view remains visible beside it.
- No multi-entry expansion — one entry expanded at a time (unchanged from the base feature).

## Decisions (from brainstorming)

| Decision | Choice |
| --- | --- |
| Focus model | In-pane focus mode + two-level selection (`Entry`/`Detail`); not a split leaf, not a modal. |
| Enter/exit | `Ctrl-x` + arrow toward bar enters; arrow away or `Esc` exits. |
| List keys | `↑`/`k`, `↓`/`j`; `g`/`G` top/bottom. |
| Into-detail | `↓` from an expanded entry steps into its detail; `↑` from detail returns to the entry. |
| Open | `Enter` on detail (keyboard) / click on the expanded detail region (mouse) → open at `file:line`. |
| Expand | `Enter` on an entry toggles expand (keyboard) / click an entry selects + expands (mouse). |
| Mouse | Mirrors keyboard (per the chosen option). |
| `Esc` | Exits the whole pane (not `Detail`→`Entry`; `↑` does that step). |

## Architecture (Approach 1)

A focus mode on `App`, layered over the existing chronology state. While the bar is focused, the attached-view key handler intercepts navigation keys *before* the default "encode and forward to PTY" path; otherwise input is unchanged. The navigation logic is a **pure reducer** so it is unit-testable without a terminal; the input handler is thin glue that calls the reducer and applies side effects (open editor).

```
key/mouse ─▶ (if chronology_focused) reducer(sel, key, expanded, len) ─▶ (new sel, Action)
                                                                         │
                                          Action ∈ { None, Expand(i), Collapse(i), Open(i), Exit }
                                                                         ▼
                                         input glue applies: mutate App state / open_in_editor_at / drop focus
```

## State (added to `App`)

```rust
/// Keyboard focus is currently in the chronology bar (intercept nav keys).
pub chronology_focused: bool,
/// In-pane cursor while focused.
pub chronology_sel: ChronoSel,
```

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChronoSel {
    Entry(usize),
    Detail(usize),
}
impl Default for ChronoSel { fn default() -> Self { ChronoSel::Entry(0) } }
```

Reuses existing `chronology_expanded: Option<usize>` (expansion) and `chronology_scroll: usize` (viewport). `Detail(i)` is only valid when `chronology_expanded == Some(i)`.

## Components

### 1. Navigation reducer (`src/activity/chronology.rs` or a new `src/ui/chronology_nav.rs`, pure)

```rust
pub enum NavKey { Up, Down, Top, Bottom, Enter, Esc }
pub enum NavAction { None, Expand(usize), Collapse(usize), Open(usize), Exit }

/// Pure transition: given the current selection, a key, whether the selected
/// entry is expanded, and the entry count, return the next selection + an action
/// for the caller to apply. Bounds-safe (clamps to `len`).
pub fn nav(sel: ChronoSel, key: NavKey, expanded: Option<usize>, len: usize) -> (ChronoSel, NavAction);
```

Transition rules (with `len > 0`; `len == 0` → all keys except `Esc` are `(sel, None)`):
- `Down` on `Entry(i)`:
  - if `expanded == Some(i)` → `(Detail(i), None)`;
  - else → `(Entry(min(i+1, len-1)), None)`.
- `Down` on `Detail(i)` → `(Entry(min(i+1, len-1)), None)`.
- `Up` on `Detail(i)` → `(Entry(i), None)`.
- `Up` on `Entry(i)` → `(Entry(i.saturating_sub(1)), None)`.
- `Top` → `(Entry(0), None)`; `Bottom` → `(Entry(len-1), None)`.
- `Enter` on `Entry(i)` → if `expanded == Some(i)` then `(Entry(i), Collapse(i))` else `(Entry(i), Expand(i))`.
- `Enter` on `Detail(i)` → `(Detail(i), Open(i))`.
- `Esc` → `(sel, Exit)`.

`Top`/`Bottom`/any move that lands on a non-expanded entry while `sel` was `Detail` collapses nothing here — collapse is only via explicit `Enter` toggle (single-expansion is enforced by `Expand` setting `chronology_expanded = Some(i)`, which the caller applies, implicitly replacing any prior expansion).

### 2. Enter/exit glue (`src/app/input.rs`, attached leader block)

In the `Ctrl-x` leader arrow arm (today `state.focus_direction(arrow)`):
- Resolve the configured side via `chronology::resolve(repo, store)` (the renderer already does this; the handler resolves the focused repo the same way `focused_attached_workspace` does).
- If **not** `chronology_focused` and the bar is visible/shown and `arrow` points toward the bar's side **and** the focused pane is the edge pane adjacent to the bar → set `chronology_focused = true`, `chronology_sel = Entry(0)`, return. Otherwise fall through to `state.focus_direction(arrow)` (unchanged pane navigation).
- If **already** `chronology_focused` and `arrow` points away from the bar → `chronology_focused = false`, return (focus back to agent).

### 3. In-pane key interception (`src/app/input.rs`, in `handle_key_attached`)

Immediately after leader handling and before the default `encode_key` → PTY forward (input.rs ~954): if `chronology_focused`, map the key to a `NavKey` (`Down`/`j`, `Up`/`k`, `g`→`Top`, `G`→`Bottom`, `Enter`, `Esc`), call `nav(...)`, store the new selection, and apply the `NavAction`:
- `Expand(i)` → `chronology_expanded = Some(i)`; `Collapse(i)` → `chronology_expanded = None`.
- `Open(i)` → resolve the focused workspace + the event at `i`, `resolve_line_in_file`, `open_in_editor_at` (reuses the existing T1/T5 functions and the click path's borrow-safe clone).
- `Exit` → `chronology_focused = false`.
- After any selection change, adjust `chronology_scroll` so the selected row stays visible.
Any key that is not a recognized nav key while focused is **swallowed** (return `Ok(())` without forwarding to the PTY). The `Ctrl-x` leader still arms normally (so exit and other chords work).

### 4. Mouse (mirror keyboard, `src/app/input.rs` `handle_mouse`)

- Click on an entry header rect → `chronology_focused = true`, `chronology_sel = Entry(i)`, `chronology_expanded = Some(i)`.
- Click on the **expanded detail rect** → open the editor at `file:line` for that entry (same as `Open`).
- Wheel over the bar → scroll the viewport (unchanged).

### 5. Rendering (`src/ui/chronology_bar.rs`, `src/ui/attached.rs`, `src/app/render.rs`)

- `ChronologyDraw` gains `focused: bool` and `sel: ChronoSel`.
- The bar header/border renders in an active style when `focused`. The selected `Entry` row is highlighted; when `sel == Detail(i)`, the detail block of entry `i` is highlighted instead.
- The renderer records the expanded entry's **detail rect** as `chronology_detail_rect: Option<Rect>` on `App` (cleared each frame like the other transient rects), for mouse-open hit-testing.
- Viewport: the renderer (or the input glue) keeps the selected row within `[scroll, scroll+visible_rows)`.

### 6. Lifecycle / edge cases

- The existing focused-workspace-change reset also resets `chronology_focused = false` and `chronology_sel = Entry(0)`.
- Selection is clamped to the event count every resolve; an empty timeline makes entering a no-op (nothing to select).
- If the bar auto-hides (terminal narrowed) or is toggled off while focused, `chronology_focused` drops to `false` so keys flow back to the agent.
- `chronology_detail_rect` and the entry rects are cleared at the top of each frame (the existing per-frame clear block).

## Testing

- **Reducer (`nav`)** — table-driven over every transition: `Down`/`Up` across `Entry`↔`Detail`, into-detail only when expanded, `Top`/`Bottom`, `Enter` expand/collapse/open, `Esc` exit, and `len == 0` / single-entry / boundary clamps.
- **Auto-scroll math** — selecting above/below the viewport adjusts `chronology_scroll` to keep the row visible; pure function tested directly.
- **Mouse hit-resolution** — header rect vs detail rect → select+expand vs open; reuses the existing rect-hit pattern.
- **Key mapping** — `j`/`k`/`g`/`G`/arrows/Enter/Esc → `NavKey`, and that non-nav keys are swallowed while focused (logic-level test where feasible).
- Existing chronology tests remain green; no regression to PTY forwarding when not focused.

## Files touched

- `src/ui/chronology_nav.rs` (new) — `ChronoSel`, `NavKey`, `NavAction`, `nav`, auto-scroll helper (pure).
- `src/app.rs` — `chronology_focused`, `chronology_sel`, `chronology_detail_rect` + init + reset hook.
- `src/app/input.rs` — enter/exit via `Ctrl-x`+arrow; in-pane key interception; mouse header/detail handling.
- `src/ui/chronology_bar.rs` — focus/selection highlight; detail-rect recording; `ChronologyDraw` fields.
- `src/ui/attached.rs` — thread focus/selection + detail rect through `render_panes`.
- `src/app/render.rs` — pass focus/selection into the draw; clear `chronology_detail_rect`; keep selection in view.
- `README.md` — document the keyboard navigation and the mouse model.
