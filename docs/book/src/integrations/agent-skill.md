```
wsx setup install-skill
```

Writes the [bundled skills](#bundled-skills) to each detected agent's skills directory — `~/.claude/skills/<skill>/SKILL.md` and the equivalent under `~/.codex` / `~/.hermes`. Claude is always targeted; Codex and Hermes are added when detected. The skills are embedded in the binary at compile time, so installing wsx on a new machine is `cargo install` then `wsx setup install-skill`.

Codex is considered installed when `WSX_CODEX_BIN` is set, `codex` is on `PATH`, or `~/.codex` already exists; Hermes likewise via `WSX_HERMES_BIN`, `hermes` on `PATH`, or `~/.hermes`.

Idempotent: re-running when an installed copy already matches reports "already up to date" without writing. If an installed copy has drifted (you edited it locally, or you're upgrading wsx with skill changes), it's overwritten and reports "updated".

### Bundled skills

`wsx setup install-skill` installs every bundled skill for each detected agent:

- **`wsx`** — drives the wsx CLI (workspace ops, slug-vs-`branch_prefix` naming, cross-repo orchestration).
- **`agent-review`** — run inside a workspace to spin up a peer review agent. It takes the reviewer kind (`claude` | `pi` | `hermes` | `codex`, default `claude`), spawns it with `wsx agent add`, hands it the branch diff vs `main`, and has it report a risk assessment + gap analysis back via `wsx agent send`.

Pin `agent-review` to a chip so a review is one click away — add a line to your [pinned commands](../daily-use/pinned-commands.md). Use `wsx config edit pinned_commands` to append without clobbering existing chips (`wsx config set` replaces the whole value):

```
agent-review=/agent-review
```

Because chips auto-submit, the chip runs `/agent-review` (defaulting to a `claude` reviewer); type `/agent-review codex` manually for a different kind.
