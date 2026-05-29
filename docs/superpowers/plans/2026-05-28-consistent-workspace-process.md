# Consistent Workspace Process Doctrine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Inject a non-negotiable "process doctrine" (think+plan, superpowers-by-default, logical commits, load-wsx-skill) into every developer agent session wsx spawns, with a config override.

**Architecture:** A new `src/doctrine.rs` produces agent-tailored default doctrine text and resolves a `process_doctrine` config override. The doctrine travels as a new `doctrine: Option<String>` field on the `SpawnMode::Fresh` and `SpawnMode::Continue` variants (ProjectManager has no such field, so it is excluded by construction). The three prompt composers (`build_claude_command`, `build_pi_command`, `compose_injected_prompt`) prepend the doctrine ahead of the rename prompt and custom instructions. `build_spawn_info` (the app-layer call site that already has the store) resolves the effective doctrine and populates the field.

> **Note — refinement of spec §4.** The spec described threading a `doctrine: Option<&str>` *parameter* into the builders. This plan instead carries it as a *field on the Fresh/Continue variants* the builders already receive via `&mode`. This is lower-churn (no signature changes to `spawn_session`/`build_*` and their ~30 test callers), matches how `custom_instructions` already travels, and makes the ProjectManager exclusion a compile-time guarantee. Behavior is identical to the approved design: doctrine-before-rename ordering, Fresh+Continue only, builders stay store-free.

**Tech Stack:** Rust, `cargo test`. Agent enum `crate::pty::session::AgentKind` (re-exported as `crate::pty::AgentKind`). Settings via `crate::store::Store::{get_setting,set_setting}`. `CommandBuilder::get_argv()` returns `Vec<OsString>` for assertions.

---

## File Structure

- **Create** `src/doctrine.rs` — owns the doctrine text and override resolution. Two public fns: `process_doctrine(agent) -> String`, `resolve_effective_doctrine(store, agent) -> String`.
- **Modify** `src/lib.rs` — register `pub mod doctrine;`.
- **Modify** `src/cli.rs:127` — add `"process_doctrine"` to `known_setting_key`.
- **Modify** `src/pty/session.rs` — add `doctrine` field to `SpawnMode::Fresh`/`Continue`; compose it in `build_claude_command`, `build_pi_command`, `compose_injected_prompt`.
- **Modify** `src/app.rs:803` (`build_spawn_info`) — resolve and populate the doctrine field.
- **Modify** test sites that construct `SpawnMode::Fresh`/`Continue` literals (compiler-listed) — add `doctrine: None,`.

---

## Task 1: Doctrine text module

**Files:**
- Create: `src/doctrine.rs`
- Modify: `src/lib.rs` (add module)

- [ ] **Step 1: Register the module**

In `src/lib.rs`, add the line in alphabetical position (after `pub mod detail_modules;`, before `pub mod error;`):

```rust
pub mod doctrine;
```

- [ ] **Step 2: Write the failing test**

Create `src/doctrine.rs` with only the test module and a stub:

```rust
//! The standing "process doctrine" wsx injects into developer sessions.
//!
//! These are non-negotiable defaults; an agent may stand them down only if a
//! task plainly does not warrant the planning.

use crate::pty::session::AgentKind;

pub fn process_doctrine(_agent: AgentKind) -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::session::AgentKind;

    #[test]
    fn doctrine_covers_all_practices_for_claude() {
        let d = process_doctrine(AgentKind::Claude).to_lowercase();
        assert!(d.contains("plan"), "must mention planning: {d}");
        assert!(d.contains("superpowers"), "claude must get superpowers clause: {d}");
        assert!(d.contains("commit"), "must mention commits: {d}");
        assert!(d.contains("wsx skill"), "must mention the wsx skill: {d}");
    }

    #[test]
    fn pi_also_gets_superpowers() {
        let d = process_doctrine(AgentKind::Pi).to_lowercase();
        assert!(d.contains("superpowers"), "pi must get superpowers clause: {d}");
    }

    #[test]
    fn hermes_omits_superpowers_but_keeps_the_rest() {
        let d = process_doctrine(AgentKind::Hermes).to_lowercase();
        assert!(!d.contains("superpowers"), "hermes must NOT get superpowers clause: {d}");
        assert!(d.contains("plan"), "hermes must still get planning clause: {d}");
        assert!(d.contains("commit"), "hermes must still get commits clause: {d}");
        assert!(d.contains("wsx skill"), "hermes must still get wsx skill clause: {d}");
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --lib doctrine::tests 2>&1 | tail -20`
Expected: FAIL — assertions fail (stub returns empty string).

- [ ] **Step 4: Implement `process_doctrine`**

Replace the stub body in `src/doctrine.rs` (keep the test module):

```rust
const DOCTRINE_HEADER: &str = "## wsx workspace operating doctrine\n\n\
    This is a wsx-managed workspace, and the work here is rarely trivial. Unless \
    the task is plainly simple, treat the following as your default, \
    non-negotiable operating mode. You may stand a practice down only if, after \
    evaluating, the task clearly does not warrant it.";

const CLAUSE_PLAN: &str = "- Think and plan before acting. Determine scope first, \
    applying maximum effort and explicit planning until the scope is clear. Do not \
    start editing code before you understand what you are building.";

const CLAUSE_SUPERPOWERS: &str = "- Use the superpowers skills by default when \
    evaluating the initial request. If the task turns out not to need that level \
    of planning, you may discard them and proceed.";

const CLAUSE_COMMITS: &str = "- Break the work into logical commits on this branch. \
    A workspace that ends with a single commit should be the exception, reserved \
    for the simplest tasks — not the norm.";

const CLAUSE_WSX_SKILL: &str = "- Load and follow the wsx skill. It is authoritative \
    for workspace and cross-repo operations in this environment; consult it before \
    running wsx commands.";

pub fn process_doctrine(agent: AgentKind) -> String {
    let include_superpowers = matches!(agent, AgentKind::Claude | AgentKind::Pi);
    let mut clauses = vec![CLAUSE_PLAN];
    if include_superpowers {
        clauses.push(CLAUSE_SUPERPOWERS);
    }
    clauses.push(CLAUSE_COMMITS);
    clauses.push(CLAUSE_WSX_SKILL);
    format!("{DOCTRINE_HEADER}\n\n{}", clauses.join("\n"))
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib doctrine::tests 2>&1 | tail -20`
Expected: PASS (3 tests).

- [ ] **Step 6: Commit**

```bash
git add src/doctrine.rs src/lib.rs
git commit -m "feat(doctrine): agent-tailored process doctrine text"
```

---

## Task 2: Resolve config override

**Files:**
- Modify: `src/doctrine.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/doctrine.rs`:

```rust
    #[test]
    fn resolve_returns_default_when_unset() {
        let store = crate::store::Store::open_in_memory().unwrap();
        assert_eq!(
            resolve_effective_doctrine(&store, AgentKind::Claude),
            process_doctrine(AgentKind::Claude)
        );
    }

    #[test]
    fn resolve_override_replaces_default_verbatim() {
        let store = crate::store::Store::open_in_memory().unwrap();
        store.set_setting("process_doctrine", "CUSTOM DOCTRINE").unwrap();
        assert_eq!(
            resolve_effective_doctrine(&store, AgentKind::Hermes),
            "CUSTOM DOCTRINE"
        );
    }

    #[test]
    fn resolve_treats_blank_override_as_unset() {
        let store = crate::store::Store::open_in_memory().unwrap();
        store.set_setting("process_doctrine", "   ").unwrap();
        assert_eq!(
            resolve_effective_doctrine(&store, AgentKind::Pi),
            process_doctrine(AgentKind::Pi)
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib doctrine::tests::resolve 2>&1 | tail -20`
Expected: FAIL — `resolve_effective_doctrine` not found (does not compile).

- [ ] **Step 3: Implement `resolve_effective_doctrine`**

Add to `src/doctrine.rs` (after `process_doctrine`):

```rust
/// The effective doctrine for a spawn: the `process_doctrine` setting if set
/// (replaces the default verbatim, for every agent), else the agent-tailored
/// default. A blank/whitespace override is treated as unset.
pub fn resolve_effective_doctrine(store: &crate::store::Store, agent: AgentKind) -> String {
    match store.get_setting("process_doctrine") {
        Ok(Some(v)) if !v.trim().is_empty() => v,
        _ => process_doctrine(agent),
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib doctrine::tests 2>&1 | tail -20`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
git add src/doctrine.rs
git commit -m "feat(doctrine): resolve process_doctrine config override"
```

---

## Task 3: Register the config key

**Files:**
- Modify: `src/cli.rs:127` (`known_setting_key`)

- [ ] **Step 1: Write the failing test**

Add a test near the other cli tests in `src/cli.rs` (inside its `#[cfg(test)] mod tests`; if none exists, add `#[cfg(test)] mod doctrine_key_tests { use super::*; ... }` at the end of the file):

```rust
    #[test]
    fn process_doctrine_is_a_known_setting() {
        assert!(known_setting_key("process_doctrine"));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib process_doctrine_is_a_known_setting 2>&1 | tail -20`
Expected: FAIL — assertion fails (`process_doctrine` not yet in the match).

- [ ] **Step 3: Add the key**

In `src/cli.rs`, in `known_setting_key`, add `"process_doctrine"` to the match list (e.g. after `"custom_instructions"`):

```rust
            | "custom_instructions"
            | "process_doctrine"
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib process_doctrine_is_a_known_setting 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs
git commit -m "feat(config): allow setting process_doctrine override"
```

---

## Task 4: Add the `doctrine` field to SpawnMode (plumbing only, no behavior change)

This task adds the field and fixes every constructor/destructure site so the crate compiles with no behavior change. The compiler enumerates the sites for you.

**Files:**
- Modify: `src/pty/session.rs` (enum + the three composer match arms + in-file test literals)
- Modify: `src/app.rs` (two literals in `build_spawn_info`)
- Modify: `src/app/input_tests.rs` (many literals)

- [ ] **Step 1: Add the field to the enum**

In `src/pty/session.rs`, in `enum SpawnMode` (around line 290), add `doctrine: Option<String>` to **both** `Fresh` and `Continue` (NOT `ProjectManager`):

```rust
    Fresh {
        rename_ctx: Option<RenameContext>,
        custom_instructions: Option<String>,
        /// Process doctrine to inject ahead of rename/custom content. `None`
        /// only in tests; production always supplies it via `build_spawn_info`.
        doctrine: Option<String>,
        additional_dirs: Vec<std::path::PathBuf>,
        yolo: bool,
    },
    Continue {
        custom_instructions: Option<String>,
        doctrine: Option<String>,
        additional_dirs: Vec<std::path::PathBuf>,
        yolo: bool,
    },
```

- [ ] **Step 2: Make the destructuring match arms compile**

The composers destructure these variants explicitly. For now (no behavior change), add `doctrine: _,` to each `Fresh`/`Continue` arm in these three functions so they compile:
- `build_claude_command` (`src/pty/session.rs:675`, `:687`)
- `build_pi_command` (`src/pty/session.rs:847`, `:852`)
- `compose_injected_prompt` (`src/pty/session.rs:1079`, `:1092`, `:1097`)

Example for the `build_claude_command` `Continue` arm:

```rust
            SpawnMode::Continue {
                custom_instructions,
                doctrine: _,
                additional_dirs,
                yolo,
            } => (
```

(Arms that already use `..` need no change.)

- [ ] **Step 3: Compile to get the list of literal sites to fix**

Run: `cargo build 2>&1 | grep -E "missing field|--> " | head -80`
Expected: FAIL — "missing field `doctrine`" at every `SpawnMode::Fresh`/`Continue` constructor (in `src/app.rs`, `src/app/input_tests.rs`, and the `#[cfg(test)]` blocks of `src/pty/session.rs`).

- [ ] **Step 4: Add `doctrine: None,` to every flagged literal**

For each constructor the compiler flagged, add the field. In production code (`src/app.rs:838` Continue, `:856` Fresh) add `doctrine: None,` for now — Task 8 replaces it. In every test literal add `doctrine: None,`. Example:

```rust
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
```

Repeat `cargo build` until it compiles clean.

- [ ] **Step 5: Run the full suite to confirm no behavior change**

Run: `cargo test 2>&1 | tail -15`
Expected: PASS — all existing tests still green (the field is unused so far).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "refactor(session): add doctrine field to Fresh/Continue SpawnMode"
```

---

## Task 5: Compose doctrine into the Claude command

**Files:**
- Modify: `src/pty/session.rs` (`build_claude_command`, `:661`)
- Test: `src/pty/session.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/pty/session.rs`:

```rust
    #[test]
    fn claude_prepends_doctrine_before_custom_instructions() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: Some("CUSTOM_MARK".to_string()),
            doctrine: Some("DOCTRINE_MARK".to_string()),
            additional_dirs: vec![],
            yolo: false,
        };
        let cmd = build_claude_command(&cwd, &mode, crate::remote_control::RemoteOpts::disabled());
        let argv = cmd.get_argv();
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--append-system-prompt"))
            .expect("expected --append-system-prompt");
        let prompt = argv.get(idx + 1).unwrap().to_string_lossy();
        let dpos = prompt.find("DOCTRINE_MARK").expect("doctrine present");
        let cpos = prompt.find("CUSTOM_MARK").expect("custom present");
        assert!(dpos < cpos, "doctrine must precede custom instructions: {prompt}");
        assert!(prompt.starts_with("DOCTRINE_MARK"), "doctrine must lead: {prompt}");
    }

    #[test]
    fn claude_pm_mode_has_no_doctrine_marker() {
        // PM variant has no doctrine field; ensure nothing leaks one in.
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/x/workspaces.json"),
            custom_instructions: None,
            additional_dirs: vec![],
            resume: false,
            fast_mode: false,
        };
        let cmd = build_claude_command(&cwd, &mode, crate::remote_control::RemoteOpts::disabled());
        let argv = cmd.get_argv();
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--append-system-prompt"));
        if let Some(i) = idx {
            let prompt = argv.get(i + 1).unwrap().to_string_lossy();
            assert!(!prompt.contains("DOCTRINE_MARK"), "PM must not get doctrine");
        }
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib claude_prepends_doctrine_before_custom_instructions 2>&1 | tail -20`
Expected: FAIL — `--append-system-prompt` present but doctrine marker absent (field still ignored).

- [ ] **Step 3: Implement composition**

In `build_claude_command`, the top `match mode` builds a tuple. Add a `doctrine` element to it. For the `Continue` arm bind the field; for `Fresh` bind it; for `ProjectManager` use `None`. Change the binding tuple to lead with `doctrine`:

```rust
    let (doctrine, rename_prompt, custom, allow_wsx_rename, add_continue, skip_permissions, add_dirs) =
        match mode {
            SpawnMode::Continue {
                custom_instructions,
                doctrine,
                additional_dirs,
                yolo,
            } => (
                doctrine.clone(),
                None,
                custom_instructions.clone(),
                false,
                true,
                *yolo,
                additional_dirs.clone(),
            ),
            SpawnMode::Fresh {
                rename_ctx,
                custom_instructions,
                doctrine,
                additional_dirs,
                yolo,
            } => {
                // ... existing rename_mode/rp/allow logic unchanged ...
                (
                    doctrine.clone(),
                    rp,
                    custom_instructions.clone(),
                    allow,
                    false,
                    *yolo,
                    additional_dirs.clone(),
                )
            }
            SpawnMode::ProjectManager {
                workspaces_json_path: _,
                custom_instructions,
                additional_dirs,
                resume,
                fast_mode: _,
            } => (
                None,
                Some(crate::pm::pm_system_prompt(custom_instructions.as_deref())),
                None,
                false,
                *resume,
                true,
                additional_dirs.clone(),
            ),
        };
```

Then replace the `combined` block (currently the 4-arm `match (rename_prompt, custom)`) with an ordered join of all three parts:

```rust
    let parts: Vec<String> = [doctrine, rename_prompt, custom]
        .into_iter()
        .flatten()
        .collect();
    let combined = if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    };
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib 'claude_prepends_doctrine_before_custom_instructions' 'claude_pm_mode_has_no_doctrine_marker' 2>&1 | tail -20`
Expected: PASS. Also run `cargo test --lib build_claude 2>&1 | tail -20` — existing Claude tests still PASS.

- [ ] **Step 5: Commit**

```bash
git add src/pty/session.rs
git commit -m "feat(session): inject doctrine into Claude system prompt"
```

---

## Task 6: Compose doctrine into the Pi command

**Files:**
- Modify: `src/pty/session.rs` (`build_pi_command`, `:831`)
- Test: `src/pty/session.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module:

```rust
    #[test]
    fn pi_prepends_doctrine_before_custom_instructions() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::Continue {
            custom_instructions: Some("CUSTOM_MARK".to_string()),
            doctrine: Some("DOCTRINE_MARK".to_string()),
            additional_dirs: vec![],
            yolo: false,
        };
        let cmd = build_pi_command(&cwd, &mode, crate::remote_control::RemoteOpts::disabled());
        let argv = cmd.get_argv();
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--append-system-prompt"))
            .expect("expected --append-system-prompt");
        let prompt = argv.get(idx + 1).unwrap().to_string_lossy();
        let dpos = prompt.find("DOCTRINE_MARK").expect("doctrine present");
        let cpos = prompt.find("CUSTOM_MARK").expect("custom present");
        assert!(dpos < cpos, "doctrine must precede custom instructions: {prompt}");
        assert!(prompt.starts_with("DOCTRINE_MARK"), "doctrine must lead: {prompt}");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib pi_prepends_doctrine_before_custom_instructions 2>&1 | tail -20`
Expected: FAIL — doctrine marker absent.

- [ ] **Step 3: Implement composition**

In `build_pi_command`, change the `match mode` tuple to include `doctrine` first. Bind the field in `Continue`/`Fresh`, `None` for `ProjectManager`:

```rust
    let (doctrine, rename_prompt, custom, add_continue) = match mode {
        SpawnMode::Continue {
            custom_instructions,
            doctrine,
            additional_dirs: _,
            yolo: _,
        } => (doctrine.clone(), None, custom_instructions.clone(), true),
        SpawnMode::Fresh {
            rename_ctx,
            custom_instructions,
            doctrine,
            additional_dirs: _,
            yolo: _,
        } => {
            // ... existing rename_mode/rp logic unchanged ...
            (doctrine.clone(), rp, custom_instructions.clone(), false)
        }
        SpawnMode::ProjectManager {
            workspaces_json_path: _,
            custom_instructions,
            additional_dirs: _,
            resume,
            fast_mode: _,
        } => (
            None,
            Some(crate::pm::pm_system_prompt(custom_instructions.as_deref())),
            None,
            *resume,
        ),
    };
```

Then replace the `combined` block (the 4-arm `match (rename_prompt, custom)`) with:

```rust
    let parts: Vec<String> = [doctrine, rename_prompt, custom]
        .into_iter()
        .flatten()
        .collect();
    let combined = if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    };
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib pi_prepends_doctrine_before_custom_instructions 2>&1 | tail -20`
Expected: PASS. Also `cargo test --lib build_pi 2>&1 | tail -20` — existing Pi tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/pty/session.rs
git commit -m "feat(session): inject doctrine into Pi system prompt"
```

---

## Task 7: Compose doctrine into the Hermes AGENTS.md block

**Files:**
- Modify: `src/pty/session.rs` (`compose_injected_prompt`, `:1070`)
- Test: `src/pty/session.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module (the Hermes tests call `super::compose_injected_prompt`):

```rust
    #[test]
    fn hermes_prepends_doctrine_before_custom() {
        let mode = SpawnMode::Continue {
            custom_instructions: Some("CUSTOM_MARK".to_string()),
            doctrine: Some("DOCTRINE_MARK".to_string()),
            additional_dirs: vec![],
            yolo: false,
        };
        let result = compose_injected_prompt(&mode).expect("expected Some");
        let dpos = result.find("DOCTRINE_MARK").expect("doctrine present");
        let cpos = result.find("CUSTOM_MARK").expect("custom present");
        assert!(dpos < cpos, "doctrine must precede custom: {result}");
        assert!(result.starts_with("DOCTRINE_MARK"), "doctrine must lead: {result}");
    }

    #[test]
    fn hermes_pm_mode_has_no_doctrine() {
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/x/workspaces.json"),
            custom_instructions: None,
            additional_dirs: vec![],
            resume: false,
            fast_mode: false,
        };
        let result = compose_injected_prompt(&mode).expect("PM still injects its prompt");
        assert!(!result.contains("DOCTRINE_MARK"), "PM must not get doctrine: {result}");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib hermes_prepends_doctrine_before_custom 2>&1 | tail -20`
Expected: FAIL — doctrine marker absent.

- [ ] **Step 3: Rewrite `compose_injected_prompt`**

Replace the body of `compose_injected_prompt` (`src/pty/session.rs:1070`) with a version that gathers `(doctrine, rename, custom)` then joins in order. The inner `combine` helper is no longer needed:

```rust
fn compose_injected_prompt(mode: &SpawnMode) -> Option<String> {
    let (doctrine, rename, custom) = match mode {
        SpawnMode::Fresh {
            rename_ctx: Some(ctx),
            custom_instructions,
            doctrine,
            ..
        } => (
            doctrine.clone(),
            Some(render_rename_system_prompt_hermes(
                &ctx.current_branch,
                &ctx.branch_prefix,
                &ctx.repo_name,
                &ctx.current_slug,
            )),
            custom_instructions.clone(),
        ),
        SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions,
            doctrine,
            ..
        }
        | SpawnMode::Continue {
            custom_instructions,
            doctrine,
            ..
        } => (doctrine.clone(), None, custom_instructions.clone()),
        SpawnMode::ProjectManager {
            custom_instructions, ..
        } => (
            None,
            Some(crate::pm::pm_system_prompt(custom_instructions.as_deref())),
            None,
        ),
    };

    let parts: Vec<String> = [doctrine, rename, custom].into_iter().flatten().collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib hermes 2>&1 | tail -20`
Expected: PASS — new doctrine tests plus existing `compose_injected_prompt` tests.

- [ ] **Step 5: Commit**

```bash
git add src/pty/session.rs
git commit -m "feat(session): inject doctrine into Hermes AGENTS.md block"
```

---

## Task 8: Resolve and populate doctrine at the spawn call site

**Files:**
- Modify: `src/app.rs` (`build_spawn_info`, `:803`)
- Test: `src/app/input_tests.rs`

- [ ] **Step 1: Write the failing test**

Add to `src/app/input_tests.rs` (model it on `build_spawn_info_filters_self_reference` at `:2337`):

```rust
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn build_spawn_info_populates_doctrine() {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/work/backend"), "backend", "")
            .unwrap();
        let ws_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "test-ws",
                branch: "backend/test-ws",
                worktree_path: std::path::Path::new("/wt/test-ws"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        store.set_workspace_state(ws_id, WorkspaceState::Ready).unwrap();

        let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let (_id, _path, mode, _repo_path, _agent) = build_spawn_info(&app, ws_id).unwrap();
        match mode {
            crate::pty::session::SpawnMode::Fresh { doctrine, .. } => {
                let d = doctrine.expect("doctrine must be populated");
                assert!(d.contains("superpowers"), "claude doctrine includes superpowers: {d}");
            }
            other => panic!("expected Fresh, got {other:?}"),
        }
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib build_spawn_info_populates_doctrine 2>&1 | tail -20`
Expected: FAIL — `doctrine` is `None` (Task 4 left it `None`), so `.expect(...)` panics.

- [ ] **Step 3: Populate the field**

In `src/app.rs` `build_spawn_info`, after `let agent = ws.agent;` (line 819), add:

```rust
    let doctrine = Some(crate::doctrine::resolve_effective_doctrine(&app.store, agent));
```

Then in the two literals below, replace `doctrine: None,` with `doctrine: doctrine.clone(),` (Continue at `:838`) and `doctrine,` (Fresh at `:856`, the last use — no clone needed):

```rust
        crate::pty::session::SpawnMode::Continue {
            custom_instructions: custom,
            doctrine: doctrine.clone(),
            additional_dirs,
            yolo,
        }
```
```rust
        crate::pty::session::SpawnMode::Fresh {
            rename_ctx,
            custom_instructions: custom,
            doctrine,
            additional_dirs,
            yolo,
        }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib build_spawn_info_populates_doctrine 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs src/app/input_tests.rs
git commit -m "feat(app): resolve and inject effective doctrine on spawn"
```

---

## Task 9: Full-suite verification & fastMode regression confirmation

**Files:** none (verification only)

- [ ] **Step 1: Run the entire test suite**

Run: `cargo test 2>&1 | tail -25`
Expected: PASS — entire suite green.

- [ ] **Step 2: Confirm the spec §5 fastMode guarantee still holds**

The guarantee "developer sessions never enable fastMode" is covered by pre-existing tests. Confirm they pass:

Run: `cargo test --lib fresh_mode_never_emits_settings_for_fast_mode continue_mode_never_emits_settings_for_fast_mode 2>&1 | tail -10`
Expected: PASS (2 tests). No new test needed — these already lock in the guarantee, and nothing in this plan touches fastMode gating.

- [ ] **Step 3: Lint**

Run: `cargo clippy --all-targets 2>&1 | tail -20`
Expected: no new warnings from changed files (notably the removed `combine` helper in Hermes should leave no dead-code warning).

- [ ] **Step 4: Final commit (only if clippy required fixes)**

```bash
git add -A
git commit -m "chore: clippy cleanup for doctrine injection"
```

---

## Self-Review

**Spec coverage:**
- §1 doctrine source (agent-tailored) → Task 1.
- §2 config override (`process_doctrine`, replaces verbatim) → Tasks 2 & 3.
- §3 injection Fresh+Continue, PM excluded; doctrine-before-rename → Tasks 5, 6, 7 (PM-exclusion proved by `*_pm_mode_has_no_doctrine*` tests; ordering proved by `*_prepends_doctrine_before_*` tests).
- §4 threading without store in builders → field-on-variant refinement (documented in Architecture), resolved at `build_spawn_info` (Task 8).
- §5 fastMode guard → Task 9 Step 2 (pre-existing tests).
- §6 wsx-skill directive present for all agents → `CLAUSE_WSX_SKILL`, asserted for Claude/Pi/Hermes in Task 1.
- Testing section → covered across Tasks 1–9.
- Out-of-scope items (skill contents, per-repo doctrine, Pi/Hermes skill materialization) → not implemented, as intended.

**Placeholder scan:** none — every step shows concrete code/commands.

**Type consistency:** `process_doctrine(AgentKind) -> String` and `resolve_effective_doctrine(&Store, AgentKind) -> String` used consistently; field name `doctrine: Option<String>` consistent across enum, composers, and `build_spawn_info`; marker strings `DOCTRINE_MARK`/`CUSTOM_MARK` consistent within tests.
