# Project Manager Pane Design

**Issue:** [bakedbean/workspacex#8](https://github.com/bakedbean/workspacex/issues/8)
**Date:** 2026-05-14

## Goal

Add an opt-in "Project Manager" Claude Code instance that runs at the
dashboard level. When the user opens its pane, it inspects all active
workspaces and answers three questions about each:

1. What was the workspace created for? (the original prompt or issue)
2. Where have things been left at? (most recent activity)
3. What's next to close it out?

The pane is summoned from the dashboard by a keybind, splits the dashboard
view horizontally, and is interactive — the user can ask follow-up
questions about specific workspaces.

## Non-goals (v1)

- Adjustable split ratio (fixed at 60/40 dashboard/PM).
- PM access in the attached-workspace view (PM is a dashboard-level
  concept; the keybind is inert in `View::Attached`).
- Multiple simultaneous PM conversations.
- PM acting on workspaces — read-only inspection only.
- Pre-summarized event data in `workspaces.json` (PM mines JSONLs itself).

## Architecture

A new module `src/pm.rs` owns the PM session lifecycle and the hybrid
info-flow file. The existing PTY infrastructure handles rendering and
process management.

### Persistent home

PM lives in `$XDG_STATE_HOME/wsx/project-manager/`:

- Created lazily on first `p` press.
- Contains `workspaces.json` (refreshed on each pane open and on `r`
  refresh).
- Initialized as a minimal git repo on creation so Claude Code is happy
  in it.
- Persists across wsx restarts. Claude Code keys session continuity by
  cwd, so subsequent wsx runs find PM's prior session via `--continue`.

### Spawn mode

Extend `SpawnMode` in `src/pty/session.rs`:

```rust
pub enum SpawnMode {
    Fresh { rename_ctx: Option<RenameContext>, custom_instructions: Option<String> },
    Continue { custom_instructions: Option<String> },
    ProjectManager {
        workspaces_json_path: PathBuf,
        custom_instructions: Option<String>,
        resume: bool,  // true => add --continue; false => Fresh-style start
    },
}
```

`build_claude_command` handles the new variant by:

1. Adding `--allowedTools` with the read-only tool list (below).
2. Adding `--append-system-prompt` with the PM system prompt (below),
   plus `custom_instructions` if set.
3. Adding `--continue` when `resume == true`.

The choice between `resume: true` vs `resume: false` is made by the
caller using the existing `has_prior_session(pm_cwd)` helper. PM does
NOT auto-fallback inside `build_claude_command` — if the caller asks
for resume and there's no session, claude will error and surface to the
user, same as a workspace `Continue` failure.

### Allowed tools (narrow, read-only)

```
Read
Bash(git status:*)
Bash(git log:*)
Bash(git diff:*)
Bash(git branch:*)
Bash(cat:*)
Bash(ls:*)
```

Explicitly NOT in the allowlist:

- Any write/edit tools.
- `Bash(*)` or any broad bash variant.
- Network tools.
- Any tool that could mutate a workspace.

### App integration

`App` gains:

```rust
pub pm: Option<Arc<Session>>,        // None until first `p`
pub pm_visible: bool,                // false at start
pub focus: PaneFocus,                // Dashboard | ProjectManager
```

`pm_visible` is toggled by `p`. `pm` is born on first `p` and lives for
the wsx process lifetime (killed on quit via `SessionManager::kill_all`,
which uses the existing `Drop`-based `ChildKiller` pattern).

## Info flow: `workspaces.json`

Written by wsx in two cases:

1. Every time the pane is opened (`p` with `pm_visible == false`).
2. On `r` refresh while pane is focused.

### Schema

```json
{
  "generated_at": "2026-05-14T20:51:00Z",
  "repos": [
    {
      "name": "ssk",
      "path": "/home/eben/code/ssk",
      "workspaces": [
        {
          "name": "fix-auth-bug",
          "branch": "bakedbean/fix-auth-bug",
          "worktree_path": "/.../wsx/worktrees/ssk/fix-auth-bug",
          "session_log_dir": "/home/eben/.claude/projects/-home-...-fix-auth-bug",
          "state": "Active",
          "git": { "modified": 3, "untracked": 1, "ahead": 0, "behind": 0 }
        }
      ]
    }
  ]
}
```

### Field rules

- `generated_at`: RFC 3339 UTC timestamp.
- `repos`: every registered repo, even if it has no active workspaces
  (so PM sees the full picture; empty `workspaces` array is fine).
- `workspaces.state`: only `Active` workspaces are included. `Failed`
  and any archived/deleted workspaces are omitted. (PM cannot help with
  workspaces that no longer exist.)
- `worktree_path`: absolute path; PM uses this for `cd` + git
  inspection.
- `session_log_dir`: absolute path to
  `~/.claude/projects/<encoded-cwd-of-worktree>/`. PM reads JSONLs here
  to find original prompts and recent activity. Computed using the same
  `/`/`.` → `-` encoding wsx already uses elsewhere.
- `git`: counts only; PM uses tools for deeper inspection if needed.
- No event content, no pre-extracted "first prompt", no diff bodies —
  PM mines those itself via its Read/Bash tools.

### Atomic write

`workspaces.json` is written via the standard write-to-tempfile +
rename pattern so PM never reads a half-written file.

## PM system prompt

Appended via `--append-system-prompt` on spawn:

```
You are a project manager for a developer running multiple parallel coding
workspaces under wsx. Each workspace is a git worktree with its own Claude
Code session. Your job: when asked, inspect their active workspaces and
report (1) what each was created for, (2) where it left off, (3) what's
next to close it out.

Where to find information:
  - ./workspaces.json lists all active workspaces with: name, branch,
    worktree_path, session_log_dir, state, git counts.
  - For the original prompt: read the FIRST user message in the earliest
    *.jsonl under session_log_dir.
  - For recent activity: read the LAST several entries in the most recent
    *.jsonl under session_log_dir.
  - For code state: cd to worktree_path; use git status / log / diff.

Constraints:
  - Read-only. You cannot modify workspaces.
  - Be concise — the developer is glancing at a small pane. Default to a
    per-workspace block:
        <name>: <one-line status>
          - Created for: <one-line>
          - Last activity: <one-line>
          - Next: <one-line>
  - If you're uncertain about "next", say so; don't fabricate.
  - workspaces.json refreshes when the developer asks. Trust its contents
    over stale memory.
```

If `pm_custom_instructions` is set, it's appended after the above with a
blank line separator, matching the existing `custom_instructions` flow.

## UI layout & focus

### Visual

When `pm_visible == true` and on `View::Dashboard`:

```
┌──────────────────────────────────────────────────┐
│ wsx — Workspaces                                 │
│                                                  │
│ ▶ ssk                                            │ ← dashboard
│   → ● fix-bug   bakedbean/fix-bug  ~3   active  │   (top ~60%)
│     ○ new-api   bakedbean/new-api       resumable│
│                                                  │
│ [n] new  [e] edit  [t] term  [d] archive  [q]quit│
├── Project Manager [Tab to focus]  ───────────────┤
│ fix-bug: waiting on permission to run            │ ← PM pane
│   `cargo test`. Started 1h ago re: #42.          │   (bottom ~40%)
│ new-api: idle, awaiting follow-up after schema   │
│   design discussion.                             │
└──────────────────────────────────────────────────┘
```

Split is computed with ratatui `Layout::default().direction(Vertical)`
using `Constraint::Percentage(60)` + `Constraint::Percentage(40)`.

### Pane title

- Unfocused: `── Project Manager [Tab to focus] ──`
- Focused:   `── Project Manager [Tab/Esc back · r refresh] ──`

Both rendered with `theme.dim_style()` so they stay subdued.

### Focus state machine

`PaneFocus` enum: `Dashboard | ProjectManager`. Default `Dashboard`.

Transitions (only valid while `pm_visible == true`):

| Current focus | Key | Action | New focus |
|---|---|---|---|
| Dashboard | `Tab` | Swap | ProjectManager |
| ProjectManager | `Tab` | Swap | Dashboard |
| ProjectManager | `Esc` | Swap | Dashboard |
| Dashboard | `p` | Hide pane | Dashboard (focus unchanged) |
| ProjectManager | `p` | Hide pane | Dashboard (focus reset) |

When `pm_visible == false`, focus is forced to `Dashboard`.

### Cursor

- Focus = ProjectManager: cursor placed inside the PM pane using
  `vt100`'s cursor position from PM's screen parser, same as the
  attached view.
- Focus = Dashboard: no cursor shown.

### View visibility rules

- `View::Dashboard`: PM pane rendered iff `pm_visible == true`.
- `View::Attached`: PM pane NOT rendered, regardless of `pm_visible`.
  The PM session keeps running in the background; rendering resumes on
  return to dashboard.
- `View::Modal(_)`: same as Attached — PM pane hidden, session alive.

### Resize behavior

On every terminal resize event:

- Recompute dashboard rect (now uses 60% of available height when pm
  visible, else 100%).
- If PM session exists, call `pm_session.resize(width, pm_rect.height)`
  so vt100's screen stays in sync with the rendered area.

## Keybindings

### Dashboard view, `pm_enabled == true`

| Key | When | Action |
|---|---|---|
| `p` | `pm_visible == false` | If PM session not yet spawned in this run: spawn it (Fresh or Continue depending on `has_prior_session`), write `workspaces.json`, mark visible. If session is Fresh: send auto-summary user message via PTY write (gated on PromptCapture readiness, 5s timeout). |
| `p` | `pm_visible == true` | Hide pane. Session stays alive. |
| `Tab` | `pm_visible == true`, focus = Dashboard | Swap focus to ProjectManager. |
| `Tab` / `Esc` | focus = ProjectManager | Swap focus to Dashboard. |
| `r` | focus = ProjectManager | Rewrite `workspaces.json`; send refresh user message to PM PTY. |
| Other printable / control keys | focus = ProjectManager | Forward to PM PTY, same as attached view. |
| Dashboard nav (Up/Down/n/e/t/d/Enter) | focus = Dashboard | Normal dashboard behavior; PM pane is inert. |

### Dashboard view, `pm_enabled == false`

`p` is a no-op. All other dashboard keys behave as today.

### Other views

`p` is not handled in `View::Attached` (forwarded to claude session)
or `View::Modal(_)` (handled by modal). PM pane is hidden in both.

## Auto-message delivery

When the PM pane is opened for the first time in a wsx run AND PM was
spawned Fresh (not Continue), wsx sends an initial user message into
the PM PTY:

```
Give me a status summary of all active workspaces per your instructions.
```

When the user presses `r` while the pane is focused, wsx sends:

```
Refresh: workspaces.json has been updated. Re-summarize the current state of all workspaces.
```

### Timing

Auto-messages are written to the PTY as bytes followed by `\r`. They
must land in claude's input prompt, not its startup banner. We gate on
PTY output settling, using the existing `Session::activity_ms` atomic:

- Spawn returns immediately with `Session`.
- A background task polls `activity_ms`. The readiness condition is:
  output has been observed (i.e., `activity_ms > 0`) AND no new output
  has arrived for a quiet window of 400ms (claude has finished
  rendering its banner + input prompt and is idle).
- Total timeout: 5 seconds from spawn. If readiness never fires within
  that window, log a warning via `tracing` and skip the auto-write —
  the user can type their own first message; no error modal.

The refresh case (`r` keypress) uses the same settling logic against
the most recent activity: write only after a 400ms quiet window
following the refresh keypress, so the message doesn't interleave with
something claude is still rendering. In practice claude is usually
already settled by the time the user presses `r`, so the wait returns
immediately.

### Continue-mode behavior

When `p` opens a session that resumes via `--continue`, NO auto-message
is sent. Rationale: avoid surprise token spend when the user is just
toggling the pane on a session that already has prior context. The
user can ask for a fresh summary by pressing `r`.

## Settings

Two new keys are added to the `known_setting_key` allowlist in
`src/cli.rs`:

| Key | Type | Default | Effect |
|---|---|---|---|
| `pm_enabled` | bool | `true` | When false: `p` is a no-op on the dashboard; pane is never rendered. PM session is never spawned. |
| `pm_custom_instructions` | string | empty | Appended to PM's system prompt with a blank line separator. Matches the existing `custom_instructions` pattern (supports `@file` and `wsx config edit pm_custom_instructions`). |

Boolean parsing follows the existing `nerd_fonts` / `notifications`
convention: `false` / `0` / `off` / `no` → disabled; everything else →
enabled.

## SessionManager integration

`SessionManager` gains:

- `spawn_pm(&self, mode: SpawnMode) -> Result<Arc<Session>>` — spawns
  PM and stores it on the manager separately from the workspace map.
- `pm(&self) -> Option<Arc<Session>>` — accessor for the current PM
  session.
- `kill_pm(&self)` — explicit kill (used on `pm_enabled` being toggled
  off mid-run; otherwise unused).
- `kill_all` is extended to also call `kill_pm` so quit cleans up.

The existing `Drop`-based `ChildKiller` plumbing already handles
process cleanup; no new logic needed there.

## Tests

All tests substitute `claude` with `cat` via `WSX_CLAUDE_BIN`, matching
the existing suite, and run with `--test-threads=1`.

1. **`workspaces.json` serialization**
   - Build an in-memory `Store` with two repos and three workspaces
     in mixed states (one Failed, two Active across the two repos).
   - Call `pm::write_workspaces_json(&store, &path)`.
   - Read the file back, assert:
     - `generated_at` parses as RFC 3339.
     - Both repos appear; the repo with no Active workspaces shows
       an empty `workspaces` array.
     - The Failed workspace is omitted.
     - Each Active workspace has the expected `worktree_path`,
       `session_log_dir`, branch, and git counts.

2. **`SpawnMode::ProjectManager` command builder**
   - Call `build_claude_command(pm_cwd, &SpawnMode::ProjectManager { ..., resume: false })`.
   - Assert the produced command:
     - Has `--allowedTools` followed by the read-only tool list.
     - Has `--append-system-prompt` followed by a string that
       contains the PM system prompt's distinctive phrases (e.g.
       "You are a project manager", "./workspaces.json").
     - Has NO `--continue` flag.
   - Repeat with `resume: true`; assert `--continue` is present.

3. **Focus state machine**
   - Table-driven test over the focus transitions in the table above.
   - Each case: set up `(pm_visible, focus)`, deliver `key`, assert
     `(pm_visible, focus)` after.

4. **`pm_enabled == false` short-circuit**
   - Set `pm_enabled` to `false` in the store.
   - Construct `App`; deliver `p`.
   - Assert `pm` is still `None`, `pm_visible` is still `false`,
     no PTY was spawned (check `SessionManager::pm()` returns `None`).

5. **Auto-summary readiness gate**
   - Spawn PM with `cat` as the binary plus a small shell wrapper
     that produces some bytes then goes quiet (`echo ready; sleep 60`
     or similar via a tiny helper script in the test fixture). This
     simulates claude rendering output and then settling.
   - Verify the auto-summary message is written AFTER the 400ms quiet
     window elapses following the burst of output (capture PTY stdin
     via the existing test seam).
   - Run a sibling case where the child never produces output (e.g.
     `sleep 10`); assert the 5s overall timeout elapses, no message
     is written, and a warning is logged (checked via a `tracing`
     test subscriber).

6. **`r` refresh**
   - Open the pane, focus PM, press `r`.
   - Assert (a) `workspaces.json` mtime advanced and (b) the refresh
     message was written to PM's PTY.

7. **Resize wiring**
   - With pane visible, dispatch a `Resize(W, H)` event.
   - Assert PM session received a `resize` call with
     `(W, ~0.4 * H rounded)`.

## README additions

A new section between "Auto-rename modes" and "Environment variables"
titled "Project manager pane" covers:

- What it is and what questions it answers.
- `p` to open, `Tab`/`Esc` to swap focus, `r` to refresh.
- Where PM's cwd lives (`$XDG_STATE_HOME/wsx/project-manager/`) and
  that it persists across wsx restarts via `--continue`.
- The two new settings keys.
- Note that PM only sees workspaces wsx knows about.

The "Storage and configuration files" table gains one row:

| Path | Contents |
|---|---|
| `$XDG_STATE_HOME/wsx/project-manager/` | PM Claude Code session cwd; contains `workspaces.json` plus PM's own git repo init |

The CLI reference table gains the two settings keys.

## Open questions deferred to follow-up

- Per-workspace "ping PM about this one" keybind from the dashboard.
- Adjustable split ratio.
- Surfacing PM's attention state (e.g., if PM hits a permission prompt
  somehow, despite the read-only tool allowlist).
- PM access from inside `View::Attached`.
- Letting PM cross-reference Linear/GitHub issues — depends on whether
  PM is given network tools, which v1 explicitly doesn't.
