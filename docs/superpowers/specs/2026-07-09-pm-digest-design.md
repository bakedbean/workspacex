# Project Manager digest: native view + agent-authored recaps

**Date:** 2026-07-09
**Status:** Approved

## Problem

The `p` Project Manager pane spawns an interactive Claude Code session that is
told to reverse-engineer every workspace: grep raw session JSONL logs, `cd`
into worktrees, run git — serially, per workspace, inside a PTY. It is slow
enough that it never gets used, and the summaries are shallow because the
dossier it starts from (`workspaces.json`) contains only names and paths
(`GitCounts` is hardcoded to `Default::default()`; Hermes/Codex session dirs
are literal `UNSUPPORTED-...` placeholder paths).

Meanwhile wsx already holds most of the answer in memory: agent-pushed
statuses (`workspace_status` table), git counts (`App.git` cache of
`git::WorkspaceStatus`), PR lifecycle + number (`App.pr_lifecycle` /
`App.pr_number` caches), and per-workspace activity (`App.workspace_events` /
`App.workspace_activity`). The one thing missing is the narrative: *what was
this workspace for, where did it leave off, what's next*.

Constraint: headless `claude -p` invocations are off the table. The narrative
must come from a sanctioned source.

## Decision

Remove the PM agent session entirely. `p` opens a **native digest view**
rendered from data wsx already has, plus a new **agent-authored recap** that
each workspace's own agent maintains via a `wsx recap` CLI command as it
works (driven by the process doctrine). No LLM in the read path; zero
marginal token cost; the view opens instantly.

Old workspaces have no recap until their agent runs again. That is accepted
(single-user install, clean-break preference): the digest shows "no recap
yet" plus the deterministic facts.

## 1. Recap data model + CLI

New store table `workspace_recap`, one row per workspace:

```sql
CREATE TABLE workspace_recap (
    workspace_id INTEGER PRIMARY KEY
        REFERENCES workspaces(id) ON DELETE CASCADE,
    goal TEXT,            -- what the workspace is for (set ~once)
    state TEXT,           -- where the work currently stands
    next TEXT,            -- the immediate next step
    updated_at INTEGER NOT NULL  -- epoch ms of last field change (matches
                                 -- reported_at and last_log_activity_ms)
);
```

All three fields are one-liners (same spirit as the status message). Store
API mirrors `workspace_status`: `set_workspace_recap(id, goal: Option<&str>,
state: Option<&str>, next: Option<&str>)` performing a **partial upsert**
(only provided fields change, `updated_at` always bumps),
`workspace_recap(id)`, `all_workspace_recaps()`, `clear_workspace_recap(id)`.

CLI, resolving the workspace the same way `wsx status set` does
(`WSX_WORKSPACE_ID` env var first, then cwd):

```
wsx recap set [--goal "..."] [--state "..."] [--next "..."]   # ≥1 flag required
wsx recap show                                                # prints the three fields
wsx recap clear
```

The three fields deliberately mirror the old PM output contract
("what it's for; where it left off; next step") — but authored by the agent
that did the work, while it still has full context.

## 2. Doctrine clause

New `CLAUSE_RECAP` in `src/agent/doctrine.rs`, injected alongside
`CLAUSE_STATUS` for every agent kind:

> Maintain the workspace recap with `wsx recap set`: set `--goal "<one
> line>"` once when you understand the task's scope, and update `--state
> "<one line>"` and `--next "<one line>"` whenever you set status and
> whenever you end a turn with the task unfinished.

Because it is a plain CLI command, it works identically for Claude, Pi,
Hermes, and Codex — erasing the old dossier's Hermes/Codex session-log gap.

## 3. The `p` digest view

`p` toggles a native view in the same screen area the PM pane occupies today.
No PTY, no spawn — it renders synchronously from existing app state. One card
per workspace (Ready workspaces, all repos), grouped by repo.

Each card shows:

- **name / branch**, agent kind
- **status badge + message + age** (from `workspace_status`)
- **goal / state / next** recap lines, or a dim "no recap yet"
- **stale marker** when the workspace's last session activity is newer than
  `recap.updated_at` — the recap predates the latest work
- **git counts**: ahead/behind, modified+untracked (from the `App.git` cache)
- **PR chip**: number + lifecycle color (from `App.pr_lifecycle` /
  `App.pr_number`, rendered via the existing `theme::lifecycle_style`)
- **last-activity age** (from the existing activity tracking)

Ordering inside each repo, by needs-attention: `blocked` first, then
`waiting`, then remaining workspaces stalest-first (oldest last-activity
first). Keys: `j`/`k` (and arrows) move selection, `Enter` attaches to the
selected workspace, `p`/`q`/`Esc` close. Cards that don't fit scroll.

All rendered data comes from caches the dashboard already maintains; the
digest triggers no git/gh subprocesses of its own. The background loop's
existing refresh cadence (PR lifecycle throttled to 30s, git status per tick)
is sufficient.

## 4. Deletions (clean break)

- `SpawnMode::ProjectManager` and its handling in all four command builders
  in `src/pty/command.rs`; `SessionManager::spawn_pm` / `pm()`.
- `src/agent/pm.rs` spawn/refresh/dossier machinery: `open_pm*`,
  `refresh_pm`, `write_workspaces_json`, `init_pm_dir`, `PM_SYSTEM_PROMPT`,
  the auto-summary/refresh messages, `pm_fast_mode_enabled`. The module is
  deleted; the digest's view-model builder lives with the renderer under
  `src/ui/`.
- Settings `pm_enabled` and `pm_fast_mode`, including their
  `known_setting_key()` entries in `cli.rs` — the digest is always available,
  so `p` no longer needs a gate (`render::pm_enabled` goes away).
- `src/ui/pm_pane.rs` PTY rendering; the file is rewritten as the digest
  renderer. `PaneFocus::ProjectManager`, `App.pm_visible`, and the `p`/`r`
  input handling are repurposed for the native view (focus + refresh keys
  stay, but `r` now just forces a git/PR cache refresh nudge rather than
  messaging a PTY).
- The PM working directory (`dirs.pm_dir()`) is no longer created or used;
  existing `~/.../pm` dirs on disk are simply orphaned (no migration).

## 5. Testing

- **Store:** recap CRUD round-trip; partial upsert semantics (setting only
  `--state` preserves `goal`/`next`, bumps `updated_at`); cascade delete with
  workspace.
- **CLI:** `recap set` with each flag combination; zero-flag invocation is a
  usage error; `recap show` output; unknown-workspace (not in a worktree, no
  env var) fails with the same guidance as `status set`.
- **Doctrine:** `process_doctrine` output contains the recap clause for every
  agent kind.
- **Digest render:** ratatui buffer tests for populated card, no-recap card,
  stale-marker case, needs-attention ordering, and selection/scroll.
- **Regression:** pressing `p` spawns no PTY session (`SessionManager` has no
  PM session afterwards).

## Trade-offs accepted

- A session killed mid-turn can leave `state`/`next` stale; the stale marker
  plus status-message age are the mitigation, not a guarantee.
- Recap quality depends on agent doctrine compliance, like status reporting
  today.
- No cross-workspace synthesis ("which should I tackle first?") — the
  needs-attention ordering is the deterministic stand-in. If synthesis is
  ever wanted, it can be a separate feature; nothing here forecloses it.
