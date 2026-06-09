# Two diff views in the change-detail modal

## Goal

Incorporate chronox's new side-by-side diff into wsx's change-detail modal, so a
change can be viewed either as the existing **unified** diff or as a new
**side-by-side** diff, toggled with `d`. The chosen view is session-sticky.

## Background

The detail modal (`Modal::ChangeDetail`) currently stores a pre-rendered
`lines: Vec<Line>` built once by `chronox::change_detail_lines_styled` (a unified
diff: all removed lines, then all added lines). chronox `main` (PR #4,
`1b3b9f99…`) adds a framework-agnostic side-by-side model:

- `change_detail_side_by_side(detail, base_line, lang) -> Vec<SideRow>`, where
  `SideRow { left: Option<DiffCell>, right: Option<DiffCell> }` aligns old/new by
  an LCS so only genuinely-changed lines are marked.
- `side_cell_to_line(Option<&DiffCell>) -> Line<'static>` (ratatui feature)
  renders one cell to a styled line; `None` is a blank column.

wsx is pinned to an older rev (`3db0686…`) that predates this.

## Design

### 1. Dependency bump
Move the `chronox` git `rev` in `Cargo.toml` from `3db0686…` to `1b3b9f99…` and
refresh `Cargo.lock` (`cargo update -p chronox`). The existing unified API is
unchanged, so the bump alone is behaviour-neutral.

### 2. View-mode state (session-sticky)
Add `enum DiffViewMode { Unified, SideBySide }` (default `Unified`) and a field on
`App` recording the last-used view. Opening a change reads the field; toggling
writes it — the choice sticks across opens for the session, no persisted config.

### 3. Modal storage
`Modal::ChangeDetail` additionally holds `rows: Vec<SideRow>` (built once at open
via `change_detail_side_by_side`) and the current `mode: DiffViewMode`. Both
representations are tiny (a single change), so building both upfront is cheaper
and simpler than re-tokenizing per frame, and keeps the renderer a pure function
of stored state.

### 4. Rendering
`render_change_detail_modal` branches on `mode`:
- **Unified**: unchanged.
- **SideBySide**: split inner width into `left | │ | right`; render each visible
  `SideRow` via `side_cell_to_line(row.left)` / `(row.right)` clipped to each
  column, joined by a dim `│` divider. Scroll clamps against the active view's
  length (`rows.len()` vs `lines.len()`).

On a narrow terminal the columns simply clip (via `clip_line_to_width`) — no
auto-fallback to unified; the user can press `d` for the full-width unified view.

### 5. Toggle + scroll
New `KeyCode::Char('d')` arm in the ChangeDetail handler flips `mode`, mirrors it
to the `App` field, and resets scroll to 0 (the two models do not share line
indices, so a reset is predictable where a clamp would mislead). The mouse-wheel
handler measures length from the active view. The footer gains a `d split` /
`d unified` hint.

## Testing
- Toggling flips `mode` and the value persists on `App` state across opens.
- Side-by-side render emits two columns with a divider.
- Scroll resets to 0 on toggle.
- Existing unified-modal tests continue to pass.

## Commits
1. `chore(deps): bump chronox to side-by-side diff rev`
2. `feat(ui): model unified/side-by-side diff view mode`
3. `feat(ui): render side-by-side diff in detail modal`
4. `feat(input): toggle diff view with 'd' (session-sticky)`
