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
use std::collections::{HashMap, VecDeque};
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
    /// Tool_use ids the assistant has emitted but for which we haven't yet
    /// seen a matching tool_result. Used to detect permission prompts —
    /// a tool_use pending for ≥3s is almost certainly waiting on user
    /// approval. Map: id → (tool name, first-seen epoch ms).
    pub pending_tool_uses: HashMap<String, (String, i64)>,
}

impl Default for WorkspaceEvents {
    fn default() -> Self {
        Self {
            latest: None,
            log: VecDeque::with_capacity(MAX_LOG),
            file_path: None,
            byte_offset: 0,
            pending_tool_uses: HashMap::new(),
        }
    }
}

/// Output of a single `tail_session` call.
///
/// Carries both display-bound events and tool-tracking signals that the caller
/// uses to maintain a per-workspace pending-tool map.
#[derive(Debug, Clone, Default)]
pub struct TailUpdate {
    pub new_offset: u64,
    pub events: Vec<EventSnapshot>,
    /// (tool_use_id, tool_name, first-seen epoch ms) for each tool_use block
    /// observed in this batch.
    pub tool_use_starts: Vec<(String, String, i64)>,
    /// tool_use_ids resolved by a `tool_result` block in this batch.
    pub tool_use_resolves: Vec<String>,
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
/// the parsed events and tool-tracking signals.
pub fn tail_session(path: &Path, offset: u64) -> Result<TailUpdate> {
    use std::io::{BufRead, BufReader, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let file_size = file.metadata()?.len();
    // Handle truncation/replacement: if the file is now smaller than our
    // offset, reset to 0 — likely a new session in the same path (rare).
    let start = if offset > file_size { 0 } else { offset };
    file.seek(SeekFrom::Start(start))?;
    let mut reader = BufReader::new(file);
    let mut update = TailUpdate::default();
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
        let parsed = parse_jsonl_line(buf.trim_end());
        if let Some(snap) = parsed.event {
            update.events.push(snap);
        }
        update.tool_use_starts.extend(parsed.tool_use_starts);
        update.tool_use_resolves.extend(parsed.tool_use_resolves);
    }
    update.new_offset = consumed;
    Ok(update)
}

/// Result of parsing a single JSONL line: at most one display event, plus
/// any tool-tracking signals derived from its content blocks.
#[derive(Debug, Default)]
pub struct ParsedLine {
    pub event: Option<EventSnapshot>,
    pub tool_use_starts: Vec<(String, String, i64)>,
    pub tool_use_resolves: Vec<String>,
}

/// Parse a single JSONL line into a [`ParsedLine`]. Malformed lines and
/// uninteresting top-level types yield an empty result.
///
/// User `tool_result` content blocks DO NOT produce an `EventSnapshot` (they
/// stay skipped from the display log) but DO populate `tool_use_resolves`.
pub fn parse_jsonl_line(line: &str) -> ParsedLine {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return ParsedLine::default();
    };
    let Some(kind) = v.get("type").and_then(|t| t.as_str()) else {
        return ParsedLine::default();
    };
    let timestamp_ms = parse_timestamp(v.get("timestamp"));
    match kind {
        "user" => parse_user(&v, timestamp_ms),
        "assistant" => parse_assistant(&v, timestamp_ms),
        _ => ParsedLine::default(),
    }
}

fn parse_user(v: &serde_json::Value, timestamp_ms: i64) -> ParsedLine {
    let mut out = ParsedLine::default();
    let Some(content) = v.get("message").and_then(|m| m.get("content")) else {
        return out;
    };
    // User content is either:
    //   (a) a plain string (the user's prompt) — emit a display event;
    //   (b) an array containing tool_result blocks — emit resolves but no
    //       display event (tool outputs aren't user prompts).
    if let Some(text) = content.as_str() {
        if text.trim().is_empty() {
            return out;
        }
        let display = truncate_display(&format!("user: {}", collapse_ws(text)), MAX_DISPLAY_CHARS);
        out.event = Some(EventSnapshot {
            kind: EventKind::UserMessage,
            display,
            timestamp_ms,
        });
        return out;
    }
    if let Some(blocks) = content.as_array() {
        for block in blocks {
            let Some(bt) = block.get("type").and_then(|t| t.as_str()) else {
                continue;
            };
            if bt == "tool_result"
                && let Some(id) = block.get("tool_use_id").and_then(|i| i.as_str())
            {
                out.tool_use_resolves.push(id.to_string());
            }
        }
    }
    out
}

fn parse_assistant(v: &serde_json::Value, timestamp_ms: i64) -> ParsedLine {
    let mut out = ParsedLine::default();
    let Some(blocks) = v
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
    else {
        return out;
    };
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
                // Track every tool_use we see — multiple in one message is rare
                // but possible. The id is required for matching tool_results.
                if let Some(id) = block.get("id").and_then(|i| i.as_str()) {
                    out.tool_use_starts
                        .push((id.to_string(), name.to_string(), timestamp_ms));
                }
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
        out.event = Some(EventSnapshot {
            kind: EventKind::AssistantToolUse,
            display: truncate_display(&body, MAX_DISPLAY_CHARS),
            timestamp_ms,
        });
        return out;
    }
    if let Some(t) = last_text {
        let trimmed = t.trim();
        if trimmed.is_empty() {
            return out;
        }
        out.event = Some(EventSnapshot {
            kind: EventKind::AssistantText,
            display: truncate_display(&collapse_ws(trimmed), MAX_DISPLAY_CHARS),
            timestamp_ms,
        });
    }
    out
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
        let ev = parse_jsonl_line(line).event.expect("should parse");
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
        let ev = parse_jsonl_line(line).event.expect("should parse");
        assert_eq!(ev.kind, EventKind::AssistantText);
        assert!(ev.display.contains("I'll rename"), "{}", ev.display);
    }

    #[test]
    fn parses_assistant_bash_tool_use() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test --workspace","description":"run all tests"}}]},"timestamp":"2026-05-14T17:32:14.000Z"}"#;
        let ev = parse_jsonl_line(line).event.expect("should parse");
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
        let ev = parse_jsonl_line(line).event.expect("should parse");
        assert_eq!(ev.kind, EventKind::AssistantToolUse);
        assert_eq!(ev.display, "using Read");
    }

    #[test]
    fn tool_use_wins_over_text_in_same_message() {
        // When an assistant message has both a thinking block, a text block,
        // and a tool_use block, we surface the tool_use (most concrete).
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"running the tests"},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test"}}]},"timestamp":"2026-05-14T17:32:14.000Z"}"#;
        let ev = parse_jsonl_line(line).event.expect("should parse");
        assert_eq!(ev.kind, EventKind::AssistantToolUse);
        assert!(ev.display.contains("cargo test"));
    }

    #[test]
    fn skips_tool_result_user_messages() {
        // A "user" line whose content is an array (tool results, not a real
        // user prompt) should be skipped from the display log entirely. It
        // STILL emits a resolve so the caller can clear the pending entry.
        let line = r#"{"type":"user","message":{"role":"user","content":[{"tool_use_id":"t1","type":"tool_result","content":"ok","is_error":false}]},"timestamp":"2026-05-14T17:32:14.000Z"}"#;
        let parsed = parse_jsonl_line(line);
        assert!(parsed.event.is_none());
        assert_eq!(parsed.tool_use_resolves, vec!["t1".to_string()]);
    }

    #[test]
    fn skips_unknown_line_types() {
        let line = r#"{"type":"attachment","content":"x","timestamp":"2026-05-14T17:32:14.000Z"}"#;
        assert!(parse_jsonl_line(line).event.is_none());
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(parse_jsonl_line("{ not json").event.is_none());
        assert!(parse_jsonl_line("").event.is_none());
    }

    #[test]
    fn truncates_long_messages() {
        let long = "x".repeat(200);
        let line = format!(
            r#"{{"type":"user","message":{{"role":"user","content":"{long}"}},"timestamp":"2026-05-14T17:32:02.744Z"}}"#
        );
        let ev = parse_jsonl_line(&line).event.expect("should parse");
        assert!(ev.display.chars().count() <= MAX_DISPLAY_CHARS);
        assert!(ev.display.ends_with('\u{2026}'));
    }

    #[test]
    fn collapses_whitespace_in_display() {
        let line = r#"{"type":"user","message":{"role":"user","content":"hello\n\n  world\t!"},"timestamp":"2026-05-14T17:32:02.744Z"}"#;
        let ev = parse_jsonl_line(line).event.expect("should parse");
        assert_eq!(ev.display, "user: hello world !");
    }

    #[test]
    fn parser_emits_tool_use_start_on_assistant_tool_use() {
        let line = r#"{"type":"assistant","timestamp":"2026-05-14T20:00:00.000Z","message":{"content":[{"type":"tool_use","id":"toolu_abc","name":"Bash","input":{"command":"ls"}}]}}"#;
        let parsed = parse_jsonl_line(line);
        // Existing behavior: an AssistantToolUse display event.
        let ev = parsed.event.as_ref().expect("display event");
        assert_eq!(ev.kind, EventKind::AssistantToolUse);
        // New: tracking emission for the tool_use block.
        assert_eq!(parsed.tool_use_starts.len(), 1);
        assert_eq!(parsed.tool_use_starts[0].0, "toolu_abc");
        assert_eq!(parsed.tool_use_starts[0].1, "Bash");
        assert!(parsed.tool_use_resolves.is_empty());
    }

    #[test]
    fn parser_emits_tool_use_resolve_on_user_tool_result() {
        let line = r#"{"type":"user","timestamp":"2026-05-14T20:00:05.000Z","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_abc","content":"ok"}]}}"#;
        let parsed = parse_jsonl_line(line);
        // User tool_result rows stay skipped from the display log.
        assert!(parsed.event.is_none());
        assert_eq!(parsed.tool_use_resolves, vec!["toolu_abc".to_string()]);
        assert!(parsed.tool_use_starts.is_empty());
    }

    #[test]
    fn parser_handles_assistant_text_and_tool_use_in_same_message() {
        // For mixed messages we still surface the tool_use as the display
        // event AND emit a tool_use_start for it.
        let line = r#"{"type":"assistant","timestamp":"2026-05-14T20:00:00.000Z","message":{"content":[{"type":"text","text":"I'll run this"},{"type":"tool_use","id":"toolu_xyz","name":"Bash","input":{"command":"ls"}}]}}"#;
        let parsed = parse_jsonl_line(line);
        let ev = parsed.event.as_ref().expect("display event");
        assert_eq!(ev.kind, EventKind::AssistantToolUse);
        assert_eq!(parsed.tool_use_starts.len(), 1);
        assert_eq!(parsed.tool_use_starts[0].0, "toolu_xyz");
        assert_eq!(parsed.tool_use_starts[0].1, "Bash");
    }

    #[test]
    fn tail_session_emits_pairs_across_lines() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("session.jsonl");
        let line_a = r#"{"type":"assistant","timestamp":"2026-05-14T20:00:00.000Z","message":{"content":[{"type":"tool_use","id":"a1","name":"Bash","input":{"command":"x"}}]}}"#;
        let line_b = r#"{"type":"user","timestamp":"2026-05-14T20:00:01.000Z","message":{"content":[{"type":"tool_result","tool_use_id":"a1","content":"ok"}]}}"#;
        std::fs::write(&path, format!("{line_a}\n{line_b}\n")).unwrap();
        let update = tail_session(&path, 0).unwrap();
        assert_eq!(update.tool_use_starts.len(), 1);
        assert_eq!(update.tool_use_starts[0].0, "a1");
        assert_eq!(update.tool_use_starts[0].1, "Bash");
        assert_eq!(update.tool_use_resolves, vec!["a1".to_string()]);
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

        let update = tail_session(&path, 0).unwrap();
        assert_eq!(update.events.len(), 2);
        assert_eq!(update.events[0].kind, EventKind::UserMessage);
        assert_eq!(update.events[1].kind, EventKind::AssistantText);

        // Re-tailing from the same offset returns nothing.
        let update2 = tail_session(&path, update.new_offset).unwrap();
        assert!(update2.events.is_empty());
        assert_eq!(update2.new_offset, update.new_offset);

        // Append a new complete line and verify only it comes back.
        let line3 = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash","input":{"command":"ls"}}]},"timestamp":"2026-05-14T17:32:04.000Z"}"#;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        use std::io::Write;
        writeln!(f, "{line3}").unwrap();
        let update3 = tail_session(&path, update2.new_offset).unwrap();
        assert_eq!(update3.events.len(), 1);
        assert_eq!(update3.events[0].kind, EventKind::AssistantToolUse);
    }

    #[test]
    fn tail_session_ignores_unterminated_trailing_line() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("s.jsonl");
        let line1 = r#"{"type":"user","message":{"role":"user","content":"hi"},"timestamp":"2026-05-14T17:32:02.744Z"}"#;
        // Note: no trailing newline on the second line.
        let partial = r#"{"type":"user","message":{"role":"user","content":"oops"}"#;
        std::fs::write(&path, format!("{line1}\n{partial}")).unwrap();

        let update = tail_session(&path, 0).unwrap();
        // Only the first, terminated line should be committed.
        assert_eq!(update.events.len(), 1);
        // Offset advanced only past the completed line.
        assert_eq!(update.new_offset as usize, line1.len() + 1);

        // Now complete the second line and verify it's picked up.
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        use std::io::Write;
        writeln!(f, r#","timestamp":"2026-05-14T17:32:03.000Z"}}"#).unwrap();
        let update2 = tail_session(&path, update.new_offset).unwrap();
        assert_eq!(update2.events.len(), 1);
    }

    #[test]
    fn tail_session_resets_when_offset_exceeds_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("s.jsonl");
        let line = r#"{"type":"user","message":{"role":"user","content":"hi"},"timestamp":"2026-05-14T17:32:02.744Z"}"#;
        std::fs::write(&path, format!("{line}\n")).unwrap();
        // Offset way past EOF — should reset to 0 and re-read.
        let update = tail_session(&path, 9_999_999).unwrap();
        assert_eq!(update.events.len(), 1);
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
