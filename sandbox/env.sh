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
