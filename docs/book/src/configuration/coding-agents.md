By default, wsx spawns Claude Code (`claude`) as the coding agent in every workspace. You can choose a different agent per-workspace or set a global default:

```bash
wsx config set coding_agent hermes           # new workspaces use hermes by default
wsx workspace create backend --agent pi      # override for a single workspace
```

Supported agents:

| Agent              | CLI option       | Source                                                                    | Config                                    |
| ------------------ | ---------------- | ------------------------------------------------------------------------- | ----------------------------------------- |
| `claude` (default) | `--agent claude` | `claude` binary (override via `WSX_CLAUDE_BIN`)                           | Environment + `~/.claude.json` MCP        |
| `pi`               | `--agent pi`     | `pi` binary, [`@earendil-works/pi-coding-agent`](https://github.com/badlogic/pi-mono) (override via `WSX_PI_BIN`) | `~/.pi/`                 |
| `hermes`           | `--agent hermes` | [nousresearch/hermes-agent](https://github.com/nousresearch/hermes-agent) | `~/.hermes/config.yaml` (provider, model) |
| `codex`            | `--agent codex`  | `codex` binary (override via `WSX_CODEX_BIN`)                             | `~/.codex/config.toml`                    |
| `omp`              | `--agent omp`    | `omp` binary, [oh-my-pi](https://github.com/can1357/oh-my-pi) (override via `WSX_OMP_BIN`) | `~/.omp/agent/config.yml` |

### Hermes integration

When a workspace uses `coding_agent: hermes`, wsx spawns `hermes` (or the path in `WSX_HERMES_BIN`) instead of `claude`. Hermes runs in classic REPL mode and receives wsx custom instructions and auto-rename directives.

**AGENTS.md management**: Because Hermes lacks a `--append-system-prompt` flag, wsx injects instructions into a fenced block at the end of `AGENTS.md` in the worktree's working directory:

```markdown
<!-- BEGIN wsx-managed -->

…injected instructions…

<!-- END wsx-managed -->
```

The block is rewritten every time Hermes spawns and automatically cleaned up when there's nothing to inject. This approach works whether or not the repository tracks `AGENTS.md` in git:

- **Untracked `AGENTS.md`**: wsx adds it to `.git/info/exclude` so it doesn't show up in `git status`.
- **Tracked `AGENTS.md`**: the worktree will show the file as modified during a Hermes spawn — this is expected and the modification disappears on subsequent spawns when there's no custom instructions to inject.

**Session detection**: On every Hermes spawn, wsx writes a timestamp marker at `<worktree>/.git/info/wsx-hermes-spawn-at` (per-worktree-local, never committed). To find the active Hermes session for a worktree, wsx queries `~/.hermes/state.db` for the most recent session started at or after that timestamp (with a 2-second look-back buffer to absorb clock skew). This drives both the prior-session indicator on the dashboard and the `--resume <id>` flag on Continue spawns. Note: if two worktrees both spawn Hermes within a few seconds of each other, the lookup is best-effort — the more-recent session could be attributed to either worktree depending on timing.

**Session-tail**: wsx tails `~/.hermes/state.db` (sqlite) to populate the dashboard's RECENT CHAT, SESSION SUMMARY, and last-message columns for Hermes workspaces. The following fields are populated: last assistant text, first user prompt, stop reason, tool-use counts, and per-event snapshots (user messages, assistant text, and tool calls — including `ran \`<cmd>\`` display for terminal/bash tool invocations). Tool-use counts treat all Hermes tool names as "other" for now — categorization into read/edit/write/bash buckets is a follow-up since Hermes uses lowercase tool names rather than Claude's capitalized convention. Still missing compared to Claude/Pi: edited-files tracking and pending-tool-use timing for permission-prompt detection.

**Environment overrides**: configure Hermes via `~/.hermes/config.yaml` (persistent settings), or set `WSX_HERMES_MODEL` and `WSX_HERMES_PROVIDER` to override per-workspace:

```bash
WSX_HERMES_MODEL=llama-3-70b-instruct WSX_HERMES_PROVIDER=together wsx workspace create backend --agent hermes
```

### Codex integration

When a workspace uses `coding_agent: codex`, wsx spawns `codex` (or the path in `WSX_CODEX_BIN`) instead of `claude`. Codex receives wsx custom instructions and auto-rename directives.

**Instruction injection**: Codex has no `--append-system-prompt` flag, so wsx passes the workspace doctrine, the auto-rename hint, and any custom instructions as a Codex config override on the spawn command line:

```bash
codex -c 'developer_instructions="…injected instructions…"' \
      -c 'project_doc_fallback_filenames=["CLAUDE.md"]'
```

Codex renders `developer_instructions` as the first developer-role message, ahead of its own instructions and ahead of the user-role message that carries `AGENTS.md`. **Nothing is written to your worktree** — no `AGENTS.md`, no `.git/info/exclude` entry. A repo's own `AGENTS.md` is still read by Codex as usual, and `project_doc_fallback_filenames` makes Codex fall back to `CLAUDE.md` in repos that have no `AGENTS.md`. The superpowers-skills doctrine clause is omitted for Codex (those skills install under `~/.claude` and Codex can't load them).

Both overrides are applied only to **fresh** spawns. `codex resume --last` restores the session's stored configuration and ignores these two keys, so a resumed session keeps the doctrine it was started with. It also means edits to a workspace's custom instructions or related-repo context never reach an already-started Codex session — re-attaching with `resume --last` after editing them won't pick up the change, since only a fresh spawn re-composes the `-c` overrides. Requires Codex `0.146.0` or newer.

If a worktree was used with an older wsx, it may contain a wsx-created `AGENTS.md`; deleting it lets the new `CLAUDE.md` fallback work.

**Claude slash commands**: before each Codex spawn, wsx mirrors Markdown files from `~/.claude/commands/` into a local Codex plugin at `~/plugins/wsx-claude-commands/commands/` and registers that plugin in the implicit personal marketplace at `~/.agents/plugins/marketplace.json`. The marketplace entry is marked `INSTALLED_BY_DEFAULT`, so commands such as `/pull-request` and `/commit-changes` are available in Codex without maintaining a second command set. Edits to the Claude command files are picked up on the next Codex spawn.

**Spawn**: fresh workspaces launch bare `codex`. Non-yolo sessions use Codex's built-in interactive approvals + workspace-write sandbox; `--yolo` workspaces add `--dangerously-bypass-approvals-and-sandbox`.

**Continue**: `codex resume --last`, which Codex filters to the current directory natively — so wsx resumes the worktree's own most-recent session.

**Activity**: the dashboard detail bar tails the worktree's rollout file under `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`. RECENT FILES is not yet populated for Codex (file edits are inferred-via-shell and not tracked).

**Model**: set `WSX_CODEX_MODEL` to pass `-m <model>` to Codex (e.g. `gpt-5.4`). Unset = Codex default.

### Oh My Pi integration

`omp` is [oh-my-pi](https://github.com/can1357/oh-my-pi)
(`@oh-my-pi/pi-coding-agent`). It is **not** the same harness as `pi`, which is
`@earendil-works/pi-coding-agent`. The two share ancestry — which is why they
write the same session-file format — but they are separately maintained, have
different CLIs, and can both be installed at once. `--agent pi` and `--agent
omp` mean different binaries.

**Spawn**: fresh workspaces launch bare `omp`. Non-yolo sessions inherit
whatever `tools.approvalMode` you configured; `--yolo` workspaces add
`--approval-mode yolo`.

**Continue**: `omp -c`. omp resolves `--continue` against the session directory
for the current cwd, so this resumes the worktree's own most-recent session
without wsx needing a marker file or a database query.

**Instructions**: doctrine, the auto-rename directive, and a workspace's custom
instructions compose into a single `--append-system-prompt`. Related-repo paths
ride on `--add-dir`. omp is the only harness besides Claude that supports both
flags, so nothing is written into the worktree — no `AGENTS.md` block (unlike
Hermes) and no config overrides (unlike Codex).

**Skills and slash commands work with no setup.** omp's Claude discovery
provider loads `~/.claude/skills/*/SKILL.md` and `~/.claude/commands/*.md`
natively, so the skills installed by `wsx setup install-skill` and your pinned
command chips both reach omp unchanged. There is deliberately no separate omp
skills target — the Claude one already covers it, for the same reason it covers
Pi.

**Session detection and activity**: omp stores sessions at
`~/.omp/agent/sessions/<encoded-cwd>/<ts>_<uuid>.jsonl`, where the directory
name is the cwd with `$HOME` (or the temp root) stripped and `/` collapsed to
`-`. Because omp writes the same JSONL schema pi does, wsx reuses the pi parser,
so RECENT CHAT, SESSION SUMMARY, tool-use counts and the last-message column are
populated exactly as they are for Pi. Like Claude, Pi and Codex, omp indexes
sessions by worktree path, so it participates in the worktree-sessions snapshot
that stops a recycled workspace slug from resuming its predecessor's
conversation.

**Status reporting**: omp exposes pre/post *tool* hooks only — there is no
turn-lifecycle event (nothing equivalent to Claude's stop / prompt-submitted /
permission-prompt hooks, or Codex's `notify`) — so there is no deterministic
status wiring, the same position Pi and Hermes are in. Status still updates from
the agent itself calling `wsx status set`, and from the session-JSONL heuristic.
Claude and Codex remain the only harnesses with automatic harness-level status.

**Environment overrides**: configure omp via `~/.omp/agent/config.yml`, or set
`WSX_OMP_MODEL` to override the model per-workspace:

```bash
WSX_OMP_MODEL=anthropic/claude-opus-5 wsx workspace create backend --agent omp
```

There is no `WSX_OMP_PROVIDER`: omp documents `--provider` as legacy and accepts
`provider/id` in `--model`, so `WSX_OMP_MODEL` covers both.
