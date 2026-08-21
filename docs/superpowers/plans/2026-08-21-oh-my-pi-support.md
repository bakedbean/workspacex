# Oh My Pi (`omp`) Agent Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `AgentKind::Omp` to wsx so `--agent omp` spawns an oh-my-pi session with the same feature set the Claude / Codex / Pi integrations have.

**Architecture:** A fifth enum variant threaded through the existing per-agent seams — a parallel `build_omp_command`, a wsx-local `omp_events` module that reuses `sessionx`'s pi JSONL parser, and per-agent arms for readiness, prior-session detection, doctrine, and theme. No new abstraction: the compiler's exhaustive-match check enforces most of the touchpoint list.

**Tech Stack:** Rust 2024 (see `rust-toolchain.toml`), `portable_pty` for spawn command building, `vt100` for terminal replay tests, `sessionx` (pinned git dep) for JSONL parsing, `tempfile` + the repo's own `EnvGuard` for env-sensitive tests.

**Spec:** `docs/superpowers/specs/2026-08-21-oh-my-pi-support-design.md`

## Global Constraints

- The identifier is **`omp`** everywhere: `--agent omp`, `wsx agent add omp`, `coding_agent` setting value, `display_name()`, dashboard chips, tmux session names. Never `oh-my-pi`.
- Env vars: **`WSX_OMP_BIN`** (default `omp`), **`WSX_OMP_MODEL`**. There is deliberately **no** `WSX_OMP_PROVIDER`.
- omp is an **addition**. `AgentKind::Pi` keeps meaning the `@earendil-works/pi-coding-agent` binary `pi`. Never repoint, rename, or remove it.
- Empty/whitespace env-var values are treated as **unset** (a shell expands `export FOO=$UNSET` to `""`); never emit a flag with an empty value.
- `agent::status::for_agent(AgentKind::Omp)` stays **`NoopStatus`**. Do not invent a status mechanism — omp has no turn-lifecycle hook.
- Do **not** add an `InstallTarget` for omp in `src/agent/skill.rs`. omp reads `~/.claude/skills`, so the existing Claude target already covers it (same reasoning as Pi).
- CI gates are separate: `cargo fmt --check`, `cargo clippy`, `cargo test`. Clippy passing does not imply fmt is clean — run both.
- Follow the house comment style in the files you touch: doc comments explain *why*, and they cite measured/observed facts rather than assumptions.

---

### Task 1: Add the `Omp` variant and make the crate compile

Adds the enum variant and every arm the compiler demands. At the end of this task `wsx workspace create --agent omp` spawns a bare `omp` — no flags, no activity, no color yet — and the whole test suite passes.

**Files:**
- Modify: `src/pty/agent_kind.rs`
- Modify: `src/pty/session.rs` (bin env var ~line 69; ready predicate ~line 431; submit writes ~line 558; spawn dispatch ~line 664)
- Modify: `src/pty/command.rs` (new stub builder)
- Modify: `src/pty/session_detect.rs` (`has_prior_session_for` ~line 376)
- Modify: `src/app/background.rs` (~lines 39 and 72)
- Modify: `src/ui/theme.rs` (`agent_style` ~line 362)
- Test: `src/pty/session.rs` (co-located `mod tests`)

**Interfaces:**
- Consumes: nothing.
- Produces: `AgentKind::Omp`; `AgentKind::ALL: [AgentKind; 5]`; `pub fn build_omp_command(cwd: &Path, mode: &SpawnMode, remote: crate::agent::remote_control::RemoteOpts) -> CommandBuilder` (stub in this task, filled in Task 2).

- [ ] **Step 1: Write the failing test**

In `src/pty/session.rs`, find the existing taxonomy test (search for `AgentKind::ALL.len(), 4`) and extend it:

```rust
        assert_eq!(AgentKind::ALL.len(), 5);
        assert!(AgentKind::ALL.contains(&AgentKind::Claude));
        assert!(AgentKind::ALL.contains(&AgentKind::Pi));
        assert!(AgentKind::ALL.contains(&AgentKind::Hermes));
        assert!(AgentKind::ALL.contains(&AgentKind::Codex));
        assert!(AgentKind::ALL.contains(&AgentKind::Omp));

        assert_eq!(AgentKind::Claude.display_name(), "claude");
        assert_eq!(AgentKind::Pi.display_name(), "pi");
        assert_eq!(AgentKind::Hermes.display_name(), "hermes");
        assert_eq!(AgentKind::Codex.display_name(), "codex");
        assert_eq!(AgentKind::Omp.display_name(), "omp");

        assert_eq!(AgentKind::Claude.default_binary(), "claude");
        assert_eq!(AgentKind::Pi.default_binary(), "pi");
        assert_eq!(AgentKind::Hermes.default_binary(), "hermes");
        assert_eq!(AgentKind::Codex.default_binary(), "codex");
        assert_eq!(AgentKind::Omp.default_binary(), "omp");
```

Then add a new test in the same `mod tests` asserting `omp` is not confused with `pi`:

```rust
    /// oh-my-pi (`omp`, @oh-my-pi/pi-coding-agent) and pi (`pi`,
    /// @earendil-works/pi-coding-agent) are separate harnesses that are both
    /// installed on real machines. A store value of "pi" must never resolve to
    /// Omp, and vice versa — a swap here silently spawns the wrong binary.
    #[test]
    fn omp_and_pi_are_distinct_kinds() {
        assert_eq!(AgentKind::from_str_or_default(Some("omp")), AgentKind::Omp);
        assert_eq!(AgentKind::from_str_or_default(Some("pi")), AgentKind::Pi);
        assert_ne!(AgentKind::Omp, AgentKind::Pi);
        assert_eq!(AgentKind::Omp.store_value(), "omp");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib omp_and_pi_are_distinct_kinds`
Expected: FAIL to **compile**, with `no variant or associated item named 'Omp' found for enum 'AgentKind'`. A compile failure is the expected failure here — the variant does not exist yet.

- [ ] **Step 3: Add the variant**

In `src/pty/agent_kind.rs`:

```rust
/// Which coding agent to spawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Claude,
    Pi,
    Hermes,
    Codex,
    /// oh-my-pi (`omp`). A separate harness from [`AgentKind::Pi`] despite the
    /// shared ancestry: `@oh-my-pi/pi-coding-agent` vs
    /// `@earendil-works/pi-coding-agent`, different binaries, both installable
    /// at once.
    Omp,
}

impl AgentKind {
    /// All agent kinds, in stable display order. Add new variants here when
    /// extending the enum — `const` arrays do not get exhaustiveness checking,
    /// so this is the one place the compiler can't catch a drift.
    pub const ALL: [AgentKind; 5] = [
        AgentKind::Claude,
        AgentKind::Pi,
        AgentKind::Hermes,
        AgentKind::Codex,
        AgentKind::Omp,
    ];

    pub fn from_str_or_default(s: Option<&str>) -> Self {
        match s {
            Some("pi") => AgentKind::Pi,
            Some("hermes") => AgentKind::Hermes,
            Some("codex") => AgentKind::Codex,
            Some("omp") => AgentKind::Omp,
            _ => AgentKind::Claude,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Pi => "pi",
            AgentKind::Hermes => "hermes",
            AgentKind::Codex => "codex",
            AgentKind::Omp => "omp",
        }
    }
```

Leave `default_binary`, `store_value`, and `from_store` untouched — they delegate to `display_name`.

- [ ] **Step 4: Add the stub command builder**

In `src/pty/command.rs`, at the end of the builder functions (after `build_codex_command`):

```rust
/// Build a `CommandBuilder` for `omp` (or whatever `WSX_OMP_BIN` points to)
/// inside `cwd`. Inherits the current process env.
///
/// Stub: Task 2 of the oh-my-pi plan replaces this with the real spawn-mode
/// mapping. Spawning a bare `omp` is a correct, if featureless, session.
pub fn build_omp_command(
    cwd: &Path,
    _mode: &SpawnMode,
    _remote: crate::agent::remote_control::RemoteOpts,
) -> CommandBuilder {
    let bin = std::env::var("WSX_OMP_BIN").unwrap_or_else(|_| "omp".to_string());
    let mut cmd = CommandBuilder::new(bin);
    cmd.cwd(cwd);
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }
    cmd
}
```

Also add `omp` to the module doc comment on line 3 so it reads `(claude/pi/hermes/codex/omp)`.

- [ ] **Step 5: Add the arms the compiler now demands**

`src/pty/session.rs`, bin env var (~line 69):

```rust
        AgentKind::Omp => "WSX_OMP_BIN",
```

`src/pty/session.rs`, `ready_for_input` (~line 431). Task 5 replaces this with a real predicate derived from a capture; until then it must be honest about being a placeholder:

```rust
        // Placeholder until Task 5 of the oh-my-pi plan lands a predicate read
        // off a real cold boot. Do not ship a guessed signal here.
        AgentKind::Omp => true,
```

`src/pty/session.rs`, `submit_writes` (~line 558) — extend the plain-text arm:

```rust
        AgentKind::Claude | AgentKind::Pi | AgentKind::Hermes | AgentKind::Omp => {
            (text.as_bytes().to_vec(), enter)
        }
```

`src/pty/session.rs`, spawn dispatch (~line 664):

```rust
        AgentKind::Omp => build_omp_command(cwd, &mode, remote),
```

Make sure `build_omp_command` is in the `pty::session` re-export list beside `build_codex_command` (search `build_codex_command` in `src/pty/session.rs` and `src/pty/mod.rs` and mirror every place it appears in a `pub use`).

`src/pty/session_detect.rs`, `has_prior_session_for` (~line 376). Task 4 fills this in:

```rust
        // Task 4 of the oh-my-pi plan replaces this with a real lookup.
        AgentKind::Omp => false,
```

`src/app/background.rs`, both matches (~lines 39 and 72). Task 4 fills these in:

```rust
        // Task 4 of the oh-my-pi plan wires omp_events here.
        crate::pty::session::AgentKind::Omp => None,
```

and

```rust
        crate::pty::session::AgentKind::Omp => return,
```

Read the surrounding code before writing the second arm — the first match produces `Option<PathBuf>` and the second produces a `Result<TailUpdate, _>`. If an early `return` does not fit the expression position, bind `None`/an error value instead; the requirement is only that omp tails nothing until Task 4.

`src/ui/theme.rs`, `agent_style` (~line 362). Task 6 gives omp its own color:

```rust
            // Task 6 of the oh-my-pi plan gives omp its own color.
            AgentKind::Omp => AGENT_PI,
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --lib`
Expected: PASS. If a test outside the ones you edited fails on a hardcoded count of agents, fix it to the new count — do not weaken the assertion.

- [ ] **Step 7: Verify the binary actually spawns**

Run:
```bash
cargo build && ./target/debug/wsx agent add omp --help 2>&1 | head -5
```
Expected: no "kind must be one of" error for `omp` (`agent add` already validates against `AgentKind::ALL`, so the new variant is accepted automatically — this is the check that the `ALL` array was updated).

- [ ] **Step 8: Format, lint, commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
git add -A
git commit -m "feat(omp): add the Omp agent kind

Adds AgentKind::Omp (oh-my-pi, the \`omp\` binary from
@oh-my-pi/pi-coding-agent) as a fifth harness, distinct from the existing
Pi (@earendil-works/pi-coding-agent). This commit is the skeleton: the
variant, ALL, from_str_or_default, and every match arm the compiler
demands. Spawning works and produces a bare \`omp\`; flags, activity
tailing, readiness and color arrive in following commits.

Claude-Session: https://claude.ai/code/session_012R82JNY8GLGFhHD1kAAKr5"
```

---

### Task 2: `build_omp_command` — the real spawn-mode → CLI mapping

**Files:**
- Modify: `src/pty/command.rs` (replace the Task 1 stub)
- Test: `src/pty/command.rs` (co-located `mod tests`, new `mod omp_build_command`)

**Interfaces:**
- Consumes: `AgentKind::Omp` and the stub `build_omp_command` from Task 1; `SpawnMode` (`Fresh { rename_ctx, custom_instructions, doctrine, additional_dirs, yolo }` / `Continue { custom_instructions, doctrine, additional_dirs, yolo }`) from `src/pty/session.rs`; the existing private `render_rename_system_prompt_pi(current_branch, branch_prefix, repo_name, current_slug) -> String`.
- Produces: the final `build_omp_command` signature, unchanged from Task 1.

- [ ] **Step 1: Write the failing tests**

Add to `src/pty/command.rs`'s co-located `mod tests`, alongside the existing `mod hermes_build_command`:

```rust
    mod omp_build_command {
        use super::*;

        /// Build an omp command for `mode` and return its argv as lossy Strings.
        fn omp_argv(mode: &SpawnMode) -> Vec<String> {
            let cmd = super::super::build_omp_command(
                Path::new("/tmp/wt"),
                mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            cmd.get_argv()
                .iter()
                .map(|a| a.to_string_lossy().to_string())
                .collect()
        }

        fn fresh(yolo: bool) -> SpawnMode {
            SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo,
            }
        }

        #[test]
        fn fresh_is_bare_omp_with_no_approval_flags() {
            let mut env = EnvGuard::new();
            env.set("WSX_OMP_BIN", "omp");
            env.remove("WSX_OMP_MODEL");
            let argv = omp_argv(&fresh(false));
            assert!(
                !argv.iter().any(|a| a == "-c" || a == "--continue"),
                "fresh must not continue: {argv:?}"
            );
            assert!(
                !argv.iter().any(|a| a == "--approval-mode"),
                "a non-yolo session inherits the user's configured \
                 tools.approvalMode: {argv:?}"
            );
            assert!(
                !argv.iter().any(|a| a == "--model"),
                "no model env set: {argv:?}"
            );
            assert!(
                !argv.iter().any(|a| a == "--append-system-prompt"),
                "nothing to inject: {argv:?}"
            );
        }

        #[test]
        fn fresh_yolo_uses_approval_mode_yolo() {
            let mut env = EnvGuard::new();
            env.set("WSX_OMP_BIN", "omp");
            env.remove("WSX_OMP_MODEL");
            let argv = omp_argv(&fresh(true));
            let i = argv
                .iter()
                .position(|a| a == "--approval-mode")
                .unwrap_or_else(|| panic!("expected --approval-mode: {argv:?}"));
            assert_eq!(argv[i + 1], "yolo", "{argv:?}");
        }

        #[test]
        fn continue_uses_dash_c() {
            let mut env = EnvGuard::new();
            env.set("WSX_OMP_BIN", "omp");
            env.remove("WSX_OMP_MODEL");
            let argv = omp_argv(&SpawnMode::Continue {
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            });
            assert!(argv.iter().any(|a| a == "-c"), "{argv:?}");
        }

        #[test]
        fn model_env_adds_model_flag() {
            let mut env = EnvGuard::new();
            env.set("WSX_OMP_BIN", "omp");
            env.set("WSX_OMP_MODEL", "anthropic/claude-opus-5");
            let argv = omp_argv(&fresh(false));
            let i = argv
                .iter()
                .position(|a| a == "--model")
                .unwrap_or_else(|| panic!("expected --model: {argv:?}"));
            assert_eq!(argv[i + 1], "anthropic/claude-opus-5", "{argv:?}");
        }

        /// `export WSX_OMP_MODEL=$UNSET` expands to "" in every POSIX shell.
        /// Emitting `--model ""` makes omp fail to resolve a model at all, so
        /// blank must read as unset.
        #[test]
        fn blank_model_env_is_treated_as_unset() {
            let mut env = EnvGuard::new();
            env.set("WSX_OMP_BIN", "omp");
            env.set("WSX_OMP_MODEL", "   ");
            let argv = omp_argv(&fresh(false));
            assert!(
                !argv.iter().any(|a| a == "--model"),
                "blank model env must emit no flag: {argv:?}"
            );
        }

        /// Continue restores omp's stored session config, so a model override
        /// on resume would silently fight the session's own model.
        #[test]
        fn continue_omits_the_model_flag() {
            let mut env = EnvGuard::new();
            env.set("WSX_OMP_BIN", "omp");
            env.set("WSX_OMP_MODEL", "anthropic/claude-opus-5");
            let argv = omp_argv(&SpawnMode::Continue {
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            });
            assert!(
                !argv.iter().any(|a| a == "--model"),
                "resume keeps the session's own model: {argv:?}"
            );
        }

        #[test]
        fn additional_dirs_each_get_an_add_dir_flag() {
            let mut env = EnvGuard::new();
            env.set("WSX_OMP_BIN", "omp");
            env.remove("WSX_OMP_MODEL");
            let argv = omp_argv(&SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![
                    std::path::PathBuf::from("/srv/a"),
                    std::path::PathBuf::from("/srv/b"),
                ],
                yolo: false,
            });
            let dirs: Vec<&String> = argv
                .iter()
                .enumerate()
                .filter(|(i, _)| *i > 0 && argv[i - 1] == "--add-dir")
                .map(|(_, a)| a)
                .collect();
            assert_eq!(dirs, vec!["/srv/a", "/srv/b"], "{argv:?}");
        }

        #[test]
        fn doctrine_rename_and_custom_compose_into_one_system_prompt() {
            let mut env = EnvGuard::new();
            env.set("WSX_OMP_BIN", "omp");
            env.set("WSX_RENAME_MODE", "claude");
            env.remove("WSX_OMP_MODEL");
            let argv = omp_argv(&SpawnMode::Fresh {
                rename_ctx: Some(RenameContext {
                    current_branch: "wsx/bold-fern".into(),
                    branch_prefix: "wsx".into(),
                    repo_name: "myrepo".into(),
                    current_slug: "bold-fern".into(),
                }),
                custom_instructions: Some("CUSTOM_MARK".into()),
                doctrine: Some("DOCTRINE_MARK".into()),
                additional_dirs: vec![],
                yolo: false,
            });
            let i = argv
                .iter()
                .position(|a| a == "--append-system-prompt")
                .unwrap_or_else(|| panic!("expected the flag: {argv:?}"));
            let prompt = &argv[i + 1];
            assert!(
                prompt.starts_with("DOCTRINE_MARK"),
                "doctrine must lead: {prompt}"
            );
            assert!(prompt.contains("wsx workspace rename"), "{prompt}");
            assert!(prompt.contains("bold-fern"), "{prompt}");
            assert!(prompt.contains("CUSTOM_MARK"), "{prompt}");
            assert_eq!(
                argv.iter().filter(|a| *a == "--append-system-prompt").count(),
                1,
                "exactly one system-prompt flag: {argv:?}"
            );
        }

        /// `WSX_RENAME_MODE` off means wsx renames the workspace itself, so the
        /// agent must not also be told to.
        #[test]
        fn rename_prompt_is_omitted_when_rename_mode_is_not_claude() {
            let mut env = EnvGuard::new();
            env.set("WSX_OMP_BIN", "omp");
            env.set("WSX_RENAME_MODE", "wsx");
            env.remove("WSX_OMP_MODEL");
            let argv = omp_argv(&SpawnMode::Fresh {
                rename_ctx: Some(RenameContext {
                    current_branch: "wsx/bold-fern".into(),
                    branch_prefix: "wsx".into(),
                    repo_name: "myrepo".into(),
                    current_slug: "bold-fern".into(),
                }),
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            });
            assert!(
                !argv.iter().any(|a| a == "--append-system-prompt"),
                "nothing left to inject: {argv:?}"
            );
        }
    }
```

Before running: check how the neighbouring `mod hermes_build_command` imports `EnvGuard`, `RenameContext`, `SpawnMode`, and `Path`, and mirror it exactly — the `use super::*;` chain differs between nested test modules in this file. Also confirm `RenameContext`'s field names and types by reading its definition in `src/pty/session.rs` (search `pub struct RenameContext`); if a field is not a `String`, adjust the literals.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib omp_build_command`
Expected: FAIL. `fresh_is_bare_omp_with_no_approval_flags` passes trivially against the stub, but `fresh_yolo_uses_approval_mode_yolo`, `continue_uses_dash_c`, `model_env_adds_model_flag`, `additional_dirs_each_get_an_add_dir_flag`, and `doctrine_rename_and_custom_compose_into_one_system_prompt` all fail with "expected …".

- [ ] **Step 3: Write the implementation**

Replace the Task 1 stub in `src/pty/command.rs`:

```rust
/// Build a `CommandBuilder` for `omp` (or whatever `WSX_OMP_BIN` points to)
/// inside `cwd`. Inherits the current process env.
///
/// Maps wsx spawn modes to oh-my-pi CLI flags:
/// - `Fresh`    → bare `omp`, plus `--model` when `WSX_OMP_MODEL` is set.
/// - `Continue` → `-c`. omp's `SessionManager.continueRecent` falls back to the
///   newest session in the **cwd-encoded** session directory when no terminal
///   breadcrumb matches, and every wsx spawn is a fresh PTY with a fresh
///   terminal id — so a bare `-c` already resumes this worktree's own session.
///   No marker file or db query is needed (unlike Hermes).
///
/// Yolo maps to `--approval-mode yolo` rather than the equivalent
/// `--auto-approve` because it is the same knob as omp's persistent
/// `tools.approvalMode` setting, so a wsx yolo workspace and a user-configured
/// yolo session are visibly the same state. Non-yolo sessions pass **no**
/// approval flag at all, inheriting whatever the user configured — wsx should
/// not silently downgrade a harness's interactive defaults.
///
/// omp is the only harness besides Claude that supports both
/// `--append-system-prompt` and `--add-dir`, so instruction injection and
/// related-repo context both go through real flags — no AGENTS.md rewriting
/// (Hermes) and no `-c` config overrides (Codex).
///
/// Skills and slash commands need no wiring: omp's Claude discovery provider
/// loads `~/.claude/skills/*/SKILL.md` and `~/.claude/commands/*.md` natively,
/// so wsx's installed skills and the user's pinned commands already reach it.
pub fn build_omp_command(
    cwd: &Path,
    mode: &SpawnMode,
    _remote: crate::agent::remote_control::RemoteOpts,
) -> CommandBuilder {
    let bin = std::env::var("WSX_OMP_BIN").unwrap_or_else(|_| "omp".to_string());
    let mut cmd = CommandBuilder::new(bin);
    cmd.cwd(cwd);
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }

    let (doctrine, rename_prompt, custom, add_dirs, add_continue, yolo) = match mode {
        SpawnMode::Continue {
            custom_instructions,
            doctrine,
            additional_dirs,
            yolo,
        } => (
            doctrine.clone(),
            None,
            custom_instructions.clone(),
            additional_dirs.clone(),
            true,
            *yolo,
        ),
        SpawnMode::Fresh {
            rename_ctx,
            custom_instructions,
            doctrine,
            additional_dirs,
            yolo,
        } => {
            let rename_mode =
                std::env::var("WSX_RENAME_MODE").unwrap_or_else(|_| "claude".to_string());
            let rp = match rename_ctx {
                Some(ctx) if rename_mode == "claude" => Some(render_rename_system_prompt_pi(
                    &ctx.current_branch,
                    &ctx.branch_prefix,
                    &ctx.repo_name,
                    &ctx.current_slug,
                )),
                _ => None,
            };
            (
                doctrine.clone(),
                rp,
                custom_instructions.clone(),
                additional_dirs.clone(),
                false,
                *yolo,
            )
        }
    };

    for dir in &add_dirs {
        cmd.arg("--add-dir");
        cmd.arg(dir);
    }

    if add_continue {
        // Resume restores the session's stored model and approval config, so
        // re-asserting `--model` here would fight the session's own choice.
        cmd.arg("-c");
    } else {
        // Empty/whitespace reads as unset: a shell expands `export FOO=$UNSET`
        // to "", and `--model ""` leaves omp with no resolvable model.
        if let Some(model) = std::env::var("WSX_OMP_MODEL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            cmd.arg("--model");
            cmd.arg(&model);
        }
    }

    if yolo {
        cmd.arg("--approval-mode");
        cmd.arg("yolo");
    }

    let parts: Vec<String> = [doctrine, rename_prompt, custom]
        .into_iter()
        .flatten()
        .collect();
    if !parts.is_empty() {
        cmd.arg("--append-system-prompt");
        cmd.arg(parts.join("\n\n"));
    }

    cmd
}
```

Note the deliberate reuse of `render_rename_system_prompt_pi` rather than a fourth byte-identical copy: omp takes the same `--append-system-prompt` flag, has the same plain-bash tool surface, and there is no omp-specific wording to diverge on. If a divergence ever appears, split it then — the existing `render_rename_system_prompt_hermes` shows the shape.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib omp_build_command`
Expected: PASS, 9 tests.

- [ ] **Step 5: Verify against the real binary**

```bash
cargo build
WSX_OMP_BIN=omp ./target/debug/wsx --version >/dev/null   # sanity: binary builds
omp --approval-mode yolo --add-dir /tmp -p "reply with READY" --no-tools
```
Expected: the `omp` invocation exits 0 and prints `READY`, confirming the space-separated flag forms the builder emits are accepted by the installed binary.

- [ ] **Step 6: Format, lint, commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test --lib
git add -A
git commit -m "feat(omp): map spawn modes to oh-my-pi CLI flags

Fresh spawns bare omp (plus --model from WSX_OMP_MODEL); Continue passes
-c, which omp already scopes to the cwd's session directory; yolo maps to
--approval-mode yolo while non-yolo inherits the user's configured
tools.approvalMode; doctrine, rename prompt and custom instructions
compose into one --append-system-prompt; related repos ride on --add-dir.

Claude-Session: https://claude.ai/code/session_012R82JNY8GLGFhHD1kAAKr5"
```

---

### Task 3: `omp_events` — session location and cwd encoding

**Files:**
- Create: `src/activity/omp_events.rs`
- Modify: `src/activity/mod.rs`
- Test: `src/activity/omp_events.rs` (co-located `mod tests`)

**Interfaces:**
- Consumes: `sessionx::activity::pi_events::tail_session`; `sessionx::activity::events::TailUpdate`.
- Produces:
  - `pub fn encode_cwd(cwd: &Path, home: &Path, tmp: &Path) -> String`
  - `pub fn locate_session_file(worktree: &Path) -> Option<PathBuf>`
  - `pub use sessionx::activity::pi_events::tail_session;`
  - reachable as `crate::activity::omp_events::*`

- [ ] **Step 1: Write the failing tests**

Create `src/activity/omp_events.rs` containing only the test module for now (the functions come in Step 3):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Home scope: the name is `-` followed by the home-relative path with `/`
    /// collapsed to `-`. Verified against omp v17.4.0 on a real run, which put
    /// this worktree's sessions in
    /// `-.local-state-wsx-worktrees-workspacex-grand-verbena`.
    #[test]
    fn home_scope_strips_home_and_collapses_separators() {
        assert_eq!(
            encode_cwd(
                Path::new("/home/eben/.local/state/wsx/worktrees/repo/slug"),
                Path::new("/home/eben"),
                Path::new("/tmp"),
            ),
            "-.local-state-wsx-worktrees-repo-slug"
        );
    }

    /// A cwd of exactly $HOME has an empty relative path, which omp encodes as
    /// the bare prefix.
    #[test]
    fn home_itself_encodes_to_a_bare_dash() {
        assert_eq!(
            encode_cwd(Path::new("/home/eben"), Path::new("/home/eben"), Path::new("/tmp")),
            "-"
        );
    }

    /// Tmp scope uses the `-tmp` prefix, which does NOT end in `-`, so omp
    /// inserts a separator before the relative part. Verified on a real run:
    /// `/tmp/ompprobe/deep/dir` → `-tmp-ompprobe-deep-dir`.
    #[test]
    fn tmp_scope_prefixes_with_tmp_and_inserts_a_separator() {
        assert_eq!(
            encode_cwd(Path::new("/tmp/ompprobe/deep/dir"), Path::new("/home/eben"), Path::new("/tmp")),
            "-tmp-ompprobe-deep-dir"
        );
    }

    /// The tmp root itself has an empty relative path, so it is the bare prefix
    /// with no trailing separator. Verified on a real run: `/tmp` → `-tmp`.
    #[test]
    fn tmp_root_itself_encodes_to_bare_tmp() {
        assert_eq!(
            encode_cwd(Path::new("/tmp"), Path::new("/home/eben"), Path::new("/tmp")),
            "-tmp"
        );
    }

    /// Anything outside home and tmp falls back to omp's legacy absolute form.
    #[test]
    fn absolute_scope_uses_the_legacy_double_dash_form() {
        assert_eq!(
            encode_cwd(Path::new("/srv/code/x"), Path::new("/home/eben"), Path::new("/tmp")),
            "--srv-code-x--"
        );
    }

    /// omp classifies home before tmp, so a home that lives under the tmp root
    /// (containers, some CI images) still takes the home branch. Getting this
    /// backwards would send every session lookup to the wrong directory on
    /// exactly those machines.
    #[test]
    fn home_wins_when_home_is_itself_under_the_tmp_root() {
        assert_eq!(
            encode_cwd(Path::new("/tmp/home/ci/proj"), Path::new("/tmp/home/ci"), Path::new("/tmp")),
            "-proj"
        );
    }

    /// The newest .jsonl in the encoded directory wins, and non-jsonl files are
    /// ignored.
    ///
    /// Both temp dirs live under the system temp root, so `work` takes
    /// `encode_cwd`'s **tmp** branch here (it is not under the fake `$HOME`).
    /// That is why the expected directory name is computed through `encode_cwd`
    /// rather than hardcoded — it stays correct whichever branch applies.
    #[test]
    fn locate_picks_the_newest_jsonl_in_the_encoded_dir() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let abs = std::fs::canonicalize(work.path()).unwrap();
        let canon_home = std::fs::canonicalize(home.path()).unwrap();
        let tmp = std::env::temp_dir();
        let tmp = std::fs::canonicalize(&tmp).unwrap_or(tmp);
        let encoded = encode_cwd(&abs, &canon_home, &tmp);
        let dir = home.path().join(".omp/agent/sessions").join(&encoded);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), "ignore me").unwrap();
        // mtime ordering by write order, with a gap wide enough to survive a
        // coarse filesystem timestamp granularity. `filetime` is not a
        // dependency of this crate, so this avoids adding one for two lines.
        std::fs::write(dir.join("1770000000_old.jsonl"), "{}").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dir.join("1770000001_new.jsonl"), "{}").unwrap();

        let mut env = crate::test_support::EnvGuard::new();
        env.set("HOME", home.path());
        assert_eq!(
            locate_session_file(work.path()).unwrap().file_name().unwrap(),
            "1770000001_new.jsonl"
        );
    }

    #[test]
    fn locate_returns_none_when_the_worktree_has_no_session_dir() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let mut env = crate::test_support::EnvGuard::new();
        env.set("HOME", home.path());
        assert!(locate_session_file(work.path()).is_none());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

First register the module. In `src/activity/mod.rs`:

```rust
pub mod hermes_events;
pub mod omp_events;
pub mod proc;
```

Run: `cargo test --lib omp_events`
Expected: FAIL to compile — `cannot find function 'encode_cwd' in this scope`.

- [ ] **Step 3: Write the implementation**

Put this above the test module in `src/activity/omp_events.rs`:

```rust
//! Locate oh-my-pi session JSONL files for activity tailing.
//!
//! omp stores sessions at `~/.omp/agent/sessions/<encoded-cwd>/<ts>_<uuid>.jsonl`.
//! The **schema** of those files is pi's (v3): the same `{"type":"message", …,
//! "message":{…}}` envelope, the same `user`/`assistant`/`toolResult` roles, the
//! same `text`/`thinking`/`toolCall` content parts, the same `stopReason`
//! vocabulary, and the same lowercase tool names. Verified against a real omp
//! v17.4.0 capture. So this module reimplements only the *location* — the
//! parser is `sessionx`'s pi parser, re-exported below.
//!
//! If omp's schema ever diverges from pi's, the fixture test in
//! `omp_jsonl_parses_through_the_pi_parser` fails loudly, which is the signal to
//! fork the parser rather than keep sharing it.

use std::path::{Path, PathBuf};

/// Read new lines from an omp session file and parse them as pi-schema JSONL.
///
/// Deliberately the pi parser, not a copy — see the module docs.
pub use sessionx::activity::pi_events::tail_session;

/// Encode `cwd` the way omp names its session directory.
///
/// Mirrors `getDefaultSessionDirName` in omp's `src/session/session-paths.ts`,
/// which classifies the (canonicalized) cwd into one of three scopes, **in this
/// order**:
///
/// 1. **home** — `cwd` is `home` or under it. `-` + the home-relative path with
///    separators collapsed to `-`. `home` itself yields the bare `-`.
/// 2. **tmp** — `cwd` is `tmp` or under it. `-tmp`, then (when the relative part
///    is non-empty) `-` + the tmp-relative path collapsed the same way.
/// 3. **abs** — everything else. omp's legacy form: `--` + the absolute path
///    with the leading separator stripped and `/`, `\`, `:` collapsed to `-`,
///    + `--`.
///
/// Order matters: a home that lives under the tmp root (containers, some CI
/// images) must still take the home branch.
///
/// `home` and `tmp` are parameters rather than being read from the environment
/// so the classification is testable without touching the real machine.
pub fn encode_cwd(cwd: &Path, home: &Path, tmp: &Path) -> String {
    fn collapse(s: &str) -> String {
        s.replace(['/', '\\', ':'], "-")
    }

    if let Ok(rel) = cwd.strip_prefix(home) {
        let encoded = collapse(&rel.to_string_lossy());
        // The "-" prefix already ends in a separator, so nothing is inserted.
        return format!("-{encoded}");
    }
    if let Ok(rel) = cwd.strip_prefix(tmp) {
        let encoded = collapse(&rel.to_string_lossy());
        // The "-tmp" prefix does not end in a separator, so omp inserts one —
        // but only when there is a relative part to separate from.
        return if encoded.is_empty() {
            "-tmp".to_string()
        } else {
            format!("-tmp-{encoded}")
        };
    }
    let inner = collapse(cwd.to_string_lossy().trim_start_matches('/'));
    format!("--{inner}--")
}

/// omp's legacy absolute session-dir name, used before 17.x introduced the
/// home/tmp scopes.
///
/// omp migrates these lazily, on first access **by omp itself** — so a worktree
/// whose history predates the migration and that omp has not reopened since
/// still has its sessions filed here. Probing it costs one `is_dir` and closes a
/// "prior session exists but wsx can't see it" gap.
fn legacy_dir_name(cwd: &Path) -> String {
    let inner = cwd
        .to_string_lossy()
        .trim_start_matches('/')
        .replace(['/', '\\', ':'], "-");
    format!("--{inner}--")
}

/// Locate the newest session file for a worktree, or `None` when omp has none.
///
/// Canonicalizes first: omp resolves symlinks before classifying the cwd, so a
/// symlinked worktree would otherwise be looked up under the wrong name.
pub fn locate_session_file(worktree: &Path) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let home = std::fs::canonicalize(&home).unwrap_or(home);
    let tmp = std::env::temp_dir();
    let tmp = std::fs::canonicalize(&tmp).unwrap_or(tmp);
    let abs = std::fs::canonicalize(worktree).ok()?;
    let root = home.join(".omp/agent/sessions");

    let candidates = [encode_cwd(&abs, &home, &tmp), legacy_dir_name(&abs)];
    let session_dir = candidates
        .iter()
        .map(|name| root.join(name))
        .find(|d| d.is_dir())?;

    let mut newest: Option<(PathBuf, std::time::SystemTime)> = None;
    for entry in std::fs::read_dir(&session_dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(mtime) = meta.modified() else { continue };
        match &newest {
            None => newest = Some((path, mtime)),
            Some((_, prev)) if mtime > *prev => newest = Some((path, mtime)),
            _ => {}
        }
    }
    newest.map(|(p, _)| p)
}
```

Then update `src/activity/mod.rs`'s module doc to mention omp alongside hermes as a wsx-local module, and say why (location differs, schema does not).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib omp_events`
Expected: PASS, 8 tests.

- [ ] **Step 5: Verify the encoding against the real binary**

```bash
ls ~/.omp/agent/sessions/
```
Expected: at least one directory whose name matches what `encode_cwd` produces for that path — e.g. a `-`-prefixed home-relative name. If a directory name on this machine does not match any of the three scopes, stop and reconcile before continuing; the encoding is the load-bearing part of this module.

- [ ] **Step 6: Format, lint, commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test --lib
git add -A
git commit -m "feat(omp): locate oh-my-pi session files

omp writes pi-schema (v3) JSONL, so this module reimplements only the
location: the three-scope cwd encoding (home / tmp / absolute) omp uses
to name ~/.omp/agent/sessions subdirectories, plus a probe of the
pre-17.x legacy name that omp migrates only on its own first access.
tail_session is sessionx's pi parser, re-exported rather than copied.

Claude-Session: https://claude.ai/code/session_012R82JNY8GLGFhHD1kAAKr5"
```

---

### Task 4: Prior-session detection, the session-bleed snapshot, and activity tailing

Wires `omp_events` into the three places that consume a session file, and closes the recycled-slug session-bleed hazard for omp.

**Files:**
- Modify: `src/pty/session_detect.rs` (`SessionSnapshot` ~line 26; `write_worktree_sessions` ~line 128; `read_worktree_sessions` ~line 160; `has_prior_session_for` ~line 376; new `omp_session_dir` + `has_prior_omp_session`)
- Modify: `src/app/background.rs` (~lines 39 and 72 — replace the Task 1 placeholders)
- Create: `tests/fixtures/omp-session.jsonl`
- Test: `src/pty/session_detect.rs` (co-located `mod tests`), `src/activity/omp_events.rs` (co-located `mod tests`)

**Interfaces:**
- Consumes: `crate::activity::omp_events::{locate_session_file, tail_session, encode_cwd}` from Task 3; the existing private helpers `jsonl_names(&Path) -> HashSet<String>` and `read_worktree_sessions(&Path) -> Option<SessionSnapshot>`; the test helper `snapshot(worktree: &Path, lines: &[&str])`.
- Produces: `pub fn has_prior_omp_session(worktree: &Path) -> bool`; `SessionSnapshot::has_omp(&self, name: &str) -> bool`.

- [ ] **Step 1: Capture the parser fixture**

Generate a real omp session and copy its JSONL into the repo:

```bash
mkdir -p /tmp/omp-fixture && cd /tmp/omp-fixture
omp -p "Run the bash command 'echo hello from omp' and then tell me what it printed."
LATEST=$(ls -t ~/.omp/agent/sessions/-tmp-omp-fixture/*.jsonl | head -1)
cd -
mkdir -p tests/fixtures
cp "$LATEST" tests/fixtures/omp-session.jsonl
wc -l tests/fixtures/omp-session.jsonl
```

Then open the file and confirm it contains at least one `"role":"assistant"` message with a `toolCall` part and one `"role":"toolResult"` message. If the run did not produce a tool call, re-run with a prompt that forces one. If the session directory name differs from `-tmp-omp-fixture`, use whatever `ls ~/.omp/agent/sessions/` shows — and note the discrepancy, because it means Task 3's encoding needs revisiting.

Scrub the fixture before committing: `providerPayload.items[].encrypted_content` fields can be very large, and the file may embed absolute paths. Truncating those string values (or deleting the `providerPayload` key entirely) keeps the fixture readable; the pi parser does not read them. Verify the file still parses as one JSON object per line:

```bash
while read -r l; do echo "$l" | jq -e . >/dev/null || echo "BAD LINE"; done < tests/fixtures/omp-session.jsonl
```

- [ ] **Step 2: Write the failing tests**

Add to `src/activity/omp_events.rs`'s `mod tests`:

```rust
    /// The load-bearing bet of this module: omp's JSONL is pi's schema, so
    /// sessionx's pi parser reads it. This replays a REAL omp session capture
    /// (tests/fixtures/omp-session.jsonl, produced by omp v17.4.0) rather than
    /// a hand-written approximation, so a schema divergence fails here instead
    /// of silently blanking the dashboard.
    #[test]
    fn omp_jsonl_parses_through_the_pi_parser() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/omp-session.jsonl");
        let update = tail_session(&fixture, 0).expect("omp jsonl must parse");
        assert!(update.new_offset > 0, "parser consumed nothing");
        assert!(
            !update.events.is_empty(),
            "expected parsed events from a real session: {update:?}"
        );
        assert!(
            update.first_user_text.is_some(),
            "the user prompt must be recovered: {update:?}"
        );
        assert!(
            update.last_assistant_text.is_some(),
            "assistant text must be recovered: {update:?}"
        );
        assert!(
            !update.tool_use_starts.is_empty(),
            "omp's toolCall parts must be recognised as tool starts: {update:?}"
        );
    }
```

Add to `src/pty/session_detect.rs`'s `mod tests`:

```rust
    /// omp indexes sessions by path, so it inherits the recycled-slug hazard
    /// that `write_worktree_sessions` exists to close: archiving frees a slug,
    /// the next workspace for the same repo lands on a byte-identical path, and
    /// a `-c` spawn would resume a stranger's conversation.
    #[test]
    fn has_prior_omp_session_ignores_a_session_named_in_the_snapshot() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let dir = seed_omp_session(home.path(), work.path(), "1770000000_abc.jsonl");
        assert!(dir.is_dir());
        snapshot(work.path(), &["omp:1770000000_abc.jsonl"]);

        let mut env = EnvGuard::new();
        env.set("HOME", home.path());
        assert!(
            !has_prior_omp_session(work.path()),
            "omp inherits the same path-reuse hazard as claude and pi"
        );
    }

    #[test]
    fn has_prior_omp_session_finds_a_session_created_after_the_snapshot() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        seed_omp_session(home.path(), work.path(), "1770000000_old.jsonl");
        snapshot(work.path(), &["omp:1770000000_old.jsonl"]);
        seed_omp_session(home.path(), work.path(), "1770000001_new.jsonl");

        let mut env = EnvGuard::new();
        env.set("HOME", home.path());
        assert!(
            has_prior_omp_session(work.path()),
            "a session outside the snapshot is this occupant's own"
        );
    }

    /// `write_worktree_sessions` must record omp's files, or the two tests
    /// above are testing a gate that production never closes.
    #[test]
    fn write_worktree_sessions_records_omp_files() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(work.path().join(".git/info")).unwrap();
        seed_omp_session(home.path(), work.path(), "1770000000_abc.jsonl");

        let mut env = EnvGuard::new();
        env.set("HOME", home.path());
        write_worktree_sessions(work.path()).unwrap();
        let body =
            std::fs::read_to_string(work.path().join(".git/info/wsx-worktree-sessions")).unwrap();
        assert!(
            body.contains("omp:1770000000_abc.jsonl"),
            "snapshot must name omp sessions: {body:?}"
        );
    }
```

And the seeding helper, next to the existing `seed_codex_rollout`:

```rust
    /// Write an omp session JSONL under `$HOME/.omp/agent/sessions` for
    /// `worktree`, using the same encoding `omp_session_dir` resolves. Returns
    /// the session directory.
    fn seed_omp_session(
        home: &std::path::Path,
        worktree: &std::path::Path,
        name: &str,
    ) -> std::path::PathBuf {
        let abs = std::fs::canonicalize(worktree).unwrap();
        let canon_home = std::fs::canonicalize(home).unwrap();
        let tmp = std::env::temp_dir();
        let tmp = std::fs::canonicalize(&tmp).unwrap_or(tmp);
        let encoded = crate::activity::omp_events::encode_cwd(&abs, &canon_home, &tmp);
        let dir = home.join(".omp/agent/sessions").join(encoded);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), "{}\n").unwrap();
        dir
    }
```

Note: `tempfile::TempDir` allocates under the system temp dir, so `worktree` here takes `encode_cwd`'s **tmp** branch while `home` is a sibling temp dir. That is exactly why the helper calls `encode_cwd` rather than hardcoding a name — it stays correct whichever branch applies.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --lib omp`
Expected: FAIL — `cannot find function 'has_prior_omp_session'`, and `omp_jsonl_parses_through_the_pi_parser` fails on a missing fixture if Step 1 was skipped.

- [ ] **Step 4: Write the implementation**

`src/pty/session_detect.rs` — add the field to `SessionSnapshot`:

```rust
    /// Newest Codex rollout matching this cwd, as an absolute path.
    codex: Option<String>,
    /// omp session JSONL file names.
    omp: std::collections::HashSet<String>,
```

and the accessor beside `has_pi`:

```rust
    /// True if `name` is one of the omp sessions that predate this worktree.
    fn has_omp(&self, name: &str) -> bool {
        self.omp.contains(name)
    }
```

Add the session dir helper beside `pi_session_dir`:

```rust
/// omp's session directory for `worktree`, or None if the path can't be
/// canonicalized. Delegates the three-scope cwd encoding to
/// [`crate::activity::omp_events`] so the snapshot gate and the activity tail
/// can never disagree about which directory a worktree's sessions live in.
fn omp_session_dir(worktree: &Path) -> Option<std::path::PathBuf> {
    let abs = std::fs::canonicalize(worktree).ok()?;
    let home = dirs::home_dir()?;
    let home = std::fs::canonicalize(&home).unwrap_or(home);
    let tmp = std::env::temp_dir();
    let tmp = std::fs::canonicalize(&tmp).unwrap_or(tmp);
    let encoded = crate::activity::omp_events::encode_cwd(&abs, &home, &tmp);
    Some(dirs::home_dir()?.join(".omp/agent/sessions").join(encoded))
}

/// True if omp has a persisted session JSONL for this worktree that belongs to
/// the *current* occupant of the path — see [`write_worktree_sessions`].
pub fn has_prior_omp_session(worktree: &Path) -> bool {
    let Some(dir) = omp_session_dir(worktree) else {
        return false;
    };
    let snapshot = read_worktree_sessions(worktree);
    jsonl_names(&dir)
        .iter()
        .any(|name| snapshot.as_ref().is_none_or(|s| !s.has_omp(name)))
}
```

In `write_worktree_sessions`, after the pi block:

```rust
    if let Some(dir) = omp_session_dir(worktree) {
        for name in jsonl_names(&dir) {
            out.push_str(&format!("omp:{name}\n"));
        }
    }
```

In `read_worktree_sessions`, add the arm — **before** the `pi:` arm is unnecessary since the prefixes are distinct, but keep the chain's existing order and append:

```rust
        } else if let Some(name) = line.strip_prefix("omp:") {
            snap.omp.insert(name.to_string());
```

In `has_prior_session_for`, replace the Task 1 placeholder:

```rust
        AgentKind::Omp => has_prior_omp_session(worktree),
```

Also extend the doc comment on `write_worktree_sessions`: it currently says "Claude, Pi and Codex all index sessions by worktree PATH" — make it "Claude, Pi, Codex and omp".

`src/app/background.rs` — replace both Task 1 placeholders:

```rust
        crate::pty::session::AgentKind::Omp => {
            crate::activity::omp_events::locate_session_file(&worktree_path)
        }
```

```rust
        crate::pty::session::AgentKind::Omp => {
            crate::activity::omp_events::tail_session(&file, tail_from).map_err(Into::into)
        }
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib omp`
Expected: PASS. Then run the whole suite: `cargo test` — the snapshot round-trip touches shared helpers, so a regression would show up in the Claude/Pi/Codex snapshot tests.

- [ ] **Step 6: Format, lint, commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
git add -A
git commit -m "feat(omp): prior-session detection and activity tailing

Wires omp_events into the three consumers of a session file: the
dashboard's prior-session indicator, the -c/fresh spawn decision, and the
background event tail that fills RECENT CHAT and SESSION SUMMARY.

omp indexes sessions by path, so it also joins the worktree-sessions
snapshot that stops a recycled slug from resuming the previous
occupant's conversation. Includes a fixture captured from a real omp
v17.4.0 session, which is the tripwire for omp's JSONL drifting from the
pi schema this module reuses the parser for.

Claude-Session: https://claude.ai/code/session_012R82JNY8GLGFhHD1kAAKr5"
```

---

### Task 5: A real composer-readiness signal from a captured cold boot

The one part of this work that must **not** be written from omp's source. Every other agent's predicate was read off a real PTY capture, and the module documents Hermes's unconditional `true` as an acknowledged hole rather than a decision. omp is installed, so it gets a real signal.

**Files:**
- Create: `tests/fixtures/agent-boot/omp-preboot.bin`, `tests/fixtures/agent-boot/omp-composer.bin`
- Modify: `src/pty/session.rs` (`ready_for_input` ~line 431; the doc comment above it; `submit_writes` ~line 558 if the capture contradicts the hypothesis)
- Test: `src/pty/session.rs` (co-located `mod tests`)

**Interfaces:**
- Consumes: `AgentKind::Omp`; `examples/capture_agent_boot.rs` (already in the repo).
- Produces: `fn omp_ready(screen: &vt100::Screen) -> bool`.

- [ ] **Step 1: Capture a real cold boot**

```bash
mkdir -p /tmp/omp-boot && cd /tmp/omp-boot && git init -q . && cd -
cargo run --example capture_agent_boot -- /tmp/omp-boot-capture 25 /tmp/omp-boot omp
```

This writes `/tmp/omp-boot-capture.bin` and `.timing`, then prints the screen at **every** moment output went quiet for ≥400ms — i.e. every moment a wsx message injection could land. Read that output carefully. For each printed screen, decide: does a focused composer exist here that would keep typed text?

Record from the output:
- the `t=` of the first screen that genuinely has a composer,
- what distinguishes it from every earlier screen: `alt=` (alternate screen), `cursor_hidden=`, and any glyph or chrome unique to the composer row,
- whether any *later* screen loses the composer (a modal, an update prompt, an auth dialog) — omp shows a startup splash and can prompt for setup, so this is a live risk, the same one that made Codex's trust dialog a bug.

- [ ] **Step 2: Cut the two fixtures**

Using the `.timing` file (`<ms-since-spawn> <byte-count>` per read), sum the byte counts to find the offset that splits pre-composer from composer:

```bash
# Bytes emitted before the composer appears — replace 1234 with the byte offset
# you computed from the timing file for the read just before the composer paints.
head -c 1234 /tmp/omp-boot-capture.bin > tests/fixtures/agent-boot/omp-preboot.bin
tail -c +1235 /tmp/omp-boot-capture.bin > /tmp/omp-rest.bin
# Then cut omp-composer.bin from the front of /tmp/omp-rest.bin, ending just
# past the read that paints the composer.
head -c 5678 /tmp/omp-rest.bin > tests/fixtures/agent-boot/omp-composer.bin
```

Verify the cut by replaying it — the assertion in Step 3 is the real check, but confirm the sizes are sane (`ls -l tests/fixtures/agent-boot/`) and comparable to the existing pi/codex fixtures.

- [ ] **Step 3: Write the failing test**

In `src/pty/session.rs`'s `mod tests`, beside `pi_is_not_ready_mid_boot_and_is_ready_once_its_composer_paints`:

```rust
    #[test]
    fn omp_is_not_ready_mid_boot_and_is_ready_once_its_composer_paints() {
        // Real capture off a PTY. Fill in the measured timings from the
        // capture_agent_boot run in the commit message and here.
        let mut p = Parser::new(24, 80, 1000);
        p.process(boot_fixture!("omp-preboot.bin"));
        assert!(
            !ready_for_input(AgentKind::Omp, p.screen()),
            "omp before its composer exists must not be considered ready"
        );
        p.process(boot_fixture!("omp-composer.bin"));
        assert!(
            ready_for_input(AgentKind::Omp, p.screen()),
            "omp is ready once its composer is drawn"
        );
    }
```

Replace the placeholder comment with the actual measured timings from Step 1 (e.g. "omp prints its startup splash for the first ~Nms, with an Mms quiet window in there"), matching how the Claude/Codex/Pi tests document theirs.

- [ ] **Step 4: Run the test to verify it fails**

Run: `cargo test --lib omp_is_not_ready`
Expected: FAIL on the first assertion — Task 1 left `AgentKind::Omp => true`, so a pre-composer screen is wrongly reported ready.

- [ ] **Step 5: Write the predicate**

Replace the Task 1 placeholder in `ready_for_input`:

```rust
        AgentKind::Omp => omp_ready(screen),
```

and add `omp_ready` beside `pi_ready`. **Write the body from what the capture showed**, not from this plan. The three shapes already in the file are the vocabulary to pick from:

- alternate-screen (Claude): `screen.alternate_screen()`
- composer glyph + visible cursor (Codex): scan rows for a marker, and require `!screen.hide_cursor()` so a modal that draws the same marker but hides the cursor does not count
- composer chrome (Pi): count full-width rule rows in the bottom half

Document the choice the way the neighbours are documented: what was measured, at what `t=`, and what the signal separates. If the capture showed a modal that steals the composer (an update prompt, a setup wizard, an auth dialog), the predicate **must** exclude it — that is the Codex trust-dialog bug, and it fails silently.

Update the doc comment above `ready_for_input` to add an omp bullet alongside the Claude/Codex/Pi/Hermes ones.

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test --lib omp_is_not_ready`
Expected: PASS.

- [ ] **Step 7: Verify submit behaviour against a live session**

Task 1 put omp in the plain-text-plus-CR arm of `submit_writes`, on the hypothesis that omp must **not** get Codex's bracketed-paste wrapper: omp's `CustomEditor` runs its own `BracketedPasteHandler` and collapses pastes into `[Paste #N]` markers, which would replace a wsx message body with a placeholder.

Verify it end to end rather than trusting the hypothesis:

```bash
cargo build
# In one terminal: create a scratch workspace driven by omp.
./target/debug/wsx workspace create <a-test-repo> --name omp-submit-check --agent omp
# In another: send it a multi-line message.
./target/debug/wsx agent send --workspace <repo>/omp-submit-check primary "$(printf 'line one\nline two\nsay OK')"
```

Attach to the workspace and confirm: the full multi-line text arrived in the composer (not `[Paste #1]`), and it was **submitted** rather than left as a draft. If the message sits unsubmitted, omp needs the bracketed-paste treatment after all — move `AgentKind::Omp` into the Codex arm of `submit_writes` and add a test mirroring `submit_writes_wraps_codex_in_a_bracketed_paste`. If it arrives as `[Paste #N]`, keep plain text and record the finding in the comment. Either way, replace the hypothesis in `submit_writes`'s doc comment with what you observed.

Clean up: `./target/debug/wsx workspace archive <repo> omp-submit-check`.

- [ ] **Step 8: Format, lint, commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
git add -A
git commit -m "feat(omp): give omp a real composer-readiness signal

ready_for_input gates message injection on 'the agent's composer exists'.
Task 1 left omp at an honest placeholder; this replaces it with a
predicate read off a real cold boot captured through capture_agent_boot,
the same way codex and pi got theirs — plus the two fixture cuts so the
signal is regression-tested rather than asserted.

Also records the observed submit behaviour: omp's editor runs its own
bracketed-paste handler, so the delivery path's paste treatment is
chosen from what a live session actually did with a multi-line message.

Claude-Session: https://claude.ai/code/session_012R82JNY8GLGFhHD1kAAKr5"
```

---

### Task 6: Doctrine, theme color, Tab cycle, and CLI validation

**Files:**
- Modify: `src/agent/doctrine.rs` (`process_doctrine` ~line 101)
- Modify: `src/ui/theme.rs` (color consts ~lines 8-11; `agent_style` ~line 362)
- Modify: `src/app/input.rs` (Tab cycle ~lines 1402-1405)
- Modify: `src/cli.rs` (`--agent` validation ~line 997; help copy ~lines 61, 958, 985)
- Modify: `src/agent/status/mod.rs` (extend the noop test)
- Test: co-located `mod tests` in each of the above

**Interfaces:**
- Consumes: `AgentKind::Omp`, `AgentKind::ALL` (now 5 entries).
- Produces: `const AGENT_OMP: Color`.

- [ ] **Step 1: Write the failing tests**

`src/agent/doctrine.rs`:

```rust
    /// omp reads ~/.claude/skills natively (its Claude discovery provider loads
    /// `<user .claude>/skills/*/SKILL.md`), and a live omp session was observed
    /// resolving skill://using-superpowers on startup. So it belongs with
    /// Claude and Pi, not with Codex.
    #[test]
    fn omp_gets_the_superpowers_clause() {
        let d = process_doctrine(AgentKind::Omp).to_lowercase();
        assert!(d.contains("superpowers"), "omp must get superpowers: {d}");
        assert!(d.contains("wsx skill"), "{d}");
        assert!(d.contains("commit"), "{d}");
    }
```

`src/ui/theme.rs`, extend `agent_style_maps_each_kind_to_fixed_rgb`:

```rust
        assert_eq!(
            t.agent_style(AgentKind::Omp).fg,
            Some(Color::Rgb(0x5f, 0xc9, 0x8e))
        );
```

and add:

```rust
    /// Agent colors are identity, so two kinds sharing one would make peer bars
    /// and chips ambiguous on the dashboard.
    #[test]
    fn every_agent_kind_has_a_distinct_color() {
        use crate::pty::session::AgentKind;
        let t = Theme::wsx();
        let mut seen = std::collections::HashSet::new();
        for k in AgentKind::ALL {
            assert!(
                seen.insert(format!("{:?}", t.agent_style(k).fg)),
                "duplicate color for {k:?}"
            );
        }
    }
```

`src/cli.rs`:

```rust
    #[test]
    fn workspace_create_accepts_every_agent_kind() {
        use crate::pty::session::AgentKind;
        for k in AgentKind::ALL {
            let name = k.display_name();
            assert!(
                parse(&["workspace", "create", "myrepo", "--agent", name]).is_ok(),
                "--agent {name} must be accepted"
            );
        }
        assert!(parse(&["workspace", "create", "myrepo", "--agent", "bogus"]).is_err());
    }
```

`src/agent/status/mod.rs`, extend `other_agents_resolve_to_noop`'s array:

```rust
        for agent in [AgentKind::Pi, AgentKind::Hermes, AgentKind::Omp] {
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib omp_gets_the_superpowers_clause every_agent_kind_has_a_distinct_color workspace_create_accepts_every_agent_kind`
Expected: FAIL — omp is missing from `include_superpowers`, shares Pi's color from Task 1, and the `--agent` validation chain rejects `omp`.

- [ ] **Step 3: Write the implementation**

`src/agent/doctrine.rs`:

```rust
    // omp, like Claude and Pi, loads skills from ~/.claude/skills — its Claude
    // discovery provider reads `<user .claude>/skills/*/SKILL.md` — so the
    // superpowers clause points at something that is actually there. Codex and
    // Hermes do not, which is why they are excluded.
    let include_superpowers = matches!(
        agent,
        AgentKind::Claude | AgentKind::Pi | AgentKind::Omp
    );
```

`src/ui/theme.rs`:

```rust
const AGENT_CODEX: Color = Color::Rgb(0x5b, 0x9d, 0xe0); // blue
const AGENT_OMP: Color = Color::Rgb(0x5f, 0xc9, 0x8e); // green
```

```rust
            AgentKind::Omp => AGENT_OMP,
```

Green is chosen because it is the one hue not already spoken for (Claude orange, Pi purple, Hermes yellow, Codex blue) and stays distinguishable from Hermes's yellow, which a warmer green would not.

`src/app/input.rs`, Tab cycle:

```rust
                    crate::pty::session::AgentKind::Claude => crate::pty::session::AgentKind::Pi,
                    crate::pty::session::AgentKind::Pi => crate::pty::session::AgentKind::Hermes,
                    crate::pty::session::AgentKind::Hermes => crate::pty::session::AgentKind::Codex,
                    crate::pty::session::AgentKind::Codex => crate::pty::session::AgentKind::Omp,
                    crate::pty::session::AgentKind::Omp => crate::pty::session::AgentKind::Claude,
```

`src/cli.rs`, replace the hardcoded chain (~line 997) with the `ALL`-driven form the `agent add` path already uses two hundred lines away — adding a fifth agent is precisely the moment a hand-maintained chain silently rejects a valid kind:

```rust
            // Validate against the canonical agent set so this can't drift from
            // `AgentKind` as kinds are added or renamed — same reason
            // `agent add` validates this way.
            if let Some(ref a) = agent
                && !crate::pty::session::AgentKind::ALL
                    .iter()
                    .any(|k| k.display_name() == a)
            {
                let valid = crate::pty::session::AgentKind::ALL
                    .iter()
                    .map(|k| k.display_name())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(Error::Usage {
                    group: None,
                    msg: format!("--agent must be one of [{valid}], got '{a}'"),
                });
            }
```

If the existing `if let Some(ref a) = agent && …` let-chain syntax does not compile on this toolchain, restructure as a nested `if let` — read the surrounding code and match its style.

Then update the three help strings so they list omp:
- ~line 61: `blurb: "Attach an agent (claude|pi|hermes|codex|omp)"`
- ~line 958: `"workspace create <repo> [--name <slug>] [--yolo] [--shared] [--agent claude|pi|hermes|codex|omp] [--prompt <text>]"`
- ~line 985: `"--agent needs value (claude, pi, hermes, codex, or omp)"`

Search `src/cli.rs` for any other literal listing the four agent names and update it too — the tests in `src/cli.rs` around line 2658 assert on error copy, so run the full CLI test module.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib`
Expected: PASS. If a CLI test asserts the old `--agent must be 'claude', 'pi', 'hermes', or 'codex'` wording, update the assertion to the new message.

- [ ] **Step 5: Verify the picker end to end**

```bash
cargo build
./target/debug/wsx workspace create <a-test-repo> --name omp-picker-check --agent omp
./target/debug/wsx workspace list <a-test-repo> | grep omp-picker-check
```
Expected: the workspace exists and is recorded against the `omp` agent. Then launch the TUI, press `a` on that row, and confirm the agent picker lists five entries with omp in green. Clean up with `wsx workspace archive`.

- [ ] **Step 6: Format, lint, commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
git add -A
git commit -m "feat(omp): doctrine, color, tab cycle, and CLI validation

omp joins Claude and Pi in the superpowers doctrine clause: it loads
~/.claude/skills natively, so the clause points at skills that are
actually present. Gets green as its identity color (the one hue not
already taken), a Tab-cycle slot after codex, and a distinct-colors test
so a future agent can't silently collide.

Also converts the --agent validation from a hand-maintained
a != \"pi\" && … chain to the AgentKind::ALL form `agent add` already uses.
Adding a fifth agent is exactly the moment that chain would have
rejected a valid kind.

Claude-Session: https://claude.ai/code/session_012R82JNY8GLGFhHD1kAAKr5"
```

---

### Task 7: Documentation

**Files:**
- Modify: `docs/book/src/configuration/coding-agents.md`
- Modify: `docs/book/src/reference/environment-variables.md`
- Modify: `docs/book/src/configuration/multi-agent-workspaces.md`
- Modify: `docs/book/src/cli-reference/workspace-management.md`
- Modify: `docs/book/src/configuration/global-settings.md`
- Modify: `docs/book/src/integrations/agent-skill.md`
- Modify: `README.md`
- Modify: `skills/wsx/SKILL.md`

**Interfaces:**
- Consumes: everything from Tasks 1-6.
- Produces: no code.

- [ ] **Step 1: Read what each file currently says about the four agents**

```bash
grep -rn "hermes" docs/book/src README.md skills/wsx/SKILL.md
```

Every hit is a place that enumerates agents and therefore probably needs omp added. Read the surrounding prose in each before editing — some list agents as prose, some as tables, some as command examples.

- [ ] **Step 2: Add the agent table row**

In `docs/book/src/configuration/coding-agents.md`, add to the table (matching the existing column shape):

```markdown
| `omp`              | `--agent omp`    | `omp` binary (override via `WSX_OMP_BIN`)                                 | `~/.omp/agent/config.yml`                 |
```

- [ ] **Step 3: Write the omp integration section**

Add after the Codex section in the same file:

````markdown
### Oh My Pi integration

`omp` is [oh-my-pi](https://github.com/can1357/oh-my-pi)
(`@oh-my-pi/pi-coding-agent`). It is **not** the same harness as `pi`, which is
`@earendil-works/pi-coding-agent`; the two share ancestry but are separately
maintained and can both be installed at once.

**Spawn**: fresh workspaces launch bare `omp`. Non-yolo sessions inherit
whatever `tools.approvalMode` you configured; `--yolo` workspaces add
`--approval-mode yolo`.

**Continue**: `omp -c`. omp resolves `--continue` against the session directory
for the current cwd, so this resumes the worktree's own most-recent session
without wsx needing a marker file.

**Instructions**: doctrine, the auto-rename directive, and a workspace's custom
instructions are composed into a single `--append-system-prompt`. Related-repo
paths ride on `--add-dir`. omp is the only harness besides Claude that supports
both flags, so nothing is written into the worktree — no `AGENTS.md` block
(unlike Hermes) and no config overrides (unlike Codex).

**Skills and slash commands work with no setup.** omp's Claude discovery
provider loads `~/.claude/skills/*/SKILL.md` and `~/.claude/commands/*.md`
natively, so the skills installed by `wsx setup install-skill` and your pinned
command chips both reach omp unchanged. There is deliberately no separate omp
skills target.

**Activity**: the dashboard tails the worktree's newest session JSONL under
`~/.omp/agent/sessions/<encoded-cwd>/`. omp writes the same JSONL schema pi
does, so wsx reuses the pi parser; RECENT CHAT, SESSION SUMMARY, tool-use counts
and last-message columns are populated the same way they are for pi.

**Status reporting**: omp's hooks are pre/post *tool* hooks only — it exposes no
turn-lifecycle event (no stop, no prompt-submitted, no permission prompt) — so
there is no deterministic status wiring, the same position pi and hermes are in.
Status still updates from the agent itself calling `wsx status set`, and from the
session-JSONL heuristic. Claude and Codex are the only harnesses with automatic
harness-level status.

**Environment overrides**: configure omp via `~/.omp/agent/config.yml`, or set
`WSX_OMP_MODEL` to override the model per-workspace:

```bash
WSX_OMP_MODEL=anthropic/claude-opus-5 wsx workspace create backend --agent omp
```

There is no `WSX_OMP_PROVIDER`: omp treats `--provider` as legacy and accepts
`provider/id` in `--model`, so `WSX_OMP_MODEL` covers both.
````

- [ ] **Step 4: Update the remaining files**

- `docs/book/src/reference/environment-variables.md` — add `WSX_OMP_BIN` and `WSX_OMP_MODEL` rows, matching the format of the `WSX_HERMES_*` / `WSX_CODEX_*` entries. Do **not** add a provider var.
- `docs/book/src/configuration/global-settings.md` — the `wsx config set coding_agent hermes` example region lists valid values; add `omp`.
- `docs/book/src/configuration/multi-agent-workspaces.md` — add omp wherever the four kinds are enumerated.
- `docs/book/src/cli-reference/workspace-management.md` — update the `--agent` value list.
- `docs/book/src/integrations/agent-skill.md` — this documents which agents get skills installed where. State explicitly that omp needs no target because it reads `~/.claude/skills`, the same note Pi already has.
- `README.md` — add omp wherever agents are listed.
- `skills/wsx/SKILL.md` — this is the skill the agents themselves read, so its `--agent` value list must include omp or agents will not know they can create omp workspaces.

- [ ] **Step 5: Verify no stale enumeration remains**

```bash
grep -rn "claude|pi|hermes|codex\|'claude', 'pi'\|claude, pi, hermes" docs/ README.md skills/ src/ | grep -v omp
```
Expected: no hits that are meant to be exhaustive lists of agent kinds. Investigate each remaining hit; a historical spec or plan under `docs/superpowers/` legitimately keeps its original wording and should **not** be edited.

- [ ] **Step 6: Build the book and commit**

```bash
# If mdbook is available:
mdbook build docs/book 2>&1 | tail -5
cargo test
git add -A
git commit -m "docs(omp): document oh-my-pi support

Adds the agent table row, the integration section, and the WSX_OMP_BIN /
WSX_OMP_MODEL env vars. Calls out the two things that surprise people:
omp is not the same harness as pi despite the shared ancestry, and its
skills and slash commands need no setup because it reads ~/.claude
natively. Also records the status-reporting gap as a property of omp's
hook surface rather than an unfinished integration.

Claude-Session: https://claude.ai/code/session_012R82JNY8GLGFhHD1kAAKr5"
```

---

## Final verification

- [ ] `cargo fmt --check` — clean
- [ ] `cargo clippy --all-targets -- -D warnings` — clean
- [ ] `cargo test` — green (note: `click_chip_auto_spawns_session_when_missing` is a known flaky PTY-timing test; re-run before treating it as a real failure)
- [ ] `git log --oneline main..HEAD` shows seven commits, each independently reviewable
- [ ] Manual smoke test, end to end:
  ```bash
  wsx workspace create <repo> --name omp-smoke --agent omp --yolo
  # attach, confirm: omp launches, the rename prompt fires on the first
  # message, a pinned command chip dispatches a real slash command, the
  # dashboard row is green and shows activity, and the wsx skill is visible
  # to the agent (ask it to run `wsx recap set --goal "test"`).
  wsx workspace archive <repo> omp-smoke
  ```
- [ ] Detach and re-attach the workspace before archiving, and confirm the session **resumed** rather than starting fresh — this is the `-c` cwd-scoping assumption, and it is the one behaviour no unit test can prove.

## Self-review notes (for the executor)

- **Task 1 leaves three deliberate placeholders** (`ready_for_input`, `has_prior_session_for`, `background.rs`) so the crate compiles at every commit. Tasks 4 and 5 remove them. If you stop after Task 1, omp spawns but has no activity, no prior-session detection, and an unsafe readiness signal — do not ship that state.
- **Task 5 is the one task you cannot do from documentation.** If `capture_agent_boot` fails to produce a usable capture (omp exits immediately, requires auth, needs a TTY feature the harness lacks), stop and report rather than writing a guessed predicate. A wrong predicate here loses messages silently, which is the failure mode the whole mechanism exists to prevent.
- **The `omp`/`pi` distinction is the highest-risk confusion in this plan.** Before every commit, re-read your diff for a place you wrote `pi` meaning omp or vice versa. `WSX_PI_BIN` and `WSX_OMP_BIN` must never be crossed.
- **Task 3's `encode_cwd` is load-bearing for Tasks 4, 5 and the dashboard.** If Step 5's real-directory check disagrees with the implementation, fix the encoding before proceeding — everything downstream inherits the error, and the symptom (a blank detail bar) looks like a parser problem rather than a path problem.
