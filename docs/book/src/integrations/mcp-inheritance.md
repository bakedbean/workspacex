Claude Code stores MCP server config in `~/.claude.json` under
`projects.<absolute_cwd_path>.mcpServers`. The lookup is keyed on the
literal cwd path at launch time. Because wsx launches claude inside a
worktree path (under `~/.local/state/wsx/worktrees/...`), the source
repo's MCP servers aren't visible by default — claude looks up the
worktree path, finds no entry, and runs without those servers.

wsx mirrors the source repo's `mcpServers` into the worktree's project
entry every time a workspace session spawns. New servers added to the
source repo via `claude mcp add ...` show up in workspaces on the next
attach.

On `wsx workspace archive`, wsx removes the worktree's
`projects[<worktree_path>]` entry from `~/.claude.json` to keep it
tidy.

**Secrets**: MCP server configs frequently include API tokens and
other credentials. Mirroring copies them verbatim into the worktree
entry. This is the same file with the same permissions, but it does
mean the same secret is now keyed under two paths.

**Toggle**: this behavior is on by default. Disable it with:

```bash
wsx config set mcp_mirror false
```

With it disabled, wsx never reads or writes `~/.claude.json`. You can
still configure MCP servers per-workspace by running `claude mcp add
...` while attached.
