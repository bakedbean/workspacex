# Chronology Keyboard Navigation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Change Chronology bar a focusable, keyboard-navigable pane in the attached view — `Ctrl-x`+arrow to enter/exit, arrows/`j`/`k`/`g`/`G` to walk the list, `Enter` to expand a detail then `Enter` again (after arrowing into it) to open the editor at the changed line — and align the mouse to the same model.

**Architecture:** A focus mode on `App` (`chronology_focused` + a two-level `ChronoSel` cursor) layered over the existing chronology state. The navigation logic is a **pure reducer** (`nav`) so it's unit-tested without a terminal; the input handler is thin glue that calls the reducer and applies side effects. While focused, the attached key handler intercepts nav keys before the PTY-forward path. Mouse mirrors keyboard.

**Tech Stack:** Rust, `ratatui` (TUI), `crossterm` (input). Tests are `#[cfg(test)]` unit tests via `cargo test`.

**Builds on:** the shipped Change Chronology bar. Current facts (verified):
- `src/ui/attached.rs`: `pub struct ChronologyDraw<'a> { config, events, worktree, scroll, expanded }` (lines ~30-35); `render_panes(..., chronology_bar: Option<(Rect, ChronologyDraw<'_>)>, theme)`; private `render_chronology_bar(f, bar_rect, draw, theme) -> Vec<(usize, Rect)>` (returns header-line rects, skips by `draw.scroll`); `PanesDrawOutput { ..., chronology_entry_rects: Vec<(usize, Rect)> }`; `pub fn split_for_chronology(area, &Option<ChronologyDraw>) -> (Rect, Option<Rect>)`.
- `src/ui/chronology_bar.rs`: `pub fn entry_lines(ev: &ChangeEvent, worktree: &Path, expanded: bool, width: u16) -> Vec<Line<'static>>`; `pub const MIN_AGENT_COLS`; `should_auto_hide`; `relative_display`.
- `src/app.rs`: `App` has `chronology: HashMap<WorkspaceId, Timeline>`, `chronology_scroll: usize`, `chronology_expanded: Option<usize>`, `chronology_entry_rects: Vec<(usize, Rect)>`, `chronology_bar_rect: Option<Rect>`, `chronology_last_workspace: Option<WorkspaceId>`; `pub fn refresh_chronology(...)`; `fn reset_chronology_state_on_workspace_change(...)` (resets scroll/expanded on workspace change).
- `src/app/input.rs`: attached leader block (`if app.leader_pending { match k.code { ... KeyCode::Left|Right|Up|Down => state.focus_direction(arrow), ... } }`); `state.focus_direction(arrow: Arrow) -> bool` returns whether focus moved; leader re-arm at `if k.code == LEADER_KEY && ctrl { app.leader_pending = true; return }`; default `let bytes = encode_key(k); session.writer.send(bytes)`; `handle_mouse` with a `Down(Left)` rect-hit chain whose FIRST branch hits `chronology_entry_rects`; `fn focused_attached_workspace(app) -> Option<(WorkspaceId, PathBuf)>`; a wheel-over-`chronology_bar_rect` scroll block.
- `crate::config::chronology::{resolve, Side}`; `crate::activity::chronology::{ChangeEvent, ChangeDetail, resolve_line_in_file}`; `crate::commands::external::open_in_editor_at`.

---

## File Structure

- `src/ui/chronology_nav.rs` (create) — pure nav: `ChronoSel`, `NavKey`, `NavAction`, `nav`, `adjust_scroll`. The single source of truth for the cursor state machine.
- `src/ui/mod.rs` (modify) — `pub mod chronology_nav;`.
- `src/ui/chronology_bar.rs` (modify) — `EntryHighlight` + add a highlight arg to `entry_lines` so the selected header / detail block renders highlighted.
- `src/ui/attached.rs` (modify) — `ChronologyDraw` gains `focused` + `sel`; `render_chronology_bar` highlights and returns a `ChronologyHits { entries, detail, visible_entries }`; thread through `render_panes`/`PanesDrawOutput`.
- `src/app.rs` (modify) — `chronology_focused`, `chronology_sel`, `chronology_detail_rect`, `chronology_visible_entries` + init + reset hook.
- `src/app/render.rs` (modify) — pass `focused`/`sel` into the draw; store detail rect + visible count; apply `adjust_scroll`; clear the new transient rect each frame.
- `src/app/input.rs` (modify) — `Ctrl-x`+arrow enter/exit; in-pane nav-key interception; mouse header=select+expand / detail=open.
- `README.md` (modify) — document keyboard nav + mouse model.

---

## Task 1: Pure navigation reducer

**Files:**
- Create: `src/ui/chronology_nav.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Create the module with types + tests**

Create `src/ui/chronology_nav.rs`:

```rust
//! Pure cursor state machine for keyboard navigation of the chronology bar.
//! Kept free of `App`/`ratatui` so every transition is unit-testable.
//!
//! See `docs/superpowers/specs/2026-06-05-chronology-keyboard-navigation-design.md`.

/// In-pane cursor while the chronology bar is keyboard-focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChronoSel {
    Entry(usize),
    Detail(usize),
}

impl Default for ChronoSel {
    fn default() -> Self {
        ChronoSel::Entry(0)
    }
}

impl ChronoSel {
    /// The entry index this cursor refers to (entry or its detail).
    pub fn index(self) -> usize {
        match self {
            ChronoSel::Entry(i) | ChronoSel::Detail(i) => i,
        }
    }
}

/// A navigation key, already mapped from the raw keystroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKey {
    Up,
    Down,
    Top,
    Bottom,
    Enter,
    Esc,
}

/// Side effect the caller must apply after a transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavAction {
    None,
    Expand(usize),
    Collapse(usize),
    Open(usize),
    Exit,
}

/// Pure transition. `expanded` is the currently-expanded entry index (the bar
/// allows one at a time); `len` is the entry count. Bounds-safe: never returns
/// an index >= `len` (when `len > 0`).
pub fn nav(sel: ChronoSel, key: NavKey, expanded: Option<usize>, len: usize) -> (ChronoSel, NavAction) {
    if key == NavKey::Esc {
        return (sel, NavAction::Exit);
    }
    if len == 0 {
        return (sel, NavAction::None);
    }
    let last = len - 1;
    match (sel, key) {
        // Down
        (ChronoSel::Entry(i), NavKey::Down) => {
            if expanded == Some(i) {
                (ChronoSel::Detail(i), NavAction::None)
            } else {
                (ChronoSel::Entry((i + 1).min(last)), NavAction::None)
            }
        }
        (ChronoSel::Detail(i), NavKey::Down) => (ChronoSel::Entry((i + 1).min(last)), NavAction::None),
        // Up
        (ChronoSel::Detail(i), NavKey::Up) => (ChronoSel::Entry(i), NavAction::None),
        (ChronoSel::Entry(i), NavKey::Up) => (ChronoSel::Entry(i.saturating_sub(1)), NavAction::None),
        // Top / Bottom
        (_, NavKey::Top) => (ChronoSel::Entry(0), NavAction::None),
        (_, NavKey::Bottom) => (ChronoSel::Entry(last), NavAction::None),
        // Enter
        (ChronoSel::Entry(i), NavKey::Enter) => {
            if expanded == Some(i) {
                (ChronoSel::Entry(i), NavAction::Collapse(i))
            } else {
                (ChronoSel::Entry(i), NavAction::Expand(i))
            }
        }
        (ChronoSel::Detail(i), NavKey::Enter) => (ChronoSel::Detail(i), NavAction::Open(i)),
        // Esc handled above.
        (_, NavKey::Esc) => unreachable!(),
    }
}

/// Adjust the viewport `scroll` so the selected entry index stays visible,
/// given how many entries were visible last frame. One-frame lag is fine.
pub fn adjust_scroll(scroll: usize, sel_index: usize, visible: usize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    if sel_index < scroll {
        return sel_index;
    }
    if visible > 0 && sel_index >= scroll + visible {
        return sel_index + 1 - visible;
    }
    scroll
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn down_moves_to_next_entry_when_collapsed() {
        assert_eq!(nav(ChronoSel::Entry(0), NavKey::Down, None, 3), (ChronoSel::Entry(1), NavAction::None));
    }

    #[test]
    fn down_steps_into_detail_when_expanded() {
        assert_eq!(nav(ChronoSel::Entry(1), NavKey::Down, Some(1), 3), (ChronoSel::Detail(1), NavAction::None));
    }

    #[test]
    fn down_from_detail_goes_to_next_entry() {
        assert_eq!(nav(ChronoSel::Detail(1), NavKey::Down, Some(1), 3), (ChronoSel::Entry(2), NavAction::None));
    }

    #[test]
    fn up_from_detail_returns_to_entry() {
        assert_eq!(nav(ChronoSel::Detail(2), NavKey::Up, Some(2), 3), (ChronoSel::Entry(2), NavAction::None));
    }

    #[test]
    fn up_from_entry_goes_previous_saturating() {
        assert_eq!(nav(ChronoSel::Entry(1), NavKey::Up, None, 3), (ChronoSel::Entry(0), NavAction::None));
        assert_eq!(nav(ChronoSel::Entry(0), NavKey::Up, None, 3), (ChronoSel::Entry(0), NavAction::None));
    }

    #[test]
    fn down_clamps_at_last() {
        assert_eq!(nav(ChronoSel::Entry(2), NavKey::Down, None, 3), (ChronoSel::Entry(2), NavAction::None));
    }

    #[test]
    fn top_and_bottom() {
        assert_eq!(nav(ChronoSel::Detail(1), NavKey::Top, Some(1), 3), (ChronoSel::Entry(0), NavAction::None));
        assert_eq!(nav(ChronoSel::Entry(0), NavKey::Bottom, None, 3), (ChronoSel::Entry(2), NavAction::None));
    }

    #[test]
    fn enter_toggles_expand_on_entry() {
        assert_eq!(nav(ChronoSel::Entry(1), NavKey::Enter, None, 3), (ChronoSel::Entry(1), NavAction::Expand(1)));
        assert_eq!(nav(ChronoSel::Entry(1), NavKey::Enter, Some(1), 3), (ChronoSel::Entry(1), NavAction::Collapse(1)));
    }

    #[test]
    fn enter_on_detail_opens() {
        assert_eq!(nav(ChronoSel::Detail(1), NavKey::Enter, Some(1), 3), (ChronoSel::Detail(1), NavAction::Open(1)));
    }

    #[test]
    fn esc_exits_from_anywhere() {
        assert_eq!(nav(ChronoSel::Entry(0), NavKey::Esc, None, 3).1, NavAction::Exit);
        assert_eq!(nav(ChronoSel::Detail(2), NavKey::Esc, Some(2), 3).1, NavAction::Exit);
    }

    #[test]
    fn empty_list_only_exits() {
        assert_eq!(nav(ChronoSel::Entry(0), NavKey::Down, None, 0), (ChronoSel::Entry(0), NavAction::None));
        assert_eq!(nav(ChronoSel::Entry(0), NavKey::Enter, None, 0), (ChronoSel::Entry(0), NavAction::None));
        assert_eq!(nav(ChronoSel::Entry(0), NavKey::Esc, None, 0).1, NavAction::Exit);
    }

    #[test]
    fn adjust_scroll_keeps_selection_visible() {
        // selection above viewport → scroll up to it
        assert_eq!(adjust_scroll(5, 2, 4, 10), 2);
        // selection below viewport → scroll so it's the last visible
        assert_eq!(adjust_scroll(0, 6, 4, 10), 3);
        // selection already visible → unchanged
        assert_eq!(adjust_scroll(2, 3, 4, 10), 2);
        // empty
        assert_eq!(adjust_scroll(3, 0, 4, 0), 0);
    }

    #[test]
    fn index_extracts_entry_index() {
        assert_eq!(ChronoSel::Entry(4).index(), 4);
        assert_eq!(ChronoSel::Detail(7).index(), 7);
    }
}
```

- [ ] **Step 2: Wire the module**

In `src/ui/mod.rs`, add `pub mod chronology_nav;` next to the other `pub mod` lines.

- [ ] **Step 3: Run tests to verify they fail then pass**

Run: `cargo test --lib chronology_nav`
Expected: after creating the file, all tests PASS (the module is self-contained, so there's no separate red phase beyond the initial compile). If anything fails, fix the reducer — the tests are the spec.

- [ ] **Step 4: Verify build**

Run: `cargo build` — clean, zero warnings (the module is referenced by tests; `pub` items won't warn).

- [ ] **Step 5: Commit**

```bash
git add src/ui/chronology_nav.rs src/ui/mod.rs
git commit -m "feat(chronology): pure keyboard-nav reducer (ChronoSel/nav/adjust_scroll)"
```

---

## Task 2: Highlight the selected entry / detail in `entry_lines`

**Files:**
- Modify: `src/ui/chronology_bar.rs`

- [ ] **Step 1: Write/extend the failing tests**

In `src/ui/chronology_bar.rs` tests, add (and update existing `entry_lines` call sites to pass the new arg — see Step 3):

```rust
#[test]
fn header_highlight_reverses_first_line() {
    let lines = entry_lines(&ev("/wt/a.rs", "fn foo()"), Path::new("/wt"), true, 40, EntryHighlight::Header);
    // first line (header) carries REVERSED
    let has_rev = lines[0].spans.iter().any(|s| s.style.add_modifier.contains(ratatui::style::Modifier::REVERSED));
    assert!(has_rev, "header line should be highlighted");
}

#[test]
fn detail_highlight_reverses_peek_lines_only() {
    let lines = entry_lines(&ev("/wt/a.rs", "fn foo()"), Path::new("/wt"), true, 40, EntryHighlight::Detail);
    // header NOT reversed
    assert!(!lines[0].spans.iter().any(|s| s.style.add_modifier.contains(ratatui::style::Modifier::REVERSED)));
    // at least one peek line (index >= 2) reversed
    let peek_rev = lines.iter().skip(2).any(|l| l.spans.iter().any(|s| s.style.add_modifier.contains(ratatui::style::Modifier::REVERSED)));
    assert!(peek_rev, "detail peek should be highlighted");
}

#[test]
fn no_highlight_leaves_lines_unreversed() {
    let lines = entry_lines(&ev("/wt/a.rs", "fn foo()"), Path::new("/wt"), false, 40, EntryHighlight::None);
    assert!(!lines.iter().any(|l| l.spans.iter().any(|s| s.style.add_modifier.contains(ratatui::style::Modifier::REVERSED))));
}
```

(The existing `ev(...)` test helper already exists in this module.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib chronology_bar`
Expected: FAIL — `EntryHighlight` not found / arity mismatch.

- [ ] **Step 3: Implement the highlight**

Add the enum and extend `entry_lines` in `src/ui/chronology_bar.rs`. Add `use ratatui::style::Modifier;` if not already imported (it is). Define:

```rust
/// Which part of an entry is keyboard-selected (for highlight). `None` when the
/// entry isn't the cursor target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryHighlight {
    None,
    Header,
    Detail,
}
```

Change the signature to:

```rust
pub fn entry_lines(
    ev: &ChangeEvent,
    worktree: &Path,
    expanded: bool,
    width: u16,
    highlight: EntryHighlight,
) -> Vec<Line<'static>> {
```

Build the lines as today (header line, then summary, then — when `expanded` — the diff-peek lines). Then, just before returning, apply the highlight by adding `Modifier::REVERSED` to the spans of the relevant lines:

```rust
    let mut out = out; // existing accumulator
    match highlight {
        EntryHighlight::None => {}
        EntryHighlight::Header => {
            if let Some(first) = out.first_mut() {
                for s in &mut first.spans {
                    s.style = s.style.add_modifier(Modifier::REVERSED);
                }
            }
        }
        EntryHighlight::Detail => {
            // peek lines are everything after the header(0) and summary(1)
            for line in out.iter_mut().skip(2) {
                for s in &mut line.spans {
                    s.style = s.style.add_modifier(Modifier::REVERSED);
                }
            }
        }
    }
    out
```

(Adapt to the actual variable name the function uses for its `Vec<Line>` accumulator — read the current body first.)

- [ ] **Step 4: Update existing callers in this file's tests**

The current tests call `entry_lines(ev, wt, expanded, width)` with 4 args. Update each to pass `EntryHighlight::None` as the 5th arg so they compile. (The non-test caller in `attached.rs` is updated in Task 3.)

- [ ] **Step 5: Run to verify pass**

Run: `cargo test --lib chronology_bar`
Expected: PASS (existing + 3 new). NOTE: this leaves `attached.rs` calling `entry_lines` with the old arity — the crate will not fully build until Task 3. That's expected; this task's unit tests run via `cargo test --lib chronology_bar` which compiles the lib — so if the lib doesn't compile due to attached.rs, do Task 3's `entry_lines` call update in the same commit. To keep the tree compiling, **fold Step 3 of Task 3 (the single `entry_lines` call-site update in attached.rs) into this commit.** Concretely: in `src/ui/attached.rs` `render_chronology_bar`, change the `entry_lines(ev, draw.worktree, expanded, inner_width)` call to `entry_lines(ev, draw.worktree, expanded, inner_width, EntryHighlight::None)` for now (real highlight wiring lands in Task 3).

- [ ] **Step 6: Commit**

```bash
git add src/ui/chronology_bar.rs src/ui/attached.rs
git commit -m "feat(chronology): EntryHighlight arg on entry_lines for selection rendering"
```

---

## Task 3: `ChronologyDraw` focus/sel + detail rect + visible count

**Files:**
- Modify: `src/ui/attached.rs`

- [ ] **Step 1: Extend `ChronologyDraw` and the hits return**

In `src/ui/attached.rs`:

Add fields to `ChronologyDraw`:

```rust
    /// Keyboard focus is in the bar (drives the active header + selection highlight).
    pub focused: bool,
    /// In-pane cursor while focused.
    pub sel: crate::ui::chronology_nav::ChronoSel,
```

Add a hits struct near `PaneSpec`:

```rust
/// Mouse/scroll hit targets produced by painting the chronology bar.
pub struct ChronologyHits {
    /// `(entry_index, header_rect)` per drawn entry.
    pub entries: Vec<(usize, Rect)>,
    /// The expanded entry's detail block `(entry_index, rect)`, if any was drawn.
    pub detail: Option<(usize, Rect)>,
    /// Number of entries drawn this frame (for auto-scroll).
    pub visible_entries: usize,
}
```

Add to `PanesDrawOutput`:

```rust
    /// The expanded entry's detail rect (for mouse "open at line"), if shown.
    pub chronology_detail_rect: Option<(usize, Rect)>,
    /// Entries drawn in the chronology bar this frame (for keyboard auto-scroll).
    pub chronology_visible_entries: usize,
```

Initialize both at the `PanesDrawOutput` construction site (default `None` / `0` when no bar).

- [ ] **Step 2: Update `render_chronology_bar` to highlight + return `ChronologyHits`**

Change its return type to `ChronologyHits` and, inside the entry loop, compute the highlight from `draw.focused`/`draw.sel` and capture the detail rect:

```rust
fn render_chronology_bar(
    f: &mut Frame,
    bar_rect: Rect,
    draw: &ChronologyDraw<'_>,
    theme: &Theme,
) -> ChronologyHits {
    use crate::ui::chronology_bar::EntryHighlight;
    use crate::ui::chronology_nav::ChronoSel;
    // ... unchanged early returns now return ChronologyHits::default-equivalent:
    //     ChronologyHits { entries: Vec::new(), detail: None, visible_entries: 0 }
    // ... header rendering: when draw.focused, render the header with an active
    //     style, e.g. theme.header_style().add_modifier(Modifier::BOLD); else as today.

    let mut entry_rects: Vec<(usize, Rect)> = Vec::new();
    let mut detail_rect: Option<(usize, Rect)> = None;
    let mut visible_entries = 0usize;

    let mut cursor_y = body_y;
    for (i, ev) in draw.events.iter().enumerate().skip(draw.scroll) {
        if cursor_y >= body_bottom {
            break;
        }
        let expanded = Some(i) == draw.expanded;
        let highlight = if draw.focused {
            match draw.sel {
                ChronoSel::Entry(s) if s == i => EntryHighlight::Header,
                ChronoSel::Detail(s) if s == i => EntryHighlight::Detail,
                _ => EntryHighlight::None,
            }
        } else {
            EntryHighlight::None
        };
        let lines = crate::ui::chronology_bar::entry_lines(ev, draw.worktree, expanded, inner_width, highlight);
        let available = body_bottom.saturating_sub(cursor_y);
        let drawn = (lines.len() as u16).min(available);
        if drawn == 0 {
            break;
        }
        let entry_area = Rect { x: content.x, y: cursor_y, width: inner_width, height: drawn };
        f.render_widget(Paragraph::new(lines), entry_area);
        entry_rects.push((i, Rect { x: content.x, y: cursor_y, width: inner_width, height: 1 }));
        // The detail block is the rows below the header line (when expanded & drawn).
        if expanded && drawn > 1 {
            detail_rect = Some((
                i,
                Rect { x: content.x, y: cursor_y.saturating_add(1), width: inner_width, height: drawn - 1 },
            ));
        }
        visible_entries += 1;
        cursor_y = cursor_y.saturating_add(drawn);
    }

    ChronologyHits { entries: entry_rects, detail: detail_rect, visible_entries }
}
```

Adapt the early-return sites (`bar_rect.width==0`, empty content, empty events) to return a `ChronologyHits` with empty `entries`, `None` detail, `0` visible.

- [ ] **Step 3: Thread the hits through `render_panes`**

In `render_panes`, the call site currently does:

```rust
    let chronology_entry_rects = match chronology_bar {
        Some((bar_rect, draw)) => render_chronology_bar(f, bar_rect, &draw, theme),
        None => Vec::new(),
    };
```

Change to:

```rust
    let chronology_hits = match chronology_bar {
        Some((bar_rect, draw)) => render_chronology_bar(f, bar_rect, &draw, theme),
        None => ChronologyHits { entries: Vec::new(), detail: None, visible_entries: 0 },
    };
```

and in the returned `PanesDrawOutput`, set:

```rust
        chronology_entry_rects: chronology_hits.entries,
        chronology_detail_rect: chronology_hits.detail,
        chronology_visible_entries: chronology_hits.visible_entries,
```

- [ ] **Step 4: Build**

Run: `cargo build` and `cargo test --lib attached`
Expected: clean build, zero warnings; existing attached tests still pass. (Callers in `render.rs` that construct `ChronologyDraw` must now set `focused`/`sel` — update them in Task 5; to keep the tree compiling, **do Task 5's `ChronologyDraw { ... }` field additions in this commit** by setting `focused: false, sel: Default::default()` at the render.rs construction site, with the real values wired in Task 5.)

- [ ] **Step 5: Commit**

```bash
git add src/ui/attached.rs src/app/render.rs
git commit -m "feat(chronology): focus/selection + detail-rect/visible-count in bar rendering"
```

---

## Task 4: App state for focus + selection

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add fields**

Add to the `App` struct (next to the other `chronology_*` fields):

```rust
    /// Keyboard focus is in the chronology bar (intercept nav keys).
    pub chronology_focused: bool,
    /// In-pane cursor while focused.
    pub chronology_sel: crate::ui::chronology_nav::ChronoSel,
    /// Transient per-frame detail rect of the expanded entry `(index, rect)`,
    /// for mouse "open at line". `None` when nothing is expanded/shown.
    pub chronology_detail_rect: Option<(usize, ratatui::layout::Rect)>,
    /// Entries drawn in the bar last frame (for keyboard auto-scroll).
    pub chronology_visible_entries: usize,
```

Initialize in `App::new`'s `Self { ... }` literal:

```rust
            chronology_focused: false,
            chronology_sel: crate::ui::chronology_nav::ChronoSel::default(),
            chronology_detail_rect: None,
            chronology_visible_entries: 0,
```

- [ ] **Step 2: Extend the workspace-change reset**

Find `reset_chronology_state_on_workspace_change` (it resets `chronology_scroll`/`chronology_expanded` on focused-workspace change). Extend it to also reset focus + selection:

```rust
    // inside the "changed" branch, alongside scroll = 0 / expanded = None:
    *chronology_focused = false;
    *chronology_sel = crate::ui::chronology_nav::ChronoSel::Entry(0);
```

(Match the function's existing `&mut`-field parameter style; add `chronology_focused: &mut bool` and `chronology_sel: &mut ChronoSel` params and pass them at the call site in `render.rs`, OR if it takes `&mut self`-style access, set the fields directly. Read the current signature and mirror it.)

- [ ] **Step 3: Build**

Run: `cargo build`
Expected: clean (these are `pub` fields/used in render next task; no dead-code warnings for `pub`).

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "feat(chronology): app state for keyboard focus + selection"
```

---

## Task 5: Render wiring — pass focus/sel, store hits, auto-scroll

**Files:**
- Modify: `src/app/render.rs`

- [ ] **Step 1: Clear the new transient rect each frame**

In the per-frame clear block at the top of `draw()` (where `app.chronology_bar_rect = None;` etc. are set), add:

```rust
    app.chronology_detail_rect = None;
```

(`chronology_visible_entries` is overwritten each Attached frame, so it doesn't strictly need clearing, but you may set it to 0 here for cleanliness.)

- [ ] **Step 2: Build `ChronologyDraw` with focus + selection and auto-scroll before drawing**

In the `View::Attached` arm, where `ChronologyDraw { ... }` is constructed (currently with `scroll: app.chronology_scroll, expanded: app.chronology_expanded`), first apply auto-scroll so the selected row is visible, then pass focus + sel:

```rust
            // Keep the keyboard selection in view (uses last frame's visible count).
            if app.chronology_focused {
                app.chronology_scroll = crate::ui::chronology_nav::adjust_scroll(
                    app.chronology_scroll,
                    app.chronology_sel.index(),
                    app.chronology_visible_entries,
                    chronology_events.len(),
                );
            }
            // ... existing chronology_events / worktree / cfg locals ...
            let chronology_draw = bar_rect.map(|_| crate::ui::attached::ChronologyDraw {
                config: &chronology_cfg,
                events: &chronology_events,
                worktree: &chronology_worktree,
                scroll: app.chronology_scroll,
                expanded: app.chronology_expanded,
                focused: app.chronology_focused,
                sel: app.chronology_sel,
            });
```

(Match the ACTUAL local variable names already in this arm — `chronology_events`, `chronology_worktree`, `chronology_cfg`, `bar_rect`, and however `chronology_draw` is currently built/zipped. The only changes are: the `adjust_scroll` call before construction, and the two new struct fields.)

- [ ] **Step 3: Store the new hits after `render_panes` returns**

Where `app.chronology_entry_rects = out.chronology_entry_rects;` is set, add:

```rust
            app.chronology_detail_rect = out.chronology_detail_rect;
            app.chronology_visible_entries = out.chronology_visible_entries;
```

- [ ] **Step 4: Build + manual sanity**

Run: `cargo build` (zero warnings) and `cargo test --lib` (no regressions).
Manual: attach to a Claude workspace; the bar still renders; nothing is focusable yet (input lands in Task 6), but the default (`focused:false`) path must look exactly as before.

- [ ] **Step 5: Commit**

```bash
git add src/app/render.rs
git commit -m "feat(chronology): wire focus/selection + auto-scroll into the attached render"
```

---

## Task 6: Input — enter/exit + in-pane key navigation

**Files:**
- Modify: `src/app/input.rs`

- [ ] **Step 1: Enter/exit via `Ctrl-x` + arrow**

In `handle_key_attached`'s leader block, locate the arrow arm that calls `state.focus_direction(arrow)`. Replace it so the chronology bar participates. Read the focused repo's config to know the side. Add a small helper near `focused_attached_workspace`:

```rust
/// Resolve the configured chronology side for the focused attached workspace.
fn focused_chronology_side(app: &App) -> Option<crate::config::chronology::Side> {
    let crate::ui::View::Attached(state) = &app.view else { return None };
    let target = state.focused_target()?;
    let ws_id = target.workspace_id;
    let (rid, _w) = app.workspaces.iter().find(|(_, w)| w.id == ws_id)?;
    let repo = app.repos.iter().find(|r| r.id == *rid)?;
    Some(crate::config::chronology::resolve(repo, &app.store).side)
}
```

Then the arrow arm becomes (adapt to the actual `Arrow` mapping already present):

```rust
            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => {
                let arrow = /* existing match k.code -> Arrow */;
                use crate::config::chronology::Side;
                let side = focused_chronology_side(app);
                let toward_bar = matches!(
                    (side, arrow),
                    (Some(Side::Right), Arrow::Right) | (Some(Side::Left), Arrow::Left)
                );
                let away_from_bar = matches!(
                    (side, arrow),
                    (Some(Side::Right), Arrow::Left) | (Some(Side::Left), Arrow::Right)
                );
                if app.chronology_focused {
                    if away_from_bar {
                        app.chronology_focused = false; // back to the agent pane
                    }
                    // toward/parallel arrows while focused: ignored here
                    return Ok(());
                }
                if toward_bar && app.chronology_bar_rect.is_some() {
                    // Only enter the bar if there's no agent pane further in that
                    // direction (we're at the edge). focus_direction returns false
                    // when it couldn't move.
                    let moved = if let View::Attached(state) = &mut app.view {
                        state.focus_direction(arrow)
                    } else {
                        false
                    };
                    if !moved {
                        app.chronology_focused = true;
                        app.chronology_sel = crate::ui::chronology_nav::ChronoSel::Entry(0);
                        app.chronology_scroll = 0;
                    }
                    return Ok(());
                }
                if let View::Attached(state) = &mut app.view {
                    state.focus_direction(arrow);
                }
                return Ok(());
            }
```

(`app.chronology_bar_rect.is_some()` is the cheap "bar is shown" check — it's set each frame by the renderer. `Arrow` is `crate::ui::split::Arrow`.)

- [ ] **Step 2: In-pane key interception**

After the leader-arming block (`if k.code == LEADER_KEY && ctrl { app.leader_pending = true; return Ok(()) }`) and BEFORE the default `let bytes = encode_key(k); session.writer.send(bytes)`, insert:

```rust
    if app.chronology_focused {
        use crate::ui::chronology_nav::{nav, NavAction, NavKey};
        let navkey = match k.code {
            KeyCode::Down | KeyCode::Char('j') => Some(NavKey::Down),
            KeyCode::Up | KeyCode::Char('k') => Some(NavKey::Up),
            KeyCode::Char('g') => Some(NavKey::Top),
            KeyCode::Char('G') => Some(NavKey::Bottom),
            KeyCode::Enter => Some(NavKey::Enter),
            KeyCode::Esc => Some(NavKey::Esc),
            _ => None,
        };
        if let Some(navkey) = navkey {
            let len = focused_attached_workspace(app)
                .and_then(|(id, _)| app.chronology.get(&id))
                .map(|t| t.events().len())
                .unwrap_or(0);
            let (new_sel, action) = nav(app.chronology_sel, navkey, app.chronology_expanded, len);
            app.chronology_sel = new_sel;
            match action {
                NavAction::None => {}
                NavAction::Expand(i) => app.chronology_expanded = Some(i),
                NavAction::Collapse(_) => app.chronology_expanded = None,
                NavAction::Exit => app.chronology_focused = false,
                NavAction::Open(i) => {
                    if let Some((worktree, file, detail)) = focused_attached_workspace(app)
                        .and_then(|(ws_id, worktree)| {
                            app.chronology.get(&ws_id).and_then(|t| {
                                t.events().get(i).map(|ev| (worktree, ev.file_path.clone(), ev.detail.clone()))
                            })
                        })
                    {
                        let line = crate::activity::chronology::resolve_line_in_file(&file, &detail);
                        let editor = app.store.get_setting("editor_cmd").ok().flatten();
                        if let Err(e) = crate::commands::external::open_in_editor_at(&worktree, &file, line, editor.as_deref()) {
                            tracing::warn!(error = %e, "failed to open editor from chronology keyboard");
                        }
                    }
                }
            }
        }
        // While focused, swallow ALL keys (recognized or not) — the agent PTY
        // must not receive them.
        return Ok(());
    }
```

- [ ] **Step 3: Build + manual test**

Run: `cargo build` (zero warnings), `cargo test --lib` (no regressions; the nav logic itself is covered by Task 1's reducer tests).
Manual: attach to a Claude workspace with some edits. `Ctrl-x →` (default right-side bar) focuses the bar (top entry highlighted). `j`/`k`/arrows move; `g`/`G` jump; `Enter` expands; `↓` into the detail, `Enter` opens the editor at the line; `Esc` or `Ctrl-x ←` exits back to the agent. Confirm typing is NOT sent to the agent while focused.

- [ ] **Step 4: Commit**

```bash
git add src/app/input.rs
git commit -m "feat(chronology): keyboard focus enter/exit + in-pane list navigation"
```

---

## Task 7: Input — mouse mirrors keyboard

**Files:**
- Modify: `src/app/input.rs`

- [ ] **Step 1: Update the click handling**

In `handle_mouse`'s `Down(Left)` chain, the first branch currently hits `chronology_entry_rects` and (per the base feature) toggles expand / opens on second click. Replace that branch's body so it mirrors the keyboard model:

- A click on the **detail rect** opens the editor (check `chronology_detail_rect` FIRST, since the detail lies within/below the entry area).
- Otherwise a click on an **entry header rect** focuses the bar, selects that entry, and expands it.

```rust
            // Chronology: detail click opens; header click selects+expands.
            if let Some((idx, _)) = app.chronology_detail_rect.filter(|(_, r)| {
                m.column >= r.x && m.column < r.x.saturating_add(r.width)
                    && m.row >= r.y && m.row < r.y.saturating_add(r.height)
            }) {
                if let Some((worktree, file, detail)) = focused_attached_workspace(app)
                    .and_then(|(ws_id, worktree)| {
                        app.chronology.get(&ws_id).and_then(|t| {
                            t.events().get(idx).map(|ev| (worktree, ev.file_path.clone(), ev.detail.clone()))
                        })
                    })
                {
                    let line = crate::activity::chronology::resolve_line_in_file(&file, &detail);
                    let editor = app.store.get_setting("editor_cmd").ok().flatten();
                    if let Err(e) = crate::commands::external::open_in_editor_at(&worktree, &file, line, editor.as_deref()) {
                        tracing::warn!(error = %e, "failed to open editor from chronology detail click");
                    }
                }
                app.chronology_focused = true;
                app.chronology_sel = crate::ui::chronology_nav::ChronoSel::Detail(idx);
                return Ok(());
            } else if let Some(idx) = app.chronology_entry_rects.iter().find_map(|(i, r)| {
                let hit = m.column >= r.x && m.column < r.x.saturating_add(r.width)
                    && m.row >= r.y && m.row < r.y.saturating_add(r.height);
                hit.then_some(*i)
            }) {
                app.chronology_focused = true;
                app.chronology_sel = crate::ui::chronology_nav::ChronoSel::Entry(idx);
                app.chronology_expanded = Some(idx);
                return Ok(());
            } else if let Some(idx) = app.chip_rects.iter().position(|r| {
                // ... existing chip_rects branch, unchanged, remains the next else-if ...
```

Preserve the rest of the chain (chip_rects → attention_rects → agent_chip_rects → …) exactly; only the chronology branch changes (it splits into the detail-first / header-second pair above). Read the current chain and integrate carefully.

- [ ] **Step 2: Build + manual test**

Run: `cargo build` (zero warnings), `cargo test --lib` (no regressions).
Manual: click an entry → it focuses+selects+expands; click the expanded diff peek → editor opens at the line. Wheel still scrolls.

- [ ] **Step 3: Commit**

```bash
git add src/app/input.rs
git commit -m "feat(chronology): mouse mirrors keyboard (click entry expands, click detail opens)"
```

---

## Task 8: README

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Document keyboard nav + mouse model**

In the existing "Change chronology" subsection (and the attached-view keybindings table), document:
- `Ctrl-x` + arrow toward the bar's side focuses it; arrow away or `Esc` exits.
- While focused: `↑`/`k`, `↓`/`j` move; `g`/`G` top/bottom; `Enter` expands an entry; arrow into the detail and `Enter` again opens the editor at the changed line (new file → top); other keys don't reach the agent while focused.
- Mouse: click an entry to expand it; click the expanded detail to open the file at the line; wheel scrolls.

Match the README's existing prose/table style. Add the new keybindings as rows in the attached-view table alongside `Ctrl-x c` / `Ctrl-x C`.

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: document chronology keyboard navigation and mouse model"
```

---

## Self-Review (completed during planning)

**Spec coverage:**
- Focus enter/exit via `Ctrl-x`+arrow (toward/away, edge-aware via `focus_direction` return) → Task 6. ✓
- List nav arrows/`j`/`k`/`g`/`G` → Task 1 (reducer) + Task 6 (key map). ✓
- Two-level cursor (`Entry`/`Detail`), into-detail on `↓` when expanded → Task 1 (`nav`). ✓
- `Enter` expand; `Enter` on detail opens → Task 1 + Task 6. ✓
- Mouse mirrors (entry=select+expand, detail=open) → Task 7. ✓
- Selection/focus highlight + active header → Task 2 (`EntryHighlight`) + Task 3 (render). ✓
- Detail rect for mouse open + visible count for auto-scroll → Task 3. ✓
- Auto-scroll keeps selection visible → Task 1 (`adjust_scroll`) + Task 5. ✓
- Lifecycle reset on workspace change; clamp; drop focus when bar hidden → Task 4 (reset) + Task 6 (`chronology_bar_rect.is_some()` gate) + bounds-safe reducer. ✓
- Testing: pure reducer + adjust_scroll + highlight tests → Tasks 1, 2. ✓

**Placeholder scan:** No TBDs. Where exact local/variable names in `render.rs`/`input.rs` can't be reproduced verbatim, the step shows the full code and instructs to match the actual surrounding names — the code to write is concrete, not deferred. The cross-task compile ordering (Task 2 folds the `attached.rs` `entry_lines` arity bump; Task 3 folds the `render.rs` `ChronologyDraw` field defaults) is called out explicitly so the tree always compiles.

**Type consistency:** `ChronoSel` (`Entry`/`Detail`, `.index()`), `NavKey`, `NavAction` (`None`/`Expand`/`Collapse`/`Open`/`Exit`), `nav`, `adjust_scroll`, `EntryHighlight` (`None`/`Header`/`Detail`), `entry_lines(.., EntryHighlight)`, `ChronologyDraw { .., focused, sel }`, `ChronologyHits { entries, detail, visible_entries }`, `PanesDrawOutput { .., chronology_detail_rect, chronology_visible_entries }`, `App { .., chronology_focused, chronology_sel, chronology_detail_rect, chronology_visible_entries }` — names are consistent across tasks.
