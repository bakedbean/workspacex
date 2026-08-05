# Codex doctrine injection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver wsx's doctrine, rename hint, and custom instructions to Codex via `codex -c developer_instructions=...` instead of rewriting a `wsx-managed` block into the worktree's `AGENTS.md`, so Codex spawns stop mutating the user's worktree.

**Architecture:** Codex instruction delivery moves from *worktree state* to *spawn arguments*. `build_codex_command` gains two `-c` config overrides on `Fresh` spawns — `developer_instructions` (the composed prompt) and `project_doc_fallback_filenames=["CLAUDE.md"]` (replacing the one-shot `CLAUDE.md`→`AGENTS.md` copy). `prepare_codex_workspace` then sheds all of its file writes. `Continue` passes neither flag because `codex resume --last` ignores `-c` overrides for both keys.

**Tech Stack:** Rust 2024, `portable_pty::CommandBuilder`, `cargo test`, `tempfile`, in-repo `test_support::EnvGuard`.

Spec: [`docs/superpowers/specs/2026-08-05-codex-doctrine-injection-design.md`](../specs/2026-08-05-codex-doctrine-injection-design.md)

## Global Constraints

- Target Codex CLI **`0.146.0`**. Both config keys were verified against that binary; do not assume older versions accept them.
- CI runs `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-targets --all-features`. **Warnings fail the build.**
- **Do not modify anything Hermes uses.** `write_agents_md_section`, `strip_wsx_block`, `ensure_git_exclude`, `read_claude_md`, `HERMES_BLOCK_BEGIN`/`END`, `CLAUDE_PROVENANCE_COMMENT`, and every test under the `hermes_*` test modules stay exactly as they are. They become dead code only when Hermes is removed, which is a separate change.
- `compose_injected_prompt` (`src/pty/command.rs:435`) is **reused unchanged**. It already joins doctrine, rename hint, and custom instructions with blank lines and returns `None` when all three are absent.
- The exact fallback arg string is `project_doc_fallback_filenames=["CLAUDE.md"]` — a TOML array, no spaces.
- Existing Codex tests (`codex_fresh_is_bare_codex_with_no_approval_flags`, `codex_fresh_yolo_bypasses_approvals`, `codex_continue_uses_resume_last`, `codex_model_env_adds_dash_m`, `codex_fresh_injects_notify_status_wiring`) must keep passing untouched.

## File Structure

| File | Change | Responsibility after |
|---|---|---|
| `src/pty/command.rs` | Modify (`:473-538`, tests at `:1653-1771`) | Owns the whole Codex instruction channel: composes the prompt and encodes it into argv |
| `src/pty/workspace_prep.rs` | Modify (`:1-8`, `:190-203`, tests `:589-639`) | Loses its Codex responsibility entirely; keeps the Hermes AGENTS.md mechanism |
| `docs/book/src/configuration/coding-agents.md` | Modify (`:46-70`) | User-facing description of the Codex integration |

Three tasks, split where a reviewer could reject one and accept its neighbour: the escaper is a pure function with its own correctness argument; the argv wiring is the new behaviour; removing the file writes is the deletion that the wiring makes safe. Docs fold into Task 3, because that is when the old behaviour actually disappears.

---

### Task 1: `toml_basic_string` escaper

**Why this exists:** `codex -c key=value` parses `value` as TOML and falls back to a raw literal only when parsing *fails*. A value that parses as a **non-string** is a hard launch error, verified against 0.146.0:

```
$ codex debug prompt-input -c 'developer_instructions=true'
Error: invalid type: boolean `true`, expected a string
```

Custom instructions are user-supplied. Without quoting, a user whose instruction text is exactly `true` or `123` cannot launch Codex at all. Quoting makes every possible input parse as a string.

**Files:**
- Modify: `src/pty/command.rs` (add the function next to `compose_injected_prompt`, which ends at `:471`)
- Test: `src/pty/command.rs` — inside the existing `mod tests` (opens at `:541`, which already has `use super::*;`)

**Interfaces:**
- Consumes: nothing.
- Produces: `fn toml_basic_string(s: &str) -> String` — private to `src/pty/command.rs`. Returns the TOML basic-string encoding of `s` **including the surrounding double quotes**. Task 2 calls it.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src/pty/command.rs`, immediately after the `system_prompt_combines_rename_and_custom` test:

```rust
#[test]
fn toml_basic_string_wraps_and_escapes() {
    assert_eq!(toml_basic_string(""), r#""""#);
    assert_eq!(toml_basic_string("hello"), r#""hello""#);
    assert_eq!(toml_basic_string("say \"hi\""), r#""say \"hi\"""#);
    assert_eq!(toml_basic_string("a\\b"), r#""a\\b""#);
    assert_eq!(toml_basic_string("a\nb"), r#""a\nb""#);
    assert_eq!(toml_basic_string("a\tb"), r#""a\tb""#);
    assert_eq!(toml_basic_string("a\rb"), r#""a\rb""#);
    assert_eq!(toml_basic_string("a\u{1}b"), r#""a\u0001b""#);
    assert_eq!(toml_basic_string("a\u{7f}b"), r#""a\u007Fb""#);
}

/// The whole reason this helper exists: an unquoted value that parses as a
/// TOML non-string makes `codex -c` refuse to launch with
/// "invalid type: boolean `true`, expected a string".
#[test]
fn toml_basic_string_quotes_values_that_would_parse_as_non_strings() {
    assert_eq!(toml_basic_string("true"), r#""true""#);
    assert_eq!(toml_basic_string("123"), r#""123""#);
    assert_eq!(toml_basic_string("[1, 2]"), r#""[1, 2]""#);
}

/// Markdown doctrine text must survive verbatim apart from the escapes.
#[test]
fn toml_basic_string_preserves_markdown_punctuation() {
    let encoded = toml_basic_string("## Doctrine\n\n- run `wsx status set` — now");
    assert_eq!(
        encoded,
        r#""## Doctrine\n\n- run `wsx status set` — now""#
    );
}
```

Note on reading these: `r#"..."#` is a Rust raw string, so `r#""a\nb""#` is the 6-character sequence `"a\nb"` — a literal backslash followed by `n`, wrapped in real quote characters. That is exactly what TOML expects on the wire.

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test --lib toml_basic_string
```

Expected: FAIL to compile — ``cannot find function `toml_basic_string` in this scope``.

- [ ] **Step 3: Write the implementation**

Add to `src/pty/command.rs` directly after `compose_injected_prompt` (after `:471`):

```rust
/// Encode `s` as a TOML basic string, surrounding quotes included, so it can
/// be used as the value half of a `codex -c key=value` override.
///
/// `-c` parses the value as TOML and only falls back to treating it as a raw
/// literal when parsing *fails*. A value that parses as a non-string is a hard
/// launch error (`-c developer_instructions=true` →
/// "invalid type: boolean `true`, expected a string"). Since custom
/// instructions are user-supplied, quoting is what stops a user's own text
/// from breaking their spawn.
///
/// Escapes per the TOML basic-string rules: `\` and `"`, the shorthand
/// escapes for tab/newline/carriage-return, and every other control character
/// (U+0000–U+001F, U+007F) as `\uXXXX`.
fn toml_basic_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04X}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test --lib toml_basic_string
```

Expected: PASS — 3 tests.

- [ ] **Step 5: Check formatting and lints**

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: clean. `toml_basic_string` is not yet called by anything, but it is used by the tests, so there is no dead-code warning.

- [ ] **Step 6: Commit**

```bash
git add src/pty/command.rs
git commit -m "feat(codex): add TOML basic-string escaper for -c override values"
```

---

### Task 2: Emit the two `-c` overrides from `build_codex_command`

**Files:**
- Modify: `src/pty/command.rs:473-538` (`build_codex_command` and its doc comment)
- Test: `src/pty/command.rs` — `mod tests`, after `codex_fresh_injects_notify_status_wiring` (`:1770`)

**Interfaces:**
- Consumes: `toml_basic_string(&str) -> String` from Task 1; `compose_injected_prompt(&SpawnMode) -> Option<String>` (`:435`, unchanged); the existing `codex_argv(&SpawnMode) -> Vec<String>` test helper (`:1654`).
- Produces: the argv contract that Task 3 relies on — a `Fresh` Codex spawn carries its instructions in argv, so no worktree file is needed.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src/pty/command.rs`, after `codex_fresh_injects_notify_status_wiring`:

```rust
#[test]
fn codex_fresh_emits_developer_instructions() {
    let mut env = EnvGuard::new();
    env.set("WSX_CODEX_BIN", "codex");
    env.remove("WSX_CODEX_MODEL");
    let argv = codex_argv(&SpawnMode::Fresh {
        rename_ctx: None,
        custom_instructions: Some("CUSTOM_MARK".to_string()),
        doctrine: Some("DOCTRINE_MARK".to_string()),
        additional_dirs: vec![],
        yolo: false,
    });
    let value = argv
        .iter()
        .find(|a| a.starts_with("developer_instructions="))
        .unwrap_or_else(|| panic!("no developer_instructions arg: {argv:?}"));
    assert!(value.contains("DOCTRINE_MARK"), "argv: {argv:?}");
    assert!(value.contains("CUSTOM_MARK"), "argv: {argv:?}");
}

#[test]
fn codex_fresh_emits_claude_md_project_doc_fallback() {
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
    assert!(
        argv.iter()
            .any(|a| a == r#"project_doc_fallback_filenames=["CLAUDE.md"]"#),
        "argv: {argv:?}"
    );
}

/// Doctrine disabled, no rename, no custom instructions: nothing to inject, so
/// no `developer_instructions` arg — but the CLAUDE.md fallback is
/// unconditional on Fresh, since it is about how Codex finds project docs
/// rather than about wsx having something to say.
#[test]
fn codex_fresh_without_injectable_content_omits_developer_instructions() {
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
    assert!(
        !argv.iter().any(|a| a.starts_with("developer_instructions=")),
        "argv: {argv:?}"
    );
    assert!(
        argv.iter()
            .any(|a| a == r#"project_doc_fallback_filenames=["CLAUDE.md"]"#),
        "argv: {argv:?}"
    );
}

/// `codex resume --last` restores the session's stored config and silently
/// ignores `-c` overrides for both instruction keys (verified against
/// codex-cli 0.146.0). Emitting them on Continue would make the argv assert
/// something untrue.
#[test]
fn codex_continue_omits_instruction_config() {
    let mut env = EnvGuard::new();
    env.set("WSX_CODEX_BIN", "codex");
    env.remove("WSX_CODEX_MODEL");
    let argv = codex_argv(&SpawnMode::Continue {
        custom_instructions: Some("CUSTOM_MARK".to_string()),
        doctrine: Some("DOCTRINE_MARK".to_string()),
        additional_dirs: vec![],
        yolo: false,
    });
    assert!(
        !argv.iter().any(|a| a.starts_with("developer_instructions=")),
        "argv: {argv:?}"
    );
    assert!(
        !argv
            .iter()
            .any(|a| a.starts_with("project_doc_fallback_filenames=")),
        "argv: {argv:?}"
    );
    assert!(
        !argv.iter().any(|a| a.contains("DOCTRINE_MARK")),
        "no instruction text may leak into a resume argv: {argv:?}"
    );
}

/// A custom instruction of literal `true` must not reach codex as a bare TOML
/// boolean — that is a hard launch failure, not a fallback.
#[test]
fn codex_developer_instructions_value_is_a_quoted_toml_string() {
    let mut env = EnvGuard::new();
    env.set("WSX_CODEX_BIN", "codex");
    env.remove("WSX_CODEX_MODEL");
    let argv = codex_argv(&SpawnMode::Fresh {
        rename_ctx: None,
        custom_instructions: Some("true".to_string()),
        doctrine: None,
        additional_dirs: vec![],
        yolo: false,
    });
    assert!(
        argv.iter().any(|a| a == r#"developer_instructions="true""#),
        "argv: {argv:?}"
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test --lib codex_
```

Expected: the five new tests FAIL (`no developer_instructions arg: [...]`, and missing-fallback assertion failures). `codex_continue_omits_instruction_config` will *pass* already — that is fine, it is a regression guard for the next step.

- [ ] **Step 3: Write the implementation**

In `src/pty/command.rs`, replace these three lines of the `build_codex_command` doc comment:

```rust
/// Codex has no `--append-system-prompt`; instruction injection (doctrine /
/// rename / custom) is handled by `prepare_codex_workspace` via AGENTS.md.
/// The `remote` arg is unused — wsx's RemoteOpts targets Claude's
```

with:

```rust
/// Codex has no `--append-system-prompt`. Instruction injection (doctrine /
/// rename / custom) rides on `-c developer_instructions=<toml string>`, which
/// Codex renders as the first developer-role message — ahead of its own
/// instructions and of the user-role message carrying AGENTS.md. A second
/// override, `project_doc_fallback_filenames=["CLAUDE.md"]`, lets Codex read a
/// repo's `CLAUDE.md` when it has no `AGENTS.md`. Nothing is written to the
/// worktree.
///
/// Both overrides are **Fresh-only**: `codex resume --last` restores the
/// session's stored config and silently ignores `-c` for these two keys
/// (verified against codex-cli 0.146.0). A resumed session already carries the
/// doctrine in its history from the Fresh spawn that created it.
/// The `remote` arg is unused — wsx's RemoteOpts targets Claude's
```

Then insert this block into the function body, immediately after the `notify` status-wiring `if` block that ends at `:512` and before the `let (resume, yolo) = ...` binding:

```rust
    // Instruction injection + project-doc fallback. `-c` is a global flag
    // accepted before any subcommand; Fresh emits no subcommand anyway.
    if matches!(mode, SpawnMode::Fresh { .. }) {
        if let Some(prompt) = compose_injected_prompt(mode) {
            cmd.arg("-c");
            cmd.arg(format!(
                "developer_instructions={}",
                toml_basic_string(&prompt)
            ));
        }
        cmd.arg("-c");
        cmd.arg(r#"project_doc_fallback_filenames=["CLAUDE.md"]"#);
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test --lib codex_
```

Expected: PASS — the five new tests plus the five pre-existing `codex_*` tests, all green.

- [ ] **Step 5: Run the full suite to catch collateral damage**

```bash
cargo test --all-targets --all-features
```

Expected: PASS. The Hermes AGENTS.md tests must still be green — nothing in this task touched them.

- [ ] **Step 6: Check formatting and lints**

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add src/pty/command.rs
git commit -m "feat(codex): inject doctrine via -c developer_instructions

Fresh Codex spawns now carry the composed prompt in argv as a
developer-role message, plus a project_doc_fallback_filenames override
so a repo's CLAUDE.md is read when it has no AGENTS.md. Continue passes
neither: codex resume --last ignores -c for both keys."
```

---

### Task 3: Stop writing to the worktree on the Codex path

**Files:**
- Modify: `src/pty/workspace_prep.rs:1-8` (module doc), `:190-203` (`prepare_codex_workspace`)
- Test: `src/pty/workspace_prep.rs:589-639` — replace the two top-level Codex tests
- Modify: `docs/book/src/configuration/coding-agents.md:46-60`

**Interfaces:**
- Consumes: the argv contract from Task 2 — instructions now reach Codex without a file, which is what makes this deletion safe.
- Produces: `prepare_codex_workspace(_cwd: &Path, _mode: &SpawnMode)` — signature unchanged so the call site at `src/pty/session.rs:418` needs no edit; both parameters become unused.

- [ ] **Step 1: Write the failing test**

In `src/pty/workspace_prep.rs`, **delete** the two top-level Codex tests at `:589-639` — `prepare_codex_workspace_injects_rename_block_into_agents_md` and `prepare_codex_workspace_writes_no_hermes_marker` — and put this single test in their place. It subsumes both: the marker assertion is carried over, and the block assertion is inverted.

```rust
/// Codex instructions ride on `-c developer_instructions` in
/// `build_codex_command`, so preparing a Codex worktree must leave no trace:
/// no AGENTS.md, no git-exclude entry, and (as always) no Hermes marker.
#[test]
fn prepare_codex_workspace_writes_nothing_to_worktree() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path();
    std::fs::create_dir_all(cwd.join(".git/info")).unwrap();
    // A CLAUDE.md would previously have been copied into a fresh AGENTS.md.
    std::fs::write(cwd.join("CLAUDE.md"), "# Project rules\n").unwrap();
    let mode = SpawnMode::Fresh {
        rename_ctx: Some(RenameContext {
            current_branch: "prefix/my-slug".to_string(),
            branch_prefix: "prefix".to_string(),
            repo_name: "myrepo".to_string(),
            current_slug: "my-slug".to_string(),
        }),
        custom_instructions: Some("CUSTOM".to_string()),
        doctrine: Some("DOCTRINE-MARKER".to_string()),
        additional_dirs: vec![],
        yolo: false,
    };
    prepare_codex_workspace(cwd, &mode);

    assert!(
        !cwd.join("AGENTS.md").exists(),
        "codex must not write AGENTS.md"
    );
    let exclude = std::fs::read_to_string(cwd.join(".git/info/exclude")).unwrap_or_default();
    assert!(
        !exclude.contains("AGENTS.md"),
        "codex must not git-exclude AGENTS.md: {exclude:?}"
    );
    assert!(
        !cwd.join(".git/info/wsx-hermes-spawn-at").exists(),
        "codex must not write the hermes spawn marker"
    );
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cargo test --lib prepare_codex_workspace
```

Expected: FAIL — `codex must not write AGENTS.md`, because `prepare_codex_workspace` still calls `write_agents_md_section`.

- [ ] **Step 3: Write the implementation**

In `src/pty/workspace_prep.rs`, replace `prepare_codex_workspace` (`:190-203`) in full:

```rust
/// Prepare a worktree for a Codex spawn: sync Claude slash-commands into
/// Codex's plugin directory.
///
/// Instruction injection deliberately does **not** happen here. Doctrine,
/// rename hint, and custom instructions ride on `-c developer_instructions` in
/// `build_codex_command`, and a repo's `CLAUDE.md` is picked up via
/// `-c project_doc_fallback_filenames`. So unlike the Hermes path, the Codex
/// path writes nothing to the worktree — no `AGENTS.md`, no `.git/info/exclude`
/// entry. Both parameters are retained to keep the signature symmetric with
/// `prepare_hermes_workspace` at the `src/pty/session.rs` call site.
pub(crate) fn prepare_codex_workspace(_cwd: &Path, _mode: &SpawnMode) {
    #[cfg(not(test))]
    crate::agent::codex_commands::sync_claude_commands_for_codex();
}
```

Then update the module doc at `:1-8` — it currently claims the AGENTS.md mechanism covers both agents. Replace lines 1-8 with:

```rust
//! Workspace preparation for Hermes spawns.
//!
//! Hermes reads project instructions from `AGENTS.md` (rather than Claude's
//! native `CLAUDE.md` / `--append-system-prompt`), so before spawning it wsx
//! rewrites a `BEGIN/END wsx-managed` block in that file, hides it from
//! `git status`, and records a spawn-timestamp marker for session detection.
//! Pure side-effecting helpers over a worktree path + SpawnMode;
//! `prepare_*_workspace` are re-exported from `pty::session` for the spawn path.
//!
//! Codex used to share this mechanism. It no longer does — its instructions go
//! through `-c developer_instructions` in `pty::command`, so
//! `prepare_codex_workspace` writes nothing to the worktree.
```

- [ ] **Step 4: Run the test to verify it passes**

```bash
cargo test --lib prepare_codex_workspace
```

Expected: PASS — 1 test.

- [ ] **Step 5: Run the full suite**

```bash
cargo test --all-targets --all-features
```

Expected: PASS. Watch specifically for the `hermes_agents_md`, `hermes_git_exclude`, and `hermes_prepare_workspace` test modules — they are the regression guard proving the shared helpers still work for Hermes.

- [ ] **Step 6: Check formatting and lints**

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: clean. If clippy reports `read_claude_md` or `CLAUDE_PROVENANCE_COMMENT` as dead code, that means a Hermes caller was removed by mistake — revert that, do not silence the lint.

- [ ] **Step 7: Update the user-facing docs**

In `docs/book/src/configuration/coding-agents.md`, replace lines 46-60 (from the `### Codex integration` heading through the paragraph ending `…Codex can't load them).`) with:

````markdown
### Codex integration

When a workspace uses `coding_agent: codex`, wsx spawns `codex` (or the path in `WSX_CODEX_BIN`) instead of `claude`. Codex receives wsx custom instructions and auto-rename directives.

**Instruction injection**: Codex has no `--append-system-prompt` flag, so wsx passes the workspace doctrine, the auto-rename hint, and any custom instructions as a Codex config override on the spawn command line:

```bash
codex -c 'developer_instructions="…injected instructions…"' \
      -c 'project_doc_fallback_filenames=["CLAUDE.md"]'
```

Codex renders `developer_instructions` as the first developer-role message, ahead of its own instructions and ahead of the user-role message that carries `AGENTS.md`. **Nothing is written to your worktree** — no `AGENTS.md`, no `.git/info/exclude` entry. A repo's own `AGENTS.md` is still read by Codex as usual, and `project_doc_fallback_filenames` makes Codex fall back to `CLAUDE.md` in repos that have no `AGENTS.md`. The superpowers-skills doctrine clause is omitted for Codex (those skills install under `~/.claude` and Codex can't load them).

Both overrides are applied only to **fresh** spawns. `codex resume --last` restores the session's stored configuration and ignores these two keys, so a resumed session keeps the doctrine it was started with. Requires Codex `0.146.0` or newer.
````

Leave the surrounding Hermes section (lines 21-34) untouched — that mechanism is unchanged.

- [ ] **Step 8: Commit**

```bash
git add src/pty/workspace_prep.rs docs/book/src/configuration/coding-agents.md
git commit -m "refactor(codex): stop rewriting AGENTS.md on the Codex spawn path

prepare_codex_workspace now only syncs Claude slash-commands; doctrine
delivery moved to -c overrides in build_codex_command. The AGENTS.md
helpers stay in place for Hermes."
```

---

## Manual verification

After Task 3, confirm the real binary agrees with the argv tests. From any wsx Codex worktree:

```bash
# 1. Doctrine lands as the first developer-role message.
codex debug prompt-input -c 'developer_instructions="WSX-DOCTRINE-MARKER"' \
  | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d[0]["role"], repr(d[0]["content"][0]["text"][:40]))'
# Expected: developer 'WSX-DOCTRINE-MARKER'

# 2. Spawn a fresh Codex workspace through wsx, then confirm the worktree is clean.
git status --porcelain          # expect: no AGENTS.md
grep AGENTS.md .git/info/exclude # expect: no match (in a worktree that never ran the old code)
```

Then ask the spawned Codex session a question the doctrine answers — e.g. "what should you do before starting substantive work?" — and confirm it cites `wsx status set` / `wsx recap set`.

## Out of scope

- Removing Hermes, or the AGENTS.md helpers it still uses.
- Stripping stale `BEGIN/END wsx-managed` blocks left in worktrees by the old code. The spec records this as an accepted regression: the block is self-guarding and git-excluded, so the cost is redundant tokens, not wrong behaviour.
- Re-injecting refreshed custom instructions on `Continue`. Codex offers no channel for it.
- `model_instructions_file` / `instructions`, which replace Codex's base instructions rather than adding to them.
