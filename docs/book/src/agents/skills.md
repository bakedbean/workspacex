# Agent skills

wsx bundles skills that teach agents to drive wsx: creating workspaces, messaging peers, and handing work off. Install them once per machine.

```bash
wsx setup install-skill
```

This writes every bundled skill to each agent's skills directory:

| Agent            | Directory           | When                                                        |
| ---------------- | ------------------- | ----------------------------------------------------------- |
| claude, pi, omp  | `~/.claude/skills/` | Always                                                      |
| codex            | `~/.codex/skills/`  | `codex` is on `PATH`, `WSX_CODEX_BIN` is set, or `~/.codex` exists |
| hermes           | `~/.hermes/skills/` | `hermes` is on `PATH`, `WSX_HERMES_BIN` is set, or `~/.hermes` exists |

Re-run it after upgrading wsx. It reports `already up to date` for unchanged skills and overwrites any copy that differs, including your local edits. omp reads `~/.claude/skills` through a config overlay wsx passes. See [Coding agents](coding-agents.md#omp).

## Bundled skills

| Skill          | Use it to                                                                                  |
| -------------- | ------------------------------------------------------------------------------------------ |
| `wsx`          | Drive the wsx CLI: workspaces, naming, peer agents, and work across related repos          |
| `agent-review` | Start a peer agent that reviews the current branch against `main` and reports back to you  |
| `handoff`      | Continue a finished workspace's feature in a fresh workspace                               |

**`agent-review`** takes the reviewer kind (`claude`, `pi`, `hermes`, `codex`, or `omp`) and asks if you omit it. It adds the reviewer with `wsx agent add` and gives it the branch diff. The reviewer runs its harness's `code-review` skill when there is one, then replies with a risk assessment and gap analysis.

**`handoff`** takes what the new workspace should implement, or asks for it:

```
/handoff add a --json flag
```

It creates a workspace in the same repo and briefs its agent with a summary of this session: decisions, rejected approaches, gotchas, file pointers, and follow-ups. Reply `defer` to have the new agent wait for your request. The old agent marks itself `done` and doesn't archive its workspace.

## Pinning skills as chips

Add skills to your [pinned commands](../attached-view/pinned-commands.md) with `wsx config edit pinned_commands`:

```
agent-review=/agent-review ...
handoff=/handoff
```

- The `...` makes the `agent-review` chip type `/agent-review ` and wait for you to add the reviewer kind.
- The `handoff` chip runs with no request, so the agent asks what to implement.
- In omp, wsx sends a chip naming a bundled skill as `/skill:<name>`, because omp's own `/handoff` would catch it. Pin the plain `/handoff` form. Typing `/handoff` yourself in omp still runs omp's built-in command.
