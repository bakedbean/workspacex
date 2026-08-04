# Workspace rename via the workspace actions modal

**Date:** 2026-08-04
**Status:** approved

## Goal

Let the user rename a workspace from the TUI. Entry point is the existing
workspace actions card (`?` on a workspace row), which gains an `r  rename`
action that opens a small in-modal text input. No bare dashboard keybinding
(`r` is already taken for the PM-digest refresh nudge when the PM pane is
visible).

## UI flow

1. `?` on a workspace row opens `Modal::WorkspaceActions` (unchanged), whose
   body gains a `r   rename` line.
2. `r` inside the card swaps the modal to a new
   `Modal::RenameWorkspace { workspace_id, name_buffer, notice }`. Unlike the
   other action keys (`e`/`t`/`v`/`g`/`c`), `r` is handled inside the
   `Modal::WorkspaceActions` arm directly — it is NOT forwarded to
   `handle_key_dashboard`, so no dashboard-level binding is added.
3. The rename modal pre-fills `name_buffer` with the current workspace name so
   the user edits rather than retypes.
4. Keys in the rename modal:
   - printable chars (no Ctrl/Alt) → push onto `name_buffer`
   - Backspace → pop
   - Esc → close modal, no side effects
   - Enter → normalize + apply (below)
5. Rendering follows the hand-rolled input pattern (`Paragraph`, block cursor
   `█` after the buffer, footer hint `[enter] rename  [esc] cancel`), with an
   optional `notice` line for inline errors, like `Modal::ProcessList`.

## Apply semantics

On Enter:

- Normalize the buffer with a new small helper (in `src/data/workspace.rs`):
  lowercase, map non-alphanumeric to `-`, collapse runs of `-`, trim leading/
  trailing `-`. Reject empty results. Deliberately NOT `slugify_prompt` — that
  helper drops stopwords and rejects names under 6 chars, which would mangle a
  deliberately typed slug.
- If the normalized name equals the current name, just close the modal
  (`workspace::rename` is already idempotent, but skip the call).
- Otherwise call the existing core
  `crate::data::workspace::rename(&app.store, &repo, &ws, &slug)` inline
  (awaited — it is one `git branch -m` plus two sqlite UPDATEs, same as the
  existing call site in the `WSX_RENAME_MODE=local` path), then
  `app.refresh()?` and close the modal.
- On error (e.g. `git branch -m` fails because the target branch already
  exists), keep the modal open and set `notice` to a one-line error message.

## Deliberately unchanged

Matches existing `wsx workspace rename` semantics:

- The worktree directory keeps its old path (path is composed only at
  creation; nothing renames it).
- A live tmux session keeps its stored `session_ref`; names are never
  re-derived after creation (established invariant — re-deriving would orphan
  live agents; see comment at `tmux_name_for`, `src/app.rs:1650`).
- The branch is recomposed as `resolve_branch_prefix(repo) + slug` by the
  core `rename` fn — no change there.

## Components touched

| Piece | Change |
|---|---|
| `src/ui/modal/mod.rs` | new `Modal::RenameWorkspace { workspace_id, name_buffer, notice }` variant + render arm (or dedicated render fn if the generic `render()` doesn't fit); add `r   rename` to the `WorkspaceActions` body text |
| `src/app/input.rs` | `Modal::WorkspaceActions` arm: handle `Char('r')` by swapping to `RenameWorkspace` (needs the selected workspace id); new `Modal::RenameWorkspace` arm with the input keys above |
| `src/data/workspace.rs` | new `normalize_slug(&str) -> Option<String>` helper |
| `src/app/render.rs` | dispatch for the new modal if it doesn't go through generic `modal::render` |

## Error handling

- No workspace selected / workspace disappeared between card and Enter:
  close the modal silently after `refresh()` (same defensive lookup pattern as
  the existing rename call site — find ws/repo by id, bail quietly if gone).
- `workspace::rename` error: shown in `notice`, modal stays open.
- Empty/invalid normalized name: `notice` = "name cannot be empty", modal
  stays open.

## Testing

- Unit tests for `normalize_slug` (mixed case, punctuation, dash collapsing,
  empty rejection, exact-slug passthrough like `wip-ci`).
- Input-handler tests in `src/app/input_tests.rs`, mirroring existing modal
  tests:
  - `?` then `r` opens the rename modal pre-filled with the current name
  - typing + Enter renames (name and branch updated in store) and closes
  - Esc cancels without changes
  - Enter with a cleared buffer shows the notice and keeps the modal open
- Existing core coverage (`rename_updates_name_and_branch`) already exercises
  branch + DB updates.
