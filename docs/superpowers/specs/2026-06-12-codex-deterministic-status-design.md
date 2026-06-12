# Codex deterministic status reporting via `notify` — design

**Status:** Design (this document). Follow-on to the agent-driven status work in
[`2026-06-11-agent-driven-status-design.md`](2026-06-11-agent-driven-status-design.md)
(issue [#166](https://github.com/bakedbean/workspacex/issues/166), PR #167),
which built the three-tier status stack and explicitly left per-harness tier-2
integrations as pluggable follow-ups. This spec is the Codex tier-2 integration.

## Problem

The agent-driven status work made Claude Code's status **deterministic** by
wiring lifecycle hooks (`UserPromptSubmit`, `PreToolUse`, `Notification`, `Stop`)
through the inline `--settings` JSON into `wsx status from-hook`. Those pushes
land in the `workspace_status` table (the "reported" lane) and are folded into
`Status::classify()`, freshness-gated by `fresh_reported()` so the heuristic
re-arms once new JSONL activity appears after a push.

Every other agent kind — including **Codex** — currently maps to `NoopStatus`
(`src/agent/status/mod.rs`, `for_agent()`), so it gets only **tier 1** (manual
`wsx status set` from the skill) and **tier 3** (the `sessionx` JSONL heuristic
that tails `~/.codex/sessions/**/rollout-*.jsonl`). There is no deterministic
backbone: "is this Codex workspace done / waiting on me?" is inferred from a
quiescent transcript file rather than signalled by the agent's runtime.

We want to give Codex the same deterministic turn-end signal Claude gets from its
`Stop` hook, without re-architecting how wsx runs Codex.

## Goals

- Make Codex's **turn-end states (Done / Blocked-on-question) deterministic**,
  landing in the same `reported` lane and `classify()` path Claude uses.
- Reuse the existing storage, classifier, freshness gate, and
  `StatusIntegration` trait verbatim — no new arbitration machinery.
- Keep the JSONL heuristic as the universal floor; never make status a hard
  dependency on Codex cooperation.

## Non-goals

- Full lifecycle parity with Claude (turn-start, structured-question, and
  approval-blocked events). Codex's PTY surface cannot deliver these reliably at
  v0.137 — see *Findings*. Those stay heuristic by design.
- Re-architecting wsx to drive Codex headlessly (app-server / `exec --json`).
  Captured as a future option below, explicitly out of scope here.
- Any change to the `workspace_status` schema, `Status::classify()`, the
  freshness gate, or the JSONL heuristic.

## Findings that shape the design

Empirically verified against the installed **Codex CLI v0.137.0**, cross-checked
against the codex-rs source at tag `rust-v0.137.0`.

1. **Codex's `hooks.*` system is NOT usable for wsx worktrees.** Codex shipped a
   near-1:1 port of Claude's hooks (`SessionStart`, `UserPromptSubmit`,
   `PreToolUse`, `Stop`, `PermissionRequest`, …), and `-c 'hooks.Stop=[...]'`
   *parses* and is structurally wired. But discovery gates it behind project
   **trust** + **layer-enable** (`include_disabled=false`) + **managed-only**
   policy, plus open reliability bugs (#17532, #21639). wsx worktrees live under
   `~/.local/state/wsx/worktrees/...`, **outside** the user's trusted project
   roots — the exact silent-drop condition. **Verified:** a clean `codex exec`
   turn with `-c hooks.UserPromptSubmit/Stop` **and**
   `--dangerously-bypass-hook-trust` fired **neither** hook. `hooks.*` is out.

2. **`notify` IS reliable and `-c`-injectable.** Codex's `notify` program is a
   plain string array (`config_toml.rs`), registered as a *legacy after-agent
   hook* **independently of the `CodexHooks` feature flag**, dispatched from the
   shared turn loop (`core/src/session/turn.rs`) in **both** the TUI and exec.
   It has **no trust gate**. **Verified:** `-c 'notify=["/path/shim"]'` fired on
   `agent-turn-complete` with this argv payload:

   ```json
   {"type":"agent-turn-complete","thread-id":"…","turn-id":"…","cwd":"/tmp",
    "client":"codex_exec","input-messages":["…"],"last-assistant-message":"pong"}
   ```

   Note: kebab-case keys; payload arrives as the **final argv element** (not
   stdin); carries `cwd` and `last-assistant-message`. Its one limitation is that
   it fires **only** on `agent-turn-complete` — no turn-start, no
   approval-blocked event.

3. **The status architecture is already a layered hybrid that composes
   cleanly.** `App::*` builds `Status::classify(awaiting, stopped_kind, stalled,
   …, reported)` where `reported = fresh_reported_state(pushed_status, last_log_activity)`
   (`src/app.rs:618,699`). `fresh_reported()` keeps a push authoritative **only
   until JSONL activity appears strictly after `reported_at`**, then the
   heuristic re-arms. So a `notify`-driven Done sits in the `reported` lane and
   is automatically superseded the moment Codex acts again — **no race, no new
   precedence logic.** The heuristic continues to supply Working / stalled /
   awaiting.

4. **`StatusIntegration` is the designed extension point.** The trait
   (`src/agent/status/mod.rs`) has `parse_event(json) -> Option<ReportedState>`
   and `spawn_wiring(wsx_bin, fast_mode) -> Option<SpawnWiring>`. `from-hook`
   already dispatches by `--agent` kind via `for_agent()`. The prior spec's
   cross-harness section names this exact path for adding Codex.

5. **Workspace resolution already works for a `notify` subprocess.** wsx sets
   `WSX_WORKSPACE_ID` on the spawned agent child (`src/pty/session.rs:1285`); the
   `notify` program inherits Codex's environment, so `resolve_current_workspace()`
   (env-first, `src/cli.rs:1500`) resolves with no arguments. The payload `cwd`
   is an available fallback.

## Architecture

One sentence: **route Codex's `notify` `agent-turn-complete` into the same
`reported` lane Claude's `Stop` hook uses, mapping it to Done / Blocked exactly
as Claude does.** Tier 2 for Codex is *partial* — it covers the `Stop`-equivalent
turn-end only — and tiers 1 + 3 are unchanged.

```
codex (PTY/TUI)  ──fires notify on agent-turn-complete──▶
    wsx status from-notify --agent codex '<argv JSON>'   (separate process)
        │ CodexStatus::parse_event → Done | Blocked
        │ resolve_current_workspace (WSX_WORKSPACE_ID, cwd fallback)
        ▼
    set_workspace_status(.., source="notify")   →  workspace_status table
        │
        ▼  (existing) fresh_reported gate + Status::classify
    dashboard  — Done/Blocked authoritative until Codex acts again,
                 then tier-3 heuristic re-arms (Working etc.)
```

Per-state coverage after this change:

| Display state | Source for Codex |
|---|---|
| **Done / Blocked** (turn end) | **`notify` — deterministic (new)** |
| Working (Thinking) | tier-3 heuristic (JSONL tail), re-armed via `fresh_reported` |
| Waiting / Stalled | tier-3 heuristic |
| Approval prompt blocked | tier-3 heuristic (invisible to `notify`) |
| (manual override, any state + message) | tier-1 `wsx status set` |

## Component changes

### 1. `CodexStatus` integration — `src/agent/status/codex.rs` (new)

Implements `StatusIntegration`, mirroring `claude.rs`'s `Stop` branch against
Codex's payload keys:

- `parse_event(json)`:
  - if `json["type"] == "agent-turn-complete"`: read `last-assistant-message`
    (kebab-case); `Blocked` if its trimmed text ends with `?`, else `Done`.
  - otherwise `None`.
- `spawn_wiring(wsx_bin, _fast_mode)`: returns
  `SpawnWiring { args: ["-c", "notify=[\"<wsx_bin>\",\"status\",\"from-notify\",\"--agent\",\"codex\"]"] }`.
  The `wsx_bin` path is TOML-string-escaped (quotes/backslashes). Codex appends
  the JSON payload as the final argv element at fire time, so the invoked command
  is `wsx status from-notify --agent codex '<json>'`.

### 2. Routing — `src/agent/status/mod.rs`

`for_agent(AgentKind::Codex) => &CODEX` (a `static CODEX: CodexStatus`), replacing
the current `&NOOP` for Codex. Other agents unchanged.

### 3. New CLI verb — `wsx status from-notify --agent <kind>` (`src/cli.rs`)

The `notify` counterpart to `from-hook`. Difference: `notify` passes its payload
as the **last positional argv**, not stdin.

- Parse `from-notify` in `parse_status()` → a `CliAction::StatusFromNotify { agent }`.
- Handler: take the **last positional arg**, parse as JSON; resolve the workspace
  via `resolve_current_workspace` (env-first, payload `cwd` fallback); call
  `for_agent(kind).parse_event(json)`; on `Some(state)` write
  `set_workspace_status(ws.id, state, None, "notify")`. **Always exit 0** — a
  status sink must never fail a turn (matches `from-hook`).

### 4. Spawn wiring — Codex spawn path (`src/pty/session.rs`)

Append `for_agent(ws.agent).spawn_wiring(wsx_bin, fast_mode)` args to the Codex
launch, the same generic hook point Claude uses for `--settings`. If the call
site is currently Claude-specific, lift it to dispatch by agent kind so any
integration's `spawn_wiring()` is honoured. No other Codex spawn changes.

## Testing

- **Unit — `CodexStatus::parse_event`** (mirror `claude.rs` tests):
  `agent-turn-complete` with a plain message → `Done`; ending in `?` → `Blocked`;
  a non-`agent-turn-complete` `type` → `None`; missing `last-assistant-message`
  → `Done` (degrade, not panic).
- **Unit — `spawn_wiring`**: asserts the `-c notify=[…]` array shape and correct
  TOML escaping of a bin path containing a space/quote.
- **Unit — `from-notify` argv parse**: last-arg JSON extraction; malformed/absent
  JSON is a no-op that still exits 0.
- **Live verification (one step in the plan):** spawn a real Codex workspace and
  confirm `notify` fires in the **interactive TUI** and the dashboard flips to
  Done at turn end. `notify` is source-confirmed to dispatch in the TUI (shared
  `run_turn`); this was proven live only under `exec`, so a TUI confirmation is
  the single assumption to validate before calling it done.

## Caveats (documented behavior)

- **`notify` override.** `-c notify=[...]` replaces any user-configured `notify`
  for **wsx-spawned Codex sessions only** (ephemeral, managed). The user's global
  `~/.codex/config.toml notify` is untouched for their own sessions. Acceptable;
  noted so it isn't surprising.
- **Partial tier 2.** Only turn-end is deterministic. Turn-start (Working),
  stall/waiting, and approval-blocked remain tier-3 heuristic. The freshness gate
  makes the handoff seamless.
- **Prose-question disambiguation** uses the `last-assistant-message` `?`-suffix
  check — the same best-effort heuristic Claude's `from-hook` uses. Tier 1/tier 3
  refine it if wrong.
- **Version pin.** Behavior verified at Codex v0.137.0. `notify` is a
  long-stable, documented API (far less churn-prone than `hooks.*`), but the live
  TUI check should be re-run on major Codex bumps.

## Verification result (2026-06-12, Codex v0.137.0)

The implementation was verified end-to-end against the real wsx binary and DB:

- **`from-notify` CLI chain** — `wsx status from-notify --agent codex '<json>'`
  against the live store correctly wrote `done`/`source=notify` for a plain
  `agent-turn-complete`, `blocked`/`source=notify` for a `?`-terminated message,
  and was a no-op (exit 0, state unchanged) for malformed JSON and non-turn
  events.
- **Full chain under `codex exec`** — a real `codex` turn launched with the exact
  `-c notify=["…/wsx","status","from-notify","--agent","codex"]` arg that
  `build_codex_command` emits flipped the workspace from a `working`/`model`
  sentinel to `done`/`notify`, i.e. Codex itself fired `notify` and drove the
  push through the real binary.
- **Residual (human, low-risk):** the same flow inside the interactive **TUI**
  was not click-tested. Source analysis confirms `notify` dispatches from the
  shared `core/src/session/turn.rs::run_turn` loop used by both `exec` and the
  TUI, so this is expected to behave identically; re-confirm on major Codex bumps.

## Future option (out of scope)

For full deterministic parity — turn-start, structured questions, and
approval-blocked as first-class events — Codex's **app-server v2 JSON-RPC** (or
`codex exec --json`) emits `TurnStarted` / `TurnCompleted` / `HookStarted` /
approval requests directly. Adopting it would require wsx to **drive** Codex as a
protocol server rather than spawn the interactive TUI in a PTY — effectively a
new integration, and a much larger change. Recorded here so the ceiling is known
if wsx ever moves to a headless-driven model.

## Scope summary

- `src/agent/status/codex.rs` — new `CodexStatus` (`parse_event` + `spawn_wiring`).
- `src/agent/status/mod.rs` — route `AgentKind::Codex` to `CodexStatus`.
- `src/cli.rs` — `wsx status from-notify --agent <kind>` verb (argv-JSON sink).
- `src/pty/session.rs` — append `spawn_wiring()` args to the Codex launch.
- Unit tests as above; one live TUI verification.

No changes to the `workspace_status` schema, `Status::classify()`, the freshness
gate, or the JSONL heuristic — Codex simply gains a deterministic turn-end push
in the lane those layers already consume.
