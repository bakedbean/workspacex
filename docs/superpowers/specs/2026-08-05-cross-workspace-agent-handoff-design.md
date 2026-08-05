# Cross-workspace agent handoff

**Date:** 2026-08-05
**Status:** approved

## Goal

When an agent decides work belongs on a new branch, it should create a wsx
workspace and hand the task to *that workspace's own agent*, instead of
`cd`-ing into the new worktree and continuing to drive it from the originating
session.

Today the opposite is prescribed. `src/agent/related.rs:57` instructs the agent
to run `wsx workspace path` and "`cd` there to make changes, commit, and push",
and `skills/wsx/SKILL.md:91` reinforces it ("Staying in the same Claude session
… usually the right call"). The result: the new workspace shows no agent
activity on the dashboard, and the originating session accumulates history for
work that was deliberately moved elsewhere.

## What already works

The message-delivery path is already workspace-agnostic. No new transport is
needed.

- `Store::undelivered_messages` (`src/data/messages.rs:33`) has **no** workspace
  filter.
- `App::drain_agent_messages` (`src/app/messaging.rs:46`) groups by
  `target_agent_id` and calls `ensure_instance_session(target)`
  (`src/app.rs:1887`), which derives the workspace from the *instance row* via
  `workspace_agents_by_id` — a direct DB lookup — and will **cold-spawn** a
  session that isn't running yet.
- The drain is gated on `poll_external_changes()` (`src/app.rs:715`), which
  calls `refresh()` before returning true. A workspace created seconds earlier
  by a sibling CLI process is therefore already in `app.workspaces` by the time
  `build_spawn_info` (`src/app.rs:1592`) reads the cached list.
- `wsx workspace create` (`src/cli.rs:1779`) already inserts the primary agent
  instance row via `add_workspace_agent` (`src/data/workspace.rs:91`) even
  though the CLI never launches a PTY. That row is addressable immediately.

The only thing preventing handoff is CLI-side resolution: `AgentSend`
(`src/cli.rs:1894`) calls `resolve_current_workspace` and then scopes label
lookup to that workspace via `resolve_instance_label(ws.id, …)`.

## Design

Three layers, changed together. The behavior only becomes reliable when the
doctrine (always injected), the skill (consulted before acting), and the CLI
(the capability) all agree.

### 1. CLI — address agents in another workspace

```
wsx agent send [--workspace <repo>/<slug>] <label> <message...>
```

**Parsing** (`parse_agent`, `src/cli.rs:1105`). Flags are recognized only
*before* the label; everything after the label is joined verbatim as the body,
preserving today's behavior for messages that themselves begin with `--`.
`CliAction::AgentSend` gains `workspace: Option<String>`.

**Spec resolution.** New `resolve_workspace_spec(store, spec)` splits on the
**last** `/` — repo names may contain spaces, slugs never contain `/` (same
assumption as `tui_ipc::parse_line`, `src/tui_ipc.rs:47`). Errors name what was
not found and list the valid alternatives:

- no `/` in the spec → `--workspace expects <repo>/<slug>, got '<spec>'`
- unknown repo → lists registered repo names
- unknown slug → lists that repo's workspace slugs

**The `primary` label alias.** `resolve_instance_label`
(`src/data/agents.rs:191`) gains a reserved alias: the literal label `primary`
resolves to the workspace's primary instance. This removes the handoff's one
real footgun — the sender cannot run `wsx agent list` against another
workspace, so it would otherwise have to guess the target's agent-kind label. A
freshly created workspace has exactly one agent and it is primary, so `primary`
is always correct there. No `AgentKind::display_name()` is `primary`, so the
alias cannot collide.

**Dispatch.** Target-workspace and label resolution move together; enqueue is
otherwise unchanged:

```rust
let target_ws = match workspace.as_deref() {
    Some(spec) => resolve_workspace_spec(&store, spec)?,
    None => resolve_current_workspace(&store)?,
};
let target_id = store.resolve_instance_label(target_ws.id, &target)?...;
let from = /* WSX_AGENT_INSTANCE_ID, unchanged */;
store.enqueue_message(target_ws.id, target_id, from, &prompt)?;
```

`agent_messages.workspace_id` keeps its current meaning — the workspace the
message is delivered *to*. Nothing about the column's semantics changes.

### 2. Sender labelling across workspaces

`sender_label` (`src/app/messaging.rs:16`) currently resolves the sender by
scanning `workspace_agents(msg.workspace_id)`, which only works when sender and
target share a workspace. It switches to the global instance lookup
`workspace_agents_by_id(from)` — which also yields the sender's `workspace_id`,
giving us the qualification for free:

- sender's workspace **==** `msg.workspace_id` → bare label, e.g. `claude`
  (unchanged for every existing intra-workspace message)
- sender's workspace **!=** `msg.workspace_id` → qualified,
  e.g. `workspacex/parent-task claude`

`delivery_banner` (`src/app/messaging.rs:8`) is unchanged — it stays a pure
function over an already-formatted label. A handoff therefore lands in the new
session as:

```
[message from workspacex/parent-task claude]
TASK: …
```

Naming the origin is load-bearing: the receiving agent needs to know where the
work came from in order to report back or read the originating branch.

### 3. Warn when no dashboard can deliver

Delivery only happens inside a running `wsx` TUI tick. With no dashboard up, a
queued handoff sits in `agent_messages` indefinitely and the new workspace never
starts — silently, from the sender's point of view.

New `tui_ipc::any_live_tui() -> bool`: iterate `live_socket_candidates()`
(`src/tui_ipc.rs:24`) and attempt `std::os::unix::net::UnixStream::connect` on
each. A live listener accepts; a stale socket file left by a dead process
refuses. First success wins.

`AgentSend` calls it after a successful enqueue and, when false, writes to
**stderr**:

```
warning: no wsx dashboard is running — this message is queued and will not be
delivered until one starts. Tell the user to open `wsx`.
```

Exit code stays 0: the row is queued, not lost. The wording addresses an agent,
because an agent is the reader.

### 4. Doctrine (`src/agent/doctrine.rs`)

New `CLAUSE_HANDOFF`, pushed in `process_doctrine` after `CLAUSE_WSX_SKILL`
(so the skill pointer is established first) and before `CLAUSE_STATUS`. It
covers both directions of the handoff:

> - **Start a new workspace instead of a new branch.** When the work ahead needs
>   a new branch, or shifts to a concern independent enough that this session's
>   history would be noise, do not branch here — create a workspace and hand the
>   task to its own agent:
>   `wsx workspace create <repo> --name <slug>`, then
>   `wsx agent send --workspace <repo>/<slug> primary "<brief>"`.
>   Always pass `--name`; an unnamed workspace forces the new agent to rename it
>   before it can start. The brief is the receiving agent's *only* context:
>   state the task and what done looks like, why it exists, the decisions and
>   `file:line` pointers it needs, the constraints, and the first concrete step.
>   Write it so it still makes sense if this session were deleted. Then tell the
>   user which workspace is now working on what and return to your own task — do
>   **not** `cd` into the new worktree and work there yourself.
> - **If your first input is a handoff brief from another workspace's agent,
>   that brief is your task.** Set `wsx recap set --goal` from it before you
>   start.

Trigger wording is deliberately asymmetric: *needs a new branch* is a hard
trigger; *independent enough that history would be noise* is a soft one that
requires the work to genuinely stand alone. Without that asymmetry the clause
spawns a workspace per subtask.

Applies to all four agent kinds — it is not gated like `CLAUSE_SUPERPOWERS`.

### 5. Related-repos prompt (`src/agent/related.rs:34`)

`build_read_only_prompt` steps 2–3 are rewritten. The read-only warning and the
slug rules in step 1 stay as they are.

- Step 2 becomes the brief-and-hand-off command, replacing "`cd` there to make
  changes, commit, and push".
- Step 3 becomes: tell the user which workspace has the sibling task; do not
  work in it from this session.
- The two-PRs / cross-link / merge-order guidance is retained.
- The closing paragraph ("Workspaces in different repos do not share Claude
  session state") is reframed rather than dropped: the brief is now the handoff
  channel for the initial task; commits and PR bodies remain the channel for
  anything that must outlive either session.

`wsx workspace path` stays documented as a way to *read* the sibling worktree,
not as the prelude to working in it.

### 6. Skill (`skills/wsx/SKILL.md`)

- New `## Handing off to a new workspace` section: the two triggers, the two
  commands, the brief template, and one worked end-to-end example.
- `## CLI surface` gains the `--workspace` form and the `primary` alias.
- `## Cross-repo orchestration` step 3 is inverted to match `related.rs`.
- `## Common mistakes` gains: **Driving a workspace you created.** Creating a
  workspace and then `cd`-ing into it defeats the point — the dashboard shows it
  idle and the history piles up in the wrong session.

The brief template the skill teaches:

```
TASK:        what to build or fix, and what done looks like
WHY:         the decision or finding that led here
CONTEXT:     contracts, types, names, file:line pointers — anything decided in
             my session that is not yet in the repo
CONSTRAINTS: don't touch X; follow the pattern at path:line; merge after PR #N
START:       the first concrete step
```

with the governing rule stated plainly: **the brief must survive the
originating session being deleted.**

**Installation note.** `~/.claude/skills/wsx/SKILL.md` is a plain copy written
by `wsx setup install-skill` (`src/agent/skill.rs:16`, `src/cli.rs:1372`), not a
symlink, and it is already stale — it predates the recap section. Landing this
work requires re-running `wsx setup install-skill` from a build of the branch,
which is a manual step, not something the change automates.

## Deliberately not doing

- **No `--brief` flag on `workspace create`, and no `wsx handoff` verb.** Two
  existing commands compose into the flow, and cross-workspace `send`
  generalizes to re-briefing and report-back; a third way to create a workspace
  does not.
- **No brief persistence.** The brief is a queued message, like every other
  agent message. It is not stored as a first-class record, not shown in the TUI,
  and not re-readable after compaction.
- **No automated report-back.** The originating agent hands off, tells the user,
  and returns to its own work. The user coordinates via the dashboard.
- **No headless delivery.** The PTY lifecycle lives entirely in the TUI process;
  moving it is out of scope. The warning covers the gap.

## Components touched

| File | Change |
|---|---|
| `src/cli.rs` | `--workspace` parsing; `resolve_workspace_spec`; `AgentSend` dispatch; no-TUI warning |
| `src/data/agents.rs` | `primary` alias in `resolve_instance_label` |
| `src/app/messaging.rs` | `sender_label` → global instance lookup + cross-workspace qualification |
| `src/tui_ipc.rs` | `any_live_tui()` |
| `src/agent/doctrine.rs` | `CLAUSE_HANDOFF` |
| `src/agent/related.rs` | rewrite `build_read_only_prompt` steps 2–3 and closing paragraph |
| `skills/wsx/SKILL.md` | handoff section, CLI surface, cross-repo step 3, common mistakes |
| `docs/book/src/configuration/multi-agent-workspaces.md` | document `--workspace`, the `primary` alias, and the live-dashboard requirement |

## Error handling

- Unknown repo or slug in `--workspace` → `Error::UserInput` listing valid
  alternatives. Nothing is enqueued.
- Unknown label in the target workspace → `Error::UserInput`. The existing
  message ("try `wsx agent list`") is misleading cross-workspace, since
  `agent list` only reports the *current* workspace; the error instead names
  the target workspace and lists its labels directly.
- No live dashboard → warning on stderr, exit 0, message stays queued.
- `sender_label` returning `None` (human-originated CLI send) → banner falls
  back to `[message]`, unchanged.
- Delivery failures are already handled by `drain_agent_messages`: transient
  errors leave messages pending for the next tick; a missing agent binary marks
  them delivered so they don't retry forever.

## Testing

Unit:

- `parse_agent`: `--workspace` accepted before the label; a body beginning with
  `--` preserved verbatim; missing value rejected.
- `resolve_workspace_spec`: last-slash split; repo names containing spaces;
  missing `/`; unknown repo; unknown slug.
- `resolve_instance_label`: `primary` resolves to the primary instance; still
  resolves ordinary labels and `claude#2`.
- `sender_label`: same-workspace sender yields the bare label (regression
  guard); cross-workspace sender yields `repo/slug label`; unknown sender yields
  `None`.
- `any_live_tui`: bound listener in a temp socket dir → true; stale socket file
  with no listener → false; empty dir → false.
- `process_doctrine`: every `AgentKind` contains `agent send --workspace` and
  the "do not `cd` into the new worktree" instruction.
- `build_read_only_prompt`: no longer instructs `cd`-and-work; contains both
  handoff commands.

Integration:

- Two workspaces in the store; enqueue from A to B's primary via the CLI action;
  assert `undelivered_messages` returns it with B's `workspace_id` and B's
  primary as target, and that the composed banner is origin-qualified.

## Commits

1. `feat(cli): address agents in other workspaces from wsx agent send` —
   `--workspace` parsing, `resolve_workspace_spec`, `primary` alias,
   `sender_label` global lookup, qualified banner.
2. `feat(cli): warn when no dashboard is live to deliver a queued message` —
   `any_live_tui` + the stderr warning.
3. `feat(agent): doctrine clause — hand new branches to a new workspace` —
   `CLAUSE_HANDOFF` and the `related.rs` rewrite.
4. `docs(skill): handing off to a new workspace` — SKILL.md and the
   multi-agent-workspaces book page.

## Risks

- **Delivery still depends on a running TUI.** Mitigated by the warning, not
  eliminated. A user who works with the dashboard closed will queue handoffs
  that never fire.
- **Doctrine growth.** `CLAUSE_HANDOFF` is the longest clause in the doctrine
  and is injected into every session. It has to earn its length; keep it tight
  and prefer pushing detail into the skill.
- **Over-triggering.** The soft "context shift" trigger is judgment-based. If it
  produces workspace sprawl in practice, tighten the wording to the hard
  branch-creation trigger alone.
- **`workspace create` runs setup scripts synchronously**
  (`setup::run_setup`, `src/data/workspace.rs`), so on a slow repo the
  originating agent blocks during handoff. Acceptable; noted so it isn't
  mistaken for a hang.
