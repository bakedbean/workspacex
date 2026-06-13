# Clip 1 — Hero: autonomous agent-to-agent coordination over the wsx CLI.
# Create a workspace, put Claude + Codex on it, and have CLAUDE delegate the fix
# to CODEX via `wsx agent send` — Codex fixes, commits, and reports back, all
# without the user in the loop. Rendered via sandbox/render.sh.
Output demo/out/01-hero-raw.mp4
Set Shell "bash"
Set FontSize 13
Set Width 1280
Set Height 720
Set Padding 14
Set TypingSpeed 40ms
Set WaitTimeout 150s

Env XDG_STATE_HOME "/tmp/wsx-demo/state"
Env CLAUDE_CONFIG_DIR "/tmp/wsx-demo/claude-config"
Env CODEX_HOME "/tmp/wsx-demo/codex-home"

# --- INTRO: spin up the workspace from the dashboard ---
Type "wsx"
Enter
Sleep 3s
Type "N"
Sleep 1200ms
Type "security-review"
Sleep 600ms
Enter
Sleep 2s
Enter
Wait+Screen /bypass permissions on/
Sleep 1s

# --- Put a second agent (Codex) on the same workspace ---
Ctrl+x
Type "a"
Sleep 1500ms
Down
Down
Down
Sleep 700ms
Enter
Sleep 11s
Screenshot demo/out/01-agents.png

# --- Claude reviews and delegates the fix to Codex over the wsx CLI ---
Type "Review src/auth.py for the most serious security bug. Then hand the fix to your Codex teammate using `wsx agent send codex`: give it the exact bug, the location, and a parameterized-query fix, and ask it to fix, commit, and report back. Then wait for Codex's reply."
Enter
Wait+Screen@120s /queued message to codex/
Sleep 3s
Screenshot demo/out/01-claude-delegates.png

# --- Switch to Codex: show the handoff, submit it, let it work + report back ---
Ctrl+x
Type "w"
Sleep 4s
Screenshot demo/out/01-codex-handoff.png
Enter
Sleep 52s
Screenshot demo/out/01-codex-reports.png

# --- Switch back to Claude: it received Codex's report and verifies the fix ---
Ctrl+x
Type "q"
Sleep 14s
Screenshot demo/out/01-claude-verifies.png

# --- OUTRO: detach to the dashboard ---
Ctrl+x
Type "d"
Sleep 8s
Screenshot demo/out/01-outro.png
Sleep 3s
