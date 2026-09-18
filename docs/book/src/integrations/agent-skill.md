```
wsx setup install-skill
```

Writes the [bundled skills](#bundled-skills) to each detected agent's skills directory — `~/.claude/skills/<skill>/SKILL.md` and the equivalent under `~/.codex` / `~/.hermes`. Claude is always targeted; Codex and Hermes are added when detected. The skills are embedded in the binary at compile time, so installing wsx on a new machine is `cargo install` then `wsx setup install-skill`.

Codex is considered installed when `WSX_CODEX_BIN` is set, `codex` is on `PATH`, or `~/.codex` already exists; Hermes likewise via `WSX_HERMES_BIN`, `hermes` on `PATH`, or `~/.hermes`.

There is intentionally no separate target for `pi` or `omp`. Both read skills from `~/.claude/skills` — omp via its Claude discovery provider, which loads `~/.claude/skills/*/SKILL.md` (and `~/.claude/commands/*.md` as slash commands) — so the Claude target already covers them. omp 18 turned that user-level scan off by default, so wsx passes a config overlay on every omp spawn to turn it back on; see [Coding agents](../configuration/coding-agents.md).

Idempotent: re-running when an installed copy already matches reports "already up to date" without writing. If an installed copy has drifted (you edited it locally, or you're upgrading wsx with skill changes), it's overwritten and reports "updated".

### Bundled skills

`wsx setup install-skill` installs every bundled skill for each detected agent:

- **`wsx`** — drives the wsx CLI (workspace ops, slug-vs-`branch_prefix` naming, cross-repo orchestration).
- **`agent-review`** — run inside a workspace to spin up a peer review agent. It takes the reviewer kind (`claude` | `pi` | `hermes` | `codex` | `omp`; asks when omitted), spawns it with `wsx agent add`, hands it the branch diff vs `main`, and has it report a risk assessment + gap analysis back via `wsx agent send`.
- **`handoff`** — run inside a *finished* workspace (PR merged, work wrapping up) to continue the same feature or epic in a fresh workspace. The agent asks what the new workspace should implement (or takes it as the argument: `/handoff add a --json flag`), creates a same-repo workspace with `wsx workspace create <repo> --name <slug>`, and briefs its primary agent with a distilled summary of the session — decisions and why, rejected approaches, gotchas, file:line pointers, follow-ups — plus the request as its task. Reply `defer` to have the new agent wait for your request instead. The outgoing agent sets itself `done` and does not archive the old workspace.

Pin either skill to a chip so it is one click away — add a line to your [pinned commands](../daily-use/pinned-commands.md). Use `wsx config edit pinned_commands` to append without clobbering existing chips (`wsx config set` replaces the whole value):

```
agent-review=/agent-review ...
handoff=/handoff
```

The `agent-review` line ends in `...`, so the chip types `/agent-review ` and waits for you to add the reviewer kind (`codex`, `omp`, …) before pressing enter; press enter with nothing and the skill asks which kind to spawn. Pin it as a plain `/agent-review` if you'd rather always be asked. The `handoff` chip runs `/handoff` with no request, so the agent asks for one before creating the workspace — the question is the chip's way of taking input.

