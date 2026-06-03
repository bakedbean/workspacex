# Multi-Agent Workspaces Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a user attach additional agents (including a second agent of the same kind) to an existing wsx workspace, view them alongside the primary agent, and have agents exchange prompts.

**Architecture:** Introduce the *agent instance* (a row in a new `workspace_agents` table) as the unit of attachment, re-keying the session manager and split-tree from `WorkspaceId` to `AgentInstanceId`. Added agents share the worktree, receive a lightweight injected context note, and communicate via a SQLite `agent_messages` inbox the TUI drains on its existing tick.

**Tech Stack:** Rust, ratatui 0.29, rusqlite (SQLite), portable-pty, tokio. Tests are inline `#[cfg(test)] mod tests` modules run with `cargo test`. The data layer test harness is `Store::open_in_memory()`.

**Reference spec:** `docs/superpowers/specs/2026-06-03-multi-agent-workspaces-design.md`

**Conventions used throughout:**
- After any code change: `cargo build` must succeed and `cargo clippy --all-targets -- -D warnings` must be clean (the repo treats clippy warnings as errors — see existing `#[allow(...)]` attributes).
- Format with `cargo fmt` before each commit.
- Commit messages: conventional commits, **no** `Co-Authored-By`/"Generated with" trailers.

---

## File Structure

| File | Responsibility | New/Changed |
|---|---|---|
| `src/data/store.rs` | `AgentInstanceId`/`AgentInstance` types; schema V12 (both new tables + backfill) | Changed |
| `src/data/agents.rs` | Roster CRUD + `instance_label` (single source of truth for names) | **New** |
| `src/data/messages.rs` | `agent_messages` inbox CRUD | **New** |
| `src/data/mod.rs` | Register `agents` + `messages` modules | Changed |
| `src/agent/handoff.rs` | Build the injected context note for an added agent | **New** |
| `src/agent/mod.rs` | Register `handoff` module | Changed |
| `src/app/messaging.rs` | TUI-side inbox drain + delivery on tick | **New** |
| `src/pty/session.rs` | `SessionManager` re-key to `AgentInstanceId`; spawn-env identity; `session_ref` capture | Changed |
| `src/app.rs` | `primary_instance`/`session_for` helpers; roster bootstrap; tick drain call | Changed |
| `src/app/render.rs` | Pane-data aggregation via `AttachTarget` | Changed |
| `src/ui/split.rs` | `AttachTarget` leaf | Changed |
| `src/ui/attached.rs` | Footer agents row + switch-key pool helper | Changed |
| `src/ui/modal.rs` | `Modal::AgentsPanel` | Changed |
| `src/app/input.rs` | `^x a` open panel; agent switch keys; panel input; agent chip clicks | Changed |
| `src/cli.rs` | `wsx agent {list,send,add}` parsing + dispatch | Changed |
| `skills/wsx/SKILL.md` | Multi-agent workspaces section | Changed |

---

# Phase 1 — Data foundation (no behavior change)

## Task 1: `AgentInstanceId` / `AgentInstance` types + label function

**Files:**
- Modify: `src/data/store.rs` (near `WorkspaceId` at line 12)
- Create: `src/data/agents.rs`
- Modify: `src/data/mod.rs`

- [ ] **Step 1: Add the id newtype** in `src/data/store.rs` immediately after the `WorkspaceId` definition (line 12):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AgentInstanceId(pub i64);
```

- [ ] **Step 2: Create `src/data/agents.rs`** with the `AgentInstance` struct and the label function:

```rust
//! Roster of agent instances attached to a workspace.
//!
//! An *agent instance* is one agent attached to a workspace. The workspace's
//! original (creation-time) agent is its primary instance; additional agents
//! — including duplicates of the same kind — are non-primary instances.

use crate::data::store::{AgentInstanceId, Store, WorkspaceId};
use crate::pty::session::AgentKind;
use crate::Result;

#[derive(Debug, Clone)]
pub struct AgentInstance {
    pub id: AgentInstanceId,
    pub workspace_id: WorkspaceId,
    pub agent: AgentKind,
    pub ordinal: i64,
    pub is_primary: bool,
    pub session_ref: Option<String>,
    pub created_at: i64,
}

/// The single source of truth for an instance's display/address name.
/// Ordinal 1 → bare agent name; ordinal >= 2 → `name#N`.
/// The footer, the `wsx agent send` CLI, and delivered message banners all
/// call this so they cannot disagree about what "claude#2" is called.
pub fn instance_label(agent: AgentKind, ordinal: i64) -> String {
    if ordinal <= 1 {
        agent.display_name().to_string()
    } else {
        format!("{}#{}", agent.display_name(), ordinal)
    }
}

impl AgentInstance {
    pub fn label(&self) -> String {
        instance_label(self.agent, self.ordinal)
    }
}
```

- [ ] **Step 3: Register the module** — add to `src/data/mod.rs` (follow the existing `pub mod repo;` style):

```rust
pub mod agents;
```

- [ ] **Step 4: Write the failing test** at the bottom of `src/data/agents.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_label_omits_suffix_for_first_and_adds_for_rest() {
        assert_eq!(instance_label(AgentKind::Claude, 1), "claude");
        assert_eq!(instance_label(AgentKind::Claude, 2), "claude#2");
        assert_eq!(instance_label(AgentKind::Codex, 3), "codex#3");
    }
}
```

- [ ] **Step 5: Run the test**

Run: `cargo test -p wsx instance_label_omits_suffix`
(If the crate is not named `wsx`, drop `-p wsx`; use `cargo test instance_label_omits_suffix`.)
Expected: PASS.

- [ ] **Step 6: Verify build + clippy**

Run: `cargo build && cargo clippy --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
cargo fmt
git add src/data/store.rs src/data/agents.rs src/data/mod.rs
git commit -m "feat(data): add AgentInstanceId, AgentInstance, and instance_label"
```

---

## Task 2: Schema V12 — `workspace_agents` + `agent_messages` tables + backfill

**Files:**
- Modify: `src/data/store.rs` (the `migrate()` fn ending at line ~218, and the `SCHEMA_V*` constants near line 670+)

The two tables land in the same migration bump (V12) per the spec, even though
`agent_messages` is not used until Phase 4 — keeping migrations append-only.

- [ ] **Step 1: Add the schema constant** alongside the other `SCHEMA_V*` constants (after `SCHEMA_V10_WORKSPACE_LAYOUTS`, near line 709):

```rust
const SCHEMA_V12_MULTI_AGENT: &str = r#"
CREATE TABLE IF NOT EXISTS workspace_agents (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id  INTEGER NOT NULL REFERENCES workspaces(id),
    agent         TEXT    NOT NULL,
    ordinal       INTEGER NOT NULL,
    is_primary    INTEGER NOT NULL DEFAULT 0,
    session_ref   TEXT,
    created_at    INTEGER NOT NULL,
    UNIQUE(workspace_id, agent, ordinal)
);
CREATE INDEX IF NOT EXISTS idx_workspace_agents_ws ON workspace_agents(workspace_id);

CREATE TABLE IF NOT EXISTS agent_messages (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id    INTEGER NOT NULL,
    target_agent_id INTEGER NOT NULL REFERENCES workspace_agents(id),
    from_agent_id   INTEGER,
    body            TEXT NOT NULL,
    created_at      INTEGER NOT NULL,
    delivered_at    INTEGER
);
CREATE INDEX IF NOT EXISTS idx_agent_messages_undelivered
    ON agent_messages(workspace_id) WHERE delivered_at IS NULL;
"#;
```

- [ ] **Step 2: Add the migration step** in `migrate()`, immediately before the final `Ok(())` (after the `if v < 11 { … }` block at line ~217):

```rust
if v < 12 {
    self.conn.execute_batch(SCHEMA_V12_MULTI_AGENT)?;
    // Backfill one primary instance row per existing workspace from the
    // denormalized workspaces.agent column.
    self.conn.execute(
        "INSERT INTO workspace_agents (workspace_id, agent, ordinal, is_primary, created_at)
         SELECT id, agent, 1, 1, created_at FROM workspaces",
        [],
    )?;
    self.conn.execute("PRAGMA user_version = 12", [])?;
}
```

- [ ] **Step 3: Write the failing test** in the `#[cfg(test)] mod tests` block of `src/data/store.rs` (follow the existing `open_in_memory_runs_migrations_idempotently` test at line 766 for seeding):

```rust
#[test]
fn migration_v12_backfills_one_primary_instance_per_workspace() {
    let store = Store::open_in_memory().unwrap();
    let repo = store.add_repo(std::path::Path::new("/tmp/r"), "r", "wsx").unwrap();
    let ws = store
        .insert_workspace(&NewWorkspace {
            repo_id: repo,
            name: "w1",
            branch: "wsx/w1",
            worktree_path: std::path::Path::new("/tmp/r/w1"),
            yolo: false,
            agent: crate::pty::session::AgentKind::Codex,
        })
        .unwrap();
    // NOTE: insert_workspace must also seed the primary instance (Task 6).
    // For now this test asserts the backfill path via a re-migrate on a row
    // inserted before the instance table existed; see Step 4.
    let count: i64 = store
        .conn_for_test()
        .query_row(
            "SELECT count(*) FROM workspace_agents WHERE workspace_id = ?1 AND is_primary = 1",
            [ws.0],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    let v: i64 = store
        .conn_for_test()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, 12);
}
```

- [ ] **Step 4: Add a minimal test accessor** if one does not already exist. Check `src/data/store.rs` for an existing test-only connection accessor; if absent, add inside `impl Store`:

```rust
#[cfg(test)]
pub(crate) fn conn_for_test(&self) -> &rusqlite::Connection {
    &self.conn
}
```

Because `insert_workspace` does not yet seed an instance row (that arrives in Task 6), this test's backfill assertion is satisfied by the migration's `INSERT … SELECT` only if the row predates the table. To keep the test meaningful now, change it to insert the workspace row directly via SQL *before* asserting, OR accept that Task 6 will make `insert_workspace` seed the row and this test will then pass via that path. Use this interim body for Step 3's seeding instead of `insert_workspace`:

```rust
store.conn_for_test().execute(
    "INSERT INTO workspaces (repo_id, name, branch, worktree_path, state, setup_status, created_at, yolo, agent)
     VALUES (?1, 'w1', 'wsx/w1', '/tmp/r/w1', 'Ready', 'Ok', 1, 0, 'codex')",
    [repo.0],
).unwrap();
let ws = WorkspaceId(store.conn_for_test().last_insert_rowid());
// Re-run migration to exercise backfill against the just-inserted row:
store.conn_for_test().execute("PRAGMA user_version = 11", []).unwrap();
store.conn_for_test().execute("DELETE FROM workspace_agents", []).unwrap();
store.migrate_for_test().unwrap();
```

Add the test-only re-migrate accessor inside `impl Store`:

```rust
#[cfg(test)]
pub(crate) fn migrate_for_test(&self) -> Result<()> {
    self.migrate()
}
```

- [ ] **Step 5: Run the test**

Run: `cargo test migration_v12_backfills`
Expected: PASS.

- [ ] **Step 6: Verify the idempotency test still passes** (re-running migrate must not fail):

Run: `cargo test open_in_memory_runs_migrations_idempotently`
Expected: PASS.

- [ ] **Step 7: Build + clippy + commit**

```bash
cargo build && cargo clippy --all-targets -- -D warnings
cargo fmt
git add src/data/store.rs
git commit -m "feat(data): add schema V12 with workspace_agents and agent_messages"
```

---

## Task 3: Roster CRUD store methods

**Files:**
- Modify: `src/data/agents.rs`

- [ ] **Step 1: Write failing tests** at the bottom of `src/data/agents.rs` (extend the existing `mod tests`):

```rust
#[cfg(test)]
mod store_tests {
    use super::*;
    use crate::data::store::{NewWorkspace, Store, WorkspaceId};

    fn seed_ws(store: &Store) -> WorkspaceId {
        let repo = store.add_repo(std::path::Path::new("/tmp/r"), "r", "wsx").unwrap();
        store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "w1",
                branch: "wsx/w1",
                worktree_path: std::path::Path::new("/tmp/r/w1"),
                yolo: false,
                agent: AgentKind::Claude,
            })
            .unwrap()
    }

    #[test]
    fn add_then_list_computes_ordinals_and_labels() {
        let store = Store::open_in_memory().unwrap();
        let ws = seed_ws(&store); // seeds primary claude (ordinal 1) via Task 6
        let second = store.add_workspace_agent(ws, AgentKind::Claude).unwrap();
        let codex = store.add_workspace_agent(ws, AgentKind::Codex).unwrap();
        assert_eq!(second.ordinal, 2);
        assert_eq!(second.label(), "claude#2");
        assert_eq!(codex.ordinal, 1);
        assert_eq!(codex.label(), "codex");

        let all = store.workspace_agents(ws).unwrap();
        assert_eq!(all.len(), 3);
        assert!(all[0].is_primary); // primary first
    }

    #[test]
    fn remove_refuses_primary_but_removes_others() {
        let store = Store::open_in_memory().unwrap();
        let ws = seed_ws(&store);
        let primary = store.workspace_agents(ws).unwrap()[0].id;
        assert!(store.remove_workspace_agent(primary).is_err());

        let added = store.add_workspace_agent(ws, AgentKind::Pi).unwrap();
        store.remove_workspace_agent(added.id).unwrap();
        assert_eq!(store.workspace_agents(ws).unwrap().len(), 1);
    }

    #[test]
    fn resolve_label_finds_instance() {
        let store = Store::open_in_memory().unwrap();
        let ws = seed_ws(&store);
        let second = store.add_workspace_agent(ws, AgentKind::Claude).unwrap();
        assert_eq!(store.resolve_instance_label(ws, "claude#2").unwrap(), Some(second.id));
        assert_eq!(store.resolve_instance_label(ws, "nope").unwrap(), None);
    }
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test add_then_list_computes_ordinals`
Expected: FAIL (methods not found). (Tests also depend on Task 6 seeding the primary; if Task 6 is not yet done, adjust `seed_ws` to call `store.add_workspace_agent(ws, AgentKind::Claude)` once to stand in for the primary.)

- [ ] **Step 3: Implement the store methods** — add to `src/data/agents.rs`:

```rust
use crate::data::store::now_ms; // if now_ms is not pub, see note below

fn row_to_instance(r: &rusqlite::Row) -> rusqlite::Result<AgentInstance> {
    Ok(AgentInstance {
        id: AgentInstanceId(r.get(0)?),
        workspace_id: WorkspaceId(r.get(1)?),
        agent: AgentKind::from_str_or_default(Some(&r.get::<_, String>(2)?)),
        ordinal: r.get(3)?,
        is_primary: r.get::<_, i64>(4)? != 0,
        session_ref: r.get(5)?,
        created_at: r.get(6)?,
    })
}

impl Store {
    /// All instances for a workspace, primary first then by creation time.
    pub fn workspace_agents(&self, ws: WorkspaceId) -> Result<Vec<AgentInstance>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, workspace_id, agent, ordinal, is_primary, session_ref, created_at
             FROM workspace_agents WHERE workspace_id = ?1
             ORDER BY is_primary DESC, created_at ASC, id ASC",
        )?;
        let rows = stmt.query_map([ws.0], row_to_instance)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Add a non-primary instance, computing the next ordinal for its kind.
    pub fn add_workspace_agent(&self, ws: WorkspaceId, agent: AgentKind) -> Result<AgentInstance> {
        let next: i64 = self.conn().query_row(
            "SELECT COALESCE(MAX(ordinal), 0) + 1 FROM workspace_agents
             WHERE workspace_id = ?1 AND agent = ?2",
            rusqlite::params![ws.0, agent.store_value()],
            |r| r.get(0),
        )?;
        let now = now_ms();
        self.conn().execute(
            "INSERT INTO workspace_agents (workspace_id, agent, ordinal, is_primary, created_at)
             VALUES (?1, ?2, ?3, 0, ?4)",
            rusqlite::params![ws.0, agent.store_value(), next, now],
        )?;
        Ok(AgentInstance {
            id: AgentInstanceId(self.conn().last_insert_rowid()),
            workspace_id: ws,
            agent,
            ordinal: next,
            is_primary: false,
            session_ref: None,
            created_at: now,
        })
    }

    /// Seed the primary instance for a freshly created workspace.
    pub fn add_primary_agent(&self, ws: WorkspaceId, agent: AgentKind, created_at: i64) -> Result<AgentInstance> {
        self.conn().execute(
            "INSERT INTO workspace_agents (workspace_id, agent, ordinal, is_primary, created_at)
             VALUES (?1, ?2, 1, 1, ?3)",
            rusqlite::params![ws.0, agent.store_value(), created_at],
        )?;
        Ok(AgentInstance {
            id: AgentInstanceId(self.conn().last_insert_rowid()),
            workspace_id: ws,
            agent,
            ordinal: 1,
            is_primary: true,
            session_ref: None,
            created_at,
        })
    }

    pub fn remove_workspace_agent(&self, id: AgentInstanceId) -> Result<()> {
        let is_primary: i64 = self.conn().query_row(
            "SELECT is_primary FROM workspace_agents WHERE id = ?1",
            [id.0],
            |r| r.get(0),
        )?;
        if is_primary != 0 {
            return Err(crate::Error::msg("cannot remove the primary agent"));
        }
        self.conn()
            .execute("DELETE FROM workspace_agents WHERE id = ?1", [id.0])?;
        Ok(())
    }

    pub fn set_instance_session_ref(&self, id: AgentInstanceId, session_ref: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE workspace_agents SET session_ref = ?1 WHERE id = ?2",
            rusqlite::params![session_ref, id.0],
        )?;
        Ok(())
    }

    /// Resolve a label like "claude" or "claude#2" to an instance id.
    pub fn resolve_instance_label(&self, ws: WorkspaceId, label: &str) -> Result<Option<AgentInstanceId>> {
        Ok(self
            .workspace_agents(ws)?
            .into_iter()
            .find(|i| i.label() == label)
            .map(|i| i.id))
    }

    /// The primary instance id for a workspace.
    pub fn primary_instance_id(&self, ws: WorkspaceId) -> Result<Option<AgentInstanceId>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT id FROM workspace_agents WHERE workspace_id = ?1 AND is_primary = 1",
                [ws.0],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .map(AgentInstanceId))
    }
}
```

Notes for the implementer:
- `self.conn()` — if `Store` exposes its `Connection` only as a private field `conn`, add a private `fn conn(&self) -> &rusqlite::Connection { &self.conn }` to `impl Store` in `store.rs`, or move these methods into `store.rs`. Prefer a small `pub(crate) fn conn(&self)` accessor in `store.rs` so `agents.rs`/`messages.rs` can share the connection. Match whatever accessor already exists (Task 2 added `conn_for_test`; promote it to a non-test `pub(crate) fn conn`).
- `now_ms()` — confirm it is `pub(crate)` in `store.rs`; if it is private, make it `pub(crate)`.
- `crate::Error::msg` / `.optional()` — use the crate's existing error constructor and `rusqlite::OptionalExtension` (add `use rusqlite::OptionalExtension;`). If the crate's `Error` has no `msg` constructor, mirror how other store methods build domain errors.

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib agents`
Expected: PASS (after Task 6 seeds the primary; otherwise see Step 2 note).

- [ ] **Step 5: Build + clippy + commit**

```bash
cargo build && cargo clippy --all-targets -- -D warnings
cargo fmt
git add src/data/agents.rs src/data/store.rs
git commit -m "feat(data): add workspace_agents roster CRUD"
```

---

# Phase 2 — Session re-key (behavior-preserving refactor)

## Task 4: Re-key `SessionManager` to `AgentInstanceId`

**Files:**
- Modify: `src/pty/session.rs` (`SessionManager` at line 1302)

This task intentionally breaks compilation; Task 5 fixes every call site. Do
them back-to-back.

- [ ] **Step 1: Change the map key and method signatures** in `src/pty/session.rs`:

```rust
pub struct SessionManager {
    sessions: HashMap<crate::data::store::AgentInstanceId, Arc<Session>>,
    pm: Option<Arc<Session>>,
}
```

Update `spawn`, `get`, and any per-session removal to take
`AgentInstanceId` instead of `WorkspaceId`:

```rust
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    &mut self,
    id: crate::data::store::AgentInstanceId,
    cwd: &Path,
    cols: u16,
    rows: u16,
    mode: SpawnMode,
    remote: crate::agent::remote_control::RemoteOpts,
    agent: AgentKind,
) -> Result<Arc<Session>> {
    if let Some(s) = self.sessions.get(&id) {
        if matches!(*s.status.read().unwrap(), SessionStatus::Running { .. }) {
            return Ok(s.clone());
        }
    }
    let session = Arc::new(spawn_session(cwd, cols, rows, mode, remote, agent)?);
    self.sessions.insert(id, session.clone());
    Ok(session)
}

pub fn get(&self, id: crate::data::store::AgentInstanceId) -> Option<Arc<Session>> {
    self.sessions.get(&id).cloned()
}
```

`kill_all`, `spawn_pm`, and `pm` are unchanged (the `pm` field keeps its own slot).

- [ ] **Step 2: Add a `remove` method** for instance teardown (used in Task 10's remove flow):

```rust
pub fn remove(&mut self, id: crate::data::store::AgentInstanceId) {
    if let Some(s) = self.sessions.remove(&id) {
        s.kill();
    }
}
```

- [ ] **Step 3: Do NOT commit yet** — compilation is broken until Task 5. Proceed directly to Task 5.

---

## Task 5: Add `App` instance helpers and migrate call sites

**Files:**
- Modify: `src/app.rs` (App impl; `attach_workspace` ~1001, `restore_attached_state` ~929, `ensure_workspace_session` ~973)
- Modify: `src/app/render.rs` (pane-data aggregation ~451)
- Modify: any other file the compiler flags (`src/app/input.rs`, `src/app/background.rs`, etc.)

- [ ] **Step 1: Add helpers** to `impl App` in `src/app.rs`:

```rust
/// The primary agent instance for a workspace. Falls back to seeding one if
/// missing (defensive — backfill/creation should already have created it).
pub(crate) fn primary_instance(&self, ws: crate::data::store::WorkspaceId)
    -> Option<crate::data::store::AgentInstanceId> {
    self.store.primary_instance_id(ws).ok().flatten()
}

/// Session for a given instance.
pub(crate) fn session_for(&self, inst: crate::data::store::AgentInstanceId)
    -> Option<std::sync::Arc<crate::pty::session::Session>> {
    self.sessions.get(inst)
}
```

- [ ] **Step 2: Migrate call sites compiler-guided.** Run `cargo build 2>&1 | head -50` and fix each error. The mechanical transform is:

  - `self.sessions.get(ws_id)` → `self.primary_instance(ws_id).and_then(|i| self.sessions.get(i))`
  - `self.sessions.spawn(ws_id, …)` → resolve the instance id first, then `self.sessions.spawn(inst_id, …)`. In `ensure_workspace_session`, look up `primary_instance(ws_id)` (seed it via `store.add_primary_agent` if `None`) and pass that id.

  Show the `ensure_workspace_session` change explicitly (in `src/app.rs` ~973):

```rust
// Resolve (or seed) the primary instance, then spawn keyed by instance.
let inst = match self.store.primary_instance_id(ws_id)? {
    Some(i) => i,
    None => {
        // Defensive seed for pre-migration / freshly created rows.
        let ws = self.workspace_by_id(ws_id).expect("workspace exists");
        self.store.add_primary_agent(ws_id, ws.agent, ws.created_at)?.id
    }
};
let session = self.sessions.spawn(inst, &cwd, cols, rows, mode, remote, ws.agent)?;
```

  (`workspace_by_id` — use the existing accessor the codebase has for finding a workspace by id; the explorer report referenced `app.workspaces.iter().find(|(_, w)| w.id == ws_id)`. Use that idiom if no named helper exists.)

- [ ] **Step 3: Migrate pane-data aggregation** in `src/app/render.rs` (~451). It currently maps a `WorkspaceId` leaf to a session. After Task 9 the leaf becomes an `AttachTarget`; for *now* (leaf still `WorkspaceId`) resolve through the primary instance:

```rust
let session = app.session_for(app.primary_instance(ws_id)?)?;
```

- [ ] **Step 4: Build until green**

Run: `cargo build`
Expected: success. Iterate Step 2 until no errors.

- [ ] **Step 5: Run the full test suite** to confirm no behavior regression:

Run: `cargo test`
Expected: all pass (this phase is a pure refactor).

- [ ] **Step 6: Clippy + commit**

```bash
cargo clippy --all-targets -- -D warnings
cargo fmt
git add -A
git commit -m "refactor(session): key SessionManager by AgentInstanceId"
```

---

# Phase 3 — Add agents & view them

## Task 6: Seed the primary instance on workspace creation

**Files:**
- Modify: `src/data/workspace.rs` (`create()` ~22-118, after `insert_workspace`)

- [ ] **Step 1: Write the failing test** in `src/data/agents.rs` `store_tests` (replace the interim `seed_ws` stand-in): assert that `insert_workspace` followed by the creation path yields exactly one primary instance. Since `create()` does git/worktree work unsuitable for a unit test, instead test the store seam directly:

```rust
#[test]
fn add_primary_agent_seeds_single_primary() {
    let store = Store::open_in_memory().unwrap();
    let repo = store.add_repo(std::path::Path::new("/tmp/r"), "r", "wsx").unwrap();
    let ws = store.insert_workspace(&NewWorkspace {
        repo_id: repo, name: "w", branch: "wsx/w",
        worktree_path: std::path::Path::new("/tmp/r/w"), yolo: false,
        agent: AgentKind::Hermes,
    }).unwrap();
    store.add_primary_agent(ws, AgentKind::Hermes, 1).unwrap();
    let all = store.workspace_agents(ws).unwrap();
    assert_eq!(all.len(), 1);
    assert!(all[0].is_primary);
    assert_eq!(all[0].agent, AgentKind::Hermes);
}
```

- [ ] **Step 2: Run to verify it passes** (the method exists from Task 3):

Run: `cargo test add_primary_agent_seeds_single_primary`
Expected: PASS.

- [ ] **Step 3: Wire creation** — in `src/data/workspace.rs` `create()`, immediately after the `store.insert_workspace(&NewWorkspace { … })?` call that returns `id`:

```rust
// Seed the primary agent instance so the roster is authoritative from birth.
store.add_primary_agent(id, agent, now_ms())?;
```

(Import `now_ms` or reuse the timestamp already computed for the workspace row if one is in scope.)

- [ ] **Step 4: Build, test, commit**

```bash
cargo build && cargo test && cargo clippy --all-targets -- -D warnings
cargo fmt
git add src/data/workspace.rs src/data/agents.rs
git commit -m "feat(workspace): seed primary agent instance on create"
```

---

## Task 7: Context-note builder

**Files:**
- Create: `src/agent/handoff.rs`
- Modify: `src/agent/mod.rs`

- [ ] **Step 1: Create `src/agent/handoff.rs`:**

```rust
//! Builds the lightweight context note injected into an *added* agent so it
//! can orient itself in a workspace already in progress. The note is fed into
//! the existing `--append-system-prompt` seam (see `SpawnMode::Fresh`).

use crate::pty::session::AgentKind;

pub struct HandoffContext<'a> {
    pub primary_label: &'a str,   // e.g. "claude"
    pub branch: &'a str,
    pub base_ref: &'a str,        // e.g. "main"
    pub workspace_name: &'a str,
}

/// The injected note. Uniform across all agent types: it points the new agent
/// at the shared worktree + git rather than exporting any transcript.
pub fn context_note(_added: AgentKind, ctx: &HandoffContext) -> String {
    format!(
        "You are joining an existing wsx workspace \"{name}\" as an additional agent, \
         alongside `{primary}` (the primary agent). You share the same git worktree \
         and branch (`{branch}`) with the other agents here.\n\n\
         To see the work already in progress, inspect the working tree and run \
         `git diff {base}...HEAD`. The primary agent has been working on this branch; \
         review the current state before acting.\n\n\
         You can communicate with the other agents in this workspace. Run \
         `wsx agent list` to see them, and `wsx agent send <label> \"<message>\"` to \
         send one a prompt. Your own identity is in the `$WSX_AGENT_INSTANCE_ID` \
         environment variable.",
        name = ctx.workspace_name,
        primary = ctx.primary_label,
        branch = ctx.branch,
        base = ctx.base_ref,
    )
}
```

- [ ] **Step 2: Register** in `src/agent/mod.rs`:

```rust
pub mod handoff;
```

- [ ] **Step 3: Write the failing test** at the bottom of `src/agent/handoff.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_mentions_primary_branch_and_messaging() {
        let note = context_note(
            AgentKind::Codex,
            &HandoffContext { primary_label: "claude", branch: "wsx/feat", base_ref: "main", workspace_name: "feat" },
        );
        assert!(note.contains("claude"));
        assert!(note.contains("wsx/feat"));
        assert!(note.contains("git diff main...HEAD"));
        assert!(note.contains("wsx agent send"));
    }
}
```

- [ ] **Step 4: Run, build, clippy, commit**

```bash
cargo test note_mentions_primary_branch_and_messaging
cargo build && cargo clippy --all-targets -- -D warnings
cargo fmt
git add src/agent/handoff.rs src/agent/mod.rs
git commit -m "feat(agent): add context-note builder for added agents"
```

---

## Task 8: Spawn an added instance with context (fresh) + session_ref plumbing

**Files:**
- Modify: `src/app.rs` (new method `attach_instance`)
- Modify: `src/pty/session.rs` (`SpawnMode::Fresh { custom_instructions }` usage — confirm the variant shape via the explorer report; it carries `custom_instructions`)

- [ ] **Step 1: Add an `attach_instance` method** to `impl App` in `src/app.rs` that spawns a *specific* instance. For non-primary instances it always uses `SpawnMode::Fresh` with the handoff note; the primary keeps the existing `ensure_workspace_session` path:

```rust
pub(crate) fn ensure_instance_session(
    &mut self,
    inst: crate::data::store::AgentInstanceId,
) -> Result<std::sync::Arc<crate::pty::session::Session>> {
    if let Some(s) = self.sessions.get(inst) {
        return Ok(s);
    }
    let instance = self
        .store
        .workspace_agents_by_id(inst)?    // see note
        .ok_or_else(|| crate::Error::msg("instance not found"))?;
    let ws = self.workspace_by_id(instance.workspace_id).expect("workspace exists");
    let (cols, rows) = self.attached_cell_size(); // existing helper for pty size
    let remote = self.remote_opts_for(&ws);       // existing helper used by ensure_workspace_session

    let mode = if instance.is_primary {
        // Existing behavior: native continue/fresh by cwd.
        self.primary_spawn_mode(&ws)               // factor out of ensure_workspace_session
    } else if let Some(sref) = instance.session_ref.as_deref() {
        crate::pty::session::SpawnMode::ResumeRef { session_ref: sref.to_string() } // see Step 2
    } else {
        let primary_label = self
            .store
            .workspace_agents(instance.workspace_id)?
            .into_iter()
            .find(|i| i.is_primary)
            .map(|i| i.label())
            .unwrap_or_else(|| "the primary agent".into());
        let note = crate::agent::handoff::context_note(
            instance.agent,
            &crate::agent::handoff::HandoffContext {
                primary_label: &primary_label,
                branch: &ws.branch,
                base_ref: &self.base_ref_for(&ws),  // existing helper / "main" fallback
                workspace_name: &ws.name,
            },
        );
        crate::pty::session::SpawnMode::Fresh { custom_instructions: Some(note) }
    };

    let session = self.sessions.spawn(inst, &ws.worktree_path, cols, rows, mode, remote, instance.agent)?;
    Ok(session)
}
```

Notes:
- `workspace_agents_by_id` — add a small store method that selects a single instance row by id (mirror `workspace_agents` with `WHERE id = ?1`).
- `attached_cell_size`, `remote_opts_for`, `base_ref_for`, `primary_spawn_mode` — these stand in for logic that already exists inside `ensure_workspace_session`/`build_spawn_info` (`src/app.rs` ~839-997). Extract the relevant pieces into small private helpers so both the primary and added paths share them (DRY). If extraction is too invasive, inline the equivalent logic, but prefer extraction.
- `SpawnMode::Fresh { custom_instructions }` — confirm the exact field name in `src/pty/session.rs` (~329-367) and match it. If the existing variant already appends doctrine, ensure the handoff note is concatenated, not replacing doctrine.

- [ ] **Step 2: Add the `ResumeRef` spawn mode (minimal).** In `src/pty/session.rs`, add a variant to `SpawnMode`:

```rust
ResumeRef { session_ref: String },
```

For v1, route `ResumeRef` through the per-agent resume that uses an explicit
session id where the agent supports it (Claude `--resume <id>`, Hermes
`--resume <id>`, Codex `resume <id>`); for agents/cases where an explicit id is
unavailable, fall back to `Fresh` with no note. Implement the match arm in the
command builders alongside the existing `Continue`/`Fresh` arms. **Document**
(code comment) that unsupported agents degrade to fresh — this matches the
spec's Risk note.

- [ ] **Step 3: Capture `session_ref` after spawning an added instance.** Immediately after a successful non-primary fresh spawn, best-effort capture the agent's newest native session id and persist it:

```rust
if !instance.is_primary && instance.session_ref.is_none() {
    if let Some(sref) = crate::pty::session::capture_session_ref(instance.agent, &ws.worktree_path) {
        let _ = self.store.set_instance_session_ref(inst, &sref);
    }
}
```

Add `capture_session_ref(agent, worktree) -> Option<String>` to
`src/pty/session.rs`. Implement it for Claude first (newest `*.jsonl` under the
encoded cwd dir → its session id), returning `None` for others initially
(they fall back to fresh on re-attach). This is the bounded "generalize the
Hermes marker" work; later agents can be filled in without changing callers.

- [ ] **Step 4: Build (no dedicated unit test — this is integration glue).** Add a unit test for `capture_session_ref` returning `None` for a worktree with no sessions:

```rust
#[test]
fn capture_session_ref_none_when_no_sessions() {
    let dir = std::env::temp_dir().join("wsx-test-no-session-xyz");
    assert_eq!(capture_session_ref(AgentKind::Claude, &dir), None);
}
```

Run: `cargo test capture_session_ref_none_when_no_sessions`
Expected: PASS.

- [ ] **Step 5: Build, clippy, commit**

```bash
cargo build && cargo clippy --all-targets -- -D warnings
cargo fmt
git add -A
git commit -m "feat(app): spawn added agent instances with injected context"
```

---

## Task 9: `AttachTarget` leaf + layout persistence

**Files:**
- Modify: `src/ui/split.rs` (`SplitTree` ~48, `AttachedState` ~78)
- Modify: `src/app.rs` (`restore_attached_state` ~929)
- Modify: `src/app/render.rs` (pane-data ~451)

- [ ] **Step 1: Introduce `AttachTarget`** in `src/ui/split.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttachTarget {
    pub workspace_id: crate::data::store::WorkspaceId,
    pub instance: crate::data::store::AgentInstanceId,
}

pub enum SplitTree {
    Leaf(AttachTarget),
    Split { direction: SplitDirection, children: Vec<SplitTree> },
}
```

- [ ] **Step 2: Update `restore_attached_state`** (`src/app.rs` ~929) to build leaves from the primary instance when loading a workspace, and to resolve saved layouts. For existing saved layouts that stored only `WorkspaceId`, map each to `AttachTarget { workspace_id, instance: primary_instance(ws) }`.

- [ ] **Step 3: Update pane-data aggregation** (`src/app/render.rs` ~451) to read the leaf's `AttachTarget`:

```rust
.filter_map(|(target, path, rect)| {
    let session = app.session_for(target.instance)?;
    let inst = app.store.workspace_agents_by_id(target.instance).ok().flatten();
    let (label, agent) = match inst {
        Some(i) => (i.label(), Some(i.agent)),
        None => (String::new(), None),
    };
    let focused = path == state.focus;
    Some((session, label, rect, focused, agent))
})
```

- [ ] **Step 4: Layout persistence.** The `workspace_layouts` table (schema V10) serializes the split tree. Update its serialization to include the instance id. Add a forward-compatible parse: a serialized leaf with only a workspace id resolves its primary instance at load time. Keep the schema as-is (store the extra field in the existing JSON/text blob) to avoid another migration; if the format is positional, extend it and handle the short form on read.

- [ ] **Step 5: Build, test, commit**

```bash
cargo build && cargo test && cargo clippy --all-targets -- -D warnings
cargo fmt
git add -A
git commit -m "feat(ui): make split-tree leaves target agent instances"
```

---

## Task 10: `Modal::AgentsPanel`

**Files:**
- Modify: `src/ui/modal.rs` (`Modal` enum ~25; render ~148-180; `centered` ~75)
- Modify: `src/app/input.rs` (modal handlers ~1277-1333)

- [ ] **Step 1: Add the variant** to `Modal` in `src/ui/modal.rs`:

```rust
AgentsPanel {
    workspace_id: crate::data::store::WorkspaceId,
    selected: usize,      // index into AgentKind::ALL for the "add" picker
},
```

- [ ] **Step 2: Render it** — add a match arm following the `AgentPicker` style. Build a roster string from `store.workspace_agents(workspace_id)` (passed into the render fn, or fetched — match how the existing modal render accesses data) and an add-picker line over `AgentKind::ALL`:

```rust
Modal::AgentsPanel { selected, .. } => {
    let add_row = AgentKind::ALL.iter().enumerate().map(|(i, k)| {
        let marker = if i == *selected { ">" } else { " " };
        format!("{marker} {}", k.display_name())
    }).collect::<Vec<_>>().join("   ");
    (
        "agents",
        format!(
            "Attached:\n{roster}\n\nAdd:\n  {add_row}\n\n\
             Enter add   a add all   x remove   \u{2191}\u{2193} move   Esc",
            roster = roster_lines, // built by caller: "  ▎ claude  (primary)\n  ▎ claude#2"
        ),
    )
}
```

(Match the exact tuple/return shape the other arms use; the explorer report shows arms return `(title, body)`.)

- [ ] **Step 3: Handle input** in `src/app/input.rs` (add an arm mirroring the `AgentPicker` handler at ~1294):

```rust
Modal::AgentsPanel { workspace_id, selected } => {
    use crate::pty::session::AgentKind;
    match k.code {
        KeyCode::Esc => app.modal = None,
        KeyCode::Up | KeyCode::Char('k') =>
            app.modal = Some(Modal::AgentsPanel { workspace_id, selected: selected.saturating_sub(1) }),
        KeyCode::Down | KeyCode::Char('j') =>
            app.modal = Some(Modal::AgentsPanel { workspace_id, selected: (selected + 1).min(AgentKind::ALL.len() - 1) }),
        KeyCode::Enter => {
            let kind = AgentKind::ALL[selected];
            let inst = app.store.add_workspace_agent(workspace_id, kind)?;
            let _ = app.ensure_instance_session(inst.id); // spawn now so it's live
            app.modal = None;
        }
        KeyCode::Char('a') => {
            for kind in AgentKind::ALL {
                let inst = app.store.add_workspace_agent(workspace_id, kind)?;
                let _ = app.ensure_instance_session(inst.id);
            }
            app.modal = None;
        }
        KeyCode::Char('x') => {
            // Remove the most-recently-added non-primary instance (simplest v1
            // affordance; a future iteration can let the user pick which).
            if let Some(last) = app.store.workspace_agents(workspace_id)?
                .into_iter().filter(|i| !i.is_primary).last() {
                app.sessions.remove(last.id);
                app.store.remove_workspace_agent(last.id)?;
            }
        }
        _ => {}
    }
}
```

(If the agent binary is missing, `ensure_instance_session` surfaces it the same
way `attach_workspace` does today via `Modal::AgentMissing`; keep that behavior.)

- [ ] **Step 4: Manual smoke test** (no unit test for modal wiring; verified in Task 13's integration check). Build:

Run: `cargo build && cargo clippy --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add src/ui/modal.rs src/app/input.rs
git commit -m "feat(ui): add AgentsPanel modal to add/remove workspace agents"
```

---

## Task 11: `^x a` opens the AgentsPanel

**Files:**
- Modify: `src/app/input.rs` (dashboard chord ~391-407; attached chord — find the parallel block)

- [ ] **Step 1: Extend the leader-chord completion.** In the `if app.leader_pending { … }` block, add an `a` branch before the digit branch:

```rust
if app.leader_pending {
    app.leader_pending = false;
    match k.code {
        KeyCode::Char('a') => {
            if let Some(ws) = app.current_workspace_id() { // selected (dashboard) or focused (attached)
                app.modal = Some(crate::ui::modal::Modal::AgentsPanel { workspace_id: ws, selected: 0 });
            }
            return Ok(());
        }
        KeyCode::Char(c @ '1'..='9') => {
            let idx = (c as u8 - b'1') as usize;
            fire_chip(app, idx).await;
            return Ok(());
        }
        _ => return Ok(()),
    }
}
```

Apply the same `a` branch to the attached-view leader handler (the report notes
a parallel block exists). `current_workspace_id()` — use the existing helper
that resolves the selected workspace (dashboard) or focused pane's workspace
(attached); the report shows `fire_chip` already does this resolution, so factor
out a shared accessor if needed.

- [ ] **Step 2: Build + commit**

```bash
cargo build && cargo clippy --all-targets -- -D warnings
cargo fmt
git add src/app/input.rs
git commit -m "feat(input): bind ^x a to open the agents panel"
```

---

## Task 12: Footer agents row + switch-key pool

**Files:**
- Modify: `src/ui/attached.rs` (`footer_line` ~247; `layout_chrome` ~187)

- [ ] **Step 1: Write the failing test** for the switch-key pool and agent-row spans, mirroring the existing `title_bar_spans_*` tests (~491). Add to the `#[cfg(test)] mod tests` in `src/ui/attached.rs`:

```rust
#[test]
fn switch_keys_skip_reserved_and_are_unique() {
    // 'a' is reserved (opens panel); digits are pinned-command keys.
    let keys = agent_switch_keys(5);
    assert_eq!(keys.len(), 5);
    assert!(!keys.contains(&'a'));
    assert!(keys.iter().all(|c| !c.is_ascii_digit()));
    let unique: std::collections::HashSet<_> = keys.iter().collect();
    assert_eq!(unique.len(), keys.len());
}

#[test]
fn agents_row_spans_include_label_and_color_bar() {
    let theme = Theme::by_name("default");
    let agents = vec![
        (AgentKind::Claude, "claude".to_string(), 'q'),
        (AgentKind::Codex, "codex".to_string(), 'w'),
    ];
    let spans = agents_row_spans(&agents, &theme);
    let text: String = spans.iter().map(|s| s.content.clone()).collect();
    assert!(text.contains("claude"));
    assert!(text.contains("codex"));
    assert!(text.contains('q'));
    assert!(text.contains('w'));
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test switch_keys_skip_reserved agents_row_spans_include`
Expected: FAIL (functions undefined).

- [ ] **Step 3: Implement the helpers** in `src/ui/attached.rs`:

```rust
/// Switch keys for the footer agents row, drawn from a reserved-safe pool so
/// they never collide with the `^x a` panel key or the `^x 1-9` pinned keys.
/// This is the single source of truth for the mapping; both the renderer and
/// the input dispatcher call it with the same count.
pub fn agent_switch_keys(count: usize) -> Vec<char> {
    const POOL: &[char] = &['q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p'];
    POOL.iter().copied().take(count).collect()
}

/// Spans for the agents footer row: `agents:  ▎claude  q   ▎codex  w`.
pub fn agents_row_spans(
    agents: &[(AgentKind, String, char)],
    theme: &Theme,
) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = vec![Span::raw("agents:  ".to_string())];
    for (i, (kind, label, key)) in agents.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("   ".to_string()));
        }
        spans.push(Span::styled("▎".to_string(), theme.agent_style(*kind)));
        spans.push(Span::raw(format!("{label}  ")));
        spans.push(Span::styled(key.to_string(), theme.header_style())); // match pinned chip key style
    }
    spans
}
```

- [ ] **Step 4: Render the row.** In the attached render path (where `footer_line` is composed, ~79-86), when the workspace has more than one instance, build `agents` as `(kind, label, key)` zipping `store.workspace_agents(ws)` (non-pm) with `agent_switch_keys(n)`, and push an extra `Line` into the footer `Text`. Add one row to `layout_chrome`'s vertical constraints (a `Constraint::Length(1)` that is `0` when only the primary exists), mirroring how the attention row is conditionally sized (~187-203).

- [ ] **Step 5: Run tests, build, commit**

```bash
cargo test agents_row_spans_include switch_keys_skip_reserved
cargo build && cargo clippy --all-targets -- -D warnings
cargo fmt
git add src/ui/attached.rs
git commit -m "feat(ui): render footer agents row with switch keys"
```

---

## Task 13: Agent switching — keys + clickable chips

**Files:**
- Modify: `src/app.rs` (add `agent_chip_rects: Vec<(AgentInstanceId, Rect)>` field ~209-232)
- Modify: `src/app/render.rs` (populate `agent_chip_rects` while rendering the agents row)
- Modify: `src/app/input.rs` (leader+letter switch; mouse click on chip)

- [ ] **Step 1: Add the hit-rect field** to `App` (mirror `chip_rects`):

```rust
pub(crate) agent_chip_rects: Vec<(crate::data::store::AgentInstanceId, ratatui::layout::Rect)>,
```

Initialize it to `Vec::new()` in the App constructor and clear it at the start of each render (same lifecycle as `chip_rects`).

- [ ] **Step 2: Implement the switch action** — add to `impl App` in `src/app.rs`:

```rust
/// Retarget the focused attached pane to a given instance (switching the
/// visible agent in place). Spawns the instance's session if needed.
pub(crate) fn switch_focused_pane_to(&mut self, inst: crate::data::store::AgentInstanceId) -> Result<()> {
    let _ = self.ensure_instance_session(inst)?;
    if let crate::ui::mod_::View::Attached(state) = &mut self.view {  // match the real View path
        if let Some(ws) = self.store.workspace_for_instance(inst)? {   // small store helper
            state.set_leaf_target(state.focus.clone(),
                crate::ui::split::AttachTarget { workspace_id: ws, instance: inst });
        }
    }
    Ok(())
}
```

Add `AttachedState::set_leaf_target(&mut self, path: FocusPath, target: AttachTarget)` to `src/ui/split.rs` that walks `path` and replaces the leaf. Add `Store::workspace_for_instance` (`SELECT workspace_id FROM workspace_agents WHERE id = ?1`).

- [ ] **Step 3: Wire leader+letter switching** in `src/app/input.rs` attached-view leader handler: after the `a` and digit branches, handle a letter that matches `agent_switch_keys(n)`:

```rust
KeyCode::Char(c) => {
    if let Some(ws) = app.current_workspace_id() {
        let agents: Vec<_> = app.store.workspace_agents(ws)?;
        let keys = crate::ui::attached::agent_switch_keys(agents.len());
        if let Some(idx) = keys.iter().position(|k| *k == c) {
            app.switch_focused_pane_to(agents[idx].id)?;
        }
    }
    return Ok(());
}
```

- [ ] **Step 4: Wire clicks** — in the mouse handler, after the existing `chip_rects` hit-test, add an `agent_chip_rects` hit-test that calls `switch_focused_pane_to(inst)` on a click within a chip rect (mirror the `attached_pane_rects` click handling the report references).

- [ ] **Step 5: Populate `agent_chip_rects`** in `src/app/render.rs` while laying out the agents row: for each chip, push `(instance_id, chip_rect)`.

- [ ] **Step 6: Add a unit test** for `set_leaf_target` in `src/ui/split.rs`:

```rust
#[test]
fn set_leaf_target_replaces_focused_leaf() {
    let t0 = AttachTarget { workspace_id: WorkspaceId(1), instance: AgentInstanceId(10) };
    let t1 = AttachTarget { workspace_id: WorkspaceId(1), instance: AgentInstanceId(11) };
    let mut state = AttachedState { tree: SplitTree::Leaf(t0), focus: vec![] };
    state.set_leaf_target(vec![], t1);
    match state.tree { SplitTree::Leaf(t) => assert_eq!(t.instance, AgentInstanceId(11)), _ => panic!() }
}
```

- [ ] **Step 7: Run, build, commit**

```bash
cargo test set_leaf_target_replaces_focused_leaf
cargo build && cargo clippy --all-targets -- -D warnings
cargo fmt
git add -A
git commit -m "feat(ui): switch focused pane to an agent via key or click"
```

---

# Phase 4 — Inter-agent messaging

## Task 14: `agent_messages` inbox CRUD

**Files:**
- Create: `src/data/messages.rs`
- Modify: `src/data/mod.rs`

(The table already exists from schema V12, Task 2.)

- [ ] **Step 1: Create `src/data/messages.rs`:**

```rust
//! Asynchronous inbox for agent-to-agent prompts. The CLI (`wsx agent send`)
//! enqueues rows; the TUI drains them on its tick and injects them into the
//! target agent's session.

use crate::data::store::{AgentInstanceId, Store, WorkspaceId};
use crate::Result;

#[derive(Debug, Clone)]
pub struct AgentMessage {
    pub id: i64,
    pub workspace_id: WorkspaceId,
    pub target_agent_id: AgentInstanceId,
    pub from_agent_id: Option<AgentInstanceId>,
    pub body: String,
}

impl Store {
    pub fn enqueue_message(
        &self,
        workspace_id: WorkspaceId,
        target: AgentInstanceId,
        from: Option<AgentInstanceId>,
        body: &str,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT INTO agent_messages (workspace_id, target_agent_id, from_agent_id, body, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![workspace_id.0, target.0, from.map(|f| f.0), body, crate::data::store::now_ms()],
        )?;
        Ok(())
    }

    pub fn undelivered_messages(&self) -> Result<Vec<AgentMessage>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, workspace_id, target_agent_id, from_agent_id, body
             FROM agent_messages WHERE delivered_at IS NULL ORDER BY id ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(AgentMessage {
                id: r.get(0)?,
                workspace_id: WorkspaceId(r.get(1)?),
                target_agent_id: AgentInstanceId(r.get(2)?),
                from_agent_id: r.get::<_, Option<i64>>(3)?.map(AgentInstanceId),
                body: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn mark_delivered(&self, id: i64) -> Result<()> {
        self.conn().execute(
            "UPDATE agent_messages SET delivered_at = ?1 WHERE id = ?2",
            rusqlite::params![crate::data::store::now_ms(), id],
        )?;
        Ok(())
    }
}
```

- [ ] **Step 2: Register** in `src/data/mod.rs`: `pub mod messages;`

- [ ] **Step 3: Write failing test** at the bottom of `src/data/messages.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::store::{NewWorkspace, Store};
    use crate::pty::session::AgentKind;

    #[test]
    fn enqueue_then_drain_then_mark_delivered() {
        let store = Store::open_in_memory().unwrap();
        let repo = store.add_repo(std::path::Path::new("/tmp/r"), "r", "wsx").unwrap();
        let ws = store.insert_workspace(&NewWorkspace {
            repo_id: repo, name: "w", branch: "wsx/w",
            worktree_path: std::path::Path::new("/tmp/r/w"), yolo: false, agent: AgentKind::Claude,
        }).unwrap();
        store.add_primary_agent(ws, AgentKind::Claude, 1).unwrap();
        let target = store.add_workspace_agent(ws, AgentKind::Codex).unwrap();

        store.enqueue_message(ws, target.id, None, "please review").unwrap();
        let pending = store.undelivered_messages().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].body, "please review");

        store.mark_delivered(pending[0].id).unwrap();
        assert!(store.undelivered_messages().unwrap().is_empty());
    }
}
```

- [ ] **Step 4: Run, build, commit**

```bash
cargo test enqueue_then_drain_then_mark_delivered
cargo build && cargo clippy --all-targets -- -D warnings
cargo fmt
git add src/data/messages.rs src/data/mod.rs
git commit -m "feat(data): add agent_messages inbox CRUD"
```

---

## Task 15: Inject identity env vars at spawn

**Files:**
- Modify: `src/pty/session.rs` (`spawn_session` ~1190; command builders set env)

- [ ] **Step 1: Thread identity into `spawn_session`.** Add two parameters (or a small `SpawnIdentity { workspace_id: i64, instance_id: i64 }` struct) and set them as env vars on the child command, alongside the existing `WSX_*_BIN` env handling:

```rust
cmd.env("WSX_WORKSPACE_ID", workspace_id.to_string());
cmd.env("WSX_AGENT_INSTANCE_ID", instance_id.to_string());
```

Pass the ids from `SessionManager::spawn` (it already has the `AgentInstanceId`; thread the `WorkspaceId` through from the caller, or derive it — the caller in `ensure_instance_session` has both).

- [ ] **Step 2: Add a smoke unit test** if the command builder is testable in isolation; otherwise rely on the integration check in Task 17. At minimum verify the build:

Run: `cargo build && cargo clippy --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 3: Commit**

```bash
cargo fmt
git add src/pty/session.rs src/app.rs
git commit -m "feat(session): inject WSX_WORKSPACE_ID and WSX_AGENT_INSTANCE_ID at spawn"
```

---

## Task 16: `wsx agent {list,send,add}` CLI

**Files:**
- Modify: `src/cli.rs` (`CliAction` enum ~5-101; `parse_args` ~155; `run_cli` dispatch ~803+)

- [ ] **Step 1: Add `CliAction` variants:**

```rust
AgentList,
AgentSend { target: String, prompt: String },
AgentAdd { kind: String },
```

- [ ] **Step 2: Parse `wsx agent <sub>`** in `parse_args` (mirror the `workspace`/`repo` subcommand parsing). `agent list` → `AgentList`; `agent send <target> <prompt>` → `AgentSend`; `agent add <kind>` → `AgentAdd`. These commands operate on the **current workspace**, resolved from cwd.

- [ ] **Step 3: Implement dispatch** in `run_cli` (mirror `WorkspaceCreate` ~803):

```rust
CliAction::AgentList => {
    let store = Store::open(&dirs.db_path())?;
    let ws = resolve_current_workspace(&store)?; // by cwd → worktree_path
    for inst in store.workspace_agents(ws.id)? {
        let tag = if inst.is_primary { " (primary)" } else { "" };
        println!("{}{}", inst.label(), tag);
    }
}
CliAction::AgentSend { target, prompt } => {
    let store = Store::open(&dirs.db_path())?;
    let ws = resolve_current_workspace(&store)?;
    let target_id = store.resolve_instance_label(ws.id, &target)?
        .ok_or_else(|| crate::Error::msg(format!(
            "no agent '{target}' in this workspace; try `wsx agent list`")))?;
    let from = std::env::var("WSX_AGENT_INSTANCE_ID").ok()
        .and_then(|s| s.parse::<i64>().ok())
        .map(crate::data::store::AgentInstanceId);
    store.enqueue_message(ws.id, target_id, from, &prompt)?;
}
CliAction::AgentAdd { kind } => {
    let store = Store::open(&dirs.db_path())?;
    let ws = resolve_current_workspace(&store)?;
    let agent = crate::pty::session::AgentKind::from_str_or_default(Some(&kind));
    let inst = store.add_workspace_agent(ws.id, agent)?;
    println!("added {}", inst.label());
}
```

`resolve_current_workspace(&store)` — add a helper that reads `std::env::current_dir()`, then finds the workspace whose `worktree_path` is a prefix of (or equal to) cwd. Prefer `WSX_WORKSPACE_ID` env when present (set at spawn), falling back to the cwd match for human CLI use. Keep this helper in `cli.rs`.

- [ ] **Step 4: Add a parse unit test** in `cli.rs` tests (mirror existing `parse_args` tests):

```rust
#[test]
fn parses_agent_send() {
    let action = parse_args(&["agent", "send", "claude#2", "hello there"]);
    assert!(matches!(action, CliAction::AgentSend { target, prompt }
        if target == "claude#2" && prompt == "hello there"));
}
```

(Match the real `parse_args` signature/return — adjust the call to however existing tests invoke it.)

- [ ] **Step 5: Run, build, commit**

```bash
cargo test parses_agent_send
cargo build && cargo clippy --all-targets -- -D warnings
cargo fmt
git add src/cli.rs
git commit -m "feat(cli): add wsx agent list/send/add"
```

---

## Task 17: TUI inbox drain & delivery

**Files:**
- Create: `src/app/messaging.rs`
- Modify: `src/app.rs` (declare `mod messaging;`; call drain in the tick path ~627-750)

- [ ] **Step 1: Write the failing test** for the pure delivery-decision logic. Design the drain so the side-effecting part (send into a session) is separated from the decision (which message → which banner). Create `src/app/messaging.rs`:

```rust
//! Drains the agent_messages inbox and delivers each message into the target
//! instance's live session, tagged so the receiver knows it is peer mail.

use crate::data::messages::AgentMessage;
use crate::data::store::Store;

/// Format the banner injected into the receiving agent. Pure + testable.
pub fn delivery_banner(from_label: Option<&str>, body: &str) -> String {
    match from_label {
        Some(f) => format!("[message from {f}]\n{body}"),
        None => format!("[message]\n{body}"),
    }
}

/// Resolve the human-readable sender label for a message (None → CLI/human).
pub fn sender_label(store: &Store, msg: &AgentMessage) -> Option<String> {
    let from = msg.from_agent_id?;
    store.workspace_agents(msg.workspace_id).ok()?
        .into_iter().find(|i| i.id == from).map(|i| i.label())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn banner_tags_sender() {
        assert_eq!(delivery_banner(Some("claude#2"), "hi"), "[message from claude#2]\nhi");
        assert_eq!(delivery_banner(None, "hi"), "[message]\nhi");
    }
}
```

- [ ] **Step 2: Run to confirm pass**

Run: `cargo test banner_tags_sender`
Expected: PASS.

- [ ] **Step 3: Implement the drain** on `impl App` in `src/app/messaging.rs` (or `app.rs`):

```rust
impl crate::app::App {
    /// Called each tick. Delivers undelivered messages into live sessions,
    /// spawning the target instance on demand. Best-effort; failures are logged.
    pub(crate) fn drain_agent_messages(&mut self) {
        let pending = match self.store.undelivered_messages() {
            Ok(p) => p,
            Err(_) => return,
        };
        for msg in pending {
            // Ensure the target session exists (spawn on demand).
            let session = match self.ensure_instance_session(msg.target_agent_id) {
                Ok(s) => s,
                Err(_) => {
                    // Target instance gone/un-spawnable: mark delivered (no-op) + log.
                    let _ = self.store.mark_delivered(msg.id);
                    continue;
                }
            };
            let from_label = crate::app::messaging::sender_label(&self.store, &msg);
            let banner = crate::app::messaging::delivery_banner(from_label.as_deref(), &msg.body);
            let sess = session.clone();
            tokio::spawn(async move {
                sess.send_text_when_settled(&banner, 400, 5_000).await;
            });
            let _ = self.store.mark_delivered(msg.id);
        }
    }
}
```

- [ ] **Step 4: Call it from the tick.** In the event loop tick branch (`src/app.rs` ~627-750, the periodic tick arm), add `self.drain_agent_messages();` guarded by a low-frequency gate if every-16ms is too hot (e.g. only when `data_version()` changed, reusing the existing external-write detector at `store.rs:data_version`). Wire the module: add `mod messaging;` under the `app` module (in `src/app.rs` or `src/app/mod.rs` per the existing submodule pattern).

- [ ] **Step 5: Run, build, commit**

```bash
cargo test banner_tags_sender
cargo build && cargo test && cargo clippy --all-targets -- -D warnings
cargo fmt
git add -A
git commit -m "feat(app): drain agent message inbox and deliver on tick"
```

---

# Phase 5 — Skill

## Task 18: Document multi-agent workspaces in the wsx skill

**Files:**
- Modify: `skills/wsx/SKILL.md`

- [ ] **Step 1: Add a section** to `skills/wsx/SKILL.md` (it is embedded via `include_str!` in `src/agent/skill.rs` and installed by `wsx setup install-skill`):

```markdown
## Multi-agent workspaces

A workspace can have more than one agent attached, including more than one of
the same kind. You may be one of several agents sharing the same git worktree
and branch.

- **See your peers:** run `wsx agent list`. Agents are addressed by label —
  the first of a kind is its bare name (`claude`), additional ones get a numeric
  suffix (`claude#2`). The primary agent is marked `(primary)`.
- **Your identity:** the `$WSX_AGENT_INSTANCE_ID` environment variable holds
  your instance id; `$WSX_WORKSPACE_ID` holds the workspace id.
- **Message a peer:** `wsx agent send <label> "<message>"`. Delivery is
  asynchronous — the message is injected into the peer's session shortly after,
  tagged `[message from <you>]` so they know it came from you.
- **Add a peer:** `wsx agent send` only reaches agents already attached. To add
  one, use `wsx agent add <kind>` (or the `^x a` panel in the TUI).

**Example — a reviewer pinging the primary:**
```
wsx agent send claude "I reviewed the diff on this branch. The new retry loop in
fetch.rs has no upper bound — line 88. Can you cap it?"
```

Because all agents in a workspace share the worktree, coordinate before making
overlapping edits to the same files; prefer messaging to hand off work.
```

- [ ] **Step 2: Verify the embed still compiles** (the file is `include_str!`-ed):

Run: `cargo build`
Expected: success.

- [ ] **Step 3: Commit**

```bash
git add skills/wsx/SKILL.md
git commit -m "docs(skill): document multi-agent workspaces and inter-agent messaging"
```

---

# Final verification

- [ ] **Step 1: Full suite + lints**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```
Expected: all clean/green.

- [ ] **Step 2: Manual end-to-end smoke** (interactive — run in a real terminal, not the agent sandbox):
  1. Create a workspace with the primary agent; confirm it behaves exactly as before (no footer agents row when alone).
  2. `^x a` → add a second agent of a different kind → confirm the footer agents row appears with a color bar + label + switch key.
  3. Press the switch key (and click the chip) → confirm the focused pane retargets to that agent.
  4. Split the pane (existing keys), switch one pane to the other agent → confirm both agents are visible at once.
  5. Add a second agent of the **same** kind → confirm it is labelled `name#2`.
  6. From inside one agent, run `wsx agent list` then `wsx agent send <peer> "ping"` → confirm the peer receives `[message from <you>]\nping` within ~1s.
  7. Detach and re-attach → confirm the roster persists and the layout restores with the right agents.

- [ ] **Step 3: Update the issue / open the PR** per the user's workflow (use the `pull-request` skill).

---

## Spec coverage check

| Spec requirement | Task(s) |
|---|---|
| Add agent B to a workspace | 3, 10, 16 |
| Agent B gets Agent A's context (worktree + diff + note) | 7, 8 |
| Agent B available as if workspace created with it | 6, 8 |
| View A and B in the same workspace | 9, 12, 13 |
| Multiple same-type agents | 3 (ordinals), 8, 12 |
| Inter-agent communication (human-initiated) | 13 (focused-pane reply inherits re-key), 5 |
| Inter-agent communication (agent-initiated CLI) | 14, 15, 16, 17 |
| wsx skill instruction/tooling | 18 |
| `^x a` opens "agents" modal | 10, 11 |
| Footer agents row (color bar + name) below pinned | 12 |
| Per-agent keybind + clickable, switches view | 13 |
| Add all available agents | 10 (`a` branch) |
| SQLite inbox transport | 2 (table), 14, 17 |
| Auto numeric suffix naming | 1 (`instance_label`) |
| Switch retargets pane; split shows two | 9, 13 |
| Shared worktree | 8 (spawns in `ws.worktree_path`) |
