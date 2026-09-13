When you're attached to a workspace (full-screen claude session), wsx
still tracks the other workspaces in the background. Two affordances
surface that:

- A single-row status indicator above the footer, shown only when another
  workspace needs attention or has produced output in the last 60 seconds.
  Format: `⚠ <name> awaiting permission: <tool> (<age>)` for attention,
  `● <name>: <event> (<age>)` for activity. The row collapses to nothing
  when there's nothing to surface, giving claude the row back.

- A floating panel via `Ctrl-x u` listing ALL workspaces — a stripped-down
  dashboard. Rows come in the dashboard's own order: grouped by repo or by
  attention (NEEDS ATTENTION / WORKING / RECENT / IDLE, with `repo/name`
  rows), sorted within a group by recency or status, whichever the
  dashboard is set to. Each row shows the workspace's current state and
  latest event, plus the same PR chip (`⏺ #123 open ✓`) and `+N −N` line
  diff the dashboard row shows. Unlike the dashboard, nothing is folded or
  collapsed: every workspace is listed, and empty repos are skipped. Press
  `Esc` to close — with a filter active, `Esc` clears the filter first and
  closes on the second press. The panel re-renders live, so ages count up
  and attention flags appear/clear in real time.

  From the panel:

  | Key                          | Action                                                                                 |
  | ---------------------------- | -------------------------------------------------------------------------------------- |
  | `Up` / `Down` (or `k` / `j`) | Move selection within the panel; wraps from either end to the other.                   |
  | `Enter`                      | Switch the current pane to the selected workspace (replaces it).                       |
  | `v`                          | Open the selected workspace in a vertical split (panes side by side, vim's `:vsplit`). |
  | `s`                          | Open the selected workspace in a horizontal split (panes stacked, vim's `:split`).     |
  | `o`                          | Cycle the dashboard's sort mode (recency ↔ status); persisted, like the dashboard's `o`. |
  | `G`                          | Toggle the dashboard's grouping (by repo ↔ by attention).                              |
  | `/`                          | Filter the list; type to narrow it, `Esc` to clear.                                    |

  The filter matches the workspace name, its repo's name, and the row's
  status text (the same text the row shows, case-insensitively), and repo
  headers with no surviving workspaces disappear along with their rows.
  While a filter is active, printable keys are filter text rather than
  shortcuts — so the arrow keys and `Enter` are how you navigate and attach
  mid-search.
