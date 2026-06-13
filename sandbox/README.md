# sandbox/ — isolated wsx, for demos and tests

Stands up a fully isolated, live `wsx` install with synthetic repos and
pre-authenticated agents, so anything that needs to *run the real app* — the
screencast recordings under `demo/` and the e2e harness under `test/` — can build
on it. Nothing here touches your real `~/.local/state/wsx`, `~/.claude.json`,
`~/.claude/settings.json`, or `~/.codex`.

## Pieces

| File | Responsibility |
|---|---|
| `bootstrap.sh` | Provision a fresh sandbox: isolated wsx state + synthetic repos + pre-authed/pre-trusted Claude & Codex configs + the wsx agent skill + session-log bridging. Wipes and recreates the sandbox root each run. |
| `gen-repos.sh` | Generate the synthetic `toy-api` / `toy-cli` repos with deliberately planted bugs. |
| `env.sh` | `source` it to re-enter an already-provisioned sandbox (exports the env contract; provisions/wipes nothing). |
| `render.sh` | Drive the sandboxed TUI under [VHS](https://github.com/charmbracelet/vhs) with agent session-markers cleared. The reusable basis for both screencasts and image screenshots. |
| `agent-env.sh` | Single source of truth for the parent-session env markers that must be cleared so spawned agents run as top-level sessions. |

## Env contract

| Var | Meaning | Default |
|---|---|---|
| `WSX_SANDBOX_ROOT` | Root of the sandbox; everything lives under it. `WSX_DEMO_ROOT` is honored as a back-compat fallback. | `/tmp/wsx-demo` |
| `WSX_BIN` | The `wsx` binary `bootstrap.sh` provisions with — point it at a local build to exercise local changes. | `wsx` (PATH) |
| `XDG_STATE_HOME` | Isolated wsx `state.db`, worktrees, logs. | `$WSX_SANDBOX_ROOT/state` |
| `CLAUDE_CONFIG_DIR` | Isolated Claude config (copied creds + settings + per-worktree trust). | `$WSX_SANDBOX_ROOT/claude-config` |
| `CODEX_HOME` | Isolated Codex config (copied auth + per-repo trust). | `$WSX_SANDBOX_ROOT/codex-home` |

## Usage

```bash
bash sandbox/bootstrap.sh            # provision a fresh sandbox at $WSX_SANDBOX_ROOT
source sandbox/env.sh                # re-enter it in another shell
WSX_BIN=./target/debug/wsx bash sandbox/bootstrap.sh   # provision with a local build
```

## What it writes outside the sandbox

Only a set of **transient symlinks** under `~/.claude/projects/<encoded-worktree>`,
pointing into the sandbox — these bridge the isolated session logs to where wsx reads
them (`dirs::home_dir()`, no env override) so the workspace detail bars populate. They
are removed by `demo`'s `make clean` and the test harness's `harness.sh down`.

## Tests

`bash sandbox/test-gen-repos.sh`, `bash sandbox/test-bootstrap.sh`,
`bash sandbox/test-agent-env.sh`, `bash sandbox/test-env.sh` — no recording, just the
provisioning pieces.
