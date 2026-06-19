| Path                                                | Contents                                                                                                 |
| --------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `$XDG_STATE_HOME/wsx/state.db`                      | SQLite database: repos, workspaces, settings                                                             |
| `$XDG_STATE_HOME/wsx/worktrees/<repo>/<workspace>/` | Worktree directories created by `wsx`                                                                    |
| `$XDG_STATE_HOME/wsx/logs/wsx.log`                  | Daily-rotated `tracing` logs                                                                             |
| `$XDG_STATE_HOME/wsx/project-manager/`              | PM Claude Code session cwd; contains `workspaces.json` and PM's own git init. Auto-created on first `p`. |
| `~/.claude/projects/<encoded-cwd>/<session>.jsonl`  | Claude Code's own session files (wsx probes these to detect resumable workspaces)                        |
