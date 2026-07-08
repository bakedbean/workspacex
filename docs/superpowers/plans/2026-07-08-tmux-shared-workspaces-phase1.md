# tmux-Shared Workspaces — Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Workspaces can be created as (or converted to) *shared*: their agent sessions run inside per-instance tmux sessions that survive wsx quitting, are reattached automatically on restart, and are listable via `wsx shared list --json`.

**Architecture:** For a shared workspace, the per-agent `CommandBuilder` built in `src/pty/command.rs` is wrapped in `tmux new-session -A -s <name> -- <agent argv…>` at the single spawn choke point (`spawn_session()`, `src/pty/session.rs:401`). wsx's PTY child becomes the tmux *client*; the agent lives in the tmux *server*. Teardown forks: quit/drop kills only the client (agent survives); explicit kill and archive run `tmux kill-session`. The tmux session name is persisted in the dormant `workspace_agents.session_ref` column.

**Tech Stack:** Rust (edition 2024), rusqlite (bundled SQLite), portable-pty 0.9, serde/serde_json (already deps), tmux invoked as an external binary only.

**Spec:** `docs/superpowers/specs/2026-07-08-tmux-shared-workspaces-design.md`

## Global Constraints

- No new Cargo dependencies. tmux is exec'd as a binary; it is NOT a startup requirement — missing tmux errors only when a shared workspace spawns.
- Binary override seam: `WSX_TMUX_BIN` env var, mirroring `WSX_CLAUDE_BIN` (`src/pty/session.rs:71-79`).
- Migrations re-run on EVERY startup (`SCHEMA_V1` resets `user_version`); every `ALTER TABLE` must go through `add_column_if_missing` (`src/data/schema.rs:135`).
- tmux session names: `wsx-<repo>-<workspace>` (primary instance), `wsx-<repo>-<workspace>-<agent><ordinal>` (added instances); sanitize `[^A-Za-z0-9_-]` to `-`. tmux forbids `.` and `:` in names.
- Always use exact-match target syntax `=name` (leading `=`) for `tmux kill-session -t` / `has-session -t`, or tmux prefix-matches (`wsx-a` would match `wsx-a-codex2`).
- Strip `TMUX` and `TMUX_PANE` from all env passed to tmux commands (wsx itself often runs inside tmux; nested clients refuse to start otherwise).
- Tests that talk to tmux MUST isolate via a private `TMUX_TMPDIR` (never touch the user's server), kill their server in cleanup, and `return` early (skip) if `tmux` is not on PATH. tmux ≥ 3.2 required for `-e` on new-session; CI/dev machines run 3.6.
- CI gates run separately: `cargo fmt --check`, `cargo clippy --all-targets`, `cargo test`. Run all three before every commit. `click_chip_auto_spawns_session_when_missing` is a known flaky PTY-timing test — a failure there alone is not caused by this work.
- Commit messages: conventional commits (`feat(shared): …`, `test: …`). Never commit to `main`; work stays on this branch (`tmux-shared-workspaces`).

## Verified Facts (do not re-derive)

- tmux 3.6 `new-session` accepts a multi-argument command after `--`, passed to exec without a shell — no quoting of the agents' large `--append-system-prompt` args needed. Verified locally.
- `portable_pty::CommandBuilder` (0.9) exposes `get_argv() -> &Vec<OsString>`, `get_cwd() -> Option<&OsString>`, `iter_extra_env_as_str() -> impl Iterator<Item = (&str, &str)>`, `args`, `env`, `cwd`. Verified in `~/.cargo/registry/src/…/portable-pty-0.9.0/src/cmdbuilder.rs`.
- Killing the tmux *client* process (what `Session::drop` / `Session::kill` do via SIGKILL) does NOT kill the tmux server or the agent inside it. Persistence-on-quit therefore needs no code; explicit kill paths need `tmux kill-session`.
- `new-session -A` attaches when the session exists, creates otherwise (`-e` flags ignored on attach — fine).
- `tmux has-session -t =name` exits 0 iff the session exists.

## File Structure

- **Create** `src/pty/tmux.rs` — all tmux knowledge lives here: name derivation, command wrapping, kill/has-session, availability probe. Pure + subprocess helpers, no `Session` state.
- **Create** `src/commands/shared.rs` — `wsx shared list` record building + JSON serialization (mirrors `src/commands/remotes.rs` placement).
- **Modify** `src/data/schema.rs` (migration v16), `src/data/store.rs` (Workspace.shared), `src/pty/session.rs` (spawn wrap + Session.tmux_session + kill_backend), `src/pty/mod.rs` (module decl), `src/app.rs` (spawn wiring, classify override, detached sweep), `src/app/input.rs` (`S`/`T` keys, modal fields), `src/ui/modal/mod.rs` (NewWorkspace.shared, ConfirmShare), `src/ui/dashboard/status.rs` (Detached variant), `src/ui/theme.rs` (style arm), `src/data/workspace.rs` (create threads shared; archive kills sessions), `src/cli.rs` (`--shared`, `share`/`unshare`, `shared list`), `src/commands/mod.rs`.

---

### Task 1: Migration v16 — `workspaces.shared` column

**Files:**
- Modify: `src/data/schema.rs:117-121` (add v16 block)
- Modify: `src/data/store.rs:111-133` (Workspace/NewWorkspace structs), `:164-173` (insert), `:255-307` (three SELECTs), `:323-336` (row mapping), new setter
- Test: co-located `#[cfg(test)]` in `src/data/store.rs`

**Interfaces:**
- Consumes: `add_column_if_missing` (`schema.rs:135`).
- Produces: `Workspace.shared: bool`, `NewWorkspace.shared: bool`, `Store::set_workspace_shared(id: WorkspaceId, shared: bool) -> Result<()>`. Every later task reads `ws.shared`.

- [ ] **Step 1: Write failing tests** (in `store.rs` tests module)

```rust
#[test]
fn workspace_shared_flag_roundtrips_and_flips() {
    let store = Store::open_in_memory().unwrap();
    let repo = store.add_repo(Path::new("/tmp/r"), "r", "wsx").unwrap();
    let id = store
        .insert_workspace(&NewWorkspace {
            repo_id: repo,
            name: "w",
            branch: "wsx/w",
            worktree_path: Path::new("/tmp/r/w"),
            yolo: false,
            agent: AgentKind::Claude,
            shared: true,
        })
        .unwrap();
    assert!(store.workspace_by_id(id).unwrap().unwrap().shared);
    store.set_workspace_shared(id, false).unwrap();
    assert!(!store.workspace_by_id(id).unwrap().unwrap().shared);
}

#[test]
fn migrate_v16_is_idempotent() {
    let store = Store::open_in_memory().unwrap();
    store.migrate_for_test().unwrap(); // second run must not error
    let v: i64 = store
        .conn()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert!(v >= 16);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test workspace_shared_flag -- --nocapture`
Expected: COMPILE ERROR (`NewWorkspace` has no field `shared`) — that is the failing state for struct changes.

- [ ] **Step 3: Implement**

`schema.rs`, after the `if v < 15` block:

```rust
if v < 16 {
    self.add_column_if_missing(
        "workspaces",
        "shared",
        "shared INTEGER NOT NULL DEFAULT 0",
    )?;
    self.conn().execute("PRAGMA user_version = 16", [])?;
}
```

`store.rs`: add `pub shared: bool` to `Workspace` and `NewWorkspace`; extend the INSERT to `…, yolo, agent, shared) VALUES (…, ?7, ?8)` with `w.shared as i64`; append `, shared` to the three `SELECT id, repo_id, …` column lists (`workspaces()`, `workspace_by_id()`, `all_workspaces()`); in `row_to_workspace` add `shared: r.get::<_, i64>(10)? != 0`; add:

```rust
pub fn set_workspace_shared(&self, id: WorkspaceId, shared: bool) -> Result<()> {
    self.conn.execute(
        "UPDATE workspaces SET shared = ?1 WHERE id = ?2",
        rusqlite::params![shared as i64, id.0],
    )?;
    Ok(())
}
```

- [ ] **Step 4: Fix every `NewWorkspace { … }` literal compile error** by adding `shared: false` (test fixtures in `src/data/agents.rs`, `src/app/render.rs`, `src/ui/dashboard/fixture.rs`, and others — let `cargo check` enumerate them).

- [ ] **Step 5: Run tests, fmt, clippy**

Run: `cargo test data:: && cargo fmt --check && cargo clippy --all-targets`
Expected: PASS / clean.

- [ ] **Step 6: Commit** — `feat(shared): add workspaces.shared column (migration v16)`

---

### Task 2: `src/pty/tmux.rs` — names, wrapping, session control

**Files:**
- Create: `src/pty/tmux.rs`
- Modify: `src/pty/mod.rs` (add `pub mod tmux;`)
- Test: co-located `#[cfg(test)]`

**Interfaces:**
- Consumes: `AgentKind::display_name()`, `portable_pty::CommandBuilder`.
- Produces (used by Tasks 3-9):
  - `pub fn session_name(repo: &str, workspace: &str, agent: AgentKind, ordinal: i64, is_primary: bool) -> String`
  - `pub fn wrap_in_tmux(inner: &CommandBuilder, session_name: &str) -> CommandBuilder`
  - `pub fn tmux_bin() -> String` (honors `WSX_TMUX_BIN`)
  - `pub fn is_available() -> bool` (`tmux -V` succeeds)
  - `pub fn has_session(name: &str) -> bool`
  - `pub fn kill_session(name: &str) -> bool` (true = killed)
  - `pub fn spawn_window_size_fixup(name: String)` (background thread, retries `set-option`)

- [ ] **Step 1: Write failing unit tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::AgentKind;

    #[test]
    fn session_name_primary_and_added() {
        assert_eq!(
            session_name("workspacex", "big-fix", AgentKind::Claude, 1, true),
            "wsx-workspacex-big-fix"
        );
        assert_eq!(
            session_name("workspacex", "big-fix", AgentKind::Codex, 2, false),
            "wsx-workspacex-big-fix-codex2"
        );
    }

    #[test]
    fn session_name_sanitizes_tmux_hostile_chars() {
        // tmux rejects '.' and ':' in session names; spaces are just hostile.
        assert_eq!(
            session_name("my.repo", "fix: thing", AgentKind::Claude, 1, true),
            "wsx-my-repo-fix--thing"
        );
    }

    #[test]
    fn wrap_preserves_argv_env_and_strips_tmux_vars() {
        let mut inner = portable_pty::CommandBuilder::new("claude");
        inner.cwd("/tmp/wt");
        inner.arg("--continue");
        inner.env("WSX_WORKSPACE_ID", "7");
        inner.env("TMUX", "/private/socket,123,0"); // must NOT propagate
        let wrapped = wrap_in_tmux(&inner, "wsx-r-w");
        let argv: Vec<String> = wrapped
            .get_argv()
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        // head: tmux new-session -A -s <name> -c <cwd>
        assert_eq!(argv[1..7], ["new-session", "-A", "-s", "wsx-r-w", "-c", "/tmp/wt"]);
        // env forwarded via -e, minus TMUX*
        assert!(argv.iter().any(|a| a == "WSX_WORKSPACE_ID=7"));
        assert!(!argv.iter().any(|a| a.starts_with("TMUX=")));
        // tail: -- <inner argv verbatim>
        let sep = argv.iter().position(|a| a == "--").unwrap();
        assert_eq!(argv[sep + 1..], ["claude", "--continue"]);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test pty::tmux -- --nocapture`
Expected: COMPILE ERROR (module doesn't exist).

- [ ] **Step 3: Implement `src/pty/tmux.rs`**

```rust
//! tmux integration for shared workspaces.
//!
//! All tmux knowledge lives here: session-name derivation, wrapping an agent
//! `CommandBuilder` so the agent runs inside a tmux server (wsx's PTY child
//! becomes the attach client), and subprocess helpers for session lifecycle.
//! `WSX_TMUX_BIN` overrides the binary, mirroring `WSX_CLAUDE_BIN`.

use crate::pty::AgentKind;
use portable_pty::CommandBuilder;

pub fn tmux_bin() -> String {
    std::env::var("WSX_TMUX_BIN").unwrap_or_else(|_| "tmux".to_string())
}

/// `tmux -V` succeeds — used to gate shared spawns with a friendly error.
pub fn is_available() -> bool {
    std::process::Command::new(tmux_bin())
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Replace anything outside [A-Za-z0-9_-] with '-'. tmux rejects '.' and ':'
/// in session names; the rest is defensive.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '-' })
        .collect()
}

/// Deterministic tmux session name for one agent instance. Primary instances
/// get the bare `wsx-<repo>-<workspace>`; added instances append
/// `-<agent><ordinal>` (matching `instance_label`'s vocabulary, '#' replaced
/// by the ordinal suffix since '#' is a tmux format character).
pub fn session_name(
    repo: &str,
    workspace: &str,
    agent: AgentKind,
    ordinal: i64,
    is_primary: bool,
) -> String {
    let base = format!("wsx-{}-{}", sanitize(repo), sanitize(workspace));
    if is_primary {
        base
    } else {
        format!("{base}-{}{ordinal}", sanitize(agent.display_name()))
    }
}

/// Wrap a built agent command so it runs inside `tmux new-session -A`.
/// The returned builder spawns the tmux *client*; the agent process lives in
/// the tmux *server*. The inner command's env is forwarded with repeated `-e`
/// flags (session environment) because a pre-existing tmux server would not
/// otherwise inherit wsx's environment. TMUX/TMUX_PANE are stripped from both
/// the client env and the forwarded set so nesting under the user's own tmux
/// works.
pub fn wrap_in_tmux(inner: &CommandBuilder, session_name: &str) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(tmux_bin());
    if let Some(cwd) = inner.get_cwd() {
        cmd.cwd(cwd);
    }
    for (k, v) in std::env::vars() {
        if k != "TMUX" && k != "TMUX_PANE" {
            cmd.env(k, v);
        }
    }
    cmd.args(["new-session", "-A", "-s", session_name]);
    if let Some(cwd) = inner.get_cwd().and_then(|c| c.to_str()) {
        cmd.args(["-c", cwd]);
    }
    for (k, v) in inner.iter_extra_env_as_str() {
        if k == "TMUX" || k == "TMUX_PANE" {
            continue;
        }
        cmd.arg("-e");
        cmd.arg(format!("{k}={v}"));
    }
    cmd.arg("--");
    for a in inner.get_argv() {
        cmd.arg(a);
    }
    cmd
}

/// Exact-match (`=name`) session existence check.
pub fn has_session(name: &str) -> bool {
    std::process::Command::new(tmux_bin())
        .args(["has-session", "-t", &format!("={name}")])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Exact-match kill. Returns true when a session was actually killed.
pub fn kill_session(name: &str) -> bool {
    std::process::Command::new(tmux_bin())
        .args(["kill-session", "-t", &format!("={name}")])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// `window-size latest` stops simultaneously attached clients (desk + laptop)
/// from letterboxing each other to the smallest screen. Must run after the
/// session exists; the client spawn is asynchronous, so retry briefly in a
/// detached thread. Best-effort — a failure only degrades multi-client UX.
pub fn spawn_window_size_fixup(name: String) {
    std::thread::spawn(move || {
        for _ in 0..20 {
            let ok = std::process::Command::new(tmux_bin())
                .args(["set-option", "-t", &format!("={name}"), "window-size", "latest"])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if ok {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
}
```

Add `pub mod tmux;` to `src/pty/mod.rs`.

- [ ] **Step 4: Run tests, fmt, clippy** — `cargo test pty::tmux && cargo fmt --check && cargo clippy --all-targets` → PASS.

- [ ] **Step 5: Commit** — `feat(shared): add pty::tmux session naming and command wrapping`

---

### Task 3: Spawn integration — tmux-backed `Session`

**Files:**
- Modify: `src/pty/session.rs` — `Session` struct (`:93-111`), `kill()` (`:150`), `spawn_session()` (`:401`), `SessionManager::spawn` (`:537`), `remove` (`:569`)
- Test: co-located; new integration test in `src/pty/session.rs` tests module

**Interfaces:**
- Consumes: `tmux::wrap_in_tmux`, `tmux::kill_session`, `tmux::spawn_window_size_fixup`.
- Produces:
  - `spawn_session(cwd, cols, rows, mode, remote, agent, identity, tmux: Option<&str>) -> Result<Session>` — new trailing param: `Some(name)` wraps in tmux.
  - `Session.tmux_session: Option<String>` (pub)
  - `Session::kill_backend(&self)` — kills the client AND `tmux kill-session`s the backend when `tmux_session` is set. `kill()` and `Drop` stay client-only (that is the persistence semantics).
  - `SessionManager::spawn(…, tmux: Option<&str>)`; `SessionManager::remove` now calls `kill_backend()`.

- [ ] **Step 1: Write failing integration test** (bottom of `session.rs` tests; uses the `EnvGuard`/`cat_path` support already in the module)

```rust
/// Shared-session persistence semantics against a real, private tmux server.
/// Skips when tmux is absent. TMUX_TMPDIR isolation keeps the user's tmux
/// server untouched.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_session_survives_client_kill_and_dies_on_kill_backend() {
    if !crate::pty::tmux::is_available() {
        eprintln!("tmux not installed; skipping");
        return;
    }
    let tmpdir = tempfile::tempdir().unwrap();
    let _tmux_tmp = EnvGuard::set("TMUX_TMPDIR", tmpdir.path().to_str().unwrap());
    let _bin = EnvGuard::set("WSX_CLAUDE_BIN", "/bin/sh");
    // /bin/sh -c 'sleep 30' via WSX_CLAUDE_BIN won't parse; use a wrapper:
    // write a script that ignores args and sleeps.
    let script = tmpdir.path().join("fake-agent.sh");
    std::fs::write(&script, "#!/bin/sh\nsleep 30\n").unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let _bin = EnvGuard::set("WSX_CLAUDE_BIN", script.to_str().unwrap());

    let name = "wsx-test-shared";
    let mode = SpawnMode::Fresh {
        rename_ctx: None,
        custom_instructions: None,
        doctrine: None,
        additional_dirs: vec![],
        yolo: false,
    };
    let session = spawn_session(
        tmpdir.path(), 80, 24, mode,
        crate::agent::remote_control::RemoteOpts::default(),
        AgentKind::Claude, None, Some(name),
    )
    .unwrap();
    // Server-side session appears (client connect is async; poll briefly).
    let mut alive = false;
    for _ in 0..50 {
        if crate::pty::tmux::has_session(name) { alive = true; break; }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(alive, "tmux session was never created");

    // Kill the CLIENT (quit-wsx semantics): backend must survive.
    session.kill();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(crate::pty::tmux::has_session(name), "agent died with the client");

    // kill_backend (explicit-kill semantics): backend must die.
    session.kill_backend();
    let mut gone = false;
    for _ in 0..50 {
        if !crate::pty::tmux::has_session(name) { gone = true; break; }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(gone, "kill_backend left the tmux session running");
}
```

(If `tempfile` is not already a dev-dependency, use `std::env::temp_dir().join(format!("wsx-tmux-test-{}", std::process::id()))` + manual cleanup instead — check `Cargo.toml` first; do NOT add a dependency.)

- [ ] **Step 2: Run to verify failure** — `cargo test shared_session_survives -- --nocapture` → COMPILE ERROR (extra arg / missing methods).

- [ ] **Step 3: Implement**

In `spawn_session`, add trailing param `tmux: Option<&str>` and, directly after the `identity` env block (`:432-435`):

```rust
let child_cmd = match tmux {
    Some(name) => {
        if !crate::pty::tmux::is_available() {
            return Err(Error::AgentBinaryMissing(crate::pty::tmux::tmux_bin()));
        }
        crate::pty::tmux::spawn_window_size_fixup(name.to_string());
        crate::pty::tmux::wrap_in_tmux(&child_cmd, name)
    }
    None => child_cmd,
};
```

(`child_cmd` needs `mut` removed/kept per compiler.) Add to the `Session` literal: `tmux_session: tmux.map(str::to_string)`, and to the struct:

```rust
/// When set, this session's child is a tmux attach client and the agent
/// lives in the tmux server under this session name. `kill()`/`Drop` kill
/// only the client (agent survives — the shared-workspace persistence
/// contract); `kill_backend()` also kills the server session.
pub tmux_session: Option<String>,
```

Add next to `kill()`:

```rust
/// Kill the child (attach client) AND, for tmux-backed sessions, the tmux
/// session holding the agent. Explicit user intent — "kill this agent".
pub fn kill_backend(&self) {
    self.kill();
    if let Some(name) = &self.tmux_session {
        crate::pty::tmux::kill_session(name);
    }
}
```

`SessionManager::spawn` gains `tmux: Option<&str>` (threaded to `spawn_session`); `SessionManager::remove` calls `s.kill_backend()` instead of `s.kill()`. `spawn_pm` passes `None`. Fix all existing `spawn_session(…)`/`sessions.spawn(…)` call sites (app.rs, tests) with `None` for now — Task 4 wires the real value.

- [ ] **Step 4: Run** — `cargo test pty:: && cargo fmt --check && cargo clippy --all-targets` → PASS (tmux test runs for real locally; skips on tmux-less machines).

- [ ] **Step 5: Commit** — `feat(shared): spawn agent sessions inside tmux when requested`

---

### Task 4: App wiring — shared workspaces spawn in tmux, `session_ref` persisted

**Files:**
- Modify: `src/app.rs` — `ensure_workspace_session` (`:1326-1360`), `ensure_instance_session` (`:1374-1413`)
- Test: `src/app/input_tests.rs` (follows existing async app-test patterns there)

**Interfaces:**
- Consumes: `tmux::session_name`, `Store::set_instance_session_ref` (`src/data/agents.rs:153`), `ws.shared`, `AgentInstance{agent, ordinal, is_primary}`.
- Produces: every spawn path passes the derived tmux name for shared workspaces and persists it. Helper `pub(crate) fn tmux_name_for(app: &App, ws_id: WorkspaceId, instance: &AgentInstance) -> Option<String>` in `app.rs` — returns `Some(name)` only when the workspace is shared (single derivation point; Tasks 8-9 reuse the stored `session_ref` instead of re-deriving).

- [ ] **Step 1: Write failing test** (in `input_tests.rs`; use the existing fake-agent + in-memory-store app fixture used by `build_spawn_info_*` tests)

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_workspace_attach_records_tmux_session_ref() {
    if !crate::pty::tmux::is_available() { return; }
    // fixture: one repo "r", one workspace "w" with shared=true (insert via
    // store, mirror the existing attach tests' setup), TMUX_TMPDIR isolated.
    // ... existing fixture code ...
    attach_workspace(&mut app, ws_id).unwrap();
    let inst = app.store.workspace_agents(ws_id).unwrap();
    assert_eq!(inst[0].session_ref.as_deref(), Some("wsx-r-w"));
    let s = app.sessions.get(inst[0].id).unwrap();
    assert_eq!(s.tmux_session.as_deref(), Some("wsx-r-w"));
    // cleanup: kill backend so the private server dies
    s.kill_backend();
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test shared_workspace_attach_records` → FAIL (`session_ref` is None / `tmux_session` is None).

- [ ] **Step 3: Implement.** In `app.rs`:

```rust
/// The tmux session name for an instance of a *shared* workspace, or None
/// for direct workspaces. Also the single place the name is derived; it is
/// persisted to workspace_agents.session_ref at spawn so every later
/// consumer (kill, archive, `wsx shared list`) reads the stored value.
pub(crate) fn tmux_name_for(
    app: &App,
    ws_id: crate::data::store::WorkspaceId,
    instance: &crate::data::agents::AgentInstance,
) -> Option<String> {
    let (rid, ws) = app.workspaces.iter().find(|(_, w)| w.id == ws_id)?;
    if !ws.shared {
        return None;
    }
    let repo = app.repos.iter().find(|r| r.id == *rid)?;
    Some(crate::pty::tmux::session_name(
        &repo.name, &ws.name, instance.agent, instance.ordinal, instance.is_primary,
    ))
}
```

In `ensure_workspace_session` and `ensure_instance_session`, resolve the instance row, compute `let tmux = tmux_name_for(app, ws_id, &instance);`, pass `tmux.as_deref()` to `sessions.spawn(…)`, and on successful spawn:

```rust
if let Some(name) = &tmux {
    if let Err(e) = app.store.set_instance_session_ref(instance.id, name) {
        tracing::warn!(error = %e, "failed to persist tmux session_ref");
    }
}
```

- [ ] **Step 4: Run** — `cargo test shared_workspace_attach_records && cargo test app:: && cargo fmt --check && cargo clippy --all-targets` → PASS.

- [ ] **Step 5: Commit** — `feat(shared): wire shared workspaces through tmux spawn and persist session_ref`

---

### Task 5: Archive kills tmux sessions

**Files:**
- Modify: `src/data/workspace.rs` — `archive` (`:332`) and `archive_with_app` (`:377`, kill in Phase 4's locked block before `delete_workspace`)
- Test: co-located in `src/data/workspace.rs`

**Interfaces:**
- Consumes: `Store::workspace_agents`, `tmux::kill_session`, `ws.shared`.
- Produces: `pub(crate) fn kill_shared_tmux_sessions(store: &Store, ws: &Workspace)` in `workspace.rs`, called by both archive paths.

- [ ] **Step 1: Write failing test.** Fake tmux via `WSX_TMUX_BIN` pointing at a recorder script (no real tmux needed):

```rust
#[tokio::test]
async fn archive_kills_tmux_sessions_of_shared_workspace() {
    let dir = /* tempdir per existing patterns in this file's tests */;
    let log = dir.join("tmux-calls.log");
    let fake = dir.join("fake-tmux.sh");
    std::fs::write(&fake, format!("#!/bin/sh\necho \"$@\" >> {}\n", log.display())).unwrap();
    // chmod 0o755 as in Task 3
    let _g = crate::test_support::EnvGuard::set("WSX_TMUX_BIN", fake.to_str().unwrap());

    let store = Store::open_in_memory().unwrap();
    // seed repo + shared workspace + primary instance with session_ref
    // (insert_workspace with shared: true; add_primary_agent;
    //  set_instance_session_ref(id, "wsx-r-w"))
    let ws = store.workspace_by_id(ws_id).unwrap().unwrap();
    kill_shared_tmux_sessions(&store, &ws);
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(calls.contains("kill-session -t =wsx-r-w"));
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test archive_kills_tmux` → COMPILE ERROR (helper missing).

- [ ] **Step 3: Implement**

```rust
/// Kill the tmux sessions backing a shared workspace's agent instances.
/// Direct workspaces are a no-op. Instances without a session_ref never
/// spawned in tmux; nothing to kill. Best-effort: a dead server or already
/// -killed session must not block archiving.
pub(crate) fn kill_shared_tmux_sessions(store: &Store, ws: &Workspace) {
    if !ws.shared {
        return;
    }
    if let Ok(instances) = store.workspace_agents(ws.id) {
        for inst in instances {
            if let Some(name) = &inst.session_ref {
                crate::pty::tmux::kill_session(name);
            }
        }
    }
}
```

Call it: in `archive()` immediately before its `store.delete_workspace(...)`; in `archive_with_app()` inside the Phase 4 locked block before `g.store.delete_workspace(ws.id)?` (as `kill_shared_tmux_sessions(&g.store, &ws)`).

- [ ] **Step 4: Run** — `cargo test workspace:: && cargo fmt --check && cargo clippy --all-targets` → PASS.

- [ ] **Step 5: Commit** — `feat(shared): kill backing tmux sessions on workspace archive`

---

### Task 6: Create surface — CLI `--shared`, modal field, `S` keybinding

**Files:**
- Modify: `src/cli.rs` (`parse_workspace` `:763-815`, `CliAction::WorkspaceCreate` variant `:333-340` region and handler `:1368-1398`, usage strings `:22`, `:769`)
- Modify: `src/data/workspace.rs` (`create` `:43`, `create_with_app` `:196` — add `shared: bool` param, thread into `NewWorkspace`)
- Modify: `src/ui/modal/mod.rs` (`Modal::NewWorkspace` gains `shared: bool`), `src/app/input.rs` (`:528-559` open sites, `:1131-1204` modal handler, new `S` arm), modal render (`src/ui/modal/` — the NewWorkspace draw fn shows a `[shared: tmux]` line when set; Ctrl-s toggles)
- Test: `src/cli.rs` tests (`parses_workspace_create_*` neighborhood `:2100+`), `src/app/input_tests.rs`

**Interfaces:**
- Consumes: Task 1's `NewWorkspace.shared`.
- Produces: `wsx workspace create <repo> --shared`; TUI `S` opens the NewWorkspace modal with `shared: true`; Ctrl-s inside the modal toggles it (plain chars go to the name buffer). `create`/`create_with_app` signatures gain `shared: bool` after `yolo`.

- [ ] **Step 1: Failing CLI parse test** (next to `parses_workspace_create_with_name_and_yolo`):

```rust
#[test]
fn parses_workspace_create_with_shared() {
    let a = parse(["wsx", "workspace", "create", "myrepo", "--shared"]).unwrap();
    match a {
        CliAction::WorkspaceCreate { repo, shared, .. } => {
            assert_eq!(repo, "myrepo");
            assert!(shared);
        }
        other => panic!("wrong action: {other:?}"),
    }
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test parses_workspace_create_with_shared` → COMPILE ERROR.

- [ ] **Step 3: Implement CLI:** add `shared: bool` to the `WorkspaceCreate` variant; parse `"--shared" => shared = true` in the arg loop; update both usage strings to `[--shared]`; pass to `crate::data::workspace::create(…, yolo, shared, agent_kind, …)`.

- [ ] **Step 4: Implement data layer:** `create` and `create_with_app` take `shared: bool` (after `yolo`), thread into `NewWorkspace { …, shared }`. Fix call sites (`cli.rs`, `input.rs` modal Enter arm, tests) — compile errors enumerate them; existing callers pass `false` except the modal (next step).

- [ ] **Step 5: Implement TUI:** `Modal::NewWorkspace` gains `shared: bool` (all four construction sites in `input.rs` — `Enter`-on-repo `:528`, `n/N` `:553`, `Tab` rebuild `:1147`, `Backspace`/`Char` rebuilds `:1188/:1197` — plus the render fixture). New dashboard arm mirroring `n/N`:

```rust
(KeyCode::Char('S'), _) => {
    let repo_id = match app.selected_target() {
        Some(SelectionTarget::Repo(id)) => Some(id),
        Some(SelectionTarget::Workspace(wid)) => app
            .workspaces
            .iter()
            .find(|(_, w)| w.id == wid)
            .map(|(rid, _)| *rid),
        None => app.repos.first().map(|r| r.id),
    };
    if let Some(id) = repo_id {
        app.modal = Some(Modal::NewWorkspace {
            repo_id: id,
            name_buffer: String::new(),
            yolo: false,
            shared: true,
            agent: crate::pty::session::AgentKind::from_store(&app.store),
        });
    }
}
```

In the modal key handler add (before the generic `KeyCode::Char(c)` arm, guarded on modifiers):

```rust
KeyCode::Char('s') if k.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
    app.modal = Some(Modal::NewWorkspace { repo_id, name_buffer, yolo, shared: !shared, agent });
}
```

Modal draw: render a `shared (tmux): on — ^s toggles` line matching the existing yolo indicator's style. Enter arm passes `shared` to `create_with_app`.

- [ ] **Step 6: Failing-then-passing input test:** assert `S` opens the modal with `shared: true`, and Ctrl-s flips it (mirror the existing `n`/`N` modal tests in `input_tests.rs`).

- [ ] **Step 7: Run all gates** — `cargo test && cargo fmt --check && cargo clippy --all-targets` → PASS.

- [ ] **Step 8: Commit** — `feat(shared): create shared workspaces via --shared flag and S keybinding`

---

### Task 7: Dashboard status — `Detached` state for surviving tmux sessions

**Files:**
- Modify: `src/ui/dashboard/status.rs` (new variant), `src/ui/theme.rs` (`status_style` arm `:353` region), `src/app.rs` (`classify_status` `:606-663`, `refresh` `:420`, new field + sweep)
- Test: `src/ui/dashboard/status.rs` and `src/app/input_tests.rs`

**Interfaces:**
- Consumes: `tmux::has_session`, `AgentInstance.session_ref`, `ws.shared`.
- Produces: `Status::Detached` (glyph `◆`, label `"detached"`) slotted between Idle and Complete. Renumber the full `priority()` ladder to keep every value distinct: Idle 0, Detached 1, Complete 2, Thinking 3, Waiting 4, Question 5, Stalled 6 (same relative order as today, one new rung); `App.shared_detached: HashSet<WorkspaceId>` refreshed by `App::refresh_shared_detached()` (called from `refresh()`, throttled to one sweep per 10s via `App.shared_detached_polled_ms: u64`).

- [ ] **Step 1: Failing unit test** (status.rs):

```rust
#[test]
fn detached_sits_between_idle_and_complete() {
    assert!(Status::Detached.priority() > Status::Idle.priority());
    assert!(Status::Complete.priority() > Status::Detached.priority());
    assert_eq!(Status::Detached.glyph(), '◆');
    assert_eq!(Status::Detached.label(), "detached");
    assert!(!Status::Detached.is_live());
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test detached_sits` → COMPILE ERROR.

- [ ] **Step 3: Implement the variant** in all four `match self` blocks of `status.rs`, the `theme.rs` `status_style` (use the `waiting` color — a live-but-unwatched agent deserves attention-neutral warmth, not `idle` gray), and any exhaustive matches the compiler flags (status strip, section ordering).

- [ ] **Step 4: Implement the sweep + override** in `app.rs`:

```rust
/// Workspaces whose shared tmux session is alive on the server while wsx
/// holds no client for it (e.g. right after a wsx restart). Refreshed by
/// `refresh_shared_detached`, throttled — `tmux has-session` is a subprocess.
pub shared_detached: std::collections::HashSet<crate::data::store::WorkspaceId>,
```

```rust
fn refresh_shared_detached(&mut self) {
    let now = crate::time::now_ms_u64();
    if now.saturating_sub(self.shared_detached_polled_ms) < 10_000 {
        return;
    }
    self.shared_detached_polled_ms = now;
    self.shared_detached.clear();
    for (_, ws) in &self.workspaces {
        if !ws.shared {
            continue;
        }
        let has_client = self
            .primary_instance(ws.id)
            .and_then(|i| self.sessions.get(i))
            .is_some_and(|s| matches!(*s.status.read().unwrap(),
                crate::pty::session::SessionStatus::Running { .. }));
        if has_client {
            continue;
        }
        let alive = self
            .store
            .workspace_agents(ws.id)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|i| i.session_ref)
            .any(|name| crate::pty::tmux::has_session(&name));
        if alive {
            self.shared_detached.insert(ws.id);
        }
    }
}
```

Call `self.refresh_shared_detached();` at the top of `refresh()`. In `classify_status`, after computing the classified value:

```rust
let status = crate::ui::dashboard::status::Status::classify(/* …existing args… */);
if status == crate::ui::dashboard::status::Status::Idle
    && self.shared_detached.contains(&ws.id)
{
    return crate::ui::dashboard::status::Status::Detached;
}
status
```

- [ ] **Step 5: App-level test** (input_tests.rs, fake `WSX_TMUX_BIN` script exiting 0 for `has-session`): a shared workspace with a `session_ref` and no live session classifies as `Detached`; a direct workspace stays `Idle`.

- [ ] **Step 6: Run all gates** → PASS. **Commit** — `feat(shared): show detached status for live tmux sessions without a client`

---

### Task 8: Conversion — `T` toggles shared, sessions restart via `--continue`

**Files:**
- Modify: `src/ui/modal/mod.rs` (new `Modal::ConfirmShare { workspace_id, name: String, to_shared: bool }` + draw fn modeled on `ConfirmArchive`), `src/app/input.rs` (dashboard `T` arm + modal `y`/`n` handler), `src/app.rs` (respawn helper), `src/cli.rs` (`wsx workspace share|unshare <repo> <name>`)
- Test: `src/app/input_tests.rs`, `src/cli.rs` parse tests

**Interfaces:**
- Consumes: `Store::set_workspace_shared`, `Session::kill_backend`, `ensure_workspace_session`/`ensure_instance_session`, `build_spawn_info` (already yields `SpawnMode::Continue` when `has_prior_session_for` is true — conversation resume is free).
- Produces: `pub(crate) fn toggle_workspace_shared(app: &mut App, ws_id: WorkspaceId) -> Result<()>` — flips the flag, then for each instance that had a RUNNING session: `sessions.remove(id)` (kill_backend — kills direct child, or tmux session when unsharing) and re-ensures the session, which now spawns with/without tmux per the new flag and resumes via `Continue`.

- [ ] **Step 1: Failing input test:** `T` on a selected direct workspace opens `Modal::ConfirmShare { to_shared: true, .. }`; `y` flips `store.workspace_by_id(ws).shared` to true. (Session-restart assertions live in the tmux-gated e2e of Task 10; here assert the flag and that the old session id is gone from `app.sessions`.)

- [ ] **Step 2: Run to verify failure.**

- [ ] **Step 3: Implement.** Dashboard arm:

```rust
(KeyCode::Char('T'), _) => {
    if let Some(SelectionTarget::Workspace(id)) = app.selected_target()
        && let Some((_, ws)) = app.workspaces.iter().find(|(_, w)| w.id == id)
    {
        app.modal = Some(Modal::ConfirmShare {
            workspace_id: id,
            name: ws.name.clone(),
            to_shared: !ws.shared,
        });
    }
}
```

`ConfirmShare` `y` handler calls `toggle_workspace_shared`; the modal body states exactly what happens: "restart N running session(s) inside/outside tmux (conversation resumes via --continue)". `toggle_workspace_shared`:

```rust
pub(crate) fn toggle_workspace_shared(
    app: &mut App,
    ws_id: crate::data::store::WorkspaceId,
) -> Result<()> {
    let ws = app.workspaces.iter().find(|(_, w)| w.id == ws_id)
        .map(|(_, w)| w.clone())
        .ok_or_else(|| crate::error::Error::UserInput("workspace not found".into()))?;
    app.store.set_workspace_shared(ws_id, !ws.shared)?;
    app.refresh()?; // reload app.workspaces so spawn sees the new flag
    // Restart only instances that were actually running. kill_backend is
    // correct in both directions: direct child → SIGKILL kills the agent;
    // tmux-backed (unshare) → also kills the server session.
    let instances = app.store.workspace_agents(ws_id)?;
    for inst in instances {
        let was_running = app.sessions.get(inst.id).is_some_and(|s| matches!(
            *s.status.read().unwrap(),
            crate::pty::session::SessionStatus::Running { .. }));
        if !was_running {
            continue;
        }
        app.sessions.remove(inst.id); // kill_backend inside
        if inst.is_primary {
            ensure_workspace_session(app, ws_id, false)?;
        } else {
            ensure_instance_session(app, &inst, false)?;
        }
    }
    Ok(())
}
```

(Adapt the two `ensure_*` calls to their real signatures at implementation time — they are in `app.rs:1326/1374`; the `false` is the existing non-interactive flag documented at `app.rs:1370`.) CLI: `parse_workspace` gains `Some("share")`/`Some("unshare")` → `CliAction::WorkspaceShare { repo, name, shared: bool }`; handler = `lookup_workspace` + `set_workspace_shared` + println. CLI does NOT restart sessions (it can't reach the TUI's SessionManager); print `note: running sessions keep their current backend until restarted` when flipping.

- [ ] **Step 4: Run all gates** → PASS. **Commit** — `feat(shared): toggle workspace sharing with T (sessions restart via --continue)`

---

### Task 9: `wsx shared list --json`

**Files:**
- Create: `src/commands/shared.rs`; Modify: `src/commands/mod.rs`, `src/cli.rs` (parse + dispatch + usage)
- Test: co-located in `shared.rs`, parse test in `cli.rs`

**Interfaces:**
- Consumes: `Store` queries (`repo list`, `workspaces`, `workspace_agents`), `tmux::has_session`.
- Produces (the Phase 2 wire contract — additive JSON, one array of records):

```rust
#[derive(serde::Serialize)]
pub struct SharedAgentRecord {
    pub label: String,        // "claude", "codex#2"
    pub agent: String,        // store_value(): "claude" | "pi" | "hermes" | "codex"
    pub tmux_session: Option<String>,
    pub alive: bool,
}

#[derive(serde::Serialize)]
pub struct SharedWorkspaceRecord {
    pub repo: String,
    pub workspace: String,
    pub branch: String,
    pub worktree_path: String,
    pub agents: Vec<SharedAgentRecord>,
}

/// Build records for every shared workspace. `liveness` is injected so tests
/// don't need tmux; production passes `crate::pty::tmux::has_session`.
pub fn shared_list_records(
    store: &crate::data::store::Store,
    liveness: impl Fn(&str) -> bool,
) -> crate::error::Result<Vec<SharedWorkspaceRecord>>
```

- [ ] **Step 1: Failing tests:** seed an in-memory store with one shared workspace (session_ref `"wsx-r-w"`) and one direct workspace; `shared_list_records(&store, |n| n == "wsx-r-w")` returns exactly one record with `alive: true`, correct repo/branch/label; direct workspace absent. Second test: `serde_json::to_string` output contains `"tmux_session":"wsx-r-w"`. CLI test: `parse(["wsx","shared","list","--json"])` → `CliAction::SharedList { json: true }`.

- [ ] **Step 2: Run to verify failure.**

- [ ] **Step 3: Implement.** `shared_list_records` iterates `crate::data::repo::list(store)` → `store.workspaces(r.id)` filtered on `w.shared` → `store.workspace_agents(w.id)` mapping each to `SharedAgentRecord { label: inst.label(), agent: inst.agent.store_value().into(), alive: inst.session_ref.as_deref().map(&liveness).unwrap_or(false), tmux_session: inst.session_ref }`. CLI: new top-level group `shared` with subcommand `list [--json]` (register in the usage table at `cli.rs:22` region and `known` command dispatch); handler prints `serde_json::to_string_pretty` when `--json`, else a `repo\tworkspace\tsession\talive` tab table. Human output marks dead refs `(dead)`.

- [ ] **Step 4: Run all gates** → PASS. **Commit** — `feat(shared): add wsx shared list --json`

---

### Task 10: End-to-end persistence test + docs

**Files:**
- Test: extend `src/pty/session.rs` tests (reattach case) — the survive/kill case landed in Task 3
- Create: `docs/book/src/integrations/shared-workspaces.md`; Modify: `docs/book/src/SUMMARY.md` (entry next to remote-access), `docs/book/src/integrations/remote-access.md` (cross-link)

- [ ] **Step 1: Failing reattach test** (same isolation pattern as Task 3):

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_session_respawn_reattaches_instead_of_duplicating() {
    if !crate::pty::tmux::is_available() { return; }
    // …same TMUX_TMPDIR + fake-agent setup as Task 3's test…
    let s1 = spawn_session(…, Some("wsx-test-reattach")).unwrap();
    // wait for has_session, then kill the client only:
    s1.kill();
    // respawn with the SAME name — `-A` must attach, not create a second session
    let s2 = spawn_session(…, Some("wsx-test-reattach")).unwrap();
    // poll: exactly one session named wsx-test-reattach exists
    // (`tmux ls -F '#{session_name}'` via std::process::Command, count matches)
    // and s2's parser eventually receives bytes (the fake agent echoes a
    // heartbeat: use `#!/bin/sh\nwhile true; do echo beat; sleep 1; done`)
    s2.kill_backend();
}
```

- [ ] **Step 2: Run to verify failure** (it passes only once `-A` semantics work end-to-end — if Task 3 was correct it may pass immediately; that is acceptable, it pins the contract).

- [ ] **Step 3: Write the docs page.** `shared-workspaces.md` covers: what shared means (survives quit, reattach on restart), create via `S`/`--shared`, convert via `T`/`wsx workspace share`, the session-name convention, manual access (`tmux attach -t wsx-<repo>-<workspace>` — works over plain ssh today), `wsx shared list --json`, and the v1 scrollback limitation from the spec. Update SUMMARY.md and cross-link from remote-access.md ("for per-workspace sharing see Shared Workspaces").

- [ ] **Step 4: Full verification** — `cargo test && cargo fmt --check && cargo clippy --all-targets`, then `mdbook build docs/book` if mdbook is installed (skip otherwise). Expected: all green.

- [ ] **Step 5: Commit** — `feat(shared): e2e reattach test and shared-workspaces docs`

---

## Post-plan checklist (before PR)

- [ ] Run the verify skill: drive the real TUI (`cargo run`) — create a shared workspace with `S`, attach, quit wsx, run `tmux ls` (session survives), restart wsx (status shows ◆ detached), re-attach (conversation intact), archive it (`tmux ls` empty).
- [ ] `wsx shared list --json` output eyeballed on the real store.
- [ ] Open PR per the pull-request skill; reference the spec and this plan; note Phase 2 (remote browsing) follows in a separate PR.
