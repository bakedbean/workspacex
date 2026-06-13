#!/usr/bin/env bash
# Stand up an isolated wsx install with synthetic repos registered, plus an
# isolated, pre-authenticated, pre-accepted Claude config so live agents spawn
# with zero interactive prompts. Everything lives under $WSX_SANDBOX_ROOT; the real
# ~/.local/state, ~/.claude.json, and ~/.claude/settings.json are never touched.
# The ONLY thing written outside the sandbox is a set of transient symlinks under
# ~/.claude/projects (so wsx's detail bars can find the isolated session logs);
# `make clean` removes them.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
# WSX_SANDBOX_ROOT is the canonical var; WSX_DEMO_ROOT is a back-compat fallback.
export WSX_SANDBOX_ROOT="${WSX_SANDBOX_ROOT:-${WSX_DEMO_ROOT:-/tmp/wsx-demo}}"
# Canonicalize (resolve any `..`/symlinks) BEFORE the destructive-path guards, so a
# value like /tmp/wsx-test/.. can't slip past the string checks and resolve to a
# parent that `rm -rf` would then wipe. `-m` tolerates a not-yet-existing path.
WSX_SANDBOX_ROOT="$(realpath -m -- "$WSX_SANDBOX_ROOT")"
export WSX_SANDBOX_ROOT
# Guard: this script `rm -rf`s WSX_SANDBOX_ROOT, and it's overridable. Refuse to run
# with an empty value, an obviously catastrophic root (/, /tmp, $HOME), or any path
# shallower than two levels — a sandbox is always nested (e.g. /tmp/wsx-demo).
case "$WSX_SANDBOX_ROOT" in
  ""|/|/.|//|/tmp|"$HOME") echo "FATAL: unsafe WSX_SANDBOX_ROOT='$WSX_SANDBOX_ROOT'" >&2; exit 1;;
esac
case "$WSX_SANDBOX_ROOT" in
  */*/*) : ;;
  *) echo "FATAL: WSX_SANDBOX_ROOT too shallow to remove safely: '$WSX_SANDBOX_ROOT'" >&2; exit 1;;
esac
export XDG_STATE_HOME="$WSX_SANDBOX_ROOT/state"
export CLAUDE_CONFIG_DIR="$WSX_SANDBOX_ROOT/claude-config"
export CODEX_HOME="$WSX_SANDBOX_ROOT/codex-home"
REPOS="$WSX_SANDBOX_ROOT/repos"
WSX_BIN="${WSX_BIN:-wsx}"

# Fresh state each run.
rm -rf -- "$WSX_SANDBOX_ROOT"
mkdir -p "$XDG_STATE_HOME" "$REPOS" "$CLAUDE_CONFIG_DIR" "$CODEX_HOME"

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
touch "$CODEX_HOME/config.toml"
# Append a per-repo-root trust block only if that exact table header isn't already
# present — re-appending would create a duplicate TOML table (invalid TOML, which
# makes Codex fail to parse the config).
for r in toy-api toy-cli; do
  hdr="[projects.\"$REPOS/$r\"]"
  if ! grep -qF "$hdr" "$CODEX_HOME/config.toml"; then
    printf '\n[projects."%s"]\ntrust_level = "trusted"\n' "$REPOS/$r" >> "$CODEX_HOME/config.toml"
  fi
done

# --- Install the wsx agent skill into the isolated configs ---
# `wsx setup install-skill` writes to ~/.claude/skills and ~/.codex/skills via
# dirs::home_dir() (no env override), so it can't target the sandbox. Copy the
# same embedded skill (skills/wsx/SKILL.md) into the isolated dirs directly, so
# the demo agents know how to coordinate over the wsx CLI (wsx agent send) —
# without touching the real ~/.claude / ~/.codex.
SKILL_SRC="$(cd "$HERE/.." && pwd)/skills/wsx/SKILL.md"
if [ -f "$SKILL_SRC" ]; then
  mkdir -p "$CLAUDE_CONFIG_DIR/skills/wsx" "$CODEX_HOME/skills/wsx"
  cp "$SKILL_SRC" "$CLAUDE_CONFIG_DIR/skills/wsx/SKILL.md"
  cp "$SKILL_SRC" "$CODEX_HOME/skills/wsx/SKILL.md"
else
  echo "WARN: skills/wsx/SKILL.md not found — agents won't have the wsx skill." >&2
fi

# --- Synthetic repos ---
"$HERE/gen-repos.sh" "$REPOS"
"$WSX_BIN" repo add "$REPOS/toy-api" --name toy-api --prefix demo
"$WSX_BIN" repo add "$REPOS/toy-cli" --name toy-cli --prefix demo
# Set the base branch explicitly. wsx's per-workspace diff poll (which powers the
# dashboard +N/-M column and the RECENT FILES +X −Y counts) only runs when a
# repo's base_branch is Some — `repo add` leaves it None. The repos are created
# on `main` (gen-repos.sh: git init -b main), so point base_branch there.
"$WSX_BIN" repo set-base-branch toy-api main
"$WSX_BIN" repo set-base-branch toy-cli main

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
  "$WORKTREES/toy-api/null-guard"
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

# --- Bridge session logs to where wsx reads them (for the detail bars) ---
# The workspace detail bars (SESSION SUMMARY / RECENT CHAT) are built by reading
# claude's session jsonl from ~/.claude/projects/<encode(worktree)> — and wsx
# always uses dirs::home_dir() (real $HOME), with no env override. But our
# isolated CLAUDE_CONFIG_DIR sends claude's logs to $CLAUDE_CONFIG_DIR/projects.
# So we symlink each demo worktree's (isolated) log dir into ~/.claude/projects.
# This touches ONLY ~/.claude/projects (transient symlinks pointing into the
# sandbox) — never the real ~/.claude.json / settings.json. `make clean` removes
# them. (The encoding replaces every non-alphanumeric char with '-'.)
#
# NOTE: this only matters because the agents must also run as TOP-LEVEL claude
# sessions to persist a jsonl at all — see render.sh, which clears the
# CLAUDECODE/CLAUDE_CODE_* parent-session markers before launching VHS.
encode_path() { printf '%s' "$1" | sed 's#^/##; s#[^A-Za-z0-9]#-#g; s#^#-#'; }
REAL_PROJECTS="$HOME/.claude/projects"
mkdir -p "$REAL_PROJECTS"
# Clear stale demo symlinks from a previous run.
find "$REAL_PROJECTS" -maxdepth 1 -type l -lname "$WSX_SANDBOX_ROOT/*" -delete 2>/dev/null || true
for p in "${DEMO_PATHS[@]}"; do
  enc="$(encode_path "$p")"
  mkdir -p "$CLAUDE_CONFIG_DIR/projects/$enc"
  ln -sfn "$CLAUDE_CONFIG_DIR/projects/$enc" "$REAL_PROJECTS/$enc"
done
echo "bridged ${#DEMO_PATHS[@]} session-log dirs into ~/.claude/projects (symlinks)"

echo "sandbox ready at $WSX_SANDBOX_ROOT"
echo "  XDG_STATE_HOME=$XDG_STATE_HOME"
echo "  CLAUDE_CONFIG_DIR=$CLAUDE_CONFIG_DIR"
echo "  CODEX_HOME=$CODEX_HOME"
