# Related repos Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let each wsx repo declare a list of related repos. At spawn time, wsx passes `--add-dir <source-path>` to claude for each related repo AND appends a system-prompt fragment instructing claude to treat those dirs as read-only (and to ask the user to create a new wsx workspace if it needs to edit there).

**Architecture:** New per-repo column `repos.related_repos TEXT NULL` (Schema v6) holds a comma-separated list of wsx repo names. A new `src/related.rs` module owns the parser, name→path resolver, and system-prompt builder. The resolved `Vec<(String, PathBuf)>` is folded into `SpawnMode` (alongside `custom_instructions`) so it flows naturally through the existing spawn pipeline; `build_claude_command` emits `--add-dir <path>` per entry and the system-prompt fragment is concatenated into the `--append-system-prompt` value.

**Tech Stack:** Rust 2024, `rusqlite` 0.32 for storage, `portable-pty` for spawn, `tracing` for warn-on-unknown-name logs.

**Spec:** `docs/superpowers/specs/2026-05-18-related-repos-design.md`

---

## Task 1: Parser, resolver, and prompt builder

**Files:**
- Create: `src/related.rs`
- Modify: `src/lib.rs` (add `pub mod related;` between `pub mod pty;` and `pub mod remote;` to maintain alphabetical order (pty < related < remote))

- [ ] **Step 1: Add the module to `src/lib.rs`**

Open `src/lib.rs` and insert `pub mod related;` between `pub mod pty;` and `pub mod remote;` to maintain alphabetical order (pty < related < remote).

- [ ] **Step 2: Write failing tests in `src/related.rs`**

Create `src/related.rs`:

```rust
//! Related repos: parser, resolver, and read-only system-prompt builder
//! for the per-repo `related_repos` config.

use crate::store::Repo;
use std::path::PathBuf;

/// Parse a `related_repos` config value into trimmed, non-empty name strings.
/// Comma-separated; whitespace around commas trimmed; blank entries dropped.
pub fn parse(_spec: &str) -> Vec<String> {
    todo!()
}

/// Resolve each name in `spec` to its (name, source_path) by looking up
/// `all_repos`. Names with no matching repo are tracing::warn!'d and dropped.
/// Returns entries in input order.
pub fn resolve(_spec: Option<&str>, _all_repos: &[Repo]) -> Vec<(String, PathBuf)> {
    todo!()
}

/// Build the read-only system-prompt fragment claude sees when related
/// repos are present. Returns None when `resolved` is empty.
pub fn build_read_only_prompt(_resolved: &[(String, PathBuf)]) -> Option<String> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Repo, RepoId};

    fn repo(id: i64, name: &str, path: &str) -> Repo {
        Repo {
            id: RepoId(id),
            name: name.into(),
            path: PathBuf::from(path),
            branch_prefix: String::new(),
            custom_instructions: None,
            setup_script: None,
            archive_script: None,
            pinned_commands: None,
            related_repos: None,
            created_at: 0,
        }
    }

    #[test]
    fn parse_single_name() {
        assert_eq!(parse("frontend"), vec!["frontend".to_string()]);
    }

    #[test]
    fn parse_comma_separated_with_whitespace() {
        assert_eq!(
            parse(" frontend , marketing,backend "),
            vec!["frontend".to_string(), "marketing".to_string(), "backend".to_string()]
        );
    }

    #[test]
    fn parse_skips_blank_entries() {
        assert_eq!(
            parse("frontend,,marketing,"),
            vec!["frontend".to_string(), "marketing".to_string()]
        );
    }

    #[test]
    fn parse_empty_string_returns_empty() {
        assert!(parse("").is_empty());
        assert!(parse("   ").is_empty());
        assert!(parse(",,, ,").is_empty());
    }

    #[test]
    fn resolve_returns_matching_repos_in_input_order() {
        let repos = vec![
            repo(1, "frontend", "/work/frontend"),
            repo(2, "backend", "/work/backend"),
            repo(3, "marketing", "/work/marketing"),
        ];
        let out = resolve(Some("marketing, frontend"), &repos);
        assert_eq!(
            out,
            vec![
                ("marketing".to_string(), PathBuf::from("/work/marketing")),
                ("frontend".to_string(), PathBuf::from("/work/frontend")),
            ]
        );
    }

    #[test]
    fn resolve_drops_unknown_names() {
        let repos = vec![repo(1, "frontend", "/work/frontend")];
        let out = resolve(Some("frontend, ghost"), &repos);
        assert_eq!(out, vec![("frontend".to_string(), PathBuf::from("/work/frontend"))]);
    }

    #[test]
    fn resolve_none_returns_empty() {
        let repos = vec![repo(1, "frontend", "/work/frontend")];
        assert!(resolve(None, &repos).is_empty());
    }

    #[test]
    fn resolve_empty_spec_returns_empty() {
        let repos = vec![repo(1, "frontend", "/work/frontend")];
        assert!(resolve(Some(""), &repos).is_empty());
        assert!(resolve(Some("   "), &repos).is_empty());
    }

    #[test]
    fn build_read_only_prompt_empty_returns_none() {
        assert!(build_read_only_prompt(&[]).is_none());
    }

    #[test]
    fn build_read_only_prompt_single_entry_lists_it() {
        let r = vec![("frontend".to_string(), PathBuf::from("/work/frontend"))];
        let out = build_read_only_prompt(&r).unwrap();
        assert!(out.contains("/work/frontend"), "prompt missing path: {out}");
        assert!(out.contains("wsx repo: frontend"), "prompt missing label: {out}");
        assert!(
            out.contains("MUST NOT edit"),
            "prompt missing read-only directive: {out}"
        );
    }

    #[test]
    fn build_read_only_prompt_multiple_entries_lists_all() {
        let r = vec![
            ("frontend".to_string(), PathBuf::from("/work/frontend")),
            ("marketing".to_string(), PathBuf::from("/work/marketing")),
        ];
        let out = build_read_only_prompt(&r).unwrap();
        assert!(out.contains("/work/frontend"));
        assert!(out.contains("/work/marketing"));
        assert!(out.contains("wsx repo: frontend"));
        assert!(out.contains("wsx repo: marketing"));
    }
}
```

- [ ] **Step 3: Run to verify failure**

```bash
cargo test --lib related:: -- --test-threads=1
```

Expected: tests fail with `not yet implemented` from the `todo!()`s. The compile may also fail because `Repo.related_repos` doesn't exist yet — that's OK; the test helper `fn repo` includes it forward-looking. If the compile fails, comment out the `related_repos: None,` line in the test helper temporarily and add it back in Step 5.

- [ ] **Step 4: Implement `parse`**

Replace the `parse` stub with:

```rust
pub fn parse(spec: &str) -> Vec<String> {
    spec.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}
```

- [ ] **Step 5: Confirm test file is forward-compatible with Task 2**

If you commented out `related_repos: None,` in the test helper, restore it now — Task 2 will add the field. If it's still there, leave it; the compile will succeed after Task 2 lands.

For now, to test parse + the rest before Task 2, you can temporarily comment out the test-helper field. **Alternatively**, defer the test runs to Task 2 — implement `parse`, `resolve`, and `build_read_only_prompt`, commit, then come back after Task 2 lands. Pick whichever keeps the build green; both work. The plan assumes you leave it intact and let the compile fail until Task 2.

- [ ] **Step 6: Implement `resolve`**

Replace the `resolve` stub:

```rust
pub fn resolve(spec: Option<&str>, all_repos: &[Repo]) -> Vec<(String, PathBuf)> {
    let Some(s) = spec else { return Vec::new() };
    let names = parse(s);
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        match all_repos.iter().find(|r| r.name == name) {
            Some(r) => out.push((name, r.path.clone())),
            None => tracing::warn!(name = %name, "related_repos: unknown repo name; skipping"),
        }
    }
    out
}
```

- [ ] **Step 7: Implement `build_read_only_prompt`**

Replace the `build_read_only_prompt` stub:

```rust
pub fn build_read_only_prompt(resolved: &[(String, PathBuf)]) -> Option<String> {
    if resolved.is_empty() {
        return None;
    }
    let mut listing = String::new();
    for (name, path) in resolved {
        listing.push_str(&format!("  - {} (wsx repo: {})\n", path.display(), name));
    }
    Some(format!(
        "The following directories were added via --add-dir for read-only \
         reference. They are the source paths of related wsx-managed repos:\n\
         {listing}\n\
         You MUST NOT edit files in these directories. They may be on \
         different branches, have unstaged changes, or belong to other \
         active work. If you need to make changes in a related repo, tell \
         the user to create a new wsx workspace for it (via the wsx \
         dashboard's [n] keybind, or `wsx workspace create <repo>`) and \
         switch to that session — then come back here when done.\n\n\
         Read, grep, reference, and quote freely from these paths. Just \
         don't write to them.\n"
    ))
}
```

- [ ] **Step 8: Commit (defer test verification until Task 2)**

```bash
git add src/lib.rs src/related.rs
git commit -m "feat(related): parser + resolver + prompt builder

src/related.rs holds the per-repo related_repos parser, name->path
resolver against the Repo registry, and the read-only system-prompt
fragment claude sees when related dirs are present. Tests reference
Repo.related_repos field added in next commit; compile will be green
again after the schema change lands.

Part of related-repos feature (docs/superpowers/specs/2026-05-18-related-repos-design.md)."
```

---

## Task 2: DB column + Repo struct + queries

**Files:**
- Modify: `src/store.rs` — `Repo` struct, `repos()` SELECT/mapper, `migrate()`, new `set_repo_related_repos` setter, plus existing test sites that construct `Repo` literals.

- [ ] **Step 1: Add migration for the new column**

In `src/store.rs::migrate()`, after the existing `if v < 5 { ... }` block (added by the pinned-commands work), append:

```rust
        if v < 6 {
            let has_col: i64 = self.conn.query_row(
                "SELECT count(*) FROM pragma_table_info('repos') WHERE name = 'related_repos'",
                [],
                |r| r.get(0),
            )?;
            if has_col == 0 {
                self.conn
                    .execute("ALTER TABLE repos ADD COLUMN related_repos TEXT", [])?;
            }
            self.conn.execute("PRAGMA user_version = 6", [])?;
        }
```

- [ ] **Step 2: Add `related_repos` to the `Repo` struct**

In `src/store.rs`, find the `pub struct Repo` definition. Add a new field between `pinned_commands` and `created_at`:

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
    pub related_repos: Option<String>,
    pub created_at: i64,
}
```

- [ ] **Step 3: Update the `repos()` SELECT to include the new column**

Find `pub fn repos(&self)` and update:

```rust
    pub fn repos(&self) -> Result<Vec<Repo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, path, branch_prefix, custom_instructions, \
                    setup_script, archive_script, pinned_commands, \
                    related_repos, created_at \
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
                related_repos: r.get(8)?,
                created_at: r.get(9)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
```

- [ ] **Step 4: Add `set_repo_related_repos` setter**

After `set_repo_pinned_commands`, add:

```rust
    pub fn set_repo_related_repos(&self, id: RepoId, value: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE repos SET related_repos = ?1 WHERE id = ?2",
            rusqlite::params![value, id.0],
        )?;
        Ok(())
    }
```

- [ ] **Step 5: Fix existing `Repo` literal construction sites**

```bash
grep -rn "Repo {" src/ tests/
```

Every match needs `related_repos: None,` added before `created_at:`. Expected sites (from the pinned-commands pattern): `src/store.rs` tests, `src/repo.rs`, `src/ui/dashboard/tests.rs`, and Task 1's `src/related.rs` test helper (already includes it).

Open each file and add the field by hand — don't use `replace_all` because the surrounding context varies.

- [ ] **Step 6: Write a test for the setter**

In `src/store.rs`'s `#[cfg(test)] mod tests` block (alongside `set_repo_pinned_commands_round_trips`):

```rust
    #[test]
    fn set_repo_related_repos_round_trips() {
        let store = Store::open_in_memory().unwrap();
        let id = store
            .add_repo(Path::new("/x"), "demo", "")
            .unwrap();
        store
            .set_repo_related_repos(id, Some("frontend, marketing"))
            .unwrap();
        let repo = store.repos().unwrap().into_iter().find(|r| r.id == id).unwrap();
        assert_eq!(repo.related_repos.as_deref(), Some("frontend, marketing"));

        store.set_repo_related_repos(id, None).unwrap();
        let repo = store.repos().unwrap().into_iter().find(|r| r.id == id).unwrap();
        assert!(repo.related_repos.is_none());
    }
```

- [ ] **Step 7: Run the suite**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1
```

Expected: all green. Task 1's `related::tests` should now compile and pass. Test count up by 11 from before Task 1 (8 parse/resolve/prompt + 3 truncate-like + 1 round-trip) — actual delta depends on parser test breakdown; just ensure no regressions.

- [ ] **Step 8: Commit**

```bash
git add src/store.rs src/repo.rs src/ui/dashboard/tests.rs <any other touched files>
git commit -m "feat(store): related_repos column + setter

Schema v6: ALTER TABLE repos ADD COLUMN related_repos TEXT. NULL
defaults; per-repo only (no global override — relatedness is repo-
specific by nature).

Part of related-repos feature."
```

---

## Task 3: CLI — `repo set-related-repos` and `edit-related-repos`

**Files:**
- Modify: `src/cli.rs` — new `CliAction` variants, `parse_args` arms, dispatcher arms, tests.

- [ ] **Step 1: Add `CliAction` variants**

In `src/cli.rs`, find `pub enum CliAction`. Alongside `RepoSetPinnedCommands` / `RepoEditPinnedCommands`, add:

```rust
    RepoSetRelatedRepos {
        name: String,
        source: ValueSource,
    },
    RepoEditRelatedRepos {
        name: String,
    },
```

- [ ] **Step 2: Add `parse_args` arms**

Find the `Some("repo") => match it.next().as_deref()` block. After the `edit-pinned-commands` arm (added by the pinned-commands work), add:

```rust
            Some("set-related-repos") => {
                let name = it.next().ok_or_else(|| {
                    Error::UserInput("repo set-related-repos <name> <value-or-@file>".into())
                })?;
                let value = it.next().ok_or_else(|| {
                    Error::UserInput("repo set-related-repos <name> <value-or-@file>".into())
                })?;
                Ok(CliAction::RepoSetRelatedRepos {
                    name,
                    source: ValueSource::from_arg(value),
                })
            }
            Some("edit-related-repos") => {
                let name = it
                    .next()
                    .ok_or_else(|| Error::UserInput("repo edit-related-repos <name>".into()))?;
                Ok(CliAction::RepoEditRelatedRepos { name })
            }
```

- [ ] **Step 3: Add dispatcher arms**

Find the dispatcher function that consumes `CliAction` (search for `CliAction::RepoSetPinnedCommands` for the matching arm to copy from). After it, add:

```rust
        CliAction::RepoSetRelatedRepos { name, source } => {
            let store = Store::open(&db_path()?)?;
            let repo = store
                .repos()?
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no such repo: {name}")))?;
            let value = source.read()?;
            let opt = if value.trim().is_empty() { None } else { Some(value.as_str()) };
            store.set_repo_related_repos(repo.id, opt)?;
            Ok(())
        }
        CliAction::RepoEditRelatedRepos { name } => {
            let store = Store::open(&db_path()?)?;
            let repo = store
                .repos()?
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no such repo: {name}")))?;
            let current = repo.related_repos.clone().unwrap_or_default();
            let edited = edit_in_editor(&current, "related")?;
            let opt = if edited.trim().is_empty() { None } else { Some(edited.as_str()) };
            store.set_repo_related_repos(repo.id, opt)?;
            Ok(())
        }
```

(If the pinned-commands dispatcher uses slightly different shape — e.g. an "unchanged" short-circuit — match that exact shape. The snippets above show the minimal version.)

- [ ] **Step 4: Add CLI parse tests**

In the `#[cfg(test)] mod tests` block (alongside `parse_repo_set_pinned_commands_literal`):

```rust
    #[test]
    fn parse_repo_set_related_repos_literal() {
        let a = parse(&[
            "repo",
            "set-related-repos",
            "backend",
            "frontend,marketing",
        ])
        .unwrap();
        match a {
            CliAction::RepoSetRelatedRepos { name, source } => {
                assert_eq!(name, "backend");
                assert!(matches!(source, ValueSource::Literal(ref s) if s == "frontend,marketing"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_repo_set_related_repos_at_file() {
        let a = parse(&[
            "repo",
            "set-related-repos",
            "backend",
            "@./related.txt",
        ])
        .unwrap();
        match a {
            CliAction::RepoSetRelatedRepos { source, .. } => {
                assert!(matches!(source, ValueSource::File(_)));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_repo_edit_related_repos() {
        match parse(&["repo", "edit-related-repos", "backend"]).unwrap() {
            CliAction::RepoEditRelatedRepos { name } => assert_eq!(name, "backend"),
            other => panic!("unexpected: {other:?}"),
        }
    }
```

- [ ] **Step 5: Run**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test --lib cli:: -- --test-threads=1
```

Expected: all CLI tests pass. Test count up by 3.

- [ ] **Step 6: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): repo set-related-repos / edit-related-repos

Mirrors set-pinned-commands. Empty value clears; resolution drops
unknown names with a tracing::warn! at spawn time.

Part of related-repos feature."
```

---

## Task 4: Repo settings modal — add `related_repos` as the 6th field

**Files:**
- Modify: `src/app.rs` — `RepoSettingField` enum + `ALL` + `label()` + `do_pending_edit` + `apply_repo_setting` + modal index clamp.
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
    RelatedRepos,
}

impl RepoSettingField {
    pub const ALL: [Self; 6] = [
        Self::BranchPrefix,
        Self::CustomInstructions,
        Self::SetupScript,
        Self::ArchiveScript,
        Self::PinnedCommands,
        Self::RelatedRepos,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::BranchPrefix => "branch_prefix",
            Self::CustomInstructions => "custom_instructions",
            Self::SetupScript => "setup_script",
            Self::ArchiveScript => "archive_script",
            Self::PinnedCommands => "pinned_commands",
            Self::RelatedRepos => "related_repos",
        }
    }
}
```

- [ ] **Step 2: Add `do_pending_edit` arm**

Find the match block in `do_pending_edit` that maps `field` → `(value, extension)`. Add:

```rust
            RepoSettingField::RelatedRepos => {
                (repo.related_repos.clone().unwrap_or_default(), "txt")
            }
```

- [ ] **Step 3: Add `apply_repo_setting` arm**

Find `fn apply_repo_setting`. Add:

```rust
        RepoSettingField::RelatedRepos => {
            app.store.set_repo_related_repos(repo_id, opt)
        }
```

- [ ] **Step 4: Bump the modal selection clamp**

```bash
grep -n "RepoSettingField::ALL\[selected\.min" src/app.rs
```

Two sites (Enter and `d` key paths) should currently be `.min(4)` from the pinned-commands work. Update both to `.min(5)`:

```rust
let field = RepoSettingField::ALL[selected.min(5)];
```

- [ ] **Step 5: Update `render_repo_settings` rows array**

In `src/ui/modal.rs`, find the rows array (currently `; 5]` after the pinned-commands work). Change to `; 6]` and append:

```rust
        (
            crate::app::RepoSettingField::RelatedRepos,
            repo.related_repos.as_deref(),
        ),
```

- [ ] **Step 6: Run the suite**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1
```

Expected: all green. If a match becomes non-exhaustive anywhere, add the `RelatedRepos` arm.

- [ ] **Step 7: Commit**

```bash
git add src/app.rs src/ui/modal.rs
git commit -m "feat(ui): related_repos field in repo settings modal

Adds RelatedRepos to RepoSettingField as the 6th editable field.
Modal lists it alongside the others with the same edit-in-\$EDITOR
flow.

Part of related-repos feature."
```

---

## Task 5: SpawnMode carries `additional_dirs` + `build_claude_command` emits `--add-dir`

**Files:**
- Modify: `src/pty/session.rs` — `SpawnMode` variants gain `additional_dirs: Vec<PathBuf>`; `build_claude_command` emits `--add-dir <path>` per entry. Existing tests construct `SpawnMode` literals; each needs `additional_dirs: vec![]`.

- [ ] **Step 1: Write the failing test**

In `src/pty/session.rs`'s `#[cfg(test)] mod tests` block, add (near other `build_claude_command` tests):

```rust
    #[test]
    fn build_claude_command_emits_add_dir_per_related_path() {
        let cwd = PathBuf::from("/tmp/test");
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            additional_dirs: vec![
                PathBuf::from("/work/frontend"),
                PathBuf::from("/work/marketing"),
            ],
            yolo: false,
        };
        let cmd = build_claude_command(&cwd, &mode, crate::remote::RemoteOpts::disabled());
        let args: Vec<String> = cmd
            .get_argv()
            .iter()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        // Two pairs of (--add-dir, <path>) in order.
        let positions: Vec<usize> = args
            .iter()
            .enumerate()
            .filter(|(_, a)| *a == "--add-dir")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(positions.len(), 2, "expected two --add-dir flags; got: {args:?}");
        assert_eq!(args[positions[0] + 1], "/work/frontend");
        assert_eq!(args[positions[1] + 1], "/work/marketing");
    }

    #[test]
    fn build_claude_command_omits_add_dir_when_no_related() {
        let cwd = PathBuf::from("/tmp/test");
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let cmd = build_claude_command(&cwd, &mode, crate::remote::RemoteOpts::disabled());
        let args: Vec<String> = cmd
            .get_argv()
            .iter()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        assert!(!args.iter().any(|a| a == "--add-dir"), "got: {args:?}");
    }
```

If `CommandBuilder::get_argv` doesn't exist on the portable-pty version in use, use whatever public accessor returns the argv vector — search the crate's docs or `cmd.` autocomplete in your editor. (`portable-pty 0.9` exposes the argv via the public `CommandBuilder` API; if the method name differs, adapt.) If no accessor exists, fall back to asserting on the rendered `format!("{cmd:?}")` debug string.

- [ ] **Step 2: Add the field to all three `SpawnMode` variants**

Find `pub enum SpawnMode` in `src/pty/session.rs`. Update each variant to include `additional_dirs: Vec<std::path::PathBuf>`:

```rust
pub enum SpawnMode {
    Continue {
        custom_instructions: Option<String>,
        additional_dirs: Vec<std::path::PathBuf>,
        yolo: bool,
    },
    Fresh {
        rename_ctx: Option<RenameContext>,
        custom_instructions: Option<String>,
        additional_dirs: Vec<std::path::PathBuf>,
        yolo: bool,
    },
    ProjectManager {
        workspaces_json_path: std::path::PathBuf,
        custom_instructions: Option<String>,
        // PM has no owning repo, so always empty. Kept for uniformity.
        additional_dirs: Vec<std::path::PathBuf>,
        resume: bool,
    },
}
```

- [ ] **Step 3: Emit `--add-dir` flags in `build_claude_command`**

In `build_claude_command`, extract the `additional_dirs` from the mode and emit `--add-dir` args. Update the destructuring match to capture them:

```rust
    let (rename_prompt, custom, allow_git_branch, add_continue, skip_permissions, add_dirs) =
        match mode {
            SpawnMode::Continue {
                custom_instructions,
                additional_dirs,
                yolo,
            } => (
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
                additional_dirs,
                yolo,
            } => {
                let rename_mode = std::env::var("WSX_RENAME_MODE")
                    .unwrap_or_else(|_| "claude".to_string());
                let (rp, allow) = if let Some(ctx) = rename_ctx {
                    if rename_mode == "claude" {
                        (
                            Some(render_rename_system_prompt(
                                &ctx.current_branch,
                                &ctx.branch_prefix,
                            )),
                            true,
                        )
                    } else {
                        (None, false)
                    }
                } else {
                    (None, false)
                };
                (
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
            } => (
                Some(crate::pm::pm_system_prompt(custom_instructions.as_deref())),
                None,
                false,
                *resume,
                true,
                additional_dirs.clone(),
            ),
        };
```

Then, right before the existing `if add_continue { cmd.arg("--continue"); }`, add:

```rust
    for dir in &add_dirs {
        cmd.arg("--add-dir");
        cmd.arg(dir);
    }
```

- [ ] **Step 4: Add `additional_dirs: vec![]` to every existing `SpawnMode` literal**

```bash
grep -rn "SpawnMode::Continue\|SpawnMode::Fresh\|SpawnMode::ProjectManager" src/ tests/
```

Each match site (production code in `src/app.rs`, `src/pm.rs`, etc.; and test fixtures in `src/pty/session.rs` and elsewhere) needs `additional_dirs: vec![]` added to the literal. Expected sites: ~15-20. The compiler will catch missed sites.

- [ ] **Step 5: Run**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test --lib pty::session::tests::build_claude_command -- --test-threads=1
cargo test -- --test-threads=1
```

Expected: all green. The two new build_claude_command tests pass; existing tests still work.

- [ ] **Step 6: Commit**

```bash
git add src/pty/session.rs src/app.rs src/pm.rs <other touched files>
git commit -m "feat(pty): SpawnMode.additional_dirs + --add-dir emission

SpawnMode now carries the resolved per-spawn list of related-repo
source paths. build_claude_command emits --add-dir <path> per entry.
Empty Vec (the common case today) leaves the invocation unchanged.

Part of related-repos feature."
```

---

## Task 6: Resolve related repos in `build_spawn_info`; fold read-only fragment into custom_instructions

**Files:**
- Modify: `src/app.rs` — `build_spawn_info` resolves the workspace's repo's related_repos, filters self-references, builds the read-only fragment, folds it into custom_instructions, and populates `SpawnMode.additional_dirs`.

- [ ] **Step 1: Write the failing test**

In `src/app.rs`'s `#[cfg(test)] mod pm_state_tests` (or `mod tests`, whichever has store fixtures), add:

```rust
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn build_spawn_info_resolves_related_repos_to_additional_dirs() {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        let store = Store::open_in_memory().unwrap();
        let backend_id = store
            .add_repo(std::path::Path::new("/work/backend"), "backend", "")
            .unwrap();
        let _frontend_id = store
            .add_repo(std::path::Path::new("/work/frontend"), "frontend", "")
            .unwrap();
        store
            .set_repo_related_repos(backend_id, Some("frontend"))
            .unwrap();
        let ws_id = store
            .insert_workspace(&NewWorkspace {
                repo_id: backend_id,
                name: "test-ws",
                branch: "backend/test-ws",
                worktree_path: std::path::Path::new("/wt/test-ws"),
                yolo: false,
            })
            .unwrap();
        store.set_workspace_state(ws_id, WorkspaceState::Ready).unwrap();

        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let info = build_spawn_info(&app, ws_id);
        assert!(info.is_some());
        let (_id, _path, mode, _repo_path) = info.unwrap();
        match mode {
            crate::pty::session::SpawnMode::Fresh {
                additional_dirs,
                custom_instructions,
                ..
            } => {
                assert_eq!(
                    additional_dirs,
                    vec![std::path::PathBuf::from("/work/frontend")],
                    "additional_dirs should resolve to frontend's source path"
                );
                let prompt = custom_instructions.expect("read-only fragment must be folded in");
                assert!(
                    prompt.contains("/work/frontend"),
                    "system prompt missing related path: {prompt}"
                );
                assert!(
                    prompt.contains("MUST NOT edit"),
                    "system prompt missing read-only directive: {prompt}"
                );
            }
            other => panic!("expected Fresh mode; got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn build_spawn_info_filters_self_reference() {
        // Repo references itself in related_repos — should be filtered out.
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        let store = Store::open_in_memory().unwrap();
        let backend_id = store
            .add_repo(std::path::Path::new("/work/backend"), "backend", "")
            .unwrap();
        store
            .set_repo_related_repos(backend_id, Some("backend"))
            .unwrap();
        let ws_id = store
            .insert_workspace(&NewWorkspace {
                repo_id: backend_id,
                name: "test-ws",
                branch: "backend/test-ws",
                worktree_path: std::path::Path::new("/wt/test-ws"),
                yolo: false,
            })
            .unwrap();
        store.set_workspace_state(ws_id, WorkspaceState::Ready).unwrap();

        let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let (_id, _path, mode, _repo_path) = build_spawn_info(&app, ws_id).unwrap();
        match mode {
            crate::pty::session::SpawnMode::Fresh {
                additional_dirs,
                custom_instructions,
                ..
            } => {
                assert!(additional_dirs.is_empty(), "self-reference must be filtered");
                assert!(
                    custom_instructions.is_none(),
                    "no related dirs => no fragment"
                );
            }
            other => panic!("expected Fresh mode; got {other:?}"),
        }
    }
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test --lib "pm_state_tests::build_spawn_info_resolves_related" "pm_state_tests::build_spawn_info_filters_self_reference" -- --test-threads=1
```

Expected: FAIL — `build_spawn_info` doesn't resolve related repos yet, so `additional_dirs` is empty.

- [ ] **Step 3: Extend `build_spawn_info` to resolve + fold**

In `src/app.rs`, find `fn build_spawn_info` (around line 1231). Update the body. The current code computes `custom` from `resolve_custom_instructions`. Just before constructing the `SpawnMode`, add:

```rust
    // Resolve related repos (per-repo names → source paths), filter out
    // the spawning repo itself, build the read-only system-prompt
    // fragment, and fold it into custom_instructions before claude sees it.
    let resolved = crate::related::resolve(repo.related_repos.as_deref(), &app.repos);
    // Filter self-references: dropping anything whose path equals the
    // spawning repo's own source path.
    let resolved: Vec<(String, std::path::PathBuf)> = resolved
        .into_iter()
        .filter(|(_, p)| p != &repo.path)
        .collect();
    let additional_dirs: Vec<std::path::PathBuf> =
        resolved.iter().map(|(_, p)| p.clone()).collect();
    let related_prompt = crate::related::build_read_only_prompt(&resolved);
    let custom = match (custom, related_prompt) {
        (None, None) => None,
        (Some(c), None) => Some(c),
        (None, Some(r)) => Some(r),
        (Some(c), Some(r)) => Some(format!("{c}\n\n{r}")),
    };
```

Then in each `SpawnMode::Continue { ... }` and `SpawnMode::Fresh { ... }` literal in this function, add `additional_dirs: additional_dirs.clone(),`. Or, since `additional_dirs` is consumed only inside this function and the literals are built mutually exclusively, use one without `.clone()`:

```rust
    let mode = if crate::pty::session::has_prior_session(&ws.worktree_path) {
        crate::pty::session::SpawnMode::Continue {
            custom_instructions: custom,
            additional_dirs,
            yolo,
        }
    } else {
        let rename_ctx = if crate::names::is_generated_slug(&ws.name) {
            let resolved_prefix =
                crate::repo::resolve_branch_prefix(repo, &app.store).unwrap_or_default();
            Some(crate::pty::session::RenameContext {
                current_branch: ws.branch.clone(),
                branch_prefix: resolved_prefix,
            })
        } else {
            None
        };
        crate::pty::session::SpawnMode::Fresh {
            rename_ctx,
            custom_instructions: custom,
            additional_dirs,
            yolo,
        }
    };
```

- [ ] **Step 4: Run to verify pass**

```bash
cargo test --lib "pm_state_tests::build_spawn_info_resolves_related" "pm_state_tests::build_spawn_info_filters_self_reference" -- --test-threads=1
```

Expected: both pass.

- [ ] **Step 5: Run the full suite**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1
```

Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): resolve related_repos at spawn time

build_spawn_info resolves the workspace's repo's related_repos via
related::resolve, filters self-references, populates
SpawnMode.additional_dirs, and folds the read-only system-prompt
fragment into the SpawnMode's custom_instructions. claude sees one
combined --append-system-prompt with both the user's instructions
and the read-only guard.

Part of related-repos feature."
```

---

## Task 7: README updates

**Files:**
- Modify: `README.md` — per-repo CLI block + short subsection explaining the feature.

- [ ] **Step 1: Add the per-repo CLI block**

In `README.md`, find the existing `wsx repo set-pinned-commands` block. After its closing paragraph, add:

```markdown
```
wsx repo set-related-repos <name> <value-or-@file>
wsx repo edit-related-repos <name>
```

Per-repo list of other wsx-registered repos that workspaces in this repo should reference. Comma-separated names (e.g. `frontend,marketing`). At spawn time wsx looks each name up in the repo registry and passes `--add-dir <source-path>` to claude. Unknown names are silently skipped (logged at `info` level via `RUST_LOG=wsx=info`).
```

- [ ] **Step 2: Add a "Related repos" subsection**

Find a good location near the existing per-repo feature subsections (e.g. after the Pinned commands subsection or near "Per-repo setup scripts"). Add:

```markdown
## Related repos

When you work across multiple repos that need to know about each other (a backend, a frontend, a marketing site), declare related repos per primary repo:

```bash
wsx repo set-related-repos backend frontend,marketing
```

When you spawn a workspace in `backend`, wsx invokes claude with `--add-dir` pointing at each related repo's source path. Claude can read, grep, and reference files in those directories freely.

To prevent claude from accidentally editing files in the source paths of related repos (which would land changes on whatever branch the source is on), wsx also appends a system-prompt instruction telling claude:

- Treat those directories as read-only.
- If changes are needed there, ask the user to create a new wsx workspace in that repo and switch to it.

This is a soft guard, not a tool-level lock — it relies on claude following the instruction. The same trust model as `custom_instructions`.

Unknown names in the list (e.g. a repo you renamed or unregistered) are logged and skipped at spawn time; the spawn still proceeds with the recognized names.
```

- [ ] **Step 3: Add a Key features bullet (optional)**

Near the top of the README under `## Key features`, add:

```markdown
- **Related repos** — declare related wsx repos per primary repo; workspaces spawn with `--add-dir` for each and a read-only system prompt so claude can read but won't edit them.
```

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs(readme): related repos feature

Per-repo CLI block and a Related repos subsection explaining the
feature, the read-only system-prompt guard, and the unknown-name
skip behavior."
```

---

## Task 8: File issue link in spec; final verification

- [ ] **Step 1: Final verification**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1
```

All three expected clean.

- [ ] **Step 2: Manual smoke test**

```bash
cargo build --release
./target/release/wsx repo set-related-repos <some-repo> "<another-registered-repo>"
./target/release/wsx
```

In the TUI:
- Attach to a workspace in the repo where related_repos was set.
- After claude prints its startup banner, ask it: `which directories are you working in?` or similar.
- Confirm claude mentions the related repo's source path (showing the `--add-dir` took effect).
- Also confirm claude is aware of the read-only restriction (it should say it can read but not edit those paths).

- [ ] **Step 3: Open the PR**

Use the `pull-request` skill or:

```bash
git push -u origin HEAD
gh pr create --title "feat: related repos via --add-dir + read-only system prompt" \
  --body "<see PR skill template; reference issue #41>"
```

The PR body should include:
- Summary
- Manual-test items (the smoke test above)
- Closes #41

---

## Notes for the executor

- **Each task leaves the tree green.** Exception: Task 1 deliberately commits with `Repo.related_repos` referenced but not yet present in the struct — the compile becomes green again after Task 2's struct change. This is the trade-off for testing `related::tests` against forward-looking fixtures; the alternative (defer all `related::` tests to Task 2) was rejected because Task 1's parser is a clean unit worth committing in isolation.
- **Test count deltas:** Task 1: +10 (parse/resolve/prompt-builder). Task 2: +1. Task 3: +3. Task 5: +2. Task 6: +2. Total: +18 vs base.
- **The forward-compat trick in Task 1:** the test helper includes `related_repos: None,`. If you'd rather commit Task 1 with a stub that doesn't reference `Repo`, swap the resolver to take `Vec<&str>` of names and lift the Repo lookup to the call site. That avoids the forward-ref but complicates the resolver's interface. Stick with the plan as written unless you have reason to deviate.
- **Self-references**: a repo listing its own name in `related_repos` is filtered out in `build_spawn_info` (the resolver still returns it; the caller filters). The test in Task 6 covers this.
- **PM session**: `SpawnMode::ProjectManager.additional_dirs` is always empty. PM has no owning repo. The field exists for uniformity across SpawnMode variants.
- **`grep -rn "SpawnMode::"` in Task 5 Step 4 is the source of truth** for which sites need `additional_dirs: vec![]`. Don't trust my "~15-20" estimate.
- **If `CommandBuilder::get_argv()` doesn't exist** in portable-pty 0.9, the build_claude_command tests in Task 5 will need a different assertion strategy. Options: (a) assert on `format!("{cmd:?}")` debug output and grep for `--add-dir`, (b) extract the arg-building into a testable pure function `fn args_for_claude(...) -> Vec<String>` and test that. Adapt as needed.
