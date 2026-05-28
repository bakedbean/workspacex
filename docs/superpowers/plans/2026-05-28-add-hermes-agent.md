# Add Hermes Agent Support — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `AgentKind::Hermes` to wsx as a third coding-agent harness with full feature parity to the Pi integration: spawn, continue/resume, model/provider env overrides, yolo, auto-rename system prompt, and prior-session indicator.

**Architecture:** Mirror the existing Claude/Pi shape — one `match AgentKind { ... }` dispatch, one `build_<agent>_command` per agent, one `has_prior_<agent>_session` per agent. Compensate for Hermes's two missing capabilities (no `--append-system-prompt`, no per-cwd session storage) with an AGENTS.md prompt-injection helper and a `--source wsx:<cwd>`-tagged sqlite query.

**Tech Stack:** Rust 2021, `portable_pty::CommandBuilder`, `rusqlite 0.32` (already in `Cargo.toml`), `tempfile 3` (already in dev-deps), `dirs 5` (already in deps).

**Companion spec:** `docs/superpowers/specs/2026-05-28-add-hermes-agent-design.md` — read this first for the design rationale and the trade-offs we accepted.

---

## File Structure

**Modified files:**
- `src/pty/session.rs` — variant addition, all new helpers, builder, dispatcher arm, all new tests. ~400 LOC added.
- `src/cli.rs` — extend `--agent` validation + `agent_kind` match. ~4 LOC changed.
- `src/app/input.rs` — Tab toggle 2-cycle → 3-cycle. ~6 LOC changed.
- `src/ui/modal.rs` — add `Hermes => "hermes"` label arm. ~1 LOC added.
- `README.md` — document the new agent, env vars, AGENTS.md behavior. ~20 LOC added.

**No new files.** All logic lives next to existing Pi/Claude code in `src/pty/session.rs`.

---

## Task 1: Scaffold `AgentKind::Hermes` so the project compiles

**Why first:** Adding the variant turns every non-exhaustive `match AgentKind { ... }` into a compile error. We fix all of them up-front with placeholder behavior so subsequent TDD tasks can iterate on real logic without fighting a non-compiling tree.

**Files:**
- Modify: `src/pty/session.rs:16-19` (variant), `src/pty/session.rs:23-26` (from_store), `src/pty/session.rs:281-286` (has_prior_session_for), `src/pty/session.rs:~609` (dispatcher arm)
- Modify: `src/cli.rs:384` (validation), `src/cli.rs:806-809` (agent_kind match)
- Modify: `src/app/input.rs:927-930` (Tab toggle)
- Modify: `src/ui/modal.rs:104-106` (label match)

- [ ] **Step 1.1: Add the variant**

Edit `src/pty/session.rs` lines 15-19:

```rust
/// Which coding agent to spawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Claude,
    Pi,
    Hermes,
}
```

- [ ] **Step 1.2: Extend `from_store`**

Edit `src/pty/session.rs` lines 22-27:

```rust
impl AgentKind {
    pub fn from_store(store: &crate::store::Store) -> Self {
        match store.get_setting("coding_agent").ok().flatten().as_deref() {
            Some("pi") => AgentKind::Pi,
            Some("hermes") => AgentKind::Hermes,
            _ => AgentKind::Claude,
        }
    }
}
```

- [ ] **Step 1.3: Extend `has_prior_session_for` with a stub**

Edit `src/pty/session.rs` lines 281-286:

```rust
/// Resolve whether a workspace has a prior session based on the agent kind.
pub fn has_prior_session_for(worktree: &Path, agent: AgentKind) -> bool {
    match agent {
        AgentKind::Claude => has_prior_session(worktree),
        AgentKind::Pi => has_prior_pi_session(worktree),
        AgentKind::Hermes => false, // stub — replaced in Task 5
    }
}
```

- [ ] **Step 1.4: Extend dispatcher with a stub**

Locate the `match agent { ... }` near line 609 (inside the body that spawns the session) and add a stub arm. The exact context looks like:

```rust
AgentKind::Claude => build_claude_command(cwd, &mode, remote),
AgentKind::Pi => build_pi_command(cwd, &mode, remote),
```

Add a third arm immediately after:

```rust
AgentKind::Claude => build_claude_command(cwd, &mode, remote),
AgentKind::Pi => build_pi_command(cwd, &mode, remote),
AgentKind::Hermes => {
    // Placeholder until Task 13 wires the real implementation.
    // CommandBuilder::new("hermes") at least produces a valid spawnable command
    // shape so the type-checker and integration paths work.
    let mut cmd = portable_pty::CommandBuilder::new("hermes");
    cmd.cwd(cwd);
    cmd
}
```

(`portable_pty` is already imported at the top of the file as `use portable_pty::{CommandBuilder, ...};` — you can use `CommandBuilder::new` directly without the prefix.)

- [ ] **Step 1.5: Extend CLI `--agent` validation**

Edit `src/cli.rs` lines 383-388:

```rust
if let Some(ref a) = agent {
    if a != "pi" && a != "claude" && a != "hermes" {
        return Err(Error::UserInput(format!(
            "--agent must be 'pi', 'claude', or 'hermes', got '{a}'"
        )));
    }
}
```

- [ ] **Step 1.6: Extend CLI `agent_kind` match**

Edit `src/cli.rs` lines 806-809:

```rust
let agent_kind = match agent.as_deref() {
    Some("pi") => crate::pty::session::AgentKind::Pi,
    Some("hermes") => crate::pty::session::AgentKind::Hermes,
    _ => crate::pty::session::AgentKind::Claude,
};
```

- [ ] **Step 1.7: Update the error-message arg validation**

Edit `src/cli.rs:375` (the `Error::UserInput` line in `--agent` parsing):

```rust
"--agent" => {
    agent = Some(it.next().ok_or_else(|| {
        Error::UserInput("--agent needs value (pi, claude, or hermes)".into())
    })?);
}
```

- [ ] **Step 1.8: Extend modal Tab toggle to a 3-cycle**

Edit `src/app/input.rs` lines 927-930:

```rust
KeyCode::Tab => {
    agent = match agent {
        crate::pty::session::AgentKind::Claude => crate::pty::session::AgentKind::Pi,
        crate::pty::session::AgentKind::Pi => crate::pty::session::AgentKind::Hermes,
        crate::pty::session::AgentKind::Hermes => crate::pty::session::AgentKind::Claude,
    };
```

- [ ] **Step 1.9: Extend modal label**

Edit `src/ui/modal.rs` lines 104-107:

```rust
let agent_label = match agent {
    crate::pty::session::AgentKind::Claude => "claude",
    crate::pty::session::AgentKind::Pi => "pi",
    crate::pty::session::AgentKind::Hermes => "hermes",
};
```

- [ ] **Step 1.10: Build to verify the tree compiles**

Run: `cargo build 2>&1 | tail -20`

Expected: build succeeds with no errors. Warnings about unused variants or dead code are acceptable at this stage.

If a `non_exhaustive_patterns` error fires from somewhere not listed above, add the appropriate `AgentKind::Hermes` arm with a `// TODO Task 13` stub. Do NOT change behavior in other arms.

- [ ] **Step 1.11: Run existing tests to confirm no regressions**

Run: `cargo test --lib 2>&1 | tail -10`

Expected: all existing tests pass. We have added behavior only via the new variant; no existing path changed.

- [ ] **Step 1.12: Commit**

```bash
git add src/pty/session.rs src/cli.rs src/app/input.rs src/ui/modal.rs
git commit -m "feat(agent): scaffold hermes variant"
```

---

## Task 2: `hermes_source_tag` — encode cwd into the source-tag string

**Files:**
- Modify: `src/pty/session.rs` — add the helper near the existing `has_prior_pi_session` function (~line 256).
- Test: same file, inside the existing `#[cfg(test)] mod tests`.

- [ ] **Step 2.1: Write the failing test**

Append to the existing `mod tests` block at the bottom of `src/pty/session.rs`:

```rust
#[test]
fn hermes_source_tag_encodes_path_with_dashes_and_wsx_prefix() {
    let tmp = tempfile::tempdir().unwrap();
    let tag = super::hermes_source_tag(tmp.path()).expect("canonicalize should succeed for tempdir");
    assert!(tag.starts_with("wsx:"), "tag {tag} should start with wsx:");
    let after = &tag["wsx:".len()..];
    assert!(!after.contains('/'), "tag {tag} should have no slashes after prefix");
    let canonical = std::fs::canonicalize(tmp.path()).unwrap();
    let expected_tail = canonical.to_string_lossy().replace('/', "-");
    assert_eq!(after, expected_tail);
}

#[test]
fn hermes_source_tag_returns_none_for_nonexistent_path() {
    let bogus = std::path::Path::new("/this/path/definitely/does/not/exist/123456");
    assert!(super::hermes_source_tag(bogus).is_none());
}
```

- [ ] **Step 2.2: Run the tests to confirm they fail**

Run: `cargo test --lib pty::session::tests::hermes_source_tag 2>&1 | tail -10`

Expected: compile error `cannot find function 'hermes_source_tag'`.

- [ ] **Step 2.3: Implement the helper**

Add the following function in `src/pty/session.rs`, immediately after `has_prior_pi_session` (around line 278):

```rust
/// Encode a worktree path into a `--source` tag for Hermes session tagging.
/// Returns None when canonicalization fails — callers should treat that as
/// "don't pass `--source`" so we don't cluster multiple unresolvable cwds
/// under a single tag and break per-worktree session lookups.
fn hermes_source_tag(worktree: &Path) -> Option<String> {
    let abs = std::fs::canonicalize(worktree).ok()?;
    Some(format!("wsx:{}", abs.to_string_lossy().replace('/', "-")))
}
```

- [ ] **Step 2.4: Run the tests to confirm they pass**

Run: `cargo test --lib pty::session::tests::hermes_source_tag 2>&1 | tail -10`

Expected: both tests pass.

- [ ] **Step 2.5: Commit**

```bash
git add src/pty/session.rs
git commit -m "feat(agent): add hermes_source_tag for per-cwd session keying"
```

---

## Task 3: `latest_hermes_session_id` — query sqlite for the newest session matching a source tag

**Files:**
- Modify: `src/pty/session.rs` — add the helper alongside the source-tag helper.
- Test: same file, inside `mod tests`.

The function is path-parameterized for testability. A separate `_default` wrapper resolves `~/.hermes/state.db` for production callers (Task 5).

- [ ] **Step 3.1: Write the failing tests**

Append to `mod tests`:

```rust
mod hermes_session_lookup {
    use super::*;
    use std::path::PathBuf;

    fn make_db(path: &std::path::Path) -> rusqlite::Connection {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                started_at REAL NOT NULL
            );",
        ).unwrap();
        conn
    }

    fn insert(conn: &rusqlite::Connection, id: &str, source: &str, started_at: f64) {
        conn.execute(
            "INSERT INTO sessions (id, source, started_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![id, source, started_at],
        ).unwrap();
    }

    #[test]
    fn missing_db_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let bogus = tmp.path().join("nope.db");
        let worktree = tmp.path();
        assert!(latest_hermes_session_id(&bogus, worktree).is_none());
    }

    #[test]
    fn empty_sessions_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("state.db");
        let _ = make_db(&db_path);
        assert!(latest_hermes_session_id(&db_path, tmp.path()).is_none());
    }

    #[test]
    fn non_matching_source_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("state.db");
        let conn = make_db(&db_path);
        insert(&conn, "abc", "cli", 1000.0);
        insert(&conn, "def", "telegram", 2000.0);
        assert!(latest_hermes_session_id(&db_path, tmp.path()).is_none());
    }

    #[test]
    fn single_match_returns_id() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("state.db");
        let conn = make_db(&db_path);
        let tag = super::hermes_source_tag(tmp.path()).unwrap();
        insert(&conn, "abc", &tag, 1000.0);
        assert_eq!(
            latest_hermes_session_id(&db_path, tmp.path()).as_deref(),
            Some("abc")
        );
    }

    #[test]
    fn multiple_matches_returns_most_recent_by_started_at() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("state.db");
        let conn = make_db(&db_path);
        let tag = super::hermes_source_tag(tmp.path()).unwrap();
        insert(&conn, "oldest", &tag, 1000.0);
        insert(&conn, "newest", &tag, 3000.0);
        insert(&conn, "middle", &tag, 2000.0);
        assert_eq!(
            latest_hermes_session_id(&db_path, tmp.path()).as_deref(),
            Some("newest")
        );
    }

    #[test]
    fn concurrent_writer_does_not_block_read() {
        // Open a writer holding the db, then query — immutable=1 means we
        // ignore the lock and read a snapshot.
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("state.db");
        let writer = make_db(&db_path);
        let tag = super::hermes_source_tag(tmp.path()).unwrap();
        insert(&writer, "abc", &tag, 1000.0);
        // Start an explicit transaction to hold a write lock.
        writer.execute_batch("BEGIN IMMEDIATE;").unwrap();
        let result = latest_hermes_session_id(&db_path, tmp.path());
        // Even with the writer holding the lock, our ro+immutable read succeeds.
        assert_eq!(result.as_deref(), Some("abc"));
        writer.execute_batch("ROLLBACK;").unwrap();
        // bind to silence unused warning
        let _ = PathBuf::from(tmp.path());
    }
}
```

- [ ] **Step 3.2: Run the tests to confirm they fail**

Run: `cargo test --lib pty::session::tests::hermes_session_lookup 2>&1 | tail -20`

Expected: compile errors `cannot find function 'latest_hermes_session_id'`.

- [ ] **Step 3.3: Implement the function**

Add immediately below `hermes_source_tag` in `src/pty/session.rs`:

```rust
/// Return the most recent wsx-spawned Hermes session ID for this worktree, if any.
/// Path-parameterized for testing; production callers should use
/// `latest_hermes_session_id_default`.
///
/// Opens the db read-only with `immutable=1` so we don't block on Hermes's WAL
/// when Hermes is running concurrently in another worktree, and don't risk
/// rolling forward an inconsistent WAL.
fn latest_hermes_session_id(db_path: &Path, worktree: &Path) -> Option<String> {
    let tag = hermes_source_tag(worktree)?;
    if !db_path.is_file() {
        return None;
    }
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
```

- [ ] **Step 3.4: Run the tests to confirm they pass**

Run: `cargo test --lib pty::session::tests::hermes_session_lookup 2>&1 | tail -20`

Expected: all six tests pass.

- [ ] **Step 3.5: Commit**

```bash
git add src/pty/session.rs
git commit -m "feat(agent): query hermes state.db for latest session by source tag"
```

---

## Task 4: `latest_hermes_session_id_default` + `has_prior_hermes_session`

The default wrapper resolves `~/.hermes/state.db` for production use, and the bool wrapper gives the dashboard indicator its yes/no answer. Both are thin shims around Task 3; no behavior tests needed beyond compilation.

**Files:**
- Modify: `src/pty/session.rs` — add the two wrappers.

- [ ] **Step 4.1: Implement the wrappers**

Add immediately below `latest_hermes_session_id` in `src/pty/session.rs`:

```rust
/// Production wrapper for `latest_hermes_session_id` that resolves
/// `~/.hermes/state.db`.
pub fn latest_hermes_session_id_default(worktree: &Path) -> Option<String> {
    let db = dirs::home_dir()?.join(".hermes/state.db");
    latest_hermes_session_id(&db, worktree)
}

/// True if a wsx-spawned Hermes session exists for this worktree.
pub fn has_prior_hermes_session(worktree: &Path) -> bool {
    latest_hermes_session_id_default(worktree).is_some()
}
```

- [ ] **Step 4.2: Replace the `has_prior_session_for` stub from Task 1**

Edit the `has_prior_session_for` function:

```rust
pub fn has_prior_session_for(worktree: &Path, agent: AgentKind) -> bool {
    match agent {
        AgentKind::Claude => has_prior_session(worktree),
        AgentKind::Pi => has_prior_pi_session(worktree),
        AgentKind::Hermes => has_prior_hermes_session(worktree),
    }
}
```

- [ ] **Step 4.3: Run the tests to confirm nothing regressed**

Run: `cargo test --lib pty::session 2>&1 | tail -10`

Expected: all tests still pass.

- [ ] **Step 4.4: Commit**

```bash
git add src/pty/session.rs
git commit -m "feat(agent): wire hermes prior-session detection into dispatcher"
```

---

## Task 5: `ensure_git_exclude` — idempotent `.git/info/exclude` append

Used by `prepare_hermes_workspace` to hide a wsx-created `AGENTS.md` from `git status`. Per-worktree-local; never committed.

**Files:**
- Modify: `src/pty/session.rs`.
- Test: same file, `mod tests`.

- [ ] **Step 5.1: Write the failing tests**

Append to `mod tests`:

```rust
mod hermes_git_exclude {
    use super::*;
    use std::fs;
    use std::io::Read;

    fn init_gitdir(dir: &std::path::Path) {
        fs::create_dir_all(dir.join(".git/info")).unwrap();
    }

    fn read(path: &std::path::Path) -> String {
        let mut s = String::new();
        fs::File::open(path).unwrap().read_to_string(&mut s).unwrap();
        s
    }

    #[test]
    fn creates_exclude_line_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        init_gitdir(tmp.path());
        super::ensure_git_exclude(tmp.path(), "AGENTS.md");
        let contents = read(&tmp.path().join(".git/info/exclude"));
        assert!(
            contents.lines().any(|l| l == "AGENTS.md"),
            "expected AGENTS.md line in {contents:?}"
        );
    }

    #[test]
    fn idempotent_when_entry_already_present() {
        let tmp = tempfile::tempdir().unwrap();
        init_gitdir(tmp.path());
        let exclude = tmp.path().join(".git/info/exclude");
        fs::write(&exclude, "AGENTS.md\n").unwrap();
        let before = read(&exclude);
        super::ensure_git_exclude(tmp.path(), "AGENTS.md");
        let after = read(&exclude);
        assert_eq!(before, after);
    }

    #[test]
    fn handles_missing_info_dir() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join(".git")).unwrap();
        super::ensure_git_exclude(tmp.path(), "AGENTS.md");
        let contents = read(&tmp.path().join(".git/info/exclude"));
        assert!(contents.contains("AGENTS.md"));
    }

    #[test]
    fn no_op_when_gitdir_absent() {
        let tmp = tempfile::tempdir().unwrap();
        // No .git/ at all. Must not panic.
        super::ensure_git_exclude(tmp.path(), "AGENTS.md");
        assert!(!tmp.path().join(".git").exists());
    }
}
```

- [ ] **Step 5.2: Run the tests to confirm they fail**

Run: `cargo test --lib pty::session::tests::hermes_git_exclude 2>&1 | tail -20`

Expected: compile error `cannot find function 'ensure_git_exclude'`.

- [ ] **Step 5.3: Implement the helper**

Add to `src/pty/session.rs` (near the other Hermes helpers):

```rust
/// Append `name` to the worktree's `.git/info/exclude` if not already present.
/// Best-effort: silently no-ops on any IO error or if `.git/` is absent.
/// `.git/info/exclude` is per-worktree-local and never committed.
fn ensure_git_exclude(worktree: &Path, name: &str) {
    let git_dir = worktree.join(".git");
    if !git_dir.exists() {
        return;
    }
    let info_dir = git_dir.join("info");
    if !info_dir.exists() {
        if std::fs::create_dir_all(&info_dir).is_err() {
            return;
        }
    }
    let exclude_path = info_dir.join("exclude");
    let existing = std::fs::read_to_string(&exclude_path).unwrap_or_default();
    if existing.lines().any(|l| l == name) {
        return;
    }
    let mut new = existing;
    if !new.is_empty() && !new.ends_with('\n') {
        new.push('\n');
    }
    new.push_str(name);
    new.push('\n');
    let _ = std::fs::write(&exclude_path, new);
}
```

- [ ] **Step 5.4: Run the tests to confirm they pass**

Run: `cargo test --lib pty::session::tests::hermes_git_exclude 2>&1 | tail -10`

Expected: all four tests pass.

- [ ] **Step 5.5: Commit**

```bash
git add src/pty/session.rs
git commit -m "feat(agent): add ensure_git_exclude helper for AGENTS.md hiding"
```

---

## Task 6: `write_agents_md_section` — fenced wsx block in `AGENTS.md`

Reads existing AGENTS.md, strips any prior `BEGIN/END wsx-managed` block, then either appends a new block or — if `content` is None — writes back just the stripped content. Skips the write if the result is byte-identical to the original.

**Files:**
- Modify: `src/pty/session.rs`.
- Test: same file, `mod tests`.

- [ ] **Step 6.1: Write the failing tests**

Append to `mod tests`:

```rust
mod hermes_agents_md {
    use super::*;
    use std::fs;

    const MARKER_BEGIN: &str = "<!-- BEGIN wsx-managed -->";
    const MARKER_END: &str = "<!-- END wsx-managed -->";

    #[test]
    fn creates_file_with_fenced_block_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        super::write_agents_md_section(tmp.path(), Some("inject me"));
        let contents = fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
        assert!(contents.contains(MARKER_BEGIN), "missing BEGIN marker: {contents:?}");
        assert!(contents.contains(MARKER_END), "missing END marker: {contents:?}");
        assert!(contents.contains("inject me"));
    }

    #[test]
    fn preserves_user_content_outside_wsx_block() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("AGENTS.md");
        fs::write(&path, "# User notes\n\nKeep me.\n").unwrap();
        super::write_agents_md_section(tmp.path(), Some("inject me"));
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("# User notes"));
        assert!(contents.contains("Keep me."));
        assert!(contents.contains("inject me"));
    }

    #[test]
    fn replaces_existing_wsx_block_idempotently() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("AGENTS.md");
        super::write_agents_md_section(tmp.path(), Some("first"));
        let after_first = fs::read_to_string(&path).unwrap();
        super::write_agents_md_section(tmp.path(), Some("first"));
        let after_second = fs::read_to_string(&path).unwrap();
        assert_eq!(after_first, after_second, "second write should be byte-identical");
    }

    #[test]
    fn replacing_block_with_new_content_replaces_in_place() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("AGENTS.md");
        super::write_agents_md_section(tmp.path(), Some("first"));
        super::write_agents_md_section(tmp.path(), Some("second"));
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("second"));
        assert!(!contents.contains("first"), "old content should be removed");
    }

    #[test]
    fn strips_block_when_content_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("AGENTS.md");
        fs::write(&path, "user content\n").unwrap();
        super::write_agents_md_section(tmp.path(), Some("temp"));
        super::write_agents_md_section(tmp.path(), None);
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("user content"));
        assert!(!contents.contains(MARKER_BEGIN));
        assert!(!contents.contains("temp"));
    }

    #[test]
    fn no_write_when_content_is_none_and_no_existing_block() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("AGENTS.md");
        // Don't create the file at all.
        super::write_agents_md_section(tmp.path(), None);
        assert!(!path.exists(), "should not create AGENTS.md just to strip nothing");
    }

    #[test]
    fn survives_unreadable_agents_md() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("AGENTS.md");
        fs::write(&path, "untouchable\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        // Must not panic.
        super::write_agents_md_section(tmp.path(), Some("inject"));
        // Restore perms so tempdir cleanup works.
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    }
}
```

- [ ] **Step 6.2: Run the tests to confirm they fail**

Run: `cargo test --lib pty::session::tests::hermes_agents_md 2>&1 | tail -20`

Expected: compile error `cannot find function 'write_agents_md_section'`.

- [ ] **Step 6.3: Implement the function**

Add to `src/pty/session.rs`:

```rust
const HERMES_BLOCK_BEGIN: &str = "<!-- BEGIN wsx-managed -->";
const HERMES_BLOCK_END: &str = "<!-- END wsx-managed -->";

/// Rewrite the wsx-managed section of `AGENTS.md` in `cwd`.
///
/// Strips any existing `BEGIN/END wsx-managed` block, then appends a new
/// block with `content` if Some, or writes back just the stripped content if
/// None. Skips the write entirely if the result equals the existing file.
///
/// Best-effort: any IO error is silently swallowed.
fn write_agents_md_section(cwd: &Path, content: Option<&str>) {
    let path = cwd.join("AGENTS.md");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let stripped = strip_wsx_block(&existing);
    let new = match content {
        Some(c) => {
            let mut s = stripped.into_owned();
            if !s.is_empty() && !s.ends_with('\n') {
                s.push('\n');
            }
            if !s.is_empty() {
                s.push('\n');
            }
            s.push_str(HERMES_BLOCK_BEGIN);
            s.push('\n');
            s.push_str(c);
            if !c.ends_with('\n') {
                s.push('\n');
            }
            s.push_str(HERMES_BLOCK_END);
            s.push('\n');
            s
        }
        None => stripped.into_owned(),
    };

    if new == existing {
        return;
    }
    if new.is_empty() && !path.exists() {
        return;
    }
    let _ = std::fs::write(&path, new);
}

/// Remove a `BEGIN/END wsx-managed` block (and the surrounding blank lines
/// it produced when we wrote it) from `source`, returning a `Cow` so we
/// can avoid allocation in the common no-block path.
fn strip_wsx_block(source: &str) -> std::borrow::Cow<'_, str> {
    let Some(begin) = source.find(HERMES_BLOCK_BEGIN) else {
        return std::borrow::Cow::Borrowed(source);
    };
    let Some(end_rel) = source[begin..].find(HERMES_BLOCK_END) else {
        // Malformed (BEGIN without END) — strip from BEGIN onwards.
        return std::borrow::Cow::Owned(source[..begin].trim_end_matches('\n').to_string());
    };
    let end = begin + end_rel + HERMES_BLOCK_END.len();
    // Consume one trailing newline after END if present, so successive
    // strip/append cycles don't grow blank-line padding.
    let mut tail_start = end;
    if source.as_bytes().get(tail_start) == Some(&b'\n') {
        tail_start += 1;
    }
    // Trim trailing newlines from the prefix so we don't accumulate blank lines.
    let prefix = source[..begin].trim_end_matches('\n');
    let suffix = &source[tail_start..];
    let mut combined = String::with_capacity(prefix.len() + suffix.len() + 1);
    combined.push_str(prefix);
    if !prefix.is_empty() && !suffix.is_empty() {
        combined.push('\n');
    }
    combined.push_str(suffix);
    std::borrow::Cow::Owned(combined)
}
```

- [ ] **Step 6.4: Run the tests to confirm they pass**

Run: `cargo test --lib pty::session::tests::hermes_agents_md 2>&1 | tail -20`

Expected: all seven tests pass.

- [ ] **Step 6.5: Commit**

```bash
git add src/pty/session.rs
git commit -m "feat(agent): add AGENTS.md write helper with fenced wsx block"
```

---

## Task 7: `render_rename_system_prompt_hermes` — branch-rename instruction text

The Pi version's text is directly reusable for Hermes (same English, no Claude-specific tool names). We create a separate function so future divergence is a one-place change rather than a grep-and-update across helpers.

**Files:**
- Modify: `src/pty/session.rs`.
- Test: same file, `mod tests`.

- [ ] **Step 7.1: Write the failing tests**

Append to `mod tests`:

```rust
#[test]
fn render_rename_prompt_hermes_includes_branch_and_prefix() {
    let prompt = super::render_rename_system_prompt_hermes("wsx/bold-fern", "wsx");
    assert!(prompt.contains("git branch -m wsx/bold-fern"));
    assert!(prompt.contains("wsx/<slug>"));
}

#[test]
fn render_rename_prompt_hermes_handles_empty_prefix() {
    let prompt = super::render_rename_system_prompt_hermes("bold-fern", "");
    assert!(prompt.contains("git branch -m bold-fern"));
    // No trailing slash artifact when prefix is empty.
    assert!(!prompt.contains("//"), "prompt should not contain double-slash: {prompt}");
}

#[test]
fn render_rename_prompt_hermes_matches_pi_today() {
    // Soft guard against silent drift. If you intentionally diverge from
    // the Pi text (e.g., to reference a Hermes-specific tool name), update
    // this test to assert on the intentional difference.
    let hermes = super::render_rename_system_prompt_hermes("wsx/x", "wsx");
    let pi = super::render_rename_system_prompt_pi("wsx/x", "wsx");
    assert_eq!(hermes, pi, "drift between hermes and pi rename prompts");
}
```

- [ ] **Step 7.2: Run the tests to confirm they fail**

Run: `cargo test --lib pty::session::tests::render_rename_prompt_hermes 2>&1 | tail -10`

Expected: compile error `cannot find function 'render_rename_system_prompt_hermes'`.

- [ ] **Step 7.3: Implement**

Add to `src/pty/session.rs` near `render_rename_system_prompt_pi`:

```rust
/// Hermes version of the rename system prompt. Today the text is identical to
/// the Pi version — Hermes has no permission system and uses plain bash, same
/// as Pi. Keep this function distinct from the Pi helper so future divergence
/// (e.g., a Hermes-specific tool naming convention) is a one-place change.
fn render_rename_system_prompt_hermes(current_branch: &str, branch_prefix: &str) -> String {
    render_rename_system_prompt_pi(current_branch, branch_prefix)
}
```

- [ ] **Step 7.4: Run the tests to confirm they pass**

Run: `cargo test --lib pty::session::tests::render_rename_prompt_hermes 2>&1 | tail -10`

Expected: all three tests pass.

- [ ] **Step 7.5: Commit**

```bash
git add src/pty/session.rs
git commit -m "feat(agent): add hermes rename prompt as pi-text delegate"
```

---

## Task 8: `compose_injected_prompt` — match `SpawnMode` → `Option<String>`

Single pure function that maps the spawn mode to the text to inject into the wsx block (or None if nothing to inject).

**Files:**
- Modify: `src/pty/session.rs`.
- Test: same file, `mod tests`.

- [ ] **Step 8.1: Write the failing tests**

Append to `mod tests`:

```rust
mod hermes_compose {
    use super::*;

    fn rename_ctx() -> RenameContext {
        RenameContext {
            current_branch: "wsx/bold-fern".into(),
            branch_prefix: "wsx".into(),
        }
    }

    #[test]
    fn fresh_with_rename_returns_rename_text() {
        let mode = SpawnMode::Fresh {
            rename_ctx: Some(rename_ctx()),
            custom_instructions: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let result = super::compose_injected_prompt(&mode).expect("expected Some");
        assert!(result.contains("git branch -m wsx/bold-fern"));
    }

    #[test]
    fn fresh_with_rename_and_custom_combines_both() {
        let mode = SpawnMode::Fresh {
            rename_ctx: Some(rename_ctx()),
            custom_instructions: Some("Use ruff.".into()),
            additional_dirs: vec![],
            yolo: false,
        };
        let result = super::compose_injected_prompt(&mode).expect("expected Some");
        assert!(result.contains("git branch -m"));
        assert!(result.contains("Use ruff."));
        let rename_pos = result.find("git branch -m").unwrap();
        let custom_pos = result.find("Use ruff.").unwrap();
        assert!(custom_pos > rename_pos, "custom should come after rename block");
    }

    #[test]
    fn fresh_without_rename_returns_custom_only() {
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: Some("Use ruff.".into()),
            additional_dirs: vec![],
            yolo: false,
        };
        let result = super::compose_injected_prompt(&mode).expect("expected Some");
        assert_eq!(result, "Use ruff.");
    }

    #[test]
    fn fresh_with_nothing_returns_none() {
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            additional_dirs: vec![],
            yolo: false,
        };
        assert!(super::compose_injected_prompt(&mode).is_none());
    }

    #[test]
    fn continue_with_custom_returns_custom() {
        let mode = SpawnMode::Continue {
            custom_instructions: Some("Be terse.".into()),
            additional_dirs: vec![],
            yolo: false,
        };
        let result = super::compose_injected_prompt(&mode).expect("expected Some");
        assert_eq!(result, "Be terse.");
    }

    #[test]
    fn continue_without_custom_returns_none() {
        let mode = SpawnMode::Continue {
            custom_instructions: None,
            additional_dirs: vec![],
            yolo: false,
        };
        assert!(super::compose_injected_prompt(&mode).is_none());
    }

    #[test]
    fn project_manager_returns_pm_prompt() {
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: std::path::PathBuf::from("/tmp/ws.json"),
            custom_instructions: None,
            additional_dirs: vec![],
            resume: false,
            fast_mode: false,
        };
        let result = super::compose_injected_prompt(&mode).expect("expected Some");
        // The PM system prompt's exact text is owned by src/pm.rs; just assert
        // it's non-empty and recognizable as the PM prompt by its agent header.
        assert!(!result.is_empty());
    }
}
```

- [ ] **Step 8.2: Run the tests to confirm they fail**

Run: `cargo test --lib pty::session::tests::hermes_compose 2>&1 | tail -10`

Expected: compile error `cannot find function 'compose_injected_prompt'`.

- [ ] **Step 8.3: Implement**

Add to `src/pty/session.rs`:

```rust
/// Decide what text to inject into the wsx-managed block of AGENTS.md for a
/// given Hermes spawn mode. Returns None when nothing needs injecting.
fn compose_injected_prompt(mode: &SpawnMode) -> Option<String> {
    fn combine(rename: String, custom: Option<String>) -> String {
        match custom {
            None => rename,
            Some(c) => format!("{rename}\n\n{c}"),
        }
    }

    match mode {
        SpawnMode::Fresh {
            rename_ctx: Some(ctx),
            custom_instructions,
            ..
        } => Some(combine(
            render_rename_system_prompt_hermes(&ctx.current_branch, &ctx.branch_prefix),
            custom_instructions.clone(),
        )),
        SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions,
            ..
        }
        | SpawnMode::Continue {
            custom_instructions, ..
        } => custom_instructions.clone(),
        SpawnMode::ProjectManager {
            custom_instructions, ..
        } => Some(crate::pm::pm_system_prompt(custom_instructions.as_deref())),
    }
}
```

- [ ] **Step 8.4: Run the tests to confirm they pass**

Run: `cargo test --lib pty::session::tests::hermes_compose 2>&1 | tail -10`

Expected: all seven tests pass.

- [ ] **Step 8.5: Commit**

```bash
git add src/pty/session.rs
git commit -m "feat(agent): add compose_injected_prompt for hermes spawn modes"
```

---

## Task 9: `prepare_hermes_workspace` — orchestrate AGENTS.md write + git exclude

Composes Tasks 6, 7, 8 into the single entry point the dispatcher will call.

**Files:**
- Modify: `src/pty/session.rs`.
- Test: same file, `mod tests`.

- [ ] **Step 9.1: Write the failing tests**

Append to `mod tests`:

```rust
mod hermes_prepare_workspace {
    use super::*;
    use std::fs;

    fn init_gitdir(dir: &std::path::Path) {
        fs::create_dir_all(dir.join(".git/info")).unwrap();
    }

    #[test]
    fn fresh_with_rename_writes_agents_md_and_exclude() {
        let tmp = tempfile::tempdir().unwrap();
        init_gitdir(tmp.path());
        let mode = SpawnMode::Fresh {
            rename_ctx: Some(RenameContext {
                current_branch: "wsx/bold-fern".into(),
                branch_prefix: "wsx".into(),
            }),
            custom_instructions: None,
            additional_dirs: vec![],
            yolo: false,
        };
        super::prepare_hermes_workspace(tmp.path(), &mode);

        let agents = fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
        assert!(agents.contains("<!-- BEGIN wsx-managed -->"));
        assert!(agents.contains("git branch -m wsx/bold-fern"));

        let exclude = fs::read_to_string(tmp.path().join(".git/info/exclude")).unwrap();
        assert!(exclude.lines().any(|l| l == "AGENTS.md"));
    }

    #[test]
    fn continue_without_custom_instructions_strips_block() {
        let tmp = tempfile::tempdir().unwrap();
        init_gitdir(tmp.path());
        // First prepare a Fresh+rename state.
        let fresh = SpawnMode::Fresh {
            rename_ctx: Some(RenameContext {
                current_branch: "wsx/bold-fern".into(),
                branch_prefix: "wsx".into(),
            }),
            custom_instructions: None,
            additional_dirs: vec![],
            yolo: false,
        };
        super::prepare_hermes_workspace(tmp.path(), &fresh);
        // Now spawn Continue with nothing to inject.
        let cont = SpawnMode::Continue {
            custom_instructions: None,
            additional_dirs: vec![],
            yolo: false,
        };
        super::prepare_hermes_workspace(tmp.path(), &cont);
        let agents = fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap_or_default();
        assert!(!agents.contains("<!-- BEGIN wsx-managed -->"),
            "wsx block should be removed; got: {agents}");
        assert!(!agents.contains("git branch -m"),
            "rename text should be gone; got: {agents}");
    }

    #[test]
    fn no_op_when_continue_no_custom_and_no_existing_agents_md() {
        let tmp = tempfile::tempdir().unwrap();
        init_gitdir(tmp.path());
        let cont = SpawnMode::Continue {
            custom_instructions: None,
            additional_dirs: vec![],
            yolo: false,
        };
        super::prepare_hermes_workspace(tmp.path(), &cont);
        assert!(!tmp.path().join("AGENTS.md").exists());
    }
}
```

- [ ] **Step 9.2: Run the tests to confirm they fail**

Run: `cargo test --lib pty::session::tests::hermes_prepare_workspace 2>&1 | tail -10`

Expected: compile error `cannot find function 'prepare_hermes_workspace'`.

- [ ] **Step 9.3: Implement**

Add to `src/pty/session.rs`:

```rust
/// Prepare a worktree for a Hermes spawn: rewrite the wsx-managed block in
/// AGENTS.md (creating the file if needed) and ensure the file is hidden
/// from `git status` via `.git/info/exclude`.
///
/// Best-effort: all IO errors are swallowed. Hermes will still launch if
/// these side effects fail; the user just loses the rename hint.
fn prepare_hermes_workspace(cwd: &Path, mode: &SpawnMode) {
    let injected = compose_injected_prompt(mode);
    let had_content = injected.is_some();
    write_agents_md_section(cwd, injected.as_deref());
    if had_content {
        ensure_git_exclude(cwd, "AGENTS.md");
    }
}
```

- [ ] **Step 9.4: Run the tests to confirm they pass**

Run: `cargo test --lib pty::session::tests::hermes_prepare_workspace 2>&1 | tail -10`

Expected: all three tests pass.

- [ ] **Step 9.5: Commit**

```bash
git add src/pty/session.rs
git commit -m "feat(agent): add prepare_hermes_workspace orchestrator"
```

---

## Task 10: `build_hermes_command` — the spawn builder itself

The function constructs the `CommandBuilder` for spawning `hermes chat ...`. Pulls in `hermes_source_tag`, `latest_hermes_session_id_default`, and the `WSX_HERMES_*` env vars.

**Files:**
- Modify: `src/pty/session.rs`.
- Test: same file, `mod tests`. Use the `EnvGuard` pattern already present in the Pi tests at `src/pty/session.rs:1599`.

- [ ] **Step 10.1: Write the failing argv tests**

Append to `mod tests`:

```rust
mod hermes_build_command {
    use super::*;
    use std::ffi::OsStr;

    fn argv_strings(cmd: &portable_pty::CommandBuilder) -> Vec<String> {
        cmd.get_argv()
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect()
    }

    fn fresh_no_rename() -> SpawnMode {
        SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            additional_dirs: vec![],
            yolo: false,
        }
    }

    #[test]
    fn fresh_emits_chat_subcommand_and_source_tag() {
        let tmp = tempfile::tempdir().unwrap();
        let cmd = super::build_hermes_command(
            tmp.path(),
            &fresh_no_rename(),
            crate::remote_control::RemoteOpts::disabled(),
        );
        let argv = argv_strings(&cmd);
        // The first arg should be `chat` (after the bin name).
        assert_eq!(argv.first().map(|s| s.as_str()), Some("chat"), "argv: {argv:?}");
        // --source <wsx:...>
        let src_idx = argv.iter().position(|a| a == "--source").expect("expected --source");
        assert!(argv[src_idx + 1].starts_with("wsx:"), "argv: {argv:?}");
    }

    #[test]
    fn fresh_omits_continue_resume_and_yolo() {
        let tmp = tempfile::tempdir().unwrap();
        let cmd = super::build_hermes_command(
            tmp.path(),
            &fresh_no_rename(),
            crate::remote_control::RemoteOpts::disabled(),
        );
        let argv = argv_strings(&cmd);
        assert!(!argv.iter().any(|a| a == "--continue"), "argv: {argv:?}");
        assert!(!argv.iter().any(|a| a == "--resume"), "argv: {argv:?}");
        assert!(!argv.iter().any(|a| a == "--yolo"), "argv: {argv:?}");
    }

    #[test]
    fn yolo_fresh_emits_yolo_flag() {
        let tmp = tempfile::tempdir().unwrap();
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            additional_dirs: vec![],
            yolo: true,
        };
        let cmd = super::build_hermes_command(tmp.path(), &mode, crate::remote_control::RemoteOpts::disabled());
        assert!(argv_strings(&cmd).iter().any(|a| a == "--yolo"));
    }

    #[test]
    fn yolo_continue_emits_yolo_flag() {
        let tmp = tempfile::tempdir().unwrap();
        let mode = SpawnMode::Continue {
            custom_instructions: None,
            additional_dirs: vec![],
            yolo: true,
        };
        let cmd = super::build_hermes_command(tmp.path(), &mode, crate::remote_control::RemoteOpts::disabled());
        assert!(argv_strings(&cmd).iter().any(|a| a == "--yolo"));
    }

    #[test]
    fn project_manager_mode_is_always_yolo() {
        let tmp = tempfile::tempdir().unwrap();
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: std::path::PathBuf::from("/tmp/ws.json"),
            custom_instructions: None,
            additional_dirs: vec![],
            resume: false,
            fast_mode: false,
        };
        let cmd = super::build_hermes_command(tmp.path(), &mode, crate::remote_control::RemoteOpts::disabled());
        assert!(argv_strings(&cmd).iter().any(|a| a == "--yolo"));
    }

    #[test]
    fn no_worktree_flag_ever_emitted() {
        let tmp = tempfile::tempdir().unwrap();
        for mode in &[
            fresh_no_rename(),
            SpawnMode::Continue { custom_instructions: None, additional_dirs: vec![], yolo: true },
            SpawnMode::ProjectManager {
                workspaces_json_path: std::path::PathBuf::from("/tmp/ws.json"),
                custom_instructions: None,
                additional_dirs: vec![],
                resume: true,
                fast_mode: false,
            },
        ] {
            let cmd = super::build_hermes_command(tmp.path(), mode, crate::remote_control::RemoteOpts::disabled());
            let argv = argv_strings(&cmd);
            assert!(!argv.iter().any(|a| a == "--worktree" || a == "-w"),
                "should never emit --worktree; argv: {argv:?}");
        }
    }

    #[test]
    fn source_omitted_when_canonicalize_fails() {
        // Pass a path that doesn't exist — canonicalize returns Err, source omitted.
        let bogus = std::path::Path::new("/nonexistent/path/for/canonicalize");
        let cmd = super::build_hermes_command(
            bogus,
            &fresh_no_rename(),
            crate::remote_control::RemoteOpts::disabled(),
        );
        let argv = argv_strings(&cmd);
        assert!(!argv.iter().any(|a| a == "--source"),
            "expected --source to be absent when canonicalize fails; argv: {argv:?}");
        // chat subcommand should still be present.
        assert_eq!(argv.first().map(|s| s.as_str()), Some("chat"));
    }
}
```

- [ ] **Step 10.2: Run the tests to confirm they fail**

Run: `cargo test --lib pty::session::tests::hermes_build_command 2>&1 | tail -20`

Expected: compile error `cannot find function 'build_hermes_command'`.

- [ ] **Step 10.3: Implement `build_hermes_command`**

Add to `src/pty/session.rs`, near `build_pi_command`:

```rust
/// Build a `CommandBuilder` for `hermes chat` (or whatever `WSX_HERMES_BIN`
/// points to) inside `cwd`. Inherits the current process env.
///
/// Maps wsx spawn modes to Hermes CLI flags:
/// - `Fresh` → bare `hermes chat`, no continue/resume.
/// - `Continue` → `--resume <id>` if a prior wsx session exists for this cwd,
///   otherwise silently launches fresh (better than bare `--continue` which
///   would resume the globally-most-recent Hermes session regardless of cwd).
/// - `ProjectManager` → `--resume <id>` if `resume`, always `--yolo`.
///
/// Model selection uses env-var precedence:
///   1. `WSX_HERMES_MODEL` → set `HERMES_INFERENCE_MODEL` env var on the child
///      (works in all Hermes modes, unlike `--model` which is `-z/--tui` only).
///   2. `WSX_HERMES_PROVIDER` → forward as `--provider <value>` (may be a no-op
///      in classic REPL per Hermes docs; persistent provider lives in
///      `~/.hermes/config.yaml`).
///
/// `--worktree` is never emitted — wsx manages worktrees itself; passing it
/// would double-isolate.
///
/// Prompt injection (rename / custom_instructions / PM prompt) is handled
/// separately by `prepare_hermes_workspace`, which writes a wsx-managed
/// block into `AGENTS.md`.
pub fn build_hermes_command(
    cwd: &Path,
    mode: &SpawnMode,
    _remote: crate::remote_control::RemoteOpts,
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
        cmd.arg("--source");
        cmd.arg(&source);
    }

    let (add_continue, add_yolo) = match mode {
        SpawnMode::Continue { yolo, .. } => (true, *yolo),
        SpawnMode::Fresh { yolo, .. } => (false, *yolo),
        SpawnMode::ProjectManager { resume, .. } => (*resume, true),
    };

    if add_continue {
        if let Some(id) = latest_hermes_session_id_default(cwd) {
            cmd.arg("--resume");
            cmd.arg(&id);
        }
        // No prior wsx session → silently launch fresh.
    }
    if add_yolo {
        cmd.arg("--yolo");
    }

    let model = std::env::var("WSX_HERMES_MODEL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let provider = std::env::var("WSX_HERMES_PROVIDER")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if let Some(m) = &model {
        cmd.env("HERMES_INFERENCE_MODEL", m);
    }
    if let Some(p) = &provider {
        cmd.arg("--provider");
        cmd.arg(p);
    }

    cmd
}
```

- [ ] **Step 10.4: Run the tests to confirm the argv tests pass**

Run: `cargo test --lib pty::session::tests::hermes_build_command 2>&1 | tail -20`

Expected: all seven argv tests pass.

- [ ] **Step 10.5: Write the failing Continue+resume test using a temp db**

The Continue+resume path depends on `latest_hermes_session_id_default`, which resolves `~/.hermes/state.db`. Production hits the real DB. For a hermetic test, we set `HOME` to a tempdir via `EnvGuard` (the existing test scope helper) and seed a fake `~/.hermes/state.db` inside it.

Append to `mod hermes_build_command`:

```rust
    #[test]
    fn continue_without_prior_session_omits_resume() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let mut env = EnvGuard::new();
        env.set("HOME", tmp.path().to_string_lossy().as_ref());
        // Don't create ~/.hermes/state.db at all.
        let mode = SpawnMode::Continue {
            custom_instructions: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let cmd = super::build_hermes_command(cwd.path(), &mode, crate::remote_control::RemoteOpts::disabled());
        let argv = argv_strings(&cmd);
        assert!(!argv.iter().any(|a| a == "--resume"), "argv: {argv:?}");
        assert!(!argv.iter().any(|a| a == "--continue"), "argv: {argv:?}");
    }

    #[test]
    fn continue_with_prior_session_passes_resume_id() {
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        // Seed ~/.hermes/state.db with a row matching cwd's source tag.
        let hermes_dir = home.path().join(".hermes");
        std::fs::create_dir_all(&hermes_dir).unwrap();
        let db_path = hermes_dir.join("state.db");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (id TEXT PRIMARY KEY, source TEXT NOT NULL, started_at REAL NOT NULL);",
        ).unwrap();
        let tag = super::hermes_source_tag(cwd.path()).unwrap();
        conn.execute(
            "INSERT INTO sessions (id, source, started_at) VALUES ('session-abc', ?1, 1234.5);",
            rusqlite::params![tag],
        ).unwrap();
        drop(conn);

        let mut env = EnvGuard::new();
        env.set("HOME", home.path().to_string_lossy().as_ref());
        let mode = SpawnMode::Continue {
            custom_instructions: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let cmd = super::build_hermes_command(cwd.path(), &mode, crate::remote_control::RemoteOpts::disabled());
        let argv = argv_strings(&cmd);
        let idx = argv.iter().position(|a| a == "--resume").expect("expected --resume");
        assert_eq!(argv[idx + 1], "session-abc");
    }
```

- [ ] **Step 10.6: Run the resume tests to confirm they pass**

Run: `cargo test --lib pty::session::tests::hermes_build_command::continue 2>&1 | tail -10`

Expected: both `continue_*` tests pass.

If the test for `continue_with_prior_session_passes_resume_id` fails with "expected --resume", verify `dirs::home_dir()` is actually picking up the `HOME` env override on your platform. On macOS/Linux it does. On Windows it doesn't (uses `USERPROFILE`), so on Windows we'd need to also set `USERPROFILE`. The wsx codebase targets macOS/Linux per the existing `dirs` usage at `src/pty/session.rs:34`, so HOME-only is correct.

- [ ] **Step 10.7: Write the failing env-var tests**

Append to `mod hermes_build_command`:

```rust
    fn env_of(cmd: &portable_pty::CommandBuilder, key: &str) -> Option<String> {
        cmd.get_env(OsStr::new(key))
            .map(|v| v.to_string_lossy().into_owned())
    }

    #[test]
    fn wsx_hermes_model_env_sets_inference_model_env_on_child() {
        let tmp = tempfile::tempdir().unwrap();
        let mut env = EnvGuard::new();
        env.set("WSX_HERMES_MODEL", "deepseek/deepseek-v4-pro");
        env.remove("WSX_HERMES_PROVIDER");
        let cmd = super::build_hermes_command(
            tmp.path(),
            &fresh_no_rename(),
            crate::remote_control::RemoteOpts::disabled(),
        );
        assert_eq!(
            env_of(&cmd, "HERMES_INFERENCE_MODEL"),
            Some("deepseek/deepseek-v4-pro".to_string())
        );
        let argv = argv_strings(&cmd);
        assert!(!argv.iter().any(|a| a == "--model"), "argv: {argv:?}");
    }

    #[test]
    fn wsx_hermes_provider_env_passes_provider_flag() {
        let tmp = tempfile::tempdir().unwrap();
        let mut env = EnvGuard::new();
        env.remove("WSX_HERMES_MODEL");
        env.set("WSX_HERMES_PROVIDER", "openrouter");
        let cmd = super::build_hermes_command(
            tmp.path(),
            &fresh_no_rename(),
            crate::remote_control::RemoteOpts::disabled(),
        );
        let argv = argv_strings(&cmd);
        let idx = argv.iter().position(|a| a == "--provider").expect("expected --provider");
        assert_eq!(argv[idx + 1], "openrouter");
    }

    #[test]
    fn empty_model_env_treated_as_unset() {
        let tmp = tempfile::tempdir().unwrap();
        let mut env = EnvGuard::new();
        env.set("WSX_HERMES_MODEL", "   ");
        env.set("WSX_HERMES_PROVIDER", "");
        let cmd = super::build_hermes_command(
            tmp.path(),
            &fresh_no_rename(),
            crate::remote_control::RemoteOpts::disabled(),
        );
        assert!(env_of(&cmd, "HERMES_INFERENCE_MODEL").is_none());
        let argv = argv_strings(&cmd);
        assert!(!argv.iter().any(|a| a == "--provider"), "argv: {argv:?}");
    }
```

- [ ] **Step 10.8: Verify `CommandBuilder::get_env` is available**

Run:
```bash
cargo doc --no-deps --open 2>&1 | grep -i 'opening' | head -3
```

Or just `cargo build --tests` — if `get_env` is missing from `portable_pty::CommandBuilder` in the version pinned in `Cargo.toml`, the env tests will fail to compile. In that case, the fallback is to factor a small pure function `hermes_extra_env_overrides(...) -> Vec<(&'static str, String)>` that `build_hermes_command` calls and applies via `cmd.env(...)`, and test that pure function directly instead of `env_of`. This refactor is small (~15 LOC, ~5 minutes); update the env tests to assert on the returned `Vec<(&'static str, String)>` instead.

- [ ] **Step 10.9: Run all `hermes_build_command` tests**

Run: `cargo test --lib pty::session::tests::hermes_build_command 2>&1 | tail -20`

Expected: all tests pass.

- [ ] **Step 10.10: Commit**

```bash
git add src/pty/session.rs
git commit -m "feat(agent): add build_hermes_command spawn builder"
```

---

## Task 11: Wire the dispatcher to call `prepare_hermes_workspace` + `build_hermes_command`

Replace the Task 1 placeholder arm with the real call.

**Files:**
- Modify: `src/pty/session.rs` (the dispatcher around line 609).

- [ ] **Step 11.1: Update the dispatcher arm**

Locate the `match agent { ... }` near line 609. Replace the Hermes arm with:

```rust
AgentKind::Claude => build_claude_command(cwd, &mode, remote),
AgentKind::Pi => build_pi_command(cwd, &mode, remote),
AgentKind::Hermes => {
    prepare_hermes_workspace(cwd, &mode);
    build_hermes_command(cwd, &mode, remote)
}
```

- [ ] **Step 11.2: Run all session tests to confirm no regressions**

Run: `cargo test --lib pty::session 2>&1 | tail -20`

Expected: all tests pass.

- [ ] **Step 11.3: Commit**

```bash
git add src/pty/session.rs
git commit -m "feat(agent): wire hermes dispatcher to real builder"
```

---

## Task 12: Modal Tab toggle test for the 3-cycle

If the modal toggle has existing test coverage, extend it. If not, this task is documentation-only and can be skipped.

**Files:**
- Modify: `src/app/input.rs` (test for the new 3-cycle, if there's an existing test for the 2-cycle).

- [ ] **Step 12.1: Check whether the toggle has existing test coverage**

Run: `grep -n 'AgentKind' src/app/input.rs | head -20`

If you find a `#[test]` function that exercises `KeyCode::Tab` for `Modal::NewWorkspace`, extend it (Step 12.2). If no such test exists, skip to Step 12.3 (the compiler enforces match exhaustiveness, so the cycle correctness is structural).

- [ ] **Step 12.2: If a Tab toggle test exists, extend it**

Locate the test. Add or replace assertions to cover the 3-cycle:

```rust
// Three Tabs should cycle through all three agents and return to start.
assert_eq!(initial_agent, AgentKind::Claude);
// Tab 1: Claude → Pi
press_tab();
assert_eq!(current_agent, AgentKind::Pi);
// Tab 2: Pi → Hermes
press_tab();
assert_eq!(current_agent, AgentKind::Hermes);
// Tab 3: Hermes → Claude
press_tab();
assert_eq!(current_agent, AgentKind::Claude);
```

Adapt the helper names (`press_tab`, `current_agent`) to whatever the existing test uses.

- [ ] **Step 12.3: Run lib tests**

Run: `cargo test --lib 2>&1 | tail -10`

Expected: all tests pass.

- [ ] **Step 12.4: Commit (only if Step 12.2 made changes)**

```bash
git add src/app/input.rs
git commit -m "test(agent): cover 3-cycle hermes toggle in modal"
```

If no test existed and you didn't change anything, skip this step.

---

## Task 13: README

Document the new agent, env vars, and the AGENTS.md side effect.

**Files:**
- Modify: `README.md`.

- [ ] **Step 13.1: Find the existing agent-mention section**

Run: `grep -n 'claude\|pi\b\|coding_agent\|WSX_PI\|WSX_CLAUDE' README.md`

Expected: matches show where the agents are enumerated. If there's a settings/env-var section, that's where the new content goes.

- [ ] **Step 13.2: Add Hermes to the agent enumeration**

Find whatever section enumerates the supported agents (likely a list or table) and add a Hermes row/bullet. Match the existing style. Example:

```markdown
- **Hermes** (`coding_agent = "hermes"`) — Nous Research's self-improving agent ([nousresearch/hermes-agent](https://github.com/nousresearch/hermes-agent)).
  Configured via `~/.hermes/config.yaml`.
```

- [ ] **Step 13.3: Add the env-var section**

Append (or extend an existing env-vars section) with:

```markdown
### Hermes-specific env vars

- `WSX_HERMES_BIN` — override the `hermes` binary path. Default: `hermes`.
- `WSX_HERMES_MODEL` — model override, set as `HERMES_INFERENCE_MODEL` on the child. Example: `anthropic/claude-sonnet-4.6`.
- `WSX_HERMES_PROVIDER` — provider override, forwarded as `--provider`. Per Hermes docs, this flag may only apply to `-z/--oneshot` and `--tui` modes; if your launches are classic REPL, the persistent provider in `~/.hermes/config.yaml` wins.

### AGENTS.md side effect

Because Hermes has no `--append-system-prompt` flag, wsx writes the auto-rename instructions and per-workspace custom instructions into a fenced `<!-- BEGIN wsx-managed --> ... <!-- END wsx-managed -->` block in `AGENTS.md` at the worktree root. The block is rewritten on every spawn and disappears once there's nothing to inject.

- If your repo doesn't already track `AGENTS.md`, wsx adds it to `.git/info/exclude` of the worktree so it doesn't appear in `git status`.
- If your repo *does* track `AGENTS.md`, the worktree will show the file as modified. This is expected — wsx is appending the wsx block to your existing content; the block strips automatically on subsequent spawns when not needed.
```

(If the README doesn't have an env-vars or side-effect section today, create one at an appropriate place — likely after the existing Pi documentation.)

- [ ] **Step 13.4: Verify the spec's open items are accurately reflected**

Skim the "Open items deferred" section of `docs/superpowers/specs/2026-05-28-add-hermes-agent-design.md`. If any of those (PM-on-Hermes prompt fit, tracked-AGENTS.md visibility, stale session ID) deserves a README mention, add a "Known limitations" sub-bullet.

- [ ] **Step 13.5: Commit**

```bash
git add README.md
git commit -m "docs: document hermes agent and WSX_HERMES_* env vars"
```

---

## Task 14: Full test + build verification

- [ ] **Step 14.1: Run the full test suite**

Run: `cargo test 2>&1 | tail -20`

Expected: every test passes. Watch especially for:
- `pty::session::tests::hermes_*` — all new tests.
- Existing `pty::session::tests::*` for Claude and Pi — no regressions.
- `tests/smoke.rs`, `tests/branch_drift.rs` — agent-agnostic, should be unaffected.

- [ ] **Step 14.2: Run clippy**

Run: `cargo clippy --all-targets -- -D warnings 2>&1 | tail -20`

Expected: no warnings. Fix anything that's flagged. Common issues you may hit:
- `redundant_closure` — `.map(|s| s.trim().to_string())` may suggest something. Match the existing Pi code's style verbatim — `:526-529` in `session.rs` does the same pattern. If clippy flags one and not the other, it's a clippy version bump; safe to suppress on the new code with `#[allow(clippy::...)]` to match the file convention.
- Unused imports — the new code introduces `rusqlite` and `dirs` usage in `session.rs`. If clippy flags them as redundant, double-check the test module also references them.

- [ ] **Step 14.3: Run fmt**

Run: `cargo fmt && git diff --stat`

Expected: either no diff or only whitespace-noise changes from the new code. Commit any fmt-only changes separately:

```bash
git add -u
git commit -m "style(agent): cargo fmt hermes additions"
```

- [ ] **Step 14.4: Manual smoke test**

In a separate terminal:

```bash
cargo run -- workspace create <some-test-repo-name> --agent hermes
```

Expected: a new worktree is created, wsx switches to it, and `hermes chat --source wsx:...` launches inside it. The placeholder branch (`bakedbean/<adjective>-<plant>`) should be visible at startup; after you send a first message describing your task, the rename prompt in `AGENTS.md` should fire and rename the branch to something like `bakedbean/<topic-slug>`.

If you don't have a registered test repo, register one first with `cargo run -- repo add <path>`.

If the spawn fails with "hermes: command not found", verify `which hermes` resolves and consider setting `WSX_HERMES_BIN` to an absolute path.

---

## Self-review

### Spec coverage

| Spec requirement | Implementing task |
|---|---|
| `AgentKind::Hermes` variant + `from_store` | Task 1 |
| `coding_agent = "hermes"` setting honored | Task 1 (from_store) |
| `--agent hermes` CLI accepted | Task 1 (cli.rs edits) |
| Modal Tab 3-cycle | Task 1 (input.rs edit), test in Task 12 |
| Modal "hermes" label | Task 1 (modal.rs edit) |
| `--source wsx:<encoded-cwd>` tag | Tasks 2, 10 |
| `--continue` / `--resume <id>` semantics | Tasks 3, 4, 10 |
| `--yolo` on Fresh/Continue/PM | Task 10 |
| `WSX_HERMES_BIN` env override | Task 10 |
| `WSX_HERMES_MODEL` → `HERMES_INFERENCE_MODEL` env | Task 10 |
| `WSX_HERMES_PROVIDER` → `--provider` flag | Task 10 |
| Never emit `--worktree` | Task 10 (test guard) |
| Rename system prompt via `AGENTS.md` | Tasks 6, 7, 8, 9 |
| Custom instructions via `AGENTS.md` | Tasks 6, 8, 9 |
| PM mode prompt via `AGENTS.md` | Tasks 6, 8, 9 |
| `.git/info/exclude` hide of new `AGENTS.md` | Tasks 5, 9 |
| Idempotent rewrite of `AGENTS.md` wsx block | Task 6 (replace/strip tests) |
| `~/.hermes/state.db` ro+immutable read | Task 3 |
| `has_prior_session_for(_, Hermes)` | Task 4 |
| Continue resumes correct per-cwd session | Tasks 3, 4, 10 |
| README documents env vars + side effects | Task 13 |
| Tests for all of the above | Tasks 2-10 (per-task) |

No gaps.

### Placeholder scan

Searched the plan for `TBD`, `TODO`, `fill in`, `appropriate`, "implement later", and references to undefined helpers. The only `TODO` is a code comment in Step 1.4's stub, intentionally tagged for Task 13.

### Type and signature consistency

- `hermes_source_tag` defined in Task 2 (`fn hermes_source_tag(worktree: &Path) -> Option<String>`), used in Tasks 3, 10. ✓
- `latest_hermes_session_id` defined in Task 3 (`fn latest_hermes_session_id(db_path: &Path, worktree: &Path) -> Option<String>`), wrapped in Task 4. ✓
- `latest_hermes_session_id_default` defined in Task 4 (`pub fn latest_hermes_session_id_default(worktree: &Path) -> Option<String>`), used in Task 10. ✓
- `has_prior_hermes_session` defined in Task 4 (`pub fn has_prior_hermes_session(worktree: &Path) -> bool`), wired in Task 4 dispatcher. ✓
- `ensure_git_exclude` defined in Task 5 (`fn ensure_git_exclude(worktree: &Path, name: &str)`), used in Task 9. ✓
- `write_agents_md_section` defined in Task 6 (`fn write_agents_md_section(cwd: &Path, content: Option<&str>)`), used in Task 9. ✓
- `strip_wsx_block` defined in Task 6 (private helper to `write_agents_md_section`). ✓
- `render_rename_system_prompt_hermes` defined in Task 7 (`fn render_rename_system_prompt_hermes(current_branch: &str, branch_prefix: &str) -> String`), used in Task 8. ✓
- `compose_injected_prompt` defined in Task 8 (`fn compose_injected_prompt(mode: &SpawnMode) -> Option<String>`), used in Task 9. ✓
- `prepare_hermes_workspace` defined in Task 9 (`fn prepare_hermes_workspace(cwd: &Path, mode: &SpawnMode)`), used in Task 11 dispatcher. ✓
- `build_hermes_command` defined in Task 10 (`pub fn build_hermes_command(cwd: &Path, mode: &SpawnMode, _remote: crate::remote_control::RemoteOpts) -> CommandBuilder`), used in Task 11. ✓
- Marker constants `HERMES_BLOCK_BEGIN` / `HERMES_BLOCK_END` defined in Task 6; the test file references the literal strings to avoid coupling tests to the constant names. ✓

All signatures consistent. No drift between definitions and call sites.
