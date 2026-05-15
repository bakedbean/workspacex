# Move setup/archive scripts from `.claudette.json` into the DB — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace per-repo `.claudette.json` setup/archive scripts with two nullable TEXT columns on `repos`, executed via `sh -c`, and managed through `wsx repo set-setup` / `set-archive` / `edit-setup` / `edit-archive`. Drop all `.claudette.json` support.

**Architecture:** Schema v3 adds `setup_script` and `archive_script` columns to `repos`. `setup.rs` becomes a stateless script runner that takes `Option<&str>` (no I/O of its own). `workspace.rs` passes the values from the already-loaded `Repo` struct. CLI grows four new `wsx repo` subcommands mirroring `set-instructions` / `config edit`.

**Tech Stack:** Rust 2024, `rusqlite` (SQLite, schema migration), `tokio::process::Command` (subprocess), existing `ValueSource` / `open_in_editor` helpers in `cli.rs`.

**Source spec:** `docs/superpowers/specs/2026-05-14-setup-scripts-in-db-design.md`

---

## File Structure

| File | Action | Responsibility |
|---|---|---|
| `src/store.rs` | Modify | Schema v3 migration, `Repo` struct fields, store setters |
| `src/setup.rs` | Rewrite | Stateless `run_setup` / `run_archive` taking `Option<&str>` |
| `src/workspace.rs` | Modify | Pass `repo.setup_script.as_deref()` / `repo.archive_script.as_deref()` to setup; update tests |
| `src/cli.rs` | Modify | New `CliAction` variants, parsing, handlers |
| `README.md` | Modify | Replace "Per-repo setup scripts" section |

No new files. No `Cargo.toml` change (`serde_json` is still used by `events.rs` and `pm.rs`; `serde` is still used by `pm.rs` for `Serialize`).

---

## Task 1: Schema v3 — add setup_script and archive_script columns

**Files:**
- Modify: `src/store.rs`

- [ ] **Step 1: Write the failing test for the new columns being NULL on a fresh row**

Add to `src/store.rs` `mod tests`:

```rust
#[test]
fn repo_setup_and_archive_scripts_default_null() {
    let store = Store::open_in_memory().unwrap();
    let _id = store.add_repo(Path::new("/r"), "demo", "").unwrap();
    let repos = store.repos().unwrap();
    assert_eq!(repos[0].setup_script, None);
    assert_eq!(repos[0].archive_script, None);
}
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cargo test -p wsx --lib store::tests::repo_setup_and_archive_scripts_default_null -- --test-threads=1`
Expected: FAIL (compile error: `setup_script` is not a field of `Repo`).

- [ ] **Step 3: Add the struct fields**

In `src/store.rs`, extend `pub struct Repo`:

```rust
#[derive(Debug, Clone)]
pub struct Repo {
    pub id: RepoId,
    pub name: String,
    pub path: PathBuf,
    pub branch_prefix: String,
    pub custom_instructions: Option<String>,
    pub setup_script: Option<String>,
    pub archive_script: Option<String>,
    pub created_at: i64,
}
```

- [ ] **Step 4: Extend the SELECT in `repos()` and the row mapping**

Replace the body of `pub fn repos(&self)` in `src/store.rs`:

```rust
pub fn repos(&self) -> Result<Vec<Repo>> {
    let mut stmt = self.conn.prepare(
        "SELECT id, name, path, branch_prefix, custom_instructions, \
                setup_script, archive_script, created_at \
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
            created_at: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}
```

- [ ] **Step 5: Add the v3 migration step**

In `src/store.rs`, extend `fn migrate(&self)` immediately after the existing `if v < 2 { … }` block:

```rust
if v < 3 {
    let has_setup: i64 = self.conn.query_row(
        "SELECT count(*) FROM pragma_table_info('repos') WHERE name = 'setup_script'",
        [],
        |r| r.get(0),
    )?;
    if has_setup == 0 {
        self.conn
            .execute("ALTER TABLE repos ADD COLUMN setup_script TEXT", [])?;
    }
    let has_archive: i64 = self.conn.query_row(
        "SELECT count(*) FROM pragma_table_info('repos') WHERE name = 'archive_script'",
        [],
        |r| r.get(0),
    )?;
    if has_archive == 0 {
        self.conn
            .execute("ALTER TABLE repos ADD COLUMN archive_script TEXT", [])?;
    }
    self.conn.execute("PRAGMA user_version = 3", [])?;
}
```

- [ ] **Step 6: Run the test to confirm it passes**

Run: `cargo test -p wsx --lib store::tests::repo_setup_and_archive_scripts_default_null -- --test-threads=1`
Expected: PASS.

- [ ] **Step 7: Run the whole `store` test module to check for regressions**

Run: `cargo test -p wsx --lib store:: -- --test-threads=1`
Expected: all existing store tests still PASS (no behavior change for repo CRUD or settings).

Note: `cargo build` of the rest of the crate will FAIL at this point because `repo.setup_script` doesn't exist anywhere it's referenced — that's OK; Task 2 and Task 3 are where consumers come online. Workspace tests aren't broken yet because they use the old fields only.

- [ ] **Step 8: Commit**

```bash
git add src/store.rs
git commit -m "feat(store): schema v3 adds repos.setup_script + archive_script"
```

---

## Task 2: Store setters for setup_script and archive_script

**Files:**
- Modify: `src/store.rs`

- [ ] **Step 1: Write the failing round-trip test**

Add to `src/store.rs` `mod tests`:

```rust
#[test]
fn repo_setup_script_round_trip() {
    let store = Store::open_in_memory().unwrap();
    let id = store.add_repo(Path::new("/r"), "demo", "").unwrap();
    assert_eq!(store.repos().unwrap()[0].setup_script, None);

    store
        .set_repo_setup_script(id, Some("bun install"))
        .unwrap();
    assert_eq!(
        store.repos().unwrap()[0].setup_script.as_deref(),
        Some("bun install")
    );

    store.set_repo_setup_script(id, None).unwrap();
    assert_eq!(store.repos().unwrap()[0].setup_script, None);
}

#[test]
fn repo_archive_script_round_trip() {
    let store = Store::open_in_memory().unwrap();
    let id = store.add_repo(Path::new("/r"), "demo", "").unwrap();
    assert_eq!(store.repos().unwrap()[0].archive_script, None);

    store
        .set_repo_archive_script(id, Some("rm -rf node_modules"))
        .unwrap();
    assert_eq!(
        store.repos().unwrap()[0].archive_script.as_deref(),
        Some("rm -rf node_modules")
    );

    store.set_repo_archive_script(id, None).unwrap();
    assert_eq!(store.repos().unwrap()[0].archive_script, None);
}
```

- [ ] **Step 2: Run them to confirm they fail**

Run: `cargo test -p wsx --lib store::tests::repo_setup_script_round_trip store::tests::repo_archive_script_round_trip -- --test-threads=1`
Expected: FAIL (compile error: no method `set_repo_setup_script` on `Store`).

- [ ] **Step 3: Add the setters**

In `src/store.rs`, after the existing `pub fn set_repo_custom_instructions` method on `impl Store`, add:

```rust
pub fn set_repo_setup_script(&self, id: RepoId, value: Option<&str>) -> Result<()> {
    self.conn.execute(
        "UPDATE repos SET setup_script = ?1 WHERE id = ?2",
        rusqlite::params![value, id.0],
    )?;
    Ok(())
}

pub fn set_repo_archive_script(&self, id: RepoId, value: Option<&str>) -> Result<()> {
    self.conn.execute(
        "UPDATE repos SET archive_script = ?1 WHERE id = ?2",
        rusqlite::params![value, id.0],
    )?;
    Ok(())
}
```

Both are pass-through (mirrors `set_repo_custom_instructions`). Normalization of empty/whitespace values lives in the CLI layer (Task 4).

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test -p wsx --lib store::tests::repo_setup_script_round_trip store::tests::repo_archive_script_round_trip -- --test-threads=1`
Expected: both PASS.

- [ ] **Step 5: Run the full store test module**

Run: `cargo test -p wsx --lib store:: -- --test-threads=1`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add src/store.rs
git commit -m "feat(store): set_repo_setup_script + set_repo_archive_script"
```

---

## Task 3: Rewrite `setup.rs` and update `workspace.rs` callers atomically

This task is one logical unit because changing `run_setup` / `run_archive` signatures simultaneously breaks `workspace.rs` and fixes it. The two files are modified and tested together in a single commit.

**Files:**
- Rewrite: `src/setup.rs`
- Modify: `src/workspace.rs` (callers and tests)

- [ ] **Step 1: Replace `src/setup.rs` with the new stateless runner**

Overwrite `src/setup.rs` with the following exact contents:

```rust
use crate::error::{Error, Result};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[derive(Debug, Clone)]
pub enum SetupLine {
    Stdout(String),
    Stderr(String),
}

#[derive(Debug, Clone)]
pub enum SetupResult {
    Skipped,
    Ok,
    Failed { exit_code: i32 },
}

pub async fn run_setup<F: FnMut(SetupLine) + Send>(
    script: Option<&str>,
    repo_root: &Path,
    worktree: &Path,
    on_line: F,
) -> Result<SetupResult> {
    match script {
        Some(s) if !s.trim().is_empty() => run_script(s, repo_root, worktree, on_line).await,
        _ => Ok(SetupResult::Skipped),
    }
}

pub async fn run_archive<F: FnMut(SetupLine) + Send>(
    script: Option<&str>,
    repo_root: &Path,
    worktree: &Path,
    on_line: F,
) -> Result<SetupResult> {
    match script {
        Some(s) if !s.trim().is_empty() => run_script(s, repo_root, worktree, on_line).await,
        _ => Ok(SetupResult::Skipped),
    }
}

async fn run_script<F: FnMut(SetupLine) + Send>(
    script: &str,
    repo_root: &Path,
    worktree: &Path,
    mut on_line: F,
) -> Result<SetupResult> {
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(script)
        .current_dir(worktree)
        .env("WSX_REPO_ROOT", repo_root)
        .env("WSX_WORKTREE", worktree)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let mut child = cmd
        .spawn()
        .map_err(|e| Error::Setup(format!("spawn: {e}")))?;
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    let mut out_reader = BufReader::new(stdout).lines();
    let mut err_reader = BufReader::new(stderr).lines();

    loop {
        tokio::select! {
            line = out_reader.next_line() => match line {
                Ok(Some(l)) => on_line(SetupLine::Stdout(l)),
                Ok(None) => break,
                Err(e) => return Err(Error::Setup(format!("stdout read: {e}"))),
            },
            line = err_reader.next_line() => match line {
                Ok(Some(l)) => on_line(SetupLine::Stderr(l)),
                Ok(None) => break,
                Err(e) => return Err(Error::Setup(format!("stderr read: {e}"))),
            },
        }
    }
    while let Ok(Some(l)) = out_reader.next_line().await {
        on_line(SetupLine::Stdout(l));
    }
    while let Ok(Some(l)) = err_reader.next_line().await {
        on_line(SetupLine::Stderr(l));
    }

    let status = child
        .wait()
        .await
        .map_err(|e| Error::Setup(format!("wait: {e}")))?;
    if status.success() {
        Ok(SetupResult::Ok)
    } else {
        Ok(SetupResult::Failed {
            exit_code: status.code().unwrap_or(-1),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    #[tokio::test]
    async fn none_script_is_skipped() {
        let repo = TempDir::new().unwrap();
        let wt = TempDir::new().unwrap();
        let r = run_setup(None, repo.path(), wt.path(), |_| {})
            .await
            .unwrap();
        assert!(matches!(r, SetupResult::Skipped));
    }

    #[tokio::test]
    async fn empty_and_whitespace_scripts_are_skipped() {
        let repo = TempDir::new().unwrap();
        let wt = TempDir::new().unwrap();
        let r = run_setup(Some(""), repo.path(), wt.path(), |_| {})
            .await
            .unwrap();
        assert!(matches!(r, SetupResult::Skipped));
        let r = run_setup(Some("   \n\t"), repo.path(), wt.path(), |_| {})
            .await
            .unwrap();
        assert!(matches!(r, SetupResult::Skipped));
    }

    #[tokio::test]
    async fn setup_streams_stdout_and_stderr_and_succeeds() {
        let repo = TempDir::new().unwrap();
        let wt = TempDir::new().unwrap();
        let lines = Arc::new(Mutex::new(Vec::new()));
        let lines2 = lines.clone();
        let r = run_setup(
            Some("echo hello; echo bye 1>&2"),
            repo.path(),
            wt.path(),
            move |l| {
                lines2.lock().unwrap().push(l);
            },
        )
        .await
        .unwrap();
        assert!(matches!(r, SetupResult::Ok));
        let lines = lines.lock().unwrap();
        assert!(
            lines
                .iter()
                .any(|l| matches!(l, SetupLine::Stdout(s) if s == "hello"))
        );
        assert!(
            lines
                .iter()
                .any(|l| matches!(l, SetupLine::Stderr(s) if s == "bye"))
        );
    }

    #[tokio::test]
    async fn setup_reports_nonzero_exit() {
        let repo = TempDir::new().unwrap();
        let wt = TempDir::new().unwrap();
        let r = run_setup(Some("exit 7"), repo.path(), wt.path(), |_| {})
            .await
            .unwrap();
        match r {
            SetupResult::Failed { exit_code } => assert_eq!(exit_code, 7),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn setup_injects_env_vars() {
        let repo = TempDir::new().unwrap();
        let wt = TempDir::new().unwrap();
        let lines = Arc::new(Mutex::new(Vec::new()));
        let lines2 = lines.clone();
        run_setup(
            Some("echo $WSX_WORKTREE; echo $WSX_REPO_ROOT"),
            repo.path(),
            wt.path(),
            move |l| {
                lines2.lock().unwrap().push(l);
            },
        )
        .await
        .unwrap();
        let expected_wt = wt.path().to_string_lossy().to_string();
        let expected_repo = repo.path().to_string_lossy().to_string();
        let lines = lines.lock().unwrap();
        assert!(
            lines
                .iter()
                .any(|l| matches!(l, SetupLine::Stdout(s) if *s == expected_wt))
        );
        assert!(
            lines
                .iter()
                .any(|l| matches!(l, SetupLine::Stdout(s) if *s == expected_repo))
        );
    }

    #[tokio::test]
    async fn run_archive_executes_the_provided_script() {
        let repo = TempDir::new().unwrap();
        let wt = TempDir::new().unwrap();
        let r = run_archive(Some("exit 3"), repo.path(), wt.path(), |_| {})
            .await
            .unwrap();
        match r {
            SetupResult::Failed { exit_code } => assert_eq!(exit_code, 3),
            other => panic!("expected Failed, got {other:?}"),
        }
        let r = run_archive(None, repo.path(), wt.path(), |_| {})
            .await
            .unwrap();
        assert!(matches!(r, SetupResult::Skipped));
    }
}
```

This deletes `RepoConfig`, `ScriptSpec`, `load_repo_config`, the `use serde::Deserialize` import, and every previous test in the file (all of which depended on `.claudette.json` fixtures).

- [ ] **Step 2: Update `workspace.rs` to pass the script strings from `Repo`**

In `src/workspace.rs`, change the body of `pub async fn create` so the `run_setup` call passes the script:

Replace this line in `create`:

```rust
    let setup_result = setup::run_setup(&repo.path, &worktree_path, on_setup_line).await?;
```

with:

```rust
    let setup_result = setup::run_setup(
        repo.setup_script.as_deref(),
        &repo.path,
        &worktree_path,
        on_setup_line,
    )
    .await?;
```

In `pub async fn archive`, replace this line:

```rust
    let archive_result = setup::run_archive(&repo.path, &ws.worktree_path, on_archive_line).await?;
```

with:

```rust
    let archive_result = setup::run_archive(
        repo.archive_script.as_deref(),
        &repo.path,
        &ws.worktree_path,
        on_archive_line,
    )
    .await?;
```

- [ ] **Step 3: Rewrite the workspace failure test to use the new column**

In `src/workspace.rs` `mod tests`, replace `create_records_setup_failure_but_keeps_workspace_ready` with:

```rust
    #[tokio::test]
    async fn create_records_setup_failure_but_keeps_workspace_ready() {
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let id = crate::repo::add(&store, repo_dir.path(), "demo", "")
            .await
            .unwrap();
        store.set_repo_setup_script(id, Some("exit 1")).unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let base = TempDir::new().unwrap();
        let created = create(&store, &repo, Some("a"), base.path(), |_| {})
            .await
            .unwrap();
        assert_eq!(created.workspace.state, WorkspaceState::Ready);
        assert_eq!(created.workspace.setup_status, SetupStatus::Failed);
    }
```

The only change from the previous version is replacing the `std::fs::write(...".claudette.json"...)` line with a `set_repo_setup_script` call before fetching the `Repo`.

- [ ] **Step 4: Add a positive setup-script test**

In `src/workspace.rs` `mod tests`, add:

```rust
    #[tokio::test]
    async fn create_runs_setup_script_when_set() {
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let id = crate::repo::add(&store, repo_dir.path(), "demo", "")
            .await
            .unwrap();
        let base = TempDir::new().unwrap();
        // Touch a marker file inside the worktree.
        store
            .set_repo_setup_script(id, Some("touch wsx-setup-marker"))
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let created = create(&store, &repo, Some("a"), base.path(), |_| {})
            .await
            .unwrap();
        assert_eq!(created.workspace.setup_status, SetupStatus::Ok);
        assert!(created.workspace.worktree_path.join("wsx-setup-marker").exists());
    }
```

- [ ] **Step 5: Add an archive-script test**

In `src/workspace.rs` `mod tests`, add:

```rust
    #[tokio::test]
    async fn archive_runs_archive_script_when_set() {
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let id = crate::repo::add(&store, repo_dir.path(), "demo", "")
            .await
            .unwrap();
        let base = TempDir::new().unwrap();
        let scratch = TempDir::new().unwrap();
        let marker = scratch.path().join("wsx-archive-marker");
        let script = format!("touch {}", marker.display());
        store
            .set_repo_archive_script(id, Some(&script))
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let created = create(&store, &repo, Some("doomed"), base.path(), |_| {})
            .await
            .unwrap();
        archive(
            &store,
            &repo,
            &created.workspace,
            ArchiveOpts {
                force_branch_delete: true,
                ..Default::default()
            },
            |_| {},
        )
        .await
        .unwrap();
        assert!(marker.exists(), "archive script did not run");
    }
```

The archive script writes outside the (now-deleted) worktree so the assertion can read the marker after the worktree is gone.

- [ ] **Step 6: Build the whole crate**

Run: `cargo build -p wsx`
Expected: clean build. If there are unrelated callers of `run_setup` / `run_archive` outside `workspace.rs`, this surfaces them now (we don't expect any — `grep -rn "run_setup\|run_archive" src/` confirmed only `workspace.rs` calls these).

- [ ] **Step 7: Run setup and workspace test modules**

Run: `cargo test -p wsx --lib setup:: workspace:: -- --test-threads=1`
Expected: all PASS, including the four new tests.

- [ ] **Step 8: Run the full test suite for regressions**

Run: `cargo test -p wsx -- --test-threads=1`
Expected: all PASS.

- [ ] **Step 9: Commit**

```bash
git add src/setup.rs src/workspace.rs
git commit -m "refactor(setup): take script string instead of .claudette.json"
```

---

## Task 4: CLI subcommands — `repo set-setup` / `set-archive` / `edit-setup` / `edit-archive`

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 1: Write the failing parse tests**

Add to `src/cli.rs` `mod tests`:

```rust
    #[test]
    fn parses_repo_set_setup_literal() {
        let a = parse(&["repo", "set-setup", "demo", "bun install"]).unwrap();
        match a {
            CliAction::RepoSetSetup {
                name,
                source: ValueSource::Literal(v),
            } => {
                assert_eq!(name, "demo");
                assert_eq!(v, "bun install");
            }
            _ => panic!("wrong action"),
        }
    }

    #[test]
    fn parses_repo_set_setup_file_reference() {
        let a = parse(&["repo", "set-setup", "demo", "@./setup.sh"]).unwrap();
        match a {
            CliAction::RepoSetSetup {
                name,
                source: ValueSource::File(p),
            } => {
                assert_eq!(name, "demo");
                assert_eq!(p, std::path::PathBuf::from("./setup.sh"));
            }
            _ => panic!("wrong action"),
        }
    }

    #[test]
    fn parses_repo_set_archive_literal() {
        let a = parse(&["repo", "set-archive", "demo", "rm -rf node_modules"]).unwrap();
        match a {
            CliAction::RepoSetArchive {
                name,
                source: ValueSource::Literal(v),
            } => {
                assert_eq!(name, "demo");
                assert_eq!(v, "rm -rf node_modules");
            }
            _ => panic!("wrong action"),
        }
    }

    #[test]
    fn parses_repo_edit_setup_and_edit_archive() {
        match parse(&["repo", "edit-setup", "demo"]).unwrap() {
            CliAction::RepoEditSetup { name } => assert_eq!(name, "demo"),
            _ => panic!("wrong action"),
        }
        match parse(&["repo", "edit-archive", "demo"]).unwrap() {
            CliAction::RepoEditArchive { name } => assert_eq!(name, "demo"),
            _ => panic!("wrong action"),
        }
    }
```

- [ ] **Step 2: Run them to confirm they fail**

Run: `cargo test -p wsx --lib cli::tests -- --test-threads=1`
Expected: FAIL (compile error: variants `RepoSetSetup` / `RepoSetArchive` / `RepoEditSetup` / `RepoEditArchive` don't exist on `CliAction`).

- [ ] **Step 3: Add the new `CliAction` variants**

In `src/cli.rs`, extend the `enum CliAction` block, adding these variants after `RepoSetInstructions`:

```rust
    RepoSetSetup {
        name: String,
        source: ValueSource,
    },
    RepoSetArchive {
        name: String,
        source: ValueSource,
    },
    RepoEditSetup {
        name: String,
    },
    RepoEditArchive {
        name: String,
    },
```

- [ ] **Step 4: Add the parse arms**

In `src/cli.rs`, inside the `Some("repo") => match it.next().as_deref() {` block, add these arms after the existing `Some("set-instructions")` arm and before the `other =>` catch-all:

```rust
            Some("set-setup") => {
                let name = it.next().ok_or_else(|| {
                    Error::UserInput("repo set-setup <name> <value-or-@file>".into())
                })?;
                let value = it.next().ok_or_else(|| {
                    Error::UserInput("repo set-setup <name> <value-or-@file>".into())
                })?;
                Ok(CliAction::RepoSetSetup {
                    name,
                    source: ValueSource::from_arg(value),
                })
            }
            Some("set-archive") => {
                let name = it.next().ok_or_else(|| {
                    Error::UserInput("repo set-archive <name> <value-or-@file>".into())
                })?;
                let value = it.next().ok_or_else(|| {
                    Error::UserInput("repo set-archive <name> <value-or-@file>".into())
                })?;
                Ok(CliAction::RepoSetArchive {
                    name,
                    source: ValueSource::from_arg(value),
                })
            }
            Some("edit-setup") => {
                let name = it
                    .next()
                    .ok_or_else(|| Error::UserInput("repo edit-setup <name>".into()))?;
                Ok(CliAction::RepoEditSetup { name })
            }
            Some("edit-archive") => {
                let name = it
                    .next()
                    .ok_or_else(|| Error::UserInput("repo edit-archive <name>".into()))?;
                Ok(CliAction::RepoEditArchive { name })
            }
```

- [ ] **Step 5: Run the parse tests to confirm they pass**

Run: `cargo test -p wsx --lib cli::tests -- --test-threads=1`
Expected: PASS.

- [ ] **Step 6: Add the handlers in `run_cli`**

In `src/cli.rs`, inside `pub async fn run_cli`, add these match arms after the existing `CliAction::RepoSetInstructions { name, source } => { ... }` block (and before `CliAction::ConfigGet`):

```rust
        CliAction::RepoSetSetup { name, source } => {
            let repos = crate::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let value = source.resolve()?;
            if value.trim().is_empty() {
                store.set_repo_setup_script(r.id, None)?;
                println!("cleared setup for {name}");
            } else {
                store.set_repo_setup_script(r.id, Some(&value))?;
                println!("set setup for {name} ({} chars)", value.len());
            }
        }
        CliAction::RepoSetArchive { name, source } => {
            let repos = crate::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let value = source.resolve()?;
            if value.trim().is_empty() {
                store.set_repo_archive_script(r.id, None)?;
                println!("cleared archive for {name}");
            } else {
                store.set_repo_archive_script(r.id, Some(&value))?;
                println!("set archive for {name} ({} chars)", value.len());
            }
        }
        CliAction::RepoEditSetup { name } => {
            let repos = crate::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let current = r.setup_script.clone().unwrap_or_default();
            let new_value = open_in_editor("setup", &current)?;
            let new_value = new_value.trim_end_matches('\n').to_string();
            if new_value.trim().is_empty() {
                store.set_repo_setup_script(r.id, None)?;
                println!("cleared setup for {name}");
            } else if new_value == current {
                println!("setup unchanged");
            } else {
                store.set_repo_setup_script(r.id, Some(&new_value))?;
                println!("set setup for {name} ({} chars)", new_value.len());
            }
        }
        CliAction::RepoEditArchive { name } => {
            let repos = crate::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let current = r.archive_script.clone().unwrap_or_default();
            let new_value = open_in_editor("archive", &current)?;
            let new_value = new_value.trim_end_matches('\n').to_string();
            if new_value.trim().is_empty() {
                store.set_repo_archive_script(r.id, None)?;
                println!("cleared archive for {name}");
            } else if new_value == current {
                println!("archive unchanged");
            } else {
                store.set_repo_archive_script(r.id, Some(&new_value))?;
                println!("set archive for {name} ({} chars)", new_value.len());
            }
        }
```

- [ ] **Step 7: Build + run all tests**

Run: `cargo build -p wsx`
Expected: clean build.

Run: `cargo test -p wsx -- --test-threads=1`
Expected: all PASS.

- [ ] **Step 8: Manual smoke test (optional but recommended)**

Run a quick end-to-end against a scratch DB:

```bash
# Use a scratch state dir so the user's real wsx state is untouched.
XDG_STATE_HOME=$(mktemp -d) cargo run --quiet -- repo add /tmp/does-not-matter --name fake 2>&1 || true
# The above will fail because the path isn't a git repo — that's fine.
# Instead, exercise parsing only:
cargo run --quiet -- repo set-setup nope 'bun install' 2>&1 | head -3
# Expected: "Error: no repo named nope" (or similar UserInput error).
```

Expected: the CLI accepts the new subcommand syntactically and errors only on the unknown-repo lookup, not on argument parsing.

- [ ] **Step 9: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): repo set-setup / set-archive / edit-setup / edit-archive"
```

---

## Task 5: Update README — replace the `.claudette.json` section

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Replace the "Per-repo setup scripts" section**

In `README.md`, find the existing section that begins:

```
## Per-repo setup scripts

A `.claudette.json` file in the repo root is honored for setup and archive scripts that run when a workspace is created or removed:
```

and replace the entire section (through the paragraph ending `…it's surfaced as a `[setup-failed]` badge on the dashboard.`) with:

```markdown
## Per-repo setup scripts

Each repo can have a `setup` script (run when a workspace is created) and an `archive` script (run when a workspace is removed). Both are stored in the wsx state database and configured per-repo via the CLI:

```bash
wsx repo set-setup    <repo-name> 'bun install'
wsx repo set-archive  <repo-name> 'rm -rf node_modules'
```

For multi-line scripts, pass a file with the `@` prefix or open `$EDITOR`:

```bash
wsx repo set-setup    <repo-name> @./scripts/setup.sh
wsx repo edit-setup   <repo-name>
wsx repo edit-archive <repo-name>
```

Each script is executed as `sh -c "$value"` with `cwd` set to the new worktree and two extra env vars: `WSX_REPO_ROOT` (the source repo) and `WSX_WORKTREE` (the new worktree). Setup failure does not block the workspace from being usable; it's surfaced as a `[setup-failed]` badge on the dashboard. Passing an empty value clears the script.
```

- [ ] **Step 2: Verify no other `.claudette.json` references remain in README**

Run: `grep -n "claudette" README.md`
Expected: no output (zero matches).

If matches exist, remove or update those lines too.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs(readme): document repo set-setup / set-archive CLI"
```

---

## Self-Review

**Spec coverage check:**

| Spec section | Task |
|---|---|
| Schema (v3 migration, two TEXT columns) | Task 1 |
| `Repo` struct extension + SELECT | Task 1 |
| Store setters (`set_repo_setup_script`, `set_repo_archive_script`) | Task 2 |
| `setup.rs` rewrite (`Option<&str>` runner, deletions) | Task 3 |
| Workspace lifecycle (pass `repo.setup_script.as_deref()`) | Task 3 |
| CLI subcommands (set/edit, value-or-`@file`, EDITOR flow) | Task 4 |
| Error handling (Skipped/Ok/Failed paths) | Task 3 (tests cover all four) |
| Tests (setup, store, cli, workspace) | Tasks 1–4 |
| README rewrite | Task 5 |
| Non-goals (no migration, no global fallback, no structured env) | Not implemented — by omission, as intended |

No gaps.

**Placeholder scan:** No "TBD" / "implement later" / "similar to Task N". Every code step contains the exact code to type. Every test step shows the assertion.

**Type consistency:** `setup_script: Option<String>` / `archive_script: Option<String>` on `Repo` matches `Option<&str>` parameter on `run_setup` / `run_archive` via `.as_deref()`. `CliAction::RepoSetSetup { name, source: ValueSource }` matches the handler arm's destructuring. `set_repo_setup_script(id: RepoId, value: Option<&str>)` matches both the test calls (`Some("…")` / `None`) and the handler calls.

Plan ready.
