# Per-repo base branch for new workspaces — Design

**Issue:** [#43](https://github.com/bakedbean/workspacex/issues/43)

## Goal

Let users configure, per repo, the git ref new workspaces branch from. Today, every new workspace is created via `git worktree add -b <branch> <path>` with no explicit base, so it forks off whatever HEAD currently points to in the main worktree. That's surprising when the user has a non-default branch checked out — `wsx` should let them pin "always branch off `origin/main`" (or any other ref) per repo.

When unset, behavior is unchanged.

## Approach

Add a nullable `base_branch` column to the `repos` table. `workspace::create` reads it, applies a small "is this a remote-tracking ref?" heuristic to decide whether to fetch first, then passes the resolved base to `git worktree add`. CLI and TUI gain one new field each, in the same shape as the existing `branch_prefix`/`setup_script` controls.

## Decisions

### Storage

New nullable column on `repos`:

```sql
ALTER TABLE repos ADD COLUMN base_branch TEXT;
```

Schema bump v6 → v7. Migration follows the established `pragma_table_info`-guarded pattern (`store.rs:88-167`) — the bump is idempotent and re-runnable.

Struct changes:

- `Repo` (in `src/store.rs:29`) gains `pub base_branch: Option<String>`.
- `Store` gains `pub fn set_repo_base_branch(&self, id: RepoId, value: Option<&str>) -> Result<()>` mirroring the shape of `set_repo_setup_script`.

### Workspace-create logic

`workspace::create` (in `src/workspace.rs:18`) is updated to compute a base from the repo row:

```
let base: Option<&str> = repo.base_branch.as_deref().filter(|s| !s.trim().is_empty());
git::fetch_for_base(&repo.path, base).await?;
git::create_worktree(&repo.path, &branch, base, &worktree_path).await?;
```

`git::create_worktree` (in `src/git.rs:284`) gains an `Option<&str>` `base` parameter:

- `None` → `git worktree add -b <branch> <path>` (current behavior).
- `Some(b)` → `git worktree add -b <branch> <b> <path>`.

If git rejects the base (typo, missing ref, etc.), the existing failure path applies: the inserted workspace row is set to `Failed`, the error propagates.

### Fetch heuristic

A new helper `git::fetch_for_base(repo_path, base) -> Result<()>` decides whether to fetch:

- If `base` is `None` or empty after trim → no-op.
- Split `base` on the first `/`. If no `/` → no-op.
- Run `git remote` (cached for the call). If the prefix before the first `/` matches a configured remote name, run `git fetch <remote> <branch-rest>`. The function awaits the fetch — workspace creation blocks on it.
- If the prefix does NOT match any configured remote → no-op (it's a local branch with a slash in its name, e.g. `feature/login`).

Why this shape: lets `origin/main`, `upstream/release`, `mine/wip` all work without hardcoding "origin"; lets bare `main`, `develop`, `abc123` SHAs all work without spurious network calls.

If `git fetch` itself fails (network down, ref doesn't exist on remote), the error propagates to the create path. The workspace row is left in `Failed`. The user can retry after fixing connectivity or correcting the configured base.

### CLI

One new subcommand following the pattern of `repo set-prefix`:

```
wsx repo set-base-branch <repo> <ref>     # set
wsx repo set-base-branch <repo> ""        # clear (empty value = unset)
```

`""` clears the column, restoring default behavior (branch off current HEAD). This matches how `set-prefix` handles empty: it sets the per-repo override to empty so the global default applies.

`base_branch` is NOT added to `known_setting_key` in `src/cli.rs` — it's per-repo state, not a global setting.

### TUI

`render_repo_settings` (in `src/ui/modal.rs:421`) currently shows 6 rows. Add a 7th: **Base branch**. Steps:

1. Add `BaseBranch` to the `RepoSettingField` enum in `src/app.rs:38`.
2. Implement `label()` for the new variant returning `"Base branch"`.
3. Extend the `editable_value_for_field`-style match (around `src/app.rs:416`) and the edit dispatch (around `src/app.rs:1493`) to handle the new variant — same shape as `BranchPrefix`, which uses single-line edit and `set_repo_branch_prefix`.
4. Extend the 6-element array in `src/ui/modal.rs:449` to 7 elements, with `repo.base_branch.as_deref()` as the value.

The footer hint (`[↑/↓] move [enter] edit [d] clear [esc] close`) stays unchanged.

### Module layout

- **`src/store.rs`** — column added in `migrate`; `Repo` struct gains the field; `repos()` SELECT updated; new `set_repo_base_branch` method.
- **`src/git.rs`** — `create_worktree` signature gains `base: Option<&str>`; new `fetch_for_base` helper alongside it.
- **`src/workspace.rs`** — `create` passes `repo.base_branch` to git layer.
- **`src/cli.rs`** — new `CliAction::RepoSetBaseBranch { name, value }` variant + parse arm + dispatch arm (3 small additions, mirroring `RepoSetPrefix`).
- **`src/app.rs`** — new `RepoSettingField::BaseBranch` variant + the edit handler routing.
- **`src/ui/modal.rs`** — 7th row in the settings modal.

### Tests

**Storage:**

- `store::tests::migration_v7_adds_base_branch_column` — open a store at user_version 6 (or fresh), verify column exists after migrate, no-op on second migrate.
- `store::tests::set_repo_base_branch_roundtrip` — set, read back; clear (None), read back as `None`; clear via empty string treats it as `None`.

**Git layer:**

- `git::tests::create_worktree_with_explicit_base` — set up a repo with two commits on `main`, branch `staging` pointing at the older one, call `create_worktree(..., Some("staging"), ...)`, assert HEAD of new worktree matches the staging commit (not main's HEAD).
- `git::tests::fetch_for_base_fetches_when_prefix_matches_remote` — create a local "remote" via `file://` + a remote pointing at it, add a branch to the remote that doesn't exist locally, call `fetch_for_base(repo, Some("origin/newbranch"))`, assert `refs/remotes/origin/newbranch` is present after the call.
- `git::tests::fetch_for_base_no_op_when_prefix_does_not_match_remote` — base = `feature/foo`, no remote named `feature`, assert no fetch happens (no network) and the call succeeds.
- `git::tests::fetch_for_base_no_op_when_no_slash` — base = `main`, no fetch.
- `git::tests::fetch_for_base_no_op_when_unset` — base = `None`, no fetch.

**Workspace:**

- `workspace::tests::create_branches_off_configured_base` — repo with `main` advanced one commit, `staging` branch at older commit, configure repo `base_branch = "staging"`, create workspace, assert resulting branch's commit == staging's commit.
- The existing `create_makes_worktree_and_inserts_row` test stays as the `base_branch = None` regression case (verifies default behavior unchanged).

**CLI:**

- `cli::tests::parses_repo_set_base_branch` — verify arg parsing.

**TUI:** No new test — the modal renderer is exercised by the existing snapshot/test infrastructure (or not, depending on what's there); follow the pattern used by `pinned_commands` and `related_repos` rows which were added under similar specs.

## What's intentionally NOT included

- **No per-workspace override.** The issue says repo-level.
- **No always-fetch / never-fetch toggle.** The heuristic IS the policy.
- **No pre-flight validation** of the configured base (e.g. warning when it doesn't resolve). The natural failure on `worktree add` is enough — bad refs produce a clear git error.
- **No automatic `origin/HEAD` resolution.** Users who want that write `origin/main` (or whichever) explicitly. The existing `resolve_base_branch` helper in `src/git.rs:66` is for diff comparisons and stays untouched.
- **No `edit-base-branch` interactive subcommand.** Single-line value; `set-base-branch` is enough.
- **No backfill of existing repos.** New column starts as NULL for every existing repo; behavior unchanged for them.
