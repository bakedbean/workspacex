# Remote Control by default — Design

**Issue:** [#33](https://github.com/bakedbean/workspacex/issues/33)

## Goal

Every claude session wsx spawns — both workspace sessions and the PM — should be reachable from claude.ai/code and the Claude mobile app by default, with a global opt-out.

## Approach

Claude Code's `--remote-control` (alias `--rc`) flag turns any interactive session into one that *also* accepts remote connections. The local PTY UX is unchanged; the session URL + QR appear at startup inside the PTY, which wsx already renders. wsx's job here is just to pass `--remote-control` on every claude invocation, gated by a global setting.

`build_claude_command` in `src/pty/session.rs:239` is the single chokepoint that constructs the claude argv for workspace and PM sessions. The change is appending `--remote-control` (and optionally `--sandbox`) there.

## Decisions

- **Global setting `remote_control`** — defaults ON. Off-values `false`/`off`/`0`/`no` (same convention as `mcp_mirror`, `pm_enabled`, `notifications`). User can opt out via `wsx config set remote_control false`.
- **Companion setting `remote_control_sandbox`** — defaults OFF. When on, also passes `--sandbox` to claude. Lets safety-conscious users add the extra wrapper without forcing it on everyone.
- **Scope: workspace + PM sessions.** Both get the flag by default. PM benefits because it's the natural "command center" you'd want to ping from your phone ("any workspace need attention?").
- **No per-repo override in v1.** Global toggle covers privacy-conscious users. If per-repo demand surfaces, that's a follow-up.
- **No new UI.** The session URL and QR are emitted by claude itself into the PTY; wsx's existing render shows them. No wsx-side button, modal, or footer hint is needed.
- **Pass via a struct, not env vars.** Add `RemoteOpts { enabled: bool, sandbox: bool }` as a parameter to `spawn_session` / `spawn_pm` / `build_claude_command`. The store is available in App + workspace + pm modules, so passing through is straightforward and keeps the build-command function pure.
- **Direct to main.** This is a behavioral default change (a feature), not subjective UX work. Goes on main.

## Scope

### In
1. New helper functions in a small `src/remote.rs` module:
   - `enabled(&Store) -> bool` — defaults true, honors `remote_control` setting
   - `sandbox_enabled(&Store) -> bool` — defaults false, honors `remote_control_sandbox` setting
   - `RemoteOpts { enabled: bool, sandbox: bool }` plus a constructor `RemoteOpts::from_store(&Store)`
2. Extend `spawn_session`, `SessionManager::spawn`, `SessionManager::spawn_pm`, and `build_claude_command` to take a `RemoteOpts` and pass it through to the claude argv.
3. `--remote-control` and (conditionally) `--sandbox` appended in `build_claude_command`.
4. Register `remote_control` and `remote_control_sandbox` in `cli::known_setting_key`.
5. README subsection "Remote control" under Settings.
6. Tests: settings default + toggle, `build_claude_command` argv assertions for both flags' presence/absence.

### Out
- Per-repo override.
- A `Ctrl-x` keybind to flip remote-control mid-session (claude has `/remote-control` for this).
- Surfacing the session URL inside wsx UI (it's already in the PTY).
- Verifying auth state before spawn. We trust claude-code's fallback behavior — if the user isn't authed, the local session still runs and the remote relay just doesn't connect.
- Watching for `remote_control` setting changes to retroactively flip running sessions. Setting change takes effect on next spawn.

## Implementation notes

### `RemoteOpts`

```rust
// src/remote.rs
#[derive(Debug, Clone, Copy)]
pub struct RemoteOpts {
    pub enabled: bool,
    pub sandbox: bool,
}

impl RemoteOpts {
    pub fn from_store(store: &crate::store::Store) -> Self {
        Self {
            enabled: enabled(store),
            sandbox: sandbox_enabled(store),
        }
    }

    pub fn disabled() -> Self {
        Self { enabled: false, sandbox: false }
    }
}

pub fn enabled(store: &crate::store::Store) -> bool {
    !matches!(
        store.get_setting("remote_control").ok().flatten().as_deref(),
        Some("false" | "off" | "0" | "no")
    )
}

pub fn sandbox_enabled(store: &crate::store::Store) -> bool {
    matches!(
        store.get_setting("remote_control_sandbox").ok().flatten().as_deref(),
        Some("true" | "on" | "1" | "yes")
    )
}
```

### Argv emission in `build_claude_command`

Add near the end (after `--continue` and permission flags, before the system prompt):

```rust
if remote.enabled {
    cmd.arg("--remote-control");
    if remote.sandbox {
        cmd.arg("--sandbox");
    }
}
```

### Plumbing

`spawn_session(cwd, cols, rows, mode)` → `spawn_session(cwd, cols, rows, mode, remote)`.

`SessionManager::spawn(...)` and `SessionManager::spawn_pm(...)` get the same extra parameter.

Call sites (all need to be updated):
- `src/workspace.rs::create` — currently spawns via `sessions.spawn`. Hmm — wait, `create` doesn't spawn a PTY (we learned this during the MCP work). The PTY spawns lazily on attach in `app.rs`. So workspace::create has no spawn call to update.
- `src/app.rs` — the dashboard Enter handler at `app.sessions.spawn(...)` and the updates-panel Enter handler. Both compute `RemoteOpts::from_store(&app.store)` and pass through.
- `src/pm.rs::open_pm` — pass through to `mgr.spawn_pm`.
- Test sites in `pty/session.rs` tests that call `spawn_session` directly — pass `RemoteOpts::disabled()` since they exec `cat`/`sh` not claude, and the flag would just be ignored by those bins but it's cleaner to suppress it.

### README

New subsection "Remote control" under Settings. Cover:
- What it does (claude.ai/code and mobile)
- Default ON; toggle with `wsx config set remote_control false`
- Companion `remote_control_sandbox` setting for `--sandbox`
- Auth: signs in via claude.ai
- Failure mode: if not authed or offline, local session still works
- Privacy note: enabling sends session state to Anthropic's relay infrastructure

## Risks

- **Privacy.** Default-on routes session state through Anthropic. Some users will want to opt out. The setting handles that, and the README is the discoverability path.
- **Claude-code version skew.** `--remote-control` requires a recent claude-code. If the user's installed claude predates the flag, `claude --remote-control` errors out and the session fails to spawn. Mitigation: if a session fails to spawn with the flag, surface the error (current spawn error path), and the user can disable the setting. Optional follow-up: detect older claude versions on startup and warn. v1 doesn't do this.
- **`--sandbox` semantics change.** If Anthropic redefines what `--sandbox` covers, behavior changes silently. Acceptable; it's opt-in.
- **Auth prompt during spawn.** If claude prompts for auth at session start, the user sees that inside the PTY. They handle it as they would manually launching claude. No wsx code change needed.
- **Session URL leaking in shared terminals.** If a user is mob-programming or screen-sharing, the session URL is on screen. Documented in the README.

## Out-of-scope follow-ups

- Per-repo override (`repo_remote_control` field in `Repo`).
- Auto-detect missing/old claude binary and warn.
- A `Ctrl-x r` shortcut to toggle remote-control on a running session.
- A wsx dashboard glyph indicating "session is remote-attachable" (could pull from PTY state if there's a marker).
- Mirror the `--verbose` flag if connection debugging becomes a frequent need.
