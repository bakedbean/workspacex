# Remove built-in chronology, launch chronox externally

**Date:** 2026-06-10
**Status:** Approved

## Problem

wsx embeds a chronox-like "chronology" timeline view inside the agent (attached)
chat. It is toggled with `Ctrl-x c` (and its side swapped with `Ctrl-x C`).
Now that the standalone chronox TUI has matured, this in-app reimplementation is
redundant. We want to remove it entirely and instead launch the external chronox
binary for a workspace via a keybind, exactly the way wsx already launches an
editor, terminal, diff tool, and lazygit.

## Goals

1. Remove the built-in chronology feature and all of its wiring.
2. Add a `chronox` launcher modeled on the existing `lazygit` launcher.
3. Keep behavior consistent with `edit`/`diff`/`term`/`lazygit`: bare key on the
   dashboard, leader-prefixed key in the attached view, errors surfaced via
   `Modal::Error`.

## Non-goals

- No destructive store migration. Existing `chronology_config` rows are left
  orphaned and harmless.
- No changes to the `sessionx` activity-parsing crate or its use elsewhere.

## Key facts established during exploration

- **chronox CLI:** `chronox [worktree]` — takes the worktree path as its first
  positional argument, defaulting to `cwd`. It is a full-screen TUI (raw mode +
  alternate screen + mouse capture), so it must run in its own window, just like
  lazygit. (Confirmed against `/home/eben/chronox` `src/main.rs`.)
- **Launch mechanism (decision):** window wrapper, like lazygit. A configurable
  `chronox_cmd` is spawned detached; the user supplies a wrapper that opens its
  own window (e.g. `wezterm start -- chronox {path}`).
- **Keybind (decision):** reuse `c` — bare `c` on the dashboard, `Ctrl-x c` in
  the attached view (the slot vacated by the removed toggle). `c`/`C` are
  currently bound *only* in the attached leader section, so the dashboard `c` is
  free.
- **`sessionx` stays.** It is the JSONL activity parser used by
  `src/activity/mod.rs`, `src/error.rs` (`From<sessionx::Error>`), and
  `src/pty/session.rs` — independent of chronology. Only the `crate::chronology`
  module (which re-exports some sessionx types for the timeline UI) is removed.

## Part 1 — Remove the built-in chronology feature

Delete the feature and every reference to `crate::chronology`:

- **`src/chronology/`** — delete the entire module (`mod.rs`, `render.rs`, nav,
  etc.).
- **`src/config/chronology_source.rs`** — delete; remove its wiring from
  `src/config/mod.rs`.
- **`src/app.rs`** — drop all `chronology_*` fields (timeline map, scroll, focus,
  selection, hit-test rects, refresh throttle, last-workspace sentinel,
  visible-entry count), the `refresh_chronology()` method, the
  `change_detail_view` field, and `RepoSettingField::ChronologyConfig`.
- **`src/app/input.rs`** — remove the `Ctrl-x c` / `Ctrl-x C` handlers and the
  helper fns: `toggle_chronology_visible`, `swap_chronology_side`,
  `focused_chronology_side`, `open_change_modal`, `set_change_detail_scroll`,
  `toggle_change_detail_view`, `open_change_in_editor`; the chronology keyboard
  nav handler; the mouse-wheel scroll and click-to-open handlers for the bar;
  and the arrow-key movement between the bar and the panes.
- **`src/app/render.rs`** — remove chronology refresh, bar layout/draw,
  `render_change_detail_modal`, and the `side_cell_to_line` helper.
- **`src/ui/attached.rs`** — remove `ChronologyDraw`, `ChronologyHits`,
  `split_for_chronology`, `render_chronology_bar`, and the `Side` import; the
  attached pane area no longer reserves space for a bar.
- **`src/ui/modal.rs`** — remove the `Modal::ChangeDetail` variant and
  `DiffViewMode`.
- **`src/cli.rs`** — remove the `chronology_config` setting key and the
  `ChronologyConfig` references.
- **`src/ui/footer.rs`** — remove the chronology footer legend entry.
- **Settings struct / persistence** — drop the `chronology_config` field from the
  repo/workspace settings record (referenced e.g. in
  `src/ui/dashboard/tests.rs:26`). No migration; old rows are inert.
- **Tests** — remove the `change_detail_*` test modules in
  `src/app/input_tests.rs` and any chronology-specific assertions elsewhere.

Done-criteria: no remaining references to `crate::chronology`,
`chronology_config`, `ChangeDetail`, or `DiffViewMode`; `cargo build` and
`cargo test` are green.

## Part 2 — Add the chronox launcher

- **`src/commands/external.rs`** — add:
  ```rust
  pub fn open_in_chronox(worktree: &Path, configured: Option<&str>) -> Result<()> {
      let cmd = resolve_chronox_cmd(configured)?;
      // {path} substitution with append-fallback, like open_diff/spawn_with_path_arg,
      // so `wezterm start -- chronox {path}` and a bare `chronox` both work.
      spawn_with_path_arg(&cmd, worktree)
  }

  fn resolve_chronox_cmd(configured: Option<&str>) -> Result<String> {
      // configured (non-empty) wins; otherwise error with guidance:
      // "no chronox command configured; set `wsx config set chronox_cmd <cmd>`
      //  (e.g. `wezterm start -- chronox`) — wsx's own TUI owns the terminal,
      //  so chronox needs a wrapper that opens its own window"
  }
  ```
  Add a unit test for `resolve_chronox_cmd` / argv resolution (with and without a
  `{path}` placeholder) next to the existing `resolve_argv` tests.
- **`src/cli.rs`** — add `"chronox_cmd"` to `known_setting_key()`.
- **`src/app/input.rs`**:
  - Dashboard: bare `c` → resolve the selected workspace's `worktree_path`, call
    `open_in_chronox`, surface errors via `Modal::Error` (same shape as
    `e`/`t`/`v`/`g`).
  - Attached view: `Ctrl-x c` → same, for the attached workspace.
- **`src/ui/footer.rs`** — add a `c` (dashboard) / `^x c` (attached) legend entry
  for chronox.

## Testing

- `cargo build` clean after removal — no dangling `crate::chronology` references.
- `cargo test` green after removing chronology tests.
- New unit test covering `resolve_chronox_cmd` argv resolution.
- Manual smoke (`/verify`): set `chronox_cmd`, press `c` on the dashboard and
  `^x c` in the attached view, confirm chronox opens on the correct worktree.

## Commit plan

1. `feat: remove built-in chronology timeline view`
2. `feat: launch external chronox TUI via keybind`
