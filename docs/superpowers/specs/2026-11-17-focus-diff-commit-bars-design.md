# Focus view diff column: one bar per commit

## Intent

The repo focus view's diff column shows a single solid horizontal bar
(`██▌ `) sized to the branch's total diff. It reads as flat. Replace it
with a small history graph, one vertical bar per commit on the branch, so a
glance shows the shape of the work (one big drop vs. steady small commits)
as well as its size. The `+N −M` totals after it stay.

## Goals

- `▂▅▁▃█▂▄  +184 −22`: the branch's most recent commits, oldest on the
  left, each bar's height the commit's lines changed.
- No extra git process: per-commit counts come from the `git log` the
  10s diff poll already runs for the side panel.

## Non-goals

- Uncommitted work. The totals are `base...HEAD` today and stay so; no
  trailing working-tree bar.
- Other views (dashboard list rows, attached bar, waybar/menubar) keep
  their current diff cells.
- Per-commit hover/detail.

## Approaches considered

- **A. One bar per commit, height = commit size** (picked): shows the
  shape of the work from data we already fetch.
- B. One bar per time slice: shows idle gaps, but needs a slice size that
  fits hour-long and week-long branches alike, and overlaps the age column.
- C. Running total after each commit: literal staircase, but nearly always
  rising, so rows look alike; less information from the same data.

## Design

- **Data.** `branch_commits` (`src/git/mod.rs:1023`) adds `--shortstat` to
  its `git log`; `BranchCommit` gains `stats: Option<DiffStats>` (`None`
  for a commit with no line changes, e.g. binary-only or empty). Parsing
  pairs each `%h\t%s` line with the shortstat line that follows it. The
  side panel's commit list is unaffected.
- **Plumbing.** The focus view's `WorkspaceDetail` gains `commit_sizes`,
  newest first, taken from `app.workspace_branch_lists` in
  `src/app/render/dashboard.rs` (only built for the focused repo).
- **Render** (`src/ui/dashboard/tall.rs`, replacing `diff_bar`):
  - Up to `DIFF_BARS = 8` cells, the 8 newest commits, drawn oldest →
    newest left to right; fewer commits draw fewer bars, and the column
    pads to the widest row so `+N −M` stays aligned.
  - Height: `▁▂▃▄▅▆▇█` in eight even buckets, log scale (`ln_1p`) against the largest single
    commit across the view's rows; any commit with changes gets at least
    `▁`; a commit with no line changes draws a dim `▁`.
  - Color: green when the commit added at least as many lines as it
    removed, red otherwise.
  - A branch with a diff but no commit data yet (first poll, or log
    failed) shows the totals with no bars.
- **Tests.** Parser test for `log --shortstat` output (incl. a commit with
  no stat line); render tests for ordering, the 8-commit cap, log scaling
  against the view max, colour by dominant side, and alignment padding.

## Left to the builder

- Exact span batching and helper names.
- Whether more than 8 commits gets an elision mark (default: no).
