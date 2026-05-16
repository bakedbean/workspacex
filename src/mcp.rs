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
    let s = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::Io(e)),
    };
    if s.trim().is_empty() {
        return Ok(None);
    }
    let v: Value = serde_json::from_str(&s)
        .map_err(|e| Error::Pty(format!("parse ~/.claude.json: {e}")))?;
    Ok(Some(v))
}

/// Mirror `projects[repo_path].mcpServers` → `projects[worktree_path].mcpServers`
/// in `~/.claude.json`. No-op when the file or the source entry is absent.
/// Errors are returned but callers should treat them as best-effort.
pub fn mirror_mcp_servers(repo_path: &Path, worktree_path: &Path) -> Result<()> {
    let Some(p) = claude_json_path() else {
        return Ok(());
    };
    mirror_into(&p, repo_path, worktree_path)
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
        return Ok(());
    }
    write_claude_json_atomic(claude_json, &root)
}

/// Pure form of `mirror_mcp_servers` that takes the claude.json path
/// directly, for testability.
fn mirror_into(claude_json: &Path, repo: &Path, worktree: &Path) -> Result<()> {
    let Some(mut root) = read_claude_json(claude_json)? else {
        return Ok(());
    };
    let repo_key = repo.to_string_lossy().into_owned();
    let worktree_key = worktree.to_string_lossy().into_owned();
    let Some(servers) = root
        .get("projects")
        .and_then(|p| p.get(&repo_key))
        .and_then(|r| r.get("mcpServers"))
        .cloned()
    else {
        return Ok(());
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
            .open(&tmp)?;
        f.write_all(serialized.as_bytes())?;
        f.sync_all()?;
    }
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(Error::Io(e));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(!p.exists());
    }

    #[test]
    fn mirror_into_no_source_entry_is_noop() {
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path().join(".claude.json");
        let original = serde_json::json!({
            "projects": {"/some/other": {"mcpServers": {"x": {}}}}
        });
        write_json(&p, &original);
        mirror_into(&p, Path::new("/r"), Path::new("/wt")).unwrap();
        assert_eq!(read_json(&p), original);
    }

    #[test]
    fn mirror_into_no_source_mcp_is_noop() {
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path().join(".claude.json");
        let original = serde_json::json!({
            "projects": {"/r": {"lastSessionId": "abc"}}
        });
        write_json(&p, &original);
        mirror_into(&p, Path::new("/r"), Path::new("/wt")).unwrap();
        assert_eq!(read_json(&p), original);
    }

    #[test]
    fn mirror_into_happy_path_creates_worktree_entry() {
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path().join(".claude.json");
        write_json(
            &p,
            &serde_json::json!({
                "projects": {"/r": {"mcpServers": {"datadog": {"type": "http"}}}}
            }),
        );
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
        write_json(
            &p,
            &serde_json::json!({
                "projects": {
                    "/r": {"mcpServers": {"datadog": {"type": "http"}}},
                    "/wt": {"lastSessionId": "keep-me", "mcpServers": {"old": {}}}
                }
            }),
        );
        mirror_into(&p, Path::new("/r"), Path::new("/wt")).unwrap();
        let after = read_json(&p);
        assert_eq!(
            after["projects"]["/wt"]["lastSessionId"],
            serde_json::json!("keep-me")
        );
        assert_eq!(
            after["projects"]["/wt"]["mcpServers"],
            serde_json::json!({"datadog": {"type": "http"}})
        );
    }

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
        write_json(
            &p,
            &serde_json::json!({
                "projects": {
                    "/r": {"mcpServers": {}},
                    "/wt": {"mcpServers": {"x": {}}, "lastSessionId": "abc"}
                }
            }),
        );
        remove_into(&p, Path::new("/wt")).unwrap();
        let after = read_json(&p);
        assert!(after["projects"].get("/wt").is_none());
        assert!(after["projects"]["/r"].is_object());
    }

    #[test]
    fn write_claude_json_atomic_roundtrip() {
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path().join(".claude.json");
        let v = serde_json::json!({"hello": "world"});
        write_claude_json_atomic(&p, &v).unwrap();
        let back = read_claude_json(&p).unwrap().unwrap();
        assert_eq!(back, v);
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("wsx-tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "expected no temp files, got {leftovers:?}"
        );
    }
}
