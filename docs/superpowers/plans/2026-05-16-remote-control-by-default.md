# Remote Control by default — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pass `--remote-control` to every claude session wsx spawns (workspace + PM) by default. Add a global `remote_control` toggle (default on) plus a companion `remote_control_sandbox` (default off) that also passes `--sandbox`.

**Architecture:** New `src/remote.rs` module owns the setting keys + a `RemoteOpts` struct. `spawn_session`, `SessionManager::spawn`, `SessionManager::spawn_pm`, and `build_claude_command` grow a `RemoteOpts` parameter. Call sites in `app.rs` and `pm.rs` compute `RemoteOpts::from_store(&store)` once per spawn.

**Tech Stack:** Rust, `portable_pty::CommandBuilder`, the existing `Store` settings table.

**Spec:** `docs/superpowers/specs/2026-05-16-remote-control-by-default-design.md`

---

## File Structure

- `src/remote.rs` (new) — `RemoteOpts`, `enabled`, `sandbox_enabled`.
- `src/lib.rs` — `pub mod remote;`.
- `src/cli.rs` — add `"remote_control"` and `"remote_control_sandbox"` to `known_setting_key`.
- `src/pty/session.rs` — extend `build_claude_command`, `spawn_session`, `SessionManager::spawn`, `SessionManager::spawn_pm` with a `RemoteOpts` parameter. Append `--remote-control` and conditionally `--sandbox`.
- `src/app.rs` — compute `RemoteOpts::from_store(&app.store)` and pass through at both workspace spawn sites (dashboard Enter + updates-panel Enter).
- `src/pm.rs::open_pm` — same.
- `README.md` — new "Remote control" subsection under Settings.

No deletions. No keybind changes. No new dependencies.

---

### Task 1: `remote.rs` module — settings + `RemoteOpts`

**Files:**
- Create: `src/remote.rs`
- Modify: `src/lib.rs` (add `pub mod remote;`)

- [ ] **Step 1: Write failing tests**

In `src/remote.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabled_defaults_true_when_unset() {
        let store = crate::store::Store::open_in_memory().unwrap();
        assert!(enabled(&store));
    }

    #[test]
    fn enabled_false_for_off_values() {
        let store = crate::store::Store::open_in_memory().unwrap();
        for v in ["false", "off", "0", "no"] {
            store.set_setting("remote_control", v).unwrap();
            assert!(!enabled(&store), "expected disabled for {v:?}");
        }
    }

    #[test]
    fn enabled_true_for_other_values() {
        let store = crate::store::Store::open_in_memory().unwrap();
        for v in ["true", "yes", "on", "1", "anything"] {
            store.set_setting("remote_control", v).unwrap();
            assert!(enabled(&store), "expected enabled for {v:?}");
        }
    }

    #[test]
    fn sandbox_defaults_false_when_unset() {
        let store = crate::store::Store::open_in_memory().unwrap();
        assert!(!sandbox_enabled(&store));
    }

    #[test]
    fn sandbox_true_for_on_values() {
        let store = crate::store::Store::open_in_memory().unwrap();
        for v in ["true", "on", "1", "yes"] {
            store.set_setting("remote_control_sandbox", v).unwrap();
            assert!(sandbox_enabled(&store), "expected enabled for {v:?}");
        }
    }

    #[test]
    fn from_store_combines_both_settings() {
        let store = crate::store::Store::open_in_memory().unwrap();
        store.set_setting("remote_control", "false").unwrap();
        store.set_setting("remote_control_sandbox", "on").unwrap();
        let opts = RemoteOpts::from_store(&store);
        assert!(!opts.enabled);
        assert!(opts.sandbox);
    }

    #[test]
    fn disabled_constructor_is_off() {
        let opts = RemoteOpts::disabled();
        assert!(!opts.enabled);
        assert!(!opts.sandbox);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```
cargo test --lib remote:: -- --test-threads=1 2>&1 | tail -10
```

Expected: compile errors — module doesn't exist.

- [ ] **Step 3: Implement the module**

```rust
//! Settings + helper struct controlling whether claude is launched with
//! `--remote-control` (claude.ai/code + mobile relay). See
//! `docs/superpowers/specs/2026-05-16-remote-control-by-default-design.md`.

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

    /// Convenience for tests / call sites that explicitly don't want the
    /// flag (e.g. spawning `cat` instead of claude).
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            sandbox: false,
        }
    }
}

/// Defaults ON. Off-values: `false` / `off` / `0` / `no`.
pub fn enabled(store: &crate::store::Store) -> bool {
    !matches!(
        store.get_setting("remote_control").ok().flatten().as_deref(),
        Some("false" | "off" | "0" | "no")
    )
}

/// Defaults OFF. On-values: `true` / `on` / `1` / `yes`.
pub fn sandbox_enabled(store: &crate::store::Store) -> bool {
    matches!(
        store
            .get_setting("remote_control_sandbox")
            .ok()
            .flatten()
            .as_deref(),
        Some("true" | "on" | "1" | "yes")
    )
}
```

Add `pub mod remote;` to `src/lib.rs` (alphabetical position).

- [ ] **Step 4: Run tests to verify they pass**

```
cargo test --lib remote:: -- --test-threads=1 2>&1 | tail -10
```

Expected: 7 passed.

- [ ] **Step 5: Commit**

```bash
git add src/remote.rs src/lib.rs
git commit -m "feat(remote): RemoteOpts + remote_control settings"
```

---

### Task 2: Register settings in `known_setting_key`

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 1: Write failing test**

In `src/cli.rs::tests`:

```rust
#[test]
fn accepts_remote_control_settings() {
    assert!(known_setting_key("remote_control"));
    assert!(known_setting_key("remote_control_sandbox"));
}
```

- [ ] **Step 2: Run to confirm fail**

```
cargo test --lib accepts_remote_control_settings -- --test-threads=1 2>&1 | tail -10
```

- [ ] **Step 3: Add the keys to `known_setting_key`**

```rust
fn known_setting_key(k: &str) -> bool {
    matches!(
        k,
        "branch_prefix"
            | "custom_instructions"
            | "nerd_fonts"
            | "editor_cmd"
            | "terminal_cmd"
            | "diff_cmd"
            | "notifications"
            | "theme"
            | "pm_enabled"
            | "pm_custom_instructions"
            | "mcp_mirror"
            | "remote_control"
            | "remote_control_sandbox"
    )
}
```

- [ ] **Step 4: Run test**

```
cargo test --lib accepts_remote_control_settings -- --test-threads=1 2>&1 | tail -5
```

Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): register remote_control + remote_control_sandbox setting keys"
```

---

### Task 3: Thread `RemoteOpts` through the spawn API

**Files:**
- Modify: `src/pty/session.rs` (`build_claude_command`, `spawn_session`, `SessionManager::spawn`, `SessionManager::spawn_pm`)

- [ ] **Step 1: Write failing tests**

In `src/pty/session.rs::tests` (the existing `mod tests`):

```rust
#[test]
fn build_claude_command_appends_remote_control_when_enabled() {
    let cwd = std::path::PathBuf::from(".");
    let mode = SpawnMode::Fresh {
        rename_ctx: None,
        custom_instructions: None,
        yolo: false,
    };
    let opts = crate::remote::RemoteOpts {
        enabled: true,
        sandbox: false,
    };
    let cmd = build_claude_command(&cwd, &mode, opts);
    let argv: Vec<_> = cmd.get_argv().iter().collect();
    assert!(
        argv.iter().any(|a| a == &std::ffi::OsStr::new("--remote-control")),
        "expected --remote-control flag, argv: {argv:?}"
    );
    assert!(
        !argv.iter().any(|a| a == &std::ffi::OsStr::new("--sandbox")),
        "expected no --sandbox flag"
    );
}

#[test]
fn build_claude_command_appends_sandbox_when_enabled() {
    let cwd = std::path::PathBuf::from(".");
    let mode = SpawnMode::Fresh {
        rename_ctx: None,
        custom_instructions: None,
        yolo: false,
    };
    let opts = crate::remote::RemoteOpts {
        enabled: true,
        sandbox: true,
    };
    let cmd = build_claude_command(&cwd, &mode, opts);
    let argv: Vec<_> = cmd.get_argv().iter().collect();
    assert!(argv.iter().any(|a| a == &std::ffi::OsStr::new("--remote-control")));
    assert!(argv.iter().any(|a| a == &std::ffi::OsStr::new("--sandbox")));
}

#[test]
fn build_claude_command_omits_remote_control_when_disabled() {
    let cwd = std::path::PathBuf::from(".");
    let mode = SpawnMode::Fresh {
        rename_ctx: None,
        custom_instructions: None,
        yolo: false,
    };
    let cmd = build_claude_command(&cwd, &mode, crate::remote::RemoteOpts::disabled());
    let argv: Vec<_> = cmd.get_argv().iter().collect();
    assert!(
        !argv.iter().any(|a| a == &std::ffi::OsStr::new("--remote-control")),
        "expected no --remote-control flag, argv: {argv:?}"
    );
    assert!(!argv.iter().any(|a| a == &std::ffi::OsStr::new("--sandbox")));
}

#[test]
fn build_claude_command_remote_control_applies_to_pm_mode() {
    let cwd = std::path::PathBuf::from(".");
    let mode = SpawnMode::ProjectManager {
        workspaces_json_path: std::path::PathBuf::from("/tmp/ws.json"),
        custom_instructions: None,
        resume: false,
    };
    let opts = crate::remote::RemoteOpts {
        enabled: true,
        sandbox: false,
    };
    let cmd = build_claude_command(&cwd, &mode, opts);
    let argv: Vec<_> = cmd.get_argv().iter().collect();
    assert!(argv.iter().any(|a| a == &std::ffi::OsStr::new("--remote-control")));
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```
cargo test --lib build_claude_command_appends_remote_control build_claude_command_omits_remote_control build_claude_command_remote_control_applies -- --test-threads=1 2>&1 | tail -10
```

Expected: compile errors — `build_claude_command` arity changed (and the signature doesn't exist yet).

- [ ] **Step 3: Extend `build_claude_command`**

Change signature:

```rust
pub fn build_claude_command(
    cwd: &Path,
    mode: &SpawnMode,
    remote: crate::remote::RemoteOpts,
) -> CommandBuilder {
```

Right before the `combined` system-prompt construction (after the `skip_permissions` / `allow_git_branch` block), insert:

```rust
if remote.enabled {
    cmd.arg("--remote-control");
    if remote.sandbox {
        cmd.arg("--sandbox");
    }
}
```

- [ ] **Step 4: Extend `spawn_session`**

```rust
pub fn spawn_session(
    cwd: &Path,
    cols: u16,
    rows: u16,
    mode: SpawnMode,
    remote: crate::remote::RemoteOpts,
) -> Result<Session> {
```

Pass `remote` through to `build_claude_command(cwd, &mode, remote)`.

- [ ] **Step 5: Extend `SessionManager::spawn` and `spawn_pm`**

```rust
pub fn spawn(
    &mut self,
    id: WorkspaceId,
    cwd: &Path,
    cols: u16,
    rows: u16,
    mode: SpawnMode,
    remote: crate::remote::RemoteOpts,
) -> Result<Arc<Session>> { ... spawn_session(cwd, cols, rows, mode, remote) ... }
```

Same for `spawn_pm`. Update internal calls.

- [ ] **Step 6: Update all existing spawn-call sites to pass `RemoteOpts::disabled()` temporarily**

Find every `sessions.spawn(...)`, `mgr.spawn(...)`, `spawn_session(...)` call across the codebase. For each, add `crate::remote::RemoteOpts::disabled()` as the final argument. This keeps the codebase compiling while we update real call sites in Tasks 4 + 5.

```
grep -rn "sessions\.spawn\|mgr\.spawn\|spawn_session\|spawn_pm" src/ --include "*.rs"
```

Identify each site; pass `disabled()` to test sites and pre-Task-4-5 production sites alike.

- [ ] **Step 7: Run all tests**

```
cargo test --lib -- --test-threads=1 2>&1 | tail -5
```

Expected: all pass (including the 4 new build_claude_command tests).

- [ ] **Step 8: Commit**

```bash
git add src/pty/session.rs src/workspace.rs src/app.rs src/pm.rs
git commit -m "feat(pty): thread RemoteOpts through spawn API"
```

(Some of these files may not have actual call sites; only `git add` the ones that did change.)

---

### Task 4: Wire real `RemoteOpts::from_store` into workspace spawn sites

**Files:**
- Modify: `src/app.rs` (the two workspace spawn sites)

- [ ] **Step 1: Find the call sites**

```
grep -n "app.sessions.spawn(" src/app.rs
```

Currently two sites (dashboard Enter + updates-panel Enter). Both go through `build_spawn_info` followed by `app.sessions.spawn(id, &path, 80, 24, mode)`.

- [ ] **Step 2: Replace `RemoteOpts::disabled()` with `RemoteOpts::from_store(&app.store)`**

At each call site:

```rust
let remote = crate::remote::RemoteOpts::from_store(&app.store);
let _ = app.sessions.spawn(id, &path, 80, 24, mode, remote)?;
```

- [ ] **Step 3: Add an integration-style test**

In `app::pm_state_tests` (where the existing test helpers live):

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workspace_spawn_includes_remote_control_by_default() {
    // ... follow spawn_attached_workspace pattern from existing tests ...
    // Set WSX_CLAUDE_BIN to a wrapper that captures argv to a tempfile,
    // then attach and assert the captured argv contains "--remote-control".
}
```

If a clean argv-capture mechanism doesn't exist, skip the integration test and rely on the unit tests in Task 3 (which prove `build_claude_command` does the right thing). Add a TODO in the code referencing the smoke step.

- [ ] **Step 4: Run tests**

```
cargo test --lib -- --test-threads=1 2>&1 | tail -5
```

Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): pass RemoteOpts::from_store on workspace spawn"
```

---

### Task 5: Wire `RemoteOpts::from_store` into PM spawn

**Files:**
- Modify: `src/pm.rs::open_pm`

- [ ] **Step 1: Find the call**

```
grep -n "mgr.spawn_pm\|spawn_pm(" src/pm.rs
```

`open_pm` currently calls `mgr.spawn_pm(pm_dir, 80, 24, mode)`.

- [ ] **Step 2: Take a `&Store` parameter (already there) and compute `RemoteOpts::from_store(store)`**

`open_pm` already takes `&Store`. Add:

```rust
let remote = crate::remote::RemoteOpts::from_store(store);
mgr.spawn_pm(pm_dir, 80, 24, mode, remote)?;
```

- [ ] **Step 3: Build to verify**

```
cargo build 2>&1 | tail -5
```

Expected: clean.

- [ ] **Step 4: Run all pm tests** (note: pm tests use `cat` as the binary; `--remote-control` will be in argv but `cat` ignores extra args)

```
cargo test --lib pm:: -- --test-threads=1 2>&1 | tail -10
```

Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add src/pm.rs
git commit -m "feat(pm): pass RemoteOpts::from_store on PM spawn"
```

---

### Task 6: README — "Remote control" subsection

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Find the Settings table**

```
grep -n "mcp_mirror" README.md
```

- [ ] **Step 2: Add settings rows**

In the settings table (after `mcp_mirror`):

```markdown
| `remote_control` | Pass `--remote-control` to claude on every spawn so the session is reachable via [claude.ai/code](https://claude.ai/code) and the Claude mobile app. Default ON; set to `off` / `false` / `0` / `no` to disable. See [Remote control](#remote-control). |
| `remote_control_sandbox` | When `remote_control` is on, also pass `--sandbox` for extra safety on remote-issued commands. Default OFF; set to `on` / `true` / `1` / `yes` to enable. |
```

- [ ] **Step 3: Add a "Remote control" section**

Place after "MCP server inheritance":

```markdown
## Remote control

Claude Code's `--remote-control` flag exposes a running session to
[claude.ai/code](https://claude.ai/code) and the Claude iOS/Android
apps. The local PTY behavior is unchanged — claude prints a session
URL and a QR code at startup that you can scan from your phone or
open in a browser to attach remotely.

wsx passes `--remote-control` to every claude spawn (workspaces and
the PM pane) by default, so any session is reachable from your phone
without extra setup.

**Toggle**: disable with `wsx config set remote_control false`. With
it off, sessions are local-only and nothing is sent to Anthropic's
relay servers.

**Sandbox**: claude offers `--sandbox` as an extra safety wrapper for
remote-issued commands. Disabled by default in wsx; enable with
`wsx config set remote_control_sandbox true`.

**Auth**: claude relays through your claude.ai account. If you're not
signed in (or you're offline), the local session continues to work
and the remote relay just fails silently.

**Privacy**: enabling remote control routes session state through
Anthropic's relay infrastructure. The session URL emitted in the PTY
is also visible to anyone seeing your screen.
```

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs(readme): document remote_control + sandbox settings"
```

---

### Task 7: Final fmt / clippy / test / manual smoke / push

**Files:** none (verification).

- [ ] **Step 1: Format + clippy**

```
cargo fmt && cargo clippy --all-targets -- -D warnings 2>&1 | tail -10
```

Expected: clean.

- [ ] **Step 2: Full test suite**

```
cargo test --lib -- --test-threads=1 2>&1 | tail -3
```

Expected: all pass. Baseline before this plan: 278. Plan adds ~13 tests; expect ~291.

- [ ] **Step 3: Manual smoke**

1. `wsx config set remote_control true` (or unset — true is default).
2. Create / attach a workspace; verify claude's session URL + QR appear in the PTY at startup.
3. From a browser, open the URL; verify you can drive the session remotely.
4. `wsx config set remote_control false`. Re-attach (need to kill existing session first since the flag only takes effect on next spawn). Verify no remote URL appears.
5. `wsx config set remote_control_sandbox on`. Re-attach. Verify a sandbox notice from claude.
6. Try with claude offline / signed out: confirm local session still launches normally.

- [ ] **Step 4: Push to main**

```bash
git push
```
