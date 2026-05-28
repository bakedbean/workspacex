# Agent binary missing — graceful handling

## Problem

When the user enters a workspace whose configured agent's binary is not
installed (e.g. a Hermes workspace on a machine without the `hermes`
binary), wsx exits ungracefully. The failure path:

1. `spawn_session` (`src/pty/session.rs:1041`) calls
   `portable-pty`'s `spawn_command`, which returns an error wrapping an
   `io::Error { kind: NotFound }`.
2. `spawn_session` translates that to `Error::Pty(format!("spawn: {e}"))`.
3. The caller — one of the four `app.sessions.spawn(...)?` sites — bubbles
   the error with `?`.
4. `app::run` propagates the `?` out of `handle_event`
   (`src/app.rs:684`), exits the loop, and the TUI dies with a generic
   error.

The same gap exists for `pi` and `claude`. Hermes only exposed it because
it is new and unlikely to be installed.

## Goals

- Never exit the TUI because a workspace's agent binary is missing.
- Tell the user *which* binary was missing.
- Let the user recover from inside the TUI by switching the workspace's
  agent.

## Non-goals

- No guard at `wsx workspace create --agent <kind>` time. CLI creation
  remains permissive so users can provision worktrees on a build machine
  and use them from a dev machine.
- No "open workspace without a running agent" / read-only mode.
- No proactive PATH check at startup or during background refresh. We
  catch the error at the moment the user tries to attach.

## Design

### 1. Error plumbing

Add a new variant to `Error` in `src/error.rs`:

```rust
#[error("agent binary not found: {0}")]
AgentBinaryMissing(String),
```

The `String` payload is the binary name we tried to spawn — the value of
`WSX_<AGENT>_BIN` if set, otherwise the agent's default
(`claude` / `pi` / `hermes`). The caller already knows which `AgentKind`
it asked for, so the error does not need to embed the kind.

In `spawn_session` (`src/pty/session.rs:1041`), inspect the
`portable-pty` error before stringifying it:

```rust
let mut child = pair.slave.spawn_command(child_cmd).map_err(|e| {
    if is_binary_not_found(&e) {
        Error::AgentBinaryMissing(resolved_binary(agent))
    } else {
        Error::Pty(format!("spawn: {e}"))
    }
})?;
```

`is_binary_not_found(err)` matches `err`'s `Display` output against the
three "binary not on PATH" messages portable-pty 0.9.0 produces in
`src/cmdbuilder.rs::CommandBuilder::search_path`: `"because it does not
exist"`, `"doesn't exist on the filesystem"`, and `"No viable candidates
found in PATH"`. We string-match (rather than walking the error chain for
`io::ErrorKind::NotFound`) because portable-pty constructs these errors
with `anyhow::bail!` and plain strings — the underlying `io::Error` is
not preserved in the chain. The fourth portable-pty error path
(`"Unable to resolve the PATH"`, fired when the `PATH` env var is
entirely missing) is intentionally NOT matched: it signals system
misconfiguration, not a missing binary, and should surface as
`Error::Pty`. If portable-pty is bumped past 0.9.0, re-verify these
patterns; the `spawn_session_returns_agent_binary_missing_for_unknown_path`
test guards the cwd-relative branch.

`resolved_binary(agent)` reads `WSX_<AGENT>_BIN` and falls back to the
agent's default name. Both are private helpers in `src/pty/session.rs`.

### 2. Spawn call-site handling

Three of the four `app.sessions.spawn(...)?` sites need to translate
`Error::AgentBinaryMissing` into a modal. The fourth — pane restore at
`src/app.rs:910` — already discards errors with `let _ = …` and stays
that way (a missing binary during multi-pane restore should silently
skip the affected pane, not pop a modal per pane).

Introduce a small return type for `ensure_workspace_session`:

```rust
pub(crate) enum AttachReady {
    Ok,
    AgentMissing, // modal already set; caller should NOT switch view
}
```

`ensure_workspace_session` (`src/app.rs:924`) becomes:

```rust
pub(crate) fn ensure_workspace_session(
    app: &mut App,
    ws_id: WorkspaceId,
) -> Result<AttachReady> {
    if app.sessions.get(ws_id).is_some() {
        return Ok(AttachReady::Ok);
    }
    if let Some((id, path, mode, repo_path, agent)) = build_spawn_info(app, ws_id) {
        maybe_mirror_mcp(app, &repo_path, &path);
        let remote = crate::remote_control::RemoteOpts::from_store(&app.store);
        match app.sessions.spawn(id, &path, 80, 24, mode, remote, agent) {
            Ok(_) => {}
            Err(Error::AgentBinaryMissing(binary)) => {
                app.modal = Some(Modal::AgentMissing { ws_id, agent, binary });
                return Ok(AttachReady::AgentMissing);
            }
            Err(e) => return Err(e),
        }
    }
    Ok(AttachReady::Ok)
}
```

Callers of `ensure_workspace_session` (the dashboard attach in
`attach_workspace`, the Updates-panel Enter at `src/app/input.rs:1089`,
and the split-Enter at `src/app/input.rs:1117`) wrap the return value:

```rust
match ensure_workspace_session(app, ws_id)? {
    AttachReady::Ok => { /* existing flow: restore_attached_state + set view */ }
    AttachReady::AgentMissing => { /* modal is up; do nothing else */ }
}
```

For the two `src/app/input.rs` sites that currently call
`app.sessions.spawn(...)?` directly (because they need the returned
`Arc<Session>` for split layout), refactor to go through
`ensure_workspace_session` for the spawn step. The session lookup after
attach is a separate `app.sessions.get(id)` call.

### 3. Modals

Two new `Modal` variants in `src/ui/modal.rs`:

```rust
AgentMissing {
    ws_id: WorkspaceId,
    agent: AgentKind, // the one that failed
    binary: String,   // what we tried to spawn (e.g. "hermes")
},
AgentPicker {
    ws_id: WorkspaceId,
    selected: usize,  // index into AgentKind::ALL
},
```

Both variants render through the existing `render()` dispatch alongside
`Modal::Error`, using the same `centered(area, 60, 14)` popup. Bodies:

```
agent not installed

Hermes is not installed.

The `hermes` binary was not found on PATH.
Install it, then re-enter the workspace.

s    switch agent for this workspace
Esc  dismiss
```

```
pick an agent

Choose an agent for this workspace:

>  claude
   pi
   hermes  (current)

↑↓ move   Enter confirm   Esc cancel
```

Key handling extends the existing modal dispatch in `src/app/input.rs`:

- `Modal::AgentMissing`:
  - `s` → `app.modal = Some(Modal::AgentPicker { ws_id, selected: index_of(agent) })`
  - `Esc` / `Enter` → `app.modal = None`; stay on dashboard
- `Modal::AgentPicker`:
  - `↑` / `k` → decrement `selected` (saturating)
  - `↓` / `j` → increment `selected` (clamped to `ALL.len() - 1`)
  - `Esc` → `app.modal = None`
  - `Enter` → persist the new agent and retry attach (section 4)

### 4. Picker confirm — persist and retry

New store method in `src/store.rs`, alongside `rename_workspace` /
`set_workspace_branch`:

```rust
pub fn set_workspace_agent(&self, id: WorkspaceId, agent: AgentKind) -> Result<()> {
    self.conn.execute(
        "UPDATE workspaces SET agent = ?1 WHERE id = ?2",
        params![agent.store_value(), id.0],
    )?;
    Ok(())
}
```

In-memory mirror: after the store write, locate the matching entry in
`App::workspaces` and update its `agent` field. This is the same pattern
used after other in-TUI store writes, and keeps the dashboard from
showing the old agent until the next `poll_external_changes` tick.

Picker `Enter` handler in `src/app/input.rs`:

```rust
KeyCode::Enter => {
    let new_agent = AgentKind::ALL[*selected];
    let ws_id = *ws_id;
    app.store.set_workspace_agent(ws_id, new_agent)?;
    if let Some((_, ws)) = app.workspaces.iter_mut().find(|(_, w)| w.id == ws_id) {
        ws.agent = new_agent;
    }
    app.modal = None;
    attach_workspace(app, ws_id)?;
}
```

`attach_workspace` re-runs `ensure_workspace_session`, which re-spawns
under the new agent. If that agent's binary is *also* missing, the
helper sets `Modal::AgentMissing` again with the new binary name, and
the user lands back on the picker entry point — no explicit recursion in
this code, just the existing flow looping naturally.

Edge cases handled implicitly:

- Picking the same agent that's already configured: store UPDATE is a
  no-op; retry fails identically; modal re-appears with the same name.
  Costs nothing; no special case needed.
- Concurrent CLI delete of the workspace between modal open and picker
  confirm: in-memory `find()` returns `None`, store UPDATE matches zero
  rows, `attach_workspace` fails its own workspace lookup. No extra
  guard.

### 5. AgentKind helpers

Consolidate the stringly-typed conversions currently sprinkled across
`cli.rs`, `pty/session.rs`, and `ui/modal.rs` into one block in
`src/pty/session.rs`:

```rust
impl AgentKind {
    pub const ALL: [AgentKind; 3] = [
        AgentKind::Claude,
        AgentKind::Pi,
        AgentKind::Hermes,
    ];

    pub fn display_name(self) -> &'static str { ... }   // "claude" / "pi" / "hermes"
    pub fn default_binary(self) -> &'static str { ... } // same as display_name today
    pub fn store_value(self) -> &'static str { ... }    // what create writes to DB
}
```

This refactor is in scope because the picker needs `AgentKind::ALL` and
the modal/store paths need a single source of truth for the
agent-to-string mapping. It is not unrelated cleanup.

## Testing

Three layers, each at the smallest level that's still meaningful. Each
relies on the existing `WSX_<AGENT>_BIN` env-var seam plus
`EnvGuard` from `src/test_support.rs` — no real `hermes` / `pi` / `claude`
binaries required on CI.

1. **`spawn_session` returns `AgentBinaryMissing` for NotFound.**
   Set `WSX_CLAUDE_BIN` to a path that doesn't exist, call
   `spawn_session(.., AgentKind::Claude)`, assert the returned error is
   `Error::AgentBinaryMissing(_)` with the configured binary name in the
   payload. Lives next to `missing_binary_returns_pty_error` in
   `src/pty/session.rs:1300`.

2. **`ensure_workspace_session` sets the modal on missing-binary.**
   Build an `App` with a Hermes workspace, point `WSX_HERMES_BIN` at a
   nonexistent path, call `ensure_workspace_session`, assert:
   - return value is `AttachReady::AgentMissing`
   - `app.modal` is `Some(Modal::AgentMissing { ws_id, agent: Hermes, binary })`
   - the rendered modal body contains "Hermes is not installed"

3. **Picker confirm persists and retries.** Open
   `Modal::AgentPicker { ws_id, selected: index_of(Claude) }`, with
   `WSX_CLAUDE_BIN` pointing at a real `cat` binary (mirroring
   `spawn_and_echo` at `pty/session.rs:1266`), send `Enter`. Assert:
   - the workspace's `agent` in the store is now `Claude`
   - `app.sessions.get(ws_id)` is `Some(_)`
   - `app.view` is `View::Attached(_)`
   - `app.modal` is `None`

No dedicated snapshot test for the modal body — `Modal::Error` doesn't
have one, and the substring assertion in test (2) is enough.

## Files touched

- `src/error.rs` — add `AgentBinaryMissing` variant.
- `src/pty/session.rs` — classify spawn-time NotFound; add
  `AgentKind::ALL` / `display_name` / `default_binary` / `store_value`
  helpers; consolidate existing stringly conversions.
- `src/store.rs` — add `set_workspace_agent`.
- `src/app.rs` — change `ensure_workspace_session` signature to return
  `AttachReady`; update `attach_workspace` and dashboard-attach callers.
- `src/app/input.rs` — modal dispatch for `AgentMissing` and
  `AgentPicker`; route Updates-panel Enter and split Enter through
  `ensure_workspace_session`; picker-confirm handler.
- `src/ui/modal.rs` — two new `Modal` variants; render bodies.
- `src/pty/session.rs` + `src/app/input_tests.rs` — three tests
  described in §Testing.

## Out of scope (future work, if useful)

- `wsx workspace create` CLI guard (warning or refusal).
- Showing the missing-agent state on the dashboard row itself
  (e.g., reusing the `setup_failed` badge slot).
- A `wsx doctor` style command that lists configured agents and
  whether their binaries resolve on PATH.
