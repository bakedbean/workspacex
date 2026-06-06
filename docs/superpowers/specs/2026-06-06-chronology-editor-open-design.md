# Chronology Editor Open (config-driven) — Design

**Date:** 2026-06-06
**Status:** Approved for planning
**Builds on:** the Change Chronology bar + keyboard navigation (open-at-line action).

## Problem

The chronology "open this change in my editor at the modified line" action silently does nothing for the common case. Diagnosis: `editor_cmd` is unset, so the open path falls back to `$EDITOR` (here `nvim` — a terminal editor) and spawns it **detached** with null stdio. Inside wsx's full-screen TUI (which owns the controlling terminal), a terminal editor has no tty to draw on, so it exits immediately and nothing appears. The failure is swallowed (`tracing::warn!`), so it reads as "nothing happens."

A secondary defect: even with a windowing wrapper configured (e.g. `alacritty -e nvim`), the line number is dropped, because the arg-builder only inspects the **first** command token to detect the editor — it sees `alacritty`, not `nvim`.

## Goal

Make the chronology open-at-line behave predictably and put editor behavior in the user's hands (consistent with wsx's design ethos), without wsx taking over the terminal:

- **No `editor_cmd` configured →** don't launch; show a dismissible warning telling the user to configure it (with an example). No silent `$EDITOR` fallback for this path.
- **`editor_cmd` configured →** evaluate it and inject the file + line at runtime, then execute it (detached, as today).
- A single `editor_cmd` keeps working for both the dir-open (`e` / `Ctrl-x e`) and this open-at-line path — no separate setting.
- Every previously-silent failure becomes visible.

## Scope

- **In scope:** the chronology open-at-line action only — keyboard `Enter` on a selected detail, and mouse click on an expanded detail.
- **Out of scope:** `e` (dashboard) and `Ctrl-x e` (attached) "open workspace in editor" keep their current behavior, including their `$VISUAL`/`$EDITOR` fallback. (Decided during brainstorming.)
- **Rejected alternative:** "suspend-and-run" (wsx leaves raw mode / alt screen, runs a terminal editor with inherited stdio, then redraws). Heavier (touches the event loop + terminal-mode guard) and against wsx's delegate-to-the-user philosophy. Recorded for posterity; not pursued.

## Behavior

When the user triggers open-at-line on entry `i`:

1. Resolve the focused workspace's worktree and the `ChangeEvent` at `i` (file path + detail), cloning owned values.
2. Read the **`editor_cmd`** setting via `store.get_setting("editor_cmd")` — do **not** consult `$VISUAL`/`$EDITOR` for this path.
3. **Unset or whitespace-only →** set `app.modal = Some(Modal::Error { message })` and return. Message:

   > No `editor_cmd` configured. Set one to open changes in your editor, e.g.
   > `wsx config set editor_cmd 'alacritty -e nvim'`

4. **Set →** compute the changed line via `resolve_line_in_file(file, detail)`, build argv via the upgraded `resolve_editor_at_argv`, and spawn detached via the existing `open_in_editor_at` path. On `Err`, set `app.modal = Some(Modal::Error { message: format!("Failed to open editor: {e}") })`.

The `Modal::Error` block renders after the view match (`src/app/render.rs`), so it is visible over the **attached** view and is dismissible by the existing `Modal::Error` input handling.

## Runtime file+line injection (`resolve_editor_at_argv` upgrade)

`resolve_editor_at_argv(cmd: &str, file: &str, line: u32) -> Result<Vec<String>>` resolves in this order:

1. **Placeholders:** if any token contains `{file}` or `{line}`, substitute both across all tokens and return. (Unchanged; explicit escape hatch for editors not in the known set.)
2. **Scan for a known editor:** find the first token whose file-stem basename matches a known editor, and append that editor's goto syntax to the **end** of the argv:
   - `code` | `codium` | `cursor` | `zed` → append `--goto` then `{file}:{line}`
   - `vim` | `nvim` | `vi` | `nano` | `emacs` | `emacsclient` → append `+{line}` then `{file}`
3. **Fallback:** no known editor token found → append `{file}` only (line dropped). The user can add `{file}`/`{line}` placeholders for an unrecognized editor.

Appending at the end is correct for terminal wrappers because they pass trailing args to the inner command: `alacritty -e nvim` + `+42 /path` → `alacritty -e nvim +42 /path` (nvim opens at line 42 in a new window); `wezterm start -- code` + `--goto /path:42` → runs `code --goto /path:42`. A bare editor (`nvim`, `code`) is just the degenerate single-token case and keeps working.

The change from today: scan **all** tokens (today only the first token / program is inspected), so window-wrapper commands detect the inner editor and preserve the line.

## Components / files

- **`src/commands/external.rs`**
  - Extract `fn known_editor_goto(basename: &str) -> Option<GotoStyle>` where `enum GotoStyle { Goto, PlusLine }` (or equivalent) encoding the two append shapes.
  - Rewrite `resolve_editor_at_argv` to: (a) honor `{file}`/`{line}` placeholders; (b) else scan tokens for the first `known_editor_goto` match and append accordingly; (c) else append the file.
  - `open_in_editor_at` is unchanged (still detached). It is only called with a non-empty configured command, so its internal `resolve_editor_cmd(Some(cmd))` returns `cmd` without `$EDITOR` fallback.
  - Add a small pure decision helper for the call site:
    `pub fn editor_open_decision(editor_cmd: Option<&str>) -> EditorOpenDecision` returning `Launch(String)` for a non-empty trimmed command or `NeedsConfig` otherwise.

- **`src/app/input.rs`**
  - Factor the two chronology open sites (keyboard `NavAction::Open(i)` and mouse expanded-detail click) into one helper, e.g. `fn open_focused_change(app: &mut App, idx: usize)`, that: resolves focused workspace + event (cloning owned `worktree`/`file`/`detail`), reads `editor_cmd`, matches `editor_open_decision`:
    - `NeedsConfig` → `app.modal = Some(Modal::Error { message: <configure message> })`.
    - `Launch(cmd)` → `resolve_line_in_file` + `open_in_editor_at(&worktree, &file, line, Some(&cmd))`; on `Err` → `app.modal = Some(Modal::Error { message: <failure message> })`.
  - Both open sites call this helper (removes today's duplicated open block and the silent `tracing::warn!`).

## Error handling / edge cases

- Unset/whitespace `editor_cmd` → visible `Modal::Error` with an example (not a silent no-op).
- Spawn failure (bad command, missing binary) → visible `Modal::Error` with the error.
- File deleted/renamed since the edit → `resolve_line_in_file` already returns line 1; the launch proceeds with line 1.
- Unrecognized editor with no placeholders → opens the file without a line (documented); not an error.
- `Modal::Error` set from the attached view renders (render.rs modal block is after the view match) and is dismissible (existing handler).

## Testing

- **`resolve_editor_at_argv`** (pure, table-driven):
  - placeholder override: `alacritty -e nvim +{line} {file}` → `[alacritty, -e, nvim, +9, /f]`.
  - wrapper + terminal editor: `alacritty -e nvim` + (f, 42) → `[alacritty, -e, nvim, +42, /f]`.
  - wrapper + GUI editor: `wezterm start -- code` + (f, 7) → `[wezterm, start, --, code, --goto, /f:7]`.
  - bare editors still work: `nvim` → `[nvim, +42, /f]`; `code` → `[code, --goto, /f:42]`.
  - unknown editor: `foo` → `[foo, /f]` (line dropped).
- **`editor_open_decision`** (pure): `None`/`Some("")`/`Some("  ")` → `NeedsConfig`; `Some("nvim")` → `Launch("nvim")`.
- The input-level modal glue is thin over the two tested helpers; verified by build + manual (open with no config → warning modal; configure `alacritty -e nvim` → opens at line).

## Files touched

- `src/commands/external.rs` — `known_editor_goto`, `GotoStyle`, `resolve_editor_at_argv` rewrite, `editor_open_decision` + tests.
- `src/app/input.rs` — `open_focused_change` helper; keyboard + mouse open sites call it; remove silent warn.
- `README.md` — document that the chronology open requires `editor_cmd`, how file+line is injected (placeholders or auto-detected editor), and the `alacritty -e nvim` example.
