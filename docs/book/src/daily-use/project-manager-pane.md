Press `p` on the dashboard to open the project-manager digest: a horizontal
pane below the workspace list that instantly lists every `Ready` workspace,
grouped by repo. There's no agent session behind it — the digest is rendered
directly from wsx's own state (recaps, pushed status, git counts, PR
lookups), so it opens with no delay and nothing to configure.

`p` opens the digest and focuses it immediately (like the attached view).
`Tab` or `Esc` swaps focus back to the dashboard; `Tab` from the dashboard
swaps back into the digest. `p` closes it from either focus. `q` also
closes it, but only while the digest is focused — dashboard-focused `q`
quits wsx entirely (killing running sessions), so don't reach for `q` to
close the digest unless focus is already on it.

Within each repo group, cards are ordered by what needs attention first:
blocked workspaces, then waiting workspaces, then the rest oldest-activity-
first.

## What a card shows

- **Header line**: workspace name, branch, and coding agent, plus the
  agent-pushed status in brackets — `[blocked 4s]`, `[waiting 12m]`, etc. —
  with its message appended when the agent reported one. This is the same
  status set via `wsx status set`.
- **Recap lines** — `goal:`, `state:`, `next:` — the agent's own account of
  what the workspace is for, where it's at, and what's left. Only fields the
  agent has actually set are shown. Workspaces whose agent hasn't run since
  this feature landed (or that have never had a recap written) show
  `no recap yet — agent hasn't run since this feature landed` instead.
- **Facts line**: git counts (`↑ahead ↓behind ~modified ?untracked`), a PR
  chip (`PR #241 open`, `PR #241 draft`, `PR merged`, …) colored by
  lifecycle, `active <age> ago` from the workspace's last session activity,
  and a `recap stale` marker when that activity is newer than the recap —
  a sign the agent moved on without updating it.

## Where recaps come from

Each workspace's own agent maintains its recap with `wsx recap set`:

```bash
wsx recap set --goal "fix auth"
wsx recap set --state "tests failing" --next "debug the regex"
```

Any subset of `--goal`, `--state`, and `--next` can be set at once (at
least one is required); omitted flags leave the existing value untouched.
`wsx recap show` prints the current recap, and `wsx recap clear` deletes
it. This isn't something you normally run by hand — the standing operating
doctrine wsx injects into every session (see `process_doctrine` in
[Global settings](../configuration/global-settings.md)) instructs the agent
to set the goal once scope is clear and refresh state/next alongside its
status updates.

## Keys

| Key (digest focused)   | Action                                            |
| ----------------------- | -------------------------------------------------- |
| `j` / `k` (or arrows)  | Move selection                                    |
| `Enter`                | Attach to the selected workspace                  |
| `/`                    | Filter cards by workspace name (type to narrow)   |
| `Esc` / `Tab`          | Clear the filter (if active) / return focus       |
| `q` / `p`              | Close the digest                                  |
| `r`                    | Force a git/PR cache refresh                      |

| Key (dashboard focused)     | Action                                       |
| ---------------------------- | --------------------------------------------- |
| `p`                          | Toggle the digest                            |
| `Tab`                        | Focus the digest (when visible)              |
| `r` (with digest visible)   | Force a git/PR cache refresh                 |
