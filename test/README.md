# test/ — thin e2e harness for the live wsx app

Lets an agent (or you) run the **real** wsx against an isolated sandbox and assert on
it. Built on `sandbox/` (see `../sandbox/README.md` for the env contract). Default
mode is headless CLI + state inspection; TUI snapshots are available when a visual
check helps.

## Quick start

```bash
test/harness.sh up                                   # build local wsx + provision a fresh sandbox at /tmp/wsx-test
test/harness.sh wsx workspace create toy-api --name foo
test/harness.sh wsx workspace list toy-api           # CLI assertion surface
test/harness.sh state                                 # default state.db summary
test/harness.sh state "SELECT * FROM workspaces;"     # arbitrary query
test/harness.sh capture /tmp/screen.txt               # tmux text snapshot of the TUI
test/harness.sh capture l /tmp/screen.txt             # send keys (here 'l') before snapshotting
test/harness.sh shot test/shots/dashboard.tape        # VHS PNG screenshot (needs vhs) -> test/out/
test/harness.sh down                                  # wipe sandbox + bridged ~/.claude symlinks
```

The harness always drives a **locally built** `target/debug/wsx` (via `WSX_BIN`), so
tests exercise your changes — not the installed `wsx`. It uses `/tmp/wsx-test` so it
never collides with a `demo/` recording at `/tmp/wsx-demo`.

## Worked example

`test/smoke.sh` provisions, creates a workspace, and asserts it via the CLI, the
state.db, and a tmux text capture. Run it: `bash test/smoke.sh` → `SMOKE PASS`. Copy it
as the starting point for new e2e checks.

## Dependencies

`up`/`wsx`/`state` need `wsx` (built via `cargo`), `git`, `python3`, `sqlite3`.
`capture` adds `tmux`. `shot` adds `vhs` (+ `ttyd`, headless `chromium`). Each snapshot
subcommand prints a clear "not installed" message when its tool is missing, so the CLI
path works on a bare host.
