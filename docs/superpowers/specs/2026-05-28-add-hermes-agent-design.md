# Add Hermes Agent Support — Design

## Problem

wsx supports two coding-agent harnesses today: Claude Code and Pi. The user wants a third — Hermes Agent ([nousresearch/hermes-agent](https://github.com/nousresearch/hermes-agent)) — wired through with full feature parity to the Pi integration. Hermes is installed locally and configured with deepseek as its provider.

## Goal

Add `AgentKind::Hermes` such that `wsx workspace create --agent hermes` spawns a `hermes chat` session in the worktree with the same six features the Pi integration provides:

1. **Spawn** in the worktree's cwd.
2. **`--continue`** equivalent that resumes the right session for *this* worktree.
3. **Model / provider override** via env vars.
4. **Yolo** (skip approvals) when the user requests it.
5. **Auto-rename system prompt** on fresh worktrees, so the agent renames the placeholder `bakedbean/<adjective>-<plant>` branch based on the first user message.
6. **Prior-session indicator** on the dashboard.

## Non-goals

- A general agent-adapter trait refactor. The codebase tolerates per-agent parallel command builders today (Claude and Pi already triplicate similar code paths); doing it a third time is cheaper than the refactor at N=3.
- Supporting Hermes-specific flags that have no analog in the existing wsx model (`--worktree`, `--accept-hooks`, `--skills`, `--checkpoints`, `--max-turns`, `--pass-session-id`, `--tui`, `--dev`). These can be added later if needed.
- Surfacing Hermes's bare-binary REPL mode (`hermes` without subcommand). We invoke `hermes chat` exclusively so we have access to `--source` for session tagging.
- Migrating existing Claude/Pi tests to be agent-parameterized. Hermes gets its own family of tests; existing tests stay focused on the agent they were written for.

## Approach

Add an `AgentKind::Hermes` variant and a parallel `build_hermes_command` alongside `build_pi_command` and `build_claude_command`. Compensate for Hermes's two missing capabilities — no `--append-system-prompt` flag, and no per-cwd session storage in its sqlite db — with two helpers:

- **AGENTS.md prompt injection.** Hermes auto-loads `AGENTS.md` from cwd. wsx writes a fenced wsx-managed block into that file containing the rename prompt, custom instructions, or PM system prompt as appropriate. The block is idempotent-rewritten on every spawn based on the current `SpawnMode`, so no cleanup logic is needed.
- **Source-tagged sqlite query.** Hermes's `--source` flag is a free-text label stored in `sessions.source` and indexed. wsx passes `--source wsx:<encoded-cwd>` on every spawn, then reads `~/.hermes/state.db` directly (with `mode=ro&immutable=1`) to look up the most recent session for the current cwd. This drives both the prior-session indicator and the `--resume <id>` flag on Continue spawns.

## Architecture and touchpoints

| Site | File | Edit |
|---|---|---|
| Variant | `src/pty/session.rs` (`enum AgentKind`, ~line 16) | Add `Hermes` variant. |
| Setting parse | `src/pty/session.rs` (`AgentKind::from_store`, ~line 22) | Add `Some("hermes") => AgentKind::Hermes` arm above the default. |
| Spawn builder | `src/pty/session.rs` | New `build_hermes_command(cwd, mode, _remote) -> CommandBuilder`. |
| Prompt injection | `src/pty/session.rs` | New `prepare_hermes_workspace(cwd, mode)` — writes/rewrites the wsx block in `AGENTS.md`, ensures git exclude. |
| Session lookup | `src/pty/session.rs` | New `latest_hermes_session_id(db_path, worktree) -> Option<String>` (parameterized on db path for testability) + public wrapper resolving `~/.hermes/state.db`. |
| Prior-session detect | `src/pty/session.rs` (`has_prior_session_for`, ~line 281) | Add `AgentKind::Hermes => has_prior_hermes_session(worktree)` arm. |
| Dispatcher | `src/pty/session.rs` (~line 609) | Add `AgentKind::Hermes => { prepare_hermes_workspace(cwd, &mode); build_hermes_command(cwd, &mode, remote) }` arm. |
| CLI validation | `src/cli.rs:384` | Extend guard to also accept `"hermes"`; update error message to `"--agent must be 'pi', 'claude', or 'hermes'"`. |
| CLI agent_kind | `src/cli.rs:806` | Add `Some("hermes") => AgentKind::Hermes` arm. |
| Modal toggle | `src/app/input.rs:927` | Tab cycle becomes Claude → Pi → Hermes → Claude. |
| Modal label | `src/ui/modal.rs:104` | Add `AgentKind::Hermes => "hermes"` arm. |
| README | `README.md` | Document `--agent hermes`, `WSX_HERMES_BIN`, `WSX_HERMES_MODEL`, `WSX_HERMES_PROVIDER`, and the AGENTS.md side effect. |
| Cargo | `Cargo.toml` | Add `rusqlite = { version = "0.31", features = ["bundled"] }`. |

The Rust compiler's exhaustive-match check enforces that every non-exhaustive `match AgentKind { ... }` site fails to compile until the Hermes arm is added — a safety net against missing a touchpoint.

## Spawn command

```rust
pub fn build_hermes_command(
    cwd: &Path,
    mode: &SpawnMode,
    _remote: crate::remote_control::RemoteOpts,  // Hermes has no remote-control flag.
) -> CommandBuilder {
    let bin = std::env::var("WSX_HERMES_BIN").unwrap_or_else(|_| "hermes".to_string());
    let mut cmd = CommandBuilder::new(bin);
    cmd.cwd(cwd);
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }

    cmd.arg("chat");
    // Omit --source entirely if canonicalize fails — falling back to a generic
    // "wsx" tag would cluster sessions from multiple unresolvable cwds under
    // one tag and break latest_hermes_session_id's per-worktree precision.
    if let Some(source) = hermes_source_tag(cwd) {
        cmd.arg("--source").arg(&source);
    }

    let (add_continue, add_yolo) = match mode {
        SpawnMode::Continue { yolo, .. } => (true, *yolo),
        SpawnMode::Fresh { yolo, .. } => (false, *yolo),
        SpawnMode::ProjectManager { resume, .. } => (*resume, true),  // PM is always yolo, mirrors Pi.
    };

    if add_continue {
        // --resume <id> is more precise than bare --continue, which would resume
        // the globally-most-recent Hermes session regardless of worktree.
        if let Some(id) = latest_hermes_session_id_default(cwd) {
            cmd.arg("--resume").arg(&id);
        }
        // No prior wsx session for this cwd → silently launch fresh.
    }
    if add_yolo {
        cmd.arg("--yolo");
    }

    let model = std::env::var("WSX_HERMES_MODEL").ok()
        .map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let provider = std::env::var("WSX_HERMES_PROVIDER").ok()
        .map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    if let Some(m) = &model {
        // HERMES_INFERENCE_MODEL env var is honored in all Hermes modes.
        // The --model flag is documented as -z/--tui only.
        cmd.env("HERMES_INFERENCE_MODEL", m);
    }
    if let Some(p) = &provider {
        // --provider may be a no-op in classic REPL mode per Hermes docs;
        // persistent provider lives in ~/.hermes/config.yaml under model.provider.
        cmd.arg("--provider").arg(p);
    }

    cmd
}
```

**Three calls deliberately omitted:**

- **`--worktree`** — Hermes's `--worktree` makes Hermes create its own git worktree. wsx already manages worktrees; passing this would double-isolate. Comment in code.
- **`--append-system-prompt` equivalent** — doesn't exist in Hermes. Prompt injection goes through `prepare_hermes_workspace`.
- **`--accept-hooks`** — Hermes hooks are user-defined shell scripts; pre-accepting them silently bypasses a consent boundary. Leave to the user.

**Model/provider quirk:** `WSX_HERMES_MODEL` propagates via the `HERMES_INFERENCE_MODEL` env var (works in all Hermes modes). `WSX_HERMES_PROVIDER` propagates via `--provider`, which Hermes docs say only applies to `-z/--oneshot` and `--tui`. If it's a no-op in classic REPL, the persistent provider in `~/.hermes/config.yaml` wins. Document the caveat in README and the code comment.

## AGENTS.md prompt injection

Hermes has no `--append-system-prompt`. wsx writes a fenced block into `AGENTS.md` in the worktree's cwd, which Hermes auto-loads. The block is rewritten on every spawn; no cleanup logic is needed because each spawn rewrites based on current mode.

**Algorithm:**

```rust
fn prepare_hermes_workspace(cwd: &Path, mode: &SpawnMode) {
    let injected = compose_injected_prompt(mode);
    write_agents_md_section(cwd, injected.as_deref());  // None => strip block
    ensure_git_exclude(cwd, "AGENTS.md");
}

fn compose_injected_prompt(mode: &SpawnMode) -> Option<String> {
    match mode {
        SpawnMode::Fresh { rename_ctx: Some(ctx), custom_instructions, .. } => {
            let rename = render_rename_system_prompt_hermes(&ctx.current_branch, &ctx.branch_prefix);
            Some(combine(rename, custom_instructions.clone()))
        }
        SpawnMode::Fresh { rename_ctx: None, custom_instructions, .. }
        | SpawnMode::Continue { custom_instructions, .. } => custom_instructions.clone(),
        SpawnMode::ProjectManager { custom_instructions, .. } => {
            Some(crate::pm::pm_system_prompt(custom_instructions.as_deref()))
        }
    }
}
```

**Block markers:**

```markdown
<!-- BEGIN wsx-managed -->
{injected prompt}
<!-- END wsx-managed -->
```

`write_agents_md_section` reads the existing `AGENTS.md` (empty if absent), strips any prior `BEGIN/END wsx-managed` block, then either appends a new block or — if `injected` is `None` — writes back just the stripped content. If the result equals the original byte-for-byte, skip the write to avoid spurious mtime bumps.

**Rename prompt:** `render_rename_system_prompt_hermes` mirrors the Pi version (`render_rename_system_prompt_pi` at `src/pty/session.rs:566`). Hermes uses its own tool semantics; the prompt is plain English telling the agent to run `git branch -m <current> <prefix>/<slug>`, so the Pi text is directly reusable. A unit test asserts the Hermes and Pi prompts differ only in expected ways, as a guard against silent drift.

**Three AGENTS.md states in the worktree:**

| State | wsx behavior | Git visibility |
|---|---|---|
| File doesn't exist | Create it containing only the wsx block. `ensure_git_exclude` adds `AGENTS.md` to `.git/info/exclude`. | Untracked but excluded; no `git status` noise. |
| File exists, untracked | Append wsx block. | Untracked. `ensure_git_exclude` is harmless if redundant. |
| File exists, tracked | Append wsx block. The file will show as modified in `git status` — the honest cost of integrating with a tool that lacks `--append-system-prompt`. | Modified. Documented in README. |

**Why `.git/info/exclude`, not `.gitignore`:** `.git/info/exclude` is per-worktree-local (lives under the worktree's gitdir, not the repo working tree) and is never committed. `.gitignore` would either pollute the repo or have to be added to the worktree, both worse. Idempotent: grep for `^AGENTS\.md$` before appending.

**Cleanup is automatic.** Because every spawn rewrites the wsx block:
- Fresh + rename_ctx: block contains rename prompt + custom_instructions.
- Continue (rename done, no custom instructions): block is removed entirely.
- The "Only do this once per worktree" guard inside the rename prompt itself is belt-and-suspenders for the rare race where Continue happens before the rename completed.

**Failure mode:** All filesystem operations in `prepare_hermes_workspace` are best-effort. If we can't read/write `AGENTS.md` (perms, full disk, exotic filesystem), log a warning and proceed — Hermes still launches; the user just has to rename their branch manually.

## Prior-session detection + Continue resume

### Post-merge correction: source-tag approach abandoned

**Original design**: wsx was to encode cwd into `--source wsx:<encoded-cwd>` on every Hermes spawn, then query `sessions.source` to find the latest session. This is what the spec below originally described.

**What we discovered**: Hermes silently discards the `--source` flag. Its interactive chat handler hardcodes `platform="cli"` at session creation time, which preempts both the `--source` flag (which only affects `sessions list` filtering, never reaches session creation) and the `HERMES_SESSION_SOURCE` environment variable. Result: no wsx-tagged sessions ever appeared in `~/.hermes/state.db`; `latest_hermes_session_id` always returned None; the dashboard tailer never found anything to tail; Continue never resumed.

**Actual implementation (spawn-timestamp-based)**:

On every Hermes spawn, `prepare_hermes_workspace` writes a marker file at `<worktree>/.git/info/wsx-hermes-spawn-at` containing the current Unix epoch as a float (e.g., `"1779999480.123\n"`). This file is per-worktree-local (inside the gitdir) and is never committed.

Session lookup queries by timestamp:

```sql
SELECT id FROM sessions
WHERE started_at >= ?1 - 2.0
ORDER BY started_at DESC
LIMIT 1
```

`?1` is the spawn timestamp from the marker. The `-2.0` second buffer absorbs clock skew between our write and Hermes's `time.time()` call. `ORDER BY DESC LIMIT 1` returns the most recent matching session, handling the `/new` case (user starts a new Hermes session mid-run). The inherent race — if two worktrees spawn Hermes within seconds of each other, the more-recent session could be attributed wrong — is accepted and documented as best-effort.

The `--source` argument is never emitted. A code comment in `build_hermes_command` explains why.

---

*Original design (superseded):*

Hermes's `sessions` table has no `cwd` column. wsx encodes cwd into the `--source` tag on every spawn (`wsx:<encoded-cwd>`), then queries the same column to find the latest session for the current worktree. This same query drives both `has_prior_hermes_session` and the `--resume <id>` flag on Continue spawns.

**Source-tag encoding** mirrors the Pi convention: `canonical_path.to_string_lossy().replace('/', '-')`. Symmetric with `has_prior_pi_session` so future readers see one encoding scheme, not two.

**The query:**

```sql
SELECT id FROM sessions WHERE source = ?1 ORDER BY started_at DESC LIMIT 1
```

Both `source` and `started_at` are indexed (`idx_sessions_source`, `idx_sessions_started`), so this is O(log n).

**Implementation:**

```rust
fn hermes_source_tag(worktree: &Path) -> Option<String> {
    let abs = std::fs::canonicalize(worktree).ok()?;
    Some(format!("wsx:{}", abs.to_string_lossy().replace('/', "-")))
}

fn latest_hermes_session_id(db_path: &Path, worktree: &Path) -> Option<String> {
    let tag = hermes_source_tag(worktree)?;
    if !db_path.is_file() { return None; }

    let uri = format!("file:{}?mode=ro&immutable=1", db_path.display());
    let conn = rusqlite::Connection::open_with_flags(
        &uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    ).ok()?;

    conn.query_row(
        "SELECT id FROM sessions WHERE source = ?1 ORDER BY started_at DESC LIMIT 1",
        [&tag],
        |row| row.get::<_, String>(0),
    ).ok()
}

pub fn latest_hermes_session_id_default(worktree: &Path) -> Option<String> {
    let db = dirs::home_dir()?.join(".hermes/state.db");
    latest_hermes_session_id(&db, worktree)
}

pub fn has_prior_hermes_session(worktree: &Path) -> bool {
    latest_hermes_session_id_default(worktree).is_some()
}
```

**Why `immutable=1`:** Hermes is likely running concurrently in other worktrees and holds a WAL on `state.db`. `mode=ro&immutable=1` tells sqlite "treat the file as a frozen snapshot — don't check for or replay a WAL." Trade-off: we may see data 100–500 ms stale, never block on Hermes's locks, never corrupt anything. The trade matches the use case (cosmetic dashboard indicator + resume hint).

**Semantic difference from Claude/Pi indicators:** The Claude and Pi indicators stat per-cwd filesystem state that *any* invocation would write to, so they signal "has anyone (including non-wsx) run this agent here." The Hermes indicator only counts wsx-spawned sessions (because we control the `source` tag). If the user runs bare `hermes chat` outside wsx in the same cwd, those sessions get `source = "cli"` and don't count. This is intentional: the indicator reflects wsx's history, which is the more useful signal for resume semantics.

**Schema-coupling risk:** the query depends on `sessions.id`, `sessions.source`, and `sessions.started_at`. These are FK-anchored and indexed, so unlikely to change wantonly, but if Hermes drops or renames any of them in a future release, detection silently returns "no prior session." Acceptable — the dashboard indicator is best-effort, and `latest_hermes_session_id_default` falling through to `None` on Continue means we launch fresh rather than crash.

**Continue dispatch:** in `build_hermes_command`, the Continue branch uses `--resume <id>` (not `--continue`) so we resume the correct session for this worktree, not Hermes's globally-most-recent one. If `latest_hermes_session_id_default` returns `None`, we silently launch fresh — the user asked for Continue but there's nothing to continue, and launching fresh is less surprising than resuming a different worktree's session.

## CLI and UI wiring

Six small edits, all enumerated in the architecture table above. The compiler enforces completeness.

- `--agent` CLI accepts `"hermes"`; error message updates.
- `coding_agent` setting accepts `"hermes"` (already in the known-keys list at `src/cli.rs:149`; no edit there).
- Modal Tab toggle cycles Claude → Pi → Hermes → Claude.
- Modal label match adds `Hermes => "hermes"`.
- Dispatcher adds the Hermes arm, calling `prepare_hermes_workspace` then `build_hermes_command`.

**Settings:** users opt in via `wsx set coding_agent hermes` (existing setting-write CLI). Default remains Claude.

**No new test files** in `tests/`. The existing `tests/smoke.rs` and `tests/branch_drift.rs` are agent-agnostic.

## Tests

All inline in `src/pty/session.rs`'s existing `#[cfg(test)] mod tests` block. Five families.

**`build_hermes_command_*`** — spawn-arg assertions via `cmd.get_argv()`:

- `fresh_emits_chat_subcommand_and_source_tag` — argv starts with `chat --source wsx:<encoded-cwd>`.
- `fresh_omits_continue_and_resume` — no `--continue` or `--resume`.
- `continue_without_prior_session_omits_resume` — empty db → no `--resume`, silent fallback to fresh.
- `continue_with_prior_session_passes_resume_id` — inject test row in temp db → `--resume <id>` present.
- `yolo_emits_yolo_flag` (Fresh + Continue) — `--yolo` in argv.
- `non_yolo_omits_yolo_flag` — `--yolo` absent.
- `project_manager_mode_emits_yolo_and_resume_if_set` — PM always yolo; `--resume <id>` when `resume=true`.
- `wsx_hermes_model_env_sets_inference_model_env_on_child` — `HERMES_INFERENCE_MODEL` env on CommandBuilder.
- `wsx_hermes_provider_env_passes_provider_flag` — `--provider <value>` in argv.
- `empty_model_env_treated_as_unset` — mirrors Pi's whitespace-trim guard.
- `no_worktree_flag_ever_emitted` — regression guard against accidentally adding the `--worktree` flag.
- `source_omitted_when_canonicalize_fails` — when cwd can't be canonicalized, `--source` is absent rather than falling back to a non-cwd-specific tag.

**`prepare_hermes_workspace_*`** — AGENTS.md state machine:

- `writes_agents_md_with_fenced_block_on_fresh_with_rename` — file exists with BEGIN/END markers around rename prompt.
- `preserves_user_content_outside_wsx_block` — pre-existing arbitrary content intact after rewrite.
- `replaces_existing_wsx_block_idempotently` — calling twice with the same mode produces byte-identical file.
- `strips_wsx_block_when_nothing_to_inject` — Continue mode with no custom_instructions → wsx block gone, user content kept.
- `pm_mode_writes_pm_system_prompt_block` — block contains expected PM-prompt prefix.
- `survives_unreadable_agents_md` — pre-create file with no read perms → function returns without panicking.

**`latest_hermes_session_id_*`** — sqlite query (uses the path-parameterized inner function with a tempdir-backed sqlite created with the minimal schema):

- `missing_db_returns_none` — nonexistent path → `None`.
- `empty_sessions_returns_none` — db exists but no rows → `None`.
- `non_matching_source_returns_none` — rows with `source = "cli"` only → `None` for `wsx:<encoded>`.
- `single_match_returns_id` — one wsx-tagged row → returns its id.
- `multiple_matches_returns_most_recent_by_started_at` — three rows, varying timestamps → returns newest.
- `has_prior_hermes_session_returns_bool` — thin wrapper.
- `concurrent_writer_does_not_block_read` — open writer connection, then query → succeeds (immutable=1 contract).

**`render_rename_system_prompt_hermes_*`** — symmetry with `render_rename_system_prompt_pi` tests at `src/pty/session.rs:1049`:

- `includes_current_branch_and_prefix`.
- `handles_empty_prefix`.
- `differs_from_pi_only_where_necessary` — soft guard against silent drift between the Hermes and Pi rename prompts.

**`ensure_git_exclude_*`** — `.git/info/exclude` plumbing:

- `creates_exclude_line_when_absent` — fresh `.git/info/exclude` → contains `AGENTS.md` line.
- `idempotent_when_entry_already_present` — pre-seed with `AGENTS.md` → byte-for-byte unchanged.
- `handles_missing_info_dir` — `.git` exists but no `info/` subdir → creates it.
- `no_op_when_gitdir_absent` — plain dir, no `.git` → returns without panicking.

**Modal toggle test:** if `src/app/input.rs` has existing coverage of the Tab toggle, extend to assert the 3-cycle Claude → Pi → Hermes → Claude. If not, skip — one-line UI logic with compiler-enforced exhaustiveness.

**Existing tests that hardcode `AgentKind::Claude`** (~10 in `session.rs`, plus `:856` and `:745` in UI): no change. They construct a specific variant, not match exhaustively.

## Dependency

```toml
rusqlite = { version = "0.31", features = ["bundled"] }
```

Bundled libsqlite ships our own copy, avoiding system-library version skew across macOS/Linux dev boxes. Cost: ~1 MB to the binary. Worth it for the one query.

## Open items deferred

- **PM-on-Hermes prompt fit.** `pm_system_prompt` was authored for Claude/Pi and may contain Claude-isms. If running PM with Hermes turns out to need different wording, factor a Hermes-specific PM prompt — that's a follow-up, not blocking this PR.
- **AGENTS.md visibility in tracked-file repos.** The wsx block appears as a working-tree modification on `git status` for users whose repos commit AGENTS.md. Acceptable for now; could revisit with `--skip-worktree` or `--assume-unchanged` later if it bites.
- **Stale session ID.** If the user manually deletes a session via `hermes sessions delete` between our query and Hermes opening, `--resume <id>` will error. Window is tiny; not handling.
- **Configuration discovery.** No `wsx doctor`–style check that `WSX_HERMES_BIN` actually resolves. Out of scope for this PR.
