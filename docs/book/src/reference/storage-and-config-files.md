# Storage and configuration files

wsx keeps its state under `$XDG_STATE_HOME/wsx/` (default `~/.local/state/wsx/`) and its config under `$XDG_CONFIG_HOME/wsx/` (default `~/.config/wsx/`).

## wsx's own files

| Path | Contents |
| --- | --- |
| `~/.config/wsx/theme.toml` | Optional bar theme, reloaded while wsx runs. See [Themes](../configuration/themes.md). |
| `~/.local/state/wsx/state.db` | SQLite database of repos, workspaces, agents and settings. |
| `~/.local/state/wsx/worktrees/<repo>/<workspace>/` | Workspace worktrees. |
| `~/.local/state/wsx/context/<repo>/<workspace>.md` | Context digest from `wsx context write`. See [Editor-hosted agents](../integrations/editor-agent-context.md). |
| `~/.local/state/wsx/logs/wsx.log.<date>` | The dashboard's log, one file per day. |
| `~/.local/state/wsx/logs/setup-<repo>-<workspace>.log` | The last setup script run for a workspace. |
| `~/.local/state/wsx/logs/ws<id>-<time>.log` | Output of a command run in the background. |
| `~/.local/state/wsx/omp-config.yml` | Config overlay passed to oh-my-pi. |
| `~/.local/state/wsx/pi-session-report.ts` | Extension passed to pi so it reports its session. |
| `$XDG_RUNTIME_DIR/wsx/tui-<pid>.sock` | Control socket for each running dashboard. Without `XDG_RUNTIME_DIR`, under `~/.local/state/wsx/run/`. |

## Files wsx reads or writes elsewhere

| Path | Use |
| --- | --- |
| `~/.claude.json` | Claude Code config. wsx copies MCP servers into worktree entries. See [MCP server inheritance](../integrations/mcp-inheritance.md). |
| `~/.claude/projects/<encoded-cwd>/<session>.jsonl` | Claude Code transcripts, read for status, resume and recaps. |
| `~/.claude/skills/`, `~/.codex/skills/`, `~/.hermes/skills/` | Where `wsx setup install-skill` installs skills. See [Agent skills](../agents/skills.md). |
| `~/plugins/wsx-claude-commands/`, `~/.agents/plugins/marketplace.json` | Your `~/.claude/commands` mirrored as a Codex plugin when a Codex agent starts. |
