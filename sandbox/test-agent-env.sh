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
