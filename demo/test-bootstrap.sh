#!/usr/bin/env bash
set -euo pipefail
WSX_DEMO_ROOT="$(mktemp -d)/wsx-demo"
export WSX_DEMO_ROOT
trap 'rm -rf "$(dirname "$WSX_DEMO_ROOT")"' EXIT
"$(dirname "$0")/sandbox-bootstrap.sh" >/dev/null
export XDG_STATE_HOME="$WSX_DEMO_ROOT/state"
test -f "$XDG_STATE_HOME/wsx/state.db" || { echo "FAIL: no isolated db"; exit 1; }
wsx repo list | grep -q toy-api || { echo "FAIL: toy-api not registered"; exit 1; }
wsx repo list | grep -q toy-cli || { echo "FAIL: toy-cli not registered"; exit 1; }
test -f "$WSX_DEMO_ROOT/claude-config/settings.json" || { echo "FAIL: no isolated claude settings"; exit 1; }
grep -q skipDangerousModePermissionPrompt "$WSX_DEMO_ROOT/claude-config/settings.json" || { echo "FAIL: bypass flag not set"; exit 1; }
grep -q hasTrustDialogAccepted "$WSX_DEMO_ROOT/claude-config/.claude.json" || { echo "FAIL: trust not pre-seeded"; exit 1; }
grep -q 'trust_level = "trusted"' "$WSX_DEMO_ROOT/codex-home/config.toml" || { echo "FAIL: codex trust not pre-seeded"; exit 1; }
test -d "$WSX_DEMO_ROOT/pi-agent" || { echo "FAIL: pi agent dir missing"; exit 1; }
echo "PASS"
