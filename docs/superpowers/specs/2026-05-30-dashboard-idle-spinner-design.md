# Dashboard idle spinner for never-prompted sessions

**Date:** 2026-05-30
**Status:** Implemented in PR #130 (plan: `docs/superpowers/plans/2026-05-30-dashboard-idle-spinner.md`)

## Problem

On the dashboard, a workspace whose chat session has been spawned but never
given an initial prompt shows the animated braille spinner — the same
indicator used for an agent actively working. Nothing has happened yet, so the
spinner is misleading.

### Root cause

The spinner is not driven by the `None` branch of the activity match, as a
surface reading suggests. The actual sequence:

1. Opening a session spawns the agent PTY immediately
   (`src/pty/session.rs` `spawn_session`); `SessionStatus::Running` is set at
   once.
2. The agent (e.g. `claude`) renders its welcome/prompt UI on launch, which
   emits PTY output bytes.
3. The reader thread sets `activity_ms` on *any* output byte
   (`src/pty/session.rs:1238`).
4. In `App::classify_status` (`src/app.rs:427`), `seconds_since_activity`
   therefore becomes `Some(small)`, not `None`.
5. `Status::classify` (`src/ui/dashboard/status.rs:90-95`) hits
   `Some(s) if s < 30 => Status::Thinking`.
6. `Status::Thinking.is_live()` is `true`, so `row.rs` renders the spinner.

The classifier has no input that distinguishes "agent process has rendered
output" from "the user has actually started work."

## Decision

When a session is running but the user has never submitted a prompt, show
`Status::Idle` (static `·`) — the same indicator as a workspace with no live
session. No new enum variant.

Rationale: the user explicitly wants "nothing has happened" to read as idle,
and reusing `Idle` keeps the change minimal (no new glyph/label/priority/tests
for a new state).

## Detection signal

`WorkspaceEvents::first_user_text` (`src/activity/events.rs:169`) is populated
by the background JSONL tailer (`src/app/background.rs:171-174`) only once a
real user message appears in the agent's session log. It is the existing,
reliable "user has started work" flag.

`classify_status` already reads `self.workspace_events.get(&ws.id)`, so the new
signal is derived inline with no new plumbing:

```rust
let user_has_prompted = self
    .workspace_events
    .get(&ws.id)
    .and_then(|e| e.first_user_text.as_ref())
    .is_some();
```

## Change

### `src/ui/dashboard/status.rs`

Add a `user_has_prompted: bool` parameter to `Status::classify` and gate the
running branch:

```rust
if session_running {
    if !user_has_prompted {
        // Session is live but the user has never submitted a prompt — the
        // agent is idle at its welcome screen, so nothing has happened yet.
        return Status::Idle;
    }
    match seconds_since_activity {
        Some(s) if s < 30 => Status::Thinking,
        Some(_) => Status::Waiting,
        None => Status::Thinking,
    }
} else {
    let _ = has_prior_session;
    Status::Idle
}
```

The gate sits at the top of the `session_running` branch, below the
higher-priority early returns (`awaiting_tool` → Question, `stopped_kind` →
Question/Complete, `stalled` → Stalled). Those states cannot legitimately occur
before a first prompt (they all require a turn / JSONL events), so ordering is
safe and they remain unaffected.

### `src/app.rs`

In `classify_status`, derive `user_has_prompted` (above) and pass it as the new
argument to `Status::classify`.

## Behavior

| Situation | Before | After |
|-----------|--------|-------|
| Session spawned, no prompt submitted | spinner (Thinking) | `Idle (·)` |
| First prompt just submitted | spinner | `Idle` for up to ~2s, then spinner |
| Agent actively working (post-first-prompt) | spinner | spinner (unchanged) |
| Resumed session with prior history | spinner per activity | spinner per activity (unchanged) |
| No live session | `Idle (·)` | `Idle (·)` (unchanged) |

### Accepted tradeoff

`first_user_text` is sourced from the JSONL tail, which runs on an interval
(~2s). For the brief window after the user submits their *first* prompt — and,
at app startup, briefly for any already-running session before the first tail —
the workspace reads as `Idle` though the agent is working. The spinner then
appears within one tail cycle. This affects the first turn only and is
acceptable for a dashboard indicator. (The lag-free alternative — tracking the
first PTY write at the app level — was considered and rejected as not worth the
extra wiring.)

## Testing

`src/ui/dashboard/status.rs` unit tests:

- running + `!user_has_prompted` → `Idle` (the new case)
- running + `user_has_prompted` + recent activity → `Thinking` (regression)
- running + `user_has_prompted` + stale activity → `Waiting` (regression)
- higher-priority states (Question / Complete / Stalled) still win even when
  `!user_has_prompted`, confirming the gate ordering

Update all existing `Status::classify` call sites in the test module to pass
the new `user_has_prompted` argument (most existing running-state tests should
pass `true` to preserve their intent).

`Status::Idle.is_live()` is already `false`, so no renderer/`row.rs` changes or
tests are needed.

## Scope

Two files: `src/ui/dashboard/status.rs` and `src/app.rs`. One logical commit.
