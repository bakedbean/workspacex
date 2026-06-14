# agent-pr skill — design

**Date:** 2026-06-14
**Status:** approved-pending-implementation

## Problem

wsx can spawn peer review agents in a workspace (`wsx agent add`, `wsx agent
send`), but there is no packaged, repeatable way to stand up a *code-review*
peer and hand it the right context. The user wants a one-action flow: invoke a
skill — ideally from a pinned-command chip — that spawns a reviewer agent of a
chosen kind, gives it the branch's review context, and has it report findings
back.

## Goals

- A bundled Claude Code skill named `agent-pr` that, run in the primary agent's
  session, spawns a reviewer peer and hands off review context.
- The skill takes one argument: the reviewer kind (`claude` | `pi` | `hermes` |
  `codex`), defaulting to `claude`.
- Invocable from a pinned-command chip (`agent-pr=/agent-pr`).
- The skill ships in the binary and installs via `wsx setup install-skill`,
  exactly like the existing `wsx` skill.

## Non-goals

- No new wsx subcommand. The skill is a documented recipe over existing
  primitives (`wsx agent add` / `wsx agent send` / git).
- No GitHub PR posting. Findings come back inside the wsx ecosystem.
- No built-in default pinned-command list. Pinning stays user config.

## Background (verified in code)

- **Skills are embedded at compile time.** `src/agent/skill.rs` does
  `include_str!("../../skills/wsx/SKILL.md")` and `wsx setup install-skill`
  (handled in `src/cli.rs` ~1034) writes the content to each detected agent's
  skills dir (`~/.claude/skills/wsx/SKILL.md`, plus `~/.codex/...` and
  `~/.hermes/...` when those agents are present). A new skill must join this
  embed+install path to be usable as `/agent-pr`.
- **Pinned chips auto-submit.** `fire_chip` in `src/app/input.rs` appends `\r`
  to the command bytes, so a chip `agent-pr=/agent-pr` runs `/agent-pr⏎`
  immediately in the primary agent's session — with no argument. The kind
  therefore comes from a default (`claude`) or a manually-typed arg.
- **Orchestration primitives exist.** `wsx agent add <kind>` prints
  `added <label>` (capturable); `wsx agent send <label> <msg>` injects an
  async, tagged message into a peer. `wsx agent list` marks the primary agent
  `(primary)`. All resolve the current workspace from `$WSX_WORKSPACE_ID` or the
  cwd's worktree.

## Design

### 1. Skill content — `skills/agent-pr/SKILL.md`

YAML frontmatter:

```
---
name: agent-pr
description: Use in a wsx workspace to spin up a peer review agent that
  code-reviews the current branch. Takes the reviewer kind (claude|pi|hermes|
  codex, default claude); spawns it, hands off branch-diff-vs-main context, and
  has it report findings back.
---
```

Body — the recipe the primary agent follows:

1. **Resolve the kind** from the argument. Accept only `claude|pi|hermes|codex`;
   default to `claude` when absent. On an invalid value, tell the user and stop.
2. **Confirm workspace context.** Require `$WSX_WORKSPACE_ID` set or cwd under
   `~/.local/state/wsx/worktrees/`. If not in a workspace, stop and explain.
3. **Spawn the reviewer.** Run `wsx agent add <kind>`; parse the `added <label>`
   line to learn the peer's label (e.g. `claude#2`).
4. **Find the primary's label** via `wsx agent list` (the row marked
   `(primary)`) so the reviewer knows where to report back.
5. **Gather a brief** (not the full diff — the reviewer shares the worktree):
   current branch name, `git log main..HEAD --oneline`, and
   `git diff --stat main...HEAD`.
6. **Hand off** with `wsx agent send <label> "<brief>"`. The brief instructs the
   reviewer to:
   - review the current branch against `main` (run `git diff main...HEAD`
     itself), and
   - produce a **risk assessment** (security, performance, breaking changes,
     edge cases) and a **gap analysis** (test coverage, documentation, error
     handling), matching the user's standing review policy, and
   - **report findings back** to the primary via
     `wsx agent send <primary-label> "<findings>"`.
7. **Report to the user** that reviewer `<label>` is spawned and working, and
   that findings will arrive as a message from that peer.

The skill is a flexible recipe (adapt wording), but steps 1–3 and 6 are rigid:
validate the kind, confirm the workspace, capture the label, and send the
handoff.

### 2. Generalize embed/install — `src/agent/skill.rs`

Replace the single-skill assumption with a small table:

```rust
pub struct BundledSkill { pub name: &'static str, pub content: &'static str }
pub const BUNDLED_SKILLS: &[BundledSkill] = &[
    BundledSkill { name: "wsx",      content: include_str!("../../skills/wsx/SKILL.md") },
    BundledSkill { name: "agent-pr", content: include_str!("../../skills/agent-pr/SKILL.md") },
];
```

- Per-agent path helpers change from returning a fixed `.../skills/wsx/SKILL.md`
  file to returning the agent's **skills dir** (`~/.claude/skills`, etc.). Keep
  thin file-path wrappers if needed for back-compat, but the canonical input is
  the dir.
- `InstallTarget` carries `{ agent, skill_name, content, path }` where
  `path = <skills-dir>/<skill_name>/SKILL.md`.
- `default_install_targets()` yields one target per (detected agent × bundled
  skill). Agent-detection (`codex_is_installed` / `hermes_is_installed`) is
  unchanged.
- `install_to(target)` writes `target.content` (was: the single global const).
  The lower-level `write_atomic` is unchanged.

### 3. CLI handler — `src/cli.rs`

The `setup install-skill` loop already iterates targets; it now sees more of
them. Update the success/idempotent messages to name the skill, e.g.
`installed agent-pr skill for Claude at <path>`, so two-skill output stays
legible. Behavior (Created/Updated/Unchanged per file) is unchanged.

### 4. Pinned command

Set the global pinned command as part of this work:

```
wsx config set pinned_commands "agent-pr=/agent-pr"
```

(Append to any existing value rather than clobbering.) Because chips
auto-submit, the chip runs `/agent-pr` (→ default `claude`); the user types
`/agent-pr codex` manually for a different reviewer kind.

### 5. Docs & tests

- **README:** short subsection near "Agent skill" describing `agent-pr`, the
  default-kind behavior, and the pinned-command one-liner.
- **Tests (`skill.rs`):** update existing tests that assume a single
  `SKILL_CONTENT`/`wsx`-only target set; assert both bundled skills appear in
  `default_install_targets()` with correct `<dir>/<name>/SKILL.md` paths; add a
  frontmatter check for the agent-pr skill (`name: agent-pr`). Keep the
  Created/Unchanged/Updated install-outcome tests, parameterized by content.
- **Verification:** `cargo fmt --check`, `cargo clippy`, `cargo test` before
  pushing (wsx CI gates on rustfmt).

## Risks / edge cases

- **Refactor blast radius:** `skill.rs` is the only module that owns the embed;
  call sites are just `cli.rs` (handler) and its tests. Low blast radius, but
  the existing `skill.rs` tests must be migrated, not left referencing the
  removed single-skill API.
- **Skill resolves only where installed:** `/agent-pr` works in the primary
  agent only after `wsx setup install-skill` has run on the machine. Document
  this; it mirrors the existing `wsx` skill constraint.
- **Non-Claude primary agents:** the skill installs to `~/.codex` and
  `~/.hermes` too, but those agents' skill-loading differs; the recipe is
  written in plain imperative steps so any agent that loads it can follow.

## Commit plan

1. Add `skills/agent-pr/SKILL.md`.
2. Generalize `src/agent/skill.rs` to the bundled-skills table + per-(agent×skill)
   targets; migrate its tests.
3. Update the `setup install-skill` handler messages in `src/cli.rs`.
4. README subsection + set the global pinned command.
