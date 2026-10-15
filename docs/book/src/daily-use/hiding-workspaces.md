# Hiding workspaces

Long-lived workspaces can crowd a repo's section once their PRs merge or if
they never get one. Each repo can hide workspaces by PR state.

Press `F` on a repo header (or any of its rows) and toggle states with
`space`: no PR, draft, open, conflicted, merged, closed. The choice is saved
per repo.

"No PR" only hides workspaces that look abandoned. A workspace without a PR
stays visible if any of these is true:

- it has commits ahead of its base branch, or uncommitted changes;
- an agent session is running, or its agent reports working, waiting, or
  blocked;
- it was active in the last 3 days.

The other states hide every matching workspace.

Hidden workspaces collapse into one dimmed row per state under their repo:

    ▸ ⏺ merged  ·  4 hidden

Press `enter` or `l` on that row to show its workspaces inline; `h` collapses
it again. Expansion lasts until wsx restarts.

A few things stay predictable:

- Typing a `/` filter searches every workspace, hidden or not.
- A workspace whose PR state hasn't loaded yet is never hidden.
- If a selected workspace becomes hidden (say its PR merges), the selection
  moves to its group's row.
- Status bar, waybar, and menubar counts still include hidden workspaces.
- The updates panel (`Ctrl-x u`) leaves hidden workspaces out.
- The by-attention view (`G`) lists hidden groups in a HIDDEN section.
