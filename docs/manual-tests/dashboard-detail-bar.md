# Dashboard workspace detail bar — manual test

Verifies the detail bar appears for workspace selections, collapses
for repo selections, stacks with the Project Manager pane, and
accepts an inline reply that lands in the selected workspace's PTY.

## Setup

Launch wsx with at least one repo registered and at least one running
claude session in a workspace:

```
wsx
```

## Scenarios

1. **Bar shows on workspace selection.** Move selection (↑/↓) onto a
   workspace row. Expected: the bottom ~22% of the terminal becomes
   the detail bar (header strip with name/branch/lifecycle/diff/procs/
   status; three columns SESSION SUMMARY / RECENT CHAT / PROCESSES;
   reply chip at the bottom). The workspace list above keeps the
   selection visible.

2. **Bar hides on repo selection.** Move selection onto a repo header
   row. Expected: the bar disappears; the list reclaims the freed
   space. The keybind footer stays at the bottom.

3. **Reply input via Tab.** With a workspace selected, press Tab.
   Expected: the cursor appears in the `┃ Reply to agent ┃` input
   field. Type `ping`. Press Enter. Expected: the field clears, focus
   returns to the dashboard list, and (when you attach into the
   workspace via Enter) the `ping` message appears as a user prompt
   in the session.

4. **Esc cancels the draft.** Tab into the input, type a few
   characters, press Esc. Expected: the field clears and focus
   returns to the dashboard list without sending anything.

5. **Arrow nav yields focus.** Tab into the input, type a few
   characters, press ↓. Expected: the draft is discarded, the
   selection moves to the next item, and the bar updates (or hides,
   if the move landed on a repo header).

6. **PM coexistence.** With a workspace selected, toggle the Project
   Manager pane (existing keybind). Expected: the screen stacks
   list → detail bar → PM → footer, with all three regions visible.
   Toggle PM off. Expected: bar moves back to the bottom (above
   footer); list reclaims the PM area.

7. **Narrow terminal.** Resize the terminal width below 80 columns
   with a workspace selected. Expected: the body collapses to a
   single column (SESSION SUMMARY only). Header strip and reply row
   remain.

8. **Short terminal.** Resize the terminal height below 18 rows with
   a workspace selected. Expected: the bar is suppressed; only the
   list and footer render.
