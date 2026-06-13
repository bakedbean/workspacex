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
