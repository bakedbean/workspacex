# Auto-rename

After your first prompt in a new workspace, wsx renames the workspace and its git branch to match the task. Set the `WSX_RENAME_MODE` environment variable before starting wsx to choose how.

| Mode | Behavior |
|---|---|
| `claude` (default) | The coding agent picks a slug from your first message and renames the branch. Costs a few tokens per new session. |
| `local` | wsx slugifies the text of your first prompt. Costs no tokens. |
| `off` | No auto-rename. |

```bash
WSX_RENAME_MODE=local wsx
```

Auto-rename only applies to a workspace that still has its generated `<adjective>-<plant>` name. To rename a workspace by hand, press `r` on its card (`?` on the dashboard).
