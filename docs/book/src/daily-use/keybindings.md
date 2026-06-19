### Dashboard

| Key                               | Action                                                                                                       |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| `Up` / `Down` (or `k` / `j`)      | Move selection through repo headers and workspaces                                                           |
| `h` / `l`                         | Fold / unfold the focused repo (idempotent; use `zz` to toggle)                                              |
| `enter` (or `i`) on a workspace   | Attach to its claude session (spawns or resumes)                                                             |
| `enter` (or `i`) on a repo header | Open the New Workspace modal targeting that repo                                                             |
| `n`                               | New workspace in the selected row's repo                                                                     |
| `Shift + N`                       | New workspace in permissive mode (claude launches with `--dangerously-skip-permissions`)                     |
| `e`                               | Open the selected workspace in your editor (no-op on repo header)                                            |
| `t`                               | Open the selected workspace in a terminal (no-op on repo header)                                             |
| `v`                               | View diff of the selected workspace's branch vs the repo's base branch (auto-detected; no-op on repo header) |
| `Shift + K`                       | On a workspace: show processes under its worktree. On a repo header: move the repo up one slot (persisted)   |
| `Shift + J`                       | On a repo header: move the repo down one slot (persisted). No-op on a workspace                              |
| `s`                               | Open repo settings modal for the selected repo (or the parent repo when a workspace is selected)             |
| `d`                               | Archive the selected workspace (no-op on repo header)                                                        |
| `q`                               | Quit (kills all running sessions)                                                                            |
| `p`                               | Toggle the Project Manager pane (no-op when `pm_enabled` is off)                                             |
| `Tab`                             | Swap focus between dashboard and the PM pane (when visible)                                                  |
| `z z`                             | Toggle fold on the focused repo                                                                              |
| `z a`                             | Expand all repos (override default-fold heuristic)                                                           |
| `z M`                             | Fold all repos                                                                                               |
| `r` (when PM focused)             | Refresh `workspaces.json` and ask PM to re-summarize                                                         |
| `Ctrl-O` (when PM focused)        | Expand PM to full screen (use `Ctrl-x d` to detach back)                                                     |

### New Workspace / Confirm Archive / Setup Running modals

| Key                           | Action                              |
| ----------------------------- | ----------------------------------- |
| `enter`                       | Confirm                             |
| `esc`                         | Cancel                              |
| `y` / `n`                     | Confirm/cancel on ConfirmArchive    |
| Printable chars / `backspace` | Edit the name field on NewWorkspace |

### Attached workspace

Keystrokes are forwarded to the running `claude` session, except:

| Key              | Action                                                                                                      |
| ---------------- | ----------------------------------------------------------------------------------------------------------- |
| `Ctrl-x d`       | Close the focused pane. When only one pane is open, detaches back to the dashboard (session keeps running). |
| `Ctrl-x Shift-D` | Save the current split layout for this workspace, then detach to the dashboard. Restored on next attach.    |
| `Ctrl-x Esc`     | Dismiss the navigation overlay without detaching (stay in the attached view).                               |
| `Ctrl-x ←/→/↑/↓` | Move focus between split panes in that direction (vim's `Ctrl-w` motions).                                  |
| `Ctrl-x u`       | Open the floating updates panel (shows other workspaces' state; supports `v`/`s` to open in a split)        |
| `Ctrl-x a`       | Open the agents panel to add/remove agents in this workspace (see [Multi-agent workspaces](../configuration/multi-agent-workspaces.md)) |
| `Ctrl-x e`       | Open the attached workspace in your editor (same `editor_cmd` as `[e]` on the dashboard)                    |
| `Ctrl-x t`       | Open the attached workspace in a terminal (same `terminal_cmd` as `[t]`)                                    |
| `Ctrl-x v`       | View diff of the attached workspace's branch vs the base branch (same `diff_cmd` as `[v]`)                  |
| `Ctrl-x k`       | Show processes running under the attached workspace's worktree                                              |
| `Ctrl-x x`       | Send a literal `Ctrl-x` to claude                                                                           |
| `Ctrl-x c`       | Toggle the change chronology bar on/off                                                                     |
| `Ctrl-x C`       | Swap the chronology bar's side (left ↔ right)                                                               |
| `Ctrl-x →` (bar on right) / `Ctrl-x ←` (bar on left) | Move keyboard focus into the chronology bar (from the adjacent edge pane only) |
| `Ctrl-x ←` (bar on right) / `Ctrl-x →` (bar on left) | Return focus from the bar to the agent pane                                    |
| `↑` / `k` *(bar focused)* | Move selection up (toward newer entries)                                              |
| `↓` / `j` *(bar focused)* | Move selection down (toward older entries)                                            |
| `g` *(bar focused)*        | Jump to the top (newest entry)                                                        |
| `G` *(bar focused)*        | Jump to the bottom (oldest entry)                                                     |
| `Enter` *(bar focused)*    | Open the full-change detail modal for the selected entry                              |
| `Esc` *(bar focused)*      | Return focus to the agent pane                                                        |

When a workspace has more than one agent, the footer also binds bare keys `q w r y i o p s h j` (no leader) to switch the focused pane between agents — see [Multi-agent workspaces](../configuration/multi-agent-workspaces.md).
