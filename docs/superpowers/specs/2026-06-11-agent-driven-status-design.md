# Agent-driven status reporting — design spike

**Issue:** [#166](https://github.com/bakedbean/workspacex/issues/166) — *rely on running agent to update wsx with skill and CLI*

**Status:** Design spike. Deliverable is this document and a recommendation. No production code in this branch.

## Problem

Today wsx infers a workspace's chat state by tailing the Claude Code session
JSONL (via the `sessionx` crate) and running a heuristic classifier,
`Status::classify()` in `src/ui/dashboard/status.rs`. It folds several timing
and content signals into six display states:

| State | Glyph | Derived from |
|-------|-------|--------------|
| Question | `?` | `AskUserQuestion`/`ExitPlanMode` tool pending, or a permission prompt ≥3s old, or an awaiting-user stop_reason whose last text ends in `?` |
| Stalled | `!` | JSONL unchanged >60s mid tool-chain |
| Waiting | `…` | session live, 30s+ since PTY output |
| Thinking | `⠋` | session live, <30s since PTY output |
| Complete | `✓` | stopped with a non-question reason |
| Idle | `·` | no session, or never prompted |

This works without any cooperation from the agent and it works uniformly across
all agent kinds (claude / pi / hermes / codex). But it is an *inference* layer
built on brittle proxies:

- "Did the agent ask a question or finish?" is decided by whether the last
  assistant text ends in `?` — a string heuristic, not intent.
- "Is it working or stuck?" is a race between PTY-output recency and a 60s
  stall timer.
- The dashboard's status line shows the *longest assistant text this turn* as a
  recap — an approximation of "what is it doing", not a deliberate summary.

The agent itself knows its true state and intent far better than any of these
proxies. The issue proposes flipping the model: let the running agent **push**
its status to wsx through the CLI, and require it via the wsx SKILL and system
prompt.

## Goals

- Let the agent report an authoritative state and a short human-readable
  message ("running the test suite", "need your call on the auth approach").
- Make the common transitions (working → done → waiting on you) reliable, not
  heuristic.
- Keep working for agents that can't or don't push, and survive an agent that
  crashes or forgets — i.e. **hybrid**, never a hard dependency on cooperation.

## Non-goals

- Replacing the JSONL classifier outright. It stays as the universal fallback.
- Per-tool granular progress (PreToolUse/PostToolUse spam). Out of scope.
- A wsx MCP server. None exists today; CLI is the entry point (see Findings).

## Findings that shape the design

These came out of reading the spawn path, store, and CLI dispatch.

1. **wsx already injects an inline settings JSON.** The Claude spawn passes
   `--settings '{"fastMode":true}'` (`src/pty/session.rs:860`). Claude Code's
   `--settings` accepts an inline `hooks` block, so wsx can wire deterministic
   hooks by **extending JSON it already sends** — no `.claude/settings.json`
   file injection into the worktree is required.

2. **wsx already injects identity env vars at spawn.**
   `WSX_WORKSPACE_ID` and `WSX_AGENT_INSTANCE_ID` are set on the child
   (`src/pty/session.rs:1267-1268`), and the full parent env is inherited
   (`session.rs:751-752`). So a `wsx status` invocation — whether from a hook
   or from the agent directly — already knows which workspace and agent
   instance it is, with no arguments.

3. **No wsx MCP server exists.** `src/agent/mcp.rs` only *mirrors* the user's
   MCP servers into worktree config; wsx exposes zero MCP tools. Status
   reporting must be a CLI verb (or env/file), not an MCP tool.

4. **The store is WAL with no `busy_timeout`** (`src/data/store.rs:94`). A
   `wsx status` CLI write happens in a separate process while the TUI may also
   be writing; WAL permits one writer, and with no busy_timeout a contended
   write fails immediately with `SQLITE_BUSY`. The agent-facing path must set a
   `busy_timeout`.

5. **Migrations re-run every startup.** `migrate()` executes `SCHEMA_V1` each
   launch (which resets `user_version` to 1), so every `ALTER`/backfill must be
   gated behind a `pragma_table_info()` column-existence check, not just
   `if v < N` (`src/data/store.rs`; matches the known wsx migration gotcha).

6. **The skill is delivered, the system prompt is appended.** `SKILL.md` is
   embedded at compile time and installed to `~/.claude/skills/wsx/` (and the
   hermes/codex equivalents) via `src/agent/skill.rs`. The process doctrine is
   appended to the system prompt with `--append-system-prompt`
   (`src/pty/session.rs:874-876`). Both are existing, controlled injection
   points for telling the agent to report.

## Architecture: three tiers, one classifier

The central design decision is **not** to build a separate arbitration layer.
Instead, thread the pushed state into the existing `Status::classify()` as a
new, freshness-gated, high-priority input. The current liveness/stall machinery
(`last_log_activity_ms`, the 60s stall detector) then governs decay for free.

Three sources feed the same store columns; `classify()` consumes the freshest
trustworthy one and falls back down the tiers:

```
Tier 1  Model push     wsx status set blocked --message "need your call on auth"
                        Richest: carries a semantic message. Requires the model
                        to remember (skill + system prompt nudge it).

Tier 2  Hook push       UserPromptSubmit → working
                        Stop            → done
                        Notification    → waiting
                        Deterministic, fires regardless of model cooperation.
                        Claude-only (other agents have no equivalent hooks).

Tier 3  JSONL heuristic  existing Status::classify() inputs
                        Universal fallback for pi / hermes / codex, and for any
                        moment no fresh push exists.
```

Per-agent degradation is graceful: **Claude** gets tiers 1+2+3; **pi / hermes /
codex** get tiers 1+3 (they can still call `wsx status` from the skill, they
just have no hook backbone).

### Data flow

```
agent / hook  ──▶  wsx status set <state> [--message ..]   (separate process)
                        │ writes (reported_state, reported_message, reported_at)
                        ▼
                  SQLite store  (workspaces row, keyed by WSX_WORKSPACE_ID)
                        │
   TUI poll loop (~2s)  │ reads the row alongside WorkspaceEvents
                        ▼
                  Status::classify(.. reported_state, reported_at ..)
                        │ pushed state wins while fresh; else heuristic
                        ▼
                  dashboard render + bell transitions (unchanged downstream)
```

## Component changes (described, not implemented)

### 1. CLI verb — `wsx status set`

```
wsx status set <state> [--message <text>] [--workspace <id>] [--agent <instance>]
wsx status clear        [--workspace <id>] [--agent <instance>]
```

- `<state>` ∈ `working | waiting | blocked | done` (the agent-facing
  vocabulary; see *Fork A*).
- `--workspace` defaults to `$WSX_WORKSPACE_ID`; `--agent` defaults to
  `$WSX_AGENT_INSTANCE_ID`. The common call is therefore just
  `wsx status set working --message "running tests"`.
- `clear` resets the pushed state (e.g. the agent explicitly relinquishes to
  the heuristic). Optional for v1.

Dispatch: add a `status` group in `parse_args()` (`src/cli.rs:444`) routing to
a `parse_status()` returning a new `CliAction::StatusSet { .. }`, handled in
`run_cli()`. The handler opens the store with a `busy_timeout` set (Finding 4)
and updates the workspace (or `workspace_agents`) row.

### 2. Store schema — V15

Add to `workspaces` (and mirror onto `workspace_agents` for multi-agent
per-instance status):

| Column | Type | Meaning |
|--------|------|---------|
| `reported_state` | TEXT | last agent-pushed state, or NULL |
| `reported_message` | TEXT | optional one-liner, or NULL |
| `reported_at` | INTEGER | epoch ms of the push (freshness/arbitration) |
| `reported_source` | TEXT | `model` \| `hook` — for precedence + debugging |

Gated migration following the V4 `yolo` example: a `pragma_table_info()` count
guard per column inside an `if v < 15` block, then bump `PRAGMA user_version`.

### 3. Classifier input — `Status::classify()`

Extend the input struct with `reported_state: Option<ReportedState>` and
`reported_at_ms`. In the priority ladder, a fresh pushed state slots near the
top but **below** the hard signals it can't override safely:

- A fresh `blocked` → `Question`.
- A fresh `done` → `Complete`.
- A fresh `working` → `Thinking` — *but* the existing stall detector still
  applies, so a `working` push with no JSONL growth for 60s decays to
  `Stalled` automatically. This is the key reason to fold into `classify()`
  rather than special-case: stale "working" self-heals through machinery that
  already exists.
- A fresh `waiting` → `Waiting`.

"Fresh" is defined by arbitration (*Fork B*).

### 4. Hook wiring — extend the `--settings` JSON

For Claude, augment the inline settings already passed at `session.rs:860`:

```json
{
  "fastMode": true,
  "hooks": {
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "wsx status set working" }] }],
    "Stop":             [{ "hooks": [{ "type": "command", "command": "wsx status set done" }] }],
    "Notification":     [{ "hooks": [{ "type": "command", "command": "wsx status set waiting" }] }]
  }
}
```

The hook command relies on `$WSX_WORKSPACE_ID` being present in the hook's
environment (Finding 2 makes this likely, since hooks inherit the session env —
but see *Spike-validation items*). Hooks supply only coarse state, never a
`--message`; the message is the model-push tier's job.

### 5. SKILL.md + doctrine

- Add a short "Reporting your status" section to `skills/wsx/SKILL.md`
  documenting `wsx status set` and *when* to call it: on starting substantive
  work, when blocking on a user decision, and on finishing — always with a
  one-line `--message`.
- Optionally add one line to the process doctrine
  (`src/agent/doctrine.rs`) so non-Claude agents (no hooks) still get the
  nudge in their system prompt.

## Fork A — vocabulary: state + optional message (recommended)

Two options were on the table:

- **State only.** Agent reports one of the existing display states; minimal
  surface, zero new rendering.
- **State + message (recommended).** Agent reports a small intent vocabulary
  *and* an optional freeform one-liner.

**Recommendation: state + message.** The message is the highest-value part of
the whole feature. Today the dashboard approximates "what's it doing" with the
longest assistant text of the turn; a deliberate agent-authored line
("running the integration suite", "blocked on which auth library") is strictly
better and nearly free to store and render in the detail pane.

Keep the agent-facing vocabulary small and intent-shaped, distinct from the six
*display* states (`idle` and `stalled` remain wsx-inferred — an agent never
reports itself idle or stalled):

| Agent reports | Maps to display state |
|---------------|-----------------------|
| `working` | Thinking |
| `waiting` | Waiting |
| `blocked` | Question |
| `done` | Complete |

## Fork B — arbitration: liveness-gated (recommended) vs TTL

When is a pushed state still trustworthy?

- **Option 1 — blind TTL.** Pushed state wins for N seconds after
  `reported_at`, then the heuristic takes over. Dead simple, crash-safe,
  but introduces a magic constant and can flap right at the boundary.
- **Option 2 — JSONL-liveness gate (recommended).** A push stays authoritative
  until JSONL activity *after* `reported_at` contradicts it. wsx already stamps
  `last_log_activity_ms` on every JSONL growth, so this reuses existing
  machinery: if the agent pushed `done` and then a new assistant turn appears
  in the JSONL, the heuristic re-arms automatically; until then, `done` holds.

**Recommendation: liveness-gated, with the existing stall detector as the
safety backstop.** Pure liveness alone would get stuck if an agent pushes
`working` and then crashes with no further JSONL — but folding the pushed state
into `classify()` (Component 3) means the 60s stall detector already converts
that into `Stalled`. So we get crash-safety without a bespoke TTL. A small
freshness floor (e.g. ignore a push whose `reported_at` is older than the
session's own start) guards against stale rows after session rotation.

## Spike-validation items (verify before/while building)

These are assumptions the design rests on that should be confirmed live, not
taken on faith:

1. **Hook env inheritance.** Confirm Claude Code hook commands actually see
   `$WSX_WORKSPACE_ID` in their environment. If not, the hook command must be
   templated with the literal id at spawn time (wsx knows it), e.g.
   `wsx status set working --workspace <id>`.
2. **Hook → state fidelity.** Verify `Stop` vs `Notification` cleanly separate
   "done" from "waiting on you / needs permission". If `Stop` fires for both
   turn-end and question-end, tier 2 can only assert "not working"; the
   question/done distinction then leans on tier 1 (model push) or tier 3
   (the existing `?`-suffix heuristic). Worth a short manual probe.
3. **`busy_timeout`.** Set one on the agent-facing store open and confirm a
   `wsx status` write doesn't `SQLITE_BUSY` against a live TUI under rapid
   pushes.
4. **Hook latency/noise.** `UserPromptSubmit` → `working` on every prompt is
   fine, but confirm the hooks don't add perceptible input latency or clutter.

## Scope summary

Described in this doc; to be implemented in a follow-up once validated:

- Store migration V15 (gated column adds: `reported_state`,
  `reported_message`, `reported_at`, `reported_source`).
- `wsx status set` / `wsx status clear` CLI verb with env-var defaults and a
  `busy_timeout` on its store open.
- `Status::classify()` extended to consume the pushed state, freshness-gated.
- Hook block added to the inline `--settings` JSON for Claude spawns.
- `SKILL.md` "Reporting your status" section + optional doctrine line.

The existing JSONL classifier is untouched as a behavior — it simply becomes the
bottom tier of a three-tier stack.
