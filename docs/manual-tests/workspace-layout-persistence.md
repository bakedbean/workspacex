# Manual tests — workspace layout persistence

Layouts are persisted in SQLite under the `workspace_layouts` table.
These checks confirm the save / restore / prune flow end-to-end with a
real PTY.

## Test 1 — basic park & restore

1. Open wsx. Enter workspace A from the dashboard.
2. Split vertically into workspace B (use the updates panel `v` or any
   existing split entry point).
3. Press `Ctrl-x Esc` to park the layout.
4. Confirm you are back on the dashboard and that workspace A's row
   shows the `nf-fa-columns` glyph (at the start of the branch
   column, immediately before the branch glyph; only visible with
   nerd fonts).
5. Press `Enter` on workspace A.
6. Expect: the (A | B) layout restores with both PTYs live.

## Test 2 — restore across wsx restart

1. From Test 1, press `Ctrl-x Esc` again.
2. Quit wsx (`q`).
3. Restart wsx.
4. Enter workspace A.
5. Expect: same (A | B) layout restored. Sessions are fresh (claude
   restarts) but the split shape is identical.

## Test 3 — pruning a side pane

1. Park a (A | B) layout under anchor A.
2. From a separate terminal, archive workspace B with
   `wsx workspace archive <repo> <B-slug>`.
3. Wait for wsx to pick up the external change (~1s — `data_version`
   poll).
4. Enter workspace A from the dashboard.
5. Expect: single-pane view of A. The side pane was pruned.

## Test 4 — anchor cascade

1. Park any layout under anchor A.
2. Archive workspace A.
3. Inspect the DB:
   `sqlite3 ~/.local/state/wsx/wsx.db 'SELECT * FROM workspace_layouts'`
4. Expect: no row for the archived workspace (CASCADE handled it).
