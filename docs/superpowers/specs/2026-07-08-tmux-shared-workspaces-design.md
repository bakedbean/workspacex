# tmux-Shared Workspaces — Design

**Date:** 2026-07-08
**Status:** Approved for planning

## Problem

wsx agent sessions are direct child processes of the TUI: quitting wsx kills every
agent. The only way to reach a running session from another machine is to remember
to launch the whole TUI inside tmux and ssh in — and once sessions are running
outside tmux, there is no way to get them into one without losing them. The
existing `remotes` shortcuts only store ssh/tmux commands that *replace* wsx; they
don't make workspaces themselves shareable.

## Goal

Workspaces can be created as (or later converted to) **shared**: their agent
sessions run inside per-session tmux sessions on the owning machine. Shared
sessions survive wsx quitting. From another machine, wsx can list a host's shared
workspaces and attach to them over ssh, rendered like regular workspaces.

**Out of scope:** creating/archiving/renaming/messaging remote workspaces, live
migration of a running non-tmux process into tmux, any networking beyond the
user's existing ssh access, and any daemon.

## Decisions made during brainstorming

- **Topology:** wsx runs locally on machine B and reaches machine A over ssh.
- **Remote scope:** attach-only. Mutation stays on the owning machine.
- **Granularity:** per-workspace choice at create time (not a global mode).
- **Conversion:** a workspace can be marked shared later; live sessions are
  killed and respawned inside tmux with the agent's own conversation-resume
  (`SpawnMode::Continue`). No live PTY migration (reptyr-style tricks rejected
  as fragile).
- **Approach:** tmux-wrapped spawn behind the existing PTY path (wsx renders a
  tmux *client*), plus remote discovery via `ssh <host> wsx shared list --json`.
  Rejected: tmux control mode (`-CC`) — a protocol client is disproportionate
  complexity; auto-exec wsx into tmux — shares the whole TUI, not workspaces.

## Architecture

wsx today funnels every agent spawn through `spawn_session()`
(`src/pty/session.rs`), which opens a PTY and runs a per-agent `CommandBuilder`
from `src/pty/command.rs`. For shared workspaces, the command is wrapped in
`tmux new-session -A`; the agent then lives in the tmux **server** (a daemon
independent of wsx), and the child wsx owns is just the tmux **attach client**.
Everything downstream — vt100 parser, reader/writer threads, resize sync,
attach/detach views — is unchanged because it is agnostic to what is on the
other end of the PTY.

```
direct workspace:  wsx ── PTY ── agent process            (dies with wsx)
shared workspace:  wsx ── PTY ── tmux client ── tmux server ── agent
machine B:         wsx ── PTY ── ssh -t host tmux attach ── (same server)
```

## Data model and configuration

- **Migration v16:** `ALTER TABLE workspaces ADD COLUMN shared INTEGER NOT NULL
  DEFAULT 0`, gated behind a column-existence check (this codebase re-runs all
  migration blocks every startup). `Workspace` gains `shared: bool`.
- **tmux session naming:** `wsx-<repo>-<workspace>` for the primary agent
  instance; `wsx-<repo>-<workspace>-<agent><ordinal>` for additional instances.
  Names are sanitized (tmux forbids `.` and `:`) and written to the existing,
  currently-dormant `workspace_agents.session_ref` column at first spawn.
  `session_ref` is the source of truth for lookup/kill; names are never
  re-derived after creation.
- **Create surface:** CLI `wsx workspace create <repo> --shared` (composable
  with `--yolo`, `--agent`, `--name`); TUI `S` keybinding on the dashboard as a
  shared-create variant alongside `n`/`N` (`s` is already bound to repo
  settings; `S` is unbound).
- **Host list:** new `shared_hosts` setting (newline-separated `name=ssh-dest`,
  e.g. `mini=eben@ebenmini.local`), same storage and `wsx config edit` flow as
  the existing `remotes` setting. Kept separate from `remotes` because those
  values are full shell commands to exec, not ssh destinations.
- **Dependencies:** none added. tmux is invoked as a binary and required only
  when a shared workspace is used; missing tmux surfaces an error modal at
  spawn time (same pattern as `Modal::AgentMissing`).

## Spawn and lifecycle (owning machine)

**Spawn.** In `spawn_session()`, when the workspace is shared, wrap the built
agent command: `tmux new-session -A -s <name> -c <cwd> -- <agent argv…>`, with:

- `TMUX` stripped from the client's env (so this works when wsx itself runs
  inside tmux),
- session option `window-size latest` set (so simultaneously attached clients
  don't letterbox each other to the smallest screen).

`WSX_WORKSPACE_ID` / instance env vars and remote-control flags ride along
unchanged on the wrapped command. `-A` (attach-or-create) makes respawn
idempotent: after a wsx restart, the same spawn call transparently reattaches
to a still-running agent instead of duplicating it.

**Teardown semantics** — the one deliberate behavioral fork:

| Event | Direct workspace (today) | Shared workspace |
|---|---|---|
| Detach in TUI | keeps running | unchanged |
| Quit wsx | agent killed | client killed; **agent survives** |
| Explicit kill in TUI | agent killed | `tmux kill-session` (agent dies — explicit intent) |
| Archive workspace | agent killed | `tmux kill-session` per instance (no leaks) |

Quit needs almost no code: SIGKILLing the tmux client (current `Session::Drop`)
leaves the server running. The code that must be written is the kill path:
`Session::kill()` / archive call `tmux kill-session -t <session_ref>` for shared
instances.

**Status display.** For shared workspaces with no live client (e.g. right after
a wsx restart), the dashboard checks `tmux has-session -t <session_ref>` and
shows "running, detached" instead of nothing. With tmux's default
`remain-on-exit off`, an agent exiting ends its tmux session, so the client's
exit remains an accurate liveness signal while attached.

**Conversion.** Workspace action "make shared": set the flag; for each live
session, confirm, then kill → respawn with `SpawnMode::Continue` so the agent
resumes its conversation inside the new tmux session. Non-running sessions pick
up tmux on next spawn. "Make direct" is the mirror image.

## Remote browsing and attach (machine B)

**Host-side CLI (the only new protocol).** `wsx shared list --json`, run on the
owning machine, prints one record per shared workspace: workspace name, repo,
branch, agent kind(s), tmux session name(s), and per-session liveness from
`tmux has-session`. The contract is additive JSON — readers ignore unknown
fields — so machines on different wsx versions degrade gracefully.

**Browsing.** `H` on the dashboard (unbound today; mnemonic *hosts*, pairing
with `S` for shared-create) opens a "shared hosts" picker listing
`shared_hosts` entries.
Selecting a host runs `ssh <dest> sh -lc 'wsx shared list --json'` on a
background thread (login shell so PATH resolves wsx). Results render with the
same visual language as local workspaces plus a host badge. The list is
ephemeral: fetched on open, refreshable, never written to the local DB — no
sync or cache-invalidation problem exists.

**Attach.** Selecting a remote workspace spawns `ssh -t <dest> tmux attach -t
<session>` through the normal PTY/vt100 plumbing — to wsx it is just a session
whose child happens to be ssh. Detach/quit kills the ssh client; the agent
keeps running on the host. Multiple agent instances appear as separate
attachable entries.

**Failure handling.** ssh unreachable, auth failure, wsx missing on the host,
or a stale session name → error modal with the captured stderr; the entry is
marked dead. No retry loops.

## Edge cases

- **tmux missing:** error modal at shared-spawn time; not a startup dependency.
- **Name safety:** sanitize derived names; `session_ref` stores the sanitized
  value so lookups never re-derive.
- **Name collisions:** distinct workspaces can sanitize to the same base name
  (repo `a` + ws `b-c` vs repo `a-b` + ws `c`). At derivation time, if another
  instance already claims the derived name, wsx appends the workspace id
  (`wsx-a-b-c-42`) so `-A` never attaches to the wrong agent/worktree.
- **Scrollback:** attaching to an existing tmux session repaints only the
  visible screen, so local scrollback starts at attach time. Accepted for v1;
  tmux copy-mode history remains available in-session.
- **Orphans:** archive explicitly kills tmux sessions — the only leak path is
  covered; no background reaper.

## Testing

- **Unit:** tmux argv wrapping; name derivation/sanitization; `shared_hosts`
  parsing; `wsx shared list` JSON shape (serialize + parse canned output);
  migration v16 idempotence.
- **PTY integration** (existing fake-agent pattern via `WSX_CLAUDE_BIN`,
  skipped when tmux is absent): spawn shared → tmux session exists → drop wsx
  session → tmux session survives → respawn → `-A` reattached, not duplicated.
- **Remote seam:** JSON parsing and list rendering tested against canned
  output; real ssh is not exercised in CI.

## Phasing

1. **Phase 1 — shared workspaces on one machine:** migration, spawn wrap,
   teardown fork, status display, conversion, `--shared`/`S` create surface,
   `wsx shared list --json`. Independently useful: sessions survive wsx and
   are reachable by hand via `ssh` + `tmux attach`.
2. **Phase 2 — remote browsing:** `shared_hosts` setting, host picker,
   remote list fetch, ssh attach.

Each phase lands as its own PR with logical commits.
