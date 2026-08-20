# Updates panel workspace filter — manual test

Verifies the `/` filter in the agent updates panel: that printable keys
become filter text, that the needle matches names, repos, and status
text, that the cursor tracks its workspace as the list narrows, and that
Esc clears before it closes.

## Setup

Launch wsx with at least two repos registered, several workspaces in
each, and at least one live agent session:

```
wsx
```

Attach to any workspace, then press `Ctrl-x` followed by `u`.

## Scenarios

1. **Footer advertises the filter.** Expected: the footer reads
   `[↑↓] move  [↵] switch  [v/s] split  [o] sort:default  [/] filter
   [esc] close`, on one line, not clipped at the panel's right edge.

2. **`/` arms filter mode.** Press `/`. Expected: the footer becomes
   `/    [esc] clear  [↑↓] move  [↵] switch` — the bare `/` (followed by
   the four spaces the empty needle leaves behind) is visible feedback
   that the key registered. No rows have disappeared yet.

3. **Repo name narrows to one repo.** Type a registered repo's name.
   Expected: every other repo's section disappears, header included —
   not an empty header with no rows under it. All of the named repo's
   workspaces remain.

4. **Workspace name narrows to one row.** Backspace to clear the typed
   repo name (this edits the buffer in place — the filter stays armed),
   then type part of a workspace name. Expected: only matching rows
   remain, and the name column tightens to the longest surviving name.

5. **Status text is matchable.** Backspace to clear, then type
   `no session`. Expected: only workspaces with no session remain.
   Backspace to clear again and type `permission`: with a workspace
   sitting on a permission prompt, this narrows to it.

6. **Printable keys are text, arrows still navigate.** With the filter
   from scenario 5 still active, press `j`. Expected: a `j` is appended
   to the needle and the selection does NOT move. Press `↓`. Expected:
   the selection moves and the needle is unchanged. Press Enter on a
   row. Expected: it attaches, same as without a filter.

7. **Cursor tracks its workspace.** Reopen the panel (`Ctrl-x` then
   `u`). Select a row partway down the list with `↑`/`↓` (no filter
   active yet), then press `/` and type a needle that keeps that row
   but hides rows above it. Expected: the highlight stays on the same
   workspace as it moves up the list, rather than staying on the same
   screen row.

8. **No matches.** Backspace to clear, then type a needle matching
   nothing. Expected: `(no matching workspaces)` — not `(no
   workspaces)`, which would read as "you have none at all".

9. **Two-stage Esc.** With the no-match filter from scenario 8 still
   active, press Esc. Expected: the filter is gone, the full list
   returns, and the panel stays open. Press Esc again. Expected: the
   panel closes. (A single Esc always fully exits filter mode, even if
   the buffer has text — it does not just trim one character.)

10. **Reopen starts clean.** Press `Ctrl-x` then `u` again. Expected: no
    filter is active and every workspace is listed.

11. **Dashboard echo.** Return to the dashboard and press `/`, then type.
    Expected: the needle appears next to the `group:` tabs in the top
    bar, so the vanishing rows have a visible cause. Esc clears it.
