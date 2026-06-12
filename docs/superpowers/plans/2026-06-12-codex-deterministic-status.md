# Codex Deterministic Status via `notify` — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give Codex deterministic turn-end status (Done / Blocked) by routing its `notify` `agent-turn-complete` event into the same `workspace_status` "reported" lane Claude's `Stop` hook uses.

**Architecture:** Implement the existing `StatusIntegration` trait for Codex (`CodexStatus`), parsing Codex's `notify` argv-JSON payload into a `ReportedState` and emitting spawn-time `-c notify=[...]` wiring. A new `wsx status from-notify` CLI verb ingests the payload (Codex passes JSON as the final argv, not stdin). The existing `Status::classify()` + `fresh_reported()` freshness gate and the `sessionx` JSONL heuristic are unchanged — Codex simply gains a deterministic turn-end push in the lane those layers already consume.

**Tech Stack:** Rust, `serde_json`, `portable-pty` `CommandBuilder`, `rusqlite` (SQLite), `tempfile` (tests). Target: Linux/macOS (Unix).

**Design spec:** [`docs/superpowers/specs/2026-06-12-codex-deterministic-status-design.md`](../specs/2026-06-12-codex-deterministic-status-design.md). Prior foundation: [`2026-06-11-agent-driven-status-design.md`](../specs/2026-06-11-agent-driven-status-design.md).

## Key facts an implementer needs

- **Codex `notify` payload** (verified, Codex v0.137.0) arrives as the **final argv element** of the notify program invocation, kebab-case keys:
  ```json
  {"type":"agent-turn-complete","thread-id":"…","turn-id":"…","cwd":"…",
   "client":"codex_exec","input-messages":["…"],"last-assistant-message":"pong"}
  ```
- **Why `notify`, not `hooks.*`:** at v0.137 Codex `hooks.*` are silently gated by project-trust (wsx worktrees live outside trusted roots) and did not fire in testing; `notify` has no trust gate, is `-c`-injectable, and fires in both TUI and exec. See the spec's Findings.
- **The `reported` lane:** `wsx status from-*` writes `workspace_status` via `store.set_workspace_status(ws_id, state, message, source)`. `source` is a free TEXT tag (`"model"` / `"hook"` today; this plan adds `"notify"`). `Status::classify()` consumes it freshness-gated by `fresh_reported()` (`src/app.rs:699`) — a push stays authoritative only until JSONL activity appears after it. **No schema or classifier change is needed.**
- **CI is `-D warnings`** (`.github/workflows/ci.yml`): clippy `--all-targets` and build both fail on warnings. Every commit must be warning-clean — so a new integration type must be *referenced* (wired into `for_agent`) in the same commit it's introduced, never left as a dead `#[cfg(test)]`-only struct.
- **`CommandBuilder` argv** is inspectable in tests via `cmd.get_argv() -> Vec<OsString>` (see existing `build_claude_command` tests, e.g. `src/pty/session.rs:1754`).

---

### Task 1: `CodexStatus` integration — `parse_event` + `spawn_wiring`, wired into `for_agent`

Introduces the whole harness integration in one warning-clean commit (the struct is constructed by `for_agent`, so no dead-code warning).

**Files:**
- Create: `src/agent/status/codex.rs`
- Modify: `src/agent/status/mod.rs` (add `pub mod codex;`, `static CODEX`, route `AgentKind::Codex`, update existing test)

- [ ] **Step 1: Write the failing test for `parse_event`**

Create `src/agent/status/codex.rs` with exactly this content (struct, trait impl with `parse_event` only for now, and tests):

```rust
//! Codex status integration. Codex's `hooks.*` system is unusable for wsx
//! worktrees at v0.137 (silently gated behind project trust), but its `notify`
//! program fires reliably on `agent-turn-complete` with no trust gate and is
//! injectable via `-c`. We wire `notify` to call back into
//! `wsx status from-notify`, mapping the turn-end event to Done/Blocked exactly
//! as Claude's `Stop` hook does. Turn-start / working stays on the tier-3 JSONL
//! heuristic; the `fresh_reported` gate hands off between them.

use super::{SpawnWiring, StatusIntegration};
use crate::data::store::ReportedState;
use std::path::Path;

pub struct CodexStatus;

impl StatusIntegration for CodexStatus {
    /// Codex's `notify` fires only `agent-turn-complete`. Map it like Claude's
    /// `Stop`: a `?`-terminated final message reads as a blocking prose
    /// question, otherwise the turn completed. The payload uses kebab-case keys
    /// and arrives via argv (see `from-notify`), but `parse_event` only sees the
    /// already-parsed JSON value.
    fn parse_event(&self, json: &serde_json::Value) -> Option<ReportedState> {
        if json.get("type").and_then(|v| v.as_str()) != Some("agent-turn-complete") {
            return None;
        }
        let ends_with_q = json
            .get("last-assistant-message")
            .and_then(|v| v.as_str())
            .map(|s| s.trim_end().ends_with('?'))
            .unwrap_or(false);
        Some(if ends_with_q {
            ReportedState::Blocked
        } else {
            ReportedState::Done
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(json: serde_json::Value) -> Option<ReportedState> {
        CodexStatus.parse_event(&json)
    }

    #[test]
    fn turn_complete_is_done() {
        assert_eq!(
            ev(serde_json::json!({"type": "agent-turn-complete", "last-assistant-message": "All set."})),
            Some(ReportedState::Done)
        );
    }

    #[test]
    fn turn_complete_with_question_is_blocked() {
        assert_eq!(
            ev(serde_json::json!({"type": "agent-turn-complete", "last-assistant-message": "Which library should I use?"})),
            Some(ReportedState::Blocked)
        );
    }

    #[test]
    fn turn_complete_without_message_degrades_to_done() {
        assert_eq!(
            ev(serde_json::json!({"type": "agent-turn-complete"})),
            Some(ReportedState::Done)
        );
    }

    #[test]
    fn other_or_missing_type_is_ignored() {
        assert_eq!(ev(serde_json::json!({"type": "session-start"})), None);
        assert_eq!(ev(serde_json::json!({})), None);
    }
}
```

Add the module declaration to `src/agent/status/mod.rs` immediately after the existing `pub mod claude;` (line 10):

```rust
pub mod claude;
pub mod codex;
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib agent::status::codex`
Expected: FAIL to **compile** — `for_agent` still maps Codex to `NOOP`, but that's fine; the failure here is that `mod.rs` references `codex` and the struct is unused in non-test build → `dead_code` under `-D warnings`. (If your local `cargo test` doesn't enable `-D warnings`, the tests will instead PASS but `cargo clippy --all-targets -- -D warnings` will fail.) Either way, proceed to wire it in Step 3.

- [ ] **Step 3: Wire `CodexStatus` into `for_agent` and fix the existing routing test**

In `src/agent/status/mod.rs`, add the static after `static NOOP` (line 51):

```rust
static CLAUDE: claude::ClaudeStatus = claude::ClaudeStatus;
static CODEX: codex::CodexStatus = codex::CodexStatus;
static NOOP: NoopStatus = NoopStatus;
```

Update `for_agent` (lines 55-60) to route Codex:

```rust
pub fn for_agent(agent: AgentKind) -> &'static dyn StatusIntegration {
    match agent {
        AgentKind::Claude => &CLAUDE,
        AgentKind::Codex => &CODEX,
        _ => &NOOP,
    }
}
```

Update the existing `other_agents_resolve_to_noop` test (lines 75-86) — remove `AgentKind::Codex` from the loop (it is no longer a no-op) and add a Codex routing test. Replace the test with:

```rust
    #[test]
    fn other_agents_resolve_to_noop() {
        let ev = serde_json::json!({"hook_event_name": "UserPromptSubmit"});
        for agent in [AgentKind::Pi, AgentKind::Hermes] {
            assert_eq!(for_agent(agent).parse_event(&ev), None);
            assert!(
                for_agent(agent)
                    .spawn_wiring(Path::new("/usr/bin/wsx"), false)
                    .is_none()
            );
        }
    }

    #[test]
    fn codex_resolves_to_codex_integration() {
        let ev = serde_json::json!({"type": "agent-turn-complete", "last-assistant-message": "done"});
        assert_eq!(
            for_agent(AgentKind::Codex).parse_event(&ev),
            Some(ReportedState::Done)
        );
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib agent::status`
Expected: PASS (all `claude`, `codex`, and `mod` status tests).

- [ ] **Step 5: Add `spawn_wiring` — write the failing test**

Append these two tests to the `tests` module in `src/agent/status/codex.rs` (before the closing `}`):

```rust
    #[test]
    fn spawn_wiring_emits_notify_pointing_at_from_notify() {
        let w = CodexStatus
            .spawn_wiring(Path::new("/usr/local/bin/wsx"), false)
            .unwrap();
        assert_eq!(w.args[0], "-c");
        // Value is `notify=[..]` whose array parses as TOML and names from-notify.
        let val = &w.args[1];
        assert!(val.starts_with("notify=["), "got: {val}");
        assert!(val.contains("/usr/local/bin/wsx"));
        assert!(val.contains("from-notify"));
        assert!(val.contains("--agent"));
        assert!(val.contains("codex"));
    }

    #[test]
    fn spawn_wiring_toml_escapes_bin_path() {
        // A path with a space and an embedded double-quote must stay valid TOML
        // inside the array. (`toml` is not a direct dependency, so assert the
        // escaped substring directly rather than re-parsing.)
        let w = CodexStatus
            .spawn_wiring(Path::new(r#"/o dd/"wsx"#), false)
            .unwrap();
        // Embedded double-quote is backslash-escaped inside the TOML string,
        // and the space-containing path is preserved verbatim within quotes.
        assert!(
            w.args[1].contains(r#""/o dd/\"wsx""#),
            "got: {}",
            w.args[1]
        );
    }
```

- [ ] **Step 6: Run the test to verify it fails**

Run: `cargo test --lib agent::status::codex::tests::spawn_wiring`
Expected: FAIL — `spawn_wiring` not yet overridden (the trait default returns `None`, so `.unwrap()` panics).

- [ ] **Step 7: Implement `spawn_wiring` + the TOML-quote helper**

In `src/agent/status/codex.rs`, add `use std::path::Path;` is already present. Add the `spawn_wiring` method inside `impl StatusIntegration for CodexStatus` (after `parse_event`), and a free helper below the impl:

```rust
    fn spawn_wiring(&self, wsx_bin: &Path, _fast_mode: bool) -> Option<SpawnWiring> {
        // Codex appends the JSON payload as the final argv element, so the
        // invoked command becomes:
        //   <wsx_bin> status from-notify --agent codex '<json>'
        let bin = wsx_bin.to_string_lossy();
        let array = [bin.as_ref(), "status", "from-notify", "--agent", "codex"]
            .iter()
            .map(|s| toml_quote(s))
            .collect::<Vec<_>>()
            .join(",");
        Some(SpawnWiring {
            args: vec!["-c".to_string(), format!("notify=[{array}]")],
        })
    }
```

Add at the end of the file (outside any `mod`):

```rust
/// Quote a string as a TOML basic string for embedding in a `-c notify=[...]`
/// override value. Escapes backslashes and double-quotes.
fn toml_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test --lib agent::status`
Expected: PASS.

- [ ] **Step 9: Lint + commit**

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: clean.

```bash
git add src/agent/status/codex.rs src/agent/status/mod.rs
git commit -m "feat(status): add Codex status integration (notify -> Done/Blocked)

CodexStatus parses Codex's notify agent-turn-complete payload into a
ReportedState (Blocked on a ?-terminated final message, else Done) and
emits spawn-time '-c notify=[...]' wiring pointing at wsx status
from-notify. Routed via for_agent(Codex); Pi/Hermes stay Noop."
```

---

### Task 2: `wsx status from-notify` CLI verb

The ingestion entry point. Unlike `from-hook` (reads stdin), Codex `notify` passes JSON as the final argv, so this verb reads its trailing positional argument.

**Files:**
- Modify: `src/cli.rs` (enum variant ~line 372, `parse_status` ~line 982, `run_cli` handler ~line 1488, parser test ~line 2459)

- [ ] **Step 1: Write the failing parser test**

In `src/cli.rs`, find the test that parses `from-hook` (around line 2436-2458). Add a new test immediately after it (inside the same `#[cfg(test)] mod`):

```rust
    #[test]
    fn parse_status_from_notify_captures_agent_and_payload() {
        match parse_args(
            ["wsx", "status", "from-notify", "--agent", "codex", "{\"type\":\"agent-turn-complete\"}"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        )
        .unwrap()
        {
            CliAction::StatusFromNotify { agent, payload } => {
                assert_eq!(agent.as_deref(), Some("codex"));
                assert_eq!(payload.as_deref(), Some("{\"type\":\"agent-turn-complete\"}"));
            }
            other => panic!("expected StatusFromNotify, got {other:?}"),
        }
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib cli::tests::parse_status_from_notify_captures_agent_and_payload`
Expected: FAIL to compile — `CliAction::StatusFromNotify` does not exist.

- [ ] **Step 3: Add the enum variant**

In `src/cli.rs`, after the `StatusFromHook { agent: Option<String> }` variant (ends ~line 376), add:

```rust
    StatusFromNotify {
        /// The harness whose `notify` payload is the trailing positional arg.
        /// `None` falls back to the resolved workspace's agent kind.
        agent: Option<String>,
        /// The raw JSON payload Codex passes as the final argv element.
        payload: Option<String>,
    },
```

- [ ] **Step 4: Add the `parse_status` arm**

In `parse_status` (`src/cli.rs`), after the `Some("from-hook") => { … }` arm (ends ~line 982), add:

```rust
        Some("from-notify") => {
            let mut agent = None;
            let mut payload = None;
            while let Some(arg) = it.next() {
                if arg == "--agent" {
                    agent = Some(it.next().ok_or_else(|| Error::Usage {
                        group: None,
                        msg: "--agent requires a value".into(),
                    })?);
                } else {
                    // Codex appends the JSON payload as the final positional arg.
                    payload = Some(arg);
                }
            }
            Ok(CliAction::StatusFromNotify { agent, payload })
        }
```

- [ ] **Step 5: Add the `run_cli` handler**

In `run_cli` (`src/cli.rs`), after the `CliAction::StatusFromHook { agent } => { … }` arm (ends ~line 1488), add:

```rust
        CliAction::StatusFromNotify { agent, payload } => {
            // Codex `notify` passes JSON as the final argv (not stdin). Tolerate
            // missing/garbage payloads by no-op exit 0 — notify must never fail
            // a turn.
            if let Some(buf) = payload {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&buf) {
                    if let Ok(ws) = resolve_current_workspace(&store) {
                        let kind = match &agent {
                            Some(a) => crate::pty::session::AgentKind::from_str_or_default(Some(a)),
                            None => ws.agent,
                        };
                        if let Some(state) = crate::agent::status::for_agent(kind).parse_event(&json)
                        {
                            let _ = store.set_workspace_status(ws.id, state, None, "notify");
                        }
                    }
                }
            }
            // Always succeed.
        }
```

Note: workspace resolution reuses `resolve_current_workspace` (prefers `WSX_WORKSPACE_ID`, which the `notify` subprocess inherits from Codex; falls back to the process cwd, which is the worktree). The payload's `cwd` field is therefore not needed.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --lib cli::tests::parse_status_from_notify_captures_agent_and_payload`
Expected: PASS.

Also run the full CLI test module to confirm no exhaustiveness breakage in `run_cli`/match arms: `cargo test --lib cli::`
Expected: PASS.

- [ ] **Step 7: Lint + commit**

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: clean.

```bash
git add src/cli.rs
git commit -m "feat(cli): add 'wsx status from-notify' for Codex notify payloads

Reads the trailing JSON argv (Codex notify passes the payload as the
final arg, not stdin), resolves the workspace, dispatches to the agent's
parse_event, and writes workspace_status with source=notify. Always
exits 0 so notify can never fail a turn."
```

---

### Task 3: Migrate Codex+`cat` test helpers to an arg-ignoring wrapper

Three tests spawn Codex with `WSX_CODEX_BIN=cat`, relying on "Codex Fresh injects no flags so `cat` stays alive." Task 4 adds `-c notify=…`, which bare `cat` rejects (`cat: invalid option -- 'c'`). Migrate them **first** to a wrapper that ignores args and execs `cat`, so they never go red. No production change here.

**Files:**
- Modify: `src/test_support.rs` (add `cat_ignore_args_path()`)
- Modify: `src/pty/session.rs:1595` (`spawn_and_echo`)
- Modify: `src/app/input_tests.rs:1345` (`spawn_pm_for_test`), `:1373` (`spawn_attached_workspace`)

- [ ] **Step 1: Add the wrapper helper to `test_support`**

In `src/test_support.rs`, after `false_path()` (line 46), add:

```rust
/// Path to an executable wrapper that ignores all CLI arguments and cats
/// stdin. Use in place of `cat_path()` for agent spawns that now inject flags
/// the bare `cat` would reject (e.g. Codex `-c notify=...`). The script is
/// (re)written on each call to a stable temp path; callers hold `ENV_LOCK` via
/// `EnvGuard`, so concurrent writers don't race on the identical content.
#[cfg(unix)]
pub fn cat_ignore_args_path() -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = std::env::temp_dir().join("wsx_test_cat_ignore_args.sh");
    std::fs::write(&p, "#!/bin/sh\nexec cat\n").expect("write wrapper script");
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755))
        .expect("chmod wrapper script");
    p
}
```

- [ ] **Step 2: Point `spawn_and_echo` at the wrapper**

In `src/pty/session.rs`, in the `spawn_and_echo` test, replace the binary-substitution line and its stale comment (lines ~1590-1595):

```rust
        // Substitute the agent binary with a wrapper that ignores args and cats
        // stdin. Codex Fresh now injects `-c notify=...` for status reporting,
        // which bare `cat` would reject, so we can't use `cat_path()` directly.
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", crate::test_support::cat_ignore_args_path());
```

(Leave the rest of the test unchanged.) Ensure `cat_path` is no longer needed by this test; if the `use` import becomes unused, drop `cat_path` from the import on the line `use crate::test_support::{EnvGuard, cat_path};` (line 1584) → `use crate::test_support::EnvGuard;`. Verify whether other tests in this module still use `cat_path` before removing it from the import.

- [ ] **Step 3: Point the two `input_tests.rs` helpers at the wrapper**

In `src/app/input_tests.rs`, in `spawn_pm_for_test` (line ~1342-1345) replace:

```rust
        // Use a wrapper that ignores args and cats stdin: Codex Fresh now
        // injects `-c notify=...` for status reporting, which bare `cat` rejects.
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", crate::test_support::cat_ignore_args_path());
```

And the identical block in `spawn_attached_workspace` (line ~1370-1373) with the same two-line replacement. Update the `cat_path` import in this file if it becomes unused (check the rest of the file first).

- [ ] **Step 4: Run the affected tests to verify they still pass (no injection yet)**

Run: `cargo test --lib pty::session::tests::spawn_and_echo` and `cargo test --lib app::input_tests`
Expected: PASS (the wrapper execs `cat`; behavior is unchanged because Codex still injects nothing until Task 4).

- [ ] **Step 5: Lint + commit**

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: clean.

```bash
git add src/test_support.rs src/pty/session.rs src/app/input_tests.rs
git commit -m "test: route Codex+cat spawn tests through an arg-ignoring wrapper

Prep for Codex notify injection: bare cat rejects the upcoming
'-c notify=...' flag, so substitute a wrapper that ignores args and
execs cat. No behavior change yet."
```

---

### Task 4: Inject `-c notify` wiring into the Codex spawn

Wire `CodexStatus::spawn_wiring` into `build_codex_command`, gated to developer sessions (Fresh/Continue), mirroring how `build_claude_command` injects `--settings` for the same modes (`src/pty/session.rs:867-876`). The PM pane gets no status wiring (matches Claude).

**Files:**
- Modify: `src/pty/session.rs` (`build_codex_command`, ~line 1547; add a unit test near the existing Codex command tests ~line 2261)

- [ ] **Step 1: Write the failing unit test**

In `src/pty/session.rs`, in the `tests` module (near the existing Codex `--sandbox` tests around line 2261), add:

```rust
    #[test]
    fn codex_fresh_injects_notify_status_wiring() {
        let cmd = build_codex_command(
            std::path::Path::new("/work"),
            &SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            },
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv: Vec<String> = cmd
            .get_argv()
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert!(argv.iter().any(|a| a == "-c"), "argv: {argv:?}");
        assert!(
            argv.iter()
                .any(|a| a.starts_with("notify=[") && a.contains("from-notify")),
            "argv: {argv:?}"
        );
    }

    #[test]
    fn codex_pm_omits_notify_status_wiring() {
        let cmd = build_codex_command(
            std::path::Path::new("/work"),
            &SpawnMode::ProjectManager {
                workspaces_json_path: std::path::PathBuf::from("/tmp/ws.json"),
                custom_instructions: None,
                additional_dirs: vec![],
                resume: false,
                fast_mode: false,
            },
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv: Vec<String> = cmd
            .get_argv()
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert!(
            !argv.iter().any(|a| a.starts_with("notify=[")),
            "PM should not get status wiring; argv: {argv:?}"
        );
    }
```

(`SpawnMode::ProjectManager` fields confirmed against `src/pty/session.rs`: `workspaces_json_path`, `custom_instructions`, `additional_dirs`, `resume`, `fast_mode`.)

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib pty::session::tests::codex_fresh_injects_notify_status_wiring`
Expected: FAIL — no `-c notify` arg is produced yet.

- [ ] **Step 3: Implement the injection**

In `build_codex_command` (`src/pty/session.rs`), immediately after the env-var copy loop (after line 1547, before `let (resume, yolo, pm) = …`), add:

```rust
    // Status reporting: developer sessions (Fresh/Continue) get `-c notify=...`
    // so Codex calls back into `wsx status from-notify` on agent-turn-complete.
    // The PM pane is excluded, matching the Claude spawn. `-c` is a global flag
    // and is accepted before any subcommand (`resume`).
    if matches!(mode, SpawnMode::Fresh { .. } | SpawnMode::Continue { .. }) {
        let wsx_bin =
            std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("wsx"));
        if let Some(wiring) =
            crate::agent::status::for_agent(AgentKind::Codex).spawn_wiring(&wsx_bin, false)
        {
            for arg in wiring.args {
                cmd.arg(arg);
            }
        }
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib pty::session::tests::codex_fresh_injects_notify_status_wiring pty::session::tests::codex_pm_omits_notify_status_wiring`
Expected: PASS.

Run the migrated integration tests to confirm they still pass with injection live:
Run: `cargo test --lib pty::session::tests::spawn_and_echo app::input_tests`
Expected: PASS (wrapper ignores the injected `-c notify=…`).

- [ ] **Step 5: Full suite + lint**

Run: `cargo test --all-targets --all-features`
Expected: PASS.
Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/pty/session.rs
git commit -m "feat(pty): inject '-c notify' status wiring into Codex spawns

Fresh/Continue Codex sessions now pass '-c notify=[wsx,status,
from-notify,--agent,codex]' via the StatusIntegration spawn_wiring,
mirroring the Claude --settings injection. PM pane excluded."
```

---

### Task 5: Live verification in the interactive TUI (manual)

`notify` firing was proven under `codex exec`; the source confirms the same dispatch path in the interactive TUI, but this is the one assumption to validate live before declaring done. Not a unit test.

**Files:** none (manual verification).

- [ ] **Step 1: Build and create a Codex workspace**

Run: `cargo build` then use wsx to spawn a Codex workspace (e.g. `wsx workspace create <repo> --agent codex --name notify-smoke`, or via the dashboard selecting the Codex agent). Open/attach it.

- [ ] **Step 2: Confirm the wiring is present**

Before interacting, verify the spawned process carries the flag. In the workspace, run:

Run: `ps -ef | grep -- 'from-notify' | grep -v grep`
Expected: the `codex` process line includes `-c notify=["…/wsx","status","from-notify","--agent","codex"]`.

- [ ] **Step 3: Drive a turn and confirm the status flip**

Give the Codex agent a trivial prompt (e.g. "reply with the word ready"). When the turn completes, confirm the dashboard shows the workspace as **Complete/Done** (the `✓` state), driven by the push (not just heuristic). Cross-check the store:

Run: `sqlite3 ~/.local/state/wsx/state.db "SELECT workspace_id, state, source FROM workspace_status ORDER BY reported_at DESC LIMIT 5;"`
Expected: a row for the workspace with `state=done` (or `blocked` if the reply ended in `?`) and `source=notify`.

- [ ] **Step 4: Confirm the heuristic re-arms**

Send a second prompt. While Codex works, confirm the dashboard moves off Done back to a working/thinking state (the `fresh_reported` gate yielding to the JSONL heuristic once new activity post-dates the push), then back to Done on completion.

- [ ] **Step 5: Record the result**

If all pass, note it in the design spec's caveats (the "live TUI check" item). If `notify` does **not** fire in the TUI (contradicting the source analysis), stop and revisit — the fallback is to keep Codex on the heuristic (revert Task 4's injection) while retaining the harmless `from-notify` plumbing.

---

## Self-Review

**Spec coverage:**
- `CodexStatus::parse_event` (Done/Blocked from `agent-turn-complete`) → Task 1. ✅
- `spawn_wiring` emitting `-c notify=[...]` → Task 1 (Steps 5-8). ✅
- Route `AgentKind::Codex` in `for_agent` → Task 1 (Step 3). ✅
- `wsx status from-notify` argv-JSON sink, `source="notify"`, exit-0 → Task 2. ✅
- Append `spawn_wiring()` args to the Codex launch, Fresh/Continue only → Task 4. ✅
- Unit tests (parse_event ±`?`, wrong type, spawn_wiring escaping, from-notify argv, build_codex_command presence/absence) → Tasks 1, 2, 4. ✅
- Live TUI verification → Task 5. ✅
- No `workspace_status` schema / `classify()` / freshness-gate / heuristic changes → confirmed; none of the tasks touch them. ✅
- Caveat: `-c notify` overrides user notify for wsx sessions only → inherent to injecting via `-c` per-spawn; documented in spec. ✅

**Placeholder scan:** No TBD/TODO; every code step shows complete code. The two "confirm the exact fields/enum" notes (Task 4 Step 1 `SpawnMode::ProjectManager` fields; Task 2 import cleanups) are explicit verification instructions with the fallback spelled out, not placeholders.

**Type consistency:** `CodexStatus` (struct), `parse_event`/`spawn_wiring` (trait methods matching `StatusIntegration` in mod.rs), `SpawnWiring { args }`, `ReportedState::{Done,Blocked}`, `set_workspace_status(id, state, message, source)`, `CliAction::StatusFromNotify { agent, payload }`, `for_agent(AgentKind::Codex)`, `cat_ignore_args_path()`, `build_codex_command(cwd, mode, remote)` — all consistent across tasks and matched against the actual source read during planning.

**Scope:** Single subsystem (Codex status integration); one cohesive plan.
