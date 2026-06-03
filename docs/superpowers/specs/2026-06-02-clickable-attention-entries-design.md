# Clickable attention entries in the agent-chat status row

**Date:** 2026-06-02
**Status:** Approved

## Summary

In the agent-chat views (`Attached` and `AttachedPm`), a status row below the
pinned-command chips lists "other workspaces that need attention." Today that
row is display-only. This change makes each listed workspace clickable: a left
click on an entry fully attaches to that workspace — identical to pressing
`Enter` on it in the dashboard. The row's appearance is unchanged.

## Motivation

The pinned-command chips in the same chrome stack are already clickable. Users
reasonably expect the attention entries right below them to behave the same
way. Making them clickable removes a context switch (return to dashboard, find
the workspace, attach) when a glance at the status row already shows exactly
which workspace wants attention.

## Existing pattern: the "clickable chip" contract

Clickability in this TUI is a three-part contract, all keyed off rects captured
during the render pass:

1. **Layout** — at render time, compute one `Rect` per clickable element
   (`layout_chip_row` in `src/ui/attached.rs`).
2. **Per-frame storage** — store those rects on `App` (`app.chip_rects`),
   cleared at the top of every frame (`src/app/render.rs:16`).
3. **Hit-test** — in `handle_mouse` (`src/app/input.rs`), map the click
   coordinate to an element by testing each stored rect with exclusive upper
   bounds and saturating arithmetic.

This design replicates that contract for the attention entries.

## Decisions (from brainstorming)

- **Click action:** full attach via `attach_workspace()` (ensures a session,
  restores saved layout, switches the view). Same as `Enter` on the dashboard.
- **Visual affordance:** none. The row stays visually identical to today, for
  parity with the chips (which also have no hover/click hint).
- **Scope:** both the `Attached` and `AttachedPm` views' status rows.

## Design

### 1. Carry per-entry geometry out of the formatter

File: `src/ui/updates_bar.rs`

`format_attention_line_styled` currently returns `Option<Line<'static>>`,
discarding where each entry sits. Change it to return `Option<AttentionLine>`:

```rust
/// The rendered attention line plus the clickable geometry of each entry.
pub struct AttentionLine {
    pub line: Line<'static>,
    /// One segment per *rendered* entry (i.e. the `included` ones, not the
    /// `… +N more` overflow). Columns are 0-based from the line's left edge.
    pub segments: Vec<AttentionSegment>,
}

pub struct AttentionSegment {
    pub workspace_id: WorkspaceId,
    pub start_col: u16,
    pub width: u16,
}
```

The function already computes a per-entry `widths: Vec<usize>` and an
`included` count. While building spans, accumulate a running column offset and
emit one `AttentionSegment` per included entry. Each segment spans the entry's
clickable extent — glyph + space + `repo/name` + ` (age)` — and **excludes**
the `" │ "` separator that precedes entries after the first. The
`… +N more` overflow tail is not a real workspace and produces no segment.

The running offset must add the 3-column separator width before every entry
after the first, mirroring the existing layout math, so `start_col` stays in
sync with what is actually drawn.

### 2. Surface segments through the render pass

File: `src/app/render.rs`

- `compute_attention_line` returns `Option<AttentionLine>` instead of
  `Option<Line>`. Existing callers that only need the line for the layout
  decision use `line.is_some()` / `line.line`.
- In both the `View::Attached` and `View::AttachedPm` branches, once
  `status_area` is known, map each `AttentionSegment` to an absolute rect:

  ```rust
  Rect {
      x: status_area.x.saturating_add(seg.start_col),
      y: status_area.y,
      width: seg.width,
      height: 1,
  }
  ```

  Collect `Vec<(WorkspaceId, Rect)>` and store it on `app.attention_rects`.
  The `render_panes` call still receives `attention.line` as before.

  Note: the status line is rendered flush at `status_area.x` (no glyph prefix
  is prepended in `render_panes`), so `start_col` maps directly to screen
  columns with no extra offset.

### 3. New `App` field, cleared each frame

File: `src/app.rs` (struct) and `src/app/render.rs:16` (clear site)

Add:

```rust
pub attention_rects: Vec<(WorkspaceId, Rect)>,
```

Initialize empty, and clear it alongside `app.chip_rects.clear()` at the top of
the render pass so stale rects from a previous frame never fire.

### 4. Hit-test on click

File: `src/app/input.rs`, in `handle_mouse`'s
`MouseEventKind::Down(MouseButton::Left)` arm.

After the existing chip hit-test, add an `else if` branch that scans
`app.attention_rects` with the same bounds test used for chips:

```rust
} else if let Some((ws_id, _)) = app.attention_rects.iter().copied().find(|(_, r)| {
    m.column >= r.x
        && m.column < r.x.saturating_add(r.width)
        && m.row >= r.y
        && m.row < r.y.saturating_add(r.height)
}) {
    let _ = crate::app::attach_workspace(app, ws_id);
}
```

`attach_workspace` is synchronous and already `pub(crate)`; it clears the
workspace's `needs_attention`, ensures/restores the session, and flips
`app.view` to `Attached`. The chip row and status row occupy different screen
rows, so the two hit-tests cannot both match; the `else if` is for tidiness and
a clear single-dispatch.

## Edge cases

- **Separators / overflow text:** the `" │ "` gaps and the `… +N more` tail are
  not segments, so clicking them is a no-op.
- **UpdatesPanel modal open:** `compute_attention_line` already returns `None`
  in that case, so no segments and no rects exist.
- **Empty attention list:** no segments, no rects, nothing clickable.
- **Missing session at click time:** `attach_workspace` already handles
  ensuring/spawning the session and the agent-missing case.

## Testing

File: `src/ui/updates_bar.rs` tests.

- Update the two existing tests that call `format_attention_line_styled` for the
  new `Option<AttentionLine>` return type (use `.line` / `.segments`).
- Add a test with multiple entries asserting that each `AttentionSegment`'s
  `start_col` and `width` line up with the rendered spans — in particular that
  the 3-column separator offset is accounted for between entries, and that an
  overflow (`… +N more`) case produces segments only for the included entries
  (count matches `included`, not the full input length).

## Out of scope

- Any hover/underline/visual affordance on the entries.
- Keyboard navigation of the attention row.
- Changes to which workspaces appear or how they're sorted (`collect_attention`
  is unchanged).
