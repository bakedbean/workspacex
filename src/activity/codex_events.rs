//! Tail Codex CLI session events from `~/.codex/sessions/**/rollout-*.jsonl`.
//!
//! Codex rollout files are date-partitioned (`YYYY/MM/DD/`) and store the
//! originating directory INSIDE the file (first line is `session_meta` with a
//! `cwd` field), so locating "this worktree's session" matches by content,
//! not by directory path. Real implementations land in later tasks.

use crate::activity::events::TailUpdate;
use crate::error::Result;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Cap how many rollout files we content-scan per locate, newest-first, so a
/// long session history can't make the 2s dashboard poll pathological.
const SCAN_CAP: usize = 500;

/// Locate the newest Codex rollout file whose recorded `cwd` matches `worktree`.
pub fn locate_session_file(worktree: &Path) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let abs = std::fs::canonicalize(worktree).ok()?;
    let root = home.join(".codex/sessions");
    if !root.is_dir() {
        return None;
    }
    let mut candidates: Vec<(PathBuf, SystemTime)> = Vec::new();
    collect_rollouts(&root, &mut candidates);
    candidates.sort_by_key(|b| std::cmp::Reverse(b.1)); // newest first
    candidates
        .into_iter()
        .take(SCAN_CAP)
        .map(|(path, _)| path)
        .find(|path| rollout_cwd_matches(path, &abs))
}

/// Recursively collect `rollout-*.jsonl` files under `dir` with their mtimes.
/// The sessions tree is only three levels deep (YYYY/MM/DD), so plain
/// recursion is fine and avoids pulling in a directory-walk dependency.
fn collect_rollouts(dir: &Path, out: &mut Vec<(PathBuf, SystemTime)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            collect_rollouts(&path, out);
        } else if is_rollout_file(&path) {
            if let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) {
                out.push((path, mtime));
            }
        }
    }
}

fn is_rollout_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name.starts_with("rollout-") && name.ends_with(".jsonl")
}

/// Read only the first line of `path`, parse `session_meta.payload.cwd`, and
/// compare to `abs` (the canonical worktree). Matches on canonicalized cwd
/// when the path still exists, falling back to a raw path compare.
fn rollout_cwd_matches(path: &Path, abs: &Path) -> bool {
    use std::io::{BufRead, BufReader};
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut first = String::new();
    if BufReader::new(file).read_line(&mut first).is_err() {
        return false;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(first.trim_end()) else {
        return false;
    };
    if v.get("type").and_then(|t| t.as_str()) != Some("session_meta") {
        return false;
    }
    let Some(cwd) = v
        .get("payload")
        .and_then(|p| p.get("cwd"))
        .and_then(|c| c.as_str())
    else {
        return false;
    };
    let stored = Path::new(cwd);
    std::fs::canonicalize(stored).ok().as_deref() == Some(abs) || stored == abs
}

/// Tail Codex rollout JSONL from `offset`. STUB — real implementation in the
/// tail/parse task.
pub fn tail_session(_path: &Path, offset: u64) -> Result<TailUpdate> {
    Ok(TailUpdate {
        new_offset: offset,
        ..TailUpdate::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_rollout(dir: &Path, name: &str, cwd: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        let meta = format!(
            r#"{{"timestamp":"2026-06-02T18:51:58.969Z","type":"session_meta","payload":{{"id":"abc","cwd":"{cwd}","originator":"codex-tui"}}}}"#
        );
        std::fs::write(&path, format!("{meta}\n")).unwrap();
        path
    }

    #[test]
    fn locate_matches_embedded_cwd_and_prefers_newest() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let abs = std::fs::canonicalize(work.path()).unwrap();
        let day = home.path().join(".codex/sessions/2026/06/02");
        let _older = write_rollout(&day, "rollout-A.jsonl", &abs.to_string_lossy());
        std::thread::sleep(std::time::Duration::from_millis(20));
        let mine = write_rollout(&day, "rollout-B.jsonl", &abs.to_string_lossy());

        let original = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", home.path()); }
        let result = locate_session_file(work.path());
        match original {
            Some(h) => unsafe { std::env::set_var("HOME", h); },
            None => unsafe { std::env::remove_var("HOME"); },
        }
        assert_eq!(result, Some(mine));
    }

    #[test]
    fn locate_returns_none_when_no_cwd_matches() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let day = home.path().join(".codex/sessions/2026/06/02");
        write_rollout(&day, "rollout-A.jsonl", "/nowhere/relevant");

        let original = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", home.path()); }
        let result = locate_session_file(work.path());
        match original {
            Some(h) => unsafe { std::env::set_var("HOME", h); },
            None => unsafe { std::env::remove_var("HOME"); },
        }
        assert!(result.is_none());
    }

    #[test]
    fn locate_returns_none_when_sessions_dir_missing() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let original = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", home.path()); }
        let result = locate_session_file(work.path());
        match original {
            Some(h) => unsafe { std::env::set_var("HOME", h); },
            None => unsafe { std::env::remove_var("HOME"); },
        }
        assert!(result.is_none());
    }
}
