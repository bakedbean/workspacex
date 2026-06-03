# Multi-Agent Workspaces — Design

**Issue:** [#137 — multi-agent workspaces](https://github.com/bakedbean/workspacex/issues/137)
**Date:** 2026-06-03
**Status:** Approved (design); pending implementation plan

## Summary

Today a wsx workspace is bound to exactly one agent, chosen at creation. This
feature lets a user attach **additional agents** to an existing workspace —
including a second agent of the same kind (two Claudes) — so a peer agent can
review, audit, or assist on the work already in flight. Added agents share the
workspace's worktree and branch, are given lightweight context about the work,
can be viewed alongside the primary agent, and can exchange prompts with each
other.

The unifying concept is the **agent instance**: a specific agent attached to a
workspace. The workspace's original agent becomes its *primary* instance. Every
downstream system — sessions, panes, footer, messaging — keys off the instance
rather than the workspace.

## Decisions (locked during brainstorming)

| Decision | Choice | Rationale |
|---|---|---|
| Worktree model | **Shared** worktree/branch for all agents | Matches the review/audit use case; an added agent sees the primary's work directly. |
| Context handoff | **Worktree + git diff + injected task note** | Uniform across all four agent types; no transcript export or summarization step. |
| Messaging | **Both** human-initiated and agent-initiated (CLI) | Full scope per the issue. |
| Spec scope | **One holistic spec** | Covers data, runtime, UI, and messaging together. |
| Agent-CLI transport | **SQLite inbox table** drained on the TUI tick | Reuses existing `Store`/`state.db`; durable, inspectable, no socket lifecycle. |
| Same-type naming | **Auto numeric suffix** (`claude`, `claude#2`) | Stable, terse, maps to footer keys; single derivation function. |
| Switch vs. split | **Switch retargets the focused pane; split shows two** | Reuses the existing vim-split tree. |

## Non-goals (v1)

- No concurrency control for simultaneous writes to the shared worktree. Added
  agents are framed (via the context note) for a review/coordination posture.
  This is a **documented limitation**, not a solved problem (see Risks).
- No transcript export or cross-agent history replay. Context is reconstructed
  from the filesystem + git.
- No cross-*workspace* agent messaging. Messaging is scoped within a workspace.
- No automatic agent spawning on workspace creation beyond today's single
  primary agent.

---

## Architecture overview

```
                        ┌──────────────────────────────────────┐
                        │ workspace_agents (NEW table)          │
                        │  id, workspace_id, agent, ordinal,    │
                        │  is_primary, session_ref, created_at  │
                        └──────────────────────────────────────┘
                                     │ AgentInstanceId
            ┌────────────────────────┼─────────────────────────┐
            ▼                        ▼                          ▼
   SessionManager            SplitTree::Leaf            footer agents row
   HashMap<AgentInstanceId,  (AttachTarget{ws,inst})    + AgentsPanel modal
           Arc<Session>>            │
            │                       │
            ▼                       ▼
   spawn_session(agent)     switch retargets leaf.instance
   (+context note for                │
    added instances)                 ▼
            │              wsx agent send <label> "..."
            ▼                        │ inserts row
   $WSX_AGENT_INSTANCE_ID   ┌────────────────────────┐
   $WSX_WORKSPACE_ID  ◄─────│ agent_messages (NEW)    │
   injected at spawn        └────────────────────────┘
                                     │ drained on 16ms tick
                                     ▼
                          send_text_when_settled(banner + body)
```

---

## 1. Data model

### 1.1 New table: `workspace_agents`

Schema migration **V12** (`PRAGMA user_version = 12`; current head is 11),
added to `src/data/store.rs` following the existing `SCHEMA_V*` constant +
`migrate()` step pattern.

```sql
CREATE TABLE workspace_agents (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id  INTEGER NOT NULL REFERENCES workspaces(id),
    agent         TEXT    NOT NULL,            -- 'claude' | 'pi' | 'hermes' | 'codex'
    ordinal       INTEGER NOT NULL,            -- 1-based, per (workspace, agent)
    is_primary    INTEGER NOT NULL DEFAULT 0,  -- the workspace-creation agent
    session_ref   TEXT,                        -- per-instance native session id (see §2.3)
    created_at    INTEGER NOT NULL,
    UNIQUE(workspace_id, agent, ordinal)
);
CREATE INDEX idx_workspace_agents_ws ON workspace_agents(workspace_id);
```

**Backfill (part of the V12 migration):** for every existing workspace, insert
one row from `workspaces.agent` with `is_primary = 1, ordinal = 1, created_at =
workspaces.created_at`.

### 1.2 `workspaces.agent` is retained

The existing `workspaces.agent` column stays as a **denormalized pointer to the
primary instance's kind**. Many single-agent code paths (workspace creation,
dashboard rows, the primary spawn) legitimately only care about the primary;
keeping the column avoids rewriting them to join. The `workspace_agents` table
is authoritative for the *set*; the column is a convenience for the *primary*.

**Invariant:** all roster mutations go through one module (§1.4) so the mirror
cannot drift. When the primary is created, both the column and the
`is_primary` row are written together.

### 1.3 New types

In `src/data/store.rs` near `WorkspaceId`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AgentInstanceId(pub i64);

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
```

### 1.4 New module: `src/data/agents.rs`

The single home for roster CRUD and the **label derivation** that the footer,
CLI, and message banners all share:

```rust
/// `claude` for ordinal 1, `claude#2` for ordinal >= 2.
pub fn instance_label(agent: AgentKind, ordinal: i64) -> String;

// Store methods (impl on Store, declared here):
fn workspace_agents(ws: WorkspaceId) -> Result<Vec<AgentInstance>>;       // ordered: primary first, then by created_at
fn add_workspace_agent(ws: WorkspaceId, agent: AgentKind) -> Result<AgentInstance>;  // computes next ordinal
fn remove_workspace_agent(id: AgentInstanceId) -> Result<()>;            // refuses if is_primary
fn set_instance_session_ref(id: AgentInstanceId, r: &str) -> Result<()>;
fn resolve_instance_label(ws: WorkspaceId, label: &str) -> Result<Option<AgentInstanceId>>;  // for the CLI
```

`instance_label` is the **single source of truth** for an instance's name — a
correctness property, since the footer, `wsx agent send`, and the delivered
banner must all agree on what `claude#2` is called.

---

## 2. Runtime: sessions per instance & context handoff

### 2.1 Re-key the session layer

`SessionManager` (in `src/pty/session.rs`) changes its key from `WorkspaceId`
to `AgentInstanceId`:

```rust
struct SessionManager {
    sessions: HashMap<AgentInstanceId, Arc<Session>>,
    pm: Option<Arc<Session>>,   // unchanged
}
```

### 2.2 Migration helpers (strangler-fig)

To keep the re-key a mechanical, reviewable migration rather than a big-bang
rewrite, add two helpers on `App`:

```rust
fn primary_instance(&self, ws: WorkspaceId) -> AgentInstanceId;   // the workspace's main agent
fn session_for(&self, inst: AgentInstanceId) -> Option<&Arc<Session>>;
```

Most existing `sessions.get(ws_id)` call sites become
`session_for(primary_instance(ws_id))`. The genuinely multi-agent paths (panes,
switching, messaging) use `AgentInstanceId` directly. The Rust type change makes
the compiler enumerate every call site that needs migration.

### 2.3 Spawning an added instance

Reuses `spawn_session(agent, …)` unchanged — it already takes `AgentKind`. The
only subtlety is **session resume in a shared worktree**, where today's
resume-by-cwd detection (`has_prior_session_for(worktree, agent)`) is ambiguous
for two same-kind agents in one directory.

Resolution (generalizes the existing Hermes `.git/info/wsx-hermes-spawn-at`
marker pattern):

- **Primary instance:** unchanged behavior — native continue/fresh by cwd. Zero
  regression for existing single-agent workspaces.
- **Added instance:** spawns **fresh** the first time, with the injected context
  note. At spawn, capture its native session id and persist to
  `workspace_agents.session_ref`. On re-attach, resume *that* session by ref.
  This makes per-instance resume work even with N instances sharing one cwd.

Per-agent `session_ref` capture mirrors how each agent already detects sessions
(`src/pty/session.rs` has agent-specific session-location logic for Claude
JSONL, Pi JSONL, Hermes SQLite, Codex dated files); the capture hook is added
alongside each.

### 2.4 Context handoff — `src/agent/handoff.rs` (new)

Builds the `custom_instructions` string fed into the existing
`SpawnMode::Fresh { custom_instructions }` → `--append-system-prompt` seam (the
same seam `src/agent/doctrine.rs` uses). Contents:

1. **Role framing:** "You are joining an existing wsx workspace as an additional
   agent alongside `<primary-label>` (the primary). You share the same worktree
   and branch."
2. **Concrete pointers:** branch name, base ref, and an instruction to run
   `git diff <base>...HEAD` and read the working tree to see work in progress.
3. **Messaging pointer:** how to talk to peers (see §4), so the added agent
   knows it can respond.

No transcript export, no summarization. The added agent reconstructs context
from the shared filesystem + git — uniform across all four agent types.

---

## 3. UI

### 3.1 Generalize the split tree — `src/ui/split.rs`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttachTarget {
    pub workspace_id: WorkspaceId,
    pub instance: AgentInstanceId,
}

pub enum SplitTree {
    Leaf(AttachTarget),                       // was Leaf(WorkspaceId)
    Split { direction: SplitDirection, children: Vec<SplitTree> },
}
```

`restore_attached_state` (`src/app.rs`) and the pane-data aggregation
(`src/app/render.rs`) resolve a leaf's `instance` → session + label + agent kind.
The existing layout-persistence (schema V10 `workspace_layouts`) must persist
the instance dimension; migration writes the primary instance for existing saved
layouts.

- **Switching** (`^x` + pill key, or click) retargets the focused leaf's
  `instance` in place.
- **Splitting** uses the existing vim-split keys, unchanged. To view A and B at
  once: split, then switch one pane to the other agent.

The pane title bar and footer color bar already key off `AgentKind`
(`theme.agent_style`), so a pane self-colors once its target resolves.

### 3.2 Footer agents row — `src/ui/attached.rs`

A new row **below** the pinned-command row, rendered only when the workspace has
more than its primary agent (single-agent workspaces are unchanged):

```
 ▎ multi-agent-workspaces                    ^x    1 build   2 test   3 lint
 agents:  ▎claude  q   ▎claude#2  w   ▎codex  e
```

- Each pill reuses the existing chip-styling helpers: agent color bar (`▎` in
  `theme.agent_style`) + label + switch key.
- **Key assignment:** pinned commands own `^x 1-9`; `^x a` opens the panel;
  agent switch keys draw from a reserved-safe letter pool (`q w e r t y …`,
  skipping `a` and any already-bound key). The pool + assignment live in **one
  helper** so footer rendering and key dispatch cannot disagree.
- Pills are **clickable** — add `agent_chip_rects: Vec<(AgentInstanceId, Rect)>`
  to `App`, mirroring the existing `chip_rects` / `attached_pane_rects`
  hit-testing.
- `layout_chrome` (`src/ui/attached.rs`) gains one conditional row in its
  vertical constraint stack for the agents row.

### 3.3 The `^x a` panel — `Modal::AgentsPanel` (new, `src/ui/modal.rs`)

A sibling of the existing `Modal::AgentPicker` (which *replaces* a workspace's
agent — different intent). Reuses `centered()` layout + list-rendering style but
not the replace-semantics.

```
┌─ agents ───────────────────────────────────┐
│ Attached:                                   │
│   ▎ claude     (primary)                    │
│   ▎ claude#2                                 │
│                                             │
│ Add:                                        │
│  > ▎ claude     ▎ pi    ▎ hermes   ▎ codex  │
│                                             │
│  Enter add   a add all   x remove   Esc     │
└─────────────────────────────────────────────┘
```

- `Enter` adds the selected kind (duplicates allowed → next ordinal).
- `a` adds **all available** kinds.
- `x` removes the highlighted attached non-primary instance (primary cannot be
  removed).
- Adding triggers the spawn-with-context flow (§2.3–2.4). If the binary is
  missing, reuse the existing `Modal::AgentMissing` on attach.

### 3.4 Input wiring — `src/app/input.rs`

- The `^x` leader chord (`LEADER_KEY`) gains an `a` branch → open
  `Modal::AgentsPanel`, alongside the existing `1-9` pinned-command branch.
- The `^x` + letter branch fires an agent switch via the shared key-pool helper.
- `AgentsPanel` modal key handling mirrors the existing `AgentPicker` handler
  structure.

---

## 4. Inter-agent messaging

Both directions land on the same delivery primitive,
`Session::send_text_when_settled` (already used by the detail bar and PM).

### 4.1 Human-initiated (inherited from the re-key)

Once sessions are per-instance, the existing detail-bar reply already targets
the **focused** session — now a specific agent instance. "Send a prompt to the
agent I'm looking at" works with no new mechanism; the reply target resolves
through `AttachTarget`.

### 4.2 Agent-initiated — SQLite inbox

New table (migration **V12**, same bump as §1.1):

```sql
CREATE TABLE agent_messages (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id    INTEGER NOT NULL,
    target_agent_id INTEGER NOT NULL REFERENCES workspace_agents(id),
    from_agent_id   INTEGER,            -- NULL = CLI/human origin
    body            TEXT NOT NULL,
    created_at      INTEGER NOT NULL,
    delivered_at    INTEGER             -- NULL until injected
);
CREATE INDEX idx_agent_messages_undelivered
    ON agent_messages(workspace_id) WHERE delivered_at IS NULL;
```

Store methods live in `src/data/messages.rs` (new):
`enqueue_message`, `undelivered_messages`, `mark_delivered`.

### 4.3 CLI surface

New `CliAction` variants in `src/cli.rs` (parsing stays in the flat `cli.rs`
file; heavy logic delegates to `src/data/agents.rs` + `src/data/messages.rs`):

- `wsx agent list` — list this workspace's agents (resolve workspace by cwd →
  `worktree_path`), printing addressable labels.
- `wsx agent send <label> "<prompt>"` — resolve `<label>` → `target_agent_id`,
  enqueue an `agent_messages` row. Returns immediately.
- `wsx agent add <kind>` — CLI parity with the panel; lets an agent recruit a
  peer.

### 4.4 Self-identity & addressing

At spawn, each instance's PTY environment gets:

- `WSX_WORKSPACE_ID` — so the CLI resolves the workspace without guessing.
- `WSX_AGENT_INSTANCE_ID` — so the CLI knows *who* is sending (→ `from_agent_id`)
  and disambiguates same-kind instances.

This reuses the env-seam pattern already used for `WSX_*_BIN`. Injecting
identity at spawn sidesteps the otherwise-unsolvable problem of an agent
inferring *which* same-type instance it is from a shared cwd.

### 4.5 Delivery — `src/app/messaging.rs` (new), on the existing 16 ms tick

1. Query `undelivered_messages` for workspaces in a live state.
2. For each: ensure the target instance's session is spawned (spawn-on-demand if
   needed), then `send_text_when_settled` an attributed banner, e.g.
   `"[message from claude#2]\n<body>"`, so the receiver knows it is peer mail,
   not user input.
3. Stamp `delivered_at`. Messages to a removed instance are marked delivered as a
   no-op and logged.

Because the queue is a table the TUI already polls, there is no new event loop or
socket lifecycle, and undelivered mail survives a TUI restart. Latency is at most
one tick (16 ms) — irrelevant for agent-to-agent prose.

### 4.6 wsx skill update — `skills/wsx/SKILL.md`

Add a "Multi-agent workspaces" section (the file is embedded via `include_str!`
and installed by `wsx setup install-skill`). It teaches an agent:

- You may be one of several agents in a workspace, sharing the worktree/branch.
- Discover peers: `wsx agent list`. Your own identity is in
  `$WSX_AGENT_INSTANCE_ID`.
- Send peer mail: `wsx agent send <label> "<prompt>"` — delivered asynchronously
  into the peer's session, tagged with your label.
- When/why to use it, with a worked example (a reviewer agent pinging the primary
  about a finding).

---

## 5. Module organization

| Concern | Location | New / changed |
|---|---|---|
| Roster CRUD + label derivation | `src/data/agents.rs` | **new** |
| Message inbox CRUD | `src/data/messages.rs` | **new** |
| Context-note builder | `src/agent/handoff.rs` | **new** |
| TUI inbox drain/delivery | `src/app/messaging.rs` | **new** |
| `AgentInstanceId`, `AgentInstance` types | `src/data/store.rs` | changed |
| Schema V12 (both tables + backfill) | `src/data/store.rs` | changed |
| `SessionManager` re-key + `session_ref` capture | `src/pty/session.rs` | changed |
| `primary_instance` / `session_for` helpers | `src/app.rs` | changed |
| `AttachTarget` leaf + layout persistence | `src/ui/split.rs`, `src/app.rs`, `src/app/render.rs` | changed |
| Footer agents row + `agent_chip_rects` | `src/ui/attached.rs`, `src/app.rs` | changed |
| `Modal::AgentsPanel` | `src/ui/modal.rs` | changed |
| `^x a`, switch keys, panel input | `src/app/input.rs` | changed |
| `wsx agent {list,send,add}` parsing | `src/cli.rs` | changed |
| Skill doc | `skills/wsx/SKILL.md` | changed |

Design principle: new behavior goes in **new, focused modules**; existing files
change only where the type re-key or render integration genuinely requires it.
The CLI commands stay thin (parse → call store/`agents`/`messages`), keeping
business logic out of `cli.rs`.

---

## 6. Error handling

- **Added agent binary missing:** reuse the existing `Modal::AgentMissing` flow
  on attach.
- **`wsx agent send` to an unknown label:** CLI exits non-zero with the list of
  valid labels.
- **Message to a since-removed instance:** marked delivered (no-op) and logged.
- **Removing the primary:** rejected at the store layer (`remove_workspace_agent`
  refuses `is_primary`).
- **Adding a duplicate kind:** allowed; `add_workspace_agent` computes the next
  `ordinal`.

---

## 7. Testing strategy

**Unit:**
- `instance_label` derivation (ordinal 1 vs. ≥2).
- V12 migration: backfill produces exactly one primary row per workspace; idempotent re-`migrate()`.
- `add_workspace_agent` ordinal computation across duplicates; `remove_workspace_agent` refuses primary.
- Inbox `enqueue` → `undelivered_messages` ordering → `mark_delivered`.
- `AttachTarget` resolution to (session, label, kind).
- Footer agents-row span construction (mirrors the existing `title_bar_spans` unit tests).
- Context-note builder output shape (`src/agent/handoff.rs`).
- Switch-key pool assignment is collision-free against pinned/leader keys.

**Integration / behavioral:**
- Add an agent → roster grows; footer row appears; layout persists with instance.
- `wsx agent send` inserts a row; the drain logic (tested against a mock delivery
  sink, no live PTY) injects an attributed banner and stamps `delivered_at`.
- Re-attach an added instance → resumes via `session_ref` rather than starting
  fresh.

---

## 8. Risks & limitations

- **Concurrent shared-worktree writes (primary risk).** Two agents editing the
  same files can clash. v1 does not lock or coordinate; it relies on the context
  note's review/coordination framing. If this proves painful, a follow-up could
  add advisory file-level coordination or an isolated-worktree-per-agent mode.
- **Per-instance `session_ref` capture is agent-specific.** Each of the four
  agents stores/locates sessions differently; the capture hook must be
  implemented per agent. Claude/Pi (JSONL by cwd) and Codex (dated files) need a
  "newest session after spawn" capture; Hermes already has a marker to
  generalize. If capture fails for an agent, that instance falls back to
  always-fresh (degraded but functional).
- **Footer width.** Many agents + many pinned commands could overflow narrow
  terminals. The agents row truncates/elides like the pinned row does.
- **`workspaces.agent` denormalization** can drift if a roster write bypasses
  `src/data/agents.rs`. Mitigation: that module is the only sanctioned writer.

---

## 9. Implementation phasing (for the plan)

Although specified holistically, the natural build order is:

1. **Data foundation** — V12 migration, `workspace_agents`, `AgentInstanceId`,
   `src/data/agents.rs`, backfill. (No behavior change; existing tests green.)
2. **Session re-key** — `SessionManager` keyed by instance, `primary_instance` /
   `session_for` helpers. (Still single-agent behavior; pure refactor.)
3. **Add + view** — `AgentsPanel` modal (`^x a`), `AttachTarget` leaf, footer
   agents row, switch keys/clicks, context handoff (`handoff.rs`), `session_ref`
   capture.
4. **Messaging** — `agent_messages` table, `src/data/messages.rs`,
   `src/app/messaging.rs` drain, `wsx agent {list,send,add}`, spawn-env identity.
5. **Skill** — `SKILL.md` multi-agent section.

Each phase ends green and is independently reviewable; phases 1–2 are
behavior-preserving refactors that de-risk the feature work in 3–5.
