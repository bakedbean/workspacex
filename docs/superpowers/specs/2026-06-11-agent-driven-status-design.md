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

Tier 2  Hook push       UserPromptSubmit                          → working
                        PreToolUse[AskUserQuestion,ExitPlanMode]  → blocked (structured Q)
                        Notification[permission_prompt]           → blocked (needs perm)
                        Notification[idle_prompt]                 → waiting
                        Stop                                      → turn ended → done*
                        Deterministic, fires regardless of model cooperation.
                        Claude-only (other agents have no equivalent hooks).
                        *Stop can't tell a prose question from a completion on
                        its own — see Fidelity findings; that edge falls to
                        tier 1 or tier 3.

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

For Claude, augment the inline settings already passed at `session.rs:860`.
The fidelity probe (below) showed a richer, more capable mapping than the
original three-hook sketch:

```json
{
  "fastMode": true,
  "hooks": {
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "wsx status set working" }] }],
    "PreToolUse": [
      { "matcher": "AskUserQuestion", "hooks": [{ "type": "command", "command": "wsx status set blocked" }] },
      { "matcher": "ExitPlanMode",    "hooks": [{ "type": "command", "command": "wsx status set blocked" }] }
    ],
    "Notification": [
      { "matcher": "permission_prompt", "hooks": [{ "type": "command", "command": "wsx status set blocked" }] },
      { "matcher": "idle_prompt",       "hooks": [{ "type": "command", "command": "wsx status set waiting" }] }
    ],
    "Stop": [{ "hooks": [{ "type": "command", "command": "wsx status from-hook" }] }]
  }
}
```

Notes:

- The hook command relies on `$WSX_WORKSPACE_ID` in the hook's environment —
  **validated**, see Finding 2 / Spike-validation item 1.
- Hooks supply state only, never a `--message`; the message is the model-push
  tier's job.
- `Stop` uses `wsx status from-hook` (reads the hook JSON on stdin) rather than
  a fixed `set done`, because the Stop event alone can't separate a prose
  question from a completion. `from-hook` can apply the best-effort
  `last_assistant_message` `?`-suffix check (undocumented field — degrade to
  plain `done` if absent) and defer the rest to tier 3. Equivalently, map
  `Stop → done` and let tier-3 `classify()` refine done→question; pick one in
  implementation.
- **Hooks block the turn.** Command-hook default timeout is 10 min, but
  `UserPromptSubmit` is capped at 30s. `wsx status` must be fast (a single
  indexed SQLite write with `busy_timeout`). If latency is ever a concern, the
  hook can background the write (`wsx status … & exit 0`), though async command
  hooks aren't a documented guarantee — prefer just keeping the write cheap.

### 5. SKILL.md + doctrine

- Add a short "Reporting your status" section to `skills/wsx/SKILL.md`
  documenting `wsx status set` and *when* to call it: on starting substantive
  work, when blocking on a user decision, and on finishing — always with a
  one-line `--message`.
- Optionally add one line to the process doctrine
  (`src/agent/doctrine.rs`) so non-Claude agents (no hooks) still get the
  nudge in their system prompt.

## Cross-harness extensibility (tier 2 is pluggable)

The three tiers split cleanly by how harness-specific they are, and the design
is deliberately structured so other coding harnesses (Codex, Pi, Hermes, …) can
adopt the deterministic path when their mechanism is known.

- **Tier 1 (model push) and ALL storage/classifier infrastructure are
  harness-agnostic.** `wsx status set` is a plain CLI call any agent that can
  run a shell command makes; the `workspace_status` table, the `ReportedState`
  vocabulary, the `classify()` reported input, and the freshness gate are
  shared by every agent kind. The doctrine clause nudges every agent
  (claude/pi/hermes/codex) to use it. So the moment this ships, *every* harness
  already has a working push path — no per-harness work required for tier 1.

- **Tier 2 (deterministic events) is the only harness-specific layer, and it is
  pluggable.** Claude's mechanism is hooks wired via `--settings`; another
  harness might surface lifecycle events differently (a config file, a callback
  flag, an emitted event stream). This is captured behind a `StatusIntegration`
  trait with two responsibilities:
  1. `spawn_wiring()` — emit whatever spawn-time configuration that harness
     needs in order to call back into `wsx status` on its lifecycle events.
  2. `parse_event()` — interpret that harness's event payload into a
     `ReportedState`.
  Claude is the first implementation (`ClaudeStatus`); every other agent maps to
  a `NoopStatus` (tier 1 + tier 3 only) until its mechanism is built.

**To add Codex/Pi/Hermes deterministic reporting later:** implement
`StatusIntegration` for that agent and call its `spawn_wiring()` from that
agent's spawn builder. Nothing in storage, the CLI surface, the classifier, or
the freshness gate changes — that shared infrastructure was built for exactly
this. `wsx status from-hook --agent <kind>` already dispatches to the right
`parse_event` by agent kind, so the event-ingestion entry point is also
harness-neutral.

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

1. **Hook env inheritance. — ✅ VALIDATED 2026-06-11.** Confirmed empirically:
   a nested `claude -p` (v2.1.173) launched with the hook wired through the
   inline `--settings` JSON, and `WSX_WORKSPACE_ID`/`WSX_AGENT_INSTANCE_ID`
   exported on the parent with sentinel values, ran both `UserPromptSubmit` and
   `Stop` hook commands with those exact sentinel values visible in the hook
   subprocess's `printenv`. Claude Code passes its full environment through to
   hook commands, so `wsx status set working` can read `$WSX_WORKSPACE_ID`
   directly — no spawn-time templating needed. Bonus: Claude Code also sets
   `CLAUDE_PROJECT_DIR` in the hook env, a second usable anchor. (The
   templating fallback — `wsx status set working --workspace <id>` — is
   therefore unnecessary, but remains available if ever needed.)
2. **Hook → state fidelity. — ✅ PROBED 2026-06-11.** See *Fidelity findings*
   below. Outcome: structured questions and permission blocks ARE
   deterministically separable (via `PreToolUse` and `Notification`
   `notification_type`); only a *prose* question vs a completion is
   indistinguishable at `Stop` and falls to tier 1/tier 3. Residual item now
   tracked as #5.
5. **PreToolUse[AskUserQuestion] vs Stop ordering (NOT yet validated).**
   Confirm that when the agent calls `AskUserQuestion`/`ExitPlanMode`, `Stop`
   does **not** also fire while the tool is pending (which would clobber the
   `blocked` push with a `done`). Logically it shouldn't — the agent is
   awaiting a tool result, not "finished responding" — but this needs an
   interactive check (can't be driven in `-p`). If `Stop` does fire, the
   `from-hook`/`classify()` resolution must treat a just-pushed `blocked` as
   sticky until the user replies.
3. **`busy_timeout`.** Set one on the agent-facing store open and confirm a
   `wsx status` write doesn't `SQLITE_BUSY` against a live TUI under rapid
   pushes.
4. **Hook latency/noise.** `UserPromptSubmit` → `working` on every prompt is
   fine, but confirm the hooks don't add perceptible input latency or clutter.

## Fidelity findings (probed 2026-06-11, claude 2.1.173)

Two methods: an empirical hook-capture probe (nested `claude -p` with hooks
wired via `--settings`, logging which hook fired and its full stdin payload
across a completion turn vs. a question-ending turn), and an authoritative
docs check of firing conditions and payloads.

**Empirically observed:**

- A plain-completion turn and a prose-question turn produced the *identical*
  hook sequence: `UserPromptSubmit` then `Stop`. No `Notification` fired for a
  prose question. So **`Stop` alone cannot separate done from a prose
  question.**
- The `Stop` stdin payload carried `last_assistant_message` — `"done"` for the
  completion, and the full `?`-terminated text for the question. This is a
  synchronous, in-hook disambiguator for the prose case *if* relied upon — but
  it is **not in the documented schema** (the documented `Stop` fields are
  `session_id`, `transcript_path`, `cwd`, `permission_mode`, `effort`,
  `hook_event_name`, `stop_reason`, `tool_calls`). Treat as best-effort.

**Authoritative (documented) semantics that shape the mapping:**

- **`Notification`** fires with a `notification_type` discriminator and a
  `message` field; the relevant types are `permission_prompt` (→ blocked,
  needs permission) and `idle_prompt` (→ waiting). Cleanly separable.
- **`PreToolUse`** supports a per-tool `matcher`, so `AskUserQuestion` and
  `ExitPlanMode` invocations fire deterministically with the tool name —
  the strong signal for structured "blocked / asking" states.
- **`Stop`** fires on every normal turn end (completion *and* prose question)
  but **not on user interrupts**. `stop_reason` is `end_turn` for both a
  completion and a prose question, so it doesn't disambiguate either.
- **`Stop` vs `SubagentStop`:** key off `Stop` (main agent); `SubagentStop` is
  Task-tool subagents only.
- **Blocking/timeout:** command hooks run synchronously and block the turn;
  default timeout 10 min, but `UserPromptSubmit` is capped at 30s. Keep
  `wsx status` cheap.

**Net effect on the design:** tier 2 is *more* capable than the original
sketch — structured questions (`PreToolUse`) and permission blocks
(`Notification`) are deterministic. The only irreducibly ambiguous case is a
**prose question vs a completion**, for which no documented hook exists; it is
resolved by tier 1 (the model knows it asked and pushes `blocked`) or tier 3
(the existing transcript `?`-suffix / pending-`AskUserQuestion` heuristic in
`classify()`), with the undocumented `last_assistant_message` available as a
best-effort in-hook shortcut.

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
