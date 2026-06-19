After your first prompt in a freshly-created workspace, wsx renames the workspace + git branch based on the conversation. Controlled by `WSX_RENAME_MODE`:

| Mode               | Behavior                                                                                                                                                                                                                           |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `claude` (default) | Claude itself runs `git branch -m` as the first action in its response, based on your first message. A background poller propagates the rename to the wsx store. Higher-quality slugs at the cost of ~80 tokens per session start. |
| `local`            | wsx intercepts your first prompt's keystrokes locally and slugifies them. Zero tokens; literal text.                                                                                                                               |
| `off`              | No auto-rename. Workspaces keep their generated `<adjective>-<plant>` name forever.                                                                                                                                                |

The rename only fires on workspaces whose name still matches the generated `<adjective>-<plant>` pattern.
