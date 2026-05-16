# MCP server mirroring — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the source repo's MCP servers available in worktrees by mirroring `~/.claude.json:projects[<repo_path>].mcpServers` into `projects[<worktree_path>].mcpServers` on every workspace session spawn, and cleaning up on archive.

**Architecture:** New `src/mcp.rs` module with two best-effort functions (`mirror_mcp_servers`, `remove_worktree_entry`) backed by atomic file replacement. Wire them into `workspace::create`, the re-attach path in `app.rs`, and `workspace::archive`. Mirror failures log a warning and do not block spawn.

**Tech Stack:** Rust, `serde_json::Value` (untyped read so we don't model claude-code's schema), `tempfile`-style temp-then-rename atomic writes, `dirs::home_dir()` for the file path.

**Spec:** `docs/superpowers/specs/2026-05-16-mcp-server-mirroring-design.md`

---

## File Structure

- `src/mcp.rs` (new) — pure module with `mirror_mcp_servers`, `remove_worktree_entry`, plus a path-injectable inner pair (`mirror_into`, `remove_into`) for unit tests.
- `src/lib.rs` — `pub mod mcp;`.
- `src/cli.rs` — add `"mcp_mirror"` to `known_setting_key`.
- `src/workspace.rs` — hook into `create` and `archive`, gated by `mcp_mirror_enabled(&store)`.
- `src/app.rs` — hook into the re-attach path where a workspace session is (re)spawned, gated by the same helper.
- `README.md` — new "MCP server inheritance" subsection under Settings.

The `mcp_mirror_enabled` helper lives in `src/mcp.rs` since it owns the setting key string. Signature: `pub fn enabled(store: &crate::store::Store) -> bool` — reads `store.get_setting("mcp_mirror")`, defaults to true.

No deletions. No keybind changes. No new dependencies (uses `serde_json` and `dirs` already in `Cargo.toml`).

---

### Task 1: Module skeleton + atomic read/write helpers

**Files:**
- Create: `src/mcp.rs`
- Modify: `src/lib.rs` (add `pub mod mcp;`)

- [ ] **Step 1: Write failing tests for the read/write helpers**

Create `src/mcp.rs` with a `#[cfg(test)] mod tests` block at the bottom, and write these tests first:

```rust
#[test]
fn read_claude_json_missing_returns_none() {
    let dir = tempfile::TempDir::new().unwrap();
    let p = dir.path().join("nope.json");
    assert!(read_claude_json(&p).unwrap().is_none());
}

#[test]
fn read_claude_json_existing_returns_value() {
    let dir = tempfile::TempDir::new().unwrap();
    let p = dir.path().join(".claude.json");
    std::fs::write(&p, r#"{"foo": 1}"#).unwrap();
    let v = read_claude_json(&p).unwrap().unwrap();
    assert_eq!(v["foo"], serde_json::json!(1));
}

#[test]
fn write_claude_json_atomic_roundtrip() {
    let dir = tempfile::TempDir::new().unwrap();
    let p = dir.path().join(".claude.json");
    let v = serde_json::json!({"hello": "world"});
    write_claude_json_atomic(&p, &v).unwrap();
    let back = read_claude_json(&p).unwrap().unwrap();
    assert_eq!(back, v);
    // No stray tempfiles left behind.
    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains("wsx-tmp"))
        .collect();
    assert!(leftovers.is_empty(), "expected no temp files, got {leftovers:?}");
}
```

- [ ] **Step 2: Run tests to verify they fail**

```
cargo test --lib mcp:: -- --test-threads=1 2>&1 | tail -10
```

Expected: compile errors — `read_claude_json` / `write_claude_json_atomic` not defined.

- [ ] **Step 3: Add the helpers and module declaration**

In `src/mcp.rs`:

```rust
//! Mirror MCP server config from a source repo's project entry in
//! `~/.claude.json` into a worktree's entry, so claude sees the same
//! servers when launched in a worktree path. See
//! `docs/superpowers/specs/2026-05-16-mcp-server-mirroring-design.md`.

use crate::error::{Error, Result};
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn claude_json_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude.json"))
}

fn read_claude_json(path: &Path) -> Result<Option<Value>> {
    match fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => Ok(None),
        Ok(s) => {
            let v: Value = serde_json::from_str(&s)
                .map_err(|e| Error::Pty(format!("parse ~/.claude.json: {e}")))?;
            Ok(Some(v))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::Pty(format!("read ~/.claude.json: {e}"))),
    }
}

fn write_claude_json_atomic(path: &Path, value: &Value) -> Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    let pid = std::process::id();
    let tmp = parent.join(format!(
        ".claude.json.wsx-tmp.{pid}.{}",
        rand::random::<u32>()
    ));
    let serialized = serde_json::to_string_pretty(value)
        .map_err(|e| Error::Pty(format!("serialize ~/.claude.json: {e}")))?;
    // Scope the file handle so the OS closes/flushes before rename.
    {
        let mut f = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp)
            .map_err(|e| Error::Pty(format!("open tempfile: {e}")))?;
        f.write_all(serialized.as_bytes())
            .map_err(|e| Error::Pty(format!("write tempfile: {e}")))?;
        f.sync_all()
            .map_err(|e| Error::Pty(format!("fsync tempfile: {e}")))?;
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        Error::Pty(format!("rename tempfile: {e}"))
    })?;
    Ok(())
}
```

`Error::Pty` is reused because there is no `Error::Io`-style variant in `wsx::error`; check the actual variants first and pick the closest one (likely `UserInput` is wrong; if a generic variant exists use it; otherwise reuse `Pty` and follow up by adding a proper variant if it becomes noisy).

Reasoning: `rand::random::<u32>()` so two concurrent wsx instances (rare but possible) don't collide on tempfile names with just the pid.

Add to `src/lib.rs` alphabetically:

```rust
pub mod mcp;
```

- [ ] **Step 4: Run the tests to verify they pass**

```
cargo test --lib mcp:: -- --test-threads=1 2>&1 | tail -10
```

Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add src/mcp.rs src/lib.rs
git commit -m "feat(mcp): atomic ~/.claude.json read/write helpers"
```

---

### Task 2: `mirror_into` — the pure mirroring logic

**Files:**
- Modify: `src/mcp.rs` (add `mirror_into`, plus tests)

- [ ] **Step 1: Write failing tests**

Append to the tests module in `src/mcp.rs`:

```rust
fn write_json(path: &Path, v: &Value) {
    std::fs::write(path, serde_json::to_string_pretty(v).unwrap()).unwrap();
}

fn read_json(path: &Path) -> Value {
    let s = std::fs::read_to_string(path).unwrap();
    serde_json::from_str(&s).unwrap()
}

#[test]
fn mirror_into_no_file_is_noop() {
    let dir = tempfile::TempDir::new().unwrap();
    let p = dir.path().join("nope.json");
    mirror_into(&p, Path::new("/r"), Path::new("/wt")).unwrap();
    assert!(!p.exists(), "should not create file when missing");
}

#[test]
fn mirror_into_no_source_entry_is_noop() {
    let dir = tempfile::TempDir::new().unwrap();
    let p = dir.path().join(".claude.json");
    let original = serde_json::json!({
        "projects": {
            "/some/other": {"mcpServers": {"x": {}}}
        }
    });
    write_json(&p, &original);
    mirror_into(&p, Path::new("/r"), Path::new("/wt")).unwrap();
    let after = read_json(&p);
    assert_eq!(after, original, "file should be byte-equivalent (no mirror)");
}

#[test]
fn mirror_into_no_source_mcp_is_noop() {
    let dir = tempfile::TempDir::new().unwrap();
    let p = dir.path().join(".claude.json");
    let original = serde_json::json!({
        "projects": {
            "/r": {"lastSessionId": "abc"}
        }
    });
    write_json(&p, &original);
    mirror_into(&p, Path::new("/r"), Path::new("/wt")).unwrap();
    let after = read_json(&p);
    assert_eq!(after, original);
}

#[test]
fn mirror_into_happy_path_creates_worktree_entry() {
    let dir = tempfile::TempDir::new().unwrap();
    let p = dir.path().join(".claude.json");
    write_json(&p, &serde_json::json!({
        "projects": {
            "/r": {"mcpServers": {"datadog": {"type": "http"}}}
        }
    }));
    mirror_into(&p, Path::new("/r"), Path::new("/wt")).unwrap();
    let after = read_json(&p);
    assert_eq!(
        after["projects"]["/wt"]["mcpServers"],
        serde_json::json!({"datadog": {"type": "http"}})
    );
}

#[test]
fn mirror_into_preserves_existing_worktree_fields() {
    let dir = tempfile::TempDir::new().unwrap();
    let p = dir.path().join(".claude.json");
    write_json(&p, &serde_json::json!({
        "projects": {
            "/r": {"mcpServers": {"datadog": {"type": "http"}}},
            "/wt": {"lastSessionId": "keep-me", "mcpServers": {"old": {}}}
        }
    }));
    mirror_into(&p, Path::new("/r"), Path::new("/wt")).unwrap();
    let after = read_json(&p);
    assert_eq!(after["projects"]["/wt"]["lastSessionId"], serde_json::json!("keep-me"));
    assert_eq!(
        after["projects"]["/wt"]["mcpServers"],
        serde_json::json!({"datadog": {"type": "http"}}),
        "stale mcpServers should be overwritten"
    );
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```
cargo test --lib mcp::tests::mirror_into -- --test-threads=1 2>&1 | tail -15
```

Expected: compile errors — `mirror_into` not defined.

- [ ] **Step 3: Implement `mirror_into`**

In `src/mcp.rs`:

```rust
/// Pure form of `mirror_mcp_servers` that takes the claude.json path
/// directly, for testability.
fn mirror_into(claude_json: &Path, repo: &Path, worktree: &Path) -> Result<()> {
    let Some(mut root) = read_claude_json(claude_json)? else {
        return Ok(()); // missing file → nothing to mirror
    };
    let repo_key = repo.to_string_lossy().into_owned();
    let worktree_key = worktree.to_string_lossy().into_owned();
    let Some(servers) = root
        .get("projects")
        .and_then(|p| p.get(&repo_key))
        .and_then(|r| r.get("mcpServers"))
        .cloned()
    else {
        return Ok(()); // no source mcpServers → nothing to do
    };
    let projects = root
        .as_object_mut()
        .and_then(|o| o.get_mut("projects"))
        .and_then(|p| p.as_object_mut())
        .ok_or_else(|| Error::Pty("projects is not an object".into()))?;
    let entry = projects
        .entry(worktree_key)
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let obj = entry
        .as_object_mut()
        .ok_or_else(|| Error::Pty("worktree entry is not an object".into()))?;
    obj.insert("mcpServers".into(), servers);
    write_claude_json_atomic(claude_json, &root)
}
```

- [ ] **Step 4: Run tests**

```
cargo test --lib mcp::tests::mirror_into -- --test-threads=1 2>&1 | tail -15
```

Expected: 5 passed.

- [ ] **Step 5: Add the public entry point**

```rust
/// Mirror `projects[repo_path].mcpServers` → `projects[worktree_path].mcpServers`
/// in `~/.claude.json`. No-op when the file or the source entry is absent.
/// Errors are returned but callers should treat them as best-effort.
pub fn mirror_mcp_servers(repo_path: &Path, worktree_path: &Path) -> Result<()> {
    let Some(p) = claude_json_path() else {
        return Ok(());
    };
    mirror_into(&p, repo_path, worktree_path)
}
```

- [ ] **Step 6: Commit**

```bash
git add src/mcp.rs
git commit -m "feat(mcp): mirror_mcp_servers + mirror_into"
```

---

### Task 3: `remove_into` — archive cleanup

**Files:**
- Modify: `src/mcp.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn remove_into_no_file_is_noop() {
    let dir = tempfile::TempDir::new().unwrap();
    let p = dir.path().join("nope.json");
    remove_into(&p, Path::new("/wt")).unwrap();
    assert!(!p.exists());
}

#[test]
fn remove_into_no_entry_is_noop() {
    let dir = tempfile::TempDir::new().unwrap();
    let p = dir.path().join(".claude.json");
    let original = serde_json::json!({"projects": {"/other": {}}});
    write_json(&p, &original);
    remove_into(&p, Path::new("/wt")).unwrap();
    assert_eq!(read_json(&p), original);
}

#[test]
fn remove_into_drops_full_entry() {
    let dir = tempfile::TempDir::new().unwrap();
    let p = dir.path().join(".claude.json");
    write_json(&p, &serde_json::json!({
        "projects": {
            "/r": {"mcpServers": {}},
            "/wt": {"mcpServers": {"x": {}}, "lastSessionId": "abc"}
        }
    }));
    remove_into(&p, Path::new("/wt")).unwrap();
    let after = read_json(&p);
    assert!(after["projects"].get("/wt").is_none(), "expected /wt removed: {after:#}");
    assert!(after["projects"]["/r"].is_object(), "other entries preserved");
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```
cargo test --lib mcp::tests::remove_into -- --test-threads=1 2>&1 | tail -10
```

Expected: compile errors.

- [ ] **Step 3: Implement `remove_into` + public entry**

```rust
fn remove_into(claude_json: &Path, worktree: &Path) -> Result<()> {
    let Some(mut root) = read_claude_json(claude_json)? else {
        return Ok(());
    };
    let worktree_key = worktree.to_string_lossy().into_owned();
    let Some(projects) = root
        .as_object_mut()
        .and_then(|o| o.get_mut("projects"))
        .and_then(|p| p.as_object_mut())
    else {
        return Ok(());
    };
    if projects.remove(&worktree_key).is_none() {
        return Ok(()); // nothing to do
    }
    write_claude_json_atomic(claude_json, &root)
}

/// Remove `projects[worktree_path]` from `~/.claude.json`. No-op when
/// the file or entry is missing. Best-effort: callers should ignore
/// errors (log + continue).
pub fn remove_worktree_entry(worktree_path: &Path) -> Result<()> {
    let Some(p) = claude_json_path() else {
        return Ok(());
    };
    remove_into(&p, worktree_path)
}
```

- [ ] **Step 4: Run tests**

```
cargo test --lib mcp::tests::remove_into -- --test-threads=1 2>&1 | tail -10
```

Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add src/mcp.rs
git commit -m "feat(mcp): remove_worktree_entry for archive cleanup"
```

---

### Task 3b: Toggle setting (`mcp_mirror`)

**Files:**
- Modify: `src/cli.rs` (add to `known_setting_key`)
- Modify: `src/mcp.rs` (add `enabled` helper)

- [ ] **Step 1: Write failing tests**

In `src/mcp.rs` tests:

```rust
#[test]
fn enabled_defaults_true_when_unset() {
    let store = crate::store::Store::open_in_memory().unwrap();
    assert!(enabled(&store));
}

#[test]
fn enabled_false_when_setting_off() {
    let store = crate::store::Store::open_in_memory().unwrap();
    store.set_setting("mcp_mirror", "false").unwrap();
    assert!(!enabled(&store));
    store.set_setting("mcp_mirror", "off").unwrap();
    assert!(!enabled(&store));
    store.set_setting("mcp_mirror", "0").unwrap();
    assert!(!enabled(&store));
    store.set_setting("mcp_mirror", "no").unwrap();
    assert!(!enabled(&store));
}

#[test]
fn enabled_true_for_unrecognized_truthy_values() {
    let store = crate::store::Store::open_in_memory().unwrap();
    store.set_setting("mcp_mirror", "true").unwrap();
    assert!(enabled(&store));
    store.set_setting("mcp_mirror", "yes").unwrap();
    assert!(enabled(&store));
}
```

In `src/cli.rs::tests`:

```rust
#[test]
fn known_setting_includes_mcp_mirror() {
    assert!(known_setting_key("mcp_mirror"));
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```
cargo test --lib mcp::tests::enabled known_setting_includes_mcp_mirror -- --test-threads=1 2>&1 | tail -10
```

Expected: compile errors.

- [ ] **Step 3: Add `enabled` to `src/mcp.rs`**

```rust
/// Whether the mirror feature is enabled. Defaults to ON; the user can
/// opt out with `wsx settings set mcp_mirror false`.
pub fn enabled(store: &crate::store::Store) -> bool {
    match store.get_setting("mcp_mirror").ok().flatten().as_deref() {
        Some("false") | Some("off") | Some("0") | Some("no") => false,
        _ => true,
    }
}
```

- [ ] **Step 4: Add the key to `known_setting_key`**

In `src/cli.rs::known_setting_key`, insert `"mcp_mirror"` into the alphabetized list (between `"editor_cmd"` and `"nerd_fonts"` to keep order roughly grouped, or just append — match existing convention).

- [ ] **Step 5: Run tests to confirm they pass**

```
cargo test --lib mcp::tests::enabled known_setting_includes_mcp_mirror -- --test-threads=1 2>&1 | tail -10
```

Expected: 4 passed.

- [ ] **Step 6: Commit**

```bash
git add src/mcp.rs src/cli.rs
git commit -m "feat(mcp): mcp_mirror global toggle (default on)"
```

---

### Task 4: Hook into workspace create (spawn-time mirror)

**Files:**
- Modify: `src/workspace.rs` (find the spawn site in `create`)

- [ ] **Step 1: Find the spawn site**

```
grep -n "sessions.spawn\|spawn_session" src/workspace.rs
```

In `workspace::create`, the call sequence is roughly: insert workspace row → create worktree → spawn session. The mirror call must go **between worktree creation** (so the path exists; not strictly required for the mirror but matches lifecycle) and **the spawn** (so claude reads the freshly-mirrored entry at startup).

- [ ] **Step 2: Add the gated mirror call**

Right before `sessions.spawn(...)`, add:

```rust
if crate::mcp::enabled(&store) {
    if let Err(e) = crate::mcp::mirror_mcp_servers(&repo.path, &workspace.worktree_path) {
        tracing::warn!(error = %e, "failed to mirror MCP servers; continuing");
    }
}
```

`store`, `repo`, and `workspace` are the names in `create`'s local scope (verify before pasting; rename if different).

- [ ] **Step 3: Add a test asserting the mirror runs**

In the existing `workspace::tests` module (search for `mod tests` in `src/workspace.rs`):

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn create_mirrors_mcp_servers_into_worktree_entry() {
    use crate::store::Store;
    use tempfile::TempDir;
    // Redirect HOME so wsx writes to a sandbox claude.json.
    let home = TempDir::new().unwrap();
    let original_home = std::env::var_os("HOME");
    unsafe { std::env::set_var("HOME", home.path()); }

    // Source repo entry with one MCP server.
    let claude_json = home.path().join(".claude.json");
    let repo_path = home.path().join("repo");
    std::fs::create_dir_all(&repo_path).unwrap();
    let canonical_repo = std::fs::canonicalize(&repo_path).unwrap();
    let canonical_repo_key = canonical_repo.to_string_lossy().to_string();
    std::fs::write(
        &claude_json,
        serde_json::to_string_pretty(&serde_json::json!({
            "projects": {
                canonical_repo_key: {"mcpServers": {"datadog": {"type": "http"}}}
            }
        }))
        .unwrap(),
    )
    .unwrap();

    // Initialize the repo as a git repo and set wsx-required scaffolding.
    // ... follow the existing pattern from create_makes_worktree_and_inserts_row.
    // ... call create(...) here.

    // After create: the worktree path entry should have mcpServers.
    let after: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&claude_json).unwrap()).unwrap();
    let workspaces = after["projects"]
        .as_object()
        .unwrap()
        .keys()
        .filter(|k| k != &&canonical_repo_key)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(workspaces.len(), 1, "expected exactly one new project entry");
    assert_eq!(
        after["projects"][&workspaces[0]]["mcpServers"]["datadog"]["type"],
        serde_json::json!("http")
    );

    if let Some(h) = original_home {
        unsafe { std::env::set_var("HOME", h); }
    }
}
```

The "follow the existing pattern" pointer means: read whatever the closest existing `create_*` test does for git setup, repo registration, and the `create` call. Don't fabricate — use the same calls.

- [ ] **Step 4: Add a second test for the toggle**

Same scaffolding as Step 3's test, but before calling `create`:

```rust
store.set_setting("mcp_mirror", "false").unwrap();
```

After `create`, assert that no new project entry appeared (only the source repo's entry is present in `projects`):

```rust
let projects = after["projects"].as_object().unwrap();
assert_eq!(
    projects.len(),
    1,
    "with mcp_mirror=false, no worktree entry should be added: {projects:#?}"
);
```

Name this test `create_does_not_mirror_when_mcp_mirror_disabled`.

- [ ] **Step 5: Run both tests**

```
cargo test --lib create_mirrors_mcp_servers create_does_not_mirror_when_mcp_mirror_disabled -- --test-threads=1 2>&1 | tail -10
```

Expected: 2 passed.

- [ ] **Step 6: Commit**

```bash
git add src/workspace.rs
git commit -m "feat(workspace): mirror MCP servers on workspace create (gated by mcp_mirror)"
```

---

### Task 5: Hook into re-attach spawn path

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Find the re-attach spawn site**

```
grep -n "sessions.spawn\(" src/app.rs
```

In `app.rs`, the re-attach handler runs when the user presses Enter on a workspace whose session has exited. Right before `app.sessions.spawn(...)` (workspace, not PM), add the same mirror call.

- [ ] **Step 2: Add the gated mirror call**

```rust
// Refresh MCP server mirror so changes to the source repo's
// claude.json entry propagate without a workspace recreate.
if crate::mcp::enabled(&app.store) {
    let repo_path = app
        .repos
        .iter()
        .find(|r| r.id == workspace.repo_id)
        .map(|r| r.path.clone());
    if let Some(repo_path) = repo_path {
        if let Err(e) =
            crate::mcp::mirror_mcp_servers(&repo_path, &workspace.worktree_path)
        {
            tracing::warn!(error = %e, "MCP mirror failed; continuing");
        }
    }
}
```

Adjust variable names to match the actual local bindings at the spawn site (workspace might be a borrow named `w`; check before pasting).

- [ ] **Step 3: Build to verify it compiles**

```
cargo build 2>&1 | tail -5
```

Expected: clean.

- [ ] **Step 4: Skip unit test for re-attach path**

There is no clean unit-level seam for re-attach without spinning up a full PTY + simulated session-exit, which is overkill. The Task 4 test already covers `mirror_mcp_servers` end-to-end. Manual smoke (in Task 7) verifies the re-attach path.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): re-mirror MCP servers on session re-attach"
```

---

### Task 6: Hook into workspace archive

**Files:**
- Modify: `src/workspace.rs::archive` (or wherever the archive happens)

- [ ] **Step 1: Find archive**

```
grep -n "pub fn archive\|pub async fn archive\|fn archive" src/workspace.rs
```

- [ ] **Step 2: Add the gated cleanup call**

Right after the worktree directory is removed (so the cleanup happens even if claude has already been killed):

```rust
if crate::mcp::enabled(&store) {
    if let Err(e) = crate::mcp::remove_worktree_entry(&worktree_path) {
        tracing::warn!(error = %e, "failed to remove worktree entry from ~/.claude.json");
    }
}
```

Use the `worktree_path` and `store` already in scope.

- [ ] **Step 3: Add a test**

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn archive_removes_mcp_entry_from_claude_json() {
    use tempfile::TempDir;
    let home = TempDir::new().unwrap();
    let original_home = std::env::var_os("HOME");
    unsafe { std::env::set_var("HOME", home.path()); }

    // Set up source repo + a fake worktree entry in ~/.claude.json.
    // ... follow existing archive test pattern for repo + workspace setup ...

    // Pre-populate ~/.claude.json with a worktree entry.
    // Run archive(...) for the workspace.
    // Assert projects[worktree_path] is gone.

    if let Some(h) = original_home {
        unsafe { std::env::set_var("HOME", h); }
    }
}
```

Use the closest existing archive test as the scaffolding template.

- [ ] **Step 4: Run all workspace tests**

```
cargo test --lib workspace::tests -- --test-threads=1 2>&1 | tail -10
```

Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src/workspace.rs
git commit -m "feat(workspace): remove MCP entry on workspace archive"
```

---

### Task 7: README — MCP server inheritance section

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Find the Settings section**

```
grep -n "^## \|^### " README.md | head -30
```

Look for an existing "Settings" or "Configuration" section. If it exists, add the new content as a subsection. If not, add a new top-level "MCP server inheritance" section near the end of the user-facing docs (before the development/contributing material).

- [ ] **Step 2: Add the section**

Use this body (adapt the heading depth to match the surrounding doc):

```markdown
### MCP server inheritance

Claude Code stores MCP server config in `~/.claude.json` under
`projects.<absolute_cwd_path>.mcpServers`. Because wsx launches claude
inside a worktree path (under `~/.local/state/wsx/worktrees/...`),
the source repo's MCP servers aren't visible by default — claude
looks up the worktree path, finds no entry, and runs without those
servers.

wsx mirrors the source repo's `mcpServers` into the worktree's
project entry every time a workspace session spawns. New servers
added to the source repo via `claude mcp add ...` show up in
workspaces on the next attach.

On `wsx workspace archive`, wsx removes the worktree's
`projects[<worktree_path>]` entry from `~/.claude.json` to keep it
tidy.

**Secrets**: MCP server configs often include API tokens and other
credentials. Mirroring copies them verbatim into the worktree entry.
This is the same file with the same permissions, but it does mean
the same secret is now keyed under two paths.

**Toggle**: this behavior is on by default. Disable it with:

    wsx settings set mcp_mirror false

With it disabled, wsx never reads or writes `~/.claude.json`. You
can still configure MCP servers per-workspace by running
`claude mcp add ...` while attached.
```

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs(readme): document MCP server inheritance + mcp_mirror toggle"
```

---

### Task 8: Final fmt + clippy + full test + manual smoke

**Files:** none (verification).

- [ ] **Step 1: Format + clippy**

```
cargo fmt && cargo clippy --all-targets -- -D warnings 2>&1 | tail -10
```

Expected: clean.

- [ ] **Step 2: Full test suite**

```
cargo test --lib -- --test-threads=1 2>&1 | tail -5
```

Expected: all pass. Baseline before this plan: 263. Plan adds ~18 tests; expect ~281.

- [ ] **Step 3: Manual smoke**

1. Confirm `~/.claude.json` has `projects[<some-repo>].mcpServers` defined.
2. Add that repo in wsx, create a new workspace.
3. Confirm `projects[<worktree_path>].mcpServers` appears in `~/.claude.json` after the workspace launches.
4. Attach and verify claude sees the MCP servers (e.g., `/mcp` slash command should list them).
5. Archive the workspace. Confirm the entry is removed from `~/.claude.json`.
6. Add a new MCP server to the source repo via `claude mcp add ...` while no workspace is open.
7. Open a workspace for that repo; confirm the new server shows up in the workspace's entry.
8. `wsx settings set mcp_mirror false`. Create another workspace. Confirm no entry is added to `~/.claude.json`. Archive it; confirm no read or write occurs (no entry to remove either).
9. `wsx settings set mcp_mirror true` (or `wsx settings unset mcp_mirror`). Behavior returns to default.

- [ ] **Step 4: Push to main** (functional fix, per project convention)

```
git push
```
