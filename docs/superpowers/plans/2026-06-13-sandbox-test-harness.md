# Sandbox extraction + thin e2e test harness — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split the `demo/` directory into a reusable `sandbox/` provisioning unit and a screencast-only `demo/`, then add a thin `test/` harness that lets a future agent run the live, sandboxed wsx app for testing (headless CLI + state inspection, plus tmux text snapshots and VHS image screenshots).

**Architecture:** `sandbox/` becomes the shared substrate (provision an isolated wsx + synthetic repos + pre-authed agents; plus the VHS driver and env helpers). `demo/` keeps only video post-processing and consumes `sandbox/`. A new `test/harness.sh` drives the sandboxed app for assertions and reuses the same VHS driver for screenshots. The work is mostly behavior-preserving file moves plus a few small new bash scripts; each new script is covered by a focused bash test.

**Tech Stack:** Bash (`set -euo pipefail`, shellcheck-clean), `wsx` (Rust binary, `cargo build --bin wsx`), `sqlite3`, `tmux` (text capture), `vhs`/`ttyd`/`chromium` (image screenshots), `git`.

---

## File Structure

```
sandbox/                 # shared substrate (NEW dir)
  bootstrap.sh           # moved from demo/sandbox-bootstrap.sh; + WSX_SANDBOX_ROOT rename + WSX_BIN
  gen-repos.sh           # moved verbatim from demo/
  render.sh              # moved from demo/; now sources agent-env.sh
  agent-env.sh           # NEW — single source of truth for agent session-marker clearing
  env.sh                 # NEW — sourceable: re-enter an already-provisioned sandbox
  test-bootstrap.sh      # moved from demo/; ref + env-var updated
  test-gen-repos.sh      # moved verbatim from demo/
  test-agent-env.sh      # NEW — unit test for agent-env.sh
  test-env.sh            # NEW — unit test for env.sh
  README.md              # NEW — documents the env contract

demo/                    # screencast-only (video post-processing)
  deadair.sh speedramp.sh post.sh
  tapes/ captions/ Makefile SPIKE-NOTES.md README.md
  test-post.sh test-speedramp.sh

test/                    # NEW thin e2e harness
  harness.sh             # up / wsx / state / capture / shot / down
  shots/dashboard.tape   # NEW — example screenshot tape
  smoke.sh               # worked example + harness self-test
  README.md              # NEW
```

**Key invariant — the default sandbox root string does NOT change.** `demo/tapes/*.tape` hardcode `/tmp/wsx-demo/...` in their `Env` blocks. We rename the *variable* `WSX_DEMO_ROOT → WSX_SANDBOX_ROOT` but keep its *default value* `/tmp/wsx-demo`, so the tapes need no edits. The harness explicitly overrides the root to `/tmp/wsx-test`.

---

## Task 1: Move provisioning + driver scripts into `sandbox/` (behavior-preserving)

Pure relocation. No env-var rename yet — scripts keep reading `WSX_DEMO_ROOT` so the existing tests stay green and prove the move didn't break anything.

**Files:**
- Move: `demo/sandbox-bootstrap.sh` → `sandbox/bootstrap.sh`
- Move: `demo/gen-repos.sh` → `sandbox/gen-repos.sh`
- Move: `demo/render.sh` → `sandbox/render.sh`
- Move: `demo/test-bootstrap.sh` → `sandbox/test-bootstrap.sh`
- Move: `demo/test-gen-repos.sh` → `sandbox/test-gen-repos.sh`
- Modify: `sandbox/test-bootstrap.sh` (internal ref `sandbox-bootstrap.sh` → `bootstrap.sh`)
- Modify: `demo/Makefile` (recipe paths)

- [ ] **Step 1: Move the five files with git mv**

```bash
cd "$(git rev-parse --show-toplevel)"
mkdir -p sandbox
git mv demo/sandbox-bootstrap.sh sandbox/bootstrap.sh
git mv demo/gen-repos.sh         sandbox/gen-repos.sh
git mv demo/render.sh            sandbox/render.sh
git mv demo/test-bootstrap.sh    sandbox/test-bootstrap.sh
git mv demo/test-gen-repos.sh    sandbox/test-gen-repos.sh
```

Note: `sandbox/bootstrap.sh` already computes `SKILL_SRC="$(cd "$HERE/.." && pwd)/skills/wsx/SKILL.md"` and calls `"$HERE/gen-repos.sh"`; both resolve correctly from the new location (`HERE/..` is still the repo root, `gen-repos.sh` is still a sibling). No change needed there.

- [ ] **Step 2: Update the one internal reference in test-bootstrap.sh**

In `sandbox/test-bootstrap.sh`, change the bootstrap invocation:

```bash
# old:
"$(dirname "$0")/sandbox-bootstrap.sh" >/dev/null
# new:
"$(dirname "$0")/bootstrap.sh" >/dev/null
```

- [ ] **Step 3: Update demo/Makefile recipe paths**

In `demo/Makefile`, replace the moved-script paths in the `hero` and `parallel` recipes:

```makefile
# old → new (both recipes):
cd $(ROOT) && bash demo/sandbox-bootstrap.sh   →   cd $(ROOT) && bash sandbox/bootstrap.sh
cd $(ROOT) && bash demo/render.sh <tape>       →   cd $(ROOT) && bash sandbox/render.sh <tape>
```

Concretely the four affected lines become:
```makefile
	cd $(ROOT) && bash sandbox/bootstrap.sh
	cd $(ROOT) && bash sandbox/render.sh demo/tapes/01-hero-multi-agent.tape
	...
	cd $(ROOT) && bash sandbox/bootstrap.sh
	cd $(ROOT) && bash sandbox/render.sh demo/tapes/02-parallel-worktrees.tape
```

Also fix the `check` target — it currently runs the now-moved sandbox tests from
`demo/`. Drop those (they run from `sandbox/` now) so `check` covers only the
screencast scripts:
```makefile
# old:
check:
	cd $(ROOT)/demo && bash test-gen-repos.sh && bash test-bootstrap.sh && bash test-post.sh && bash test-speedramp.sh
# new:
check:
	cd $(ROOT)/demo && bash test-post.sh && bash test-speedramp.sh
```

- [ ] **Step 4: Run the moved sandbox tests to verify the move is clean**

Run:
```bash
cd "$(git rev-parse --show-toplevel)"
bash sandbox/test-gen-repos.sh && bash sandbox/test-bootstrap.sh
```
Expected: `generated repos in ...` then a series of checks ending in `PASS`. (Requires `wsx` on PATH; the bootstrap WARNs but continues if Claude/Codex creds are absent.)

- [ ] **Step 5: Shellcheck the moved scripts**

Run: `shellcheck sandbox/bootstrap.sh sandbox/gen-repos.sh sandbox/render.sh sandbox/test-bootstrap.sh sandbox/test-gen-repos.sh`
Expected: no output (clean).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "refactor(demo): move sandbox provisioning + VHS driver into sandbox/"
```

---

## Task 2: Extract `sandbox/agent-env.sh` and source it from `render.sh`

The list of parent-session markers to clear is currently hardcoded in `render.sh`'s `env -u …` line. Extract it to one sourceable file so the harness's tmux path clears the identical set.

**Files:**
- Create: `sandbox/agent-env.sh`
- Create: `sandbox/test-agent-env.sh`
- Modify: `sandbox/render.sh`

- [ ] **Step 1: Write the failing test**

Create `sandbox/test-agent-env.sh`:
```bash
#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=/dev/null
source "$HERE/agent-env.sh"

# The marker list must include the parent-session vars that make a spawned agent
# treat itself as a nested child (and skip persisting its session jsonl).
for v in AI_AGENT CLAUDECODE CLAUDE_CODE_SESSION_ID CLAUDE_CODE_CHILD_SESSION; do
  printf '%s\n' "${WSX_AGENT_ENV_UNSET[@]}" | grep -qx "$v" \
    || { echo "FAIL: $v missing from WSX_AGENT_ENV_UNSET"; exit 1; }
done

# wsx_clear_agent_env must actually unset them in the current shell.
export CLAUDECODE=1 CLAUDE_CODE_SESSION_ID=abc
wsx_clear_agent_env
[ -z "${CLAUDECODE:-}" ] || { echo "FAIL: CLAUDECODE not cleared"; exit 1; }
[ -z "${CLAUDE_CODE_SESSION_ID:-}" ] || { echo "FAIL: session id not cleared"; exit 1; }
echo "PASS"
```

- [ ] **Step 2: Run it to verify it fails**

Run: `bash sandbox/test-agent-env.sh`
Expected: FAIL — `sandbox/agent-env.sh: No such file or directory`.

- [ ] **Step 3: Create sandbox/agent-env.sh**

```bash
#!/usr/bin/env bash
# Source this to get the parent-session env markers that must be cleared before
# launching a Claude/Codex agent, so the agent runs as a genuine TOP-LEVEL session
# (and persists its per-worktree session jsonl) instead of treating itself as a
# nested child — which leaves wsx's detail bars stuck on "loading…".
# See demo/SPIKE-NOTES.md for the full mechanics.
#
# Exposes:
#   WSX_AGENT_ENV_UNSET  — array of var names to clear
#   wsx_clear_agent_env  — unset them in the current shell
WSX_AGENT_ENV_UNSET=(
  AI_AGENT
  CLAUDECODE
  CLAUDE_EFFORT
  CLAUDE_CODE_ENTRYPOINT
  CLAUDE_CODE_EXECPATH
  CLAUDE_CODE_SESSION_ID
  CLAUDE_CODE_CHILD_SESSION
)
wsx_clear_agent_env() { unset "${WSX_AGENT_ENV_UNSET[@]}"; }
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `bash sandbox/test-agent-env.sh`
Expected: `PASS`.

- [ ] **Step 5: Rewrite sandbox/render.sh to source the shared list**

Replace the body of `sandbox/render.sh` (keep the existing explanatory header comment) so the marker list comes from `agent-env.sh`:
```bash
#!/usr/bin/env bash
# Render a VHS tape with Claude Code parent-session markers cleared.
# (keep the existing multi-line "Why:" comment block here unchanged)
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=/dev/null
source "$HERE/agent-env.sh"
unset_args=()
for v in "${WSX_AGENT_ENV_UNSET[@]}"; do unset_args+=(-u "$v"); done
exec env "${unset_args[@]}" vhs "${1:?usage: render.sh <tape-file>}"
```

- [ ] **Step 6: Shellcheck both files**

Run: `shellcheck sandbox/agent-env.sh sandbox/render.sh sandbox/test-agent-env.sh`
Expected: clean. (If shellcheck flags the `source` with SC1091, the `# shellcheck source=/dev/null` directive already suppresses it.)

- [ ] **Step 7: Commit**

```bash
git add sandbox/agent-env.sh sandbox/test-agent-env.sh sandbox/render.sh
git commit -m "refactor(sandbox): extract agent-env marker list into shared agent-env.sh"
```

---

## Task 3: Rename `WSX_DEMO_ROOT → WSX_SANDBOX_ROOT` (with fallback) + add `WSX_BIN`

Rename the primary env var (keeping `WSX_DEMO_ROOT` as a back-compat fallback and `/tmp/wsx-demo` as the default value), and route every `wsx` call in bootstrap through `$WSX_BIN` so a consumer can point at a locally-built binary.

**Files:**
- Modify: `sandbox/bootstrap.sh`
- Modify: `sandbox/test-bootstrap.sh`
- Modify: `demo/Makefile` (`clean` recipe)

- [ ] **Step 1: Update the test first (it should still pass after the rename)**

In `sandbox/test-bootstrap.sh`, switch the temp-root var and add a fallback assertion. Replace the top of the file (the first 8 lines through the bootstrap call) with:
```bash
#!/usr/bin/env bash
set -euo pipefail
WSX_SANDBOX_ROOT="$(mktemp -d)/wsx-demo"
export WSX_SANDBOX_ROOT
# clean both the temp sandbox AND the session-log symlinks bridged into ~/.claude
trap 'rm -rf "$(dirname "$WSX_SANDBOX_ROOT")"; find "$HOME/.claude/projects" -maxdepth 1 -type l -lname "$WSX_SANDBOX_ROOT/*" -delete 2>/dev/null || true' EXIT
"$(dirname "$0")/bootstrap.sh" >/dev/null
export XDG_STATE_HOME="$WSX_SANDBOX_ROOT/state"
```
Then replace the remaining `$WSX_DEMO_ROOT` references in the body with `$WSX_SANDBOX_ROOT` (the `claude-config`, `codex-home`, and symlink-target checks).

- [ ] **Step 2: Run the test to verify it fails (bootstrap still only honors WSX_DEMO_ROOT)**

Run: `bash sandbox/test-bootstrap.sh`
Expected: FAIL — bootstrap ignores `WSX_SANDBOX_ROOT`, falls back to `/tmp/wsx-demo`, so `$XDG_STATE_HOME` (pointed at the mktemp dir) has no `state.db`: `FAIL: no isolated db`.

- [ ] **Step 3: Update bootstrap.sh resolution + guards + WSX_BIN**

In `sandbox/bootstrap.sh`:

(a) Replace the root resolution line:
```bash
# old:
export WSX_DEMO_ROOT="${WSX_DEMO_ROOT:-/tmp/wsx-demo}"
# new (new var wins; old var is back-compat; default value unchanged):
export WSX_SANDBOX_ROOT="${WSX_SANDBOX_ROOT:-${WSX_DEMO_ROOT:-/tmp/wsx-demo}}"
```

(b) Update the two destructive-path guards to use the new var:
```bash
case "$WSX_SANDBOX_ROOT" in
  ""|/|/.|//) echo "FATAL: unsafe WSX_SANDBOX_ROOT='$WSX_SANDBOX_ROOT'" >&2; exit 1;;
esac
[ "$WSX_SANDBOX_ROOT" = "$HOME" ] && { echo "FATAL: WSX_SANDBOX_ROOT must not be \$HOME" >&2; exit 1; }
```

(c) Replace every remaining `$WSX_DEMO_ROOT` in the file with `$WSX_SANDBOX_ROOT` (the `XDG_STATE_HOME`/`CLAUDE_CONFIG_DIR`/`CODEX_HOME`/`REPOS` derivations, the `rm -rf`, the symlink find/clean, and the closing `echo`s).

(d) Add a `WSX_BIN` default just after the env exports (near line 21):
```bash
WSX_BIN="${WSX_BIN:-wsx}"
```

(e) Route the four `wsx` provisioning calls through it:
```bash
"$WSX_BIN" repo add "$REPOS/toy-api" --name toy-api --prefix demo
"$WSX_BIN" repo add "$REPOS/toy-cli" --name toy-cli --prefix demo
"$WSX_BIN" repo set-base-branch toy-api main
"$WSX_BIN" repo set-base-branch toy-cli main
```

- [ ] **Step 4: Run the test to verify it passes (and the fallback still works)**

Run:
```bash
bash sandbox/test-bootstrap.sh
# also confirm the back-compat fallback path:
tmp="$(mktemp -d)/wsx-demo"; WSX_DEMO_ROOT="$tmp" bash -c '
  set -e
  bash sandbox/bootstrap.sh >/dev/null
  test -f "$WSX_DEMO_ROOT/state/wsx/state.db" && echo "FALLBACK PASS"
  rm -rf "$(dirname "$WSX_DEMO_ROOT")"
  find "$HOME/.claude/projects" -maxdepth 1 -type l -lname "$tmp/*" -delete 2>/dev/null || true'
```
Expected: `PASS` from the first, `FALLBACK PASS` from the second.

- [ ] **Step 5: Update demo/Makefile clean recipe to the new var (keep default value)**

In `demo/Makefile`, the `clean` recipe references `WSX_DEMO_ROOT` twice. Update to prefer the new var while keeping `/tmp/wsx-demo` as the default:
```makefile
clean:
	@root="$${WSX_SANDBOX_ROOT:-$${WSX_DEMO_ROOT:-/tmp/wsx-demo}}"; \
	case "$$root" in ""|/|/.|//|"$$HOME") echo "refusing unsafe WSX_SANDBOX_ROOT='$$root'" >&2; exit 1;; esac; \
	rm -rf "$$root" $(ROOT)/demo/out
	@# remove the transient session-log symlinks bridged into ~/.claude/projects
	find $$HOME/.claude/projects -maxdepth 1 -type l -lname "$${WSX_SANDBOX_ROOT:-$${WSX_DEMO_ROOT:-/tmp/wsx-demo}}/*" -delete 2>/dev/null || true
```

- [ ] **Step 6: Shellcheck**

Run: `shellcheck sandbox/bootstrap.sh sandbox/test-bootstrap.sh`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add sandbox/bootstrap.sh sandbox/test-bootstrap.sh demo/Makefile
git commit -m "refactor(sandbox): rename WSX_DEMO_ROOT->WSX_SANDBOX_ROOT (fallback) + add WSX_BIN"
```

---

## Task 4: Add `sandbox/env.sh` (re-enter a provisioned sandbox)

A sourceable file that exports the four-var env contract for an already-provisioned sandbox, without provisioning or wiping.

**Files:**
- Create: `sandbox/env.sh`
- Create: `sandbox/test-env.sh`

- [ ] **Step 1: Write the failing test**

Create `sandbox/test-env.sh`:
```bash
#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
root="$(mktemp -d)/wsx-sb"
# default-value path: unset both vars, expect /tmp/wsx-demo
( unset WSX_SANDBOX_ROOT WSX_DEMO_ROOT; source "$HERE/env.sh"
  [ "$WSX_SANDBOX_ROOT" = "/tmp/wsx-demo" ] || { echo "FAIL: default root"; exit 1; }
  [ "$XDG_STATE_HOME" = "/tmp/wsx-demo/state" ] || { echo "FAIL: default XDG"; exit 1; } )
# explicit path: WSX_SANDBOX_ROOT wins and derives the other three
( export WSX_SANDBOX_ROOT="$root"; source "$HERE/env.sh"
  [ "$XDG_STATE_HOME" = "$root/state" ] || { echo "FAIL: XDG"; exit 1; }
  [ "$CLAUDE_CONFIG_DIR" = "$root/claude-config" ] || { echo "FAIL: claude dir"; exit 1; }
  [ "$CODEX_HOME" = "$root/codex-home" ] || { echo "FAIL: codex home"; exit 1; } )
# back-compat: WSX_DEMO_ROOT honored when WSX_SANDBOX_ROOT unset
( unset WSX_SANDBOX_ROOT; export WSX_DEMO_ROOT="$root"; source "$HERE/env.sh"
  [ "$WSX_SANDBOX_ROOT" = "$root" ] || { echo "FAIL: fallback"; exit 1; } )
echo "PASS"
```

- [ ] **Step 2: Run it to verify it fails**

Run: `bash sandbox/test-env.sh`
Expected: FAIL — `sandbox/env.sh: No such file or directory`.

- [ ] **Step 3: Create sandbox/env.sh**

```bash
#!/usr/bin/env bash
# Source this to (re-)enter an already-provisioned sandbox: it exports the env
# contract that bootstrap.sh established, deriving the three XDG/agent dirs from
# the sandbox root. It does NOT provision or wipe anything.
#
#   source sandbox/env.sh                       # uses $WSX_SANDBOX_ROOT or the default
#   WSX_SANDBOX_ROOT=/tmp/wsx-test source env.sh # enter a specific sandbox
#
# Resolution mirrors bootstrap.sh: WSX_SANDBOX_ROOT, else WSX_DEMO_ROOT (back-compat),
# else /tmp/wsx-demo.
WSX_SANDBOX_ROOT="${WSX_SANDBOX_ROOT:-${WSX_DEMO_ROOT:-/tmp/wsx-demo}}"
export WSX_SANDBOX_ROOT
export XDG_STATE_HOME="$WSX_SANDBOX_ROOT/state"
export CLAUDE_CONFIG_DIR="$WSX_SANDBOX_ROOT/claude-config"
export CODEX_HOME="$WSX_SANDBOX_ROOT/codex-home"
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `bash sandbox/test-env.sh`
Expected: `PASS`.

- [ ] **Step 5: Shellcheck**

Run: `shellcheck sandbox/env.sh sandbox/test-env.sh`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add sandbox/env.sh sandbox/test-env.sh
git commit -m "feat(sandbox): add env.sh to re-enter a provisioned sandbox"
```

---

## Task 5: Write `sandbox/README.md`

**Files:**
- Create: `sandbox/README.md`

- [ ] **Step 1: Create sandbox/README.md**

```markdown
# sandbox/ — isolated wsx, for demos and tests

Stands up a fully isolated, live `wsx` install with synthetic repos and
pre-authenticated agents, so anything that needs to *run the real app* — the
screencast recordings under `demo/` and the e2e harness under `test/` — can build
on it. Nothing here touches your real `~/.local/state/wsx`, `~/.claude.json`,
`~/.claude/settings.json`, or `~/.codex`.

## Pieces

| File | Responsibility |
|---|---|
| `bootstrap.sh` | Provision a fresh sandbox: isolated wsx state + synthetic repos + pre-authed/pre-trusted Claude & Codex configs + the wsx agent skill + session-log bridging. Wipes and recreates the sandbox root each run. |
| `gen-repos.sh` | Generate the synthetic `toy-api` / `toy-cli` repos with deliberately planted bugs. |
| `env.sh` | `source` it to re-enter an already-provisioned sandbox (exports the env contract; provisions/wipes nothing). |
| `render.sh` | Drive the sandboxed TUI under [VHS](https://github.com/charmbracelet/vhs) with agent session-markers cleared. The reusable basis for both screencasts and image screenshots. |
| `agent-env.sh` | Single source of truth for the parent-session env markers that must be cleared so spawned agents run as top-level sessions. |

## Env contract

| Var | Meaning | Default |
|---|---|---|
| `WSX_SANDBOX_ROOT` | Root of the sandbox; everything lives under it. `WSX_DEMO_ROOT` is honored as a back-compat fallback. | `/tmp/wsx-demo` |
| `WSX_BIN` | The `wsx` binary `bootstrap.sh` provisions with — point it at a local build to exercise local changes. | `wsx` (PATH) |
| `XDG_STATE_HOME` | Isolated wsx `state.db`, worktrees, logs. | `$WSX_SANDBOX_ROOT/state` |
| `CLAUDE_CONFIG_DIR` | Isolated Claude config (copied creds + settings + per-worktree trust). | `$WSX_SANDBOX_ROOT/claude-config` |
| `CODEX_HOME` | Isolated Codex config (copied auth + per-repo trust). | `$WSX_SANDBOX_ROOT/codex-home` |

## Usage

```bash
bash sandbox/bootstrap.sh            # provision a fresh sandbox at $WSX_SANDBOX_ROOT
source sandbox/env.sh                # re-enter it in another shell
WSX_BIN=./target/debug/wsx bash sandbox/bootstrap.sh   # provision with a local build
```

## What it writes outside the sandbox

Only a set of **transient symlinks** under `~/.claude/projects/<encoded-worktree>`,
pointing into the sandbox — these bridge the isolated session logs to where wsx reads
them (`dirs::home_dir()`, no env override) so the workspace detail bars populate. They
are removed by `demo`'s `make clean` and the test harness's `harness.sh down`.

## Tests

`bash sandbox/test-gen-repos.sh`, `bash sandbox/test-bootstrap.sh`,
`bash sandbox/test-agent-env.sh`, `bash sandbox/test-env.sh` — no recording, just the
provisioning pieces.
```

- [ ] **Step 2: Commit**

```bash
git add sandbox/README.md
git commit -m "docs(sandbox): document the env contract and pieces"
```

---

## Task 6: Harness core (`up` / `wsx` / `state` / `down`) + smoke (CLI + state)

TDD: write the smoke test first against the harness interface, watch it fail (no harness), implement the four core subcommands, watch it pass.

**Files:**
- Create: `test/smoke.sh`
- Create: `test/harness.sh`

- [ ] **Step 1: Write the failing smoke test**

Create `test/smoke.sh`:
```bash
#!/usr/bin/env bash
# Worked example + self-test of the harness: provision, create a workspace via the
# wsx CLI, and assert it appears via both `wsx workspace list` and a state.db query.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
H="$HERE/harness.sh"
export WSX_SANDBOX_ROOT="${WSX_SANDBOX_ROOT:-/tmp/wsx-test}"
trap 'bash "$H" down >/dev/null 2>&1 || true' EXIT

bash "$H" up

bash "$H" wsx workspace create toy-api --name smoke-check

# Assertion 1 — the CLI lists it.
bash "$H" wsx workspace list toy-api | grep -q smoke-check \
  || { echo "FAIL: smoke-check not in workspace list"; exit 1; }

# Assertion 2 — it landed in state.db.
bash "$H" state "SELECT name FROM workspaces;" | grep -q smoke-check \
  || { echo "FAIL: smoke-check not in state.db"; exit 1; }

echo "SMOKE PASS"
```

- [ ] **Step 2: Run it to verify it fails**

Run: `bash test/smoke.sh`
Expected: FAIL — `test/harness.sh: No such file or directory` (or the `up` line erroring).

- [ ] **Step 3: Implement test/harness.sh with the four core subcommands**

Create `test/harness.sh`:
```bash
#!/usr/bin/env bash
# Thin e2e harness: stand up a sandboxed, locally-built wsx and drive it for tests.
# Default drive mode is headless CLI + state inspection; `capture`/`shot` add TUI
# snapshots (see Task 7/8). All state is derived from WSX_SANDBOX_ROOT each call, so
# the harness is stateless across invocations.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# A test sandbox, distinct from the demo's /tmp/wsx-demo so a test never clobbers a
# recording in progress.
export WSX_SANDBOX_ROOT="${WSX_SANDBOX_ROOT:-/tmp/wsx-test}"
# Exercise the LOCAL build, not whatever `wsx` is installed on PATH.
export WSX_BIN="${WSX_BIN:-$ROOT/target/debug/wsx}"

_enter() { # shellcheck source=/dev/null
  source "$ROOT/sandbox/env.sh"; }

_need() { command -v "$1" >/dev/null 2>&1 || { echo "harness: '$1' not installed (needed for '$2')" >&2; exit 127; }; }

cmd="${1:-}"; shift || true
case "$cmd" in
  up)
    cargo build --manifest-path "$ROOT/Cargo.toml" --bin wsx
    WSX_BIN="$WSX_BIN" WSX_SANDBOX_ROOT="$WSX_SANDBOX_ROOT" bash "$ROOT/sandbox/bootstrap.sh"
    _enter
    echo "sandbox up at $WSX_SANDBOX_ROOT (WSX_BIN=$WSX_BIN)"
    echo "  next: test/harness.sh wsx workspace list"
    ;;
  wsx)
    _enter
    exec "$WSX_BIN" "$@"
    ;;
  state)
    _need sqlite3 state
    _enter
    db="$XDG_STATE_HOME/wsx/state.db"
    test -f "$db" || { echo "harness: no state.db at $db — run 'harness.sh up' first" >&2; exit 1; }
    if [ "$#" -gt 0 ]; then
      sqlite3 "$db" "$*"
    else
      sqlite3 -header -column "$db" \
        "SELECT r.name AS repo, w.name AS workspace, w.branch, w.state
           FROM workspaces w JOIN repos r ON r.id = w.repo_id;"
    fi
    ;;
  down)
    case "$WSX_SANDBOX_ROOT" in
      ""|/|/.|//|"$HOME") echo "harness: refusing unsafe WSX_SANDBOX_ROOT='$WSX_SANDBOX_ROOT'" >&2; exit 1;;
    esac
    rm -rf "$WSX_SANDBOX_ROOT"
    find "$HOME/.claude/projects" -maxdepth 1 -type l -lname "$WSX_SANDBOX_ROOT/*" -delete 2>/dev/null || true
    echo "sandbox down ($WSX_SANDBOX_ROOT)"
    ;;
  *)
    echo "usage: harness.sh {up|wsx <args>|state [sql]|capture|shot|down}" >&2
    exit 1
    ;;
esac
```

Note: `harness.sh wsx workspace create` runs the CLI non-interactively; `workspace create` does not prompt. The first `up` pays a `cargo build` (debug) compile.

- [ ] **Step 4: Run the smoke test to verify it passes**

Run: `bash test/smoke.sh`
Expected: ends in `SMOKE PASS` (after a one-time debug build of wsx). The `down` trap wipes `/tmp/wsx-test` and its symlinks on exit.

- [ ] **Step 5: Shellcheck**

Run: `shellcheck test/harness.sh test/smoke.sh`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add test/harness.sh test/smoke.sh
git commit -m "feat(test): thin e2e harness (up/wsx/state/down) + smoke test"
```

---

## Task 7: Add `capture` (tmux text snapshot) + extend smoke

**Files:**
- Modify: `test/harness.sh` (add `capture` subcommand)
- Modify: `test/smoke.sh` (add a capture assertion)

- [ ] **Step 1: Extend the smoke test with a capture assertion (failing)**

In `test/smoke.sh`, insert before the final `echo "SMOKE PASS"`:
```bash
# Assertion 3 — the TUI text-renders the new workspace (tmux capture path).
shot_txt="$(mktemp)"
bash "$H" capture "$shot_txt"
grep -q smoke-check "$shot_txt" \
  || { echo "FAIL: smoke-check not visible in TUI capture"; cat "$shot_txt"; rm -f "$shot_txt"; exit 1; }
rm -f "$shot_txt"
```

- [ ] **Step 2: Run smoke to verify the new assertion fails**

Run: `bash test/smoke.sh`
Expected: FAIL at the usage error from `harness.sh capture` (unknown subcommand → `usage:` line, non-zero exit).

- [ ] **Step 3: Implement the `capture` subcommand**

In `test/harness.sh`, add a `capture)` arm before the `down)` arm:
```bash
  capture)
    _need tmux capture
    out="${!#:-}"   # last arg is the output text file
    [ "$#" -ge 1 ] || { echo "usage: harness.sh capture [keys...] <out.txt>" >&2; exit 1; }
    set -- "${@:1:$(($#-1))}"   # remaining args (if any) are keys to send
    _enter
    # Clear agent markers so any agent the TUI spawns runs top-level (matches reality).
    # shellcheck source=/dev/null
    source "$ROOT/sandbox/agent-env.sh"; wsx_clear_agent_env
    sock="wsx-harness-$$"
    # Launch the sandboxed TUI in a detached tmux session, passing the isolated env.
    tmux -L "$sock" new-session -d -x 200 -y 50 \
      -e "XDG_STATE_HOME=$XDG_STATE_HOME" \
      -e "CLAUDE_CONFIG_DIR=$CLAUDE_CONFIG_DIR" \
      -e "CODEX_HOME=$CODEX_HOME" \
      "$WSX_BIN"
    sleep 2                              # let the dashboard paint
    for k in "$@"; do tmux -L "$sock" send-keys "$k"; sleep 0.3; done
    tmux -L "$sock" capture-pane -p > "$out"
    tmux -L "$sock" kill-server 2>/dev/null || true
    echo "captured TUI text -> $out"
    ;;
```

Note on arg handling: `capture` takes an optional sequence of key sends followed by a required output path as the **last** argument. The smoke test calls `capture <out.txt>` with no keys, so it just snapshots the initial dashboard.

- [ ] **Step 4: Run smoke to verify it passes**

Run: `bash test/smoke.sh`
Expected: `SMOKE PASS`, now including the capture assertion. (Requires `tmux`.)

- [ ] **Step 5: Shellcheck**

Run: `shellcheck test/harness.sh test/smoke.sh`
Expected: clean. (If SC2124/SC2294 appears around the array slicing, prefer the explicit `set --` form shown; re-run to confirm clean.)

- [ ] **Step 6: Commit**

```bash
git add test/harness.sh test/smoke.sh
git commit -m "feat(test): add tmux text-capture subcommand to harness"
```

---

## Task 8: Add `shot` (VHS image screenshot) + `test/shots/dashboard.tape`

The image path reuses `sandbox/render.sh`. It depends on `vhs`; the smoke test does **not** gate it (so the suite runs on a bare host), but we add a standalone guarded check.

**Files:**
- Create: `test/shots/dashboard.tape`
- Modify: `test/harness.sh` (add `shot` subcommand)

- [ ] **Step 1: Create the example screenshot tape**

Create `test/shots/dashboard.tape`. The `Env` paths target the harness's `/tmp/wsx-test` sandbox, and it screenshots to `test/out/` (CWD-relative; the harness runs VHS from the repo root):
```tape
Output test/out/dashboard.gif
Set Width 1200
Set Height 700
Set Shell "bash"
Env XDG_STATE_HOME "/tmp/wsx-test/state"
Env CLAUDE_CONFIG_DIR "/tmp/wsx-test/claude-config"
Env CODEX_HOME "/tmp/wsx-test/codex-home"
Type "target/debug/wsx"
Enter
Sleep 3s
Screenshot test/out/dashboard.png
Sleep 1s
```
Note: VHS requires an `Output` directive; the `.gif` is a throwaway — the deliverable is the `Screenshot` PNG. `test/out/` is gitignored (Task 9).

- [ ] **Step 2: Implement the `shot` subcommand**

In `test/harness.sh`, add a `shot)` arm before `down)`:
```bash
  shot)
    _need vhs shot
    tape="${1:?usage: harness.sh shot <tape>}"
    test -f "$tape" || { echo "harness: no tape at $tape" >&2; exit 1; }
    mkdir -p "$ROOT/test/out"
    # render.sh clears agent markers and execs vhs; run from repo root so the tape's
    # CWD-relative Output/Screenshot paths resolve under test/out/.
    ( cd "$ROOT" && bash "$ROOT/sandbox/render.sh" "$tape" )
    echo "shot rendered (see Screenshot path in $tape, under test/out/)"
    ;;
```

- [ ] **Step 3: Guarded manual verification (only where vhs is installed)**

Run:
```bash
if command -v vhs >/dev/null 2>&1; then
  bash test/harness.sh up
  bash test/harness.sh shot test/shots/dashboard.tape
  test -f test/out/dashboard.png && echo "SHOT PASS" || echo "SHOT FAIL"
  bash test/harness.sh down
else
  echo "skip: vhs not installed — checking the guard message instead"
  bash test/harness.sh shot test/shots/dashboard.tape 2>&1 | grep -q "not installed" && echo "GUARD PASS"
fi
```
Expected: `SHOT PASS` where vhs exists (a real `test/out/dashboard.png`), otherwise `skip: ...` then `GUARD PASS` (the `_need` guard prints a clear message and exits 127).

- [ ] **Step 4: Shellcheck**

Run: `shellcheck test/harness.sh`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add test/harness.sh test/shots/dashboard.tape
git commit -m "feat(test): add VHS image-screenshot subcommand + example tape"
```

---

## Task 9: Docs — `test/README.md`, `test/.gitignore`, and update `demo/` prose

**Files:**
- Create: `test/README.md`
- Create: `test/.gitignore`
- Modify: `demo/README.md`
- Modify: `demo/SPIKE-NOTES.md`

- [ ] **Step 1: Create test/.gitignore**

```gitignore
out/
```

- [ ] **Step 2: Create test/README.md**

```markdown
# test/ — thin e2e harness for the live wsx app

Lets an agent (or you) run the **real** wsx against an isolated sandbox and assert on
it. Built on `sandbox/` (see `../sandbox/README.md` for the env contract). Default
mode is headless CLI + state inspection; TUI snapshots are available when a visual
check helps.

## Quick start

```bash
test/harness.sh up                                   # build local wsx + provision a fresh sandbox at /tmp/wsx-test
test/harness.sh wsx workspace create toy-api --name foo
test/harness.sh wsx workspace list toy-api           # CLI assertion surface
test/harness.sh state                                 # default state.db summary
test/harness.sh state "SELECT * FROM workspaces;"     # arbitrary query
test/harness.sh capture /tmp/screen.txt               # tmux text snapshot of the TUI
test/harness.sh shot test/shots/dashboard.tape        # VHS PNG screenshot (needs vhs) -> test/out/
test/harness.sh down                                  # wipe sandbox + bridged ~/.claude symlinks
```

The harness always drives a **locally built** `target/debug/wsx` (via `WSX_BIN`), so
tests exercise your changes — not the installed `wsx`. It uses `/tmp/wsx-test` so it
never collides with a `demo/` recording at `/tmp/wsx-demo`.

## Worked example

`test/smoke.sh` provisions, creates a workspace, and asserts it via the CLI, the
state.db, and a tmux text capture. Run it: `bash test/smoke.sh` → `SMOKE PASS`. Copy it
as the starting point for new e2e checks.

## Dependencies

`up`/`wsx`/`state` need `wsx` (built via `cargo`), `git`, `python3`, `sqlite3`.
`capture` adds `tmux`. `shot` adds `vhs` (+ `ttyd`, headless `chromium`). Each snapshot
subcommand prints a clear "not installed" message when its tool is missing, so the CLI
path works on a bare host.
```

- [ ] **Step 3: Update demo/README.md**

Make these edits to `demo/README.md`:
- In the Pipeline section, step 1 (`sandbox-bootstrap.sh`): change the script name to
  `sandbox/bootstrap.sh`.
- In the Pipeline section, step 2 (`render.sh tapes/*.tape`): change to
  `sandbox/render.sh tapes/*.tape`.
- Add a sentence near the top, after the intro paragraph:
  `> Provisioning is shared with the e2e test harness — see ../sandbox/README.md (env contract) and ../test/README.md (running the app for tests).`
- In the Customizing section, update the `gen-repos.sh` and tape references to note
  `gen-repos.sh` now lives under `sandbox/` (`sandbox/gen-repos.sh`).

- [ ] **Step 4: Update demo/SPIKE-NOTES.md path/var references**

In `demo/SPIKE-NOTES.md`, update mentions to the new homes/names (prose only, no
behavior): `sandbox-bootstrap.sh` → `sandbox/bootstrap.sh`; `demo/render.sh` →
`sandbox/render.sh`; where `WSX_DEMO_ROOT` is named as the canonical var, note it is
now `WSX_SANDBOX_ROOT` (with `WSX_DEMO_ROOT` honored as a fallback). The hardcoded
`/tmp/wsx-demo/...` example paths in the `Env` block listing stay as-is (that is still
the demo default).

- [ ] **Step 5: Verify no stale references remain**

Run:
```bash
grep -rn "sandbox-bootstrap\.sh\|demo/render\.sh" demo/ README.md 2>/dev/null || echo "no stale refs"
```
Expected: `no stale refs` (the only matches, if any, should be inside `docs/superpowers/plans/*`, which we intentionally leave alone).

- [ ] **Step 6: Commit**

```bash
git add test/README.md test/.gitignore demo/README.md demo/SPIKE-NOTES.md
git commit -m "docs: document test harness; update demo prose for sandbox/ split"
```

---

## Task 10: Final full verification

**Files:** none (verification only)

- [ ] **Step 1: Run every sandbox + screencast-script check**

Run:
```bash
cd "$(git rev-parse --show-toplevel)"
bash sandbox/test-gen-repos.sh
bash sandbox/test-bootstrap.sh
bash sandbox/test-agent-env.sh
bash sandbox/test-env.sh
make -C demo check
```
Expected: each sandbox test ends in `PASS`; `make -C demo check` runs `test-post.sh` and `test-speedramp.sh` to success (requires `ffmpeg`).

- [ ] **Step 2: Run the e2e smoke test**

Run: `bash test/smoke.sh`
Expected: `SMOKE PASS`.

- [ ] **Step 3: Shellcheck the whole surface**

Run:
```bash
shellcheck sandbox/*.sh test/*.sh
```
Expected: clean.

- [ ] **Step 4: Confirm the demo still wires up (non-recording dry check)**

Run: `make -C demo -n hero`
Expected: prints the recipe with `bash sandbox/bootstrap.sh` and `bash sandbox/render.sh demo/tapes/01-hero-multi-agent.tape` — confirming the moved paths resolve in the Makefile. (Full `make -C demo hero` recording is out of scope for automated verification.)

- [ ] **Step 5: Final commit if anything is outstanding**

```bash
git status --porcelain   # expect clean; commit any stragglers
```

---

## Notes for the implementer

- **Default value vs variable name:** we rename the variable to `WSX_SANDBOX_ROOT` but its default value stays `/tmp/wsx-demo`. Do not change tape `Env` paths — they point at the demo default deliberately.
- **`harness.sh` is stateless:** every subcommand re-derives the env from `WSX_SANDBOX_ROOT` (default `/tmp/wsx-test`) by sourcing `sandbox/env.sh`, and recomputes `WSX_BIN` as `target/debug/wsx`. There is no persisted "current sandbox" file.
- **First `up` compiles wsx (debug).** Subsequent runs are fast.
- **tmux determinism:** `capture` sleeps a fixed 2s for the dashboard to paint. If a capture is flaky on a slow host, bump the sleep — it is the only timing knob and is intentionally simple for a thin harness.
- **`shot` output path:** the screenshot path lives inside the tape (`Screenshot test/out/dashboard.png`), so `harness.sh shot <tape>` takes only the tape; the PNG location is whatever the tape declares under `test/out/`. This differs slightly from the spec's `shot <tape> <out.png>` sketch — the tape-declares-output form is what VHS actually supports cleanly.
```
