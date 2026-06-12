#!/usr/bin/env bash
# Stand up an isolated wsx install with synthetic repos registered, plus an
# isolated, pre-authenticated, pre-accepted Claude config so live agents spawn
# with zero interactive prompts. Everything lives under $WSX_DEMO_ROOT; the real
# ~/.local/state and ~/.claude are never touched.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
export WSX_DEMO_ROOT="${WSX_DEMO_ROOT:-/tmp/wsx-demo}"
export XDG_STATE_HOME="$WSX_DEMO_ROOT/state"
export CLAUDE_CONFIG_DIR="$WSX_DEMO_ROOT/claude-config"
export CODEX_HOME="$WSX_DEMO_ROOT/codex-home"
export PI_CODING_AGENT_DIR="$WSX_DEMO_ROOT/pi-agent"
REPOS="$WSX_DEMO_ROOT/repos"

# Fresh state each run.
rm -rf "$WSX_DEMO_ROOT"
mkdir -p "$XDG_STATE_HOME" "$REPOS" "$CLAUDE_CONFIG_DIR" "$CODEX_HOME" "$PI_CODING_AGENT_DIR"

# --- Isolated Claude config (auth + bypass pre-accepted) ---
# Copy credentials so the demo agents are authenticated without a login prompt.
if [ -f "$HOME/.claude/.credentials.json" ]; then
  cp -a "$HOME/.claude/.credentials.json" "$CLAUDE_CONFIG_DIR/.credentials.json"
else
  echo "WARN: ~/.claude/.credentials.json not found — demo agents may not be authenticated." >&2
fi
# Copy app-state (onboarding/theme flags) so the TUI doesn't run first-run onboarding.
[ -f "$HOME/.claude.json" ] && cp -a "$HOME/.claude.json" "$CLAUDE_CONFIG_DIR/.claude.json"
# Pre-accept --dangerously-skip-permissions so the bypass warning never appears.
python3 - "$CLAUDE_CONFIG_DIR" <<'PY'
import json, os, sys
d = sys.argv[1]
src = os.path.expanduser("~/.claude/settings.json")
data = {}
if os.path.exists(src):
    try:
        data = json.load(open(src))
    except Exception:
        data = {}
data["skipDangerousModePermissionPrompt"] = True
json.dump(data, open(os.path.join(d, "settings.json"), "w"), indent=2)
PY

# --- Isolated Codex config (auth + per-repo-root trust pre-accepted) ---
# Codex authenticates from auth.json and gates fresh repo roots behind a trust
# prompt; both relocate with CODEX_HOME. Pre-seed trust for the demo repo roots.
if [ -f "$HOME/.codex/auth.json" ]; then
  cp -a "$HOME/.codex/auth.json" "$CODEX_HOME/auth.json"
else
  echo "WARN: ~/.codex/auth.json not found — demo Codex agent may not be authenticated." >&2
fi
[ -f "$HOME/.codex/config.toml" ] && cp -a "$HOME/.codex/config.toml" "$CODEX_HOME/config.toml"
{
  printf '\n[projects."%s"]\ntrust_level = "trusted"\n' "$REPOS/toy-api"
  printf '\n[projects."%s"]\ntrust_level = "trusted"\n' "$REPOS/toy-cli"
} >> "$CODEX_HOME/config.toml"

# --- Isolated Pi config (copy the agent dir; Pi runs offline via PI_OFFLINE) ---
if [ -d "$HOME/.pi/agent" ]; then
  cp -a "$HOME/.pi/agent/." "$PI_CODING_AGENT_DIR/"
else
  echo "WARN: ~/.pi/agent not found — demo Pi agent may not be configured." >&2
fi

# --- Synthetic repos ---
"$HERE/gen-repos.sh" "$REPOS"
wsx repo add "$REPOS/toy-api" --name toy-api --prefix demo
wsx repo add "$REPOS/toy-cli" --name toy-cli --prefix demo

# --- Pre-seed Claude trust for the worktree paths the demo tapes attach to ---
# Claude gates a fresh folder behind a "do you trust this folder?" dialog that
# --dangerously-skip-permissions does NOT bypass, and which it does not reliably
# persist. Worktree paths are deterministic ($XDG_STATE_HOME/wsx/worktrees/<repo>/<slug>),
# so we mark them trusted up front and the dialog never appears on camera.
# These (repo/slug) pairs MUST match the slugs used in demo/tapes/*.tape.
WORKTREES="$XDG_STATE_HOME/wsx/worktrees"
DEMO_PATHS=(
  "$WORKTREES/toy-api/security-review"
  "$WORKTREES/toy-api/add-rate-limit"
  "$WORKTREES/toy-api/fix-auth"
  "$WORKTREES/toy-cli/arg-parsing"
)
python3 - "$CLAUDE_CONFIG_DIR/.claude.json" "${DEMO_PATHS[@]}" <<'PY'
import json, os, sys
cfg, paths = sys.argv[1], sys.argv[2:]
data = {}
if os.path.exists(cfg):
    try:
        data = json.load(open(cfg))
    except Exception:
        data = {}
projs = data.setdefault("projects", {})
for p in paths:
    e = projs.setdefault(p, {})
    e["hasTrustDialogAccepted"] = True
    e.setdefault("hasClaudeMdExternalIncludesApproved", False)
    e.setdefault("hasClaudeMdExternalIncludesWarningShown", False)
json.dump(data, open(cfg, "w"), indent=2)
print(f"pre-trusted {len(paths)} demo worktree paths")
PY

echo "sandbox ready at $WSX_DEMO_ROOT"
echo "  XDG_STATE_HOME=$XDG_STATE_HOME"
echo "  CLAUDE_CONFIG_DIR=$CLAUDE_CONFIG_DIR"
echo "  CODEX_HOME=$CODEX_HOME"
echo "  PI_CODING_AGENT_DIR=$PI_CODING_AGENT_DIR"
