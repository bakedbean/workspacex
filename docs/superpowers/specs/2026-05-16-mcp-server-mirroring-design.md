# MCP server mirroring for worktrees — Design

**Issue:** [#26](https://github.com/bakedbean/workspacex/issues/26)

## Goal

When wsx launches a claude session in a worktree, the user's MCP servers configured for the source repo should be available — exactly as they would be if claude were launched in the source repo directly.

## Root cause

Claude Code stores MCP server config in `~/.claude.json` under `projects.<absolute_cwd_path>.mcpServers`. The lookup is keyed on the **exact cwd path** at launch time. wsx spawns claude in worktree paths (e.g. `~/.local/state/wsx/worktrees/<repo>/<name>/`), which have no entry in `projects`, so none of the source repo's MCP servers are visible.

## Approach

Before every workspace session spawn, copy `projects[<repo_path>].mcpServers` from `~/.claude.json` into `projects[<worktree_path>].mcpServers` (creating the worktree entry if missing). On workspace archive, remove the entire `projects[<worktree_path>]` entry.

Atomic file replacement (write-to-temp + rename) handles the race with claude-code's own writes to `~/.claude.json`. Worst case is one spawn missing the mirror; the next spawn (or the user's next attach after some other event) recovers.

## Decisions

- **Mirror on every spawn**, not just on workspace create. Source-repo MCP edits propagate without requiring workspace recreation.
- **Mirror only `mcpServers`**. Per the user's pick: enabled/disabled lists default to "all enabled," which matches the usual intent. If a server is genuinely broken in a worktree, the user can disable it via claude itself and that disable lives under `projects[<worktree_path>]`.
- **No-op when there's nothing to mirror.** If `~/.claude.json` is missing, the source repo has no entry, or it has no `mcpServers`, do nothing. Don't create an empty mirror entry just to be present.
- **Cleanup on archive**: remove the entire `projects[<worktree_path>]` entry. Claude-code re-creates it next time the path is touched. We're not preserving claude-code-managed state across an archive — the worktree is gone, so there's nothing to come back to.
- **PM session is out of scope.** The PM lives at `~/.local/state/wsx/project-manager/` and has no source repo. If the user wants MCP servers in PM, they configure them on PM's path directly. (We can revisit if it comes up.)
- **Global toggle setting `mcp_mirror`**. Defaults to ON. When `false`/`0`/`off`/`no`, both `mirror_mcp_servers` and `remove_worktree_entry` are skipped at the call sites. Users who don't want wsx writing into `~/.claude.json` can opt out via `wsx settings set mcp_mirror false`. The toggle lives next to the existing global settings (`pm_enabled`, `notifications`, etc.) in the store, recognized by `cli::known_setting_key`.
- **README section.** Document the behavior under the Settings section: what gets mirrored, the trigger points, the toggle, and that secrets in `mcpServers` get copied (same trust level, but worth saying out loud).
- **Direct to main.** Functional behavior fix; not subjective.

## Scope

### In
1. New module `src/mcp.rs` with two pure functions:
   - `mirror_mcp_servers(repo_path: &Path, worktree_path: &Path) -> Result<()>`
   - `remove_worktree_entry(worktree_path: &Path) -> Result<()>`
2. Call `mirror_mcp_servers` immediately before any workspace session spawn, **gated on** the `mcp_mirror` setting being on (default).
3. Call `remove_worktree_entry` during workspace archive (after the worktree itself is removed), **gated on** the same setting.
4. Atomic write semantics for `~/.claude.json` — write to a tempfile beside it, fsync, rename over.
5. Add `mcp_mirror` to `cli::known_setting_key` so `wsx settings set mcp_mirror false` is accepted.
6. README: a new "MCP server inheritance" subsection under Settings explaining the behavior, the toggle, and the secrets caveat.
7. Tests for: missing file, missing source entry, missing mcpServers, full mirror, overwriting existing mirror, atomic write integrity, archive removal, toggle gating.

### Out
- PM session inheritance.
- Mirroring `enabledMcpjsonServers`, `disabledMcpServers`, `disabledMcpjsonServers`. (Pure simplicity decision per the user's pick.)
- Locking against concurrent claude-code writes beyond the atomic rename. If we see corruption issues, we revisit with `fcntl` advisory locks.
- A wsx UI for managing MCP servers. Out of scope; users edit ~/.claude.json or use `claude mcp` commands.
- Watching ~/.claude.json for changes and re-mirroring while a session is running. Spawn-time mirror is enough; live edits during a session shouldn't be common.

## Implementation notes

### Module shape

```rust
// src/mcp.rs

use std::path::Path;
use crate::error::Result;

/// Copy `~/.claude.json:projects[<repo_path>].mcpServers` into
/// `~/.claude.json:projects[<worktree_path>].mcpServers`, preserving any
/// other fields already at the worktree entry. No-op when ~/.claude.json
/// doesn't exist, or the source has nothing to mirror.
pub fn mirror_mcp_servers(repo_path: &Path, worktree_path: &Path) -> Result<()> { ... }

/// Remove the entire projects[<worktree_path>] entry from ~/.claude.json.
/// No-op when the file doesn't exist or has no such entry.
pub fn remove_worktree_entry(worktree_path: &Path) -> Result<()> { ... }
```

Internals:
- `read_claude_json() -> Result<Option<serde_json::Value>>` — None when missing/empty
- `write_claude_json_atomic(&Value) -> Result<()>` — temp + rename
- Both `mirror_mcp_servers` and `remove_worktree_entry` use these helpers.

### Atomic write

```rust
let tmp = path.with_extension(format!("json.wsx-tmp.{pid}"));
fs::write(&tmp, serialized)?;
// fsync via OpenOptions + sync_all on the file handle
fs::rename(&tmp, &path)?;
```

If the rename fails (cross-device unlikely since temp is alongside the target), clean up the tempfile and bubble the error.

### Path canonicalization

Both `repo_path` and `worktree_path` are stored as canonical absolute paths in the wsx store (verified by `git::preflight` and worktree creation). We pass them through unchanged. Claude-code uses the absolute path of the cwd as its key, which matches the cwd we pass to `portable_pty::spawn_command` (= the workspace's `worktree_path`).

If `canonicalize` would resolve differently than what claude-code uses... in practice claude-code uses the cwd as-given. We mirror that: use the path as stored, not canonicalized inside `mcp.rs`. The store guarantees canonical paths.

### Spawn integration

Every spawn site for a *workspace* session needs the mirror call right before. The choke points:

- `src/workspace.rs::create` — calls `sessions.spawn` after worktree creation.
- `src/app.rs` — re-attach path (when user hits Enter on an existing workspace whose session has exited).

Approach: rather than scattering `mirror_mcp_servers(...)` at each call site, add a thin wrapper helper:

```rust
// In src/workspace.rs (or a new helper module):
pub fn ensure_mcp_mirror(repo: &Repo, workspace: &Workspace) {
    if let Err(e) = crate::mcp::mirror_mcp_servers(&repo.path, &workspace.worktree_path) {
        tracing::warn!(error = %e, "failed to mirror MCP servers; continuing without");
    }
}
```

Best-effort: a mirror failure should not block spawn. Log and continue.

### Archive integration

`src/workspace.rs::archive` (or wherever archive happens) currently removes the worktree directory and the store row. After those, call:

```rust
if let Err(e) = crate::mcp::remove_worktree_entry(&worktree_path) {
    tracing::warn!(error = %e, "failed to remove worktree entry from ~/.claude.json");
}
```

Best-effort again.

### Testing

- Unit tests in `src/mcp.rs` using `tempfile` for the home directory and `serde_json` to inspect the result.
- Override `~/.claude.json` location by passing an explicit path to the read/write helpers; the public `mirror_mcp_servers` resolves via `dirs::home_dir()` but the internal `mirror_into(claude_json_path, repo_path, worktree_path)` takes the file path. Tests use the inner.
- Test cases: file missing, projects key missing, source missing, mcpServers missing, mirror happy path, mirror preserves existing worktree fields, mirror overwrites stale mcpServers, atomic write leaves no `.tmp.*` files on success, atomic write leaves source intact on serialization error.

## Risks

- **Race with claude-code's own writes to `~/.claude.json`.** Atomic rename guarantees no partial-write corruption. If claude-code writes between our read and write, our write wins (we overwrite their changes). In practice claude-code's writes are batched and infrequent — telemetry-style fields like `lastSessionId`, `lastDuration`. Worst case the user loses one batch of those telemetry updates. Acceptable.
- **Schema drift in `~/.claude.json`.** The file is internal to claude-code. If its schema changes incompatibly, our reader using `serde_json::Value` still loads it (everything is `Value`), but `projects[path].mcpServers` may move. If that happens, our mirror silently no-ops. We log at debug. The user notices "MCP servers missing in worktree" and we revisit.
- **Path mismatch.** If claude-code canonicalizes the cwd differently than what `portable_pty` passes (e.g., symlinks resolved or not), claude-code looks up under a different key than we wrote. Mitigation: we use the same path string for the cwd we hand to spawn and for the mirror key. If symlinks bite, we canonicalize both consistently. Not expected in practice.
- **Secrets in `~/.claude.json`.** MCP server configs frequently embed tokens (API keys, GitHub PATs). Mirroring them copies those secrets into the worktree entry — same trust level as the source entry, same file, same permissions. No new exposure. Just worth noting.

## Out-of-scope follow-ups

- Mirror PM session's MCP config from a designated "PM source path" (could be the user's home directory entry).
- TUI for MCP server management (add/remove/disable per workspace).
- Detect external edits to `~/.claude.json` during a session and re-mirror.
- Support `.mcp.json` project-level config as an alternative storage location (would require a fall-through resolver).
