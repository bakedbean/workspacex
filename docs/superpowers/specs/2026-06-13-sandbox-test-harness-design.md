# Sandbox extraction + thin e2e test harness — design

**Date:** 2026-06-13
**Status:** Approved (pending spec review)

## Problem

The `demo/` directory bundles two unrelated concerns:

1. **Sandbox provisioning** — standing up a fully isolated, live wsx install:
   isolated state.db + synthetic repos with planted bugs + pre-authed/pre-trusted
   Claude & Codex configs + the wsx agent skill + session-log bridging into
   `~/.claude`. This is exactly what a future agent needs to *run the real app for
   testing*.
2. **Screencast production** — VHS tapes, dead-air collapse, speed-ramping,
   caption burn-in, and the ≤10MB budget.

Because the reusable provisioning half lives inside a directory named `demo/`, its
value as an end-to-end testing substrate is hidden. This work separates the two so
the sandbox is a first-class, discoverable, reusable unit, and adds a thin
agent-facing harness that drives the live app headlessly for testing.

## Decisions (from brainstorming)

- **Scope:** refactor + a *thin* harness (a "spin up a live sandboxed wsx and drive
  it" entrypoint), not a full e2e suite. One worked-example smoke test is included.
- **Drive mode:** headless CLI + state inspection — run `wsx` CLI commands against
  the sandbox and assert on `state.db` / session logs. No TUI rendering, no video.
- **Agent provisioning:** the shared sandbox keeps doing *everything* bootstrap does
  today, including live-agent auth/skill/trust/session-bridge. No core/opt-in split.
  (Bootstrap already degrades gracefully — it `WARN`s and continues when real
  credentials are absent.)
- **Layout:** a shared top-level `sandbox/`; `demo/` (screencast) and `test/` (e2e
  harness) are sibling consumers.
- **Rename:** `WSX_DEMO_ROOT` → `WSX_SANDBOX_ROOT`, with `WSX_DEMO_ROOT` honored as a
  fallback so nothing breaks.

## Target layout

```
sandbox/                 # shared: "stand up a real wsx in full isolation"
  bootstrap.sh           # was demo/sandbox-bootstrap.sh — behavior unchanged
  gen-repos.sh           # moved verbatim
  agent-env.sh           # NEW — CLAUDECODE/CLAUDE_CODE_* clearing, extracted from render.sh
  env.sh                 # NEW — sourceable: re-enter an already-provisioned sandbox
  test-bootstrap.sh      # moved (paths/env-var updated)
  test-gen-repos.sh      # moved
  README.md              # NEW — documents the env contract + provision/enter/clean flow

demo/                    # screencast-only (consumes sandbox/)
  render.sh              # now sources sandbox/agent-env.sh, then execs vhs
  deadair.sh speedramp.sh post.sh
  tapes/ captions/ Makefile SPIKE-NOTES.md
  test-post.sh test-speedramp.sh README.md

test/                    # NEW thin e2e harness (consumes sandbox/)
  harness.sh             # agent-facing entrypoint: up / wsx / state / down
  smoke.sh               # one worked example = the harness's own self-test
  README.md
```

Only `docs/superpowers/plans/*` reference the old `demo/` paths, and those are dated
historical records — they are intentionally left unchanged. Every other reference is
`demo/`-internal and gets updated.

## Component details

### `sandbox/bootstrap.sh` (was `demo/sandbox-bootstrap.sh`)

Behavior is preserved verbatim except for two parameterizations and the rename:

- **`WSX_SANDBOX_ROOT`** replaces `WSX_DEMO_ROOT` as the primary env var. Resolution
  order: `WSX_SANDBOX_ROOT` if set, else `WSX_DEMO_ROOT` if set (back-compat), else
  the default. The existing destructive-path guards (`""`/`/`/`//`/`$HOME`) apply to
  the resolved value.
- **`WSX_BIN`** (default `wsx` from PATH) — every `wsx …` invocation in bootstrap
  (`repo add`, `repo set-base-branch`) goes through `"$WSX_BIN"`. This lets a
  consumer point provisioning at a locally-built binary so tests exercise local
  changes, not the installed wsx.
- All other behavior — isolated `XDG_STATE_HOME`/`CLAUDE_CONFIG_DIR`/`CODEX_HOME`,
  credential copy with WARN-on-missing, skill install, per-repo/per-worktree trust
  pre-seeding, and the `~/.claude/projects` session-log symlink bridge — is unchanged.

### `sandbox/agent-env.sh` (NEW)

Single source of truth for the parent-session markers that must be cleared so a
spawned agent runs as a genuine top-level session (and persists its per-worktree
session jsonl). Today this list is hardcoded in `demo/render.sh`. Shape: a sourceable
script exposing the marker list (e.g. an `WSX_AGENT_ENV_UNSET` array and/or a
`wsx_clear_agent_env` helper) so both `demo/render.sh` and `test/harness.sh` clear the
same set without drift. Markers: `AI_AGENT`, `CLAUDECODE`, `CLAUDE_EFFORT`,
`CLAUDE_CODE_ENTRYPOINT`, `CLAUDE_CODE_EXECPATH`, `CLAUDE_CODE_SESSION_ID`,
`CLAUDE_CODE_CHILD_SESSION`.

### `sandbox/env.sh` (NEW)

`source sandbox/env.sh` exports the four sandbox env vars
(`WSX_SANDBOX_ROOT`, `XDG_STATE_HOME`, `CLAUDE_CONFIG_DIR`, `CODEX_HOME`) for an
*already-provisioned* sandbox, deriving the latter three from `WSX_SANDBOX_ROOT`
exactly as bootstrap does. Lets a consumer re-enter a sandbox without re-bootstrapping
(and without duplicating the derivation logic). Does not provision or wipe anything.

### `sandbox/README.md` (NEW)

Documents the unit's interface: the env contract (the four vars + `WSX_BIN`), how to
provision (`bootstrap.sh`), how to re-enter (`env.sh`), what gets written outside the
sandbox (only the transient `~/.claude/projects` symlinks) and how it's cleaned, and
that `gen-repos.sh` defines the synthetic repos / planted bugs.

### `demo/` changes (screencast-only)

- `render.sh` sources `sandbox/agent-env.sh` and clears the markers via the shared
  helper instead of the hardcoded `env -u …` list, then `exec vhs "$tape"`.
- `Makefile`: recipes call `sandbox/bootstrap.sh` (from `$(ROOT)`) instead of
  `demo/sandbox-bootstrap.sh`; the `clean` recipe's `WSX_DEMO_ROOT` references become
  `WSX_SANDBOX_ROOT` (still honoring the fallback); the `check` target runs only the
  screencast tests (`test-post.sh`, `test-speedramp.sh`).
- `README.md`: trimmed to recording concerns; the Pipeline step 1 and Isolation
  sections point at `sandbox/` and link `sandbox/README.md`; env-var name updated.
- `SPIKE-NOTES.md`: env-var name and `demo/render.sh`/`demo/sandbox-bootstrap.sh`
  path mentions updated to their new homes.

### `test/harness.sh` (NEW) — agent-facing entrypoint

Subcommands for headless CLI + state-inspection testing:

- `harness.sh up` — build the local wsx (`cargo build` → `target/debug/wsx`), set
  `WSX_BIN` to it, default `WSX_SANDBOX_ROOT=/tmp/wsx-test` (distinct from the demo's
  `/tmp/wsx-demo` so a test never clobbers a recording in progress), run
  `sandbox/bootstrap.sh`, then print the activated env + a one-line quick-start.
- `harness.sh wsx <args…>` — run the sandboxed, locally-built wsx with the right env.
- `harness.sh state [sql]` — convenience query against the sandbox `state.db` (no
  args → a default summary; with SQL → run it) for assertions.
- `harness.sh down` — wipe the sandbox and the bridged `~/.claude` symlinks (same
  guards as `demo/Makefile clean`).

Clears agent-session markers via `sandbox/agent-env.sh` so any wsx-spawned agents run
as top-level sessions, matching real usage.

### `test/smoke.sh` (NEW) — worked example + self-test

The minimal proof the harness drives the live app end-to-end, and the pattern a
future agent copies:

1. `harness.sh up` (fresh sandbox).
2. Create a workspace via the wsx CLI against a synthetic repo
   (e.g. `harness.sh wsx workspace create toy-api --name smoke-check`).
3. Assert it appears — via `harness.sh wsx workspace list` and/or
   `harness.sh state` querying `state.db`.
4. `harness.sh down` on exit (trap), always.

Illustrative, lightweight, and self-cleaning — not a broad suite.

### `test/README.md` (NEW)

"How a future agent runs the app for testing": the `up`/`wsx`/`state`/`down` flow,
the headless CLI + state-inspection model, and a pointer to `smoke.sh` as the
copyable example. Links `sandbox/README.md` for the underlying contract.

## Testing strategy

- `sandbox/test-bootstrap.sh` + `sandbox/test-gen-repos.sh` — prove provisioning
  still works after the move and rename (updated to `WSX_SANDBOX_ROOT` and new paths).
- `test/smoke.sh` — proves the harness drives the live app and inspects state.
- `demo` screencast scripts: `make -C demo check` (`test-post.sh`,
  `test-speedramp.sh`) proves the recording pieces still run after `render.sh` and
  `Makefile` edits. A non-recording manual confirmation that `make -C demo hero`
  still wires up (bootstrap path resolves) is out of scope for automated checks but
  noted for the implementer.
- All `*.sh` keep `#!/usr/bin/env bash` + `set -euo pipefail` and should pass
  `shellcheck`.

## Out of scope

- A broad e2e test suite (only the single smoke example).
- TUI/PTY-driven testing (headless CLI only, per the drive-mode decision).
- Re-recording or re-tuning the screencasts.
- Updating historical `docs/superpowers/plans/*` references.

## Risks / notes

- **Shared `~/.claude/projects` symlink namespace + default roots:** demo defaults to
  `/tmp/wsx-demo`, the harness to `/tmp/wsx-test`; each bootstrap clears its own stale
  symlinks. Running a test will not clobber a demo sandbox, but both write transient
  symlinks under the real `~/.claude/projects` — documented, and removed by the
  respective clean/`down`.
- **`WSX_BIN` + local build:** `harness.sh up` builds debug wsx; first run pays a
  compile. Acceptable for an agent-driven harness.
- **Back-compat:** honoring `WSX_DEMO_ROOT` as a fallback keeps any external muscle
  memory / scripts working through the rename.
