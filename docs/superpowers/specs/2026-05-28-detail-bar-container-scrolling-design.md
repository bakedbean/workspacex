# Detail-Bar Container Scrolling — Design

## Problem

Some workspace detail-bar modules (notably `RECENT FILES` and `SESSION SUMMARY`) produce more content than fits in their container's available height. Today the excess is silently clipped — `Paragraph::new(lines)` shows the top K lines and discards the rest. Users can't reach the hidden content.

## Goal

Allow each detail-bar container to scroll its content independently via the mouse wheel, with a visible scrollbar indicating that more content exists.

## Non-goals

- Keyboard scrolling for detail-bar containers. The detail bar has no focus model on the dashboard view; adding one is out of scope.
- Sticky module titles within a scrolling container. Titles scroll with content.
- Per-workspace scroll memory. Switching workspaces resets all detail-bar offsets to zero.
- Reflowing or re-laying-out modules in response to scroll. Layout is fixed at draw time; scrolling just changes which slice of the laid-out content is visible.

## Approach

Refactor `DetailModule` so modules produce content (`Vec<Line<'static>>`) rather than paint into a `Frame`. Containers then own placement: they build a virtual line list (titles, module bodies, gap rows), maintain a scroll offset, and paint a slice into the visible area along with a scrollbar in a reserved rightmost column.

This eliminates the need for an off-screen `ratatui::buffer::Buffer` that was contemplated in the initial brainstorming — since modules return `Vec<Line>`, slicing the vec and feeding it to `Paragraph::new` is equivalent and simpler.

## Trait change

```rust
pub trait DetailModule: Send + Sync {
    fn id(&self) -> &'static str;
    fn title(&self) -> &'static str;
    fn lines(&self, ctx: &DetailContext<'_>, width: u16) -> Vec<Line<'static>>;
}
```

Removed: `height_hint(&self, ctx) -> Constraint`, `render(&self, area, ctx, frame)`.

`width` is passed because every existing module uses `area.width` to decide wrapping/truncation widths. Natural content height becomes `lines.len() as u16`; no separate hint method is needed.

The semantic shift: today `Constraint::Min(N)` lets modules share leftover space within a container (the layout solver grows them). Tomorrow each module gets exactly `lines.len()` rows. This is acceptable because scrolling fills the role the leftover-space sharing previously filled — a tall module can produce many rows and the user scrolls to see them.

## Container rendering

Inside `render_container`, for each container:

1. **Build virtual lines.**
   ```text
   virtual: Vec<Line> = []
   for each module m in container:
       virtual.push(Line(m.title(), bold))           // 1 row
       virtual.extend(m.lines(ctx, content_width))
       if not last module: virtual.push(Line(""))    // 1-row gap
   content_height = virtual.len() as u16
   ```
   `content_width = area.width.saturating_sub(1)` — reserves the rightmost column for the scrollbar. The column is reserved unconditionally (even when content fits) to keep column widths stable across draws.

2. **Clamp the offset.**
   ```text
   max_offset = content_height.saturating_sub(area.height)
   offset = min(stored_offset, max_offset)
   if offset != stored_offset: write back the clamped value
   ```
   This handles the "content shrunk past offset" reset rule automatically — no explicit hook needed.

3. **Paint content slice.**
   ```text
   end = min(offset + area.height, content_height)
   visible = virtual[offset .. end].to_vec()
   content_area = Rect { x: area.x, y: area.y, width: content_width, height: area.height }
   frame.render_widget(Paragraph::new(visible), content_area)
   ```

4. **Paint scrollbar** in the reserved column, only when `content_height > area.height`. Use ratatui's `Scrollbar` widget with `ScrollbarState::new(content_height as usize).position(offset as usize)`. When content fits, the column stays blank.

The outer body region (top/bottom rules, vertical `┬`/`│`/`┴` separators between containers, header strip, chip row, reply input) is unchanged.

## State on `App`

```rust
// New fields:
detail_scroll_offsets: [u16; 4],
detail_scroll_last_workspace: Option<WorkspaceId>,
detail_container_rects: [Option<Rect>; 4],
```

- `detail_scroll_offsets`: per-slot scroll position. Fixed-size array because `DetailBarConfig.containers` is bounded to 4.
- `detail_scroll_last_workspace`: sentinel for reset-on-workspace-switch. On each draw, before `render_body_region`, match `app.selected_target()` against `Some(SelectionTarget::Workspace(id))` and compare `id` to this value; if different, zero the offsets and update the sentinel. When `selected_target` is anything else, do nothing (no workspace is selected, so the detail bar isn't drawn anyway — see `app/render.rs:231`).
- `detail_container_rects`: populated each draw with the `Rect` for each rendered container; consumed by `handle_mouse` for hit-testing. Same draw-populates-input-reads pattern as the existing `chip_rects` field on `App` (which is a `Vec<Rect>`). We use a fixed-size `[Option<Rect>; 4]` here rather than `Vec` because containers have stable slot identities tied to scroll offsets — slot index matters, not order.

## Input wiring

In `handle_mouse` (`src/app/input.rs`), ahead of the existing `scroll_active` routing:

```rust
if matches!(m.kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown)
    && matches!(app.view, View::Dashboard)
{
    if let Some(idx) = container_under_cursor(app, m.column, m.row) {
        let up = matches!(m.kind, MouseEventKind::ScrollUp);
        adjust_detail_scroll(app, idx, 3, up);
        return; // consumed
    }
}
// else: fall through to existing scroll_active routing
```

- `container_under_cursor(app, col, row) -> Option<usize>`: point-in-rect against `app.detail_container_rects`.
- `adjust_detail_scroll(app, idx, delta, up)`: bumps `detail_scroll_offsets[idx]` by ±`delta`, clamped to `[0, u16::MAX]`. The next draw clamps down to the real `max_offset` — this "optimistic update, clamp on draw" pattern keeps input handling decoupled from layout state.

The `View::Dashboard` guard ensures we don't steal scroll events from `View::Attached` (PTY scroll) or `View::AttachedPm`.

Scroll step is **3 rows** to match the existing `scroll_active(app, 3, …)` constant — consistent feel across views.

## Reset rules

| Trigger | Mechanism |
|---|---|
| Selected workspace changes | Zero `detail_scroll_offsets` when `detail_scroll_last_workspace` doesn't match current workspace ID; runs in `render` before any container draws. |
| Content height shrinks below offset | Per-container clamp during `render_container`: `offset = min(offset, max_offset)`. |

Not implemented: reset on `DetailBarConfig` change. Empirically rare (config is edited offline), and the per-container clamp handles the degenerate case where new modules produce less content than the old offset.

## Testing

### Module-level

Each of `recent_chat`, `recent_files`, `processes`, `session_summary`:
- Drop the existing `height_hint_*` tests (method is gone).
- Add `lines_*` tests asserting line counts and content shape against a stub context. Existing `tests_helpers::stub_context` carries over unchanged.

### Container-level (new tests in `src/ui/dashboard/detail.rs`)

Using ratatui's `TestBackend` + `Terminal::draw`:

- `container_with_short_content_no_scrollbar`: content_height ≤ area.height → rightmost column is blank.
- `container_with_tall_content_renders_scrollbar`: content_height > area.height → Scrollbar glyphs present at expected cells.
- `offset_clamps_when_content_shrinks`: pre-seed offset = 20, render with content_height = 5, area.height = 3 → offset clamps to 2.
- `offset_resets_on_workspace_change`: render with workspace A and offset 5, then with workspace B → offset is 0.
- `scrollbar_position_reflects_offset`: at offset ≈ content_height / 2, thumb is near the middle of the column.

### Input (new tests in `src/app/input_tests.rs`)

- `wheel_over_container_scrolls_that_container`: seed `detail_container_rects` with two non-overlapping rects, ScrollDown inside rect 1 → `detail_scroll_offsets[1] == 3`, `[0] == 0`.
- `wheel_outside_containers_falls_through`: ScrollDown outside any container rect → offsets unchanged.
- `wheel_in_attached_view_does_not_touch_detail_offsets`: view = Attached, ScrollDown → `detail_scroll_offsets` unchanged even when rects are populated from a prior draw.

## Migration / blast radius

| File | Change |
|---|---|
| `src/detail_modules/mod.rs` | Trait redefinition. Drop `height_hint`, replace `render` with `lines`. |
| `src/detail_modules/recent_chat.rs` | Refactor `render` → `lines`. Drop `height_hint`. ~10–15 lines. |
| `src/detail_modules/recent_files.rs` | Same. ~10 lines. |
| `src/detail_modules/processes.rs` | Same. ~10 lines. |
| `src/detail_modules/session_summary.rs` | Same. ~15 lines. |
| `src/ui/dashboard/detail.rs` | Rewrite `render_container` per Container Rendering section. Add reset-on-workspace-change check in `render`. Populate `app.detail_container_rects`. |
| `src/app/input.rs` | Add `container_under_cursor`, `adjust_detail_scroll`. New branch in `handle_mouse`. |
| `src/app/mod.rs` (or wherever `App` lives) | Add three new fields, default-initialize. |

No public-API or config-schema changes. Existing `DetailBarConfig` is untouched.

## Open questions

None — all design decisions are settled. Implementation can proceed.
