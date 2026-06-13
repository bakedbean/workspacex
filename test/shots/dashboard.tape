# Example screenshot tape for `test/harness.sh shot`. VHS Env lines do NOT expand
# variables, so these paths are hardcoded to the harness default sandbox
# (/tmp/wsx-test). If you override WSX_SANDBOX_ROOT, copy this tape and update the
# three Env paths + the Type line to match.
Output test/out/dashboard.gif
Set Width 1200
Set Height 700
Set Shell "bash"
Env XDG_STATE_HOME "/tmp/wsx-test/state"
Env CLAUDE_CONFIG_DIR "/tmp/wsx-test/claude-config"
Env CODEX_HOME "/tmp/wsx-test/codex-home"
Type "target/debug/wsx"
Enter
Sleep 3s
Screenshot test/out/dashboard.png
Sleep 1s
