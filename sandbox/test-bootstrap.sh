#!/usr/bin/env bash
set -euo pipefail
WSX_DEMO_ROOT="$(mktemp -d)/wsx-demo"
export WSX_DEMO_ROOT
# clean both the temp sandbox AND the session-log symlinks bridged into ~/.claude
trap 'rm -rf "$(dirname "$WSX_DEMO_ROOT")"; find "$HOME/.claude/projects" -maxdepth 1 -type l -lname "$WSX_DEMO_ROOT/*" -delete 2>/dev/null || true' EXIT
"$(dirname "$0")/bootstrap.sh" >/dev/null
export XDG_STATE_HOME="$WSX_DEMO_ROOT/state"
test -f "$XDG_STATE_HOME/wsx/state.db" || { echo "FAIL: no isolated db"; exit 1; }
wsx repo list | grep -q toy-api || { echo "FAIL: toy-api not registered"; exit 1; }
wsx repo list | grep -q toy-cli || { echo "FAIL: toy-cli not registered"; exit 1; }
test -f "$WSX_DEMO_ROOT/claude-config/settings.json" || { echo "FAIL: no isolated claude settings"; exit 1; }
grep -q skipDangerousModePermissionPrompt "$WSX_DEMO_ROOT/claude-config/settings.json" || { echo "FAIL: bypass flag not set"; exit 1; }
grep -q hasTrustDialogAccepted "$WSX_DEMO_ROOT/claude-config/.claude.json" || { echo "FAIL: trust not pre-seeded"; exit 1; }
grep -q 'trust_level = "trusted"' "$WSX_DEMO_ROOT/codex-home/config.toml" || { echo "FAIL: codex trust not pre-seeded"; exit 1; }
# session-log bridge symlink exists in ~/.claude/projects and points into the sandbox
enc="$(printf '%s' "$XDG_STATE_HOME/wsx/worktrees/toy-api/add-rate-limit" | sed 's#^/##; s#[^A-Za-z0-9]#-#g; s#^#-#')"
test -L "$HOME/.claude/projects/$enc" || { echo "FAIL: session-log symlink not created"; exit 1; }
case "$(readlink "$HOME/.claude/projects/$enc")" in "$WSX_DEMO_ROOT"/*) : ;; *) echo "FAIL: symlink points outside sandbox"; exit 1;; esac
echo "PASS"
