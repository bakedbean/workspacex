# MCP server inheritance

Claude Code keys its MCP server config on the directory it starts in, so an agent in a worktree wouldn't see the MCP servers you added in the source repo. wsx copies the source repo's `mcpServers` from `~/.claude.json` into the worktree's entry each time a workspace's agent starts.

- Servers you add to the source repo with `claude mcp add` reach a workspace the next time its agent starts.
- Archiving a workspace removes its entry from `~/.claude.json`.
- MCP configs often hold API tokens. Copying puts the same secret under a second key in the same file.

To turn it off, set `mcp_mirror` to `false` (see [Settings](../configuration/settings.md)). wsx then never reads or writes `~/.claude.json`, and you can still add servers per workspace with `claude mcp add` while attached.

```bash
wsx config set mcp_mirror false
```
