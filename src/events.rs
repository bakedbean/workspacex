//! Tail Claude Code session JSONL files for activity events.
//!
//! Claude Code writes one JSONL file per session at
//! `~/.claude/projects/<encoded-cwd>/<uuid>.jsonl`, where the cwd encoding
//! replaces `/` and `.` with `-` (same encoding used by
//! [`crate::pty::session::has_prior_session`]).
//!
//! ## JSONL schema (as of Claude Code v2.x)
//!
//! Each line is one JSON object. Lines we care about look roughly like:
//!
//! ```jsonc
//! // User text message:
//! {
//!   "type": "user",
//!   "message": { "role": "user", "content": "<text>" },
//!   "uuid": "...", "timestamp": "2026-05-14T17:32:02.744Z",
//!   "sessionId": "...", "cwd": "...", "gitBranch": "...", ...
//! }
//!
//! // Assistant text message (content is an array of content blocks):
//! {
//!   "type": "assistant",
//!   "message": {
//!     "role": "assistant",
//!     "content": [
//!       { "type": "thinking", "thinking": "...", "signature": "..." },
//!       { "type": "text", "text": "<text>" }
//!     ], ...
//!   },
//!   "uuid": "...", "timestamp": "2026-05-14T...", ...
//! }
//!
//! // Assistant tool use (also in content array):
//! {
//!   "type": "assistant",
//!   "message": {
//!     "content": [
//!       { "type": "tool_use", "id": "...", "name": "Bash",
//!         "input": { "command": "git status", "description": "..." } }
//!     ], ...
//!   }, ...
//! }
//!
//! // Tool result (back as "user" with structured content array — skipped):
//! { "type": "user", "message": { "role": "user", "content": [
//!     { "tool_use_id": "...", "type": "tool_result", "content": "...", "is_error": false }
//!   ] }, ... }
//! ```
//!
//! Other top-level `type` values seen: `attachment`, `last-prompt`,
//! `permission-mode`, `ai-title`, `file-history-snapshot`. We skip those.
//!
//! Timestamps are ISO 8601 with millisecond precision and a trailing `Z`.
//! We parse them ourselves to avoid pulling in chrono.

use crate::error::Result;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

const MAX_LOG: usize = 50;
const MAX_DISPLAY_CHARS: usize = 70;

#[derive(Debug, Clone)]
pub struct WorkspaceEvents {
    pub latest: Option<EventSnapshot>,
    /// Recent events, oldest first; bounded to MAX_LOG.
    pub log: VecDeque<EventSnapshot>,
    pub file_path: Option<PathBuf>,
    pub byte_offset: u64,
}

impl Default for WorkspaceEvents {
    fn default() -> Self {
        Self {
            latest: None,
            log: VecDeque::with_capacity(MAX_LOG),
            file_path: None,
            byte_offset: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EventSnapshot {
    pub kind: EventKind,
    /// Pre-formatted line ready to render. Already truncated.
    pub display: String,
    pub timestamp_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    UserMessage,
    AssistantText,
    AssistantToolUse,
    Other,
}

/// Encode an absolute path the way Claude Code does for `~/.claude/projects/`.
/// Mirrors [`crate::pty::session::has_prior_session`].
fn encode_cwd(path: &Path) -> String {
    path.to_string_lossy().replace(['/', '.'], "-")
}

/// Locate the active session file for a worktree.
///
/// Returns the latest-modified `.jsonl` in
/// `~/.claude/projects/<encoded-cwd>/`, if any.
pub fn locate_session_file(worktree: &Path) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let abs = std::fs::canonicalize(worktree).ok()?;
    let encoded = encode_cwd(&abs);
    let session_dir = home.join(".claude/projects").join(encoded);
    if !session_dir.is_dir() {
        return None;
    }
    let mut newest: Option<(PathBuf, std::time::SystemTime)> = None;
    for entry in std::fs::read_dir(&session_dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(mtime) = meta.modified() else { continue };
        match &newest {
            None => newest = Some((path, mtime)),
            Some((_, prev)) if mtime > *prev => newest = Some((path, mtime)),
            _ => {}
        }
    }
    newest.map(|(p, _)| p)
}

/// Read new lines from `path` starting at `offset` and parse them.
/// Returns the new committed offset (only fully terminated lines count) plus
/// the parsed events.
pub fn tail_session(path: &Path, offset: u64) -> Result<(u64, Vec<EventSnapshot>)> {
    use std::io::{BufRead, BufReader, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let file_size = file.metadata()?.len();
    // Handle truncation/replacement: if the file is now smaller than our
    // offset, reset to 0 — likely a new session in the same path (rare).
    let start = if offset > file_size { 0 } else { offset };
    file.seek(SeekFrom::Start(start))?;
    let mut reader = BufReader::new(file);
    let mut events = Vec::new();
    let mut buf = String::new();
    let mut consumed = start;
    loop {
        buf.clear();
        let n = reader.read_line(&mut buf)?;
        if n == 0 {
            break;
        }
        // Only fully-terminated lines (ending in '\n') are committed. A
        // partial trailing line may still be in flight; the next poll picks
        // it up after it completes.
        if !buf.ends_with('\n') {
            break;
        }
        consumed += n as u64;
        if let Some(snap) = parse_jsonl_line(buf.trim_end()) {
            events.push(snap);
        }
    }
    Ok((consumed, events))
}

/// Parse a single JSONL line. Returns `None` for malformed lines or line
/// types we don't render (attachments, snapshots, tool results, etc.).
pub fn parse_jsonl_line(line: &str) -> Option<EventSnapshot> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let kind = v.get("type")?.as_str()?;
    let timestamp_ms = parse_timestamp(v.get("timestamp"));
    match kind {
        "user" => parse_user(&v, timestamp_ms),
        "assistant" => parse_assistant(&v, timestamp_ms),
        _ => None,
    }
}

fn parse_user(v: &serde_json::Value, timestamp_ms: i64) -> Option<EventSnapshot> {
    let content = v.get("message")?.get("content")?;
    // User content is either a plain string (the user's prompt) or an array
    // containing tool_result blocks (which we skip — they're tool outputs,
    // not user messages).
    let text = content.as_str()?;
    if text.trim().is_empty() {
        return None;
    }
    let display = truncate_display(&format!("user: {}", collapse_ws(text)), MAX_DISPLAY_CHARS);
    Some(EventSnapshot {
        kind: EventKind::UserMessage,
        display,
        timestamp_ms,
    })
}

fn parse_assistant(v: &serde_json::Value, timestamp_ms: i64) -> Option<EventSnapshot> {
    let blocks = v.get("message")?.get("content")?.as_array()?;
    // Prefer tool_use over text — tool use is the most concrete signal of
    // "what's happening right now". Fall back to assistant text.
    let mut last_text: Option<&str> = None;
    let mut last_tool: Option<(&str, &serde_json::Value)> = None;
    for block in blocks {
        let Some(bt) = block.get("type").and_then(|t| t.as_str()) else {
            continue;
        };
        match bt {
            "text" => {
                if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                    last_text = Some(t);
                }
            }
            "tool_use" => {
                let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let input = block.get("input").unwrap_or(&serde_json::Value::Null);
                last_tool = Some((name, input));
            }
            _ => {}
        }
    }
    if let Some((name, input)) = last_tool {
        let body = if name == "Bash" {
            let cmd = input
                .get("command")
                .and_then(|c| c.as_str())
                .unwrap_or("(no command)");
            format!("ran `{}`", collapse_ws(cmd))
        } else if name.is_empty() {
            "using a tool".to_string()
        } else {
            format!("using {}", name)
        };
        return Some(EventSnapshot {
            kind: EventKind::AssistantToolUse,
            display: truncate_display(&body, MAX_DISPLAY_CHARS),
            timestamp_ms,
        });
    }
    if let Some(t) = last_text {
        let trimmed = t.trim();
        if trimmed.is_empty() {
            return None;
        }
        return Some(EventSnapshot {
            kind: EventKind::AssistantText,
            display: truncate_display(&collapse_ws(trimmed), MAX_DISPLAY_CHARS),
            timestamp_ms,
        });
    }
    None
}

/// Parse an ISO 8601 timestamp (e.g. `2026-05-14T17:32:02.744Z`) to epoch
/// milliseconds. Returns the current time on failure.
fn parse_timestamp(v: Option<&serde_json::Value>) -> i64 {
    let now_ms = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    };
    let Some(v) = v else { return now_ms() };
    // Could also be an epoch number — handle both.
    if let Some(n) = v.as_i64() {
        // Heuristic: > 10^12 means already ms; else seconds.
        return if n > 1_000_000_000_000 { n } else { n * 1000 };
    }
    let Some(s) = v.as_str() else { return now_ms() };
    parse_iso8601_ms(s).unwrap_or_else(now_ms)
}

/// Minimal ISO 8601 parser for the format Claude Code emits:
/// `YYYY-MM-DDTHH:MM:SS.fffZ` (always UTC, always millisecond precision).
fn parse_iso8601_ms(s: &str) -> Option<i64> {
    // Strip trailing Z; we treat the timestamp as UTC.
    let s = s.strip_suffix('Z').unwrap_or(s);
    // Split date and time at 'T'.
    let (date, time) = s.split_once('T')?;
    let mut date_parts = date.split('-');
    let y: i32 = date_parts.next()?.parse().ok()?;
    let mo: u32 = date_parts.next()?.parse().ok()?;
    let d: u32 = date_parts.next()?.parse().ok()?;

    let (hms, frac) = match time.split_once('.') {
        Some((hms, frac)) => (hms, frac),
        None => (time, "0"),
    };
    let mut tp = hms.split(':');
    let h: u32 = tp.next()?.parse().ok()?;
    let mi: u32 = tp.next()?.parse().ok()?;
    let se: u32 = tp.next()?.parse().ok()?;
    // Treat fractional seconds as milliseconds (truncate/pad to 3 digits).
    let mut frac_ms_str = String::new();
    for c in frac.chars().take(3) {
        frac_ms_str.push(c);
    }
    while frac_ms_str.len() < 3 {
        frac_ms_str.push('0');
    }
    let ms: i64 = frac_ms_str.parse().ok()?;

    let days = days_from_civil(y, mo, d);
    let secs_of_day = h as i64 * 3600 + mi as i64 * 60 + se as i64;
    Some(days * 86_400_000 + secs_of_day * 1000 + ms)
}

/// Howard Hinnant's `days_from_civil` algorithm — days since 1970-01-01 for a
/// proleptic Gregorian calendar date. Avoids pulling in chrono just for this.
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32; // [0, 399]
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era as i64 * 146_097 + doe as i64 - 719_468
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    out.trim().to_string()
}

fn truncate_display(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('\u{2026}'); // ellipsis
        out
    }
}

/// Append `event` into a [`WorkspaceEvents`] log, evicting the oldest entry
/// once the cap is hit. Updates `latest` to the appended event.
pub fn push_event(store: &mut WorkspaceEvents, event: EventSnapshot) {
    if store.log.len() >= MAX_LOG {
        store.log.pop_front();
    }
    store.latest = Some(event.clone());
    store.log.push_back(event);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_user_text_message() {
        let line = r#"{"type":"user","message":{"role":"user","content":"how do I add a new migration?"},"uuid":"u1","timestamp":"2026-05-14T17:32:02.744Z"}"#;
        let ev = parse_jsonl_line(line).expect("should parse");
        assert_eq!(ev.kind, EventKind::UserMessage);
        assert!(
            ev.display.starts_with("user: how do I add"),
            "{}",
            ev.display
        );
        // 2026-05-14T17:32:02.744Z is a real, finite epoch — sanity check.
        assert!(ev.timestamp_ms > 1_700_000_000_000);
    }

    #[test]
    fn parses_assistant_text_message() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"I'll rename the branch."}]},"timestamp":"2026-05-14T17:32:13.536Z"}"#;
        let ev = parse_jsonl_line(line).expect("should parse");
        assert_eq!(ev.kind, EventKind::AssistantText);
        assert!(ev.display.contains("I'll rename"), "{}", ev.display);
    }

    #[test]
    fn parses_assistant_bash_tool_use() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test --workspace","description":"run all tests"}}]},"timestamp":"2026-05-14T17:32:14.000Z"}"#;
        let ev = parse_jsonl_line(line).expect("should parse");
        assert_eq!(ev.kind, EventKind::AssistantToolUse);
        assert!(
            ev.display.contains("ran `cargo test --workspace`"),
            "{}",
            ev.display
        );
    }

    #[test]
    fn parses_assistant_other_tool_use() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/x"}}]},"timestamp":"2026-05-14T17:32:14.000Z"}"#;
        let ev = parse_jsonl_line(line).expect("should parse");
        assert_eq!(ev.kind, EventKind::AssistantToolUse);
        assert_eq!(ev.display, "using Read");
    }

    #[test]
    fn tool_use_wins_over_text_in_same_message() {
        // When an assistant message has both a thinking block, a text block,
        // and a tool_use block, we surface the tool_use (most concrete).
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"running the tests"},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test"}}]},"timestamp":"2026-05-14T17:32:14.000Z"}"#;
        let ev = parse_jsonl_line(line).expect("should parse");
        assert_eq!(ev.kind, EventKind::AssistantToolUse);
        assert!(ev.display.contains("cargo test"));
    }

    #[test]
    fn skips_tool_result_user_messages() {
        // A "user" line whose content is an array (tool results, not a real
        // user prompt) should be skipped — content.as_str() returns None.
        let line = r#"{"type":"user","message":{"role":"user","content":[{"tool_use_id":"t1","type":"tool_result","content":"ok","is_error":false}]},"timestamp":"2026-05-14T17:32:14.000Z"}"#;
        assert!(parse_jsonl_line(line).is_none());
    }

    #[test]
    fn skips_unknown_line_types() {
        let line = r#"{"type":"attachment","content":"x","timestamp":"2026-05-14T17:32:14.000Z"}"#;
        assert!(parse_jsonl_line(line).is_none());
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(parse_jsonl_line("{ not json").is_none());
        assert!(parse_jsonl_line("").is_none());
    }

    #[test]
    fn truncates_long_messages() {
        let long = "x".repeat(200);
        let line = format!(
            r#"{{"type":"user","message":{{"role":"user","content":"{long}"}},"timestamp":"2026-05-14T17:32:02.744Z"}}"#
        );
        let ev = parse_jsonl_line(&line).expect("should parse");
        assert!(ev.display.chars().count() <= MAX_DISPLAY_CHARS);
        assert!(ev.display.ends_with('\u{2026}'));
    }

    #[test]
    fn collapses_whitespace_in_display() {
        let line = r#"{"type":"user","message":{"role":"user","content":"hello\n\n  world\t!"},"timestamp":"2026-05-14T17:32:02.744Z"}"#;
        let ev = parse_jsonl_line(line).expect("should parse");
        assert_eq!(ev.display, "user: hello world !");
    }

    #[test]
    fn iso8601_parser_roundtrips_known_value() {
        // 2026-05-14T17:32:02.744Z. Compute the same way: days_from_civil
        // for 2026-05-14 plus the time components.
        let ms = parse_iso8601_ms("2026-05-14T17:32:02.744Z").unwrap();
        let days = days_from_civil(2026, 5, 14);
        let expected = days * 86_400_000 + (17 * 3600 + 32 * 60 + 2) * 1000 + 744;
        assert_eq!(ms, expected);
    }

    #[test]
    fn tail_session_reads_all_then_nothing_then_appended() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("s.jsonl");
        let line1 = r#"{"type":"user","message":{"role":"user","content":"hi"},"timestamp":"2026-05-14T17:32:02.744Z"}"#;
        let line2 = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hello"}]},"timestamp":"2026-05-14T17:32:03.000Z"}"#;
        std::fs::write(&path, format!("{line1}\n{line2}\n")).unwrap();

        let (off, evs) = tail_session(&path, 0).unwrap();
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].kind, EventKind::UserMessage);
        assert_eq!(evs[1].kind, EventKind::AssistantText);

        // Re-tailing from the same offset returns nothing.
        let (off2, evs2) = tail_session(&path, off).unwrap();
        assert!(evs2.is_empty());
        assert_eq!(off2, off);

        // Append a new complete line and verify only it comes back.
        let line3 = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash","input":{"command":"ls"}}]},"timestamp":"2026-05-14T17:32:04.000Z"}"#;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        use std::io::Write;
        writeln!(f, "{line3}").unwrap();
        let (_, evs3) = tail_session(&path, off2).unwrap();
        assert_eq!(evs3.len(), 1);
        assert_eq!(evs3[0].kind, EventKind::AssistantToolUse);
    }

    #[test]
    fn tail_session_ignores_unterminated_trailing_line() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("s.jsonl");
        let line1 = r#"{"type":"user","message":{"role":"user","content":"hi"},"timestamp":"2026-05-14T17:32:02.744Z"}"#;
        // Note: no trailing newline on the second line.
        let partial = r#"{"type":"user","message":{"role":"user","content":"oops"}"#;
        std::fs::write(&path, format!("{line1}\n{partial}")).unwrap();

        let (off, evs) = tail_session(&path, 0).unwrap();
        // Only the first, terminated line should be committed.
        assert_eq!(evs.len(), 1);
        // Offset advanced only past the completed line.
        assert_eq!(off as usize, line1.len() + 1);

        // Now complete the second line and verify it's picked up.
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        use std::io::Write;
        writeln!(f, r#","timestamp":"2026-05-14T17:32:03.000Z"}}"#).unwrap();
        let (_, evs2) = tail_session(&path, off).unwrap();
        assert_eq!(evs2.len(), 1);
    }

    #[test]
    fn tail_session_resets_when_offset_exceeds_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("s.jsonl");
        let line = r#"{"type":"user","message":{"role":"user","content":"hi"},"timestamp":"2026-05-14T17:32:02.744Z"}"#;
        std::fs::write(&path, format!("{line}\n")).unwrap();
        // Offset way past EOF — should reset to 0 and re-read.
        let (_off, evs) = tail_session(&path, 9_999_999).unwrap();
        assert_eq!(evs.len(), 1);
    }

    #[test]
    fn locate_session_file_finds_newest() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let abs = std::fs::canonicalize(work.path()).unwrap();
        let encoded = encode_cwd(&abs);
        let session_dir = home.path().join(".claude/projects").join(&encoded);
        std::fs::create_dir_all(&session_dir).unwrap();
        let older = session_dir.join("older.jsonl");
        let newer = session_dir.join("newer.jsonl");
        std::fs::write(&older, "{}").unwrap();
        // Sleep a hair to guarantee a different mtime even on coarse fs.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&newer, "{}").unwrap();

        let original = std::env::var_os("HOME");
        unsafe {
            std::env::set_var("HOME", home.path());
        }
        let result = locate_session_file(work.path());
        if let Some(h) = original {
            unsafe {
                std::env::set_var("HOME", h);
            }
        } else {
            unsafe {
                std::env::remove_var("HOME");
            }
        }
        assert_eq!(result, Some(newer));
    }

    #[test]
    fn locate_session_file_returns_none_when_dir_missing() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let original = std::env::var_os("HOME");
        unsafe {
            std::env::set_var("HOME", home.path());
        }
        let result = locate_session_file(work.path());
        if let Some(h) = original {
            unsafe {
                std::env::set_var("HOME", h);
            }
        } else {
            unsafe {
                std::env::remove_var("HOME");
            }
        }
        assert!(result.is_none());
    }

    #[test]
    fn push_event_bounds_log() {
        let mut ws = WorkspaceEvents::default();
        for i in 0..(MAX_LOG + 10) {
            push_event(
                &mut ws,
                EventSnapshot {
                    kind: EventKind::Other,
                    display: format!("e{i}"),
                    timestamp_ms: i as i64,
                },
            );
        }
        assert_eq!(ws.log.len(), MAX_LOG);
        assert_eq!(
            ws.latest.as_ref().unwrap().display,
            format!("e{}", MAX_LOG + 9)
        );
        // Oldest entry should have been evicted.
        assert_eq!(ws.log.front().unwrap().display, format!("e{}", 10));
    }
}
