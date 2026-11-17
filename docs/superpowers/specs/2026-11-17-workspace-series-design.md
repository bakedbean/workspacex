# Workspace series: "part x of feature y" in the focus view

## Intent

Features often ship as many small PRs, one workspace each, chained with
the `handoff` skill (finished workspace → fresh workspace for the next
slice). Today nothing records that chain, so the repo focus view can't say
that `focus-diff-bars` is the fourth slice of the focus-view rework. Record
the chain when the next workspace is created and show each workspace's
place in it: `part 4 · Focus view declutter`, with the earlier slices'
PRs.

## Goals

- `wsx workspace create <repo> --continues <slug>` joins the new
  workspace to `<slug>`'s series as the next part, starting a series
  (parent = part 1) if `<slug>` isn't in one yet.
- The `handoff` skill passes `--continues` so its chains link
  automatically.
- The series outlives archiving. Archived slices keep their part
  number, branch and last-known PR, so part 4 stays part 4 after parts
  1–3 are archived.
- The focus view shows, for each workspace in a series, its part number,
  the series title, and the earlier parts' PRs.
- The series names the feature, not a slice. When handoff starts a
  series, the outgoing agent sets a feature title. Anyone can change it
  later.
- Archived slices show their PR's final state (merged or closed), even
  when the PR merged after the slice was last polled.

## Non-goals

- GitHub epics, sub-issues, linked issues, labels. Nothing is read from
  GitHub beyond the PR data the dashboard already fetches.
- Linking existing workspaces after the fact (no `series join`). Chains
  that already exist, like the focus-view PRs #466–#469, stay unlinked.
- Showing series anywhere besides the focus view: the dashboard list,
  attached bar, waybar and menubar don't change.
- Linking workspaces in different repos.
- A total ("part 4 of 6"). How many parts there will be isn't known.

## Anti-goals

- Inferring a series from creating one workspace from inside another.
  The workspace doctrine also creates workspaces that way to split off
  unrelated work, and those must not join a series.
- Making `create` or the focus view noticeably slower. The series read
  is local SQLite only.

## Approaches considered

1. Auto-link every create run from inside a workspace (via
   `WSX_WORKSPACE_ID`). Needs no effort, but it puts unrelated spin-offs
   into the series.
2. **An explicit `--continues <slug>` flag that handoff passes (picked).**
   Links exactly the follow-ups that were meant as follow-ups.
3. A named `--feature <name>` given on every create. Gives the cleanest
   name, but the name has to be typed on every create and the handoff
   chain doesn't supply one.

## Design

### Data (schema v31)

Two new tables. Neither has a foreign key to `workspaces`, so rows
survive `delete_workspace` (archiving deletes the workspace row):

```sql
CREATE TABLE IF NOT EXISTS series (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_id    INTEGER NOT NULL REFERENCES repos(id),
    title      TEXT,             -- NULL: fall back to part 1's slug
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS series_parts (
    series_id    INTEGER NOT NULL REFERENCES series(id),
    part         INTEGER NOT NULL,      -- 1-based, assigned at join
    workspace_id INTEGER NOT NULL UNIQUE, -- AUTOINCREMENT ids are never reused
    slug         TEXT NOT NULL,         -- kept current on rename
    branch       TEXT NOT NULL,         -- kept current on rename
    pr_number    INTEGER,               -- snapshot, refreshed (below)
    pr_lifecycle TEXT,                  -- same encoding as scm_cache
    archived_at  INTEGER,               -- NULL while the workspace lives
    PRIMARY KEY (series_id, part)
);
```

- **Live vs archived.** A part is live when `archived_at IS NULL`. Live
  parts read their PR from the dashboard's in-memory PR status, the same
  source the PR chip uses. `pr_number` and `pr_lifecycle` are a snapshot
  for after archiving: written whenever the PR status poll stores a
  result for a workspace in a series, and once more in `archive` just
  before `delete_workspace`, from `scm_cache`.
- **Settling archived PRs.** The dashboard's PR poll
  (`PR_POLL_INTERVAL_MS`, `src/app/background.rs`) also takes archived
  parts whose `pr_number` is set and whose `pr_lifecycle` is not merged
  or closed. For each one it runs `gh pr view <number> --json <fields>`
  from the repo's path and updates the snapshot. Once a part is merged
  or closed it drops out, so the extra cost is one `gh` call per
  unsettled archived part per poll, and that set is usually empty. It
  stays basic, with no review-gate or thread probes. Failures leave the
  snapshot as it is.
- **Rename.** `slug` and `branch` on the part row are kept current inside
  `Store::rename_workspace` and `Store::set_workspace_branch`, so every
  rename path is covered: the CLI, the TUI modal, and the dashboard's
  branch-drift rename. (Builder's call.)
- **Removing a repo** deletes its series and parts. That path already
  deletes the repo's other rows explicitly.

### Joining (`--continues`)

- `wsx workspace create <repo> --name <slug> --continues <parent-slug>`.
  `<parent-slug>` is a live workspace in the same repo. The repo comes
  from the positional `<repo>`, so a cross-repo parent can't be named.
  If the parent is unknown, `create` fails before any worktree is made.
- One transaction, after the new workspace row is inserted:
  1. If the parent has no part row, create a `series` with a NULL title
     and give the parent part 1. A recap goal names a slice, not the
     feature, so it isn't used as a default.
  2. Give the new workspace `MAX(part) + 1` in the parent's series.
- Branching (two workspaces both continuing part 3) gives them parts 4
  and 5, in creation order. Parts are a sequence, not a tree.
- If the create fails after the row is inserted (the workspace ends up
  `Failed`), its part row stays. Archiving the failed workspace marks the
  part archived with no PR, like any other archive.
- After a successful create with `--continues`, the human report adds a
  line: `continues <repo>/<parent> as part N of "<title>"`. When this
  create started the series, the line is instead
  `started a series: <parent> is part 1, <new> is part 2 (no title yet;
  set one with wsx series title)`.

### CLI

- `--continues <slug>` on `workspace create`, parsed like `--prompt`
  (it takes a value, and is an error without one).
- `wsx series show [--workspace <repo>/<slug>] [--json]` prints the
  series of the current workspace (resolved like `wsx recap`):
  the title, then one line per part with part, slug, branch, PR, and
  whether it's archived. This lets a handoff agent name the lineage in a
  PR body ("part 4 of …"). If the workspace isn't in a series, it exits
  0 with `not part of a series`.
- `wsx series title <text> [--workspace <repo>/<slug>]` sets the title.
  An empty `<text>` clears it, going back to the slug fallback.

### Handoff skill

`skills/handoff/SKILL.md` step 5 becomes
`wsx workspace create <repo> --name <new-slug> --continues <current-slug>`,
using the slug resolved in step 3. Then a new step: if the series has no
title (the report says so, or `wsx series show` prints none), the
outgoing agent picks a short name for the *feature* the chain is
building, 2–5 words, drawn from its session, and runs
`wsx series title --workspace <repo>/<current-slug> "<name>"`. It
reports the title it chose to the user, so they can rename it. In step
6, the brief's WHY can cite `wsx series show` output. Users need to rerun `wsx setup install-skill`
to get the updated skill. The PR body and CHANGELOG should say so.

### Focus view

The `part` line goes in the prose block under the branch, as its first
line (before `prompt`):

```text
▎▎ ⠋ bakedbean/focus-diff-bars   ⏷ #469 draft  ▸ working  …
 ▎   part    4 · Focus view declutter   #466 ✓  #467 ✓  #468 ⏺
 ▎   goal    One diff bar per commit
```

- `part N · <title>`: the title in the section style, cut at the prose
  wrap width with `…`. Without a title, part 1's slug is used.
- After the title, the earlier parts' PRs in part order, oldest first.
  Each shows its number and the same lifecycle glyph and color as the
  PR chip (merged, open, draft, closed). A part with no PR shows its
  slug, dimmed. If they don't fit on the line, the oldest are dropped,
  starting from the left, and a dim `+k` replaces them.
- Later parts aren't listed. When a later part exists, the line ends with
  a dim `→ part M` naming the newest one, so part 1 can show it was
  continued.
- Workspaces not in a series show no `part` line.
- **Plumbing.** `WorkspaceDetail` gains `series: Option<SeriesSlice>`
  (part, title, earlier parts' `(number, lifecycle | slug)`, newest
  part). The series rows are loaded in `App::refresh()` next to `recaps`
  (`src/app/state.rs`) and cached on `App`. `src/app/render/dashboard.rs`
  joins them to live PR status for live parts when building details.
  (Builder's call.)

### Docs

- `docs/book/src/dashboard/repo-focus.md`: the `part` line under **The
  prose**, and in the example block.
- The CLI reference for `--continues` and `wsx series`.
- The handoff skill text, as above.

### Tests

- Poll: unsettled archived parts are selected, settled parts are not,
  and a fetched state updates the snapshot.
- Store: starting a series, joining as next part, branching ordinals,
  part rows surviving `delete_workspace`, rename keeping slug and branch
  current, the archive snapshot, and repo removal cleaning up.
- CLI parse: `--continues` with and without a value; `series show` and
  `series title`.
- Create: an unknown parent fails with no worktree made; the report line.
- Render: the part line's position and content, the slug fallback,
  dropping old PRs with `+k`, `→ part M`, and no line outside a series.

## Left to the builder

- Module layout (`src/data/series.rs` mirroring `recap.rs` is the
  obvious fit) and the exact `SeriesSlice` shape.
- How the PR poll writes the snapshot (inline in the scm_cache write, or
  a separate update).
- The `series show` text layout and JSON field names.
