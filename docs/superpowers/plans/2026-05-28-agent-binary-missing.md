# Agent Binary Missing — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent wsx from exiting when the user tries to attach to a workspace whose agent binary (`claude` / `pi` / `hermes`) is not installed; replace the crash with a modal that explains the problem and lets the user switch agents from inside the TUI.

**Architecture:** Catch `portable-pty`'s `NotFound`-rooted spawn error inside `spawn_session` and translate it to a typed `Error::AgentBinaryMissing(String)`. Three of the four `app.sessions.spawn(...)?` call sites route that variant into a new `Modal::AgentMissing { ws_id, agent, binary }` (the fourth — pane restore — already discards errors). The modal offers Esc to dismiss and `s` to open a `Modal::AgentPicker` that lists the three agents, persists the user's choice via a new `store.set_workspace_agent`, and immediately retries `attach_workspace` — which loops naturally back to the modal if the newly-picked agent is also missing.

**Tech Stack:** Rust, ratatui (TUI), crossterm (key events), portable-pty (PTY spawn), rusqlite (workspace store), thiserror (Error enum), tokio (async runtime for input tests). Test scaffolding: `Store::open_in_memory()` + `App::new(store, path)` + `EnvGuard` for `WSX_<AGENT>_BIN` env-var seam.

---

## File Structure

**Files modified:**

- `src/pty/session.rs` — `AgentKind` helpers (`ALL`, `display_name`, `default_binary`, `store_value`); classify NotFound at spawn time.
- `src/error.rs` — new `AgentBinaryMissing(String)` variant.
- `src/store.rs` — new `set_workspace_agent(id, agent)` method.
- `src/app.rs` — `AttachReady` enum; `ensure_workspace_session` returns `Result<AttachReady>`; `attach_workspace` and `restore_after_pane_close` call sites adapt.
- `src/app/input.rs` — modal dispatch for `AgentMissing` and `AgentPicker`; route Updates-panel Enter and split Enter through `ensure_workspace_session`; picker-confirm handler.
- `src/ui/modal.rs` — two new `Modal` variants and their render bodies.

**Test files modified:**

- `src/pty/session.rs` (existing `tests` module) — one test for spawn-time NotFound classification.
- `src/store.rs` (existing test module at line ~812) — one test for `set_workspace_agent`.
- `src/app/input_tests.rs::pm_state_tests` — tests for `ensure_workspace_session`, modal rendering, key dispatch, and picker confirm.

No new files created.

---

## Task 1: AgentKind helpers and string consolidation

**Files:**
- Modify: `src/pty/session.rs:16-32` (the `AgentKind` enum and its existing `impl`)

**Rationale:** Centralizes the stringly-typed conversions currently scattered across `cli.rs`, `pty/session.rs`, and `ui/modal.rs` so subsequent tasks (the picker, the store update, the binary lookup) share one source of truth. Pure refactor — no behavior change.

- [ ] **Step 1: Write the failing test**

Add at the bottom of the `tests` module in `src/pty/session.rs` (just before the closing `}` of `mod tests`):

```rust
#[test]
fn agent_kind_helpers_match_existing_strings() {
    use super::AgentKind;
    assert_eq!(AgentKind::ALL.len(), 3);
    assert!(AgentKind::ALL.contains(&AgentKind::Claude));
    assert!(AgentKind::ALL.contains(&AgentKind::Pi));
    assert!(AgentKind::ALL.contains(&AgentKind::Hermes));

    assert_eq!(AgentKind::Claude.display_name(), "claude");
    assert_eq!(AgentKind::Pi.display_name(), "pi");
    assert_eq!(AgentKind::Hermes.display_name(), "hermes");

    assert_eq!(AgentKind::Claude.default_binary(), "claude");
    assert_eq!(AgentKind::Pi.default_binary(), "pi");
    assert_eq!(AgentKind::Hermes.default_binary(), "hermes");

    // store_value must round-trip with AgentKind::from_str_or_default
    for k in AgentKind::ALL {
        assert_eq!(AgentKind::from_str_or_default(Some(k.store_value())), k);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib pty::session::tests::agent_kind_helpers_match_existing_strings`
Expected: FAIL — `ALL`, `display_name`, `default_binary`, `store_value`, `from_str_or_default` not found on `AgentKind`.

- [ ] **Step 3: Add the helpers**

Extend the existing `impl AgentKind` block in `src/pty/session.rs` (currently spanning lines 22-32):

```rust
impl AgentKind {
    pub const ALL: [AgentKind; 3] = [AgentKind::Claude, AgentKind::Pi, AgentKind::Hermes];

    pub fn from_str_or_default(s: Option<&str>) -> Self {
        match s {
            Some("pi") => AgentKind::Pi,
            Some("hermes") => AgentKind::Hermes,
            _ => AgentKind::Claude,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Pi => "pi",
            AgentKind::Hermes => "hermes",
        }
    }

    pub fn default_binary(self) -> &'static str {
        // Same as display_name today, but kept as a separate method so callers
        // documenting "binary to spawn" intent don't accidentally rely on the
        // display string changing later.
        self.display_name()
    }

    pub fn store_value(self) -> &'static str {
        // What `wsx workspace create` writes to the `agent` column.
        self.display_name()
    }
}
```

Then **delete** the existing `from_str_or_default` body that was there before (lines 22-32 had a `from_str_or_default` already — replace it with the version above so the test's round-trip assertion uses the same code path).

- [ ] **Step 4: Update existing callers to use the new helpers**

Replace the three sites that hand-roll the string mapping with `from_str_or_default`:

In `src/cli.rs:806-810`, replace:

```rust
let agent_kind = match agent.as_deref() {
    Some("pi") => crate::pty::session::AgentKind::Pi,
    Some("hermes") => crate::pty::session::AgentKind::Hermes,
    _ => crate::pty::session::AgentKind::Claude,
};
```

with:

```rust
let agent_kind = crate::pty::session::AgentKind::from_str_or_default(agent.as_deref());
```

In `src/ui/modal.rs:104-108`, replace:

```rust
let agent_label = match agent {
    crate::pty::session::AgentKind::Claude => "claude",
    crate::pty::session::AgentKind::Pi => "pi",
    crate::pty::session::AgentKind::Hermes => "hermes",
};
```

with:

```rust
let agent_label = agent.display_name();
```

(Search `src/` for `Some("hermes") => AgentKind` to find any other match arms — replace each with `AgentKind::from_str_or_default(...)` where the surrounding code shape allows it. If a site has additional surrounding logic, leave it alone.)

- [ ] **Step 5: Run all tests to verify nothing regressed**

Run: `cargo test --lib pty::session`
Expected: PASS, including the new test.

Run: `cargo build --lib`
Expected: clean build.

- [ ] **Step 6: Commit**

```bash
git add src/pty/session.rs src/cli.rs src/ui/modal.rs
git commit -m "refactor(agent): consolidate AgentKind string helpers

Introduce AgentKind::ALL, display_name(), default_binary(), and
store_value() so the picker, modal, and store paths share one source
of truth. Replace existing inline string matches in cli.rs and
ui/modal.rs with the new helpers."
```

---

## Task 2: AgentBinaryMissing error variant and spawn-time classification

**Files:**
- Modify: `src/error.rs`
- Modify: `src/pty/session.rs:1059-1070` (the `spawn_command` call site inside `spawn_session`)

**Rationale:** Lets `spawn_session` return a typed signal that "the binary wasn't on PATH" without parsing error strings. Other portable-pty failures (permission denied, PTY exhausted) keep their existing `Error::Pty(...)` shape.

- [ ] **Step 1: Write the failing test**

Add at the bottom of the `tests` module in `src/pty/session.rs`:

```rust
#[test]
fn spawn_session_returns_agent_binary_missing_for_unknown_path() {
    let mut env = EnvGuard::new();
    env.set("WSX_CLAUDE_BIN", "/nonexistent/wsx-test-bin-does-not-exist");
    let cwd = PathBuf::from(".");
    let err = spawn_session(
        &cwd,
        80,
        24,
        SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            additional_dirs: vec![],
            yolo: false,
        },
        crate::remote_control::RemoteOpts::disabled(),
        AgentKind::Claude,
    )
    .expect_err("spawn should fail when binary is missing");
    match err {
        crate::Error::AgentBinaryMissing(binary) => {
            assert_eq!(binary, "/nonexistent/wsx-test-bin-does-not-exist");
        }
        other => panic!("expected AgentBinaryMissing, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib pty::session::tests::spawn_session_returns_agent_binary_missing_for_unknown_path`
Expected: FAIL — `Error::AgentBinaryMissing` variant doesn't exist; the spawn currently returns `Error::Pty(...)`.

- [ ] **Step 3: Add the error variant**

Edit `src/error.rs`, inserting the new variant after `Pty` (around line 12):

```rust
#[derive(Debug, Error)]
pub enum Error {
    #[error("git: {0}")]
    Git(String),
    #[error("store: {0}")]
    Store(#[from] rusqlite::Error),
    #[error("pty: {0}")]
    Pty(String),
    #[error("agent binary not found: {0}")]
    AgentBinaryMissing(String),
    #[error("setup: {0}")]
    Setup(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("invalid input: {0}")]
    UserInput(String),
    #[error("cancelled")]
    Cancelled,
}
```

- [ ] **Step 4: Classify the spawn-time error**

Add this private helper near the top of `src/pty/session.rs` (just after the `use` block):

```rust
/// Look for an `io::Error` anywhere in the `anyhow::Error` chain produced by
/// portable-pty's `spawn_command` and return its kind. Used to distinguish
/// "binary not on PATH" from generic PTY failures without parsing strings.
fn root_io_kind(err: &anyhow::Error) -> Option<std::io::ErrorKind> {
    for cause in err.chain() {
        if let Some(io_err) = cause.downcast_ref::<std::io::Error>() {
            return Some(io_err.kind());
        }
    }
    None
}

/// Resolve the binary name we will attempt to spawn for `agent`, honoring
/// the `WSX_<AGENT>_BIN` env-var seam used by tests.
fn resolved_binary(agent: AgentKind) -> String {
    let env_var = match agent {
        AgentKind::Claude => "WSX_CLAUDE_BIN",
        AgentKind::Pi => "WSX_PI_BIN",
        AgentKind::Hermes => "WSX_HERMES_BIN",
    };
    std::env::var(env_var).unwrap_or_else(|_| agent.default_binary().to_string())
}
```

Then change the `spawn_command` call site at `src/pty/session.rs:1067-1070`. Replace:

```rust
let mut child = pair
    .slave
    .spawn_command(child_cmd)
    .map_err(|e| Error::Pty(format!("spawn: {e}")))?;
```

with:

```rust
let mut child = pair.slave.spawn_command(child_cmd).map_err(|e| {
    if root_io_kind(&e) == Some(std::io::ErrorKind::NotFound) {
        Error::AgentBinaryMissing(resolved_binary(agent))
    } else {
        Error::Pty(format!("spawn: {e}"))
    }
})?;
```

- [ ] **Step 5: Run tests**

Run: `cargo test --lib pty::session::tests::spawn_session_returns_agent_binary_missing_for_unknown_path`
Expected: PASS.

Run: `cargo test --lib pty::session::tests::missing_binary_returns_pty_error`
Expected: PASS (the existing test only exercises portable-pty's raw return; our wrapper isn't on the path).

Run: `cargo build --lib`
Expected: clean build.

- [ ] **Step 6: Commit**

```bash
git add src/error.rs src/pty/session.rs
git commit -m "feat(pty): classify NotFound spawn failures as AgentBinaryMissing

When portable-pty's spawn_command fails because the agent binary is
not on PATH, return Error::AgentBinaryMissing(<binary-name>) instead
of a generic Error::Pty. Other PTY failures keep their existing
shape. The error payload is what was actually attempted (honoring
WSX_<AGENT>_BIN), which the caller will surface in the modal."
```

---

## Task 3: Store API — set_workspace_agent

**Files:**
- Modify: `src/store.rs:504` (insert after `set_workspace_branch`)

**Rationale:** Mirrors the existing `set_workspace_branch` shape. Trivial.

- [ ] **Step 1: Write the failing test**

Find the existing store test module in `src/store.rs` (it's the `mod tests` block that already contains `insert_workspace` calls — search for `set_workspace_branch` to find a sibling test as a template if one exists; otherwise add the test below alongside other `set_workspace_*` tests).

Add this test:

```rust
#[test]
fn set_workspace_agent_updates_row() {
    use crate::pty::session::AgentKind;
    let store = Store::open_in_memory().unwrap();
    let repo_id = store
        .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
        .unwrap();
    let id = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "ws",
            branch: "repo/ws",
            worktree_path: std::path::Path::new("/tmp/wsx-test/ws"),
            yolo: false,
            agent: AgentKind::Claude,
        })
        .unwrap();
    store.set_workspace_agent(id, AgentKind::Hermes).unwrap();
    let ws = store
        .list_workspaces()
        .unwrap()
        .into_iter()
        .find(|(_, w)| w.id == id)
        .expect("workspace present")
        .1;
    assert_eq!(ws.agent, AgentKind::Hermes);
}
```

(If `list_workspaces` returns a different shape, mirror whatever pattern an existing test like `set_workspace_branch` uses to read the row back. The point is to assert the `agent` column updated.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib store::tests::set_workspace_agent_updates_row`
Expected: FAIL — `set_workspace_agent` not found.

- [ ] **Step 3: Add the store method**

Insert after `set_workspace_branch` in `src/store.rs:510`:

```rust
pub fn set_workspace_agent(
    &self,
    id: WorkspaceId,
    agent: crate::pty::session::AgentKind,
) -> Result<()> {
    self.conn.execute(
        "UPDATE workspaces SET agent = ?1 WHERE id = ?2",
        rusqlite::params![agent.store_value(), id.0],
    )?;
    Ok(())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib store::tests::set_workspace_agent_updates_row`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/store.rs
git commit -m "feat(store): add set_workspace_agent

UPDATE workspaces SET agent = ? WHERE id = ?, mirroring
set_workspace_branch. Used by the agent-picker modal to persist
the user's choice when their original agent's binary is missing."
```

---

## Task 4: AttachReady and ensure_workspace_session refactor

**Files:**
- Modify: `src/app.rs:924-937` (`ensure_workspace_session`) — change return type
- Modify: `src/app.rs:941-960` (`attach_workspace`) — adapt to new return type
- Modify: `src/app/input.rs:1089-1106` (Updates-panel Enter) — route through `ensure_workspace_session`
- Modify: `src/app/input.rs:1117-1170` (split Enter — `v` / `s`) — route through `ensure_workspace_session`

**Rationale:** Replaces the `?`-propagation that crashes the TUI with an in-band signal callers can pattern-match on. The fourth `spawn` site at `src/app.rs:910` already uses `let _ = ...` and is intentionally left alone (multi-pane restore should silently skip failing panes).

- [ ] **Step 1: Write the failing test**

Add to `src/app/input_tests.rs::pm_state_tests`:

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ensure_workspace_session_sets_modal_when_binary_missing() {
    use crate::pty::session::AgentKind;
    use crate::store::{NewWorkspace, WorkspaceState};
    let mut env = EnvGuard::new();
    env.set("WSX_HERMES_BIN", "/nonexistent/wsx-test-hermes");
    let store = Store::open_in_memory().unwrap();
    let repo_id = store
        .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
        .unwrap();
    let id = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "ws",
            branch: "repo/ws",
            worktree_path: std::path::Path::new("/tmp/wsx-test/ws"),
            yolo: false,
            agent: AgentKind::Hermes,
        })
        .unwrap();
    store
        .set_workspace_state(id, WorkspaceState::Ready)
        .unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let outcome = crate::app::ensure_workspace_session(&mut app, id).unwrap();
    assert!(matches!(outcome, crate::app::AttachReady::AgentMissing));
    match app.modal {
        Some(crate::ui::modal::Modal::AgentMissing { ws_id, agent, ref binary }) => {
            assert_eq!(ws_id, id);
            assert_eq!(agent, AgentKind::Hermes);
            assert_eq!(binary, "/nonexistent/wsx-test-hermes");
        }
        ref other => panic!("expected AgentMissing modal, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib app::input::input_tests::pm_state_tests::ensure_workspace_session_sets_modal_when_binary_missing`
Expected: FAIL — `AttachReady` not defined; `Modal::AgentMissing` not defined.

(If the test module path is different in this codebase, adjust the `cargo test` filter to match where `pm_state_tests` lives.)

- [ ] **Step 3: Add the AttachReady enum and Modal::AgentMissing stub**

Add to `src/app.rs` near the top of the file (after the existing `use` block, near other public types):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachReady {
    Ok,
    AgentMissing,
}
```

Add a placeholder `AgentMissing` variant to `Modal` in `src/ui/modal.rs:26` (full render comes in Task 5, but the variant must exist for this task's test to compile):

```rust
pub enum Modal {
    // ... existing variants ...
    AgentMissing {
        ws_id: crate::store::WorkspaceId,
        agent: crate::pty::session::AgentKind,
        binary: String,
    },
    AgentPicker {
        ws_id: crate::store::WorkspaceId,
        selected: usize,
    },
}
```

In `src/ui/modal.rs::render`, add the two new variants to the early-return guard so the unimplemented bodies don't panic during this intermediate state (Task 5 will replace these stubs with real bodies):

```rust
if matches!(
    modal,
    Modal::UpdatesPanel { .. }
        | Modal::ProcessList { .. }
        | Modal::RepoSettings { .. }
        | Modal::AgentMissing { .. }
        | Modal::AgentPicker { .. }
) {
    return;
}
```

- [ ] **Step 4: Change ensure_workspace_session to return AttachReady**

Replace the body of `ensure_workspace_session` at `src/app.rs:924-937` with:

```rust
pub(crate) fn ensure_workspace_session(
    app: &mut App,
    ws_id: crate::store::WorkspaceId,
) -> Result<AttachReady> {
    if app.sessions.get(ws_id).is_some() {
        return Ok(AttachReady::Ok);
    }
    if let Some((id, path, mode, repo_path, agent)) = build_spawn_info(app, ws_id) {
        maybe_mirror_mcp(app, &repo_path, &path);
        let remote = crate::remote_control::RemoteOpts::from_store(&app.store);
        match app.sessions.spawn(id, &path, 80, 24, mode, remote, agent) {
            Ok(_) => {}
            Err(crate::Error::AgentBinaryMissing(binary)) => {
                app.modal = Some(crate::ui::modal::Modal::AgentMissing {
                    ws_id,
                    agent,
                    binary,
                });
                return Ok(AttachReady::AgentMissing);
            }
            Err(e) => return Err(e),
        }
    }
    Ok(AttachReady::Ok)
}
```

- [ ] **Step 5: Adapt attach_workspace**

`attach_workspace` at `src/app.rs:941-960` currently calls `ensure_workspace_session(app, ws_id)?;` and proceeds unconditionally. Wrap the rest of its body in a match:

```rust
pub(crate) fn attach_workspace(app: &mut App, ws_id: crate::store::WorkspaceId) -> Result<()> {
    app.workspace_needs_attention.remove(&ws_id);
    match ensure_workspace_session(app, ws_id)? {
        AttachReady::Ok => {}
        AttachReady::AgentMissing => return Ok(()), // modal is up; stay on dashboard
    }
    if app.sessions.get(ws_id).is_some() {
        // ... existing body that switches the view ...
    }
    Ok(())
}
```

(Preserve the existing body inside the `if app.sessions.get(ws_id).is_some()` block exactly as it was — don't rewrite the layout-restore logic.)

- [ ] **Step 6: Route the two `src/app/input.rs` direct-spawn sites through ensure_workspace_session**

At `src/app/input.rs:1089-1106` (Updates-panel `KeyCode::Enter`), replace the body that calls `app.sessions.spawn(...)?` with:

```rust
KeyCode::Enter => {
    if let Some(ws_id) = order.get(selected_now).copied() {
        app.workspace_needs_attention.remove(&ws_id);
        match crate::app::ensure_workspace_session(app, ws_id)? {
            crate::app::AttachReady::Ok => {
                if let Some(session) = app.sessions.get(ws_id) {
                    let _ = session; // existing flow uses the session implicitly via app.sessions.get
                    let restored = crate::app::restore_attached_state(app, ws_id);
                    app.leader_pending = false;
                    app.view = View::Attached(restored);
                }
            }
            crate::app::AttachReady::AgentMissing => {
                // modal is up; leave view alone
            }
        }
    }
    app.modal = None;
}
```

(Read the existing body carefully — the dispatch may reference `build_spawn_info`, `maybe_mirror_mcp`, `restore_attached_state`. After the change, those calls are subsumed by `ensure_workspace_session` + the post-attach restore. The post-attach restore stays in this site.)

Note: keep the `app.modal = None` at the bottom so a successful attach still closes the Updates-panel modal. But if `AttachReady::AgentMissing` was returned, `ensure_workspace_session` already set `app.modal` to the AgentMissing modal, which `app.modal = None` would then wipe out. Guard:

```rust
let was_agent_missing =
    matches!(app.modal, Some(crate::ui::modal::Modal::AgentMissing { .. }));
if !was_agent_missing {
    app.modal = None;
}
```

Apply the same shape at `src/app/input.rs:1117-1170` (the split-Enter handler under `KeyCode::Char('v')` / `KeyCode::Char('s')`). The split logic that runs after `ensure_workspace_session` (deciding focus, tree mutation) only runs in the `AttachReady::Ok` branch.

- [ ] **Step 7: Run tests**

Run: `cargo test --lib app::input::input_tests::pm_state_tests::ensure_workspace_session_sets_modal_when_binary_missing`
Expected: PASS.

Run: `cargo test --lib`
Expected: all tests pass (except `attached_view_shows_status_row_for_other_workspace_needing_attention` which is pre-existing-broken on main; ignore it).

Run: `cargo build --lib`
Expected: clean build.

- [ ] **Step 8: Commit**

```bash
git add src/app.rs src/app/input.rs src/ui/modal.rs
git commit -m "feat(app): catch AgentBinaryMissing at attach sites

ensure_workspace_session now returns Result<AttachReady>; callers
pattern-match on Ok / AgentMissing instead of letting the error
propagate to run() and crash the TUI. When the spawn fails because
the agent binary is missing, the helper sets Modal::AgentMissing
and returns AttachReady::AgentMissing; callers skip the view switch.

The pane-restore site in app.rs:910 keeps its let _ = ... shape: a
modal per silently-failed side pane during multi-pane restore would
be noisy."
```

---

## Task 5: Render the AgentMissing and AgentPicker modal bodies

**Files:**
- Modify: `src/ui/modal.rs:84-160` (the `render` function and the match arms inside it)

**Rationale:** Task 4 stubbed the variants into the early-return guard so they don't panic. This task replaces those stubs with real bodies the user can read.

- [ ] **Step 1: Write the failing test**

Add to `src/app/input_tests.rs::pm_state_tests`:

```rust
#[test]
fn agent_missing_modal_renders_binary_name() {
    use crate::pty::session::AgentKind;
    use crate::ui::modal::Modal;
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.modal = Some(Modal::AgentMissing {
        ws_id: crate::store::WorkspaceId(1),
        agent: AgentKind::Hermes,
        binary: "/nonexistent/hermes".to_string(),
    });
    let backend = TestBackend::new(80, 24);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| draw_for_test(f, &mut app)).unwrap();
    let buf = term.backend().buffer();
    let rendered = (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        rendered.contains("Hermes is not installed") || rendered.contains("hermes is not installed"),
        "expected 'Hermes is not installed' line:\n{rendered}"
    );
    assert!(
        rendered.contains("/nonexistent/hermes"),
        "expected binary path in modal body:\n{rendered}"
    );
    assert!(
        rendered.contains("s") && rendered.contains("switch agent"),
        "expected switch-agent hint:\n{rendered}"
    );
}

#[test]
fn agent_picker_modal_renders_three_agents_with_current_marker() {
    use crate::pty::session::AgentKind;
    use crate::ui::modal::Modal;
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    // Selected index 2 = Hermes. Make Hermes the current agent so the
    // picker can render the "(current)" marker.
    use crate::store::{NewWorkspace, WorkspaceState};
    let repo_id = app
        .store
        .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
        .unwrap();
    let id = app
        .store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "ws",
            branch: "repo/ws",
            worktree_path: std::path::Path::new("/tmp/wsx-test/ws"),
            yolo: false,
            agent: AgentKind::Hermes,
        })
        .unwrap();
    app.store.set_workspace_state(id, WorkspaceState::Ready).unwrap();
    app.refresh_workspaces();
    app.modal = Some(Modal::AgentPicker { ws_id: id, selected: 0 });
    let backend = TestBackend::new(80, 24);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| draw_for_test(f, &mut app)).unwrap();
    let buf = term.backend().buffer();
    let rendered = (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("claude"), "expected claude row: {rendered}");
    assert!(rendered.contains("pi"), "expected pi row: {rendered}");
    assert!(rendered.contains("hermes"), "expected hermes row: {rendered}");
    assert!(rendered.contains("current"), "expected current marker: {rendered}");
}
```

If `App` has no `refresh_workspaces()` method, look at how other tests that insert workspaces make the rows visible to render (they typically pass `App::new(store, ...)` *after* inserting, so the constructor's initial load picks them up). Adjust the test to construct `App` after the insert — same shape as `updates_panel_modal_down_advances_selection` in `src/app/input_tests.rs:202`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib pm_state_tests::agent_missing_modal_renders_binary_name pm_state_tests::agent_picker_modal_renders_three_agents_with_current_marker`
Expected: FAIL — the modals currently early-return without rendering anything, so the buffer is empty.

- [ ] **Step 3: Remove the early-return guard for the new variants**

In `src/ui/modal.rs::render`, change the guard back to its original shape (the new variants will be rendered, not skipped):

```rust
if matches!(
    modal,
    Modal::UpdatesPanel { .. } | Modal::ProcessList { .. } | Modal::RepoSettings { .. }
) {
    return;
}
```

- [ ] **Step 4: Add render bodies**

Inside the `match modal { ... }` block in `src/ui/modal.rs::render`, add arms for the two new variants alongside `Modal::Error`:

```rust
Modal::AgentMissing { agent, binary, .. } => (
    "agent not installed",
    format!(
        "{name} is not installed.\n\n\
         The `{binary}` binary was not found on PATH.\n\
         Install it, then re-enter the workspace.\n\n\
         s    switch agent for this workspace\n\
         Esc  dismiss",
        name = capitalize_first(agent.display_name()),
        binary = binary,
    ),
),
Modal::AgentPicker { ws_id, selected } => {
    let current = app_current_agent_for_picker(modal);
    // Build a 3-line list: "> claude", "  pi", "  hermes (current)".
    let body = crate::pty::session::AgentKind::ALL
        .iter()
        .enumerate()
        .map(|(i, k)| {
            let marker = if i == *selected { ">" } else { " " };
            let current_tag = if Some(*k) == current { "  (current)" } else { "" };
            format!("{marker}  {name}{current_tag}", name = k.display_name())
        })
        .collect::<Vec<_>>()
        .join("\n");
    (
        "pick an agent",
        format!(
            "Choose an agent for this workspace:\n\n{body}\n\n\
             ↑↓ move   Enter confirm   Esc cancel"
        ),
    )
}
```

`app_current_agent_for_picker` is a helper that needs the `App` context to look up the workspace's current agent. Since `render()` doesn't take an `App`, embed the current agent directly in `Modal::AgentPicker`:

Refine the `Modal::AgentPicker` variant in `src/ui/modal.rs:26` to:

```rust
AgentPicker {
    ws_id: crate::store::WorkspaceId,
    selected: usize,
    current: crate::pty::session::AgentKind,
},
```

Then drop `app_current_agent_for_picker` and use `current` directly in the picker render body:

```rust
let current_tag = if *k == *current { "  (current)" } else { "" };
```

`capitalize_first` is a tiny private helper at the bottom of `src/ui/modal.rs`:

```rust
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
```

- [ ] **Step 5: Propagate the new `current` field to construction sites**

Since `Modal::AgentPicker` now requires `current`, update the two places that construct it:

1. In `src/app.rs::ensure_workspace_session` (Task 4) — does NOT construct `AgentPicker` directly, so no change there.
2. The `s` key handler for `Modal::AgentMissing` (Task 6) will construct `AgentPicker`. Note in Task 6's prompt: include `current: agent` when transitioning.

Update the test from Step 1 to pass `current`:

```rust
app.modal = Some(Modal::AgentPicker { ws_id: id, selected: 0, current: AgentKind::Hermes });
```

- [ ] **Step 6: Run tests**

Run: `cargo test --lib pm_state_tests::agent_missing_modal_renders_binary_name pm_state_tests::agent_picker_modal_renders_three_agents_with_current_marker`
Expected: PASS.

Run: `cargo build --lib`
Expected: clean build.

- [ ] **Step 7: Commit**

```bash
git add src/ui/modal.rs src/app/input_tests.rs
git commit -m "feat(ui): render AgentMissing and AgentPicker modal bodies

AgentMissing shows the capitalized agent name, the binary path that
wsx tried to spawn, and key hints (s/Esc). AgentPicker shows the
three-agent list with > marker on the selected index and (current)
tag on the workspace's existing agent."
```

---

## Task 6: Modal key dispatch — AgentMissing

**Files:**
- Modify: `src/app/input.rs:1052-1056` (the `Modal::Error` match arm — add new arms after it)

**Rationale:** Handles Esc/Enter (dismiss) and `s` (open picker) on `Modal::AgentMissing`. Picker key handling is in Task 7.

- [ ] **Step 1: Write the failing tests**

Add to `src/app/input_tests.rs::pm_state_tests`:

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_missing_modal_esc_dismisses() {
    use crate::pty::session::AgentKind;
    use crate::ui::modal::Modal;
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.modal = Some(Modal::AgentMissing {
        ws_id: crate::store::WorkspaceId(1),
        agent: AgentKind::Hermes,
        binary: "hermes".to_string(),
    });
    let shared = Arc::new(Mutex::new(
        App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap(),
    ));
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(crossterm::event::KeyCode::Esc, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(app.modal.is_none(), "Esc should dismiss AgentMissing");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_missing_modal_s_opens_picker_with_current_preselected() {
    use crate::pty::session::AgentKind;
    use crate::ui::modal::Modal;
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = crate::store::WorkspaceId(42);
    app.modal = Some(Modal::AgentMissing {
        ws_id,
        agent: AgentKind::Hermes,
        binary: "hermes".to_string(),
    });
    let shared = Arc::new(Mutex::new(
        App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap(),
    ));
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(crossterm::event::KeyCode::Char('s'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    match app.modal {
        Some(Modal::AgentPicker {
            ws_id: picker_ws,
            selected,
            current,
        }) => {
            assert_eq!(picker_ws, ws_id);
            assert_eq!(current, AgentKind::Hermes);
            // Selected pre-highlights the current agent.
            assert_eq!(AgentKind::ALL[selected], AgentKind::Hermes);
        }
        ref other => panic!("expected AgentPicker, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib pm_state_tests::agent_missing_modal_esc_dismisses pm_state_tests::agent_missing_modal_s_opens_picker_with_current_preselected`
Expected: FAIL — both tests panic because `handle_key_modal` has no arm for `Modal::AgentMissing` (the match is non-exhaustive and the test hits `unreachable` or whatever Rust generates).

- [ ] **Step 3: Add the dispatch arm**

In `src/app/input.rs`, inside `handle_key_modal`, after the `Modal::Error { .. }` arm (line 1052-1056), add:

```rust
Modal::AgentMissing { ws_id, agent, .. } => match k.code {
    KeyCode::Esc | KeyCode::Enter => {
        app.modal = None;
    }
    KeyCode::Char('s') => {
        let selected = crate::pty::session::AgentKind::ALL
            .iter()
            .position(|k| *k == agent)
            .unwrap_or(0);
        app.modal = Some(Modal::AgentPicker {
            ws_id,
            selected,
            current: agent,
        });
    }
    _ => {}
},
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib pm_state_tests::agent_missing_modal_esc_dismisses pm_state_tests::agent_missing_modal_s_opens_picker_with_current_preselected`
Expected: PASS.

Run: `cargo build --lib`
Expected: clean build.

- [ ] **Step 5: Commit**

```bash
git add src/app/input.rs src/app/input_tests.rs
git commit -m "feat(input): dispatch keys on Modal::AgentMissing

Esc/Enter dismisses the modal. 's' transitions to Modal::AgentPicker
with the workspace's current (broken) agent pre-highlighted, so the
user can see which agent they're replacing."
```

---

## Task 7: Modal key dispatch — AgentPicker (movement + confirm with persist + retry)

**Files:**
- Modify: `src/app/input.rs` (add a new match arm after the `Modal::AgentMissing` arm from Task 6)

**Rationale:** Closes the loop: user picks an agent, store gets updated, in-memory `workspaces` mirror is updated, `attach_workspace` retries. If the new agent's binary is *also* missing, `ensure_workspace_session` re-sets `Modal::AgentMissing` and the user is back at the entry point.

- [ ] **Step 1: Write the failing tests**

Add to `src/app/input_tests.rs::pm_state_tests`:

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_picker_down_advances_and_clamps() {
    use crate::pty::session::AgentKind;
    use crate::ui::modal::Modal;
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.modal = Some(Modal::AgentPicker {
        ws_id: crate::store::WorkspaceId(1),
        selected: 0,
        current: AgentKind::Claude,
    });
    let shared = Arc::new(Mutex::new(
        App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap(),
    ));

    for expected in [1usize, 2, 2 /* clamps */] {
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(crossterm::event::KeyCode::Down, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        match app.modal {
            Some(Modal::AgentPicker { selected, .. }) => {
                assert_eq!(selected, expected, "Down step");
            }
            ref other => panic!("expected AgentPicker, got {other:?}"),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_picker_enter_persists_and_retries_attach() {
    use crate::pty::session::AgentKind;
    use crate::store::{NewWorkspace, WorkspaceState};
    use crate::ui::modal::Modal;
    // Switch from broken Hermes to Claude, where Claude is `cat` so the
    // retry attach succeeds.
    let mut env = EnvGuard::new();
    env.set("WSX_CLAUDE_BIN", cat_path());
    let store = Store::open_in_memory().unwrap();
    let repo_id = store
        .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
        .unwrap();
    let id = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "ws",
            branch: "repo/ws",
            worktree_path: std::path::Path::new("."),
            yolo: false,
            agent: AgentKind::Hermes,
        })
        .unwrap();
    store
        .set_workspace_state(id, WorkspaceState::Ready)
        .unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let claude_idx = AgentKind::ALL
        .iter()
        .position(|k| *k == AgentKind::Claude)
        .unwrap();
    app.modal = Some(Modal::AgentPicker {
        ws_id: id,
        selected: claude_idx,
        current: AgentKind::Hermes,
    });
    let shared = Arc::new(Mutex::new(
        App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap(),
    ));
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(crossterm::event::KeyCode::Enter, KeyModifiers::NONE),
    )
    .await
    .unwrap();

    // Store now reports Claude.
    let stored = app
        .store
        .list_workspaces()
        .unwrap()
        .into_iter()
        .find(|(_, w)| w.id == id)
        .expect("workspace present")
        .1;
    assert_eq!(stored.agent, AgentKind::Claude);
    // In-memory mirror also updated.
    let mem = app
        .workspaces
        .iter()
        .find(|(_, w)| w.id == id)
        .expect("workspace in memory")
        .1
        .clone();
    assert_eq!(mem.agent, AgentKind::Claude);
    // A session exists.
    assert!(app.sessions.get(id).is_some(), "session should be alive");
    // Modal closed.
    assert!(app.modal.is_none(), "modal should be cleared on success");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib pm_state_tests::agent_picker_down_advances_and_clamps pm_state_tests::agent_picker_enter_persists_and_retries_attach`
Expected: FAIL — no dispatch arm for `Modal::AgentPicker`.

- [ ] **Step 3: Add the dispatch arm**

In `src/app/input.rs::handle_key_modal`, after the `Modal::AgentMissing` arm from Task 6, add:

```rust
Modal::AgentPicker {
    ws_id,
    selected,
    current,
} => {
    use crate::pty::session::AgentKind;
    match k.code {
        KeyCode::Esc => {
            app.modal = None;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let new_sel = selected.saturating_sub(1);
            app.modal = Some(Modal::AgentPicker {
                ws_id,
                selected: new_sel,
                current,
            });
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let new_sel = (selected + 1).min(AgentKind::ALL.len() - 1);
            app.modal = Some(Modal::AgentPicker {
                ws_id,
                selected: new_sel,
                current,
            });
        }
        KeyCode::Enter => {
            let new_agent = AgentKind::ALL[selected];
            app.store.set_workspace_agent(ws_id, new_agent)?;
            if let Some((_, ws)) = app.workspaces.iter_mut().find(|(_, w)| w.id == ws_id) {
                ws.agent = new_agent;
            }
            app.modal = None;
            crate::app::attach_workspace(app, ws_id)?;
        }
        _ => {}
    }
}
```

- [ ] **Step 4: Run the new tests**

Run: `cargo test --lib pm_state_tests::agent_picker_down_advances_and_clamps pm_state_tests::agent_picker_enter_persists_and_retries_attach`
Expected: PASS.

- [ ] **Step 5: Run the full test suite to catch any regressions**

Run: `cargo test --lib`
Expected: all tests pass except the pre-existing `attached_view_shows_status_row_for_other_workspace_needing_attention` (broken on main; not introduced here — verify by `git stash && cargo test --lib <name>` if in doubt).

Run: `cargo build --lib`
Expected: clean build.

Run: `cargo fmt`
Expected: only modifies files this plan touched. If other files are reformatted (pre-existing drift), revert them with `git checkout -- <path>` so this branch stays scoped.

- [ ] **Step 6: Commit**

```bash
git add src/app/input.rs src/app/input_tests.rs
git commit -m "feat(input): AgentPicker confirm persists agent and retries attach

Up/Down (and k/j) move selection; Esc cancels; Enter persists the
new agent via store.set_workspace_agent, mirrors the change to the
in-memory App::workspaces copy, closes the modal, and calls
attach_workspace to retry. If the newly-picked agent's binary is
also missing, ensure_workspace_session re-sets Modal::AgentMissing
and the user lands back at the entry point — no explicit recursion;
the existing flow loops naturally."
```

---

## Task 8: Final verification and PR

**Files:** none — verification only.

- [ ] **Step 1: Manual smoke test (optional but recommended)**

Outside of CI, verify the user-facing flow:

```bash
# In a wsx-managed repo:
WSX_HERMES_BIN=/nonexistent/hermes cargo run -- 2>&1 | head -20
```

Inside the TUI, create a Hermes workspace if one doesn't exist, then press Enter on it. Expect: modal pops up with "Hermes is not installed" and the bad path. Press `s` to open the picker. Press Down to highlight Claude. Press Enter. Expect: the workspace re-attaches with Claude.

Press Ctrl-X to detach. Esc to verify Esc dismisses cleanly when in the AgentMissing modal directly.

- [ ] **Step 2: Run final checks**

Run: `cargo fmt --check`
Expected: no diff on files this plan touched.

Run: `cargo clippy --lib 2>&1 | grep -E "row\\.rs|error\\.rs|store\\.rs|session\\.rs|app\\.rs|input\\.rs|modal\\.rs" | grep -v "^warning: \`wsx\`"`
Expected: empty (no new clippy findings in any file this plan modifies). Pre-existing clippy warnings in other files are out of scope.

Run: `cargo test --lib`
Expected: all tests pass except the pre-existing broken one.

- [ ] **Step 3: Push the branch and open a PR**

Use the existing `/pull-request` skill flow:

```bash
git push -u origin HEAD
gh pr create --title "feat: graceful handling for missing agent binaries" --body "$(cat <<'EOF'
## Summary

Stops wsx from exiting when the user attaches to a workspace whose
agent binary (claude/pi/hermes) is not installed. Replaces the crash
with a modal that names the missing binary and offers Esc to dismiss
or `s` to open an agent picker. Picking a new agent persists it,
mirrors the in-memory copy, and retries the attach automatically.

## Complexity Notes

- `ensure_workspace_session` now returns `Result<AttachReady>` instead
  of `Result<()>`. Three call sites updated; the pane-restore site at
  `src/app.rs:910` is intentionally left alone (it already discards
  errors with `let _ = ...`).
- Missing-binary detection uses `io::ErrorKind::NotFound` walked out
  of the `anyhow::Error` chain — no string parsing.
- The `Modal::AgentPicker` carries `current: AgentKind` (not just the
  workspace id) so the render path doesn't need App context.

## Test Steps

1. \`cargo test --lib\` — all tests pass (except the pre-existing
   broken \`attached_view_shows_status_row_for_other_workspace_needing_attention\`).
2. \`WSX_HERMES_BIN=/nonexistent/hermes cargo run\`, enter a Hermes
   workspace from the dashboard. Verify the modal appears, names
   \`/nonexistent/hermes\`, and accepts \`s\` / Esc.
3. From the picker, switch to Claude (with \`claude\` installed).
   Verify the workspace attaches and the agent column in
   \`wsx workspace list\` is now \`claude\`.
4. Repeat with all three agents to confirm the loop behavior when
   the new agent is also missing.

## Checklist

- [x] Tests added (spawn classification, ensure_workspace_session,
      modal rendering, key dispatch, picker confirm)
- [ ] Documentation updated (not applicable — internal behavior change)
EOF
)"
```

---

## Self-review

**Spec coverage:**

- §1 Error plumbing → Task 2.
- §2 Spawn call-site handling (AttachReady + 3 sites + leave 4th alone) → Task 4.
- §3 Modal variants + key dispatch → variants in Task 4 (compile stub) + Task 5 (render); dispatch in Tasks 6 & 7.
- §4 Picker confirm — persist + retry → Task 7.
- §5 AgentKind helpers → Task 1.
- §Testing (three layers) → Task 2 (spawn-level), Task 4 (ensure_workspace_session), Task 5 (render), Task 7 (picker confirm). All four layers covered.

**Placeholder scan:** no "TBD" / "TODO" / "appropriate error handling" / undefined types. The Task-1 string-search step ("Search `src/` for `Some(\"hermes\") => AgentKind`...") is action-able and bounded.

**Type consistency:**
- `AttachReady::Ok` / `AttachReady::AgentMissing` used consistently across Tasks 4, 6, 7.
- `Modal::AgentMissing { ws_id, agent, binary }` and `Modal::AgentPicker { ws_id, selected, current }` field names match across Tasks 4, 5, 6, 7.
- `set_workspace_agent` signature in Task 3 (`id, AgentKind`) matches the Task 7 call site (`app.store.set_workspace_agent(ws_id, new_agent)?`).
- `AgentKind::ALL` / `display_name()` / `default_binary()` / `store_value()` / `from_str_or_default()` names introduced in Task 1 are used in Tasks 2, 5, 6, 7 with the same shapes.
- `resolved_binary(agent)` returns a `String`, embedded into `Error::AgentBinaryMissing(String)` — Tasks 2 and 4 agree.

No gaps; no name mismatches found.
