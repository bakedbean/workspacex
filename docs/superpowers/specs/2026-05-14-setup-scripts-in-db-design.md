# Move setup/archive scripts from `.claudette.json` into the database

## Background

Each `wsx` repo currently honors a `.claudette.json` file at the repo root,
which carries two structured `ScriptSpec`s — `setup` (run when a workspace is
created) and `archive` (run when a workspace is removed). The format is a
holdover from the original Claudette tool. wsx has since accumulated its own
per-repo settings: `branch_prefix` and `custom_instructions` are stored as
nullable columns on the `repos` table and edited via the `wsx repo …` CLI.

Keeping a second source of truth in `.claudette.json` adds:

- a JSON loader, a `ScriptSpec`/`RepoConfig` data model, and a serde dep usage
  that is otherwise unneeded in `setup.rs`,
- a file-system read on every workspace create/archive,
- a divergence from how every other per-repo knob is configured.

## Goal

Replace `.claudette.json` entirely with two per-repo columns on `repos`,
configured through the same CLI surface that already exists for other per-repo
settings. Drop all support for the old file format — no automatic migration.

## Non-goals

- A global fallback for setup/archive scripts. They are per-repo only.
- A structured `env{}` field. Users put `FOO=bar` inline in the shell string.
- A migration path from existing `.claudette.json` files. They are silently
  ignored after this change ships.

## Decisions

| Question | Choice |
|---|---|
| Where do scripts live? | Per-repo columns on `repos` — no global fallback. |
| Value format | Single shell string, run as `sh -c "$value"`. |
| Both setup and archive? | Yes, both. |
| Migration | None. `.claudette.json` is silently ignored. |

## Schema

Bump `user_version` to 3. Add two nullable TEXT columns to `repos`:

```sql
ALTER TABLE repos ADD COLUMN setup_script   TEXT;
ALTER TABLE repos ADD COLUMN archive_script TEXT;
```

Migration guards each `ALTER` with the same `pragma_table_info` count check
already used in the v2 step for `custom_instructions`, so partial-migration
retries are safe.

NULL → "no script" → `SetupResult::Skipped` (existing variant). The CLI
handlers are responsible for choosing `None` vs `Some(&value)` based on
`trim().is_empty()` (mirroring `RepoSetInstructions`); the store setter is a
pass-through.

## Data model (`src/store.rs`)

Extend the `Repo` struct:

```rust
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

Update the `SELECT` in `repos()` and the row mapping. Add two setters that
mirror `set_repo_custom_instructions`:

```rust
pub fn set_repo_setup_script(&self, id: RepoId, value: Option<&str>) -> Result<()>;
pub fn set_repo_archive_script(&self, id: RepoId, value: Option<&str>) -> Result<()>;
```

Both are pass-through: `Some("...")` writes the string as-is, `None` writes
SQL NULL. Normalization of empty/whitespace input lives in the CLI layer.

## Execution (`src/setup.rs`)

Delete:

- `ScriptSpec`, `RepoConfig`, `load_repo_config`,
- the `Deserialize` derives,
- the `tests` module (file-based fixtures) and the two `run_tests` cases that
  write `.claudette.json` fixtures.

Replace the two public entry points so they take the script from the caller
instead of reading it from disk:

```rust
pub async fn run_setup<F: FnMut(SetupLine) + Send>(
    script: Option<&str>,
    repo_root: &Path,
    worktree: &Path,
    on_line: F,
) -> Result<SetupResult>;

pub async fn run_archive<F: FnMut(SetupLine) + Send>(
    script: Option<&str>,
    repo_root: &Path,
    worktree: &Path,
    on_line: F,
) -> Result<SetupResult>;
```

Both delegate to one private `run_script(sh_command: &str, repo_root, worktree,
on_line)`. Execution invokes `sh -c "$script"` with:

- `cwd` = worktree,
- env: `WSX_REPO_ROOT`, `WSX_WORKTREE`,
- stdout/stderr streamed line-by-line into the closure (unchanged).

Skipped path: if `script` is `None` or trims to empty, return
`SetupResult::Skipped` without spawning a process.

`SetupResult` and `SetupLine` are unchanged.

## Workspace lifecycle (`src/workspace.rs`)

- `create` passes `repo.setup_script.as_deref()` to `setup::run_setup`.
- `archive` passes `repo.archive_script.as_deref()` to `setup::run_archive`.

No new I/O. `Repo` already carries the script values from the store.

The existing `create_records_setup_failure_but_keeps_workspace_ready` test is
updated to set `setup_script` on the row via the new store setter (then reload
the `Repo`), instead of writing a `.claudette.json` fixture.

## CLI (`src/cli.rs`)

Four new actions, all under the existing `wsx repo` subcommand. They mirror
`RepoSetInstructions` exactly in shape:

```
wsx repo set-setup    <name> <value-or-@file>
wsx repo set-archive  <name> <value-or-@file>
wsx repo edit-setup   <name>
wsx repo edit-archive <name>
```

`set-*` accepts the same `@file` convention as `repo set-instructions` (via
`ValueSource::from_arg`). Empty or whitespace-only resolved value clears the
column with `"cleared setup for <name>"`; non-empty sets it with `"set setup
for <name> (<N> chars)"`.

`edit-*` reuses the existing `open_in_editor` helper (already factored out for
`config edit`). After the editor exits, the resulting value is trimmed of
trailing newlines; if it's then empty, the column is cleared; if unchanged,
print `"<key> unchanged"`; otherwise save. Same behavior as `config edit`.

`known_setting_key` is NOT modified. These are per-repo columns, not entries in
the global `settings` table — they sit alongside `branch_prefix` and
`custom_instructions`, which also live on `repos` and which are also excluded
from `known_setting_key`.

## Error handling

| Condition | Result |
|---|---|
| `script` is `None` or whitespace | `SetupResult::Skipped`, no process spawned |
| `sh` spawn failure | `Error::Setup(...)` (unchanged wrapping) |
| Non-zero exit (incl. shell syntax errors → exit 2) | `SetupResult::Failed { exit_code }`; workspace stays `Ready` with `[setup-failed]` badge |
| stdout/stderr I/O error | `Error::Setup(...)` (unchanged) |

The `Error::Setup(".claudette.json parse: …")` message is gone, but the
`Error::Setup` variant stays — it's still produced by spawn/wait/read failures.

## Tests

### `src/setup.rs`

Drop all existing tests in this module. Replace with direct-input tests on
`run_script` / `run_setup`:

- `None` → `Skipped`
- `Some("")` and `Some("   ")` → `Skipped`
- `Some("echo hi; echo bye 1>&2")` → `Ok`, stdout has `hi`, stderr has `bye`
- `Some("exit 7")` → `Failed { exit_code: 7 }`
- `Some("echo $WSX_WORKTREE")` → `Ok`, stdout contains the worktree path
- `Some("not-a-real-command")` → `Failed { exit_code: 127 }` (sh's
  "command not found")

Repeat the success/failure/skipped cases for `run_archive` to confirm the
parameter plumbing.

### `src/store.rs`

Add a round-trip test for setup/archive columns:

- New repo → both columns `None`.
- `set_repo_setup_script(id, Some("bun install"))` → reads back as
  `Some("bun install")`.
- `set_repo_setup_script(id, Some("   "))` → reads back as `None`.
- `set_repo_setup_script(id, None)` → reads back as `None`.
- Same for archive_script.
- The v3 migration is idempotent (calling `migrate` twice does not error).

### `src/cli.rs`

Parse tests for the four new subcommands, mirroring the existing
`set-instructions` test:

- `repo set-setup demo 'bun install'` → `RepoSetSetup { name: "demo",
  source: Literal("bun install") }`
- `repo set-setup demo @./script.sh` → `source: File(PathBuf::from("./script.sh"))`
- `repo set-archive demo 'rm -rf node_modules'` → `RepoSetArchive { ... }`
- `repo edit-setup demo` → `RepoEditSetup { name: "demo" }`
- `repo edit-archive demo` → `RepoEditArchive { name: "demo" }`
- Missing-name errors handled like the existing variants.

### `src/workspace.rs`

- Rewrite `create_records_setup_failure_but_keeps_workspace_ready` to set
  `setup_script` on the row (no JSON fixture).
- Add `create_runs_setup_script_when_set`: writes a `setup_script` that
  produces a marker file, asserts `SetupStatus::Ok` and that the marker exists.
- Add an `archive` test that runs `archive_script` similarly.

All tests use `tempfile::TempDir` and `sh -c` — no Claude binary needed,
consistent with the existing test style.

## README

Replace the "Per-repo setup scripts" section. The new copy:

- Documents `wsx repo set-setup <name> <value-or-@file>` and `set-archive`,
  plus the `edit-setup` / `edit-archive` editor flow.
- States that the value is executed as `sh -c "$value"` with
  `cwd = <worktree>` and the two env vars `WSX_REPO_ROOT` / `WSX_WORKTREE`.
- Notes that failures surface as the `[setup-failed]` badge on the dashboard
  (unchanged from today).
- Removes the JSON example and the file-format description.

No mention of `.claudette.json` in the README after this change.

## Out-of-scope follow-ups

- A `wsx repo show <name>` summary command that prints all per-repo columns
  (branch_prefix, custom_instructions, setup_script, archive_script) in one
  view. Useful but separate.
- A TUI-side editor for setup/archive (the editor flow stays CLI-only here).

## Risks

- **Silent regression for any user with a `.claudette.json`.** Mitigation: the
  README change calls out the new CLI, and the file is rare in practice (this
  is an early-stage tool). No deprecation warning is logged — explicit per the
  decision above.
- **Shell-quoting footguns.** Users who previously had `args` with embedded
  spaces will need to quote them in the shell string. The `@file` and
  `edit-*` flows reduce this friction.
