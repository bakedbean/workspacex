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
| `o`                               | Cycle how workspaces are ordered inside a repo: by recency (default) or by status. Persisted.                |
| `q`                               | Quit (kills all running sessions)                                                                            |
| `p`                               | Toggle the project-manager digest pane (opens focused, instant, no agent session)                            |
| `Tab`                             | Swap focus between dashboard and the digest pane (when visible)                                              |
| `z z`                             | Toggle fold on the focused repo                                                                              |
| `z a`                             | Expand all repos (override default-fold heuristic)                                                           |
| `z M`                             | Fold all repos                                                                                               |
| `j` / `k` (or arrows) (when digest focused) | Move selection through digest cards                                                                |
| `Enter` (when digest focused)     | Attach to the selected workspace                                                                             |
| `q` / `p` (when digest focused)   | Close the digest (`q` only closes it while the digest is focused — dashboard-focused `q` quits wsx)          |
| `r` (when digest visible)         | Force a git/PR cache refresh                                                                                 |

### New Workspace / Confirm Archive modals

| Key                           | Action                              |
| ----------------------------- | ----------------------------------- |
| `enter`                       | Confirm                             |
| `esc`                         | Cancel                              |
| `y` / `n`                     | Confirm/cancel on ConfirmArchive    |
| Printable chars / `backspace` | Edit the name field on NewWorkspace |

### Workspace actions card (`?` on a workspace)

| Key   | Action                                                                                  |
| ----- | --------------------------------------------------------------------------------------- |
| `r`   | Rename the workspace (and its git branch)                                                |
| `C`   | Pick a name color for its dashboard row                                                  |
| `o`   | Open its setup log — see below                                                            |
| `x`   | Cancel an in-flight setup (creates only; archive is not cancellable)                     |
| `?` / `esc` | Close the card                                                                     |

Other keys (`e`, `t`, `v`, `g`, `c`, `enter`) are forwarded to the dashboard and act on the selected workspace.

### Setup log viewer (`o`)

Whenever the selected workspace carries a lifecycle badge — `⚙!` (setup failed), `⚙?` (setup cancelled), or a spinner (being created or archived) — the footer shows a `? o  setup log` hint, since the badge itself has no room to say where the reason lives.

The viewer shows the [setup script](../configuration/per-repo-setup-scripts.md)'s output for the selected workspace, whatever state it is in: the live tail while the workspace is still being created, and the persisted log from `~/.local/state/wsx/logs/` once it has finished. Stderr lines are marked `!` and highlighted in both. Only the last 256 KiB of a log is read, and at most 2000 lines are shown, so a very verbose setup script loses the start of its output — the end, which says how the run finished, is always kept.

A workspace with no log to show says which case it is rather than showing an empty pane: no setup script was ever run, the file could not be read (with the path and the error), or — while archiving — that archive output is not kept at all.

| Key                    | Action                    |
| ---------------------- | ------------------------- |
| `Up` / `Down` (`k`/`j`)| Scroll one line           |
| `PageUp` / `PageDown`  | Scroll ten lines          |
| `g` / `Home`           | Jump to the start of the log |
| `G` / `End`            | Jump back to the end      |
| `esc` / `enter`        | Close (background work keeps running) |

### Attached workspace

Keystrokes are forwarded to the running `claude` session, except:

| Key              | Action                                                                                                      |
| ---------------- | ----------------------------------------------------------------------------------------------------------- |
| `Ctrl-x d`       | Close the focused pane. When only one pane is open, detaches back to the dashboard (session keeps running). |
| `Ctrl-x Shift-D` | Save the current split layout for this workspace, then detach to the dashboard. Restored on next attach.    |
| `Ctrl-x Esc`     | Dismiss the navigation overlay without detaching (stay in the attached view).                               |
| `Ctrl-x ←/→/↑/↓` | Move focus between split panes in that direction (vim's `Ctrl-w` motions).                                  |
| `Ctrl-x u`       | Open the floating updates panel (a stripped-down dashboard in the dashboard's order; `v`/`s` open in a split, `o` / `G` cycle the dashboard's sort and grouping, `/` filters the list) |
| `Ctrl-x a`       | Open the agents panel to add/remove agents in this workspace (see [Multi-agent workspaces](../configuration/multi-agent-workspaces.md)) |
| `Ctrl-x e`       | Open the attached workspace in your editor (same `editor_cmd` as `[e]` on the dashboard)                    |
| `Ctrl-x t`       | Open the attached workspace in a terminal (same `terminal_cmd` as `[t]`)                                    |
| `Ctrl-x v`       | View diff of the attached workspace's branch vs the base branch (same `diff_cmd` as `[v]`)                  |
| `Ctrl-x k`       | Show processes running under the attached workspace's worktree                                              |
| `Ctrl-x <`       | Open the prompt-tag picker (wrap a body in an XML tag and insert it unsubmitted)                            |
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
