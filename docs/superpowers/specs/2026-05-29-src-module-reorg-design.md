# src/ module reorganization

**Date:** 2026-05-29
**Status:** Implemented (PR #123)

## Problem

`src/` has ~26 loose top-level `.rs` files alongside the existing TUI
subdirectories (`app/`, `ui/`, `pty/`, `detail_modules/`). The loose files are
mostly the non-TUI "backend" — persistence, git, config, external integrations,
and session-activity tailing — and it isn't obvious from the flat listing what
each does or how they relate. Goal: group them into role-based directories so
the layering is legible, without touching the TUI dirs.

## Approach

Group by **technical layer / role**. Move files into six new directories; leave
the entrypoint and crate-wide primitives at top level; leave the TUI layer
(`app/`, `ui/`, `pty/`, `detail_modules/`) unchanged. Update **all** module-path
references (no re-export shims) for a clean end state.

## Target layout

```
src/
  data/          # domain model · persistence · workspace lifecycle
    mod.rs
    store.rs  repo.rs  workspace.rs  setup.rs
  git/           # version control + PR/forge host
    mod.rs       # <- former git.rs content (keeps git::* paths)
    forge.rs
  agent/         # provisioning & orchestrating the Claude agent session
    mod.rs
    doctrine.rs  skill.rs  remote_control.rs  mcp.rs  related.rs  pm.rs
  activity/      # live introspection of running sessions & processes
    mod.rs
    events.rs  hermes_events.rs  pi_events.rs  proc.rs
  commands/      # launching user-configured external tools/commands
    mod.rs
    external.rs  remotes.rs  pinned.rs
  config/        # app settings & filesystem layout
    mod.rs       # <- former config.rs content (keeps config::Dirs paths)
    detail_bar_config.rs

  # left at top level: entrypoint + crate-wide primitives
  cli.rs  error.rs  names.rs  test_support.rs  app.rs  lib.rs  main.rs

  # TUI layer — unchanged
  app/  ui/  pty/  detail_modules/
```

### Why these groupings

- **data/** — `store` is the persistence hub (28 files import it); `repo` and
  `workspace` are CRUD/lifecycle orchestration over it; `setup` runs the
  per-worktree setup script during workspace creation.
- **git/** — `git` is the `git -C` wrapper; `forge` is GitHub PR-lifecycle
  detection (a git-host concern), so it nests as `git::forge`.
- **agent/** — everything that shapes a Claude agent session: prompt injection
  (`doctrine`, `related`), launch configuration (`remote_control`, `mcp`),
  session orchestration (`pm`), and skill installation (`skill`).
- **activity/** — read-only tailing/introspection of live sessions and
  processes (`events` + the `hermes`/`pi` variants, plus `proc` via `lsof`).
- **commands/** — launching user-configured external things: editors/terminals/
  difftools (`external`), named remote shell commands (`remotes`), pinned
  command chips (`pinned`).
- **config/** — filesystem layout (`config::Dirs`) and detail-bar display
  config.
- **top level** — `cli` (CLI dispatch root), `error`/`names`/`test_support`
  (crate-wide primitives), `app.rs` (already paired with `app/`).

## Rust mechanics

- Each new directory needs a module declaration. For dirs whose name matches a
  member (`git`, `config`), the existing file *becomes* the dir's `mod.rs` so
  its public paths are unchanged: `git.rs` → `git/mod.rs` (keeps `git::preflight`
  etc.), `config.rs` → `config/mod.rs` (keeps `config::Dirs`). `git/mod.rs` adds
  `pub mod forge;`; `config/mod.rs` adds `pub mod detail_bar_config;`.
- Dirs with no name-collision (`data`, `agent`, `activity`, `commands`) get a
  fresh `mod.rs` that just declares `pub mod ...;` for each member.
- `lib.rs` is rewritten to declare the new top-level set:
  `activity, agent, app, cli, commands, config, data, detail_modules, error,
  git, names, pty, test_support, ui`.
- Use `git mv` for moves so history follows the files.

### Path renames (apply repo-wide, `crate::` and `wsx::` forms)

| Old path | New path |
|---|---|
| `store` | `data::store` |
| `repo` | `data::repo` |
| `workspace` | `data::workspace` |
| `setup` | `data::setup` |
| `forge` | `git::forge` |
| `git` | *(unchanged — now the dir's mod root)* |
| `config` | *(unchanged — now the dir's mod root)* |
| `detail_bar_config` | `config::detail_bar_config` |
| `doctrine` | `agent::doctrine` |
| `skill` | `agent::skill` |
| `remote_control` | `agent::remote_control` |
| `mcp` | `agent::mcp` |
| `related` | `agent::related` |
| `pm` | `agent::pm` |
| `events` | `activity::events` |
| `hermes_events` | `activity::hermes_events` |
| `pi_events` | `activity::pi_events` |
| `proc` | `activity::proc` |
| `external` | `commands::external` |
| `remotes` | `commands::remotes` |
| `pinned` | `commands::pinned` |

`main.rs` needs `wsx::store` → `wsx::data::store` (and its `store::Store::open`
call site). Bare `events::`-style references left by `use` statements are caught
by the per-commit build loop.

## Implementation plan (one green commit per directory)

Each step keeps `cargo build` + `cargo test` passing before committing:

1. **data/** — move `store, repo, workspace, setup`; add `mod.rs`; update
   `lib.rs`; rewrite all `store/repo/workspace/setup` path refs repo-wide.
2. **git/** — move `forge` in; `git.rs` → `git/mod.rs` (+ `pub mod forge;`);
   update `forge` refs.
3. **agent/** — move the six files; add `mod.rs`; update refs.
4. **activity/** — move the four files; add `mod.rs`; update refs.
5. **commands/** — move the three files; add `mod.rs`; update refs.
6. **config/** — `config.rs` → `config/mod.rs` (+ `pub mod detail_bar_config;`);
   move `detail_bar_config` in; update its refs.

## Verification

- After each step: `cargo build` and `cargo test` are green before committing.
- Final: full `cargo build`, `cargo test`, and `cargo clippy` clean; `git mv`
  preserves file history; no behavior change (pure code-organization refactor).

## Out of scope

- The TUI dirs (`app/`, `ui/`, `pty/`, `detail_modules/`) are untouched.
- Splitting large files (`events.rs`, `store.rs`, `cli.rs`) is deferred.
- No re-export shims; old paths are fully removed.
