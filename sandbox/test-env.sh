#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
root="$(mktemp -d)/wsx-sb"
# default-value path: unset both vars, expect /tmp/wsx-demo
# shellcheck disable=SC1091
( unset WSX_SANDBOX_ROOT WSX_DEMO_ROOT; source "$HERE/env.sh"
  [ "$WSX_SANDBOX_ROOT" = "/tmp/wsx-demo" ] || { echo "FAIL: default root"; exit 1; }
  [ "$XDG_STATE_HOME" = "/tmp/wsx-demo/state" ] || { echo "FAIL: default XDG"; exit 1; } )
# explicit path: WSX_SANDBOX_ROOT wins and derives the other three
# shellcheck disable=SC1091,SC2030
( export WSX_SANDBOX_ROOT="$root"; source "$HERE/env.sh"
  # shellcheck disable=SC2031
  [ "$XDG_STATE_HOME" = "$root/state" ] || { echo "FAIL: XDG"; exit 1; }
  [ "$CLAUDE_CONFIG_DIR" = "$root/claude-config" ] || { echo "FAIL: claude dir"; exit 1; }
  [ "$CODEX_HOME" = "$root/codex-home" ] || { echo "FAIL: codex home"; exit 1; } )
# back-compat: WSX_DEMO_ROOT honored when WSX_SANDBOX_ROOT unset
# shellcheck disable=SC1091
( unset WSX_SANDBOX_ROOT; export WSX_DEMO_ROOT="$root"; source "$HERE/env.sh"
  # shellcheck disable=SC2031
  [ "$WSX_SANDBOX_ROOT" = "$root" ] || { echo "FAIL: fallback"; exit 1; } )
echo "PASS"
