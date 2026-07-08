# tmux-Shared Workspaces — Phase 2 (Remote Browsing & Attach) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** From machine B, press `H` to pick a configured ssh host, see that host's shared workspaces (fetched via `ssh <dest> wsx shared list --json`), and attach to any live session — rendered through the normal PTY plumbing, with detach leaving the remote agent running.

**Architecture:** Remote sessions follow the PM-pane precedent: a plain `Arc<Session>` slot on `App` (`app.remote`) outside the `AgentInstanceId`-keyed map, a full-screen `View::AttachedRemote` cloned from the `AttachedPm` render/input arms, and a child process that is just `ssh -t <dest> tmux attach -t =<name>` spawned through a new `spawn_command_session` (the generic bottom half of `spawn_session`, extracted). Discovery is a one-shot background fetch using the existing generation-counter + reconcile idiom, deserializing the Phase 1 wire contract. Nothing is written to the local DB — the remote list is ephemeral.

**Tech Stack:** Rust (edition 2024), portable-pty 0.9, serde/serde_json (existing deps), ssh + remote tmux invoked as external binaries only.

**Spec:** `docs/superpowers/specs/2026-07-08-tmux-shared-workspaces-design.md` — "Remote browsing and attach (machine B)" + "Failure handling" sections. Phase 1 is merged (PR #222).

## Global Constraints

- No new Cargo dependencies. ssh is exec'd as a binary; override seam `WSX_SSH_BIN` (mirrors `WSX_TMUX_BIN`/`WSX_CLAUDE_BIN`) so tests never need a network.
- Remote fetch command shape (spec): `ssh <dest> sh -lc 'wsx shared list --json'` — login shell so PATH resolves wsx on the host.
- Remote attach command shape: `ssh -t <dest> -- tmux attach -t =<session>` — exact-match `=` target (Phase 1 convention; names are sanitized `[A-Za-z0-9_-]` so no remote-shell quoting is needed).
- The wire contract is Phase 1's `SharedWorkspaceRecord`/`SharedAgentRecord` JSON, additive-only: the reader must tolerate unknown fields (serde's default) and missing hosts/old versions must degrade to an error modal, not a panic.
- A remote session's `Session.tmux_session` MUST be `None` — `kill()`/`Drop` then only sever the local ssh client; the remote agent persists server-side. Never call `kill_backend` semantics on a remote session.
- `shared_hosts` setting: newline-separated `name=ssh-dest` (e.g. `mini=eben@ebenmini.local`), stored via the generic settings table, edited with `wsx config edit shared_hosts`, kept separate from `remotes` (those values are full shell commands).
- The remote list is ephemeral: fetched on demand, held in `App`, never persisted.
- CI gates run separately: `cargo fmt --check`, `cargo clippy --all-targets`, `cargo test`. Run all three before every commit. Known ubuntu-CI flakes (not caused by this work; retry in isolation): `click_chip_auto_spawns_session_when_missing`, `ctrl_x_digit_works_while_reply_focused`. Tests that spawn a tmux client must EnvGuard-set `TERM=xterm-256color` (CI leaves TERM unset).
- Conventional commits (`feat(remote): …`); never commit to `main` — work on branch `tmux-shared-phase2`.

## Verified Facts (do not re-derive)

- `SessionManager` keeps PM in a separate `pm: Option<Arc<Session>>` slot (`src/pty/session.rs:545-548`); `App` mirrors it as `pub pm: Option<Arc<Session>>` (`src/app.rs:235`). Full-screen render for `View::AttachedPm` is `src/app/render.rs:605-680` (single `PaneSpec` through `attached::render_panes`); input is `handle_key_attached_pm` (`src/app/input.rs:1106-1152`); dispatch arm at `input.rs:2226`.
- `spawn_session` (`src/pty/session.rs:419-539`) is agent-specific only in lines 439-454 (command build + identity env); lines 455-538 (tmux wrap, spawn, killer, vt100 parser, reader thread, tokio writer task, `Session` literal) are generic. `Session.agent` is consumed only by `submit_writes` paste quirks; `prompt` capture is driven only by workspace handlers — both inert for a remote session.
- Background one-shot idiom: gen counter on `App` (`next_create_gen`/`pending_create_gen`, `app.rs:126-130`, alloc at `:508-512`), `tokio::spawn` + `reconcile_create_result(shared, gen, result)` (`app.rs:1705-1758`) that checks `pending == Some(my_gen)` before touching modals. `SharedApp = Arc<tokio::sync::Mutex<App>>` (`app.rs:840`).
- Modal templates: `Modal::AgentPicker { ws_id, selected, current }` (simple list picker; render `src/ui/modal/mod.rs:301-321`, keys `input.rs:1699-1720`); `Modal::UpdatesPanel { selected }` (scrollable results with dedicated renderer reading live App state; Enter mirrors the attach flow, `input.rs:1387-1436`). Modals that read App state are drawn from `render.rs` and early-returned by the generic `render()` guard at `mod.rs:167-181`.
- `shared_hosts` has an exact template: `src/commands/remotes.rs` (`parse`/`list`/`lookup`, first-`=` split, sort, last-write-wins), `known_setting_key` at `src/cli.rs:438-466`, generic `config edit` dispatch at `cli.rs:1395-1420` (no normalization needed for a plain key).
- `SharedWorkspaceRecord { repo, workspace, branch, worktree_path, agents }` / `SharedAgentRecord { label, agent, tmux_session, alive }` in `src/commands/shared.rs:10-25` currently derive `Serialize` only. `shared list --json` prints `serde_json::to_string_pretty` of the array (`cli.rs:1454-1480`). `AgentKind::from_str_or_default(Some(&s))` converts the `agent` string back (`src/pty/agent_kind.rs:27`).
- `H` is unbound on the dashboard (bound Char keys in `handle_key_dashboard` `input.rs:493-795`: `q k j h l i n N S e t v g c K J s d T r G z / ? p`). Footer pills are a curated subset (`src/ui/dashboard/layout.rs:117-127`) — new keys need no footer change to work; layout tests assert `hints.len()` if a pill IS added.

## File Structure

- **Create** `src/commands/shared_hosts.rs` — `SharedHost { name, dest }`, `parse`/`list`/`lookup` (clone of `remotes.rs`), plus `parse_shared_list_output(&str) -> Result<Vec<SharedWorkspaceRecord>>` and the async `fetch_shared_list(dest)` that shells out to ssh (`WSX_SSH_BIN` seam). All remote-host knowledge in one module.
- **Modify** `src/commands/shared.rs` (add `Deserialize` derives), `src/commands/mod.rs` (module decl), `src/cli.rs` (`known_setting_key`), `src/pty/session.rs` (extract `spawn_command_session`), `src/app.rs` (remote state fields, `RemoteTarget`, reconcile fn, attach/detach helpers), `src/ui/mod.rs` (`View::AttachedRemote`), `src/ui/modal/mod.rs` (`RemoteHostPicker`, `RemoteWorkspaceList` + render guard), `src/app/render.rs` (list renderer + attached-remote arm), `src/app/input.rs` (`H` key, picker/list/attached handlers, dispatch arms), `docs/book/src/integrations/shared-workspaces.md` + `remote-access.md` (docs).

---

### Task 1: `shared_hosts` setting module

**Files:**
- Create: `src/commands/shared_hosts.rs`
- Modify: `src/commands/mod.rs` (add `pub mod shared_hosts;`), `src/cli.rs:438-466` (`known_setting_key` adds `"shared_hosts"`)
- Test: co-located `#[cfg(test)]` + the existing known-key test at `src/cli.rs:2229` region

**Interfaces:**
- Consumes: `Store::get_setting` (`src/data/settings.rs:8`).
- Produces: `pub struct SharedHost { pub name: String, pub dest: String }`; `pub fn parse(text: &str) -> Vec<SharedHost>`; `pub fn list(store: &Store) -> Result<Vec<SharedHost>>` (sorted by name); `pub fn lookup(store: &Store, name: &str) -> Result<Option<SharedHost>>` (last-write-wins). Tasks 5-6 consume `list`.

- [ ] **Step 1: Write failing tests** (mirror `src/commands/remotes.rs` tests; same behaviors, key `shared_hosts`):

```rust
#[test]
fn parse_splits_on_first_equals_and_skips_blank_and_invalid() {
    let hosts = parse("mini=eben@ebenmini.local\n\nbad-line\nlab=user@lab=box\n");
    assert_eq!(hosts.len(), 2);
    assert_eq!(hosts[0].name, "mini");
    assert_eq!(hosts[0].dest, "eben@ebenmini.local");
    // first '=' splits; the rest stays in dest
    assert_eq!(hosts[1].dest, "user@lab=box");
}

#[test]
fn list_reads_setting_sorted_and_lookup_is_last_write_wins() {
    let store = Store::open_in_memory().unwrap();
    store
        .set_setting("shared_hosts", "b=host-b\na=host-a\na=host-a2")
        .unwrap();
    let hosts = list(&store).unwrap();
    assert_eq!(hosts[0].name, "a");
    assert_eq!(lookup(&store, "a").unwrap().unwrap().dest, "host-a2");
    assert!(lookup(&store, "zz").unwrap().is_none());
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test shared_hosts` → COMPILE ERROR (module missing).

- [ ] **Step 3: Implement** by cloning `src/commands/remotes.rs` line-for-line with the struct/key renamed (`Remote`→`SharedHost`, `command`→`dest`, `"remotes"`→`"shared_hosts"`) and a module doc stating the value semantics (`name=ssh-destination`, NOT a shell command — that's what distinguishes it from `remotes`). Add `"shared_hosts"` to `known_setting_key` and extend its test so `wsx config edit shared_hosts` works with zero further wiring.

- [ ] **Step 4: Gates** — `cargo test shared_hosts && cargo test cli:: && cargo fmt --check && cargo clippy --all-targets` → PASS.

- [ ] **Step 5: Commit** — `feat(remote): add shared_hosts setting module`

---

### Task 2: Deserialize the wire contract + fetch/parse helpers

**Files:**
- Modify: `src/commands/shared.rs:10-25` (derives), `src/commands/shared_hosts.rs` (fetch + parse)
- Test: co-located in both files

**Interfaces:**
- Consumes: Task 1's module; `SharedWorkspaceRecord`/`SharedAgentRecord`.
- Produces:
  - `SharedWorkspaceRecord`/`SharedAgentRecord` gain `serde::Deserialize` (one shared definition for both ends of the wire, per the module doc's contract note).
  - In `shared_hosts.rs`: `pub fn ssh_bin() -> String` (honors `WSX_SSH_BIN`, defaults `"ssh"`); `pub fn parse_shared_list_output(stdout: &str) -> crate::error::Result<Vec<crate::commands::shared::SharedWorkspaceRecord>>`; `pub async fn fetch_shared_list(dest: &str) -> crate::error::Result<Vec<crate::commands::shared::SharedWorkspaceRecord>>`.
  - Fetch failure carries stderr: map non-zero exit to `Error::UserInput(format!("ssh {dest}: {stderr}"))` (spec: "error modal with the captured stderr").

- [ ] **Step 1: Write failing tests**

In `shared.rs` (roundtrip pins the contract from both directions):

```rust
#[test]
fn records_roundtrip_serde_and_tolerate_unknown_fields() {
    let json = r#"[{
        "repo": "r", "workspace": "w", "branch": "wsx/w",
        "worktree_path": "/tmp/r/w",
        "future_field": "ignored",
        "agents": [{"label": "claude", "agent": "claude",
                    "tmux_session": "wsx-r-w", "alive": true,
                    "another_future_field": 7}]
    }]"#;
    let recs: Vec<SharedWorkspaceRecord> = serde_json::from_str(json).unwrap();
    assert_eq!(recs[0].workspace, "w");
    assert_eq!(recs[0].agents[0].tmux_session.as_deref(), Some("wsx-r-w"));
    assert!(recs[0].agents[0].alive);
    // and what we serialize, we can deserialize
    let back: Vec<SharedWorkspaceRecord> =
        serde_json::from_str(&serde_json::to_string(&recs).unwrap()).unwrap();
    assert_eq!(back[0].agents[0].label, "claude");
}
```

In `shared_hosts.rs` (fetch through a fake ssh — no network):

```rust
#[tokio::test]
async fn fetch_shared_list_parses_fake_ssh_output_and_surfaces_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = crate::test_support::EnvGuard::new();
    let ok = dir.path().join("fake-ssh-ok.sh");
    std::fs::write(&ok, "#!/bin/sh\necho '[{\"repo\":\"r\",\"workspace\":\"w\",\"branch\":\"b\",\"worktree_path\":\"/x\",\"agents\":[]}]'\n").unwrap();
    std::fs::set_permissions(&ok, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    env.set("WSX_SSH_BIN", ok.to_str().unwrap());
    let recs = fetch_shared_list("mini").await.unwrap();
    assert_eq!(recs[0].workspace, "w");

    let bad = dir.path().join("fake-ssh-bad.sh");
    std::fs::write(&bad, "#!/bin/sh\necho 'connection refused' >&2\nexit 255\n").unwrap();
    std::fs::set_permissions(&bad, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    env.set("WSX_SSH_BIN", bad.to_str().unwrap());
    let err = fetch_shared_list("mini").await.unwrap_err().to_string();
    assert!(err.contains("connection refused"), "stderr must reach the error: {err}");
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test records_roundtrip && cargo test fetch_shared_list` → COMPILE ERRORS.

- [ ] **Step 3: Implement.** Derives: `#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]` on both records (serde ignores unknown fields by default — do NOT add `deny_unknown_fields`). In `shared_hosts.rs`:

```rust
pub fn ssh_bin() -> String {
    std::env::var("WSX_SSH_BIN").unwrap_or_else(|_| "ssh".to_string())
}

pub fn parse_shared_list_output(
    stdout: &str,
) -> crate::error::Result<Vec<crate::commands::shared::SharedWorkspaceRecord>> {
    serde_json::from_str(stdout).map_err(|e| {
        crate::error::Error::UserInput(format!("bad shared-list JSON from host: {e}"))
    })
}

/// Run `ssh <dest> sh -lc 'wsx shared list --json'` and parse the result.
/// Login shell so PATH resolves wsx on the host. Non-zero exit maps to a
/// user-facing error carrying the captured stderr (spec: failure handling).
pub async fn fetch_shared_list(
    dest: &str,
) -> crate::error::Result<Vec<crate::commands::shared::SharedWorkspaceRecord>> {
    let out = tokio::process::Command::new(ssh_bin())
        .args([dest, "sh", "-lc", "wsx shared list --json"])
        .output()
        .await
        .map_err(|e| crate::error::Error::UserInput(format!("ssh spawn failed: {e}")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(crate::error::Error::UserInput(format!(
            "ssh {dest}: {}",
            stderr.trim()
        )));
    }
    parse_shared_list_output(&String::from_utf8_lossy(&out.stdout))
}
```

(Check `crate::error::Error` variants — if `UserInput` doesn't fit house style for this layer, use the variant `remotes`/CLI errors use; keep the stderr in the message either way.)

- [ ] **Step 4: Gates** — full `cargo test && cargo fmt --check && cargo clippy --all-targets` → PASS.

- [ ] **Step 5: Commit** — `feat(remote): deserialize shared-list wire contract and fetch over ssh`

---

### Task 3: Extract `spawn_command_session`

**Files:**
- Modify: `src/pty/session.rs:419-539`
- Test: co-located

**Interfaces:**
- Consumes: existing plumbing.
- Produces: `pub fn spawn_command_session(child_cmd: CommandBuilder, cols: u16, rows: u16, agent: AgentKind, tmux: Option<&str>) -> Result<Session>` — the generic bottom half (tmux wrap → spawn → killer/parser/reader/writer → `Session` literal). `spawn_session` keeps its signature and becomes: build agent command (lines 439-450) + identity env (451-454) + delegate. Behavior identical for every existing caller. Task 7 calls `spawn_command_session` directly with an ssh command, `tmux: None`, `agent: AgentKind::Claude` (inert for remote sessions — `agent` only drives paste quirks via `submit_writes`, which remote sessions never use; note this in the doc comment).

- [ ] **Step 1: Write failing test**

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spawn_command_session_runs_arbitrary_command_through_pty() {
    let mut cmd = portable_pty::CommandBuilder::new("/bin/sh");
    cmd.args(["-c", "printf remote-hello; sleep 5"]);
    cmd.cwd("/tmp");
    let session = spawn_command_session(cmd, 80, 24, AgentKind::Claude, None).unwrap();
    let mut seen = false;
    for _ in 0..50 {
        if session.parser.lock().unwrap().screen().contents().contains("remote-hello") {
            seen = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    session.kill();
    assert!(seen, "PTY must deliver the command's output");
    assert!(session.tmux_session.is_none());
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test spawn_command_session_runs` → COMPILE ERROR (fn missing).

- [ ] **Step 3: Implement the extraction.** Move `session.rs:455-538` (starting at the `let child_cmd = match tmux {` wrap block, through the `Ok(Session { .. })`) into the new fn; `spawn_session` builds the agent command + identity env and returns `spawn_command_session(child_cmd, cols, rows, agent, tmux)`. Keep the `openpty` INSIDE the new fn (it is generic); `spawn_session` no longer opens the pty itself. The `Error::AgentBinaryMissing(resolved_binary(agent))` mapping on binary-not-found stays in the generic half but must keep producing the right binary name for agent spawns — pass through the `agent` param (for the ssh case a missing ssh binary then reports as the ssh path via `is_binary_not_found`; acceptable and still actionable). Pure refactor otherwise: no behavior change, existing suite is the regression net.

- [ ] **Step 4: Gates** — full `cargo test && cargo fmt --check && cargo clippy --all-targets` → PASS (the two tmux e2e tests exercising `spawn_session` are the refactor's proof).

- [ ] **Step 5: Commit** — `refactor(pty): extract generic spawn_command_session from spawn_session`

---

### Task 4: Remote list state + background fetch reconcile

**Files:**
- Modify: `src/app.rs` (state fields near `:126-130`, alloc fn near `:508`, reconcile fn near `reconcile_create_result` at `:1705`)
- Test: `src/app/input_tests.rs`

**Interfaces:**
- Consumes: `fetch_shared_list` (Task 2).
- Produces on `App`:
  - `pub remote_list: Option<RemoteList>` where `pub struct RemoteList { pub host_name: String, pub dest: String, pub records: Vec<crate::commands::shared::SharedWorkspaceRecord> }` (defined in `app.rs`)
  - `pub next_remote_gen: u64`, `pub pending_remote_gen: Option<u64>`, `pub fn alloc_remote_gen(&mut self) -> u64` (clone of `alloc_create_gen`)
  - `pub(crate) async fn reconcile_remote_list(shared: SharedApp, gen: u64, host_name: String, dest: String, result: crate::error::Result<Vec<SharedWorkspaceRecord>>)` — if `pending_remote_gen != Some(gen)`: do nothing (stale). If mine: clear `pending_remote_gen`; on Ok store `RemoteList` and set `app.modal = Some(Modal::RemoteWorkspaceList { selected: 0 })`; on Err set `Modal::Error { message }` with the fetch error text.
- Task 5 spawns the fetch; Task 6 renders from `app.remote_list`.

- [ ] **Step 1: Write failing test** (pure reconcile logic — no ssh needed):

```rust
#[tokio::test]
async fn reconcile_remote_list_stores_records_and_discards_stale_gens() {
    let (shared, _env) = /* build SharedApp via the existing app fixture used by
        reconcile_create tests — in-memory store, no repos needed */;
    let (g1, g2) = {
        let mut app = shared.lock().await;
        (app.alloc_remote_gen(), app.alloc_remote_gen()) // g2 supersedes g1
    };
    let rec = crate::commands::shared::SharedWorkspaceRecord {
        repo: "r".into(), workspace: "w".into(), branch: "b".into(),
        worktree_path: "/x".into(), agents: vec![],
    };
    // Stale gen: ignored entirely.
    crate::app::reconcile_remote_list(
        shared.clone(), g1, "mini".into(), "host".into(), Ok(vec![rec.clone()])).await;
    assert!(shared.lock().await.remote_list.is_none());
    // Current gen: stored + list modal opened.
    crate::app::reconcile_remote_list(
        shared.clone(), g2, "mini".into(), "host".into(), Ok(vec![rec])).await;
    {
        let app = shared.lock().await;
        assert_eq!(app.remote_list.as_ref().unwrap().records.len(), 1);
        assert!(matches!(app.modal, Some(Modal::RemoteWorkspaceList { .. })));
        assert!(app.pending_remote_gen.is_none());
    }
    // Error path: error modal with the message.
    let g3 = shared.lock().await.alloc_remote_gen();
    crate::app::reconcile_remote_list(
        shared.clone(), g3, "mini".into(), "host".into(),
        Err(crate::error::Error::UserInput("ssh mini: refused".into()))).await;
    match &shared.lock().await.modal {
        Some(Modal::Error { message }) => assert!(message.contains("refused")),
        other => panic!("expected error modal, got {other:?}"),
    }
}
```

(Adapt fixture construction to whatever `reconcile_create_result` tests actually use — read them first; the assertions above are the contract. `Modal::RemoteWorkspaceList` is added in this task as a bare `{ selected: usize }` variant so this compiles; its handler/renderer come in Task 6.)

- [ ] **Step 2: Run to verify failure** — COMPILE ERROR.

- [ ] **Step 3: Implement** the fields (init in `App::new` alongside `next_create_gen`), `alloc_remote_gen`, `RemoteList`, the reconcile fn modeled line-for-line on `reconcile_create_result`'s gen-guard shape, and the bare `Modal::RemoteWorkspaceList { selected: usize }` variant (plus a temporary arm in the modal key handler that just closes on Esc, so the enum is total — Task 6 replaces it).

- [ ] **Step 4: Gates** → PASS. **Step 5: Commit** — `feat(remote): remote-list state and fetch reconcile`

---

### Task 5: `H` host picker

**Files:**
- Modify: `src/ui/modal/mod.rs` (variant + render arm), `src/app/input.rs` (dashboard `H` arm + picker key handler)
- Test: `src/app/input_tests.rs`

**Interfaces:**
- Consumes: `shared_hosts::list` (Task 1), `alloc_remote_gen`/`reconcile_remote_list`/`fetch_shared_list` (Tasks 2/4).
- Produces: `Modal::RemoteHostPicker { hosts: Vec<(String, String)>, selected: usize }` (name, dest pairs snapshot — the picker is self-contained like `AgentPicker`). `H` on the dashboard opens it (any selection state; no workspace required). Enter allocates a gen, swaps the modal to `Modal::RemoteListLoading { host_name: String }` (new lightweight variant rendered by the generic `render()` as a one-line "fetching shared workspaces from <host>…" box), and `tokio::spawn`s the fetch + reconcile. Esc closes. Zero configured hosts → `Modal::Error` explaining `wsx config edit shared_hosts`.

- [ ] **Step 1: Write failing tests** (mirror the `S`-key and AgentPicker tests in `input_tests.rs`):

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capital_h_opens_host_picker_with_configured_hosts() {
    // fixture app; store.set_setting("shared_hosts", "mini=eben@mini\nlab=eben@lab")
    press(&mut app, &shared, key('H')).await;
    match &app.modal {
        Some(Modal::RemoteHostPicker { hosts, selected }) => {
            assert_eq!(hosts.len(), 2);
            assert_eq!(hosts[0].0, "lab"); // sorted by name
            assert_eq!(*selected, 0);
        }
        other => panic!("expected host picker, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capital_h_with_no_hosts_explains_config_edit() {
    press(&mut app, &shared, key('H')).await;
    match &app.modal {
        Some(Modal::Error { message }) => assert!(message.contains("config edit shared_hosts")),
        other => panic!("expected error modal, got {other:?}"),
    }
}
```

(Use the file's actual key-dispatch helper — read neighbors like the `capital_s_opens_new_workspace_modal_with_shared_true` test for the exact fixture/press idiom.)

- [ ] **Step 2: Run to verify failure.** — COMPILE ERROR / FAIL.

- [ ] **Step 3: Implement.** Dashboard arm (place near the `S` arm):

```rust
(KeyCode::Char('H'), _) => {
    let hosts: Vec<(String, String)> = crate::commands::shared_hosts::list(&app.store)
        .unwrap_or_default()
        .into_iter()
        .map(|h| (h.name, h.dest))
        .collect();
    if hosts.is_empty() {
        app.modal = Some(Modal::Error {
            message: "no shared hosts configured — add name=ssh-dest lines via `wsx config edit shared_hosts`".into(),
        });
    } else {
        app.modal = Some(Modal::RemoteHostPicker { hosts, selected: 0 });
    }
}
```

Picker handler (clone AgentPicker's Up/Down/Enter/Esc shape); Enter:

```rust
KeyCode::Enter => {
    let (name, dest) = hosts[selected].clone();
    let gen = app.alloc_remote_gen();
    app.modal = Some(Modal::RemoteListLoading { host_name: name.clone() });
    let shared_clone = shared.clone();
    tokio::spawn(async move {
        let result = crate::commands::shared_hosts::fetch_shared_list(&dest).await;
        crate::app::reconcile_remote_list(shared_clone, gen, name, dest, result).await;
    });
}
```

Render arms in `mod.rs`: picker renders name + dest rows with the `>` selected marker (AgentPicker style); `RemoteListLoading` renders a single-line box. Esc during loading just closes the modal — the reconcile's gen guard plus `pending_remote_gen` mismatch (clear `pending_remote_gen` on Esc) makes the late result a no-op.

- [ ] **Step 4: Gates** → PASS. **Step 5: Commit** — `feat(remote): H opens shared-hosts picker and fetches remote list`

---

### Task 6: Remote workspace list modal

**Files:**
- Modify: `src/ui/modal/mod.rs` (render guard at `:167-181` adds `RemoteWorkspaceList`), `src/app/render.rs` (dedicated renderer, drawn like `render_updates_panel`), `src/app/input.rs` (key handler replacing Task 4's stub)
- Test: `src/app/input_tests.rs` + a render smoke test following how `render_updates_panel` is tested (grep for its tests first)

**Interfaces:**
- Consumes: `app.remote_list: Option<RemoteList>` (Task 4).
- Produces: rows are FLATTENED per agent instance — spec: "Multiple agent instances appear as separate attachable entries." Row model (helper in `app.rs` so input + render agree):

```rust
/// One attachable row of the remote list: workspace context + one agent session.
pub(crate) struct RemoteRow<'a> {
    pub workspace: &'a str,
    pub repo: &'a str,
    pub branch: &'a str,
    pub label: &'a str,          // "claude", "codex#2"
    pub tmux_session: Option<&'a str>,
    pub alive: bool,
}
pub(crate) fn remote_rows(list: &RemoteList) -> Vec<RemoteRow<'_>>
```

Keys: `j`/`k`/Up/Down move `selected` (clamped to rows); Enter on an `alive` row with a `tmux_session` → Task 7's `attach_remote` (this task lands a stub call site guarded by `#[allow(unused)]` or lands after Task 7 — see Step 3 note); Enter on a dead/ref-less row → keep modal, show inline notice (ProcessList's `notice` idiom); `r` re-fetches (re-runs Task 5's Enter flow for the same host); Esc closes and clears `app.remote_list`.

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn remote_rows_flatten_agents_and_mark_dead() {
    let list = RemoteList { host_name: "mini".into(), dest: "d".into(), records: vec![
        SharedWorkspaceRecord {
            repo: "r".into(), workspace: "w".into(), branch: "b".into(),
            worktree_path: "/x".into(),
            agents: vec![
                SharedAgentRecord { label: "claude".into(), agent: "claude".into(),
                                    tmux_session: Some("wsx-r-w".into()), alive: true },
                SharedAgentRecord { label: "codex#2".into(), agent: "codex".into(),
                                    tmux_session: None, alive: false },
            ],
        },
    ]};
    let rows = remote_rows(&list);
    assert_eq!(rows.len(), 2);
    assert!(rows[0].alive && rows[0].tmux_session.is_some());
    assert!(!rows[1].alive);
}
```

Plus an input test: with a two-row `remote_list` seeded on the app and `Modal::RemoteWorkspaceList { selected: 0 }` open, `j` moves selection to 1, Enter on the dead row leaves the modal open with a notice, Esc clears `app.remote_list`.

- [ ] **Step 2: Run to verify failure.**

- [ ] **Step 3: Implement.** Renderer: dedicated fn in `render.rs` (early-return guard entry in modal `render()` like UpdatesPanel), rows formatted `repo/workspace  branch  label  ●alive|✗dead`, title carries the host badge: `shared workspaces on <host_name>`. Handler mutates `selected`/notice by rebuilding the modal (house idiom). For Enter-on-alive: build `RemoteTarget { host_name, dest, tmux }` from the row + `app.remote_list` and call `crate::app::attach_remote(app, target, cols, rows)` — Task 7 defines it; to keep THIS task compiling and shippable first, land Enter-on-alive as the same inline notice `"attach lands in the next commit"` ONLY if Task 7 is not yet merged, and replace it in Task 7's Step 3 (the plan's task order makes Task 7 the very next commit; the placeholder never ships beyond one commit).

- [ ] **Step 4: Gates** → PASS. **Step 5: Commit** — `feat(remote): remote workspace list modal with per-agent rows`

---

### Task 7: Remote attach — `View::AttachedRemote`

**Files:**
- Modify: `src/app.rs` (`RemoteTarget`, `pub remote`, `attach_remote`, `detach_remote`), `src/ui/mod.rs:12-30` (`View::AttachedRemote`), `src/app/render.rs` (arm cloned from `:605-680`), `src/app/input.rs` (handler + dispatch arm at `:2226`; replace Task 6's Enter placeholder)
- Test: `src/app/input_tests.rs` (fake-ssh e2e via `WSX_SSH_BIN`)

**Interfaces:**
- Consumes: `spawn_command_session` (Task 3), `ssh_bin()` (Task 2).
- Produces:

```rust
#[derive(Debug, Clone)]
pub struct RemoteTarget { pub host_name: String, pub dest: String, pub tmux: String }
// on App:
pub remote: Option<std::sync::Arc<crate::pty::session::Session>>,
pub remote_target: Option<RemoteTarget>,

/// Spawn `ssh -t <dest> -- tmux attach -t =<tmux>` through the PTY plumbing
/// and enter View::AttachedRemote. The Session's tmux_session is None on
/// purpose: kill()/Drop sever only the local ssh client; the remote agent
/// persists in the remote tmux server (the Phase 1 persistence contract,
/// one hop away).
pub(crate) fn attach_remote(app: &mut App, target: RemoteTarget, cols: u16, rows: u16) -> Result<()>
pub(crate) fn detach_remote(app: &mut App)  // kill ssh client, clear remote+target, View::Dashboard
```

`attach_remote` body:

```rust
let mut cmd = portable_pty::CommandBuilder::new(crate::commands::shared_hosts::ssh_bin());
cmd.args(["-t", &target.dest, "--", "tmux", "attach", "-t", &format!("={}", target.tmux)]);
let session = crate::pty::session::spawn_command_session(
    cmd, cols, rows, crate::pty::session::AgentKind::Claude, None,
)?;
app.remote = Some(std::sync::Arc::new(session));
app.remote_target = Some(target);
app.modal = None;
app.view = crate::ui::View::AttachedRemote;
Ok(())
```

Input handler `handle_key_attached_remote`: cloned from `handle_key_attached_pm` (`input.rs:1106-1152`) minus the PM leader menu — forward `encode_key(k)` bytes to the session writer with `scroll_to_live()`; `Ctrl-x d` (leader style, matching attached views) detaches via `detach_remote`. If the ssh child exits (status `Exited`), any key returns to the dashboard with `Modal::Error { message: "remote session ended: <host>/<tmux>" }` — covers spec's "stale session name → error modal" (tmux prints `can't find session` to the PTY before exit; the modal points at it). Render arm: clone the `AttachedPm` block substituting `app.remote` and label `format!("{}/{}", host_name, tmux)`.

- [ ] **Step 1: Write failing e2e test** (fake ssh = local script; no network, no remote tmux):

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attach_remote_spawns_ssh_and_detach_severs_client_only() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = EnvGuard::new();
    // Fake ssh: prove argv shape, then stream a heartbeat like a remote attach.
    let log = dir.path().join("ssh-args.log");
    let fake = dir.path().join("fake-ssh.sh");
    std::fs::write(&fake, format!(
        "#!/bin/sh\necho \"$@\" > {}\nfor i in $(seq 1 60); do echo remote-beat; sleep 1; done\n",
        log.display())).unwrap();
    std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    env.set("WSX_SSH_BIN", fake.to_str().unwrap());

    // fixture app...
    crate::app::attach_remote(&mut app, RemoteTarget {
        host_name: "mini".into(), dest: "eben@mini".into(), tmux: "wsx-r-w".into(),
    }, 80, 24).unwrap();
    assert!(matches!(app.view, View::AttachedRemote));
    let session = app.remote.clone().unwrap();
    // beats arrive through the PTY
    let mut seen = false;
    for _ in 0..50 {
        if session.parser.lock().unwrap().screen().contents().contains("remote-beat") { seen = true; break; }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(seen);
    // argv shape: -t <dest> -- tmux attach -t =<name>
    let args = std::fs::read_to_string(&log).unwrap();
    assert!(args.contains("-t eben@mini -- tmux attach -t =wsx-r-w"), "got: {args}");
    assert!(session.tmux_session.is_none(), "remote sessions must never own a local tmux backend");

    crate::app::detach_remote(&mut app);
    assert!(app.remote.is_none() && matches!(app.view, View::Dashboard));
}
```

- [ ] **Step 2: Run to verify failure.** — COMPILE ERROR.

- [ ] **Step 3: Implement** per the interfaces above; wire Task 6's Enter-on-alive to `attach_remote` (removing the one-commit placeholder notice); add the `View::AttachedRemote` dispatch arm and render arm. On quit, `App` drop kills the ssh child via `Session::Drop` — nothing extra needed; note it in `detach_remote`'s doc comment.

- [ ] **Step 4: Gates** → PASS. **Step 5: Commit** — `feat(remote): attach to remote shared workspaces over ssh`

---

### Task 8: Docs + spec status

**Files:**
- Modify: `docs/book/src/integrations/shared-workspaces.md` (new "Browsing another machine" section), `docs/book/src/integrations/remote-access.md` (update the cross-link to mention in-TUI browsing), `docs/superpowers/specs/2026-07-08-tmux-shared-workspaces-design.md` (status note: Phase 2 implemented)

- [ ] **Step 1: Write the docs section**: configuring `shared_hosts` (`wsx config edit shared_hosts`, `name=ssh-dest` lines), `H` → pick host → list (repo/workspace/branch/agent/alive, host badge) → Enter attaches, `r` refreshes, detach (`Ctrl-x d`) leaves the remote agent running; requirements (ssh key access; wsx on the host's login-shell PATH; the host created the workspaces as shared); failure modes (unreachable host / wsx missing / dead session → error modal with stderr). Match the page's existing tone; keep the ephemeral-list property explicit (nothing persisted locally).
- [ ] **Step 2: Validate** — `mdbook build docs/book` if installed (skip + note otherwise).
- [ ] **Step 3: Gates** (docs-only, but run anyway) → PASS. **Commit** — `docs(remote): document remote browsing and attach`

---

## Post-plan checklist (before PR)

- [ ] Verify end-to-end against a real second host if available (or localhost ssh loopback: add `local=<user>@localhost` to shared_hosts): create a shared workspace, `H` → pick → attach → interact → detach → confirm agent still alive on the host.
- [ ] Confirm quitting wsx while `View::AttachedRemote` is active leaves the remote agent running (only the ssh client dies).
- [ ] Open PR per the pull-request skill; reference the spec, this plan, and PR #222.
