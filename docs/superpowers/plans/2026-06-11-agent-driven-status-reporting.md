# Agent-Driven Status Reporting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the running agent push its own status (working / waiting / blocked / done, plus an optional message) to wsx via a CLI verb and deterministic Claude Code hooks, feeding a freshness-gated input into the existing status classifier — with the JSONL heuristic preserved as the fallback tier.

**Architecture:** Three tiers write one store row per workspace: (1) model push `wsx status set ...`, (2) Claude hooks calling `wsx status from-hook` (decision logic centralized in Rust), (3) the existing `Status::classify()` JSONL heuristic as fallback. The TUI loads pushed rows in its existing external-change refresh path and passes a freshness-gated `ReportedState` into `Status::classify()`. The pushed state slots into the classifier's priority ladder so the existing stall detector still self-heals a stuck "working".

**Tech Stack:** Rust (edition 2024), rusqlite (SQLite, WAL), serde_json, ratatui TUI. Tests are `cargo test`; lint is `cargo clippy`.

**Design refinements vs. the spec** (`docs/superpowers/specs/2026-06-11-agent-driven-status-design.md`), both reducing blast radius:
- Storage is a **dedicated `workspace_status` table** keyed by `workspace_id`, not new columns on `workspaces`. This leaves the `Workspace` struct and its three SELECT sites untouched and matches the frequently-rewritten-from-a-separate-process access pattern.
- Hooks all call **`wsx status from-hook`** which reads the hook JSON on stdin and decides the state in Rust (`state_from_hook`), instead of many matcher-specific `set <fixed-state>` entries. The decision logic becomes a pure, unit-tested function rather than fragile matcher strings.

**Known v1 limitation (tracked, not fixed here):** writes are last-write-wins by `workspace_id`. A model `blocked` push immediately followed by a `Stop` hook could clobber `blocked`→`done`. Spec spike-validation item #5 (PreToolUse-vs-Stop ordering) must be confirmed interactively; if it bites, make `blocked` sticky in a follow-up. Documented in Task 5.

---

## File Structure

| File | Responsibility | Change |
|------|----------------|--------|
| `src/data/store.rs` | `ReportedState`/`ReportedStatus` types, V15 migration, status read/write, `busy_timeout` | Modify |
| `src/agent/hooks.rs` | Build the Claude `--settings` hooks JSON; `state_from_hook()` decision fn | **Create** |
| `src/agent/mod.rs` | Register the new `hooks` module | Modify |
| `src/cli.rs` | `status` command group: parse + dispatch `set`/`clear`/`from-hook` | Modify |
| `src/ui/dashboard/status.rs` | Thread `Option<ReportedState>` into `Status::classify()` | Modify |
| `src/app.rs` | `pushed_status` map, load in `refresh()`, freshness gate in `classify_status()` | Modify |
| `src/pty/session.rs` | Inject hooks settings JSON in `build_claude_command` | Modify |
| `src/agent/doctrine.rs` | Add a status-reporting doctrine clause | Modify |
| `skills/wsx/SKILL.md` | Add "Reporting your status" section | Modify |

---

## Task 1: Store — `ReportedState` types + V15 `workspace_status` table

**Files:**
- Modify: `src/data/store.rs` (types near line 33; `migrate()` after line 286; methods near line 663; schema consts near line 831)
- Test: `src/data/store.rs` `#[cfg(test)] mod tests`

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/data/store.rs`:

```rust
#[test]
fn workspace_status_round_trips() {
    let store = Store::open_in_memory().unwrap();
    let repo = store.add_repo(std::path::Path::new("/tmp/r"), "r", "r/").unwrap();
    let ws = store
        .insert_workspace(&NewWorkspace {
            repo_id: repo,
            name: "w",
            branch: "r/w",
            worktree_path: std::path::Path::new("/tmp/r/w"),
            yolo: false,
            agent: AgentKind::Claude,
        })
        .unwrap(); // returns WorkspaceId

    assert!(store.workspace_status(ws).unwrap().is_none());

    store
        .set_workspace_status(ws, ReportedState::Blocked, Some("need a decision"), "model")
        .unwrap();
    let got = store.workspace_status(ws).unwrap().unwrap();
    assert_eq!(got.state, ReportedState::Blocked);
    assert_eq!(got.message.as_deref(), Some("need a decision"));
    assert_eq!(got.source, "model");
    assert!(got.reported_at > 0);

    // INSERT OR REPLACE: second write wins, keyed by workspace.
    store
        .set_workspace_status(ws, ReportedState::Done, None, "hook")
        .unwrap();
    let got = store.workspace_status(ws).unwrap().unwrap();
    assert_eq!(got.state, ReportedState::Done);
    assert_eq!(got.message, None);
    assert_eq!(got.source, "hook");

    store.clear_workspace_status(ws).unwrap();
    assert!(store.workspace_status(ws).unwrap().is_none());
}

#[test]
fn all_workspace_status_returns_map() {
    let store = Store::open_in_memory().unwrap();
    let repo = store.add_repo(std::path::Path::new("/tmp/r"), "r", "r/").unwrap();
    let ws = store
        .insert_workspace(&NewWorkspace {
            repo_id: repo,
            name: "w",
            branch: "r/w",
            worktree_path: std::path::Path::new("/tmp/r/w"),
            yolo: false,
            agent: AgentKind::Claude,
        })
        .unwrap();
    store
        .set_workspace_status(ws, ReportedState::Working, None, "model")
        .unwrap();
    let map = store.all_workspace_status().unwrap();
    assert_eq!(map.get(&ws).map(|s| s.state), Some(ReportedState::Working));
}

#[test]
fn reported_state_parse_round_trips() {
    for st in [
        ReportedState::Working,
        ReportedState::Waiting,
        ReportedState::Blocked,
        ReportedState::Done,
    ] {
        assert_eq!(ReportedState::parse(st.as_str()), Some(st));
    }
    assert_eq!(ReportedState::parse("nonsense"), None);
}
```

> The workspace-creation method is `store.insert_workspace(&NewWorkspace { .. })` (src/data/store.rs:574), which returns a `WorkspaceId` — that's why `ws` is passed directly to the status methods. `NewWorkspace`, `WorkspaceId`, and `AgentKind` are already in scope in the store tests module.

- [ ] **Step 2: Run tests to verify they fail to compile**

Run: `cargo test --lib data::store::tests::workspace_status_round_trips`
Expected: FAIL — `ReportedState` / `set_workspace_status` not found.

- [ ] **Step 3: Add the types**

Insert after the `SetupStatus` enum (after line 33) in `src/data/store.rs`:

```rust
/// The agent-facing status vocabulary. Distinct from the six *display*
/// `Status` states: an agent never reports itself idle or stalled — those
/// stay wsx-inferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportedState {
    Working,
    Waiting,
    Blocked,
    Done,
}

impl ReportedState {
    pub fn as_str(self) -> &'static str {
        match self {
            ReportedState::Working => "working",
            ReportedState::Waiting => "waiting",
            ReportedState::Blocked => "blocked",
            ReportedState::Done => "done",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "working" => Some(ReportedState::Working),
            "waiting" => Some(ReportedState::Waiting),
            "blocked" => Some(ReportedState::Blocked),
            "done" => Some(ReportedState::Done),
            _ => None,
        }
    }
}

/// A row from the `workspace_status` table: the last status an agent pushed.
#[derive(Debug, Clone)]
pub struct ReportedStatus {
    pub state: ReportedState,
    pub message: Option<String>,
    pub source: String,
    pub reported_at: i64,
}
```

- [ ] **Step 4: Add the schema constant**

Add near the other `SCHEMA_V*` consts (around line 831) in `src/data/store.rs`:

```rust
const SCHEMA_V15_WORKSPACE_STATUS: &str = "
CREATE TABLE IF NOT EXISTS workspace_status (
    workspace_id INTEGER PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
    state        TEXT NOT NULL,
    message      TEXT,
    source       TEXT NOT NULL,
    reported_at  INTEGER NOT NULL
);
";
```

- [ ] **Step 5: Add the migration block**

In `migrate()`, immediately before the closing `Ok(())` (after the `v < 14` block, line 286), add:

```rust
        if v < 15 {
            self.conn.execute_batch(SCHEMA_V15_WORKSPACE_STATUS)?;
            self.conn.execute("PRAGMA user_version = 15", [])?;
        }
```

> The table uses `CREATE TABLE IF NOT EXISTS`, so it is safe under the "migrate re-runs every startup" gotcha without a `pragma_table_info` guard (there is no `ALTER`/backfill to repeat).

- [ ] **Step 6: Add the read/write methods**

Add as methods on `impl Store` (e.g. after `set_setup_status`, around line 663):

```rust
    pub fn set_workspace_status(
        &self,
        id: WorkspaceId,
        state: ReportedState,
        message: Option<&str>,
        source: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO workspace_status \
                 (workspace_id, state, message, source, reported_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![id.0, state.as_str(), message, source, now_ms()],
        )?;
        Ok(())
    }

    pub fn clear_workspace_status(&self, id: WorkspaceId) -> Result<()> {
        self.conn
            .execute("DELETE FROM workspace_status WHERE workspace_id = ?1", [id.0])?;
        Ok(())
    }

    pub fn workspace_status(&self, id: WorkspaceId) -> Result<Option<ReportedStatus>> {
        let r = self
            .conn
            .query_row(
                "SELECT state, message, source, reported_at \
                 FROM workspace_status WHERE workspace_id = ?1",
                [id.0],
                row_to_reported_status,
            )
            .optional()?;
        Ok(r)
    }

    pub fn all_workspace_status(
        &self,
    ) -> Result<std::collections::HashMap<WorkspaceId, ReportedStatus>> {
        let mut stmt = self.conn.prepare(
            "SELECT workspace_id, state, message, source, reported_at FROM workspace_status",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((WorkspaceId(r.get(0)?), row_to_reported_status_offset1(r)?))
        })?;
        let mut map = std::collections::HashMap::new();
        for row in rows {
            let (id, status) = row?;
            map.insert(id, status);
        }
        Ok(map)
    }
```

And add the two free functions next to `row_to_workspace` (around line 923):

```rust
fn row_to_reported_status(r: &rusqlite::Row) -> rusqlite::Result<ReportedStatus> {
    Ok(ReportedStatus {
        state: ReportedState::parse(&r.get::<_, String>(0)?)
            .unwrap_or(ReportedState::Working),
        message: r.get(1)?,
        source: r.get(2)?,
        reported_at: r.get(3)?,
    })
}

// Same as `row_to_reported_status` but for queries that select the
// workspace_id in column 0, shifting the status columns to 1..=4.
fn row_to_reported_status_offset1(r: &rusqlite::Row) -> rusqlite::Result<ReportedStatus> {
    Ok(ReportedStatus {
        state: ReportedState::parse(&r.get::<_, String>(1)?)
            .unwrap_or(ReportedState::Working),
        message: r.get(2)?,
        source: r.get(3)?,
        reported_at: r.get(4)?,
    })
}
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test --lib data::store::tests::workspace_status_round_trips data::store::tests::all_workspace_status_returns_map data::store::tests::reported_state_parse_round_trips`
Expected: PASS (all three).

- [ ] **Step 8: Commit**

```bash
git add src/data/store.rs
git commit -m "feat(store): workspace_status table + ReportedState (schema v15)"
```

---

## Task 2: Store — set `busy_timeout` on the file-backed connection

Concurrent writers: the `wsx status` CLI process writes while the TUI may also write. WAL allows one writer; with no `busy_timeout` a contended write fails immediately with `SQLITE_BUSY`. Set a timeout so writers wait briefly instead.

**Files:**
- Modify: `src/data/store.rs` `open()` (line 89-99)
- Test: `src/data/store.rs` `tests`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn open_sets_busy_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("state.db")).unwrap();
    let ms: i64 = store
        .conn()
        .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
        .unwrap();
    assert!(ms >= 3000, "expected busy_timeout >= 3000ms, got {ms}");
}
```

> `tempfile` is already a dev-dependency if other tests use `tempdir()`. Verify with `grep -n "tempfile" Cargo.toml`; if absent, add `tempfile = "3"` under `[dev-dependencies]`. `conn()` is `pub(crate)` (store.rs:822) so the in-module test can call it.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib data::store::tests::open_sets_busy_timeout`
Expected: FAIL — busy_timeout is 0 by default.

- [ ] **Step 3: Add the pragma**

In `open()`, after the `foreign_keys` pragma (line 95), add:

```rust
        // Writers (the TUI and a sibling `wsx status` CLI process) contend for
        // the single WAL writer slot; wait up to 3s rather than erroring out
        // immediately with SQLITE_BUSY.
        conn.pragma_update(None, "busy_timeout", 3000)?;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib data::store::tests::open_sets_busy_timeout`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/data/store.rs Cargo.toml
git commit -m "feat(store): set busy_timeout so concurrent CLI writes don't fail"
```

---

## Task 3: Hooks module — `state_from_hook()` + settings JSON builder

**Files:**
- Create: `src/agent/hooks.rs`
- Modify: `src/agent/mod.rs` (add `pub mod hooks;`)
- Test: `src/agent/hooks.rs` `#[cfg(test)] mod tests`

- [ ] **Step 1: Create the module with failing tests**

Create `src/agent/hooks.rs`:

```rust
//! Claude Code hook wiring for agent status reporting.
//!
//! wsx injects a `hooks` block into the Claude `--settings` JSON. Every hook
//! calls `wsx status from-hook`, which reads the hook payload on stdin and
//! maps it to a `ReportedState` here (`state_from_hook`) — so the mapping is
//! pure Rust we can unit-test, not matcher strings spread across config.

use crate::data::store::ReportedState;
use std::path::Path;

/// Decide the status a hook payload implies, or `None` when the event is not
/// status-relevant (the CLI then writes nothing and exits 0).
///
/// Mapping (see the design spec's Fidelity findings):
/// - `UserPromptSubmit`                       -> Working
/// - `PreToolUse` for AskUserQuestion/ExitPlanMode -> Blocked
/// - `Notification` permission_prompt          -> Blocked
/// - `Notification` idle_prompt                -> Waiting
/// - `Stop` with a `?`-terminated last message -> Blocked (best-effort)
/// - `Stop` otherwise                          -> Done
pub fn state_from_hook(json: &serde_json::Value) -> Option<ReportedState> {
    let event = json.get("hook_event_name")?.as_str()?;
    match event {
        "UserPromptSubmit" => Some(ReportedState::Working),
        "PreToolUse" => {
            let tool = json.get("tool_name").and_then(|v| v.as_str()).unwrap_or("");
            if matches!(tool, "AskUserQuestion" | "ExitPlanMode") {
                Some(ReportedState::Blocked)
            } else {
                None
            }
        }
        "Notification" => match json.get("notification_type").and_then(|v| v.as_str()) {
            Some("permission_prompt") => Some(ReportedState::Blocked),
            Some("idle_prompt") => Some(ReportedState::Waiting),
            _ => None,
        },
        "Stop" => {
            // `last_assistant_message` is observed but undocumented; degrade to
            // Done when absent. A trailing `?` is the best-effort prose-question
            // signal, otherwise the turn read as a completion.
            let ends_with_q = json
                .get("last_assistant_message")
                .and_then(|v| v.as_str())
                .map(|s| s.trim_end().ends_with('?'))
                .unwrap_or(false);
            Some(if ends_with_q {
                ReportedState::Blocked
            } else {
                ReportedState::Done
            })
        }
        _ => None,
    }
}

/// Build the `--settings` JSON string for a Claude spawn. Always includes the
/// status hooks; includes `"fastMode": true` only when `fast_mode` is set.
/// `wsx_bin` is the absolute path to the running wsx binary so hooks invoke the
/// same build regardless of PATH.
pub fn claude_settings_json(fast_mode: bool, wsx_bin: &Path) -> String {
    let cmd = format!("{} status from-hook", shell_quote(wsx_bin));
    let one = |ev: &str| {
        serde_json::json!([{ "hooks": [{ "type": "command", "command": cmd }] }])
            .as_array()
            .cloned()
            .map(|a| (ev.to_string(), serde_json::Value::Array(a)))
            .unwrap()
    };
    let hooks: serde_json::Map<String, serde_json::Value> = ["UserPromptSubmit", "PreToolUse", "Notification", "Stop"]
        .into_iter()
        .map(one)
        .collect();

    let mut root = serde_json::Map::new();
    if fast_mode {
        root.insert("fastMode".into(), serde_json::Value::Bool(true));
    }
    root.insert("hooks".into(), serde_json::Value::Object(hooks));
    serde_json::Value::Object(root).to_string()
}

/// Minimal POSIX single-quote escaping for a path embedded in a hook command.
fn shell_quote(p: &Path) -> String {
    let s = p.to_string_lossy();
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(json: serde_json::Value) -> Option<ReportedState> {
        state_from_hook(&json)
    }

    #[test]
    fn user_prompt_submit_is_working() {
        assert_eq!(
            ev(serde_json::json!({"hook_event_name": "UserPromptSubmit"})),
            Some(ReportedState::Working)
        );
    }

    #[test]
    fn pretooluse_question_tools_are_blocked() {
        for tool in ["AskUserQuestion", "ExitPlanMode"] {
            assert_eq!(
                ev(serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": tool})),
                Some(ReportedState::Blocked)
            );
        }
        assert_eq!(
            ev(serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": "Bash"})),
            None
        );
    }

    #[test]
    fn notification_types_map_or_ignore() {
        assert_eq!(
            ev(serde_json::json!({"hook_event_name": "Notification", "notification_type": "permission_prompt"})),
            Some(ReportedState::Blocked)
        );
        assert_eq!(
            ev(serde_json::json!({"hook_event_name": "Notification", "notification_type": "idle_prompt"})),
            Some(ReportedState::Waiting)
        );
        assert_eq!(
            ev(serde_json::json!({"hook_event_name": "Notification", "notification_type": "auth_success"})),
            None
        );
    }

    #[test]
    fn stop_distinguishes_question_from_completion() {
        assert_eq!(
            ev(serde_json::json!({"hook_event_name": "Stop", "last_assistant_message": "All done."})),
            Some(ReportedState::Done)
        );
        assert_eq!(
            ev(serde_json::json!({"hook_event_name": "Stop", "last_assistant_message": "Which option do you prefer?"})),
            Some(ReportedState::Blocked)
        );
        // Undocumented field absent -> degrade to Done, never panic.
        assert_eq!(
            ev(serde_json::json!({"hook_event_name": "Stop"})),
            Some(ReportedState::Done)
        );
    }

    #[test]
    fn unknown_event_is_ignored() {
        assert_eq!(ev(serde_json::json!({"hook_event_name": "SubagentStop"})), None);
        assert_eq!(ev(serde_json::json!({})), None);
    }

    #[test]
    fn settings_json_is_valid_and_contains_hooks_and_bin() {
        let json = claude_settings_json(true, Path::new("/usr/local/bin/wsx"));
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["fastMode"], serde_json::json!(true));
        assert!(v["hooks"]["Stop"].is_array());
        assert!(v["hooks"]["UserPromptSubmit"].is_array());
        let cmd = v["hooks"]["Stop"][0]["hooks"][0]["command"].as_str().unwrap();
        assert!(cmd.contains("/usr/local/bin/wsx"));
        assert!(cmd.ends_with("status from-hook"));
    }

    #[test]
    fn settings_json_omits_fastmode_when_false() {
        let json = claude_settings_json(false, Path::new("/usr/local/bin/wsx"));
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v.get("fastMode").is_none());
        assert!(v["hooks"]["Notification"].is_array());
    }
}
```

- [ ] **Step 2: Register the module**

In `src/agent/mod.rs`, add alongside the other `pub mod` declarations:

```rust
pub mod hooks;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test --lib agent::hooks::tests`
Expected: PASS (all seven tests).

- [ ] **Step 4: Commit**

```bash
git add src/agent/hooks.rs src/agent/mod.rs
git commit -m "feat(agent): hooks module — state_from_hook + settings JSON builder"
```

---

## Task 4: Classifier — thread `Option<ReportedState>` into `Status::classify()`

**Files:**
- Modify: `src/ui/dashboard/status.rs` (`classify`, line 65-121)
- Test: `src/ui/dashboard/status.rs` `tests`

The pushed state is placed in the priority ladder so that `blocked`/`done` override the JSONL `stopped_kind` heuristic, but a pushed `working` does **not** suppress the stall detector (a stuck "working" still surfaces as `Stalled`).

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/ui/dashboard/status.rs`:

```rust
use crate::data::store::ReportedState;

// Helper mirroring the new signature with sensible "live session, just active"
// defaults so each test only sets what it exercises.
fn classify_reported(reported: Option<ReportedState>, stalled: bool) -> Status {
    Status::classify(
        false,            // awaiting_tool
        None,             // stopped_kind
        stalled,          // stalled
        s(5),             // seconds_since_activity (live)
        true,             // session_running
        true,             // user_has_prompted
        false,            // has_prior_session
        reported,         // NEW: reported state
    )
}

#[test]
fn reported_blocked_maps_to_question() {
    assert_eq!(
        classify_reported(Some(ReportedState::Blocked), false),
        Status::Question
    );
}

#[test]
fn reported_done_maps_to_complete() {
    assert_eq!(
        classify_reported(Some(ReportedState::Done), false),
        Status::Complete
    );
}

#[test]
fn reported_working_does_not_override_stall() {
    // Pushed "working" but the JSONL has been quiet 60s and the PTY isn't
    // streaming (secs >= 2): the stall detector must still win.
    let st = Status::classify(
        false, None, /*stalled*/ true, s(120), true, true, false,
        Some(ReportedState::Working),
    );
    assert_eq!(st, Status::Stalled);
}

#[test]
fn reported_working_maps_to_thinking_when_not_stalled() {
    assert_eq!(
        classify_reported(Some(ReportedState::Working), false),
        Status::Thinking
    );
}

#[test]
fn reported_waiting_maps_to_waiting() {
    assert_eq!(
        classify_reported(Some(ReportedState::Waiting), false),
        Status::Waiting
    );
}

#[test]
fn no_reported_state_falls_back_to_heuristic() {
    // Unchanged behaviour: live session, recent activity -> Thinking.
    assert_eq!(classify_reported(None, false), Status::Thinking);
}
```

You must also update **every existing call to `Status::classify(...)`** in this file's existing tests to pass a trailing `None`. Find them with `grep -n "Status::classify(" src/ui/dashboard/status.rs` and append `, None` before the closing paren.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib ui::dashboard::status`
Expected: FAIL — arity mismatch / `reported` unused.

- [ ] **Step 3: Update the signature and body**

In `src/ui/dashboard/status.rs`, change the `classify` signature to add the parameter:

```rust
    pub fn classify(
        awaiting_tool: bool,
        stopped_kind: Option<StoppedKind>,
        stalled: bool,
        seconds_since_activity: Option<u64>,
        session_running: bool,
        user_has_prompted: bool,
        has_prior_session: bool,
        reported: Option<crate::data::store::ReportedState>,
    ) -> Self {
```

Then weave `reported` into the body. Replace the existing body from the `pty_active` line down to the end with:

```rust
        use crate::data::store::ReportedState;
        let pty_active = matches!(seconds_since_activity, Some(s) if s < 2);

        // A genuine permission prompt is ground truth — keep it first.
        if awaiting_tool && !pty_active {
            return Status::Question;
        }

        // Tier 1/2 push: the agent (or a hook) explicitly told us it is
        // blocked or done. These are terminal user-facing states; trust them
        // over the JSONL stopped_kind heuristic.
        match reported {
            Some(ReportedState::Blocked) => return Status::Question,
            Some(ReportedState::Done) => return Status::Complete,
            _ => {}
        }

        match stopped_kind {
            Some(StoppedKind::AwaitingAnswer) => return Status::Question,
            Some(StoppedKind::Complete) => return Status::Complete,
            None => {}
        }

        // Stall detector runs BEFORE the pushed live-states below, so a stuck
        // "working" push (JSONL quiet 60s, PTY idle) still self-heals to
        // Stalled — the key reason the push is folded into classify() rather
        // than short-circuiting it.
        if stalled && !pty_active {
            return Status::Stalled;
        }

        // Tier 1/2 push: live states. Below the stall guard by design.
        match reported {
            Some(ReportedState::Working) => return Status::Thinking,
            Some(ReportedState::Waiting) => return Status::Waiting,
            _ => {}
        }

        if session_running {
            if !user_has_prompted {
                return Status::Idle;
            }
            match seconds_since_activity {
                Some(s) if s < 30 => Status::Thinking,
                Some(_) => Status::Waiting,
                None => Status::Thinking,
            }
        } else {
            let _ = has_prior_session;
            Status::Idle
        }
```

> Preserve the existing explanatory comments where they still apply; the snippet condenses them for brevity but keep the originals' intent.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib ui::dashboard::status`
Expected: PASS (new and pre-existing tests).

- [ ] **Step 5: Commit**

```bash
git add src/ui/dashboard/status.rs
git commit -m "feat(status): fold agent-reported state into Status::classify"
```

---

## Task 5: App — load pushed status and apply a freshness gate

The pushed row is authoritative only while no JSONL activity has occurred **after** it (liveness gate, reusing `WorkspaceEvents::last_log_activity_ms`). If the agent did something post-push, the heuristic re-arms.

**Files:**
- Modify: `src/app.rs` (`App` struct ~line 167; `refresh()` line 397; `classify_status()` line 527; add a free fn + tests)
- Test: `src/app.rs` `#[cfg(test)]`

- [ ] **Step 1: Write the failing unit test for the freshness gate**

Add a test module at the bottom of `src/app.rs` (or extend an existing one):

```rust
#[cfg(test)]
mod reported_freshness_tests {
    use super::fresh_reported_state;
    use crate::data::store::{ReportedState, ReportedStatus};

    fn status(at: i64) -> ReportedStatus {
        ReportedStatus {
            state: ReportedState::Done,
            message: None,
            source: "model".into(),
            reported_at: at,
        }
    }

    #[test]
    fn push_newer_than_last_log_activity_is_fresh() {
        // reported_at >= last_log_activity_ms -> still authoritative.
        assert_eq!(
            fresh_reported_state(Some(&status(1000)), 900),
            Some(ReportedState::Done)
        );
        assert_eq!(
            fresh_reported_state(Some(&status(1000)), 1000),
            Some(ReportedState::Done)
        );
    }

    #[test]
    fn jsonl_activity_after_push_re_arms_heuristic() {
        // Agent did something after reporting -> drop the push.
        assert_eq!(fresh_reported_state(Some(&status(1000)), 1500), None);
    }

    #[test]
    fn no_push_is_none() {
        assert_eq!(fresh_reported_state(None, 1500), None);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib reported_freshness_tests`
Expected: FAIL — `fresh_reported_state` not found.

- [ ] **Step 3: Add the freshness helper**

Add as a free function in `src/app.rs` (near `derive_stopped_kind`, line 615):

```rust
/// Decide whether a pushed status is still authoritative. The push wins while
/// no JSONL activity has happened strictly after it; once the log grows past
/// `reported_at`, the agent has acted since reporting and the heuristic
/// re-arms. `last_log_activity_ms` of 0 means "no log activity observed",
/// which never contradicts a push.
pub(crate) fn fresh_reported_state(
    reported: Option<&crate::data::store::ReportedStatus>,
    last_log_activity_ms: i64,
) -> Option<crate::data::store::ReportedState> {
    let r = reported?;
    if r.reported_at >= last_log_activity_ms {
        Some(r.state)
    } else {
        None
    }
}
```

- [ ] **Step 4: Add the `pushed_status` field**

In the `App` struct (near the other workspace maps, ~line 167), add:

```rust
    /// Last agent-pushed status per workspace, loaded from the store in
    /// `refresh()` (which fires on every external-change tick — a sibling
    /// `wsx status` write bumps `data_version`).
    pub pushed_status: std::collections::HashMap<
        crate::data::store::WorkspaceId,
        crate::data::store::ReportedStatus,
    >,
```

Initialize it in `App::new` (near line 319 where `workspace_events` is initialized):

```rust
            pushed_status: std::collections::HashMap::new(),
```

- [ ] **Step 5: Load it in `refresh()`**

In `refresh()` (line 397), after the workspaces loop / before `Ok(())`, add:

```rust
        self.pushed_status = self.store.all_workspace_status().unwrap_or_default();
```

- [ ] **Step 6: Use it in `classify_status()`**

In `classify_status()` (line 527), compute the gated state and pass it to `classify`. Just before the final `Status::classify(...)` call (line 581), add:

```rust
        let last_log_activity = self
            .workspace_events
            .get(&ws.id)
            .map(|e| e.last_log_activity_ms)
            .unwrap_or(0);
        let reported = fresh_reported_state(self.pushed_status.get(&ws.id), last_log_activity);
```

Then change the `Status::classify(...)` call to pass `reported` as the new trailing argument:

```rust
        crate::ui::dashboard::status::Status::classify(
            awaiting,
            stopped_kind,
            stalled,
            secs,
            running,
            user_has_prompted,
            has_prior,
            reported,
        )
```

> Confirm the field name on `WorkspaceEvents` is `last_log_activity_ms` (grep in `~/sessionx/src/activity/events/mod.rs` and/or `src/app/background.rs`). If it differs, use the real accessor; the freshness helper only needs an `i64` epoch-ms of last JSONL growth.

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test --lib reported_freshness_tests && cargo build`
Expected: tests PASS; build OK.

- [ ] **Step 8: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): load pushed status, liveness-gate it into classify_status"
```

---

## Task 6: CLI — `wsx status set | clear | from-hook`

**Files:**
- Modify: `src/cli.rs` (`CliAction` enum ~line 244; `parse_args` group match line 444; `group_name`; `run_cli` line 955; per-group help)
- Test: `src/cli.rs` parse tests

- [ ] **Step 1: Write the failing parse tests**

Add to the cli tests module in `src/cli.rs`:

```rust
#[test]
fn parses_status_set_with_message() {
    let a = parse_args(
        ["wsx", "status", "set", "blocked", "--message", "need a decision"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    )
    .unwrap();
    match a {
        CliAction::StatusSet { state, message } => {
            assert_eq!(state, "blocked");
            assert_eq!(message.as_deref(), Some("need a decision"));
        }
        other => panic!("expected StatusSet, got {other:?}"),
    }
}

#[test]
fn parses_status_set_without_message() {
    let a = parse_args(
        ["wsx", "status", "set", "working"].iter().map(|s| s.to_string()).collect(),
    )
    .unwrap();
    match a {
        CliAction::StatusSet { state, message } => {
            assert_eq!(state, "working");
            assert_eq!(message, None);
        }
        other => panic!("expected StatusSet, got {other:?}"),
    }
}

#[test]
fn parses_status_clear_and_from_hook() {
    assert!(matches!(
        parse_args(["wsx", "status", "clear"].iter().map(|s| s.to_string()).collect()).unwrap(),
        CliAction::StatusClear
    ));
    assert!(matches!(
        parse_args(["wsx", "status", "from-hook"].iter().map(|s| s.to_string()).collect()).unwrap(),
        CliAction::StatusFromHook
    ));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib cli::`
Expected: FAIL — `StatusSet` etc. not found.

- [ ] **Step 3: Add the `CliAction` variants**

In the `CliAction` enum (around line 244-349), add:

```rust
    StatusSet {
        state: String,
        message: Option<String>,
    },
    StatusClear,
    StatusFromHook,
```

- [ ] **Step 4: Add the parser**

Add a `parse_status` function near `parse_workspace`/`parse_agent`:

```rust
fn parse_status(it: &mut impl Iterator<Item = String>) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("set") => {
            let state = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "usage: wsx status set <working|waiting|blocked|done> [--message <text>]".into(),
            })?;
            let mut message = None;
            while let Some(arg) = it.next() {
                if arg == "--message" || arg == "-m" {
                    message = it.next();
                } else {
                    return Err(Error::Usage {
                        group: None,
                        msg: format!("unexpected argument: {arg}"),
                    });
                }
            }
            Ok(CliAction::StatusSet { state, message })
        }
        Some("clear") => Ok(CliAction::StatusClear),
        Some("from-hook") => Ok(CliAction::StatusFromHook),
        other => Err(Error::Usage {
            group: None,
            msg: format!(
                "unknown status subcommand: {}",
                other.unwrap_or("(none)")
            ),
        }),
    }
}
```

- [ ] **Step 5: Wire the group into dispatch + help**

In `parse_args` (line 444 match), add the arm:

```rust
        "status" => parse_status(&mut it).map_err(|e| tag_group(e, group)),
```

Find `group_name` (`grep -n "fn group_name" src/cli.rs`) and add `"status"` so it is recognized as a group (mirror an existing entry, e.g. how `"agent"` is registered). If `render_group_help` has a per-group match, add a `status` help string mirroring the format of the `agent` group; if it falls through to a default, no extra change is needed.

- [ ] **Step 6: Add the `run_cli` handlers**

In `run_cli` (after the existing `CliAction::AgentSend` arm, around line 1356), add:

```rust
        CliAction::StatusSet { state, message } => {
            let parsed = crate::data::store::ReportedState::parse(&state).ok_or_else(|| {
                Error::UserInput(format!(
                    "invalid status '{state}'; expected working|waiting|blocked|done"
                ))
            })?;
            let ws = resolve_current_workspace(&store)?;
            store.set_workspace_status(ws.id, parsed, message.as_deref(), "model")?;
            println!("status: {}", parsed.as_str());
        }
        CliAction::StatusClear => {
            let ws = resolve_current_workspace(&store)?;
            store.clear_workspace_status(ws.id)?;
            println!("status cleared");
        }
        CliAction::StatusFromHook => {
            use std::io::Read;
            let mut buf = String::new();
            // Hooks pipe JSON on stdin; tolerate empty/garbage by no-op exit 0
            // so a hook never fails the agent's turn.
            let _ = std::io::stdin().read_to_string(&mut buf);
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&buf) {
                if let Some(state) = crate::agent::hooks::state_from_hook(&json) {
                    if let Ok(ws) = resolve_current_workspace(&store) {
                        let _ = store.set_workspace_status(ws.id, state, None, "hook");
                    }
                }
            }
            // Always succeed: a status hook must never block or fail the turn.
        }
```

> Rationale for the swallowed errors in `from-hook`: hooks run synchronously and block the turn (10-min default, 30s for `UserPromptSubmit`). A status write that errored or panicked would degrade the agent session, so the hook path is strictly best-effort and always exits 0.

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test --lib cli::`
Expected: PASS.

- [ ] **Step 8: Manual smoke test**

Run:
```bash
cargo build
WSX_WORKSPACE_ID=$WSX_WORKSPACE_ID ./target/debug/wsx status set working --message "smoke test"
echo '{"hook_event_name":"Stop","last_assistant_message":"Which one?"}' | WSX_WORKSPACE_ID=$WSX_WORKSPACE_ID ./target/debug/wsx status from-hook
WSX_WORKSPACE_ID=$WSX_WORKSPACE_ID ./target/debug/wsx status clear
```
Expected: prints `status: working`, then the piped `from-hook` exits 0 silently, then `status cleared`. (Uses the real dev DB; `clear` removes the row again.)

- [ ] **Step 9: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): wsx status set | clear | from-hook"
```

---

## Task 7: Spawn — inject the hooks settings JSON for Claude workspaces

**Files:**
- Modify: `src/pty/session.rs` (`build_claude_command`, the `--settings` block at line 856-862)
- Test: `src/pty/session.rs` (or wherever `build_claude_command` is tested — `grep -n "build_claude_command" src/pty/session.rs`)

- [ ] **Step 1: Write the failing test**

Locate the existing test module for spawn-command building (`grep -n "mod tests" src/pty/session.rs` and search for tests that inspect `build_claude_command`'s args). Add a test that renders a `Fresh` spawn and asserts the settings JSON carries the hooks. Use the existing helpers/fixtures in that module for constructing a `SpawnMode::Fresh { .. }`; the assertion is the new part:

```rust
#[test]
fn fresh_spawn_injects_status_hooks_settings() {
    // Build a Fresh spawn (reuse the module's existing fixture helper for
    // SpawnMode::Fresh; see neighbouring tests for the exact constructor).
    let cmd = build_claude_command(
        std::path::Path::new("/tmp/ws"),
        &fresh_mode_fixture(),
        crate::agent::remote_control::RemoteOpts { enabled: false, sandbox: false },
    );
    let args = command_args(&cmd); // existing helper that extracts argv as Vec<String>
    let idx = args.iter().position(|a| a == "--settings").expect("has --settings");
    let json: serde_json::Value = serde_json::from_str(&args[idx + 1]).unwrap();
    assert!(json["hooks"]["Stop"].is_array());
    assert!(json["hooks"]["UserPromptSubmit"].is_array());
}
```

> If the test module lacks a `command_args`/argv helper, add a small one using `portable_pty::CommandBuilder`'s accessors, or assert against `cmd` via whatever inspection the neighbouring tests already use. Match the existing test style rather than inventing a new harness.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib pty::session::`
Expected: FAIL — Fresh spawn currently emits no `--settings`.

- [ ] **Step 3: Replace the settings block**

In `build_claude_command`, replace the PM-only settings block (line 856-862):

```rust
    if let SpawnMode::ProjectManager {
        fast_mode: true, ..
    } = mode
    {
        cmd.arg("--settings");
        cmd.arg(r#"{"fastMode":true}"#);
    }
```

with a general one that injects status hooks for the real workspace agents (Fresh/Continue) and preserves `fastMode` for PM:

```rust
    // Status-reporting hooks go to the developer agents (Fresh/Continue);
    // the PM pane keeps just its fastMode flag. The hook command points at the
    // running wsx binary by absolute path so PATH differences can't break it.
    let pm_fast = matches!(mode, SpawnMode::ProjectManager { fast_mode: true, .. });
    let inject_hooks = matches!(mode, SpawnMode::Fresh { .. } | SpawnMode::Continue { .. });
    if inject_hooks {
        let wsx_bin = std::env::current_exe()
            .unwrap_or_else(|_| std::path::PathBuf::from("wsx"));
        cmd.arg("--settings");
        cmd.arg(crate::agent::hooks::claude_settings_json(false, &wsx_bin));
    } else if pm_fast {
        cmd.arg("--settings");
        cmd.arg(r#"{"fastMode":true}"#);
    }
```

> `fast_mode` is `false` for the developer-agent settings because fast mode is a PM-pane concern; if a future requirement wants fast mode on dev agents, pass the real flag here.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib pty::session::`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/pty/session.rs
git commit -m "feat(spawn): inject status-reporting hooks via Claude --settings"
```

---

## Task 8: SKILL.md + doctrine — tell the agent to report

**Files:**
- Modify: `skills/wsx/SKILL.md` (after the `## CLI surface` section, before `## Slug rules`)
- Modify: `src/agent/doctrine.rs` (add `CLAUSE_STATUS`; include it; update tests)
- Test: `src/agent/doctrine.rs` `tests`

- [ ] **Step 1: Write the failing doctrine test**

In `src/agent/doctrine.rs` tests, extend `doctrine_covers_all_practices_for_claude`:

```rust
    #[test]
    fn doctrine_mentions_status_reporting() {
        let d = process_doctrine(AgentKind::Claude).to_lowercase();
        assert!(
            d.contains("wsx status"),
            "doctrine must tell the agent to report status: {d}"
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib agent::doctrine::tests::doctrine_mentions_status_reporting`
Expected: FAIL — clause absent.

- [ ] **Step 3: Add the doctrine clause**

In `src/agent/doctrine.rs`, add the const (after `CLAUSE_WSX_SKILL`, line 30):

```rust
const CLAUSE_STATUS: &str = "- Report your status as you go with `wsx status set \
    <working|blocked|done> --message \"<one line>\"`: `working` when you start \
    substantive work, `blocked` when you need a decision or answer from the user, \
    and `done` when the task is finished. This keeps the wsx dashboard accurate.";
```

Include it in `process_doctrine` (line 62-71), after the wsx-skill clause:

```rust
    clauses.push(CLAUSE_WSX_SKILL);
    clauses.push(CLAUSE_STATUS);
    format!("{DOCTRINE_HEADER}\n\n{}", clauses.join("\n"))
```

> All agent kinds get this clause (claude/pi/hermes/codex can all call the CLI; non-Claude agents have no hooks, so the doctrine nudge is their primary push path).

- [ ] **Step 4: Add the SKILL.md section**

In `skills/wsx/SKILL.md`, after the `## CLI surface` block (line 22-46) and before `## Slug rules` (line 47), insert:

```markdown
## Reporting your status

wsx shows each workspace's state on its dashboard. Keep it accurate by pushing
your status — it operates on the CURRENT workspace (resolved from
`$WSX_WORKSPACE_ID`, else the cwd's worktree), no `<repo>/<slug>` args:

```bash
wsx status set working --message "running the test suite"
wsx status set blocked --message "need your call on the auth approach"
wsx status set done    --message "implemented and tests green"
```

When to call it:
- `working` — when you begin substantive work on a request.
- `blocked` — when you stop to ask the user a question or need a decision.
- `done` — when the task is complete.

The `--message` is a short one-liner shown on the dashboard. Claude Code hooks
also report coarse state automatically, but an explicit `set` with a message is
always clearer — prefer it at the transitions above.
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib agent::doctrine::tests`
Expected: PASS (new test + the existing doctrine tests, which only assert substring presence and are unaffected).

- [ ] **Step 6: Commit**

```bash
git add src/agent/doctrine.rs skills/wsx/SKILL.md
git commit -m "docs(agent): doctrine + skill guidance for wsx status reporting"
```

---

## Task 9: Full verification

- [ ] **Step 1: Run the entire test suite**

Run: `cargo test`
Expected: PASS, no failures.

- [ ] **Step 2: Lint**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings. Fix any that appear (commonly: unused imports from edits, `clippy::needless_return` in the classifier — adjust inline).

- [ ] **Step 3: Format**

Run: `cargo fmt`
Then: `git diff --stat` — if fmt changed anything, review and commit:
```bash
git add -A && git commit -m "style: cargo fmt"
```

- [ ] **Step 4: End-to-end manual check (optional but recommended)**

In a real wsx TUI session, start a workspace agent and confirm: (a) the dashboard reflects `working`/`blocked`/`done` as you drive the agent, (b) a stale `working` still decays to Stalled after 60s of JSONL silence, (c) `wsx status clear` returns the workspace to heuristic classification. This also exercises spec spike-validation item #5 (does `AskUserQuestion` cause a `Stop` that clobbers `blocked`?) — note the observed behaviour for a follow-up if it bites.

---

## Out of scope (future work)

- Per-agent-instance status (multi-agent workspaces): write to a `workspace_agents`-keyed row instead of/in addition to the workspace row. The `WSX_AGENT_INSTANCE_ID` env var is already available for this.
- Rendering `reported_message` in the dashboard detail pane (this plan stores it; surfacing it in the UI is a separate change to the detail modules).
- Making `blocked` sticky against a trailing `Stop`→`done` clobber, pending the interactive ordering check.
