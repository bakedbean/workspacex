# Related repos — Design

**Issue:** [#41](https://github.com/bakedbean/workspacex/issues/41)

## Goal

Let the user declare related repos per primary repo. When a workspace is spawned in the primary repo, wsx automatically tells claude about the related repos — both giving it `--add-dir` read access to their source paths AND instructing it (via system prompt) not to write to them. If claude needs to make changes in a related repo, the instruction is to open a new wsx workspace in that repo instead.

Solves the "I have a backend repo and a frontend repo and I need to mention frontend every time I start a workspace" friction.

## Approach

Per-repo config field `related_repos` holds a comma-separated list of **other wsx-registered repo names**. At spawn time, wsx resolves each name to its source path (or skips it with a warning), then:

1. Appends `--add-dir <path>` to the claude invocation for each resolved path.
2. Appends a system-prompt fragment to claude (via the existing `--append-system-prompt` pipeline) naming each path and instructing read-only access.

Using **names** rather than paths means renames of the related repo are auto-resolved at spawn time and the config is portable (no `/home/eben/...` baked in). Resolution misses are logged but don't block spawn.

## Decisions

### Data model

- **New column** on `repos` table: `related_repos TEXT NULL` (Schema v6 migration, idempotent ALTER guard matching the existing pattern).
- **`Repo` struct** gains `pub related_repos: Option<String>` (between `pinned_commands` and `created_at`).
- **Format:** comma-separated names. Whitespace around commas is trimmed; blank entries dropped. Empty value or NULL → no related repos.
- **New setter:** `Store::set_repo_related_repos(&self, id: RepoId, value: Option<&str>) -> Result<()>` matching the existing `set_repo_pinned_commands` shape.

### New `src/related.rs` module

```rust
pub fn parse(spec: &str) -> Vec<String>;
//   ↑ comma-split, trim, skip blanks. Pure.

pub fn resolve(spec: Option<&str>, all_repos: &[Repo]) -> Vec<(String, PathBuf)>;
//   ↑ For each name in spec, find the matching Repo by name and return
//     (name, repo.path). Names with no match are tracing::warn!'d and
//     skipped. Empty spec / None / no matches → empty Vec.

pub fn build_read_only_prompt(resolved: &[(String, PathBuf)]) -> Option<String>;
//   ↑ Returns None when resolved.is_empty(). Otherwise returns the
//     paragraph documented in the System prompt section below.
```

Keeping these as free functions in a dedicated module mirrors the `src/pinned.rs` precedent (small, single-purpose, well-tested).

### System prompt

When `related` is non-empty, append this fragment to claude's system prompt via `--append-system-prompt`:

```
The following directories were added via --add-dir for read-only
reference. They are the source paths of related wsx-managed repos:
  - /home/eben/work/frontend (wsx repo: frontend)
  - /home/eben/work/marketing (wsx repo: marketing)

You MUST NOT edit files in these directories. They may be on
different branches, have unstaged changes, or belong to other
active work. If you need to make changes in a related repo, tell
the user to create a new wsx workspace for it (via the wsx
dashboard's [n] keybind, or `wsx workspace create <repo>`) and
switch to that session — then come back here when done.

Read, grep, reference, and quote freely from these paths. Just
don't write to them.
```

The fragment is concatenated with the existing custom_instructions string (with a blank line separator) before being passed to `--append-system-prompt`, so claude sees one combined system prompt.

**Enforcement note:** the read-only rule is instructional, not tool-level. wsx trusts claude to follow it, the same way it trusts the rename-prompt and custom_instructions today. If claude misbehaves and writes to a related dir, it surfaces as normal `git status` dirt in that repo — documented limitation.

### Spawn-time plumbing

- **`SpawnMode`** (in `src/pty/session.rs`) gains an `additional_dirs: Vec<PathBuf>` field on every variant (`Continue`, `Fresh`, `ProjectManager`). Rationale: matches how `custom_instructions` and `yolo` are already carried — keeps related spawn-time configuration in one place rather than threading new function parameters through every spawn caller.
- **`build_claude_command`** destructures `additional_dirs` from the mode and emits `cmd.arg("--add-dir"); cmd.arg(path);` per entry, placed before the existing `--continue` / `--append-system-prompt` args. The function signature itself is unchanged.
- **`build_spawn_info` in `src/app.rs`** owns the resolution + folding work: it calls `related::resolve` against `app.repos`, filters self-references, populates `SpawnMode.additional_dirs`, and folds the read-only prompt fragment from `build_read_only_prompt` into `custom_instructions` (which `build_claude_command` then combines with the rename prompt and passes via `--append-system-prompt`).
- **PM session spawn**: `SpawnMode::ProjectManager.additional_dirs` is always empty. PM has no owning repo, so no related repos resolve.

### CLI surface

Following the established pattern (`set-setup` / `set-pinned-commands`):

```bash
wsx repo set-related-repos <name> <value-or-@file>
wsx repo edit-related-repos <name>           # opens $EDITOR
```

New `CliAction` variants:
- `RepoSetRelatedRepos { name: String, source: ValueSource }`
- `RepoEditRelatedRepos { name: String }`

Empty value (or `""`) clears the field.

### Repo Settings modal

Add `RepoSettingField::RelatedRepos` as the 6th field. Updates required:
- Enum + `ALL: [Self; 6]` + `label()` arm
- `do_pending_edit` arm: `(repo.related_repos.clone().unwrap_or_default(), "txt")`
- `apply_repo_setting` arm: `Store::set_repo_related_repos(repo_id, opt)`
- Modal `.min(N)` clamp bumps from 4 → 5 (both sites)
- `render_repo_settings` rows array: `; 6]` + new row

### Resolution semantics

| Input | Output |
|---|---|
| Unset / NULL / empty after trim | Empty Vec — spawn args identical to today |
| `"frontend"` and `frontend` is registered | `[("frontend", /path/to/frontend)]` |
| `"frontend, marketing"` both registered | both, in input order |
| `"frontend, ghost"` and `ghost` is unregistered | `[("frontend", ...)]` + `tracing::warn!` for `ghost` |
| `"ghost"` only, unregistered | Empty Vec + warn |
| `","` or whitespace-only | Empty Vec, no warn (parser skips blanks) |

### Live reload caveat

- The `repos` cache in `App` (used at spawn time via `build_spawn_info`) refreshes on explicit user actions (workspace create/archive, dashboard interactions). Editing `related_repos` via `wsx repo set-related-repos` from another shell takes effect for the NEXT workspace spawn that occurs after a refresh-triggering action — not necessarily immediately.
- Already-running claude sessions don't pick up changes regardless — claude args are spawn-time only.
- This matches how `pinned_commands` per-repo and `custom_instructions` per-repo already behave; documented but not "fixed."

## Scope

### In

1. New `src/related.rs` module: `parse`, `resolve`, `build_read_only_prompt`.
2. Schema v6 migration: `ALTER TABLE repos ADD COLUMN related_repos TEXT`.
3. `Repo` struct + `repos()` SELECT/mapper + `set_repo_related_repos` setter.
4. `build_claude_command` extended with `related` parameter; spawn callers updated.
5. New CLI: `repo set-related-repos`, `repo edit-related-repos`.
6. `RepoSettingField::RelatedRepos` in the Repo Settings modal.
7. README: per-repo CLI block + short subsection explaining the feature.
8. Tests for parser, resolver, prompt-builder, spawn-arg integration.

### Out

- **Per-workspace overrides.** Per-repo is the right granularity for "this repo's workspaces all need these refs."
- **Bidirectional linking.** If `backend.related_repos = "frontend"`, that doesn't auto-add `backend` to `frontend.related_repos`. User declares both sides if they want both.
- **Per-related-repo configuration** (e.g. "read-only" vs "writable"). For v1, all related repos are read-only. If writable mode is ever desired, the user can spawn a workspace in that repo directly.
- **Including active worktrees of related repos**. Only the source repo path is `--add-dir`'d. Worktrees of related repos are not auto-included.
- **Auto-detection / suggestion** of related repos (e.g. by git remote analysis). Manual list, no inference.
- **TUI feedback for missing references.** Skipped names log to `wsx.log` only; the dashboard doesn't surface stale references. Could be a follow-up if it becomes a friction point.
- **Global default** for related_repos. Per-repo only — global doesn't make semantic sense here.

## Acceptance criteria

- `wsx repo set-related-repos backend "frontend,marketing"` persists the column.
- Spawning a workspace in `backend` invokes `claude ... --add-dir /path/to/frontend --add-dir /path/to/marketing` AND the appended system prompt contains the read-only fragment naming both paths.
- Unknown name in the list → `tracing::warn!` line, skipped silently, spawn proceeds with the recognized names.
- Empty/unset `related_repos` → no `--add-dir` args, no read-only fragment in the prompt; spawn invocation is byte-identical to today.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, full test suite green.
