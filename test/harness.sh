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
    # Kill the detached server on exit even if send-keys/capture-pane errors under
    # `set -e` — a detached tmux server otherwise lingers indefinitely.
    trap 'tmux -L "$sock" kill-server 2>/dev/null || true' EXIT
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
  down)
    # Destructive. Normalize away `..` FIRST so a value like /tmp/wsx-test/.. can't
    # slip past the string guards and resolve to a parent; refuse obviously-unsafe
    # roots; require the root nested at least two levels deep. Operate on the
    # normalized path with `rm -rf --` (the `--` stops a leading-dash root being read
    # as an option). python3's os.path.abspath is portable (GNU `realpath -m` is not
    # on macOS) and handles a not-yet-existing path.
    root="$(python3 -c 'import os,sys; print(os.path.abspath(sys.argv[1]))' "$WSX_SANDBOX_ROOT")"
    case "$root" in
      ""|/|/.|//|/tmp|"$HOME") echo "harness: refusing unsafe WSX_SANDBOX_ROOT='$root'" >&2; exit 1;;
    esac
    case "$root" in
      */*/*) : ;;
      *) echo "harness: WSX_SANDBOX_ROOT too shallow to remove safely: '$root'" >&2; exit 1;;
    esac
    rm -rf -- "$root"
    find "$HOME/.claude/projects" -maxdepth 1 -type l -lname "$root/*" -delete 2>/dev/null || true
    echo "sandbox down ($root)"
    ;;
  *)
    echo "usage: harness.sh {up|wsx <args>|state [sql]|capture|shot|down}" >&2
    exit 1
    ;;
esac
