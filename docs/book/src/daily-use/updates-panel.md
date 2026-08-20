When you're attached to a workspace (full-screen claude session), wsx
still tracks the other workspaces in the background. Two affordances
surface that:

- A single-row status indicator above the footer, shown only when another
  workspace needs attention or has produced output in the last 60 seconds.
  Format: `⚠ <name> awaiting permission: <tool> (<age>)` for attention,
  `● <name>: <event> (<age>)` for activity. The row collapses to nothing
  when there's nothing to surface, giving claude the row back.

- A floating panel via `Ctrl-x u` listing ALL workspaces grouped by repo,
  with their current state and latest event. Press `Esc` to close — with a
  filter active, `Esc` clears the filter first and closes on the second
  press. The panel re-renders live, so ages count up and attention flags
  appear/clear in real time.

  From the panel:

  | Key                          | Action                                                                                 |
  | ---------------------------- | -------------------------------------------------------------------------------------- |
  | `Up` / `Down` (or `k` / `j`) | Move selection within the panel.                                                       |
  | `Enter`                      | Switch the current pane to the selected workspace (replaces it).                       |
  | `v`                          | Open the selected workspace in a vertical split (panes side by side, vim's `:vsplit`). |
  | `s`                          | Open the selected workspace in a horizontal split (panes stacked, vim's `:split`).     |
  | `o`                          | Cycle the sort order: default → status urgency → PR status.                            |
  | `/`                          | Filter the list; type to narrow it, `Esc` to clear.                                    |

  The filter matches the workspace name, its repo's name, and the row's
  status text (the same text the row shows, case-insensitively), and repo
  headers with no surviving workspaces disappear along with their rows.
  While a filter is active, printable keys are filter text rather than
  shortcuts — so the arrow keys and `Enter` are how you navigate and attach
  mid-search.
