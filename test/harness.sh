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
