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
