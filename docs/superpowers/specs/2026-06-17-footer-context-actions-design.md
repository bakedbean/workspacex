# Dashboard footer: move workspace-only actions into a `?` overlay

**Date:** 2026-06-17
**Branch:** `bakedbean/footer-context-actions`
**Status:** Implemented (PR #193)

> **Updated post-implementation.** This doc has been reconciled with the shipped
> behavior. Three refinements came out of testing after the original design was
> approved: the overlay is **navigable** (it drives the dashboard selection and
> dispatches the action keys rather than swallowing them), `?` only opens **when
> a workspace is selected**, and the `? actions` footer pill is **hidden** (not
> just inert) when no workspace is selected. The sections below reflect what was
> built; see PR #193 for the commit history.

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
- The overlay's **content** is a static reference list (it does not re-render
  per selection). It is, however, **navigable**: arrow keys move the underlying
  dashboard selection while the card stays open. "Static" refers to the rendered
  text, not to interactivity.
- No general/global help screen — the overlay lists workspace actions only.

## Design

### 1. Footer (`src/ui/dashboard/layout.rs:98-109`)

Remove the four entries `("e", "edit")`, `("t", "term")`, `("v", "diff")`,
`("g", "lazygit")` from the `keys` array. Add `("?", "actions")` immediately
before `("q", "quit")` — **but only when a workspace is selected**. The footer
renderer is threaded a `workspace_selected: bool`
(`footer()` → `render_footer()` → the `render.rs` call site, computed as
`matches!(app.selected_target(), Some(SelectionTarget::Workspace(_)))`) and the
`? actions` entry is conditionally pushed. When a repo header or nothing is
selected the pill is **omitted entirely** (not shown-but-inert). Footer with a
workspace selected:

```
↑↓ nav   ↵ open   n new   G group   / filter   ? actions   q quit
```

Footer with no workspace selected (repo header / empty):

```
↑↓ nav   ↵ open   n new   G group   / filter   q quit
```

The clickable-pill machinery is unchanged. The footer loop maps each glyph to a
synthesized key via `key_for_glyph` (`src/ui/footer.rs:41-58`); `?` maps to the
`?` key event, so clicking the `? actions` pill opens the overlay exactly as
typing `?` does. The four removed pills lose their hint **and** their clickable
`FooterHintSpan` together — no orphaned click region remains. Because the pill
is omitted when no workspace is selected, there is no clickable region for it
in that state either.

### 2. New modal variant (`src/ui/modal/mod.rs:42`)

Add a dataless variant `Modal::WorkspaceActions` to the `Modal` enum. This reuses
the existing modal system, which is the right tier: when `app.modal.is_some()`
the dispatch gate at `src/app/input.rs:2051` routes **all** keys to
`handle_key_modal`. (The `/` filter and `G` group are inline toggles that only
partially capture input — wrong tier for this.)

Rather than swallow every non-dismiss key, the `WorkspaceActions` arm makes the
card **navigable** by selectively forwarding keys back to `handle_key_dashboard`
(a safe one-level call — the dashboard handler never calls back into the modal
handler). See §3 for the exact key routing.

### 3. Open / dismiss (`src/app/input.rs`)

- **Open (gated):** in `handle_key_dashboard`, add an arm that opens the overlay
  **only when a workspace is selected**:
  ```rust
  (KeyCode::Char('?'), _) => {
      if matches!(app.selected_target(), Some(SelectionTarget::Workspace(_))) {
          app.modal = Some(Modal::WorkspaceActions);
      }
  }
  ```
  On a repo header or empty selection, `?` is a no-op. (This pairs with the
  footer pill being hidden in that state — there is nothing to open.)
- **Key routing while open** — the `Modal::WorkspaceActions` arm in
  `handle_key_modal`:
  - `Esc` / `?` → close the overlay (`app.modal = None`), nothing else.
  - `Up` / `Down` / `j` / `k` → forward to `handle_key_dashboard` to move the
    dashboard selection, **keeping the card open** (a "navigable card").
  - `e` / `t` / `v` / `g` / `c` / `Enter` → close the overlay, then forward to
    `handle_key_dashboard` so the action fires against the current selection.
  - Any other key → inert (swallowed), card stays open.

### 4. Render (generic text-modal path in `src/ui/modal/mod.rs`)

No `src/app/render.rs` change is needed. `WorkspaceActions` is **not** added to
the early-return guard in `modal::render`, so the dispatch catch-all
(`other => modal::render(...)`) routes it to the existing generic text-modal
renderer, which draws it in the shared `centered(area, 60, 14)` → `Clear` →
bordered `Block` box styled with `theme.header_style()`. The
`WorkspaceActions` arm in `render()` just returns a `(title, body)` pair. (The
earlier `panel_frame`/dedicated-`render.rs`-arm approach was dropped in favor of
this simpler path — fewer touch points, consistent with the other small text
modals.)

Static content (`(title, body)`):

```
 workspace actions

 These apply to the selected workspace:

   e   edit        t   term
   v   diff        g   lazygit
   c   chronox

   ?/Esc  close
```

## Testing

- Unit/layout-level: with a workspace selected the footer contains
  `("?", "actions")` and not edit/term/diff/lazygit; with no workspace selected
  the `? actions` pill is omitted (hint count drops accordingly).
- Render: rendering `Modal::WorkspaceActions` produces a box listing all five
  action labels (edit/term/diff/lazygit/chronox).
- Input — gating: `?` opens the overlay only when a workspace is selected; it is
  a no-op on a repo header or empty selection.
- Input — navigable card: from within the overlay, `Esc`/`?` close it; a nav key
  (`Down`) keeps the card open; an action key (e.g. `c`) and `Enter` close the
  card (forwarding to the dashboard handler, which acts on the selection).
- Manual smoke test in the running TUI: footer shows `? actions` only with a
  workspace selected; `?` opens the box; clicking the pill opens it; arrows
  navigate with the card open; `e/t/v/g/c`/Enter act + close; Esc/`?` close.

## Commits (on this branch)

As shipped in PR #193 (feedback-driven refinements landed as their own commits):

1. Add `Modal::WorkspaceActions` variant + renderer.
2. Open the overlay with `?`.
3. Remove the four context-only hints from the footer array, add `? actions`.
4. Render-content test asserting the overlay lists all five actions.
5. Make the overlay a navigable card.
6. Gate `?` to open only when a workspace is selected.
7. Hide the `? actions` footer pill when no workspace is selected.

## Decisions / defaults (easily revisited)

- Footer label is `? actions` (not `? keys`/`? more`) because the overlay lists
  workspace actions only.
- Overlay uses a two-column layout for compactness; single column is acceptable.
