# Consistent Workspace Process Doctrine

**Issue:** [#108 — establish consistent workspace process](https://github.com/bakedbean/workspacex/issues/108)
**Date:** 2026-05-28

## Problem

wsx is used to develop software, and the problems it solves are rarely simple. The
agents wsx spawns (Claude Code, Pi, Hermes) should follow a consistent set of
working practices by default, rather than depending on each agent's ad-hoc
defaults or on the user re-stating expectations every session.

The issue asks for four practices to become **non-negotiable defaults** for every
developer session:

1. **Think, maximum effort, plan** until scope has been determined.
2. **Invoke the superpowers skills by default** when evaluating the initial user
   prompt — for **Claude Code and Pi**. If the task turns out not to justify that
   level of planning, the agent may discard them.
3. **Break work into logical commits per branch.** Single-commit workspaces should
   be rare — reserved for the simplest tasks.
4. **Always load and reference the wsx skill** (`skills/wsx/SKILL.md`).

The open design question from the issue: *what is the best way to convey these as
non-negotiable processes for Claude Code, Pi, and Hermes?*

## Current state

wsx already injects per-session context at spawn time, through different channels
per agent:

- **Claude Code / Pi** — via the `--append-system-prompt` CLI flag, assembled in
  `build_claude_command` (`src/pty/session.rs:661`) and `build_pi_command`
  (`src/pty/session.rs:820`). The existing rename prompt and custom instructions
  are merged at the `combined` seam (`src/pty/session.rs:768`).
- **Hermes** — has no system-prompt flag; wsx writes a delimited
  `<!-- BEGIN wsx-managed -->` … `<!-- END wsx-managed -->` block into the
  worktree's `AGENTS.md` via `prepare_hermes_workspace` (`src/pty/session.rs:1299`).
  The content is chosen by `compose_injected_prompt` (`src/pty/session.rs:1070`).

Spawn behaviour varies by `SpawnMode` (`src/pty/session.rs:290`):
- `Fresh` — new session (optionally with rename context + custom instructions).
- `Continue` — resume the prior session.
- `ProjectManager` — the multi-workspace reviewer/summarizer; has its own
  `pm_system_prompt` and is the only mode that may enable Claude's `fastMode`.

The wsx skill is installed to `~/.claude/skills/wsx/SKILL.md` via
`wsx setup install-skill` (`src/skill.rs`) and relies on Claude's own skill
discovery. It is **not** force-referenced at launch, and Pi/Hermes never see it.

Configurable settings live in the store and are gated by `known_setting_key`
(`src/cli.rs:127`); examples include `custom_instructions`, `coding_agent`,
`pm_fast_mode`, `remote_control`.

## Decision

Deliver the four practices as a wsx-injected **process doctrine**: a source-defined
default text, overridable by a config setting, composed into each agent's existing
injection channel at spawn time. Enforcement is achieved by injecting at the
lowest level (the command builders / AGENTS.md composer), not by trusting callers.

### Resolved decisions

| Question | Decision |
|---|---|
| Where the doctrine lives / enforcement | Source-defined default, **injected at launch**, with a **config override**. |
| Hermes & superpowers | **Match the issue exactly** — superpowers clause for Claude/Pi only; Hermes gets the other three practices. |
| Spawn-mode scope | **Fresh + Continue**, never ProjectManager. |
| Expression of think/effort/plan | **Prose directives** + wire any genuine flags. (See §"Flag wiring" — there are essentially none.) |
| Override semantics | The config value **replaces** the built-in default verbatim (not appended). A `off`/`none`/`disabled` sentinel disables injection entirely; blank restores the default. |
| Pi/Hermes loading the wsx skill *content* | **Out of scope** (follow-up). The doctrine references the skill; materializing its content for non-Claude agents is deferred. |

## Design

### 1. Doctrine source

A new module `src/doctrine.rs` exposes:

```rust
pub fn process_doctrine(agent: AgentKind) -> String
```

It returns const-backed default text encoding the four practices. The text is
**agent-tailored**:

- **Claude / Pi** variants include the superpowers clause (#2).
- **Hermes** variant omits the superpowers clause; the other three practices are
  identical.

The doctrine is framing/standing guidance, written in the second person, e.g.:

- "Before determining scope, think hard and plan. Treat planning as the default,
  not the exception; apply maximum effort until scope is clear."
- (Claude/Pi only) "Use the superpowers skills by default when evaluating the
  initial request. If the task turns out not to need that level of planning, you
  may discard them."
- "Break work into logical commits on this branch. A workspace with a single
  commit should be rare — reserved for the simplest tasks."
- "Load and follow the wsx skill — it is authoritative for workspace and
  cross-repo operations in this environment."

(Exact wording finalized during implementation; the above captures intent.)

### 2. Config override

Add `process_doctrine` to `known_setting_key` (`src/cli.rs:127`) so it is settable
via `wsx config set process_doctrine <value>` (literal or `--file`, using the
existing `ValueSource` plumbing).

Resolution (mirrors the `custom_instructions` pattern):

```text
effective_doctrine(agent) =
    store.get_setting("process_doctrine")   // replaces default verbatim, all agents
    .unwrap_or_else(|| process_doctrine(agent))   // agent-tailored default
```

When an override is set it applies to **all agents** verbatim (agent-tailoring is a
property of the default only).

**Disable sentinel.** Setting `process_doctrine` to one of `off` / `none` /
`disabled` (case-insensitive, trimmed) suppresses injection entirely — the
resolver returns `None` and no doctrine reaches the agent. A blank/whitespace
value is still treated as unset (restores the default), so blanking is *not* an
off switch; the sentinel is. `resolve_effective_doctrine` therefore returns
`Option<String>` (`None` = disabled), which the spawn call site threads straight
into the `doctrine` field.

### 3. Injection points — Fresh + Continue only, never PM

- **Claude / Pi** (`build_claude_command`, `build_pi_command`): the resolved
  doctrine is composed into the `--append-system-prompt` content, ordered **before**
  the rename prompt and custom instructions:

  ```text
  {doctrine}\n\n{rename_prompt?}\n\n{custom_instructions?}
  ```

  Rationale: the doctrine is standing framing; the rename prompt is the immediate
  first action and should remain prominent after it.

- **Hermes** (`compose_injected_prompt`): the resolved doctrine is prepended into
  the `<!-- BEGIN wsx-managed -->` AGENTS.md block, ahead of rename/custom content,
  same ordering.

- **ProjectManager**: unchanged. `pm_system_prompt` is returned as-is; the doctrine
  is **not** injected. The reviewer role does not "break work into logical commits,"
  and PM is the one mode permitted to use `fastMode`.

### 4. Threading

The command builders are currently pure functions (no store access), which keeps
them unit-testable. To preserve that:

- Resolve the effective doctrine at the **spawn call sites**, where `SpawnMode`,
  `AgentKind`, and the store are already in scope.
- Pass it into the builders / composer as a new parameter `doctrine: Option<&str>`
  — `Some(text)` for Fresh/Continue, `None` for ProjectManager.
- Each builder composes the doctrine per §3 only when `Some`.

This keeps the PM exclusion explicit at the call site and the builders free of
store dependencies.

### 5. Flag wiring (honest scope)

The codebase audit found **no genuine thinking/effort/planning CLI flags** on any
of the three agents:

- Claude Code: effort/thinking are interactive; no CLI flag. No `--plan` flag.
- Pi: exposes only model/provider selection.
- Hermes: exposes only model/provider.

The only real flag in this space is Claude's `fastMode`, which is enabled **only**
in ProjectManager mode and is the opposite of "maximum effort." Therefore the
"wire real flags" requirement reduces to a single guarantee:

> Developer sessions (Fresh/Continue) must never enable `fastMode`.

This already holds (fastMode is gated to `SpawnMode::ProjectManager` at
`src/pty/session.rs:760`). We lock it in with a regression test. All other
practices are expressed as prose directives — which is also why a single injected
doctrine string is the right delivery mechanism.

### 6. wsx skill loading across agents

- **Claude**: `~/.claude/skills/wsx/SKILL.md` is auto-discovered. The doctrine
  reinforces "load and follow the wsx skill."
- **Pi / Hermes**: no guaranteed skill-discovery path. The doctrine carries the
  directive to treat the wsx skill as authoritative and points at its location.

Materializing the *actual* SKILL.md content into the worktree for Pi/Hermes is a
**follow-up**, explicitly out of scope here.

## Testing

- `process_doctrine(agent)`: superpowers clause present for Claude/Pi, absent for
  Hermes; the other three practices present for all.
- `build_claude_command` / `build_pi_command`: include the doctrine in
  `--append-system-prompt` for Fresh and Continue; exclude it for ProjectManager;
  doctrine precedes rename/custom content.
- `compose_injected_prompt` (Hermes): doctrine present for Fresh/Continue, absent
  for ProjectManager.
- Config override: when `process_doctrine` setting is present, it replaces the
  default verbatim for every agent.
- Regression: `fastMode` is enabled only in ProjectManager mode.

## Out of scope

- The contents of the superpowers or wsx skills.
- Per-repo doctrine (the override is global, like other settings).
- Materializing wsx skill content into worktrees for Pi/Hermes (follow-up).
- Adding real "effort/thinking/planning" flags to any agent (none exist to wire).
