# Codex CLI Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add OpenAI's Codex CLI as a fourth first-class coding agent in wsx, alongside Claude, Pi, and Hermes.

**Architecture:** Codex slots into the existing `AgentKind` abstraction. It is closest to Hermes: it has no `--append-system-prompt` (instructions go through `AGENTS.md`, reusing wsx's agent-neutral block machinery), but unlike Hermes its `codex resume --last` is cwd-filtered natively, so per-worktree continue is free. The one piece of genuinely new code is `src/activity/codex_events.rs`, a JSONL tailer mirroring `pi_events.rs` but locating the worktree's session by matching the `cwd` embedded in each rollout file's `session_meta` line.

**Tech Stack:** Rust, `portable-pty`, `serde_json`, `vt100`, `rusqlite` (existing deps only — no new crates).

**Reference design:** `docs/superpowers/specs/2026-06-02-codex-cli-support-design.md`

**Conventions used throughout:**
- Run a single test with: `cargo test --lib <test_name> -- --exact` (or a substring without `--exact`).
- Run a module's tests: `cargo test --lib codex_events`.
- Full check before each commit: `cargo test --lib` and `cargo clippy --all-targets -- -D warnings`.
- The `WSX_CODEX_BIN` env var is the test seam for spawning (point it at `cat` or a stub), exactly like `WSX_CLAUDE_BIN` / `WSX_PI_BIN`.
- Tests that mutate process-global env vars use `EnvGuard` from `crate::test_support` to serialize; HOME-swap tests in `codex_events` follow the raw set_var/restore pattern already used in `pi_events.rs` tests.

---

## Task 1: Skeleton — add the `Codex` variant and make the crate compile

Adding an enum variant makes every `match AgentKind` non-exhaustive, so this task adds the variant **and** every required arm/stub in one compiling unit. Later tasks replace the stubs with real logic via TDD.

**Files:**
- Modify: `src/pty/session.rs` (enum, `ALL`, `resolved_binary`, `display_name`, `from_str_or_default`, `has_prior_session_for`, `spawn_session`; add stubs `has_prior_codex_session`, `build_codex_command`, `prepare_codex_workspace`; update helper test ~3651)
- Modify: `src/activity/mod.rs` (declare module)
- Create: `src/activity/codex_events.rs` (stubs)
- Modify: `src/app/background.rs` (two dispatch arms, ~38-48 and ~68-78)
- Modify: `src/agent/pm.rs` (`compute_session_log_dir` arm, ~100-113)
- Modify: `src/app/input.rs` (Tab-cycle arm, ~953-958)
- Modify: `src/cli.rs` (`--agent` validation + strings, ~358-393)

- [ ] **Step 1: Update the helper test to expect four agents (failing test)**

In `src/pty/session.rs`, the test `agent_kind_helpers_match_existing_strings` (~line 3651). Change the count assertion and add Codex assertions:

```rust
        assert_eq!(AgentKind::ALL.len(), 4);
        assert!(AgentKind::ALL.contains(&AgentKind::Claude));
        assert!(AgentKind::ALL.contains(&AgentKind::Pi));
        assert!(AgentKind::ALL.contains(&AgentKind::Hermes));
        assert!(AgentKind::ALL.contains(&AgentKind::Codex));

        assert_eq!(AgentKind::Claude.display_name(), "claude");
        assert_eq!(AgentKind::Pi.display_name(), "pi");
        assert_eq!(AgentKind::Hermes.display_name(), "hermes");
        assert_eq!(AgentKind::Codex.display_name(), "codex");

        assert_eq!(AgentKind::Claude.default_binary(), "claude");
        assert_eq!(AgentKind::Pi.default_binary(), "pi");
        assert_eq!(AgentKind::Hermes.default_binary(), "hermes");
        assert_eq!(AgentKind::Codex.default_binary(), "codex");
```

- [ ] **Step 2: Verify it fails to compile**

Run: `cargo test --lib agent_kind_helpers_match_existing_strings`
Expected: compile error — `no variant named Codex found for enum AgentKind`.

- [ ] **Step 3: Add the variant and the four trivial helper arms**

In `src/pty/session.rs`:

Enum (~line 56):
```rust
pub enum AgentKind {
    Claude,
    Pi,
    Hermes,
    Codex,
}
```

`ALL` (~line 66):
```rust
    pub const ALL: [AgentKind; 4] =
        [AgentKind::Claude, AgentKind::Pi, AgentKind::Hermes, AgentKind::Codex];
```

`from_str_or_default` (~line 68) — add the `codex` arm:
```rust
    pub fn from_str_or_default(s: Option<&str>) -> Self {
        match s {
            Some("pi") => AgentKind::Pi,
            Some("hermes") => AgentKind::Hermes,
            Some("codex") => AgentKind::Codex,
            _ => AgentKind::Claude,
        }
    }
```

`display_name` (~line 76) — add arm:
```rust
            AgentKind::Codex => "codex",
```

`resolved_binary` env-var match (~line 46) — add arm:
```rust
        AgentKind::Codex => "WSX_CODEX_BIN",
```

- [ ] **Step 4: Add the dispatch arms that reference not-yet-real functions**

`has_prior_session_for` (~line 681):
```rust
        AgentKind::Codex => has_prior_codex_session(worktree),
```

`spawn_session` match (~line 1198):
```rust
        AgentKind::Codex => {
            prepare_codex_workspace(cwd, &mode);
            build_codex_command(cwd, &mode, remote)
        }
```

- [ ] **Step 5: Add stub implementations in `src/pty/session.rs`**

Place near the other agents' command builders. These are minimal-but-real stubs replaced in Tasks 2–4:

```rust
/// True if Codex has a recorded session whose `cwd` matches this worktree.
/// Real implementation lands in the codex_events locate task.
pub fn has_prior_codex_session(worktree: &Path) -> bool {
    crate::activity::codex_events::locate_session_file(worktree).is_some()
}

/// Prepare a worktree for a Codex spawn: inject the wsx-managed instruction
/// block into AGENTS.md (Codex reads project instructions from there, like
/// Hermes) and hide the file from `git status`. Codex needs NO spawn-timestamp
/// marker — session detection is cwd-in-file, not marker-based.
fn prepare_codex_workspace(cwd: &Path, mode: &SpawnMode) {
    let injected = compose_injected_prompt(mode);
    let had_content = injected.is_some();
    write_agents_md_section(cwd, injected.as_deref());
    if had_content {
        ensure_git_exclude(cwd, "AGENTS.md");
    }
}

/// Build a `CommandBuilder` for `codex` inside `cwd`. STUB: bare `codex`,
/// fleshed out (resume/yolo/model/PM) in the build_codex_command task.
pub fn build_codex_command(
    cwd: &Path,
    _mode: &SpawnMode,
    _remote: crate::agent::remote_control::RemoteOpts,
) -> CommandBuilder {
    let bin = std::env::var("WSX_CODEX_BIN").unwrap_or_else(|_| "codex".to_string());
    let mut cmd = CommandBuilder::new(bin);
    cmd.cwd(cwd);
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }
    cmd
}
```

- [ ] **Step 6: Create `src/activity/codex_events.rs` with stubs and declare it**

Create `src/activity/codex_events.rs`:
```rust
//! Tail Codex CLI session events from `~/.codex/sessions/**/rollout-*.jsonl`.
//!
//! Codex rollout files are date-partitioned (`YYYY/MM/DD/`) and store the
//! originating directory INSIDE the file (first line is `session_meta` with a
//! `cwd` field), so locating "this worktree's session" matches by content,
//! not by directory path. Real implementations land in later tasks.

use crate::activity::events::TailUpdate;
use crate::error::Result;
use std::path::{Path, PathBuf};

/// Locate the newest Codex rollout file whose recorded `cwd` matches `worktree`.
/// STUB — real implementation in the locate task.
pub fn locate_session_file(_worktree: &Path) -> Option<PathBuf> {
    None
}

/// Tail Codex rollout JSONL from `offset`. STUB — real implementation in the
/// tail/parse task.
pub fn tail_session(_path: &Path, offset: u64) -> Result<TailUpdate> {
    Ok(TailUpdate {
        new_offset: offset,
        ..TailUpdate::default()
    })
}
```

In `src/activity/mod.rs`, add after the `pub mod pi_events;` line:
```rust
pub mod codex_events;
```

- [ ] **Step 7: Add the activity dispatch arms in `src/app/background.rs`**

In the `current_file` match (~line 38):
```rust
        crate::pty::session::AgentKind::Codex => {
            crate::activity::codex_events::locate_session_file(&worktree_path)
        }
```

In the `tail_result` match (~line 68):
```rust
        crate::pty::session::AgentKind::Codex => {
            crate::activity::codex_events::tail_session(&file, tail_from)
        }
```

- [ ] **Step 8: Add the PM `compute_session_log_dir` arm in `src/agent/pm.rs`**

In the match (~line 100), mirror the Hermes unsupported-marker stance — Codex has no per-cwd log dir for the PM-Claude dossier to tail:
```rust
        crate::pty::session::AgentKind::Codex => {
            // Codex stores sessions in ~/.codex/sessions/YYYY/MM/DD/ with cwd
            // embedded per-file, not a per-cwd log directory. PM dossier
            // session-tail is not supported for Codex workspaces.
            home.join(".codex/UNSUPPORTED-no-session-log-dir-for-codex")
        }
```

- [ ] **Step 9: Add the Tab-cycle arm in `src/app/input.rs`**

The cycle (~line 953) currently goes Claude → Pi → Hermes → Claude. Insert Codex before wrapping to Claude:
```rust
                agent = match agent {
                    crate::pty::session::AgentKind::Claude => crate::pty::session::AgentKind::Pi,
                    crate::pty::session::AgentKind::Pi => crate::pty::session::AgentKind::Hermes,
                    crate::pty::session::AgentKind::Hermes => {
                        crate::pty::session::AgentKind::Codex
                    }
                    crate::pty::session::AgentKind::Codex => {
                        crate::pty::session::AgentKind::Claude
                    }
                };
```

- [ ] **Step 10: Update `--agent` validation in `src/cli.rs`**

Help string (~line 358):
```rust
                        "workspace create <repo> [--name <slug>] [--yolo] [--agent claude|pi|hermes|codex]"
```
`--agent needs value` message (~line 377):
```rust
                                    "--agent needs value (claude, pi, hermes, or codex)".into(),
```
Validation (~line 386):
```rust
                if let Some(ref a) = agent
                    && a != "pi"
                    && a != "claude"
                    && a != "hermes"
                    && a != "codex"
                {
                    return Err(Error::UserInput(format!(
                        "--agent must be 'claude', 'pi', 'hermes', or 'codex', got '{a}'"
                    )));
                }
```

- [ ] **Step 11: Compile and run the helper test**

Run: `cargo test --lib agent_kind_helpers_match_existing_strings`
Expected: PASS.

- [ ] **Step 12: Full build + clippy**

Run: `cargo test --lib && cargo clippy --all-targets -- -D warnings`
Expected: all green. (The existing agent-picker render test still passes because it only asserts claude/pi/hermes appear — Codex appearing too doesn't break it; that test is tightened in Task 6.)

- [ ] **Step 13: Commit**

```bash
git add src/pty/session.rs src/activity/mod.rs src/activity/codex_events.rs src/app/background.rs src/agent/pm.rs src/app/input.rs src/cli.rs
git commit -m "feat(codex): add Codex AgentKind variant and compiling skeleton"
```

---

## Task 2: `build_codex_command` — real spawn-mode → CLI mapping

Replace the Task 1 stub with the full mapping: Fresh → `codex`; Continue → `codex resume --last`; PM → `codex [resume --last] --ask-for-approval never --sandbox read-only`; yolo → `--dangerously-bypass-approvals-and-sandbox`; `WSX_CODEX_MODEL` → `-m <model>`.

**Files:**
- Modify: `src/pty/session.rs` (`build_codex_command`)
- Test: `src/pty/session.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Write failing tests for the argv mapping**

Add to the `tests` module in `src/pty/session.rs`. These assert on the `CommandBuilder`'s argv via `cmd.get_argv()` — the exact convention the existing `build_claude_command` tests use (it returns a slice of `OsString`; collect to `Vec<String>` for easy checks). Use `EnvGuard` to isolate env.

```rust
    /// Build a Codex command for `mode` and return its argv as lossy Strings.
    fn codex_argv(mode: &SpawnMode) -> Vec<String> {
        let cmd = build_codex_command(
            Path::new("/tmp/wt"),
            mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        cmd.get_argv()
            .iter()
            .map(|a| a.to_string_lossy().to_string())
            .collect()
    }

    #[test]
    fn codex_fresh_is_bare_codex_with_no_approval_flags() {
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", "codex");
        env.remove("WSX_CODEX_MODEL");
        let argv = codex_argv(&SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        });
        assert!(!argv.iter().any(|a| a == "resume"), "fresh must not resume: {argv:?}");
        assert!(
            !argv.iter().any(|a| a.starts_with("--dangerously-bypass")),
            "non-yolo must not bypass: {argv:?}"
        );
        assert!(
            !argv.iter().any(|a| a == "--ask-for-approval"),
            "dev session uses codex defaults: {argv:?}"
        );
        assert!(!argv.iter().any(|a| a == "-m"), "no model env set: {argv:?}");
    }

    #[test]
    fn codex_fresh_yolo_bypasses_approvals() {
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", "codex");
        let argv = codex_argv(&SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: true,
        });
        assert!(
            argv.iter().any(|a| a == "--dangerously-bypass-approvals-and-sandbox"),
            "yolo must bypass: {argv:?}"
        );
    }

    #[test]
    fn codex_continue_uses_resume_last() {
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", "codex");
        let argv = codex_argv(&SpawnMode::Continue {
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        });
        assert!(argv.iter().any(|a| a == "resume"), "continue must resume: {argv:?}");
        assert!(argv.iter().any(|a| a == "--last"), "continue must use --last: {argv:?}");
    }

    #[test]
    fn codex_pm_is_read_only_and_never_asks() {
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", "codex");
        let argv = codex_argv(&SpawnMode::ProjectManager {
            workspaces_json_path: std::path::PathBuf::from("/tmp/pm/workspaces.json"),
            custom_instructions: None,
            additional_dirs: vec![],
            resume: false,
            fast_mode: false,
        });
        assert!(
            argv.windows(2).any(|w| w[0] == "--ask-for-approval" && w[1] == "never"),
            "pm must never ask: {argv:?}"
        );
        assert!(
            argv.windows(2).any(|w| w[0] == "--sandbox" && w[1] == "read-only"),
            "pm must be read-only: {argv:?}"
        );
        assert!(!argv.iter().any(|a| a == "resume"), "pm fresh must not resume: {argv:?}");
    }

    #[test]
    fn codex_model_env_adds_dash_m() {
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", "codex");
        env.set("WSX_CODEX_MODEL", "gpt-5.4");
        let argv = codex_argv(&SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        });
        assert!(
            argv.windows(2).any(|w| w[0] == "-m" && w[1] == "gpt-5.4"),
            "model must be passed via -m: {argv:?}"
        );
    }
```

- [ ] **Step 2: Run the tests to verify failure**

Run: `cargo test --lib codex_`
Expected: `codex_continue_uses_resume_last`, `codex_pm_is_read_only_and_never_asks`, `codex_model_env_adds_dash_m`, `codex_fresh_yolo_bypasses_approvals` FAIL (stub emits bare `codex`). `codex_fresh_is_bare_codex_with_no_approval_flags` passes against the stub.

- [ ] **Step 3: Implement `build_codex_command`**

Replace the stub body in `src/pty/session.rs`. Note: all the flags below are accepted by both bare `codex` and the `codex resume` subcommand (verified against `codex resume --help` 0.136.0), so subcommand tokens are pushed first, then shared flags.

```rust
/// Build a `CommandBuilder` for `codex` (or whatever `WSX_CODEX_BIN` points to)
/// inside `cwd`. Inherits the current process env.
///
/// Spawn-mode mapping:
/// - `Fresh`            → `codex`
/// - `Continue`         → `codex resume --last` (cwd-filtered by Codex itself)
/// - `ProjectManager`   → `codex [resume --last]` + `--ask-for-approval never
///                         --sandbox read-only` (PM reads only, never prompts)
///
/// `yolo` adds `--dangerously-bypass-approvals-and-sandbox`. Non-yolo dev
/// sessions pass no approval flags, inheriting Codex's interactive defaults.
/// `WSX_CODEX_MODEL` (trimmed, non-empty) adds `-m <model>`.
///
/// Codex has no `--append-system-prompt`; instruction injection (doctrine /
/// rename / custom / PM prompt) is handled by `prepare_codex_workspace` via
/// AGENTS.md. The `remote` arg is unused — wsx's RemoteOpts targets Claude's
/// `--remote-control`, which is unrelated to Codex's `--remote`.
pub fn build_codex_command(
    cwd: &Path,
    mode: &SpawnMode,
    _remote: crate::agent::remote_control::RemoteOpts,
) -> CommandBuilder {
    let bin = std::env::var("WSX_CODEX_BIN").unwrap_or_else(|_| "codex".to_string());
    let mut cmd = CommandBuilder::new(bin);
    cmd.cwd(cwd);
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }

    let (resume, yolo, pm) = match mode {
        SpawnMode::Fresh { yolo, .. } => (false, *yolo, false),
        SpawnMode::Continue { yolo, .. } => (true, *yolo, false),
        SpawnMode::ProjectManager { resume, .. } => (*resume, false, true),
    };

    if resume {
        cmd.arg("resume");
        cmd.arg("--last");
    }

    if yolo {
        cmd.arg("--dangerously-bypass-approvals-and-sandbox");
    } else if pm {
        cmd.arg("--ask-for-approval");
        cmd.arg("never");
        cmd.arg("--sandbox");
        cmd.arg("read-only");
    }

    let model = std::env::var("WSX_CODEX_MODEL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if let Some(m) = model {
        cmd.arg("-m");
        cmd.arg(&m);
    }

    cmd
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib codex_`
Expected: all PASS.

- [ ] **Step 5: Spawn smoke test via the `cat` seam**

Add to the `tests` module:
```rust
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn codex_spawn_and_echo() {
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", cat_path());
        let cwd = PathBuf::from(".");
        let s = spawn_session(
            &cwd,
            80,
            24,
            SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            },
            crate::agent::remote_control::RemoteOpts::disabled(),
            AgentKind::Codex,
        )
        .unwrap();
        s.writer.send(b"hello-codex\n".to_vec()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        let screen = s.parser.lock().unwrap().screen().contents();
        assert!(screen.contains("hello-codex"), "screen: {screen:?}");
    }
```

Run: `cargo test --lib codex_spawn_and_echo`
Expected: PASS. (`prepare_codex_workspace` writes an AGENTS.md block in `.`; the Fresh mode here has no rename_ctx/doctrine/custom so `compose_injected_prompt` returns `None` and no file is written — confirm `git status` is clean after.)

- [ ] **Step 6: Commit**

```bash
git add src/pty/session.rs
git commit -m "feat(codex): map spawn modes to codex CLI flags"
```

---

## Task 3: AGENTS.md instruction injection for Codex

Confirm `prepare_codex_workspace` injects doctrine + rename + custom instructions into AGENTS.md exactly as Hermes does, by reusing `compose_injected_prompt` / `write_agents_md_section` (already agent-neutral). This task is mostly a verification-via-test that the reused machinery behaves for Codex, plus the doctrine decision.

**Files:**
- Modify: `src/agent/doctrine.rs` (add Codex doctrine test; no logic change — Codex stays excluded from superpowers like Hermes)
- Test: `src/pty/session.rs` (`prepare_codex_workspace` behavior)

- [ ] **Step 1: Write a failing test for AGENTS.md injection**

Add to the `tests` module in `src/pty/session.rs`:
```rust
    #[test]
    fn prepare_codex_workspace_injects_rename_block_into_agents_md() {
        let dir = tempfile::TempDir::new().unwrap();
        let cwd = dir.path();
        let mode = SpawnMode::Fresh {
            rename_ctx: Some(RenameContext {
                current_branch: "bakedbean/rusty-azalea".to_string(),
                branch_prefix: "bakedbean".to_string(),
                repo_name: "workspacex".to_string(),
                current_slug: "rusty-azalea".to_string(),
            }),
            custom_instructions: None,
            doctrine: Some("DOCTRINE-MARKER".to_string()),
            additional_dirs: vec![],
            yolo: false,
        };
        prepare_codex_workspace(cwd, &mode);
        let agents = std::fs::read_to_string(cwd.join("AGENTS.md")).unwrap();
        assert!(agents.contains("BEGIN wsx-managed"), "block markers: {agents}");
        assert!(agents.contains("DOCTRINE-MARKER"), "doctrine injected: {agents}");
        assert!(agents.contains("wsx workspace rename"), "rename hint: {agents}");
    }

    #[test]
    fn prepare_codex_workspace_writes_no_hermes_marker() {
        let dir = tempfile::TempDir::new().unwrap();
        let cwd = dir.path();
        std::fs::create_dir_all(cwd.join(".git/info")).unwrap();
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: Some("CUSTOM".to_string()),
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        prepare_codex_workspace(cwd, &mode);
        // Codex uses cwd-in-file detection, not the Hermes spawn marker.
        assert!(
            !cwd.join(".git/info/wsx-hermes-spawn-at").exists(),
            "codex must not write the hermes spawn marker"
        );
    }
```

- [ ] **Step 2: Run to verify**

Run: `cargo test --lib prepare_codex_workspace`
Expected: PASS already — `prepare_codex_workspace` from Task 1 reuses the correct helpers and writes no marker. (If `prepare_codex_workspace_injects_rename_block_into_agents_md` fails, the bug is in Task 1's wiring; fix it there.)

This is a confirm-by-test task: the reused machinery is correct, and these tests lock that contract so a future refactor of the Hermes helpers can't silently break Codex.

- [ ] **Step 3: Add the Codex doctrine test (locks the superpowers-exclusion decision)**

In `src/agent/doctrine.rs` `tests` module, add:
```rust
    #[test]
    fn codex_omits_superpowers_but_keeps_the_rest() {
        let d = process_doctrine(AgentKind::Codex).to_lowercase();
        assert!(
            !d.contains("superpowers"),
            "codex must NOT get superpowers clause (skills live under ~/.claude): {d}"
        );
        assert!(d.contains("plan"), "codex keeps planning clause: {d}");
        assert!(d.contains("commit"), "codex keeps commits clause: {d}");
        assert!(d.contains("wsx skill"), "codex keeps wsx skill clause: {d}");
    }
```

Note: `process_doctrine`'s `include_superpowers = matches!(agent, AgentKind::Claude | AgentKind::Pi)` already excludes Codex — no logic change needed; this test asserts and documents it.

- [ ] **Step 4: Run the doctrine test**

Run: `cargo test --lib codex_omits_superpowers`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/pty/session.rs src/agent/doctrine.rs
git commit -m "test(codex): lock AGENTS.md injection and superpowers-exclusion contracts"
```

---

## Task 4: `codex_events::locate_session_file` — match worktree by embedded cwd

Replace the locate stub: walk `~/.codex/sessions/`, sort rollout files newest-first, and return the first whose `session_meta.payload.cwd` matches the worktree (capped scan).

**Files:**
- Modify: `src/activity/codex_events.rs`

- [ ] **Step 1: Write failing tests**

Add a `tests` module to `src/activity/codex_events.rs` (HOME-swap pattern mirrors `pi_events.rs`):
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn write_rollout(dir: &Path, name: &str, cwd: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        let meta = format!(
            r#"{{"timestamp":"2026-06-02T18:51:58.969Z","type":"session_meta","payload":{{"id":"abc","cwd":"{cwd}","originator":"codex-tui"}}}}"#
        );
        std::fs::write(&path, format!("{meta}\n")).unwrap();
        path
    }

    #[test]
    fn locate_matches_embedded_cwd_and_prefers_newest() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let abs = std::fs::canonicalize(work.path()).unwrap();
        let day = home.path().join(".codex/sessions/2026/06/02");
        let other = write_rollout(&day, "rollout-A.jsonl", "/some/other/dir");
        let _ = other;
        std::thread::sleep(std::time::Duration::from_millis(20));
        let mine = write_rollout(&day, "rollout-B.jsonl", &abs.to_string_lossy());

        let original = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", home.path()); }
        let result = locate_session_file(work.path());
        match original {
            Some(h) => unsafe { std::env::set_var("HOME", h); },
            None => unsafe { std::env::remove_var("HOME"); },
        }
        assert_eq!(result, Some(mine));
    }

    #[test]
    fn locate_returns_none_when_no_cwd_matches() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let day = home.path().join(".codex/sessions/2026/06/02");
        write_rollout(&day, "rollout-A.jsonl", "/nowhere/relevant");

        let original = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", home.path()); }
        let result = locate_session_file(work.path());
        match original {
            Some(h) => unsafe { std::env::set_var("HOME", h); },
            None => unsafe { std::env::remove_var("HOME"); },
        }
        assert!(result.is_none());
    }

    #[test]
    fn locate_returns_none_when_sessions_dir_missing() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let original = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", home.path()); }
        let result = locate_session_file(work.path());
        match original {
            Some(h) => unsafe { std::env::set_var("HOME", h); },
            None => unsafe { std::env::remove_var("HOME"); },
        }
        assert!(result.is_none());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib codex_events::tests::locate`
Expected: `locate_matches_embedded_cwd_and_prefers_newest` FAILS (stub returns `None`); the two `none` tests pass vacuously.

- [ ] **Step 3: Implement `locate_session_file` + helpers**

Replace the stub in `src/activity/codex_events.rs`:
```rust
use std::time::SystemTime;

/// Cap how many rollout files we content-scan per locate, newest-first, so a
/// long session history can't make the 2s dashboard poll pathological.
const SCAN_CAP: usize = 500;

/// Locate the newest Codex rollout file whose recorded `cwd` matches `worktree`.
pub fn locate_session_file(worktree: &Path) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let abs = std::fs::canonicalize(worktree).ok()?;
    let root = home.join(".codex/sessions");
    if !root.is_dir() {
        return None;
    }
    let mut candidates: Vec<(PathBuf, SystemTime)> = Vec::new();
    collect_rollouts(&root, &mut candidates);
    candidates.sort_by(|a, b| b.1.cmp(&a.1)); // newest first
    for (path, _) in candidates.into_iter().take(SCAN_CAP) {
        if rollout_cwd_matches(&path, &abs) {
            return Some(path);
        }
    }
    None
}

/// Recursively collect `rollout-*.jsonl` files under `dir` with their mtimes.
/// The sessions tree is only three levels deep (YYYY/MM/DD), so plain
/// recursion is fine and avoids pulling in a directory-walk dependency.
fn collect_rollouts(dir: &Path, out: &mut Vec<(PathBuf, SystemTime)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            collect_rollouts(&path, out);
        } else if is_rollout_file(&path) {
            if let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) {
                out.push((path, mtime));
            }
        }
    }
}

fn is_rollout_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name.starts_with("rollout-") && name.ends_with(".jsonl")
}

/// Read only the first line of `path`, parse `session_meta.payload.cwd`, and
/// compare to `abs` (the canonical worktree). Matches on canonicalized cwd
/// when the path still exists, falling back to a raw path compare.
fn rollout_cwd_matches(path: &Path, abs: &Path) -> bool {
    use std::io::{BufRead, BufReader};
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut first = String::new();
    if BufReader::new(file).read_line(&mut first).is_err() {
        return false;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(first.trim_end()) else {
        return false;
    };
    if v.get("type").and_then(|t| t.as_str()) != Some("session_meta") {
        return false;
    }
    let Some(cwd) = v
        .get("payload")
        .and_then(|p| p.get("cwd"))
        .and_then(|c| c.as_str())
    else {
        return false;
    };
    let stored = Path::new(cwd);
    std::fs::canonicalize(stored).ok().as_deref() == Some(abs) || stored == abs
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib codex_events::tests::locate`
Expected: all PASS.

- [ ] **Step 5: Full check + commit**

Run: `cargo test --lib && cargo clippy --all-targets -- -D warnings`
```bash
git add src/activity/codex_events.rs
git commit -m "feat(codex): locate worktree session by embedded cwd"
```

---

## Task 5: `codex_events::tail_session` — parse rollout JSONL into `TailUpdate`

The core parser. Tails the rollout file by byte offset (append-only, like `pi_events`), mapping Codex's `event_msg` and `response_item` streams onto `TailUpdate`. See the design doc's mapping table for which lines map to what and which are ignored.

**Files:**
- Modify: `src/activity/codex_events.rs`

- [ ] **Step 1: Write failing parser tests**

Add to the `tests` module in `src/activity/codex_events.rs`:
```rust
    #[test]
    fn parses_user_message_event() {
        let line = r#"{"timestamp":"2026-06-02T18:56:04.390Z","type":"event_msg","payload":{"type":"user_message","message":"fix the billing bug"}}"#;
        let p = parse_jsonl_line(line);
        let ev = p.event.expect("event");
        assert_eq!(ev.kind, EventKind::UserMessage);
        assert!(ev.display.contains("fix the billing bug"));
        assert!(p.is_user_text);
    }

    #[test]
    fn parses_agent_message_event() {
        let line = r#"{"timestamp":"2026-06-02T18:56:09.622Z","type":"event_msg","payload":{"type":"agent_message","message":"I'll trace the billing path first."}}"#;
        let p = parse_jsonl_line(line);
        let ev = p.event.expect("event");
        assert_eq!(ev.kind, EventKind::AssistantText);
        assert!(ev.display.contains("trace the billing path"));
        assert_eq!(p.last_assistant_text.as_deref(), Some("I'll trace the billing path first."));
    }

    #[test]
    fn task_complete_sets_end_turn_without_duplicate_event() {
        let line = r#"{"timestamp":"2026-06-02T18:57:52.806Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"t1","last_agent_message":"Done. No edits made."}}"#;
        let p = parse_jsonl_line(line);
        assert_eq!(p.stop_reason, Some(StopReason::EndTurn));
        // task_complete must NOT push its own display event (it duplicates the
        // final agent_message), but it DOES feed the recap text trackers.
        assert!(p.event.is_none(), "no duplicate event for task_complete");
        assert_eq!(p.last_assistant_text.as_deref(), Some("Done. No edits made."));
    }

    #[test]
    fn parses_function_call_as_tool_use() {
        let line = r#"{"timestamp":"2026-06-02T18:56:09.626Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"rg -n invoice .\",\"workdir\":\"/x\"}","call_id":"call_abc"}}"#;
        let p = parse_jsonl_line(line);
        let ev = p.event.expect("event");
        assert_eq!(ev.kind, EventKind::AssistantToolUse);
        assert!(ev.display.contains("ran `rg -n invoice .`"), "display: {}", ev.display);
        assert_eq!(p.tool_use_starts.len(), 1);
        assert_eq!(p.tool_use_starts[0].0, "call_abc");
        assert_eq!(p.tool_use_starts[0].1, "exec_command");
    }

    #[test]
    fn parses_non_exec_function_call_generically() {
        let line = r#"{"timestamp":"2026-06-02T18:56:09.626Z","type":"response_item","payload":{"type":"function_call","name":"apply_patch","arguments":"{}","call_id":"call_p"}}"#;
        let p = parse_jsonl_line(line);
        let ev = p.event.expect("event");
        assert_eq!(ev.display, "using apply_patch");
    }

    #[test]
    fn parses_function_call_output_as_resolve() {
        let line = r#"{"timestamp":"2026-06-02T18:56:09.820Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call_abc","output":"ok"}}"#;
        let p = parse_jsonl_line(line);
        assert!(p.event.is_none());
        assert_eq!(p.tool_use_resolves, vec!["call_abc".to_string()]);
    }

    #[test]
    fn ignores_duplicate_assistant_response_item_and_reasoning_and_context() {
        for line in [
            r#"{"timestamp":"2026-06-02T18:56:09.623Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"dup of agent_message"}]}}"#,
            r#"{"timestamp":"2026-06-02T18:56:11.230Z","type":"response_item","payload":{"type":"reasoning","summary":[],"content":null,"encrypted_content":"xxx"}}"#,
            r#"{"timestamp":"2026-06-02T18:56:04.386Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>...</environment_context>"}]}}"#,
            r#"{"timestamp":"2026-06-02T18:56:04.382Z","type":"event_msg","payload":{"type":"token_count"}}"#,
            r#"{"timestamp":"2026-06-02T18:51:58.969Z","type":"session_meta","payload":{"cwd":"/x"}}"#,
            r#"{"timestamp":"2026-06-02T18:56:04.382Z","type":"turn_context","payload":{}}"#,
        ] {
            let p = parse_jsonl_line(line);
            assert!(p.event.is_none(), "must ignore: {line}");
            assert!(p.tool_use_starts.is_empty(), "no tool starts: {line}");
            assert!(!p.is_user_text, "not user text: {line}");
        }
    }

    #[test]
    fn tail_session_reads_then_appended_then_advances_offset() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("rollout-x.jsonl");
        let l1 = r#"{"timestamp":"2026-06-02T18:56:04.390Z","type":"event_msg","payload":{"type":"user_message","message":"hi"}}"#;
        let l2 = r#"{"timestamp":"2026-06-02T18:56:09.626Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"ls\"}","call_id":"c1"}}"#;
        std::fs::write(&path, format!("{l1}\n{l2}\n")).unwrap();

        let u = tail_session(&path, 0).unwrap();
        assert_eq!(u.events.len(), 2);
        assert_eq!(u.events[0].kind, EventKind::UserMessage);
        assert_eq!(u.events[1].kind, EventKind::AssistantToolUse);
        assert_eq!(u.tool_use_starts.len(), 1);

        let u2 = tail_session(&path, u.new_offset).unwrap();
        assert!(u2.events.is_empty());
        assert_eq!(u2.new_offset, u.new_offset);

        let l3 = r#"{"timestamp":"2026-06-02T18:56:09.820Z","type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"ok"}}"#;
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        use std::io::Write;
        writeln!(f, "{l3}").unwrap();
        let u3 = tail_session(&path, u2.new_offset).unwrap();
        assert_eq!(u3.tool_use_resolves, vec!["c1".to_string()]);
    }

    #[test]
    fn tail_session_resets_when_offset_exceeds_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("rollout-x.jsonl");
        let l1 = r#"{"timestamp":"2026-06-02T18:56:04.390Z","type":"event_msg","payload":{"type":"user_message","message":"hi"}}"#;
        std::fs::write(&path, format!("{l1}\n")).unwrap();
        let u = tail_session(&path, 9_999_999).unwrap();
        assert_eq!(u.events.len(), 1);
        assert!(u.reset_from_zero);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib codex_events`
Expected: the new parser tests FAIL (`parse_jsonl_line` / `ParsedLine` don't exist yet; `tail_session` stub returns empty).

- [ ] **Step 3: Implement the parser and real `tail_session`**

In `src/activity/codex_events.rs`, add the imports/types/parsers and replace the `tail_session` stub. Add to the top imports:
```rust
use crate::activity::events::{EventKind, EventSnapshot, StopReason, TailUpdate};
```
(remove the now-redundant `use crate::activity::events::TailUpdate;` line.)

Add display helpers (same semantics as `pi_events`):
```rust
const MAX_DISPLAY_CHARS: usize = 512;

fn truncate_display(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('\u{2026}');
        out
    }
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    out.trim().to_string()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
```

Add the `ParsedLine` type and parser:
```rust
/// Result of parsing a single Codex rollout JSONL line.
#[derive(Debug, Default)]
pub struct ParsedLine {
    pub event: Option<EventSnapshot>,
    pub tool_use_starts: Vec<(String, String, i64)>,
    pub tool_use_resolves: Vec<String>,
    pub stop_reason: Option<StopReason>,
    pub is_user_text: bool,
    pub last_assistant_text: Option<String>,
    pub longest_text_in_message: Option<String>,
}

/// Parse one Codex rollout line. Codex emits two parallel streams; we map a
/// chosen subset to avoid double-counting (see the design doc mapping table):
///   event_msg/user_message   → user turn
///   event_msg/agent_message  → assistant narration
///   event_msg/task_complete  → end_turn + recap text (no separate event)
///   response_item/function_call         → tool start
///   response_item/function_call_output  → tool resolve
/// Everything else (response_item/message, reasoning, token_count,
/// session_meta, turn_context, task_started) is ignored.
pub fn parse_jsonl_line(line: &str) -> ParsedLine {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return ParsedLine::default();
    };
    let ts = v
        .get("timestamp")
        .and_then(|t| t.as_str())
        .and_then(crate::activity::events::parse_iso8601_ms)
        .unwrap_or_else(now_ms);
    let Some(kind) = v.get("type").and_then(|t| t.as_str()) else {
        return ParsedLine::default();
    };
    let Some(payload) = v.get("payload") else {
        return ParsedLine::default();
    };
    let ptype = payload.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match (kind, ptype) {
        ("event_msg", "user_message") => parse_user_message(payload, ts),
        ("event_msg", "agent_message") => parse_agent_message(payload, ts),
        ("event_msg", "task_complete") => parse_task_complete(payload),
        ("response_item", "function_call") => parse_function_call(payload, ts),
        ("response_item", "function_call_output") => parse_function_call_output(payload),
        _ => ParsedLine::default(),
    }
}

fn parse_user_message(payload: &serde_json::Value, ts: i64) -> ParsedLine {
    let mut out = ParsedLine::default();
    let Some(msg) = payload.get("message").and_then(|m| m.as_str()) else {
        return out;
    };
    let trimmed = msg.trim();
    if trimmed.is_empty() {
        return out;
    }
    out.event = Some(EventSnapshot {
        kind: EventKind::UserMessage,
        display: truncate_display(&format!("user: {}", collapse_ws(trimmed)), MAX_DISPLAY_CHARS),
        timestamp_ms: ts,
    });
    out.is_user_text = true;
    out
}

fn parse_agent_message(payload: &serde_json::Value, ts: i64) -> ParsedLine {
    let mut out = ParsedLine::default();
    let Some(msg) = payload.get("message").and_then(|m| m.as_str()) else {
        return out;
    };
    let trimmed = msg.trim();
    if trimmed.is_empty() {
        return out;
    }
    out.last_assistant_text = Some(trimmed.to_string());
    out.longest_text_in_message = Some(trimmed.to_string());
    out.event = Some(EventSnapshot {
        kind: EventKind::AssistantText,
        display: truncate_display(&collapse_ws(trimmed), MAX_DISPLAY_CHARS),
        timestamp_ms: ts,
    });
    out
}

fn parse_task_complete(payload: &serde_json::Value) -> ParsedLine {
    let mut out = ParsedLine::default();
    out.stop_reason = Some(StopReason::EndTurn);
    // Feed the recap text into the trackers but DO NOT push a display event:
    // last_agent_message duplicates the final agent_message we already emitted.
    if let Some(msg) = payload.get("last_agent_message").and_then(|m| m.as_str()) {
        let trimmed = msg.trim();
        if !trimmed.is_empty() {
            out.last_assistant_text = Some(trimmed.to_string());
            out.longest_text_in_message = Some(trimmed.to_string());
        }
    }
    out
}

fn parse_function_call(payload: &serde_json::Value, ts: i64) -> ParsedLine {
    let mut out = ParsedLine::default();
    let name = payload.get("name").and_then(|n| n.as_str()).unwrap_or("");
    if let Some(id) = payload.get("call_id").and_then(|i| i.as_str()) {
        out.tool_use_starts
            .push((id.to_string(), name.to_string(), ts));
    }
    // Codex `arguments` is a JSON-encoded STRING; exec_command carries a `cmd`.
    let display = if name == "exec_command" {
        let cmd = payload
            .get("arguments")
            .and_then(|a| a.as_str())
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .and_then(|args| args.get("cmd").and_then(|c| c.as_str()).map(str::to_string));
        match cmd {
            Some(c) => format!("ran `{}`", collapse_ws(&c)),
            None => "ran a command".to_string(),
        }
    } else if name.is_empty() {
        "using a tool".to_string()
    } else {
        format!("using {name}")
    };
    out.event = Some(EventSnapshot {
        kind: EventKind::AssistantToolUse,
        display: truncate_display(&display, MAX_DISPLAY_CHARS),
        timestamp_ms: ts,
    });
    out
}

fn parse_function_call_output(payload: &serde_json::Value) -> ParsedLine {
    let mut out = ParsedLine::default();
    if let Some(id) = payload.get("call_id").and_then(|i| i.as_str()) {
        out.tool_use_resolves.push(id.to_string());
    }
    out
}
```

Replace the `tail_session` stub with the byte-offset tailer (structurally identical to `pi_events::tail_session`):
```rust
/// Read new lines from `path` starting at `offset` and parse them as Codex
/// rollout JSONL. Returns the new committed offset and parsed events.
pub fn tail_session(path: &Path, offset: u64) -> Result<TailUpdate> {
    use std::io::{BufRead, BufReader, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let file_size = file.metadata()?.len();
    let reset_from_zero = offset > file_size;
    let start = if reset_from_zero { 0 } else { offset };
    file.seek(SeekFrom::Start(start))?;
    let mut reader = BufReader::new(file);
    let mut update = TailUpdate {
        reset_from_zero,
        ..TailUpdate::default()
    };
    let mut buf = String::new();
    let mut consumed = start;
    loop {
        buf.clear();
        let n = reader.read_line(&mut buf)?;
        if n == 0 {
            break;
        }
        if !buf.ends_with('\n') {
            break; // ignore an unterminated trailing line; reread next tick
        }
        consumed += n as u64;
        let parsed = parse_jsonl_line(buf.trim_end());
        if let Some(snap) = parsed.event {
            update.events.push(snap);
        }
        update.tool_use_starts.extend(parsed.tool_use_starts);
        update.tool_use_resolves.extend(parsed.tool_use_resolves);
        if let Some(sr) = parsed.stop_reason {
            update.last_stop_reason = Some(sr);
            update.human_replied_after_last_stop = false;
            update.last_user_interrupted = Some(false);
        }
        if parsed.is_user_text {
            update.human_replied_after_last_stop = true;
            update.last_user_interrupted = Some(false);
        }
        if let Some(longest) = parsed.longest_text_in_message {
            let len = longest.chars().count();
            let beats = update
                .longest_assistant_text_in_batch
                .as_ref()
                .map(|cur| cur.chars().count() < len)
                .unwrap_or(true);
            if beats {
                update.longest_assistant_text_in_batch = Some(longest);
            }
        }
        if let Some(text) = parsed.last_assistant_text {
            update.last_assistant_text = Some(text);
        }
    }
    update.new_offset = consumed;
    Ok(update)
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib codex_events`
Expected: all PASS.

- [ ] **Step 5: Full check + commit**

Run: `cargo test --lib && cargo clippy --all-targets -- -D warnings`
```bash
git add src/activity/codex_events.rs
git commit -m "feat(codex): parse rollout JSONL into activity TailUpdate"
```

---

## Task 6: Tighten the agent-picker test for four agents

The picker modal iterates `AgentKind::ALL`, so Codex already renders. Update the test to assert it and rename it for accuracy.

**Files:**
- Modify: `src/app/input_tests.rs` (`agent_picker_modal_renders_three_agents_with_current_marker`, ~line 3461)

- [ ] **Step 1: Update the test**

Rename and add the Codex assertion:
```rust
    fn agent_picker_modal_renders_four_agents_with_current_marker() {
```
After the `hermes` assertion block, add:
```rust
        assert!(
            rendered.contains("codex"),
            "expected codex row: {rendered}"
        );
```

- [ ] **Step 2: Run it**

Run: `cargo test --lib agent_picker_modal_renders_four_agents_with_current_marker`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/app/input_tests.rs
git commit -m "test(codex): assert agent picker renders codex row"
```

---

## Task 7: README documentation

Document Codex everywhere the other three agents are documented.

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Locate the agent documentation sections**

Run: `grep -n "hermes\|WSX_HERMES_BIN\|coding_agent\|--agent" README.md`
This surfaces: the coding-agent overview, the agents table, the per-agent detail section, the `coding_agent` config row, the `workspace create` help line, and the env-var table. Update each to include Codex.

- [ ] **Step 2: Add Codex to the agents table and overview**

Wherever the supported agents are listed (e.g. "Claude, Pi, and Hermes"), add Codex. In the agents table, add a row:

```markdown
| `codex` | `codex` | `WSX_CODEX_BIN` | `~/.codex/config.toml` |
```

- [ ] **Step 3: Add the per-agent Codex detail subsection**

Mirror the Hermes subsection's depth. Content to include verbatim:

```markdown
### Codex

- **Binary:** `codex` (override with `WSX_CODEX_BIN`).
- **Spawn:** fresh workspaces launch bare `codex`; non-yolo sessions use Codex's
  built-in interactive approvals + `workspace-write` sandbox, `--yolo` workspaces
  add `--dangerously-bypass-approvals-and-sandbox`.
- **Continue:** `codex resume --last`, which Codex filters to the current
  directory natively — so wsx resumes the worktree's own most-recent session.
- **Instructions:** Codex has no `--append-system-prompt`; wsx injects the
  workspace doctrine, the auto-rename hint, and any custom instructions into a
  `wsx-managed` block in the worktree's `AGENTS.md` (git-excluded), the same
  mechanism used for Hermes.
- **Model:** set `WSX_CODEX_MODEL` to pass `-m <model>` (e.g. `gpt-5.4`).
- **Activity:** the dashboard detail bar tails the worktree's rollout file under
  `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`. RECENT FILES is not yet
  populated for Codex (file edits are inferred-via-shell and not tracked).
- **Superpowers:** the superpowers-skills doctrine clause is omitted for Codex
  (those skills install under `~/.claude` and Codex can't load them).
```

- [ ] **Step 4: Update the `coding_agent` config row and `--agent` help**

In the `coding_agent` config description, change the allowed values to `claude` (default), `pi`, `hermes`, `codex`. Update the `workspace create` usage line to `[--agent claude|pi|hermes|codex]`.

- [ ] **Step 5: Add env-var rows**

In the environment-variables table, add:
```markdown
| `WSX_CODEX_BIN` | Path to the `codex` binary (default: `codex` on `PATH`). |
| `WSX_CODEX_MODEL` | Model passed to Codex as `-m` (e.g. `gpt-5.4`). Unset = Codex default. |
```

- [ ] **Step 6: Commit**

```bash
git add README.md
git commit -m "docs(codex): document Codex agent, env vars, and activity behavior"
```

---

## Final verification

- [ ] **Step 1: Full test suite**

Run: `cargo test`
Expected: all pass (lib + integration tests under `tests/`).

- [ ] **Step 2: Clippy clean**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 3: Manual smoke (optional, requires real codex)**

```bash
cargo run -- workspace create <some-repo> --agent codex
```
Open the workspace, confirm `codex` launches, type a first message, confirm the auto-rename fires (AGENTS.md block present and git-excluded), detach, and confirm the dashboard detail bar shows recent chat/tool activity for the Codex session. Reopen and confirm `codex resume --last` picks up the same session.

---

## Self-review notes (for the executor)

- **Spec coverage:** CLI mapping → Task 2; AGENTS.md injection + superpowers exclusion → Task 3; session detection/location → Tasks 1 (`has_prior_codex_session`) + 4; activity parser → Task 5; PM dossier unsupported-marker → Task 1 Step 8; CLI/store/UI/docs → Tasks 1, 6, 7. The one explicit spec gap — `edited_file_paths` empty for Codex — is intentional and asserted indirectly (RECENT FILES blank); no task populates it, by design.
- **Type consistency:** `ParsedLine` fields and `EventSnapshot { kind, display, timestamp_ms }` / `EventKind::{UserMessage, AssistantText, AssistantToolUse}` / `StopReason::{EndTurn, ToolUse, MaxTokens, Other}` match the shared definitions in `src/activity/events.rs`. `build_codex_command` / `has_prior_codex_session` / `prepare_codex_workspace` signatures match their call sites added in Task 1.
- **Ordering:** Task 1 is the only task that must land first (it makes the crate compile); Tasks 2–7 each compile and test independently and could be reordered.
