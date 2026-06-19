Multiple workspace PTYs can be tiled in the attached view, vim-style. Any
pane can be split again — recursively — into a tree of vertical and
horizontal splits. Each pane shows a 1-line title bar with the workspace
name and a `●` marker on the focused pane (which receives keystrokes).

The flow:

1. Attach to a workspace as usual (`Enter` on the dashboard).
2. Press `Ctrl-x u` to open the updates panel.
3. Move to another workspace; press `v` (vertical) or `s` (horizontal) to
   add it as a new pane alongside the current one. Focus jumps to the
   new pane.
4. Navigate between panes with `Ctrl-x ←/→/↑/↓` — direction-aware
   walking up the split tree, like vim's `Ctrl-w` motions.
5. Close the focused pane with `Ctrl-x d`. The other panes keep
   running; when the last pane closes you detach back to the dashboard.

When you split the _focused_ pane again in the same direction as its
parent, the new pane is inserted as a sibling instead of nesting deeper —
matches vim and keeps the tree shallow.

**Saving a layout.** `Ctrl-x d` detaches without remembering how the panes
were arranged. To keep the arrangement, press `Ctrl-x Shift-D` instead: wsx
saves the split tree (and which pane was focused) against the _anchor_
workspace — the first pane you attached to — then detaches to the
dashboard. (`Ctrl-x Esc` just dismisses the navigation overlay and leaves
you attached.) The next time you attach to that workspace, wsx restores the
layout and respawns the side panes' sessions. Panes whose workspaces no
longer exist are pruned on restore; if none survive you get a plain
single-pane view. Workspaces with a saved multi-pane layout show a columns
glyph next to their branch on the dashboard (nerd fonts only).
