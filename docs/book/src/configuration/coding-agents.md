By default, wsx spawns Claude Code (`claude`) as the coding agent in every workspace. You can choose a different agent per-workspace or set a global default:

```bash
wsx config set coding_agent hermes           # new workspaces use hermes by default
wsx workspace create backend --agent pi      # override for a single workspace
```

Supported agents:

| Agent              | CLI option       | Source                                                                    | Config                                    |
| ------------------ | ---------------- | ------------------------------------------------------------------------- | ----------------------------------------- |
| `claude` (default) | `--agent claude` | `claude` binary (override via `WSX_CLAUDE_BIN`)                           | Environment + `~/.claude.json` MCP        |
| `pi`               | `--agent pi`     | `pi` binary (override via `WSX_PI_BIN`)                                   | `~/.pi/`                                  |
| `hermes`           | `--agent hermes` | [nousresearch/hermes-agent](https://github.com/nousresearch/hermes-agent) | `~/.hermes/config.yaml` (provider, model) |
| `codex`            | `--agent codex`  | `codex` binary (override via `WSX_CODEX_BIN`)                             | `~/.codex/config.toml`                    |

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

**AGENTS.md management**: Because Codex has no `--append-system-prompt` flag, wsx injects the workspace doctrine, the auto-rename hint, and any custom instructions into a `wsx-managed` fenced block in the worktree's `AGENTS.md` — the same mechanism used for Hermes:

```markdown
<!-- BEGIN wsx-managed -->

…injected instructions…

<!-- END wsx-managed -->
```

The block is rewritten every time Codex spawns and automatically cleaned up when there's nothing to inject. The file is git-excluded via `.git/info/exclude` if untracked, or will show as modified during a spawn if already tracked. The superpowers-skills doctrine clause is omitted for Codex (those skills install under `~/.claude` and Codex can't load them).

**Claude slash commands**: before each Codex spawn, wsx mirrors Markdown files from `~/.claude/commands/` into a local Codex plugin at `~/plugins/wsx-claude-commands/commands/` and registers that plugin in the implicit personal marketplace at `~/.agents/plugins/marketplace.json`. The marketplace entry is marked `INSTALLED_BY_DEFAULT`, so commands such as `/pull-request` and `/commit-changes` are available in Codex without maintaining a second command set. Edits to the Claude command files are picked up on the next Codex spawn.

**Spawn**: fresh workspaces launch bare `codex`. Non-yolo sessions use Codex's built-in interactive approvals + workspace-write sandbox; `--yolo` workspaces add `--dangerously-bypass-approvals-and-sandbox`.

**Continue**: `codex resume --last`, which Codex filters to the current directory natively — so wsx resumes the worktree's own most-recent session.

**Activity**: the dashboard detail bar tails the worktree's rollout file under `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`. RECENT FILES is not yet populated for Codex (file edits are inferred-via-shell and not tracked).

**Model**: set `WSX_CODEX_MODEL` to pass `-m <model>` to Codex (e.g. `gpt-5.4`). Unset = Codex default.
