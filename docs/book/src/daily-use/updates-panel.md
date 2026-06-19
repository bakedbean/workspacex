When you're attached to a workspace (full-screen claude session) or the
project manager pane is expanded full-screen, wsx still tracks the other
workspaces in the background. Two affordances surface that:

- A single-row status indicator above the footer, shown only when another
  workspace needs attention or has produced output in the last 60 seconds.
  Format: `⚠ <name> awaiting permission: <tool> (<age>)` for attention,
  `● <name>: <event> (<age>)` for activity. The row collapses to nothing
  when there's nothing to surface, giving claude the row back.

- A floating panel via `Ctrl-x u` listing ALL workspaces grouped by repo,
  with their current state and latest event. Press `Esc` to close. The
  panel re-renders live, so ages count up and attention flags appear/clear
  in real time.

  From the panel, the selected workspace can be opened three ways:

  | Key                          | Action                                                                                 |
  | ---------------------------- | -------------------------------------------------------------------------------------- |
  | `Up` / `Down` (or `k` / `j`) | Move selection within the panel.                                                       |
  | `Enter`                      | Switch the current pane to the selected workspace (replaces it).                       |
  | `v`                          | Open the selected workspace in a vertical split (panes side by side, vim's `:vsplit`). |
  | `s`                          | Open the selected workspace in a horizontal split (panes stacked, vim's `:split`).     |
