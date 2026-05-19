# Named remotes (`wsx remote <name>`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `wsx remote <name>` — store named shell commands (typically `ssh -t host '…tmux attach…'`) in the settings table and exec them by name.

**Architecture:** Reuse the `pinned_commands` storage shape (one `settings` row at key `remotes`, newline-separated `name=command` blob) and parser shape (split-on-first-`=`, trim, drop empties). New module `src/remotes.rs` for parse + lookup. New `CliAction::RemoteList` / `RemoteRun` variants in `src/cli.rs`; the run arm `exec`-replaces the wsx process via `sh -c`. The existing `src/remote.rs` is renamed to `src/remote_control.rs` to free the `remote` module name.

**Tech Stack:** Rust, hand-rolled CLI parser in `src/cli.rs`, `rusqlite` via the existing `Store` API, `std::os::unix::process::CommandExt::exec` for process replacement.

**Spec:** [`docs/superpowers/specs/2026-05-18-named-remotes-design.md`](../specs/2026-05-18-named-remotes-design.md)

---

## File map

- **Rename:** `src/remote.rs` → `src/remote_control.rs` (mechanical, ~5 call sites)
- **Create:** `src/remotes.rs` — `Remote` struct, `parse`, `list`, `lookup`
- **Modify:** `src/lib.rs` — swap module declaration
- **Modify:** `src/cli.rs` — add `RemoteList` / `RemoteRun` variants, parse arm, dispatch, add `"remotes"` to `known_setting_key`
- **Create:** `docs/manual-tests/named-remotes.md` — smoke procedure

---

## Task 1: Rename `remote.rs` → `remote_control.rs`

Mechanical rename to free the `remote` module name. The renamed module is solely about claude's `--remote-control` flag (see its own module doc); the new name is more accurate.

**Files:**
- Rename: `src/remote.rs` → `src/remote_control.rs`
- Modify: `src/lib.rs:16`
- Modify: `src/pty/session.rs` (~30 references)
- Modify: `src/pm.rs:188`
- Modify: `src/app.rs:1248`

- [ ] **Step 1: Rename the file**

Run:
```bash
git mv src/remote.rs src/remote_control.rs
```

- [ ] **Step 2: Update `src/lib.rs` module declaration**

Change line 16:
```rust
pub mod remote;
```
to:
```rust
pub mod remote_control;
```

- [ ] **Step 3: Update all `crate::remote::` references**

Use the Edit tool with `replace_all` on each of these files, replacing `crate::remote::` with `crate::remote_control::`:

- `src/pty/session.rs`
- `src/pm.rs`
- `src/app.rs`

Verify zero remaining references afterward:
```bash
grep -rn "crate::remote::" src/ tests/
```
Expected: no output (the new uses are `crate::remote_control::`).

- [ ] **Step 4: Build + test**

Run:
```bash
cargo build 2>&1 | tail -20
cargo test --lib 2>&1 | tail -20
```
Expected: build succeeds; all existing tests pass (including the `remote_control::tests::*` block that moved with the file).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: rename remote module to remote_control

Frees the \`remote\` name for an upcoming CLI subcommand
(\`wsx remote <name>\`). The renamed module is solely about
claude's \`--remote-control\` flag, so the new name is also
more accurate."
```

---

## Task 2: Parser in `src/remotes.rs` (TDD)

Mirrors `src/pinned.rs::parse` but with `Remote { name, command }` types. Same semantics: split-on-first-`=`, trim, drop empty halves, skip blank lines. Lines without `=` use the line as both name and command (matches pinned's permissive fallback).

**Files:**
- Create: `src/remotes.rs`
- Modify: `src/lib.rs` (add `pub mod remotes;`)

- [ ] **Step 1: Create `src/remotes.rs` skeleton with failing tests**

Write `src/remotes.rs`:

```rust
//! Named remote shell commands. Stored as a newline-separated
//! `name=command` blob in the `remotes` setting; executed by name
//! via `wsx remote <name>` (which exec-replaces wsx with `sh -c`).
//!
//! See `docs/superpowers/specs/2026-05-18-named-remotes-design.md`.

use crate::error::Result;
use crate::store::Store;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub name: String,
    pub command: String,
}

pub fn parse(_text: &str) -> Vec<Remote> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_labeled_line() {
        let out = parse("ebenmini=ssh -4 -t ebenmini.local 'tmux attach'");
        assert_eq!(
            out,
            vec![Remote {
                name: "ebenmini".into(),
                command: "ssh -4 -t ebenmini.local 'tmux attach'".into(),
            }]
        );
    }

    #[test]
    fn parse_unlabeled_line_uses_command_as_name() {
        let out = parse("ssh foo");
        assert_eq!(
            out,
            vec![Remote {
                name: "ssh foo".into(),
                command: "ssh foo".into(),
            }]
        );
    }

    #[test]
    fn parse_skips_blank_lines() {
        let out = parse("a=ssh a\n\nb=ssh b\n");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].name, "a");
        assert_eq!(out[1].name, "b");
    }

    #[test]
    fn parse_trims_both_sides_of_equals() {
        let out = parse("  gpu  =   ssh gpu-box   ");
        assert_eq!(
            out,
            vec![Remote {
                name: "gpu".into(),
                command: "ssh gpu-box".into(),
            }]
        );
    }

    #[test]
    fn parse_treats_only_first_equals_as_separator() {
        // The command may legitimately contain `=` (e.g. env vars).
        let out = parse("envset=FOO=bar ssh host");
        assert_eq!(
            out,
            vec![Remote {
                name: "envset".into(),
                command: "FOO=bar ssh host".into(),
            }]
        );
    }

    #[test]
    fn parse_drops_empty_name_or_command() {
        assert!(parse("=").is_empty());
        assert!(parse("name=").is_empty());
        assert!(parse("=cmd").is_empty());
    }

    #[test]
    fn parse_preserves_nested_quotes_verbatim() {
        // The motivating example: nested double + single quotes.
        let out = parse(r#"ebenmini=ssh -4 -t ebenmini.local "zsh -lc 'tmux attach'""#);
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].command,
            r#"ssh -4 -t ebenmini.local "zsh -lc 'tmux attach'""#
        );
    }
}
```

Note: `Result` and `Store` are imported now for use in Task 3 — keep them.

- [ ] **Step 2: Wire the module up**

Edit `src/lib.rs`. The current module list (after Task 1) has `pub mod remote_control;`. Insert `pub mod remotes;` in alphabetical order — between `remote_control` and `repo`:

```rust
pub mod remote_control;
pub mod remotes;
pub mod repo;
```

- [ ] **Step 3: Run the parser tests; verify they fail**

Run:
```bash
cargo test --lib remotes::tests 2>&1 | tail -20
```
Expected: tests fail with `panicked at 'not implemented'` from `unimplemented!()`.

- [ ] **Step 4: Implement `parse`**

Replace the body of `parse` in `src/remotes.rs`:

```rust
pub fn parse(text: &str) -> Vec<Remote> {
    text.lines()
        .filter_map(|raw| {
            let line = raw.trim();
            if line.is_empty() {
                return None;
            }
            let (name, command) = match line.split_once('=') {
                Some((lhs, rhs)) => (lhs.trim().to_string(), rhs.trim().to_string()),
                None => (line.to_string(), line.to_string()),
            };
            if name.is_empty() || command.is_empty() {
                return None;
            }
            Some(Remote { name, command })
        })
        .collect()
}
```

- [ ] **Step 5: Run tests; verify they pass**

Run:
```bash
cargo test --lib remotes::tests 2>&1 | tail -20
```
Expected: all 7 `remotes::tests::*` tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/remotes.rs src/lib.rs
git commit -m "feat(remotes): add parser for named remote shell commands

Adds \`src/remotes.rs\` with the \`Remote\` type and a \`parse\` fn
matching the \`pinned_commands\` parser shape (split on first \`=\`,
trim, drop empties). Storage and dispatch follow in subsequent
commits."
```

---

## Task 3: `list` + `lookup` helpers in `src/remotes.rs` (TDD)

Reads the `remotes` setting from the store, parses it, exposes a sorted list and a name-to-command lookup.

**Files:**
- Modify: `src/remotes.rs`

- [ ] **Step 1: Add failing tests for `list` and `lookup`**

Append inside the `#[cfg(test)] mod tests` block in `src/remotes.rs`, after the existing tests:

```rust
    #[test]
    fn list_returns_empty_when_unset() {
        let store = Store::open_in_memory().unwrap();
        assert!(list(&store).unwrap().is_empty());
    }

    #[test]
    fn list_returns_alphabetized() {
        let store = Store::open_in_memory().unwrap();
        store
            .set_setting("remotes", "zebra=ssh z\napple=ssh a\nmango=ssh m\n")
            .unwrap();
        let out = list(&store).unwrap();
        let names: Vec<_> = out.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["apple", "mango", "zebra"]);
    }

    #[test]
    fn lookup_returns_command_for_known_name() {
        let store = Store::open_in_memory().unwrap();
        store
            .set_setting("remotes", "gpu=ssh gpu-box -t 'tmux attach'\n")
            .unwrap();
        assert_eq!(
            lookup(&store, "gpu").unwrap().as_deref(),
            Some("ssh gpu-box -t 'tmux attach'")
        );
    }

    #[test]
    fn lookup_returns_none_for_unknown_name() {
        let store = Store::open_in_memory().unwrap();
        store.set_setting("remotes", "gpu=ssh gpu-box\n").unwrap();
        assert!(lookup(&store, "nope").unwrap().is_none());
    }

    #[test]
    fn lookup_returns_none_when_unset() {
        let store = Store::open_in_memory().unwrap();
        assert!(lookup(&store, "anything").unwrap().is_none());
    }

    #[test]
    fn lookup_last_write_wins_for_duplicate_names() {
        let store = Store::open_in_memory().unwrap();
        store
            .set_setting("remotes", "h=first\nh=second\n")
            .unwrap();
        assert_eq!(lookup(&store, "h").unwrap().as_deref(), Some("second"));
    }
```

- [ ] **Step 2: Run tests; verify they fail to compile**

Run:
```bash
cargo test --lib remotes::tests 2>&1 | tail -20
```
Expected: compile error — `list` and `lookup` not defined.

- [ ] **Step 3: Implement `list` and `lookup`**

Add to `src/remotes.rs`, between `parse` and the `#[cfg(test)]` block:

```rust
/// Returns all configured remotes, alphabetized by name.
pub fn list(store: &Store) -> Result<Vec<Remote>> {
    let raw = store.get_setting("remotes")?.unwrap_or_default();
    let mut out = parse(&raw);
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Returns the command for `name`, or `None` if no remote with that
/// name is configured. When the blob contains duplicate names, the
/// last one wins (matches the order of the underlying blob).
pub fn lookup(store: &Store, name: &str) -> Result<Option<String>> {
    let raw = store.get_setting("remotes")?.unwrap_or_default();
    Ok(parse(&raw)
        .into_iter()
        .rev()
        .find(|r| r.name == name)
        .map(|r| r.command))
}
```

- [ ] **Step 4: Run tests; verify they pass**

Run:
```bash
cargo test --lib remotes::tests 2>&1 | tail -20
```
Expected: all `remotes::tests::*` tests pass (parser tests from Task 2 plus 6 new ones = 13 total).

- [ ] **Step 5: Commit**

```bash
git add src/remotes.rs
git commit -m "feat(remotes): add list and lookup helpers

\`list\` returns all configured remotes alphabetized; \`lookup\`
resolves a name to its command (or None). Duplicate names: last
write wins."
```

---

## Task 4: CLI parsing for `remote` subcommand (TDD)

Adds two `CliAction` variants and the parse arm. Also adds `"remotes"` to `known_setting_key` so `wsx config edit remotes` works.

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 1: Add failing parse tests + `known_setting_key` test**

In `src/cli.rs`, inside the existing `#[cfg(test)] mod tests` block, add at the end (before the closing `}`):

```rust
    #[test]
    fn parses_remote_list_no_args() {
        match parse(&["remote"]).unwrap() {
            CliAction::RemoteList => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_remote_run_with_name() {
        match parse(&["remote", "ebenmini"]).unwrap() {
            CliAction::RemoteRun { name } => assert_eq!(name, "ebenmini"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn accepts_remotes_setting_key() {
        assert!(known_setting_key("remotes"));
    }
```

- [ ] **Step 2: Run tests; verify they fail to compile**

Run:
```bash
cargo test --lib cli::tests 2>&1 | tail -20
```
Expected: compile errors — `CliAction::RemoteList` / `RemoteRun` not defined, and `known_setting_key("remotes")` returns false.

- [ ] **Step 3: Add the `CliAction` variants**

In `src/cli.rs`, inside the `pub enum CliAction` block (after `ConfigEdit { key: String }`, before the closing `}` near line 64):

```rust
    RemoteList,
    RemoteRun {
        name: String,
    },
```

- [ ] **Step 4: Add `"remotes"` to `known_setting_key`**

In `src/cli.rs`, edit `known_setting_key` (around line 90). Add `"remotes"` to the `matches!` arm. The block becomes:

```rust
fn known_setting_key(k: &str) -> bool {
    matches!(
        k,
        "branch_prefix"
            | "custom_instructions"
            | "nerd_fonts"
            | "editor_cmd"
            | "terminal_cmd"
            | "diff_cmd"
            | "notifications"
            | "theme"
            | "pm_enabled"
            | "pm_custom_instructions"
            | "mcp_mirror"
            | "remote_control"
            | "remote_control_sandbox"
            | "pinned_commands"
            | "remotes"
    )
}
```

- [ ] **Step 5: Add the parse arm**

In `src/cli.rs`, inside `parse_args`, the outer `match it.next().as_deref()` block currently has arms for `None`, `Some("repo")`, `Some("config")`, and a final `Some(other)` catch-all. Insert a new arm **before** the `Some(other)` catch-all (the final `Err(...)` arm):

```rust
        Some("remote") => match it.next() {
            None => Ok(CliAction::RemoteList),
            Some(name) => Ok(CliAction::RemoteRun { name }),
        },
```

- [ ] **Step 6: Run tests; verify they pass**

Run:
```bash
cargo test --lib cli::tests 2>&1 | tail -20
```
Expected: all `cli::tests::*` tests pass, including the 3 new ones.

Also confirm the dispatch arm doesn't yet exist — the build should fail because `run_cli` lacks `RemoteList` / `RemoteRun` match arms:

```bash
cargo build 2>&1 | tail -10
```
Expected: error E0004 (non-exhaustive patterns in `run_cli`'s match). That's fine — Task 5 fills it in.

- [ ] **Step 7: Commit (skip — defer until Task 5)**

Do not commit here; the workspace doesn't build. Task 5 closes the loop.

---

## Task 5: CLI dispatch — list + exec

Implements the `RemoteList` and `RemoteRun` arms of `run_cli`. `RemoteList` prints sorted names. `RemoteRun` looks up the command, prints a helpful error with the available names on miss, and exec-replaces the process with `sh -c <command>` on hit.

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 1: Add the dispatch arms**

In `src/cli.rs`, inside `run_cli`'s `match action` block, add two new arms after the `ConfigEdit` arm (before the closing `}` of the match):

```rust
        CliAction::RemoteList => {
            let remotes = crate::remotes::list(&store)?;
            if remotes.is_empty() {
                println!("no remotes configured. add one with: wsx config edit remotes");
                return Ok(());
            }
            for r in remotes {
                println!("{}", r.name);
            }
        }
        CliAction::RemoteRun { name } => {
            let command = crate::remotes::lookup(&store, &name)?.ok_or_else(|| {
                let available = crate::remotes::list(&store)
                    .ok()
                    .map(|v| {
                        v.into_iter()
                            .map(|r| r.name)
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                if available.is_empty() {
                    Error::UserInput(format!(
                        "no remote named '{name}'. no remotes configured \
                         (add one with: wsx config edit remotes)"
                    ))
                } else {
                    Error::UserInput(format!(
                        "no remote named '{name}'. available: {available}"
                    ))
                }
            })?;
            use std::os::unix::process::CommandExt;
            let err = std::process::Command::new("sh")
                .arg("-c")
                .arg(&command)
                .exec();
            // exec only returns on failure.
            return Err(Error::UserInput(format!("exec sh: {err}")));
        }
```

- [ ] **Step 2: Build + run all tests**

Run:
```bash
cargo build 2>&1 | tail -10
cargo test --lib 2>&1 | tail -20
```
Expected: build succeeds; all tests pass (the new code is not unit-tested — `exec` is a process-replacement side effect, covered by manual smoke in Task 6).

- [ ] **Step 3: Quick live sanity check (no real ssh)**

```bash
cargo run -- remote 2>&1
# expected: "no remotes configured. add one with: wsx config edit remotes"

cargo run -- config set remotes "demo=echo hello && sleep 0.2"
cargo run -- remote
# expected: "demo"

cargo run -- remote demo
# expected: prints "hello", sleeps briefly, exits 0

cargo run -- remote nope 2>&1
# expected: non-zero exit; message contains "no remote named 'nope'"
# and "available: demo". Exact format wraps via thiserror Debug
# (something like: Error: UserInput("no remote named 'nope'. available: demo")).

cargo run -- config set remotes ""
# resets to empty
```

If any of these don't behave as described, stop and debug before Task 6.

- [ ] **Step 4: Commit (covers Task 4 + Task 5)**

```bash
git add src/cli.rs
git commit -m "feat(cli): add \`wsx remote <name>\` to exec named shell commands

- \`wsx remote\` lists configured remotes (sorted).
- \`wsx remote <name>\` resolves the stored command and exec-replaces
  wsx with \`sh -c <command>\` so signals and TTY pass through directly.
- \`remotes\` is now a known setting key, so \`wsx config edit remotes\`
  opens the editable blob.

Closes the feature work begun in the preceding parser + lookup
commits."
```

---

## Task 6: Manual smoke test doc

The exec path isn't unit-tested. Document the live smoke procedure so future work can re-verify.

**Files:**
- Create: `docs/manual-tests/named-remotes.md`

- [ ] **Step 1: Write `docs/manual-tests/named-remotes.md`**

```markdown
# Manual smoke test: named remotes (`wsx remote <name>`)

The automated test suite covers the parser, store lookup, and CLI
arg parsing. This procedure covers what tests can't: process
replacement via `exec`, TTY pass-through, and shell-quoted command
strings reaching `sh -c` unmangled.

## Setup

Build the release binary or use `cargo run --`. Steps below use
`wsx` as shorthand for either.

## Test 1: empty state

```
wsx remote
```

Expected: `no remotes configured. add one with: wsx config edit remotes`

## Test 2: basic exec + return-to-shell

```
wsx config set remotes "demo=echo hello && sleep 1"
wsx remote
# prints: demo
wsx remote demo
# prints: hello (1s pause), exit 0, back at local shell
```

Verify `ps` shows no leftover `wsx` process during the sleep — exec
should have replaced it.

## Test 3: unknown name lists available

```
wsx remote nope
```

Expected: non-zero exit, message includes `no remote named 'nope'.
available: demo`.

## Test 4: nested-quote command (the motivating example)

```
wsx config edit remotes
# add a line like:
#   self=ssh -4 -t localhost "zsh -lc 'tmux new -s wsx-test || tmux attach -t wsx-test'"
wsx remote self
```

Expected:
- Lands inside a tmux session on the remote (`tmux ls` from another
  shell confirms `wsx-test` exists).
- `Ctrl-b d` detaches; ssh exits; you're back at the local shell.
- The nested `"…'…'…"` quoting reached `sh -c` intact — no
  "command not found" or quoting errors.

## Test 5: signal pass-through

```
wsx remote demo  # but with a longer sleep, e.g. "demo=sleep 30"
# press Ctrl-C
```

Expected: Ctrl-C kills `sleep` (not wsx — wsx is already gone),
shell returns immediately with exit 130.

## Cleanup

```
wsx config set remotes ""
```
```

- [ ] **Step 2: Commit**

```bash
git add docs/manual-tests/named-remotes.md
git commit -m "docs: manual smoke procedure for \`wsx remote\`

Covers what unit tests can't: exec process replacement, TTY
pass-through, nested-quote command strings reaching \`sh -c\`
verbatim, and signal pass-through after exec."
```

---

## Done. Final verification

After all tasks:

```bash
cargo build --release 2>&1 | tail -5
cargo test --lib 2>&1 | tail -5
grep -rn "crate::remote::" src/ tests/  # expect: no output
```

Then run through `docs/manual-tests/named-remotes.md` once end-to-end with a real ssh target to confirm the production path.
