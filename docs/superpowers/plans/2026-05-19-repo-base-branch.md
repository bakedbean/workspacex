# Per-repo `base_branch` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let users configure, per repo, the git ref new workspaces branch from. Unset → today's behavior (fork off current HEAD) is preserved.

**Architecture:** Add a nullable `base_branch` column to the `repos` table (schema v7). `workspace::create` reads it, calls a new `git::fetch_for_base` helper to fetch when the value's prefix matches a configured remote, then passes the base through to `git::create_worktree`. CLI gets `wsx repo set-base-branch <repo> <ref>`; TUI gets a 7th row in the repo-settings modal.

**Tech Stack:** Rust, `rusqlite`, hand-rolled CLI parser (`src/cli.rs`), `ratatui` TUI, `tokio::process::Command` for git invocations.

**Spec:** [`docs/superpowers/specs/2026-05-19-repo-base-branch-design.md`](../specs/2026-05-19-repo-base-branch-design.md)

**Issue:** [#43](https://github.com/bakedbean/workspacex/issues/43)

---

## File map

- **Modify:** `src/store.rs` — schema v7 migration; `Repo.base_branch` field; `repos()` SELECT updated; `set_repo_base_branch` method; round-trip test
- **Modify:** `src/git.rs` — `create_worktree` signature gains `base: Option<&str>`; new `fetch_for_base` helper; tests
- **Modify:** `src/workspace.rs` — `create` reads `repo.base_branch`, calls `fetch_for_base`, passes base to `create_worktree`; test
- **Modify:** `src/cli.rs` — `CliAction::RepoSetBaseBranch` variant + parse arm + dispatch arm + test
- **Modify:** `src/app.rs` — `RepoSettingField::BaseBranch` variant + `ALL` extended to 7 + `label()` + `editable_value_for_field`-style match + edit dispatch
- **Modify:** `src/ui/modal.rs` — extend 6-element row array to 7

---

## Task 1: Store layer — migration v7 + field + setter (TDD)

Adds the column, struct field, SELECT update, and setter. Round-trip test guards the whole layer.

**Files:**
- Modify: `src/store.rs`

- [ ] **Step 1: Write the failing round-trip test**

In `src/store.rs`, inside the existing `#[cfg(test)] mod tests` block (after `fn repo_custom_instructions_round_trip` around line 548), append:

```rust
    #[test]
    fn repo_base_branch_round_trip() {
        let store = Store::open_in_memory().unwrap();
        let id = store.add_repo(Path::new("/r"), "demo", "").unwrap();
        let repos = store.repos().unwrap();
        assert_eq!(repos[0].base_branch, None);

        store
            .set_repo_base_branch(id, Some("origin/main"))
            .unwrap();
        let repos = store.repos().unwrap();
        assert_eq!(repos[0].base_branch.as_deref(), Some("origin/main"));

        store.set_repo_base_branch(id, None).unwrap();
        let repos = store.repos().unwrap();
        assert_eq!(repos[0].base_branch, None);
    }
```

- [ ] **Step 2: Run the test; verify it fails to compile**

Run:
```bash
cargo test --lib store::tests::repo_base_branch_round_trip 2>&1 | tail -15
```
Expected: compile error mentioning `base_branch` field missing on `Repo` and `set_repo_base_branch` method not found.

- [ ] **Step 3: Add the migration step**

In `src/store.rs`, in the `migrate` function (currently ends at `user_version = 6` around line 167), insert a new `if v < 7 { ... }` block right before `Ok(())`. The block:

```rust
        if v < 7 {
            let has_col: i64 = self.conn.query_row(
                "SELECT count(*) FROM pragma_table_info('repos') WHERE name = 'base_branch'",
                [],
                |r| r.get(0),
            )?;
            if has_col == 0 {
                self.conn
                    .execute("ALTER TABLE repos ADD COLUMN base_branch TEXT", [])?;
            }
            self.conn.execute("PRAGMA user_version = 7", [])?;
        }
```

- [ ] **Step 4: Add the field to `Repo`**

In `src/store.rs`, the `pub struct Repo` block (around line 29). Add `pub base_branch: Option<String>` AFTER `pub related_repos: Option<String>` and BEFORE `pub created_at: i64`:

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
    pub base_branch: Option<String>,
    pub created_at: i64,
}
```

- [ ] **Step 5: Update `repos()` SELECT and row mapping**

In `src/store.rs`, find `fn repos()` (around line 188). Update the SELECT to include `base_branch`, and add the corresponding `r.get(...)` mapping. The whole function becomes:

```rust
    pub fn repos(&self) -> Result<Vec<Repo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, path, branch_prefix, custom_instructions, \
                    setup_script, archive_script, pinned_commands, \
                    related_repos, base_branch, created_at \
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
                base_branch: r.get(9)?,
                created_at: r.get(10)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
```

- [ ] **Step 6: Add the `set_repo_base_branch` method**

In `src/store.rs`, after `set_repo_related_repos` (around line 262), add:

```rust
    pub fn set_repo_base_branch(&self, id: RepoId, value: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE repos SET base_branch = ?1 WHERE id = ?2",
            rusqlite::params![value, id.0],
        )?;
        Ok(())
    }
```

- [ ] **Step 7: Run tests; verify pass**

Run:
```bash
cargo test --lib store::tests 2>&1 | tail -10
```
Expected: all `store::tests::*` pass, including the new `repo_base_branch_round_trip`.

- [ ] **Step 8: Build**

Run:
```bash
cargo build 2>&1 | tail -5
```
Expected: build fails — the new `Repo.base_branch` field means struct literals in tests elsewhere (e.g. `workspace::tests` constructing `Repo` directly) may break. **Check carefully**: if struct construction sites exist outside `store.rs`, they need the new field. Search:

```bash
grep -rn "Repo {" src/ tests/ | grep -v "store.rs\|src/store.rs"
```

If matches exist, update each to include `base_branch: None,`. If no matches, build is clean.

(Most code reads `repo.field`, doesn't construct `Repo` literals — but verify.)

- [ ] **Step 9: Commit**

```bash
git add src/store.rs
git commit -m "$(cat <<'EOF'
feat(store): add base_branch column to repos (#43)

Schema v7. Adds a nullable TEXT column and a `set_repo_base_branch`
setter mirroring the existing `set_repo_setup_script` shape. Repo
struct gains the field; `repos()` SELECT updated to read it.

Behavior is unchanged until workspace::create reads the column
(separate commit).
EOF
)"
```

---

## Task 2: Extend `git::create_worktree` with optional base (TDD)

Adds a `base: Option<&str>` parameter to `create_worktree`. When `Some`, passes the base to `git worktree add` so the new branch forks off that ref. All existing call sites pass `None` (no behavior change yet).

**Files:**
- Modify: `src/git.rs`
- Modify: `src/workspace.rs` (only call site, passes `None` for now)

- [ ] **Step 1: Write the failing test**

In `src/git.rs`, inside `#[cfg(test)] mod worktree_tests` (around line 337), append (before the closing `}` of the module):

```rust
    #[tokio::test]
    async fn create_worktree_with_explicit_base() {
        let repo = init_repo();
        // Add a second commit on main so HEAD advances.
        std::fs::write(repo.path().join("a.txt"), "v1").unwrap();
        let r = |args: &[&str]| {
            assert!(
                std::process::Command::new("git")
                    .current_dir(repo.path())
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        };
        r(&["add", "a.txt"]);
        r(&["commit", "-q", "-m", "add a"]);
        // Capture the previous commit (init) and create `staging` pointing at it.
        let prev = std::process::Command::new("git")
            .current_dir(repo.path())
            .args(["rev-parse", "HEAD~1"])
            .output()
            .unwrap();
        let prev_sha = String::from_utf8_lossy(&prev.stdout).trim().to_string();
        r(&["branch", "staging", &prev_sha]);

        let wt_root = TempDir::new().unwrap();
        let wt = wt_root.path().join("from-staging");
        create_worktree(repo.path(), "feature", Some("staging"), &wt)
            .await
            .unwrap();

        let head = std::process::Command::new("git")
            .current_dir(&wt)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let wt_head = String::from_utf8_lossy(&head.stdout).trim().to_string();
        assert_eq!(wt_head, prev_sha, "new worktree should be at staging's commit, not main HEAD");
    }
```

Also update the existing `create_and_list_worktree` and `remove_worktree_cleans_up` tests in the same module: change `create_worktree(repo.path(), "feature", &wt)` to `create_worktree(repo.path(), "feature", None, &wt)` (and similarly for the `"scratch"` test).

- [ ] **Step 2: Run tests; verify they fail to compile**

Run:
```bash
cargo test --lib git::worktree_tests 2>&1 | tail -15
```
Expected: compile errors — `create_worktree` arity mismatch.

- [ ] **Step 3: Update `create_worktree` signature**

In `src/git.rs`, replace the existing `create_worktree` function (around line 284):

```rust
pub async fn create_worktree(
    repo: &Path,
    branch: &str,
    base: Option<&str>,
    path: &Path,
) -> Result<()> {
    let path_s = path.to_string_lossy();
    let mut args: Vec<&str> = vec!["worktree", "add", "-b", branch, &path_s];
    if let Some(b) = base {
        args.push(b);
    }
    run(repo, &args).await?;
    Ok(())
}
```

- [ ] **Step 4: Update the single call site in `src/workspace.rs`**

In `src/workspace.rs` (around line 46), change:

```rust
    if let Err(e) = git::create_worktree(&repo.path, &branch, &worktree_path).await {
```

to:

```rust
    if let Err(e) = git::create_worktree(&repo.path, &branch, None, &worktree_path).await {
```

Task 4 changes this to actually pass `repo.base_branch.as_deref()`.

- [ ] **Step 5: Also update `workspace::tests` call sites**

In `src/workspace.rs`, search the test module for `create_worktree(`:

```bash
grep -n "git::create_worktree\|create_worktree(" src/workspace.rs
```

The visible test helper around line 413 calls `create_worktree(&repo.path, "orphan", &wt)`. Update it to pass `None`:

```rust
        git::create_worktree(&repo.path, "orphan", None, &wt)
```

If any other test call sites in the codebase use it, update those too. Confirm:

```bash
grep -rn "create_worktree(" src/ tests/
```
Every call must now have 4 args (`repo, branch, base, path`).

- [ ] **Step 6: Build + run tests**

```bash
cargo build 2>&1 | tail -5
cargo test --lib git::worktree_tests workspace::tests 2>&1 | tail -10
```

Note: `cargo test` only accepts ONE positional filter, so run them separately if needed:

```bash
cargo test --lib git::worktree_tests 2>&1 | tail -10
cargo test --lib workspace::tests 2>&1 | tail -10
```

Expected: clean build; all `git::worktree_tests` pass (including the new one); all `workspace::tests` pass (existing behavior preserved).

- [ ] **Step 7: Commit**

```bash
git add src/git.rs src/workspace.rs
git commit -m "$(cat <<'EOF'
feat(git): add optional base to create_worktree (#43)

Threads `Option<&str>` for the base ref through `create_worktree`.
When `Some`, the value is passed to `git worktree add` so the new
branch forks off that ref instead of current HEAD. All call sites
pass `None` for now — no behavior change. Subsequent commits wire
the value through from the repo settings.
EOF
)"
```

---

## Task 3: `git::fetch_for_base` heuristic helper (TDD)

Decides whether to fetch from a remote based on the configured `base` value. Heuristic: if the prefix before the first `/` matches a configured remote name, fetch `<remote> <rest>`. Otherwise no-op.

**Files:**
- Modify: `src/git.rs`

- [ ] **Step 1: Write failing tests**

In `src/git.rs`, inside `#[cfg(test)] mod worktree_tests` (before the closing `}` of the module), append:

```rust
    /// Test helper: clone `src` as a bare remote and add it as `origin`
    /// in a fresh local repo. Returns (local_repo, _remote_dir_guard).
    async fn local_with_origin() -> (TempDir, TempDir) {
        let remote = init_repo();
        // Make the remote bare so it can be pushed to / fetched from.
        let bare = TempDir::new().unwrap();
        std::process::Command::new("git")
            .args(["clone", "--bare", "--quiet"])
            .arg(remote.path())
            .arg(bare.path())
            .status()
            .unwrap();

        let local = init_repo();
        let bare_url = format!("file://{}", bare.path().display());
        std::process::Command::new("git")
            .current_dir(local.path())
            .args(["remote", "add", "origin", &bare_url])
            .status()
            .unwrap();
        // Push a new branch on the remote that doesn't exist locally.
        std::process::Command::new("git")
            .current_dir(remote.path())
            .args(["checkout", "-q", "-b", "feature-x"])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(remote.path())
            .args(["commit", "--allow-empty", "-q", "-m", "x"])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(remote.path())
            .args(["push", "--quiet", &bare_url, "feature-x"])
            .status()
            .unwrap();

        // Keep both alive by returning both TempDirs to the caller.
        (local, bare)
    }

    #[tokio::test]
    async fn fetch_for_base_no_op_when_unset() {
        let repo = init_repo();
        // No remote configured. Should not fail.
        fetch_for_base(repo.path(), None).await.unwrap();
    }

    #[tokio::test]
    async fn fetch_for_base_no_op_when_no_slash() {
        let repo = init_repo();
        // base = "main" — no slash, no fetch attempt.
        fetch_for_base(repo.path(), Some("main")).await.unwrap();
    }

    #[tokio::test]
    async fn fetch_for_base_no_op_when_prefix_does_not_match_remote() {
        let repo = init_repo();
        // No remote named "feature" — base "feature/foo" is a local branch.
        fetch_for_base(repo.path(), Some("feature/foo"))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn fetch_for_base_fetches_when_prefix_matches_remote() {
        let (local, _bare) = local_with_origin().await;
        // Sanity: before fetch, origin/feature-x doesn't exist locally.
        let pre = std::process::Command::new("git")
            .current_dir(local.path())
            .args(["rev-parse", "--verify", "refs/remotes/origin/feature-x"])
            .output()
            .unwrap();
        assert!(!pre.status.success(), "ref should not exist pre-fetch");

        fetch_for_base(local.path(), Some("origin/feature-x"))
            .await
            .unwrap();

        let post = std::process::Command::new("git")
            .current_dir(local.path())
            .args(["rev-parse", "--verify", "refs/remotes/origin/feature-x"])
            .output()
            .unwrap();
        assert!(post.status.success(), "ref should exist after fetch");
    }
```

- [ ] **Step 2: Run tests; verify they fail to compile**

Run:
```bash
cargo test --lib git::worktree_tests::fetch_for_base 2>&1 | tail -15
```
Expected: compile error — `fetch_for_base` not found.

- [ ] **Step 3: Implement `fetch_for_base` + helper**

In `src/git.rs`, add ABOVE `create_worktree` (around line 284):

```rust
/// List configured remote names for `repo`. Returns an empty Vec if
/// `git remote` fails (e.g. no remotes configured).
pub async fn remote_names(repo: &Path) -> Vec<String> {
    let out = match run(repo, &["remote"]).await {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    out.lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// Fetch the named branch from a remote IF `base` looks like a
/// remote-tracking ref. Heuristic: split on the first `/`; if the
/// prefix matches a configured remote name, run `git fetch <remote>
/// <branch>`. Otherwise no-op. `None`, empty values, and values with
/// no `/` are also no-ops.
///
/// Errors from the fetch itself propagate to the caller so workspace
/// creation can fail fast on bad refs or network issues.
pub async fn fetch_for_base(repo: &Path, base: Option<&str>) -> Result<()> {
    let Some(value) = base else { return Ok(()) };
    let value = value.trim();
    if value.is_empty() {
        return Ok(());
    }
    let Some((prefix, rest)) = value.split_once('/') else {
        return Ok(());
    };
    if rest.is_empty() {
        return Ok(());
    }
    let remotes = remote_names(repo).await;
    if !remotes.iter().any(|r| r == prefix) {
        return Ok(());
    }
    run(repo, &["fetch", prefix, rest]).await?;
    Ok(())
}
```

- [ ] **Step 4: Run tests; verify they pass**

Run:
```bash
cargo test --lib git::worktree_tests::fetch_for_base 2>&1 | tail -15
cargo test --lib git::worktree_tests 2>&1 | tail -10
```
Expected: all 4 new `fetch_for_base_*` tests pass; the earlier `create_worktree_*` tests still pass.

- [ ] **Step 5: Commit**

```bash
git add src/git.rs
git commit -m "$(cat <<'EOF'
feat(git): add fetch_for_base helper (#43)

Decides whether to fetch from a remote based on a configured base
ref. Heuristic: if the prefix before the first `/` matches a
configured remote name (as returned by `git remote`), run
`git fetch <remote> <branch>`. Otherwise no-op.

Lets `origin/main`, `upstream/release`, etc. work transparently
without spurious network calls for bare local refs like `main` or
SHAs.

Also adds `remote_names` as a small public helper consumed by
`fetch_for_base` (and useful on its own).
EOF
)"
```

---

## Task 4: Wire `base_branch` through `workspace::create` (TDD)

`workspace::create` reads `repo.base_branch`, calls `fetch_for_base`, then passes the base through to `create_worktree`. Fetch happens BEFORE the workspace row is inserted, so a fetch failure doesn't leave an orphan `Pending` row.

**Files:**
- Modify: `src/workspace.rs`

- [ ] **Step 1: Write failing test**

In `src/workspace.rs`, inside `#[cfg(test)] mod tests` (around line 198), append after the last existing test:

```rust
    #[tokio::test]
    async fn create_branches_off_configured_base() {
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        // Add a second commit on main so HEAD advances.
        let r = |args: &[&str]| {
            assert!(
                std::process::Command::new("git")
                    .current_dir(repo_dir.path())
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        };
        std::fs::write(repo_dir.path().join("b.txt"), "v1").unwrap();
        r(&["add", "b.txt"]);
        r(&["commit", "-q", "-m", "add b"]);
        let prev_out = std::process::Command::new("git")
            .current_dir(repo_dir.path())
            .args(["rev-parse", "HEAD~1"])
            .output()
            .unwrap();
        let prev_sha = String::from_utf8_lossy(&prev_out.stdout).trim().to_string();
        r(&["branch", "staging", &prev_sha]);

        let id = crate::repo::add(&store, repo_dir.path(), "demo", "")
            .await
            .unwrap();
        store
            .set_repo_base_branch(id, Some("staging"))
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == id)
            .unwrap();
        let wt_root = TempDir::new().unwrap();

        let created = create(&store, &repo, Some("from-staging"), wt_root.path(), false, |_| {})
            .await
            .unwrap();

        let head = std::process::Command::new("git")
            .current_dir(&created.workspace.worktree_path)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let wt_head = String::from_utf8_lossy(&head.stdout).trim().to_string();
        assert_eq!(
            wt_head, prev_sha,
            "workspace should be at staging's commit, not main HEAD"
        );
    }
```

- [ ] **Step 2: Run; verify it fails**

Run:
```bash
cargo test --lib workspace::tests::create_branches_off_configured_base 2>&1 | tail -10
```
Expected: test FAILS at the final `assert_eq!` — the worktree was created off current HEAD (the new commit), not the configured `staging` base. Or it may panic on `set_repo_base_branch` if Task 1 didn't land — confirm Task 1 is committed before proceeding.

- [ ] **Step 3: Wire `base_branch` through `create`**

In `src/workspace.rs`, find `create` (around line 18). Update the body so it reads:

```rust
pub async fn create<F: FnMut(SetupLine) + Send>(
    store: &Store,
    repo: &Repo,
    name: Option<&str>,
    worktree_base: &Path,
    yolo: bool,
    on_setup_line: F,
) -> Result<CreatedWorkspace> {
    let name = match name {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => names::generate(),
    };
    let prefix = crate::repo::resolve_branch_prefix(repo, store)?;
    let branch = if prefix.is_empty() {
        name.clone()
    } else {
        format!("{}/{}", prefix.trim_end_matches('/'), name)
    };
    let worktree_path = worktree_base.join(&repo.name).join(&name);

    let base = repo
        .base_branch
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    // Fetch before inserting the workspace row so a fetch failure
    // (network down, bad remote ref) doesn't leave an orphan Pending row.
    git::fetch_for_base(&repo.path, base).await?;

    let id = store.insert_workspace(&NewWorkspace {
        repo_id: repo.id,
        name: &name,
        branch: &branch,
        worktree_path: &worktree_path,
        yolo,
    })?;

    if let Err(e) = git::create_worktree(&repo.path, &branch, base, &worktree_path).await {
        store.set_workspace_state(id, WorkspaceState::Failed)?;
        return Err(e);
    }
    store.set_workspace_state(id, WorkspaceState::Ready)?;

    let setup_result = setup::run_setup(
        repo.setup_script.as_deref(),
        &repo.path,
        &worktree_path,
        on_setup_line,
    )
    .await?;
    let status = match &setup_result {
        SetupResult::Ok => SetupStatus::Ok,
        SetupResult::Skipped => SetupStatus::Skipped,
        SetupResult::Failed { .. } => SetupStatus::Failed,
    };
    store.set_setup_status(id, status)?;

    let ws = store
        .workspaces(repo.id)?
        .into_iter()
        .find(|w| w.id == id)
        .ok_or_else(|| Error::Store(rusqlite::Error::QueryReturnedNoRows))?;
    Ok(CreatedWorkspace {
        workspace: ws,
        setup_result,
    })
}
```

- [ ] **Step 4: Run tests; verify pass**

Run:
```bash
cargo test --lib workspace::tests 2>&1 | tail -10
```
Expected: all `workspace::tests::*` pass (including the new one and the existing regression tests that verify `base = None` still works).

- [ ] **Step 5: Commit**

```bash
git add src/workspace.rs
git commit -m "$(cat <<'EOF'
feat(workspace): branch new workspaces off repo.base_branch (#43)

When a repo has `base_branch` configured, `workspace::create` now:
- pre-fetches the remote (only if the value's prefix matches a
  configured remote name) before inserting any DB row, so a fetch
  failure doesn't leave an orphan Pending workspace;
- passes the base to `create_worktree`, which forwards it to
  `git worktree add` as an explicit start point.

Repos without `base_branch` set (i.e. all existing repos) keep
today's behavior: fork off whatever HEAD points to.
EOF
)"
```

---

## Task 5: CLI subcommand `wsx repo set-base-branch` (TDD)

Adds `CliAction::RepoSetBaseBranch { name, value }`, a parse arm, a dispatch arm, and parse tests. Mirrors the shape of `RepoSetPrefix`.

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 1: Write failing parse tests**

In `src/cli.rs`, inside the existing `#[cfg(test)] mod tests` block (at the end, before the closing `}`), append:

```rust
    #[test]
    fn parses_repo_set_base_branch_literal() {
        match parse(&["repo", "set-base-branch", "demo", "origin/main"]).unwrap() {
            CliAction::RepoSetBaseBranch { name, value } => {
                assert_eq!(name, "demo");
                assert_eq!(value, "origin/main");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_repo_set_base_branch_empty_value() {
        match parse(&["repo", "set-base-branch", "demo", ""]).unwrap() {
            CliAction::RepoSetBaseBranch { name, value } => {
                assert_eq!(name, "demo");
                assert_eq!(value, "");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }
```

- [ ] **Step 2: Run; verify they fail to compile**

Run:
```bash
cargo test --lib cli::tests::parses_repo_set_base_branch 2>&1 | tail -15
```
Expected: compile error — `CliAction::RepoSetBaseBranch` not defined.

- [ ] **Step 3: Add the `CliAction` variant**

In `src/cli.rs`, in the `pub enum CliAction` block. The simplest placement is right after `RepoSetPrefix` (around line 17-22). The variant:

```rust
    RepoSetBaseBranch {
        name: String,
        value: String,
    },
```

Don't worry about exact position within the enum — just add it; Rust doesn't care.

- [ ] **Step 4: Add the parse arm**

In `src/cli.rs`, find the existing `Some("set-prefix")` arm (around line 158). Add a sibling arm right below it (still inside the `match it.next().as_deref()` inside the `Some("repo")` outer arm):

```rust
            Some("set-base-branch") => {
                let name = it.next().ok_or_else(|| {
                    Error::UserInput("repo set-base-branch <name> <ref-or-empty>".into())
                })?;
                let value = it.next().ok_or_else(|| {
                    Error::UserInput("repo set-base-branch <name> <ref-or-empty>".into())
                })?;
                Ok(CliAction::RepoSetBaseBranch { name, value })
            }
```

- [ ] **Step 5: Add the dispatch arm**

In `src/cli.rs`, in `run_cli` (around line 326), find the `CliAction::RepoSetPrefix` arm. Add a sibling arm right below it:

```rust
        CliAction::RepoSetBaseBranch { name, value } => {
            let repos = crate::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let trimmed = value.trim();
            if trimmed.is_empty() {
                store.set_repo_base_branch(r.id, None)?;
                println!("cleared base branch for {name} (using current HEAD)");
            } else {
                store.set_repo_base_branch(r.id, Some(trimmed))?;
                println!("set base branch for {name} to {trimmed}");
            }
        }
```

- [ ] **Step 6: Run tests; verify pass**

Run:
```bash
cargo test --lib cli::tests 2>&1 | tail -10
cargo build 2>&1 | tail -5
```
Expected: all `cli::tests::*` pass (including the 2 new ones); clean build.

- [ ] **Step 7: Live sanity check**

```bash
cargo run -- repo set-base-branch nonexistent origin/main 2>&1 | tail -3
# expected: error mentioning "no repo named nonexistent"

# If you have a repo registered locally, you can also try:
# cargo run -- repo list
# cargo run -- repo set-base-branch <real-name> origin/main
# cargo run -- repo set-base-branch <real-name> ""
```

- [ ] **Step 8: Commit**

```bash
git add src/cli.rs
git commit -m "$(cat <<'EOF'
feat(cli): add \`wsx repo set-base-branch\` (#43)

\`wsx repo set-base-branch <name> <ref>\` sets the per-repo base
branch; empty value clears it (restoring default of branching off
current HEAD). Mirrors the shape of \`set-prefix\`.

The setting is per-repo state, so it does NOT appear in
\`known_setting_key\` — \`wsx config set/get\` does not address it.
EOF
)"
```

---

## Task 6: TUI — add Base branch row in repo-settings modal

Adds `RepoSettingField::BaseBranch`, extends `ALL` to 7, adds the label, the value lookup, the dispatch, and the modal row.

**Files:**
- Modify: `src/app.rs`
- Modify: `src/ui/modal.rs`

- [ ] **Step 1: Extend the enum and the `ALL` constant**

In `src/app.rs`, find `pub enum RepoSettingField` (around line 38). Add `BaseBranch` as a new variant. After this edit, the enum and its `ALL` array become:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoSettingField {
    BranchPrefix,
    BaseBranch,
    CustomInstructions,
    SetupScript,
    ArchiveScript,
    PinnedCommands,
    RelatedRepos,
}

impl RepoSettingField {
    pub const ALL: [Self; 7] = [
        Self::BranchPrefix,
        Self::BaseBranch,
        Self::CustomInstructions,
        Self::SetupScript,
        Self::ArchiveScript,
        Self::PinnedCommands,
        Self::RelatedRepos,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::BranchPrefix => "branch_prefix",
            Self::BaseBranch => "base_branch",
            Self::CustomInstructions => "custom_instructions",
            Self::SetupScript => "setup_script",
            Self::ArchiveScript => "archive_script",
            Self::PinnedCommands => "pinned_commands",
            Self::RelatedRepos => "related_repos",
        }
    }
}
```

(Putting `BaseBranch` right after `BranchPrefix` keeps related settings adjacent. Adjust if you have a preference.)

- [ ] **Step 2: Update the editor value-for-field match**

In `src/app.rs`, find the `match edit.field` block (around line 415-432) inside the editor function. Add a new arm for `BaseBranch`:

```rust
            RepoSettingField::BranchPrefix => (repo.branch_prefix.clone(), "txt"),
            RepoSettingField::BaseBranch => (repo.base_branch.clone().unwrap_or_default(), "txt"),
            RepoSettingField::CustomInstructions => {
                (repo.custom_instructions.clone().unwrap_or_default(), "md")
            }
```

- [ ] **Step 3: Update the dispatch in `apply_repo_setting`-style fn**

In `src/app.rs`, find the `match field { ... }` block (around line 1492) that maps `RepoSettingField` to `store.set_repo_*` calls. Add a new arm. The block becomes:

```rust
    match field {
        RepoSettingField::BranchPrefix => app.store.set_repo_branch_prefix(repo_id, trimmed),
        RepoSettingField::BaseBranch => app.store.set_repo_base_branch(repo_id, opt),
        RepoSettingField::CustomInstructions => {
            app.store.set_repo_custom_instructions(repo_id, opt)
        }
        RepoSettingField::SetupScript => app.store.set_repo_setup_script(repo_id, opt),
        RepoSettingField::ArchiveScript => app.store.set_repo_archive_script(repo_id, opt),
        RepoSettingField::PinnedCommands => app.store.set_repo_pinned_commands(repo_id, opt),
        RepoSettingField::RelatedRepos => app.store.set_repo_related_repos(repo_id, opt),
    }
```

Note: `BaseBranch` uses `opt` (the `Option<&str>` already prepared above this match), not `trimmed`. Confirmed by re-reading lines 1485-1502: `BranchPrefix` is special-cased because its setter takes `&str`; all the others take `Option<&str>`. `BaseBranch` follows the latter.

- [ ] **Step 4: Update `render_repo_settings` to 7 rows**

In `src/ui/modal.rs` (around line 449), the rows array is declared as `[(RepoSettingField, Option<&str>); 6]`. Change the length to 7 and insert a new entry for `BaseBranch`. The whole block becomes:

```rust
    let rows: [(crate::app::RepoSettingField, Option<&str>); 7] = [
        (
            crate::app::RepoSettingField::BranchPrefix,
            if repo.branch_prefix.is_empty() {
                None
            } else {
                Some(repo.branch_prefix.as_str())
            },
        ),
        (
            crate::app::RepoSettingField::BaseBranch,
            repo.base_branch.as_deref(),
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
        (
            crate::app::RepoSettingField::RelatedRepos,
            repo.related_repos.as_deref(),
        ),
    ];
```

- [ ] **Step 5: Build + run all tests**

```bash
cargo build 2>&1 | tail -5
cargo test --lib 2>&1 | tail -10
```

Expected: clean build; all `store::tests`, `git::worktree_tests`, `workspace::tests`, `cli::tests` pass. Pre-existing intermittent flakes in `external::tests::editor_*`, `pty::session::tests::kill_all_*`, and `pm::tests::*resume*` are environmental — if they show up in the full run, verify each in isolation:

```bash
cargo test --lib external::tests::editor_falls_back_to_env
# etc.
```

If they pass standalone, treat the full-suite "fail" as noise.

- [ ] **Step 6: Run rustfmt**

```bash
cargo fmt --check 2>&1 | head -5
```

If any drift, fix it:

```bash
cargo fmt
git status --porcelain  # see what fmt changed
```

If `cargo fmt` modified files, include them in the commit below.

- [ ] **Step 7: Manual TUI smoke (optional but recommended)**

```bash
cargo build --release 2>&1 | tail -3
./target/release/wsx
# Open the dashboard, focus a registered repo, press 'r' (or whatever
# key opens repo settings — check the footer hint). Verify there are
# 7 rows and the second is "base_branch". Cursor-navigate to it,
# press enter, edit it, save, press 'd' to clear, press 'esc' to close.
```

If the modal layout looks broken (clipped, mis-sized), the height constraint at `src/ui/modal.rs:430` (`h = area.height.clamp(8, 16)`) may need bumping to 17. Default is fine for 7 rows + header + footer = 9 lines, well under 16. No change expected.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
feat(tui): show + edit base_branch in repo-settings modal (#43)

Extends \`RepoSettingField\` with \`BaseBranch\`; the repo-settings
modal now shows 7 rows, with \`base_branch\` listed right under
\`branch_prefix\`. Edit and clear use the existing single-line
editor flow.

Closes #43.
EOF
)"
```

---

## Done. Final verification

After all 6 tasks:

```bash
cargo build --release 2>&1 | tail -5
cargo fmt --check 2>&1 | tail -3
# Run test groups individually (cargo test takes only one positional filter):
cargo test --lib store::tests 2>&1 | tail -3
cargo test --lib git::worktree_tests 2>&1 | tail -3
cargo test --lib workspace::tests 2>&1 | tail -3
cargo test --lib cli::tests 2>&1 | tail -3
```

Expected:
- Release build clean.
- `cargo fmt --check` clean.
- All four test groups green.

Then a manual sanity check on a real repo:

```bash
./target/release/wsx repo list
./target/release/wsx repo set-base-branch <name> origin/main
# Open the TUI, create a new workspace, verify it branched off origin/main
./target/release/wsx repo set-base-branch <name> ""
# Create another workspace, verify it branched off current HEAD
```
