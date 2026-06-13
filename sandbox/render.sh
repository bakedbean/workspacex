#!/usr/bin/env bash
# Render a VHS tape with Claude Code parent-session markers cleared.
#
# Why: when the harness is run from *inside* a Claude Code session (e.g. an agent
# driving it), every child process inherits CLAUDECODE / CLAUDE_CODE_SESSION_ID /
# CLAUDE_CODE_CHILD_SESSION / etc. A claude spawned with those set treats itself
# as a *nested child* and does NOT persist its own per-worktree session jsonl —
# which means wsx's workspace detail bars (SESSION SUMMARY / RECENT CHAT) stay on
# "loading…". Clearing the markers makes the demo agents genuine top-level
# sessions, exactly like a normal `wsx` launch from a terminal.
#
# Outside Claude Code these vars are unset, so this is a harmless no-op.
#
# Usage: render.sh <tape-file>
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=/dev/null
source "$HERE/agent-env.sh"
unset_args=()
for v in "${WSX_AGENT_ENV_UNSET[@]}"; do unset_args+=(-u "$v"); done
exec env "${unset_args[@]}" vhs "${1:?usage: render.sh <tape-file>}"
