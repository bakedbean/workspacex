# Dashboard footer: move workspace-only actions into a `?` overlay

**Date:** 2026-06-17
**Branch:** `bakedbean/footer-context-actions`
**Status:** Approved design

## Problem

The dashboard footer hint bar (`src/ui/dashboard/layout.rs:98-109`) is a static
list of 10 key hints. Four of them — `e` edit, `t` term, `v` diff, `g` lazygit —
only do anything when a **Workspace** row is selected. Their key handlers
(`src/app/input.rs:545-583`) all guard on
`Some(SelectionTarget::Workspace(id))` and silently no-op when nothing, or a
repo header, is selected. So they advertise themselves unconditionally in a
context-free bar even though they are context-only.

A fifth workspace-only action, `c` chronox (`src/app/input.rs:584-592`), follows
the same workspace-only pattern but is not shown in the footer at all, so it is
currently undiscoverable.

## Goal

Remove the four context-only hints from the always-on footer and give the
workspace-only actions a discoverable home: a small `?`-triggered overlay
listing them. The footer stays a single stable line.

## Non-goals

- No change to the behaviour of the `e`/`t`/`v`/`g`/`c` key handlers themselves.
- No selection-awareness inside the overlay (it is a static reference list).
- No general/global help screen — the overlay lists workspace actions only.

## Design

### 1. Footer (`src/ui/dashboard/layout.rs:98-109`)

Remove the four entries `("e", "edit")`, `("t", "term")`, `("v", "diff")`,
`("g", "lazygit")` from the `keys` array. Add `("?", "actions")` immediately
before `("q", "quit")`. Resulting footer:

```
↑↓ nav   ↵ open   n new   G group   / filter   ? actions   q quit
```

The clickable-pill machinery is unchanged. The footer loop maps each glyph to a
synthesized key via `key_for_glyph` (`src/ui/footer.rs:41-58`); `?` maps to the
`?` key event, so clicking the `? actions` pill opens the overlay exactly as
typing `?` does. The four removed pills lose their hint **and** their clickable
`FooterHintSpan` together — no orphaned click region remains.

### 2. New modal variant (`src/ui/modal/mod.rs:42`)

Add a dataless variant `Modal::WorkspaceActions` to the `Modal` enum. This reuses
the existing modal system, which is the right tier: when `app.modal.is_some()`
the dispatch gate at `src/app/input.rs:2051` routes **all** keys to
`handle_key_modal`, fully capturing input so the `e`/`t`/`v`/`g` keys cannot fire
their actions while the overlay is open. (The `/` filter and `G` group are inline
toggles that only partially capture input — wrong tier for this.)

### 3. Open / dismiss (`src/app/input.rs`)

- **Open:** in `handle_key_dashboard`, add an arm
  `(KeyCode::Char('?'), _) => { app.modal = Some(Modal::WorkspaceActions); }`,
  placed alongside the other dashboard key arms (near `/` at `input.rs:672` and
  `G` at `input.rs:662`).
- **Dismiss:** in `handle_key_modal`, the `Modal::WorkspaceActions` arm closes the
  overlay (`app.modal = None`) on `Esc` and on `?` (toggle). All other keys are
  swallowed with no pass-through, consistent with modal capture.

### 4. Render (`src/app/render.rs:653` + small renderer)

Add a `Modal::WorkspaceActions` arm to the `&app.modal` match. Draw the overlay
with the existing floating-box idiom — `centered(area, w, h)` → `Clear` →
`Block::default().borders(Borders::ALL).title(...).style(theme.dim_style())`,
filling `block.inner(rect)` (the `panel_frame` pattern in `src/ui/modal/mod.rs`).
Size roughly `centered(area, 40, 11)`, adjusted to fit content.

Static content (two-column layout to stay compact):

```
 Workspace actions
 (apply to the selected workspace)

   e   edit        t   term
   v   diff        g   lazygit
   c   chronox

   ?/Esc  close
```

Body text uses `theme.header_style()` for the title and normal/`dim_style` for
the rows, matching the other modals.

## Testing

- Unit/layout-level: assert the footer `keys` array no longer contains
  edit/term/diff/lazygit and does contain `("?", "actions")`.
- Input: `?` from the dashboard sets `app.modal = Some(Modal::WorkspaceActions)`;
  `Esc` and `?` from within the overlay clear it; an action key such as `e` while
  the overlay is open does not invoke the editor (input is captured by the modal
  gate).
- Manual smoke test in the running TUI: footer reads correctly, `?` opens the
  box, click on the `? actions` pill opens it, Esc/`?` close it.

## Commits (on this branch)

1. Add `Modal::WorkspaceActions` variant, its renderer, and the open/dismiss
   input wiring.
2. Remove the four context-only hints from the footer array and add `? actions`.

## Decisions / defaults (easily revisited)

- Footer label is `? actions` (not `? keys`/`? more`) because the overlay lists
  workspace actions only.
- Overlay uses a two-column layout for compactness; single column is acceptable.
