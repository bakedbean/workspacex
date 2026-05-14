# wsx

Terminal UI for managing Claude Code sessions in git worktrees.

## Quick start

```bash
cargo build --release
./target/release/wsx repo add /path/to/your/repo --name myrepo --prefix wsx
./target/release/wsx              # launch TUI
```

In the TUI:

- `n` — new workspace
- `enter` — attach to the selected workspace's `claude` session (spawns if not running)
- `Ctrl-a d` — detach back to dashboard
- `d` — archive the selected workspace
- `q` — quit (kills all running sessions)

## Configuration

State lives at `$XDG_STATE_HOME/wsx/state.db`. Worktrees are created
under `$XDG_STATE_HOME/wsx/worktrees/<repo>/<workspace>`.

A `.claudette.json` file in the repo root is honored for setup and
archive scripts:

```json
{
  "setup":   { "command": "bun",  "args": ["install"] },
  "archive": { "command": "rm",   "args": ["-rf", "node_modules"] }
}
```

## Testing

```bash
cargo test -- --test-threads=1
```

The test suite substitutes `claude` with `cat` via `WSX_CLAUDE_BIN`, so it runs without Claude Code installed. `--test-threads=1` is required because several tests mutate the global `WSX_CLAUDE_BIN` env var.
