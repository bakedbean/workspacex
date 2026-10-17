# Coding agents

wsx runs Claude Code in each workspace by default. You can pick a different coding agent for all new workspaces or for one workspace.

```bash
wsx config set coding_agent codex            # default for new workspaces
wsx workspace create backend --agent pi      # this workspace only
```

A workspace created from inside another workspace inherits its agent kind. See [`wsx workspace create`](../cli-reference/workspace.md#wsx-workspace-create).

| Agent              | Project                                                                          | Its own config            |
| ------------------ | -------------------------------------------------------------------------------- | ------------------------- |
| `claude` (default) | Claude Code                                                                      | `~/.claude.json`          |
| `pi`               | [`@earendil-works/pi-coding-agent`](https://github.com/badlogic/pi-mono)         | `~/.pi/`                  |
| `hermes`           | [hermes-agent](https://github.com/nousresearch/hermes-agent)                     | `~/.hermes/config.yaml`   |
| `codex`            | OpenAI Codex                                                                     | `~/.codex/config.toml`    |
| `omp`              | [oh-my-pi](https://github.com/can1357/oh-my-pi) (`@oh-my-pi/pi-coding-agent`)    | `~/.omp/agent/config.yml` |

wsx runs the binary of the same name from `PATH`. Override the path with `WSX_<AGENT>_BIN`, and the model with the agent's model variables. See [Environment variables](../reference/environment-variables.md).

## Capabilities

| Capability                             | claude | pi  | hermes | codex | omp |
| -------------------------------------- | :----: | :-: | :----: | :---: | :-: |
| wsx instructions and auto-rename       | ✓      | ✓   | ✓      | ✓     | ✓   |
| [Related repos](../integrations/related-repos.md) as extra dirs | ✓ | – | – | – | ✓ |
| Automatic status from the harness      | ✓      | –   | –      | ✓     | –   |
| Each agent resumes its own session     | ✓      | ✓   | –      | ✓     | ✓   |
| Yolo mode (`--yolo`)                   | ✓      | –   | ✓      | ✓     | ✓   |
| [MCP inheritance](../integrations/mcp-inheritance.md) | ✓ | – | – | – | – |
| [Remote control](../integrations/remote-access.md)   | ✓ | – | – | – | – |
| `~/.claude/skills` and `~/.claude/commands` | ✓  | skills | – | commands | ✓ |

- "wsx instructions" is the [process doctrine](status-and-recap.md#what-agents-are-told), the auto-rename prompt, and the repo's custom instructions.
- Agents without automatic status still report through `wsx status set`. See [Status and recap](status-and-recap.md).
- Session resume is covered in [Multi-agent workspaces](multi-agent-workspaces.md#sessions-survive-a-restart).

## claude

- Instructions go in `--append-system-prompt`. Related repos go in `--add-dir`.
- wsx adds status hooks through `--settings`.
- Yolo passes `--dangerously-skip-permissions`.
- Remote control is on by default. Turn it off with the `remote_control` setting.

## pi

- Instructions go in `--append-system-prompt`.
- wsx loads a small extension (`pi -e`) that reports each session start, so each pi agent resumes its own session.
- `WSX_PI_MODEL` and `WSX_PI_PROVIDER` pick the model for new sessions.
- pi reads skills from `~/.claude/skills`.

## hermes

- Hermes has no flag for extra instructions, so wsx writes them into a `<!-- BEGIN wsx-managed -->` block at the end of the worktree's `AGENTS.md` on each start.
  - When wsx creates `AGENTS.md`, it copies the repo's `CLAUDE.md` into it.
  - An untracked `AGENTS.md` is added to `.git/info/exclude`.
  - A tracked `AGENTS.md` shows as modified while the block is present.
- On resume, wsx picks the newest Hermes session started since the workspace's first Hermes spawn. If two worktrees start Hermes within a few seconds, it can pick the wrong one.
- Added Hermes agents always start fresh.
- `WSX_HERMES_MODEL` overrides the model in `~/.hermes/config.yaml`. `WSX_HERMES_PROVIDER` is passed as `--provider`, which Hermes's default REPL mode may ignore; set the provider in `config.yaml` to be sure.
- The detail bar shows chat and tool counts, but not edited files.

## codex

- Requires Codex 0.146.0 or newer.
- Instructions go in `-c developer_instructions=…`. Nothing is written to the worktree. Codex reads `CLAUDE.md` when the repo has no `AGENTS.md`.
- Instructions are set only on a fresh start. A resumed session keeps the ones it started with, so edits to custom instructions or related repos don't reach it.
- Status comes from Codex's `notify` after each turn.
- Your `~/.claude/commands/*.md` are mirrored into a Codex plugin on each start, so commands such as `/pull-request` work in Codex.
- wsx turns off Codex's startup update prompt, which would block input. Update Codex yourself, for example with `npm install -g @openai/codex`.
- Yolo passes `--dangerously-bypass-approvals-and-sandbox`.
- The detail bar doesn't show edited files.

## omp

`omp` and `pi` are separate harnesses with different binaries. You can install both.

- Instructions go in `--append-system-prompt`. Related repos go in `--add-dir`.
- Yolo passes `--approval-mode yolo`. Otherwise omp uses your `tools.approvalMode`.
- `WSX_OMP_MODEL` sets the model. It accepts `provider/id`.
- On each start wsx passes `--config <state dir>/omp-config.yml`. This overlay turns on omp's `~/.claude` skills, commands and Claude plugins, which omp 18 turns off by default.
  - It overrides your own settings for those keys in wsx sessions, and replaces your `enabledProviders` list.
  - It enables every skill and command under `~/.claude`, not only wsx's.
  - Your `~/.omp/agent/config.yml` is never edited.
- Pinned chips that name a bundled skill are sent as `/skill:<name>`. See [Agent skills](skills.md#pinning-skills-as-chips).
