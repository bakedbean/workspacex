# Pinned commands Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a user-defined chip row to the attached workspace view that lets you fire pinned slash commands at the claude session via `Ctrl-x <digit>` or mouse click.

**Architecture:** A new `src/pinned.rs` module parses a newline-separated `Label=command` config value into a `Vec<PinnedCommand>` and resolves global vs per-repo precedence. The attached view (`src/ui/attached.rs`) gains a conditional one-row chip strip rendered only when commands exist. App-level handlers in `src/app.rs` dispatch `Ctrl-x N` keystrokes and click coordinates to the same `session.writer.send(format!("{cmd}\r").into_bytes())` path that today's keystrokes use. Storage layers on the existing global-settings + per-repo-column override pattern (matches `branch_prefix`, `custom_instructions`).

**Tech Stack:** Rust 2024, `ratatui` 0.29 for layout, `crossterm` 0.28 for input, `rusqlite` 0.32 for storage, `tokio` for the async PTY writer channel.

**Spec:** `docs/superpowers/specs/2026-05-17-pinned-commands-design.md`

---

## Task 1: Parser module — types, parse, label truncation

**Files:**
- Create: `src/pinned.rs`
- Modify: `src/lib.rs` (add `pub mod pinned;`)

- [ ] **Step 1: Add the module to `src/lib.rs`**

Open `src/lib.rs`, find the section where other top-level modules are declared (e.g. `pub mod cli;`, `pub mod store;`), and add:

```rust
pub mod pinned;
```

Keep alphabetical order with siblings if the existing list is sorted.

- [ ] **Step 2: Write failing parser tests in `src/pinned.rs`**

Create `src/pinned.rs`:

```rust
//! Pinned commands: parses a newline-separated `Label=command` list into
//! addressable chips for the attached view.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedCommand {
    /// Text shown in the chip. Already trimmed; not yet width-truncated
    /// (render decides what fits).
    pub label: String,
    /// Bytes sent to the claude PTY (sans the trailing `\r`).
    pub command: String,
}

pub fn parse(_text: &str) -> Vec<PinnedCommand> {
    todo!()
}

pub fn resolve(_global: Option<&str>, _repo: Option<&str>) -> Vec<PinnedCommand> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_labeled_line() {
        let out = parse("PR=/pull-request");
        assert_eq!(out, vec![PinnedCommand {
            label: "PR".into(),
            command: "/pull-request".into(),
        }]);
    }

    #[test]
    fn parse_unlabeled_line_uses_command_as_label() {
        let out = parse("/feedback");
        assert_eq!(out, vec![PinnedCommand {
            label: "/feedback".into(),
            command: "/feedback".into(),
        }]);
    }

    #[test]
    fn parse_skips_blank_lines() {
        let out = parse("PR=/pull-request\n\n/feedback\n");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].label, "PR");
        assert_eq!(out[1].label, "/feedback");
    }

    #[test]
    fn parse_trims_both_sides_of_equals() {
        let out = parse("  Loop  =   /loop /babysit-prs   ");
        assert_eq!(out, vec![PinnedCommand {
            label: "Loop".into(),
            command: "/loop /babysit-prs".into(),
        }]);
    }

    #[test]
    fn parse_keeps_internal_spaces_in_command() {
        let out = parse("X=/loop /babysit-prs");
        assert_eq!(out[0].command, "/loop /babysit-prs");
    }

    #[test]
    fn parse_treats_only_first_equals_as_separator() {
        // The label is everything before the first `=`. Anything after is the
        // command, including further `=` characters (rare but valid for some
        // commands).
        let out = parse("Kv=/set FOO=bar");
        assert_eq!(out, vec![PinnedCommand {
            label: "Kv".into(),
            command: "/set FOO=bar".into(),
        }]);
    }

    #[test]
    fn parse_returns_lines_past_nine_uncapped() {
        // Render layer caps at 9; parser does not.
        let input = (1..=12)
            .map(|n| format!("/cmd{n}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(parse(&input).len(), 12);
    }

    #[test]
    fn parse_drops_empty_label_or_command() {
        // A line that's just `=` is malformed; drop it. A line where the
        // command after `=` is empty after trim is also dropped.
        assert!(parse("=").is_empty());
        assert!(parse("Label=").is_empty());
        assert!(parse("=cmd").is_empty()); // label is empty after trim
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

```bash
cargo test --lib pinned:: -- --test-threads=1
```

Expected: all `parse_*` tests fail with `not yet implemented` panics from the `todo!()`.

- [ ] **Step 4: Implement `parse`**

Replace the `pub fn parse(_text: &str) -> Vec<PinnedCommand> { todo!() }` stub with:

```rust
pub fn parse(text: &str) -> Vec<PinnedCommand> {
    text.lines()
        .filter_map(|raw| {
            let line = raw.trim();
            if line.is_empty() {
                return None;
            }
            let (label, command) = match line.split_once('=') {
                Some((lhs, rhs)) => (lhs.trim().to_string(), rhs.trim().to_string()),
                None => (line.to_string(), line.to_string()),
            };
            if label.is_empty() || command.is_empty() {
                return None;
            }
            Some(PinnedCommand { label, command })
        })
        .collect()
}
```

- [ ] **Step 5: Run parser tests to verify they pass**

```bash
cargo test --lib pinned::tests::parse_ -- --test-threads=1
```

Expected: 8 passed.

- [ ] **Step 6: Write failing resolve tests**

Append to the `#[cfg(test)] mod tests` block in `src/pinned.rs`:

```rust
    #[test]
    fn resolve_repo_overrides_global() {
        let out = resolve(Some("A=/global"), Some("B=/repo"));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].label, "B");
    }

    #[test]
    fn resolve_empty_repo_falls_back_to_global() {
        let out = resolve(Some("A=/global"), Some(""));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].label, "A");
    }

    #[test]
    fn resolve_whitespace_only_repo_falls_back_to_global() {
        let out = resolve(Some("A=/global"), Some("   \n  \n"));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].label, "A");
    }

    #[test]
    fn resolve_no_repo_uses_global() {
        let out = resolve(Some("A=/global"), None);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].label, "A");
    }

    #[test]
    fn resolve_both_none_returns_empty() {
        assert!(resolve(None, None).is_empty());
    }

    #[test]
    fn resolve_no_global_uses_repo() {
        let out = resolve(None, Some("B=/repo"));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].label, "B");
    }
```

- [ ] **Step 7: Run resolve tests to verify they fail**

```bash
cargo test --lib pinned::tests::resolve_ -- --test-threads=1
```

Expected: 6 tests fail with `not yet implemented` from the `todo!()`.

- [ ] **Step 8: Implement `resolve`**

Replace the stub:

```rust
pub fn resolve(global: Option<&str>, repo: Option<&str>) -> Vec<PinnedCommand> {
    let repo_has_value = repo.map(|s| !s.trim().is_empty()).unwrap_or(false);
    let source = if repo_has_value { repo } else { global };
    match source {
        Some(text) => parse(text),
        None => Vec::new(),
    }
}
```

- [ ] **Step 9: Run all pinned tests + clippy + fmt**

```bash
cargo test --lib pinned:: -- --test-threads=1
```

Expected: 14 passed (8 parse + 6 resolve).

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
```

Expected: both clean.

- [ ] **Step 10: Commit**

```bash
git add src/lib.rs src/pinned.rs
git commit -m "feat(pinned): parser + resolve helper for pinned commands

Newline-separated Label=command syntax; per-repo value overrides
global when non-empty after trim. Render-layer 9-cap not enforced
here.

Part of pinned-commands feature (docs/superpowers/specs/2026-05-17-pinned-commands-design.md)."
```

---

## Task 2: DB column + Repo struct + queries

**Files:**
- Modify: `src/store.rs` — `Repo` struct, `repos()` query, `add_repo` upserts, migration block, new `set_repo_pinned_commands` setter, plus existing tests that construct `Repo` literals.

- [ ] **Step 1: Add migration for the new column**

In `src/store.rs`, find the end of the `fn migrate(&self)` function — currently the last guarded block is `if v < 4 { ... }`. After it (still inside `migrate` before the trailing `Ok(())`), add:

```rust
        if v < 5 {
            let has_col: i64 = self.conn.query_row(
                "SELECT count(*) FROM pragma_table_info('repos') WHERE name = 'pinned_commands'",
                [],
                |r| r.get(0),
            )?;
            if has_col == 0 {
                self.conn
                    .execute("ALTER TABLE repos ADD COLUMN pinned_commands TEXT", [])?;
            }
            self.conn.execute("PRAGMA user_version = 5", [])?;
        }
```

- [ ] **Step 2: Add `pinned_commands` field to the `Repo` struct**

Find the `pub struct Repo` definition (`src/store.rs` around line 29). Add a new field after `archive_script`:

```rust
pub struct Repo {
    pub id: RepoId,
    pub name: String,
    pub path: PathBuf,
    pub branch_prefix: String,
    pub custom_instructions: Option<String>,
    pub setup_script: Option<String>,
    pub archive_script: Option<String>,
    pub pinned_commands: Option<String>,
    pub created_at: i64,
}
```

- [ ] **Step 3: Update the `repos()` SELECT to include and map the new column**

Find `fn repos(&self)`. Update the SELECT and the row mapper:

```rust
    pub fn repos(&self) -> Result<Vec<Repo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, path, branch_prefix, custom_instructions, \
                    setup_script, archive_script, pinned_commands, created_at \
             FROM repos ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Repo {
                id: RepoId(r.get(0)?),
                name: r.get(1)?,
                path: PathBuf::from(r.get::<_, String>(2)?),
                branch_prefix: r.get(3)?,
                custom_instructions: r.get(4)?,
                setup_script: r.get(5)?,
                archive_script: r.get(6)?,
                pinned_commands: r.get(7)?,
                created_at: r.get(8)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
```

- [ ] **Step 4: Add `set_repo_pinned_commands` setter**

After `set_repo_archive_script`, add:

```rust
    pub fn set_repo_pinned_commands(&self, id: RepoId, value: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE repos SET pinned_commands = ?1 WHERE id = ?2",
            rusqlite::params![value, id.0],
        )?;
        Ok(())
    }
```

- [ ] **Step 5: Fix existing tests that construct `Repo` literals**

Search the codebase for `Repo {` constructions:

```bash
grep -rn "Repo {" src/ tests/
```

Every match in `src/store.rs`, `src/ui/dashboard/tests.rs`, `src/ui/dashboard/label_tests.rs`, and any other test file needs `pinned_commands: None,` added before `created_at`. Expected count: roughly 10-15 sites. Use the Edit tool with `replace_all` is unsafe here because the surrounding fields differ subtly — open each file and add the line by hand.

- [ ] **Step 6: Write a test for the setter**

In `src/store.rs`, find the `#[cfg(test)] mod tests` block. Add (near `set_repo_branch_prefix_updates_value`):

```rust
    #[test]
    fn set_repo_pinned_commands_round_trips() {
        let store = Store::open_in_memory().unwrap();
        let id = store
            .add_repo(Path::new("/x"), "demo", "")
            .unwrap();
        store
            .set_repo_pinned_commands(id, Some("PR=/pull-request"))
            .unwrap();
        let repo = store.repos().unwrap().into_iter().find(|r| r.id == id).unwrap();
        assert_eq!(repo.pinned_commands.as_deref(), Some("PR=/pull-request"));

        store.set_repo_pinned_commands(id, None).unwrap();
        let repo = store.repos().unwrap().into_iter().find(|r| r.id == id).unwrap();
        assert!(repo.pinned_commands.is_none());
    }
```

- [ ] **Step 7: Run store tests**

```bash
cargo test --lib store:: -- --test-threads=1
```

Expected: all pass, including the new `set_repo_pinned_commands_round_trips`.

- [ ] **Step 8: Run the full suite + clippy + fmt**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1
```

Expected: all green. Test count up by 1 vs. before.

- [ ] **Step 9: Commit**

```bash
git add src/store.rs src/ui/dashboard/tests.rs src/ui/dashboard/label_tests.rs
# add any other test files touched in Step 5:
git status
git commit -m "feat(store): pinned_commands column + setter

Schema v5: ALTER TABLE repos ADD COLUMN pinned_commands TEXT. NULL
defaults; per-repo override resolution lives in src/pinned.rs.

Part of pinned-commands feature."
```

---

## Task 3: CLI — register `pinned_commands` as a known setting key

**Files:**
- Modify: `src/cli.rs` — `known_setting_key` list + existing config-key tests.

- [ ] **Step 1: Write failing test**

In `src/cli.rs`, find the existing `#[cfg(test)] mod tests` block. Add:

```rust
    #[test]
    fn config_set_accepts_pinned_commands_key() {
        let a = parse(&["config", "set", "pinned_commands", "/feedback"]).unwrap();
        match a {
            CliAction::ConfigSet { key, .. } => assert_eq!(key, "pinned_commands"),
            other => panic!("unexpected action: {other:?}"),
        }
    }
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test --lib cli::tests::config_set_accepts_pinned_commands_key -- --test-threads=1
```

Expected: FAIL with `unknown setting key: pinned_commands`.

- [ ] **Step 3: Add `pinned_commands` to `known_setting_key`**

In `src/cli.rs`, find `fn known_setting_key`. Add the new key to the `matches!` arm:

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
    )
}
```

- [ ] **Step 4: Run to verify pass**

```bash
cargo test --lib cli::tests::config_set_accepts_pinned_commands_key -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: fmt + clippy + full suite**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1
```

Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): register pinned_commands config key

Accept \`wsx config set/get/edit pinned_commands\` via the
existing free-text settings path.

Part of pinned-commands feature."
```

---

## Task 4: CLI — `repo set-pinned-commands` and `repo edit-pinned-commands`

**Files:**
- Modify: `src/cli.rs` — new `CliAction` variants, `parse_args` arms, dispatcher arms in the action runner.

- [ ] **Step 1: Add `CliAction` variants**

In `src/cli.rs`, find the `pub enum CliAction` definition. Near `RepoSetSetup` and `RepoEditSetup`, add:

```rust
    RepoSetPinnedCommands {
        name: String,
        source: ValueSource,
    },
    RepoEditPinnedCommands {
        name: String,
    },
```

- [ ] **Step 2: Add `parse_args` arms**

Find the `Some("repo") => match it.next().as_deref()` block. After the `edit-archive` arm, add:

```rust
            Some("set-pinned-commands") => {
                let name = it.next().ok_or_else(|| {
                    Error::UserInput("repo set-pinned-commands <name> <value-or-@file>".into())
                })?;
                let value = it.next().ok_or_else(|| {
                    Error::UserInput("repo set-pinned-commands <name> <value-or-@file>".into())
                })?;
                Ok(CliAction::RepoSetPinnedCommands {
                    name,
                    source: ValueSource::from_arg(value),
                })
            }
            Some("edit-pinned-commands") => {
                let name = it
                    .next()
                    .ok_or_else(|| Error::UserInput("repo edit-pinned-commands <name>".into()))?;
                Ok(CliAction::RepoEditPinnedCommands { name })
            }
```

- [ ] **Step 3: Add dispatcher arms**

Find the `pub async fn run(action: CliAction, ...)` function (or the equivalent dispatch site — search `CliAction::RepoSetSetup`). After the `RepoSetSetup` arm, add:

```rust
        CliAction::RepoSetPinnedCommands { name, source } => {
            let store = Store::open(&db_path()?)?;
            let repo = store
                .repos()?
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no such repo: {name}")))?;
            let value = source.read()?;
            let opt = if value.trim().is_empty() { None } else { Some(value.as_str()) };
            store.set_repo_pinned_commands(repo.id, opt)?;
            Ok(())
        }
        CliAction::RepoEditPinnedCommands { name } => {
            let store = Store::open(&db_path()?)?;
            let repo = store
                .repos()?
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no such repo: {name}")))?;
            let current = repo.pinned_commands.clone().unwrap_or_default();
            let edited = edit_in_editor(&current, "pinned")?;
            let opt = if edited.trim().is_empty() { None } else { Some(edited.as_str()) };
            store.set_repo_pinned_commands(repo.id, opt)?;
            Ok(())
        }
```

NOTE: copy the *exact* structure and helper-call names from the `RepoSetSetup` / `RepoEditSetup` arms in the same file — those are the source of truth for `ValueSource::read()`, `edit_in_editor`, and `db_path()`. If signatures differ, follow the pattern from those existing arms.

- [ ] **Step 4: Write tests**

Add to the `#[cfg(test)] mod tests` block (alongside other `repo set-setup` parse tests):

```rust
    #[test]
    fn parse_repo_set_pinned_commands_literal() {
        let a = parse(&[
            "repo",
            "set-pinned-commands",
            "demo",
            "PR=/pull-request",
        ])
        .unwrap();
        match a {
            CliAction::RepoSetPinnedCommands { name, source } => {
                assert_eq!(name, "demo");
                assert!(matches!(source, ValueSource::Literal(ref s) if s == "PR=/pull-request"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_repo_set_pinned_commands_at_file() {
        let a = parse(&[
            "repo",
            "set-pinned-commands",
            "demo",
            "@./pinned.txt",
        ])
        .unwrap();
        match a {
            CliAction::RepoSetPinnedCommands { source, .. } => {
                assert!(matches!(source, ValueSource::File(_)));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_repo_edit_pinned_commands() {
        match parse(&["repo", "edit-pinned-commands", "demo"]).unwrap() {
            CliAction::RepoEditPinnedCommands { name } => assert_eq!(name, "demo"),
            other => panic!("unexpected: {other:?}"),
        }
    }
```

(Adjust `ValueSource::Literal` / `ValueSource::File` arm shapes if the actual enum differs — check the existing `parse_repo_set_setup_literal` test for the right pattern.)

- [ ] **Step 5: Run tests**

```bash
cargo test --lib cli:: -- --test-threads=1
```

Expected: all pass.

- [ ] **Step 6: fmt + clippy + full suite**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1
```

Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): repo set-pinned-commands / edit-pinned-commands

Mirrors set-setup / edit-setup. Empty value clears the per-repo
override; resolution then falls back to the global setting.

Part of pinned-commands feature."
```

---

## Task 5: Repo settings modal — add `pinned_commands` as the 5th field

**Files:**
- Modify: `src/app.rs` — `RepoSettingField` enum + `ALL` array + `label` impl + `do_pending_edit` arm + `apply_repo_setting` arm + `handle_key_modal` index bound.
- Modify: `src/ui/modal.rs` — `render_repo_settings` rows array.

- [ ] **Step 1: Extend `RepoSettingField`**

In `src/app.rs`, find `pub enum RepoSettingField`. Update to:

```rust
pub enum RepoSettingField {
    BranchPrefix,
    CustomInstructions,
    SetupScript,
    ArchiveScript,
    PinnedCommands,
}

impl RepoSettingField {
    pub const ALL: [Self; 5] = [
        Self::BranchPrefix,
        Self::CustomInstructions,
        Self::SetupScript,
        Self::ArchiveScript,
        Self::PinnedCommands,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::BranchPrefix => "branch_prefix",
            Self::CustomInstructions => "custom_instructions",
            Self::SetupScript => "setup_script",
            Self::ArchiveScript => "archive_script",
            Self::PinnedCommands => "pinned_commands",
        }
    }
}
```

- [ ] **Step 2: Add the `do_pending_edit` mapping for the new field**

In `src/app.rs`, find the match block around line 335 that picks `(value, extension)` per field:

```rust
            RepoSettingField::PinnedCommands => {
                (repo.pinned_commands.clone().unwrap_or_default(), "txt")
            }
```

Add this arm to the match.

- [ ] **Step 3: Add the `apply_repo_setting` arm**

Find `fn apply_repo_setting` (around line 1083). Add:

```rust
        RepoSettingField::PinnedCommands => {
            app.store.set_repo_pinned_commands(repo_id, opt)
        }
```

- [ ] **Step 4: Update modal index bound**

Find `let field = RepoSettingField::ALL[selected.min(3)];` (around line 1580). Change to:

```rust
let field = RepoSettingField::ALL[selected.min(4)];
```

(The `.min()` is a defensive clamp — bumping the bound matches the new `ALL.len() - 1`.)

- [ ] **Step 5: Update `render_repo_settings` rows array**

In `src/ui/modal.rs`, find the rows array around line 439:

```rust
    let rows: [(crate::app::RepoSettingField, Option<&str>); 5] = [
        (
            crate::app::RepoSettingField::BranchPrefix,
            Some(repo.branch_prefix.as_str()),
        ),
        (
            crate::app::RepoSettingField::CustomInstructions,
            repo.custom_instructions.as_deref(),
        ),
        (
            crate::app::RepoSettingField::SetupScript,
            repo.setup_script.as_deref(),
        ),
        (
            crate::app::RepoSettingField::ArchiveScript,
            repo.archive_script.as_deref(),
        ),
        (
            crate::app::RepoSettingField::PinnedCommands,
            repo.pinned_commands.as_deref(),
        ),
    ];
```

The literal length annotation goes from `; 4]` to `; 5]`; add the new row as the last element.

- [ ] **Step 6: Run the suite**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1
```

Expected: all green. Any existing test asserting `ALL.len() == 4` would surface here — fix to 5 if so.

- [ ] **Step 7: Commit**

```bash
git add src/app.rs src/ui/modal.rs
git commit -m "feat(ui): pinned_commands field in repo settings modal

Adds PinnedCommands to RepoSettingField; modal lists it as the
5th editable field with the same edit-in-\$EDITOR flow as
setup_script / archive_script.

Part of pinned-commands feature."
```

---

## Task 6: Render chip row in attached view

**Files:**
- Modify: `src/ui/attached.rs` — `render` signature gains `pinned`, layout grows by 1 row when non-empty, new `render_chip_row` returns chip Rects, public re-export of `PinnedCommand` consumed.

- [ ] **Step 1: Add a chip-row hit-test helper to `src/pinned.rs`**

Append to `src/pinned.rs` (outside the test module):

```rust
/// Truncate a chip label to fit within `max_cols` columns. If `s` exceeds
/// the budget, returns `s` truncated to `max_cols - 1` chars + `…`.
pub fn truncate_label(s: &str, max_cols: usize) -> String {
    if s.chars().count() <= max_cols {
        return s.to_string();
    }
    let keep: String = s.chars().take(max_cols.saturating_sub(1)).collect();
    format!("{keep}…")
}
```

And a unit test:

```rust
    #[test]
    fn truncate_label_short_passthrough() {
        assert_eq!(truncate_label("PR", 12), "PR");
    }

    #[test]
    fn truncate_label_long_uses_ellipsis() {
        assert_eq!(truncate_label("/loop /babysit-prs", 12), "/loop /baby…");
    }

    #[test]
    fn truncate_label_exact_width_passthrough() {
        assert_eq!(truncate_label("abcdefghijkl", 12), "abcdefghijkl");
    }
```

Run:

```bash
cargo test --lib pinned::tests::truncate -- --test-threads=1
```

Expected: 3 pass.

- [ ] **Step 2: Write failing render tests**

In `src/ui/attached.rs`, ensure a `#[cfg(test)] mod tests` block exists (create one if not). Add:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::pinned::PinnedCommand;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn cmds(specs: &[(&str, &str)]) -> Vec<PinnedCommand> {
        specs
            .iter()
            .map(|(l, c)| PinnedCommand {
                label: (*l).into(),
                command: (*c).into(),
            })
            .collect()
    }

    #[test]
    fn chip_row_layout_returns_rects_for_each_visible_chip() {
        // Wide-enough terminal: all 3 chips fit, expect 3 Rects, none zero-width.
        let area = ratatui::layout::Rect::new(0, 0, 80, 1);
        let pinned = cmds(&[("PR", "/pr"), ("FB", "/fb"), ("UR", "/ur")]);
        let rects = layout_chip_row(area, &pinned);
        assert_eq!(rects.len(), 3);
        for r in &rects {
            assert!(r.width > 0);
            assert_eq!(r.y, 0);
        }
        // Chips render left-to-right with at least one column of gap.
        assert!(rects[1].x > rects[0].x + rects[0].width);
    }

    #[test]
    fn chip_row_drops_trailing_chips_when_too_narrow() {
        let area = ratatui::layout::Rect::new(0, 0, 12, 1);
        let pinned = cmds(&[("PR", "/pr"), ("FB", "/fb"), ("UR", "/ur")]);
        let rects = layout_chip_row(area, &pinned);
        // Exact count depends on chip widths; at width 12 we expect strictly
        // fewer than 3, with at least 1.
        assert!(!rects.is_empty(), "should render at least one chip");
        assert!(rects.len() < 3, "should drop trailing chips at width 12");
    }

    #[test]
    fn chip_row_empty_list_returns_no_rects() {
        let area = ratatui::layout::Rect::new(0, 0, 80, 1);
        assert!(layout_chip_row(area, &[]).is_empty());
    }
}
```

- [ ] **Step 3: Run to verify failure**

```bash
cargo test --lib ui::attached::tests -- --test-threads=1
```

Expected: FAIL — `layout_chip_row` is not defined.

- [ ] **Step 4: Implement `layout_chip_row` and `render_chip_row`**

In `src/ui/attached.rs`, after the `render` function, add:

```rust
/// Compute the clickable Rect for each chip that fits within `area`.
/// Returns one Rect per chip rendered left-to-right; chips that don't fit
/// are dropped from the end. The full chip text is `[N] <label>` joined by
/// 3-space gaps. Labels are individually truncated to 12 columns first.
pub fn layout_chip_row(
    area: ratatui::layout::Rect,
    pinned: &[crate::pinned::PinnedCommand],
) -> Vec<ratatui::layout::Rect> {
    let mut rects = Vec::new();
    let mut x = area.x;
    let max_x = area.x.saturating_add(area.width);
    const GAP: u16 = 3;
    for (i, cmd) in pinned.iter().enumerate().take(9) {
        let label = crate::pinned::truncate_label(&cmd.label, 12);
        // Chip text: "[N] label"  (1 + 1 + 1 + 1 + label.chars())
        let chip_chars = 4 + label.chars().count() as u16; // "[N] "
        if i > 0 {
            x = x.saturating_add(GAP);
        }
        if x.saturating_add(chip_chars) > max_x {
            break;
        }
        rects.push(ratatui::layout::Rect {
            x,
            y: area.y,
            width: chip_chars,
            height: 1,
        });
        x = x.saturating_add(chip_chars);
    }
    rects
}

fn render_chip_row(
    f: &mut Frame,
    area: ratatui::layout::Rect,
    pinned: &[crate::pinned::PinnedCommand],
    theme: &Theme,
) -> Vec<ratatui::layout::Rect> {
    let rects = layout_chip_row(area, pinned);
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(rects.len() * 3);
    for (i, _r) in rects.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("   "));
        }
        let label = crate::pinned::truncate_label(&pinned[i].label, 12);
        spans.push(Span::styled(format!("[{}]", i + 1), theme.dim_style()));
        spans.push(Span::raw(format!(" {label}")));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
    rects
}
```

- [ ] **Step 5: Wire `render_chip_row` into the main `render` and update the signature**

Modify `pub fn render`:

```rust
pub fn render(
    f: &mut Frame,
    area: Rect,
    session: &Arc<Session>,
    label: &str,
    attention_line: Option<&str>,
    pinned: &[crate::pinned::PinnedCommand],
    theme: &Theme,
) -> Vec<Rect> {
    let chip_height = if pinned.is_empty() { 0 } else { 1 };
    let status_height = if attention_line.is_some() { 1 } else { 0 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(chip_height),
            Constraint::Length(status_height),
            Constraint::Length(1),
        ])
        .split(area);
    let term_area = chunks[0];
    let chip_area = chunks[1];
    let status_area = chunks[2];
    let footer_area = chunks[3];

    // (preserve existing PTY render block here, unchanged — uses term_area)

    // ... existing render_screen / cursor handling ...

    let chip_rects = if chip_height == 1 {
        render_chip_row(f, chip_area, pinned, theme)
    } else {
        Vec::new()
    };

    if let Some(text) = attention_line {
        let line = format!(" ⚠ {text}");
        f.render_widget(Paragraph::new(line).style(theme.warn_style()), status_area);
    }

    let footer = format!(
        " {label}   [Ctrl-x] d=detach u=updates e=edit t=term v=diff k=procs x=send-Ctrl-x "
    );
    f.render_widget(Paragraph::new(footer).style(theme.dim_style()), footer_area);

    chip_rects
}
```

Keep the existing PTY render / cursor block intact between the chunk split and the chip_rects assignment. Only the function signature, the layout-constraints list, and the new `chip_rects` block are net-new.

- [ ] **Step 6: Update every caller of `attached::render`**

Search:

```bash
grep -rn "attached::render\|ui::attached::render\|attached_module::render" src/
```

Each caller must pass two new args (`pinned: &[]` — an intentional empty slice that Task 7 replaces with the resolved list — plus accept the returned `Vec<Rect>`). Expected sites: `src/app.rs` `draw()` for `View::Attached`, possibly `View::AttachedPm`. Update the caller to:

```rust
let chip_rects = attached::render(f, area, &session, &label, attention_line.as_deref(), &[], theme);
```

We'll store `chip_rects` on `App` in Task 7.

- [ ] **Step 7: Run the suite**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1
```

Expected: all green. Chip-layout tests now pass; rendering tests for the existing attached view (if any) still pass because the chip row is zero-height when no commands.

- [ ] **Step 8: Commit**

```bash
git add src/ui/attached.rs src/pinned.rs src/app.rs
git commit -m "feat(ui): chip row layout + render in attached view

render() returns chip Rects so the mouse handler can hit-test.
Zero pinned commands -> zero-height row, same layout as before."
```

---

## Task 7: Wire chip row to `draw()` — read settings, store rects on `App`

**Files:**
- Modify: `src/app.rs` — `App` struct gains `pub chip_rects: Vec<Rect>` plus an init in `App::new`; `draw()` resolves pinned commands per attached view and passes them to render.

- [ ] **Step 1: Add `chip_rects` field to `App`**

Find the `pub struct App` definition. Add (alongside other render-output fields):

```rust
    pub chip_rects: Vec<ratatui::layout::Rect>,
```

Init in `App::new`:

```rust
            chip_rects: Vec::new(),
```

- [ ] **Step 2: Resolve pinned commands and pass them in `draw()`**

Find the `View::Attached(id)` arm inside `fn draw` (search `attached::render`). Before the render call, add:

```rust
            let pinned = {
                let global = self.store.get_setting("pinned_commands").ok().flatten();
                let repo_value = self
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == id)
                    .and_then(|(_, w)| {
                        self.repos
                            .iter()
                            .find(|r| r.id == w.repo_id)
                            .and_then(|r| r.pinned_commands.clone())
                    });
                crate::pinned::resolve(global.as_deref(), repo_value.as_deref())
            };
            let chip_rects = crate::ui::attached::render(
                f,
                attached_area,
                &session,
                &label,
                attention_line.as_deref(),
                &pinned,
                &self.theme,
            );
            self.chip_rects = chip_rects;
            self.pinned_commands_cache = pinned;
```

- [ ] **Step 3: Add `pinned_commands_cache` to `App`**

Same place as `chip_rects`:

```rust
    pub pinned_commands_cache: Vec<crate::pinned::PinnedCommand>,
```

Init in `App::new`:

```rust
            pinned_commands_cache: Vec::new(),
```

This cache lets the key/mouse handlers know which command to fire without re-reading settings.

- [ ] **Step 4: Reset both when leaving the attached view**

In `draw`, in branches other than `View::Attached`, clear:

```rust
            self.chip_rects.clear();
            self.pinned_commands_cache.clear();
```

(Put this at the top of the non-Attached branches or unconditionally before the match — whichever the existing code style favors. Search for an existing reset pattern like the `attention_line` clear.)

- [ ] **Step 5: Run the suite**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1
```

Expected: all green. No new behavior yet — the chips render but nothing fires them.

- [ ] **Step 6: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): resolve + cache pinned commands per draw tick

App now stores the rendered chip Rects and the resolved command
list so key/mouse handlers can dispatch without re-reading settings."
```

---

## Task 8: Keyboard handler — `Ctrl-x <digit>` fires a chip

**Files:**
- Modify: `src/app.rs` `handle_key_attached` — leader-pending branch picks up digits 1-9.

- [ ] **Step 1: Write a failing integration-style test**

In `src/app.rs`'s `#[cfg(test)] mod tests` block, add (alongside other `leader_*` tests):

```rust
    #[tokio::test]
    async fn leader_digit_sends_pinned_command_to_pty() {
        // Spawn an attached session with WSX_CLAUDE_BIN=cat so writes round-trip.
        let mut app = test_app_with_attached_workspace().await;
        let id = app.view_workspace_id().expect("view must be Attached");

        // Populate the cache directly (Task 7's resolution path is tested elsewhere).
        app.pinned_commands_cache = vec![crate::pinned::PinnedCommand {
            label: "PR".into(),
            command: "/pull-request".into(),
        }];

        // Leader, then '1'.
        handle_key_attached(
            &mut app,
            id,
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('x'),
                crossterm::event::KeyModifiers::CONTROL,
            ),
        )
        .await
        .unwrap();
        assert!(app.leader_pending);

        handle_key_attached(
            &mut app,
            id,
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('1'),
                crossterm::event::KeyModifiers::NONE,
            ),
        )
        .await
        .unwrap();
        assert!(!app.leader_pending);

        // Drain the PTY output to confirm cat saw the bytes.
        let stdout = app.drain_attached_stdout_for_test().await;
        assert!(stdout.contains("/pull-request"), "stdout was: {stdout:?}");
    }
```

NOTE: `test_app_with_attached_workspace`, `view_workspace_id`, and `drain_attached_stdout_for_test` may already exist as helpers (the existing `leader_keystroke_does_not_reset_scrollback` test reads PTY output in some form — model the new helper on that). If they don't, add minimal versions in the same `mod tests` block. The goal is: after the keys flow through `handle_key_attached`, the bytes for `/pull-request\r` have reached the cat process backing the PTY.

- [ ] **Step 2: Run to verify failure**

```bash
cargo test --lib app::tests::leader_digit_sends_pinned_command_to_pty -- --test-threads=1
```

Expected: FAIL — the digit branch doesn't exist; `app.leader_pending` clears but no bytes flow.

- [ ] **Step 3: Add the digit arm**

In `src/app.rs`, in `async fn handle_key_attached`, inside the `if app.leader_pending` block's `match k.code`:

```rust
            KeyCode::Char(c @ '1'..='9') => {
                let idx = (c as u8 - b'1') as usize;
                if let Some(cmd) = app.pinned_commands_cache.get(idx) {
                    let mut bytes = cmd.command.as_bytes().to_vec();
                    bytes.push(b'\r');
                    session.scroll_to_live();
                    let _ = session.writer.send(bytes).await;
                }
                return Ok(());
            }
```

Place it next to the other single-char arms (`KeyCode::Char('d')`, etc.) — order doesn't matter functionally; keep it adjacent to `'k'` so the related-shortcuts cluster stays together.

- [ ] **Step 4: Run to verify pass**

```bash
cargo test --lib app::tests::leader_digit_sends_pinned_command_to_pty -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 5: Add an out-of-range test**

Append:

```rust
    #[tokio::test]
    async fn leader_digit_out_of_range_is_noop() {
        let mut app = test_app_with_attached_workspace().await;
        let id = app.view_workspace_id().unwrap();
        app.pinned_commands_cache = vec![crate::pinned::PinnedCommand {
            label: "PR".into(),
            command: "/pull-request".into(),
        }];

        // Leader + '5' — index 4, but the cache only has index 0.
        handle_key_attached(
            &mut app,
            id,
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('x'),
                crossterm::event::KeyModifiers::CONTROL,
            ),
        )
        .await
        .unwrap();
        handle_key_attached(
            &mut app,
            id,
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('5'),
                crossterm::event::KeyModifiers::NONE,
            ),
        )
        .await
        .unwrap();
        assert!(!app.leader_pending);

        let stdout = app.drain_attached_stdout_for_test().await;
        assert!(
            !stdout.contains("/pull-request"),
            "out-of-range digit should not fire any chip; stdout: {stdout:?}"
        );
    }
```

- [ ] **Step 6: Run + fmt + clippy + full suite**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1
```

Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): Ctrl-x <digit> fires the pinned command at that index

Reads from App.pinned_commands_cache (populated each draw tick).
Out-of-range digits clear leader_pending without sending anything."
```

---

## Task 9: Mouse click handler — single-click on a chip rect fires the chip

**Files:**
- Modify: `src/app.rs` `handle_mouse` — extend to async (or use try_send), add hit-test against `app.chip_rects`.

- [ ] **Step 1: Decide async vs try_send**

The existing `handle_mouse` is `fn handle_mouse(app: &App, m: MouseEvent)` — sync. To send bytes to the PTY we need either:
- Make `handle_mouse` async and `.await` `session.writer.send(...)`.
- Use `session.writer.try_send(...)` (which is `Result<(), TrySendError>` — drop on full).

Pick **async**. The call site in `run()` is already inside an async loop. Update the signature to `async fn handle_mouse(app: &mut App, m: MouseEvent)` (also `&mut` since we may want to reset state — actually the body below doesn't mutate `app`, so `&App` is fine; but match the existing `&App` to avoid unrelated churn).

- [ ] **Step 2: Write a failing test**

```rust
    #[tokio::test]
    async fn click_in_chip_rect_fires_pinned_command() {
        let mut app = test_app_with_attached_workspace().await;
        app.pinned_commands_cache = vec![crate::pinned::PinnedCommand {
            label: "PR".into(),
            command: "/pull-request".into(),
        }];
        // Place a 7-wide chip at (5, 30): "[1] PR " = 7 cols.
        app.chip_rects = vec![ratatui::layout::Rect { x: 5, y: 30, width: 7, height: 1 }];

        let click = MouseEvent {
            kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 6,
            row: 30,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        handle_mouse(&app, click).await;

        let stdout = app.drain_attached_stdout_for_test().await;
        assert!(stdout.contains("/pull-request"), "stdout: {stdout:?}");
    }

    #[tokio::test]
    async fn click_outside_chip_rect_does_nothing() {
        let mut app = test_app_with_attached_workspace().await;
        app.pinned_commands_cache = vec![crate::pinned::PinnedCommand {
            label: "PR".into(),
            command: "/pull-request".into(),
        }];
        app.chip_rects = vec![ratatui::layout::Rect { x: 5, y: 30, width: 7, height: 1 }];

        let click = MouseEvent {
            kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 50,
            row: 10,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        handle_mouse(&app, click).await;

        let stdout = app.drain_attached_stdout_for_test().await;
        assert!(!stdout.contains("/pull-request"), "no chip should fire; stdout: {stdout:?}");
    }
```

- [ ] **Step 3: Run to verify failure**

```bash
cargo test --lib app::tests::click_ -- --test-threads=1
```

Expected: FAIL — chip handling absent.

- [ ] **Step 4: Implement the click arm**

In `src/app.rs`, change `fn handle_mouse(app: &App, m: MouseEvent)` to:

```rust
async fn handle_mouse(app: &App, m: MouseEvent) {
    match m.kind {
        MouseEventKind::ScrollUp => scroll_active(app, 3, true),
        MouseEventKind::ScrollDown => scroll_active(app, 3, false),
        MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
            if let Some(idx) = app.chip_rects.iter().position(|r| {
                m.column >= r.x
                    && m.column < r.x.saturating_add(r.width)
                    && m.row >= r.y
                    && m.row < r.y.saturating_add(r.height)
            }) {
                if let Some(cmd) = app.pinned_commands_cache.get(idx) {
                    if let Some(session) = active_session(app) {
                        let mut bytes = cmd.command.as_bytes().to_vec();
                        bytes.push(b'\r');
                        session.scroll_to_live();
                        let _ = session.writer.send(bytes).await;
                    }
                }
            }
        }
        _ => {}
    }
}
```

Update the call site (search `handle_mouse(app, m)`):

```rust
CtEvent::Mouse(m) => handle_mouse(app, m).await,
```

- [ ] **Step 5: Run to verify pass**

```bash
cargo test --lib app::tests::click_ -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 6: fmt + clippy + full suite**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1
```

Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): single-left-click on a chip fires the pinned command

handle_mouse is now async; new Down(Left) arm hit-tests against
App.chip_rects and dispatches the same way as Ctrl-x <digit>."
```

---

## Task 10: README updates

**Files:**
- Modify: `README.md` — add Pinned Commands subsection under "Attached workspace" / "Keybindings" + a new known-key row in the config table + Repo-management subcommands.

- [ ] **Step 1: Add the config-table row**

In `README.md`, find the `## Global settings` `Known keys` table (around line 71). After the last row, add:

```markdown
| `pinned_commands` | Newline-separated list of `Label=command` (or bare `command`) entries. Each becomes a chip in the attached view, fired via `Ctrl-x <digit>` or click. Max 9 visible/keyable. Per-repo override available via `wsx repo set-pinned-commands`. |
```

- [ ] **Step 2: Add the per-repo CLI subsection**

Find the `wsx repo set-instructions` block (around line 54). After it, add:

```markdown
```
wsx repo set-pinned-commands <name> <value-or-@file>
wsx repo edit-pinned-commands <name>
```

Per-repo override of `pinned_commands`. Empty value clears the override; resolution then falls back to the global setting.
```

- [ ] **Step 3: Add a Pinned Commands section under "Attached workspace"**

Find the `### Attached workspace` heading (around line 126). After the existing table and before `#### Mouse, scrollback, and text selection`, add:

```markdown
#### Pinned commands

If `pinned_commands` is configured (globally or per-repo), a one-row chip strip appears between the claude pane and the footer. Each chip shows `[N] Label`:

```
[1] PR   [2] FB   [3] /loop /baby…   [4] UR
```

Fire a chip with `Ctrl-x <digit>` (1-9) or by clicking on it. The chip's command + `\r` is written to claude exactly as if you'd typed and submitted it.

Configure with one entry per line:

```
PR=/pull-request
FB=/feedback
/loop /babysit-prs
UR=/ultrareview
```

`Label=command` shows the label as the chip; a bare line uses the command itself (truncated past 12 columns). Both sides of `=` are trimmed.

At narrow terminal widths trailing chips drop from view; their keyboard shortcuts still work.
```

- [ ] **Step 4: Add to the Key features section**

Find the `## Key features` bullets near the top of the README. Add a new bullet:

```markdown
- **Pinned commands** — define your `/pull-request`, `/feedback`, `/ultrareview` shortcuts once; fire them with `Ctrl-x <digit>` or a click while attached.
```

- [ ] **Step 5: Commit**

```bash
git add README.md
git commit -m "docs(readme): pinned commands feature

Adds the pinned_commands key to the settings table, the per-repo
subcommands, and a usage subsection in the Attached workspace
section. Also surfaces it in the Key features list."
```

---

## Task 11: File the GitHub issue + final verification

- [ ] **Step 1: Open the tracking issue**

```bash
gh issue create --title "feat: pinned slash commands in the attached view" \
  --body "$(cat <<'EOF'
Adds a user-defined chip row to the attached workspace view that lets
the user fire pinned slash commands at the claude session via
\`Ctrl-x <digit>\` or mouse click.

Spec: docs/superpowers/specs/2026-05-17-pinned-commands-design.md
Plan: docs/superpowers/plans/2026-05-17-pinned-commands.md
EOF
)"
```

Note the issue number returned.

- [ ] **Step 2: Update the spec doc's `Issue: TBD` line**

Open `docs/superpowers/specs/2026-05-17-pinned-commands-design.md` and replace:

```markdown
**Issue:** TBD (file before implementation lands).
```

with the actual issue link, e.g.:

```markdown
**Issue:** [#NN](https://github.com/bakedbean/workspacex/issues/NN)
```

Commit:

```bash
git add docs/superpowers/specs/2026-05-17-pinned-commands-design.md
git commit -m "docs(spec): link pinned-commands issue"
```

- [ ] **Step 3: Final verification**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1
```

All three expected to be clean.

- [ ] **Step 4: Manual smoke test**

```bash
cargo build --release
./target/release/wsx config set pinned_commands "PR=/pull-request
FB=/feedback
UR=/ultrareview"
./target/release/wsx
```

In the TUI:
- Attach to any workspace.
- Verify the chip row renders between the claude pane and the footer.
- Press `Ctrl-x 1` — confirm `/pull-request\n` appears in the claude pane.
- Click `[2] FB` — confirm `/feedback\n` appears.
- Press `Ctrl-x d` to detach; confirm chip row disappears from the dashboard view.
- Resize the terminal narrow — confirm trailing chips drop visually but `Ctrl-x N` for dropped chips still fires.

- [ ] **Step 5: Open the PR**

Use the `pull-request` skill or:

```bash
git push -u origin HEAD
gh pr create --title "feat: pinned slash commands in the attached view" \
  --body "<see PR skill template>"
```

Reference: `Closes #NN` where NN is the issue from Step 1.

---

## Notes for the executor

- **Each task is meant to leave the tree green.** If a task's tests fail at the boundary between steps, that's normal — the TDD cycle is "red → green → commit". The final step of each task always runs the *full* suite to confirm no collateral damage.
- **If `gh` isn't available**, skip Task 11 Step 1 and leave `Issue: TBD` until the user files the issue manually.
- **Helper functions in tests** (`test_app_with_attached_workspace`, `drain_attached_stdout_for_test`) may need to be created from scratch. Look at the existing `leader_keystroke_does_not_reset_scrollback` test (`src/app.rs` around line 2646) for the pattern of spawning an attached session against `WSX_CLAUDE_BIN=cat`.
- **`session.writer` type:** `tokio::sync::mpsc::Sender<Vec<u8>>`. Awaiting `.send(bytes)` is the canonical path; `try_send` is a fallback only if we ever revert `handle_mouse` to sync.
- **If a step's expected output differs from what you see**, stop and investigate before continuing. Likely causes: an existing field/function changed name (the codebase moves fast), or a test helper that the plan assumed exists actually doesn't.
