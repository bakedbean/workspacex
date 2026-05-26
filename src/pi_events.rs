//! Tail pi session JSONL files for activity events.
//!
//! Pi stores sessions at `~/.pi/agent/sessions/--<encoded-cwd>--/<ts>_<uuid>.jsonl`,
//! where the cwd encoding strips the leading `/`, replaces remaining `/` with
//! `-`, and wraps with `--`. So `/home/eben` becomes `--home-eben--`, not
//! `---home-eben--`.
//!
//! ## JSONL schema (pi v3)
//!
//! Each line is one JSON object. Message lines have `type: "message"`:
//!
//! ```jsonc
//! // User text message:
//! {
//!   "type": "message", "id": "...", "parentId": "...", "timestamp": "...",
//!   "message": {
//!     "role": "user",
//!     "content": [{"type": "text", "text": "<text>"}],
//!     "timestamp": 1779475462031
//!   }
//! }
//!
//! // Assistant message:
//! {
//!   "type": "message", "id": "...", "parentId": "...", "timestamp": "...",
//!   "message": {
//!     "role": "assistant",
//!     "content": [
//!       {"type": "thinking", "thinking": "..."},
//!       {"type": "text", "text": "<text>"},
//!       {"type": "toolCall", "id": "call_...", "name": "bash",
//!        "arguments": {"command": "git status"}}
//!     ],
//!     "stopReason": "stop" | "toolUse" | "length" | "error" | "aborted",
//!     ...
//!   }
//! }
//!
//! // Tool result:
//! {
//!   "type": "message", "id": "...", "parentId": "...", "timestamp": "...",
//!   "message": {
//!     "role": "toolResult",
//!     "toolCallId": "call_...",
//!     "toolName": "bash",
//!     "content": [{"type": "text", "text": "..."}],
//!     "isError": false,
//!     ...
//!   }
//! }
//! ```
//!
//! Other top-level `type` values: `session`, `model_change`,
//! `thinking_level_change`, `compaction_summary`, `branch_summary`. We skip those.

use crate::error::Result;
use crate::events::{EventKind, EventSnapshot, StopReason, TailUpdate};
use std::path::{Path, PathBuf};

/// Encode an absolute path the way pi does for `~/.pi/agent/sessions/`.
/// Strips the leading `/`, replaces remaining `/` with `-`, and wraps with `--`.
pub fn encode_cwd(path: &Path) -> String {
    let inner = path
        .to_string_lossy()
        .trim_start_matches('/')
        .replace('/', "-");
    format!("--{}--", inner)
}

/// Locate the newest active session file for a worktree.
///
/// Returns the latest-modified `.jsonl` in
/// `~/.pi/agent/sessions/--<encoded-cwd>--/`, if any.
pub fn locate_session_file(worktree: &Path) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let abs = std::fs::canonicalize(worktree).ok()?;
    let encoded = encode_cwd(&abs);
    let session_dir = home.join(".pi/agent/sessions").join(encoded);
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

/// Read new lines from `path` starting at `offset` and parse them as pi
/// session JSONL. Returns the new committed offset and parsed events.
pub fn tail_session(path: &Path, offset: u64) -> Result<TailUpdate> {
    use std::io::{BufRead, BufReader, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let file_size = file.metadata()?.len();
    let reset_from_zero = offset > file_size;
    let start = if reset_from_zero { 0 } else { offset };
    file.seek(SeekFrom::Start(start))?;
    let mut reader = BufReader::new(file);
    let mut update = TailUpdate {
        reset_from_zero,
        ..TailUpdate::default()
    };
    let mut buf = String::new();
    let mut consumed = start;
    loop {
        buf.clear();
        let n = reader.read_line(&mut buf)?;
        if n == 0 {
            break;
        }
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
        if let Some(sr) = parsed.stop_reason {
            update.last_stop_reason = Some(sr);
            update.human_replied_after_last_stop = false;
            update.last_user_interrupted = Some(false);
        }
        if parsed.is_user_text {
            update.human_replied_after_last_stop = true;
            update.last_user_interrupted = Some(false);
        }
        if let Some(text) = parsed.last_assistant_text {
            // Track the longest text block seen in this batch alongside
            // the latest, for recap extraction. Narration is short;
            // real recaps are long.
            let len = text.chars().count();
            let replace_longest = update
                .longest_assistant_text_in_batch
                .as_ref()
                .map(|cur| cur.chars().count() < len)
                .unwrap_or(true);
            if replace_longest {
                update.longest_assistant_text_in_batch = Some(text.clone());
            }
            update.last_assistant_text = Some(text);
        }
    }
    update.new_offset = consumed;
    Ok(update)
}

/// Result of parsing a single pi JSONL line.
#[derive(Debug, Default)]
pub struct ParsedLine {
    pub event: Option<EventSnapshot>,
    pub tool_use_starts: Vec<(String, String, i64)>,
    pub tool_use_resolves: Vec<String>,
    pub stop_reason: Option<StopReason>,
    pub is_user_text: bool,
    pub last_assistant_text: Option<String>,
}

/// Parse a single pi session JSONL line into a [`ParsedLine`].
/// Skips non-message entries (session header, model changes, etc.).
pub fn parse_jsonl_line(line: &str) -> ParsedLine {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return ParsedLine::default();
    };
    // Only process message-type entries.
    let Some(kind) = v.get("type").and_then(|t| t.as_str()) else {
        return ParsedLine::default();
    };
    if kind != "message" {
        return ParsedLine::default();
    }
    let timestamp_ms = parse_pi_timestamp(v.get("timestamp"));
    let Some(msg) = v.get("message") else {
        return ParsedLine::default();
    };
    let Some(role) = msg.get("role").and_then(|r| r.as_str()) else {
        return ParsedLine::default();
    };
    match role {
        "user" => parse_pi_user(msg, timestamp_ms),
        "assistant" => parse_pi_assistant(msg, timestamp_ms),
        "toolResult" => parse_pi_tool_result(msg),
        _ => ParsedLine::default(),
    }
}

const MAX_DISPLAY_CHARS: usize = 512;

fn truncate_display(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('\u{2026}');
        out
    }
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

fn parse_pi_user(msg: &serde_json::Value, timestamp_ms: i64) -> ParsedLine {
    let mut out = ParsedLine::default();
    let Some(content) = msg.get("content") else {
        return out;
    };
    // Pi user content is always an array of content blocks.
    let blocks = match content.as_array() {
        Some(arr) => arr,
        None => {
            // String content (legacy?): treat as user text.
            if let Some(text) = content.as_str() {
                let t = text.trim();
                if t.is_empty() {
                    return out;
                }
                let display =
                    truncate_display(&format!("user: {}", collapse_ws(t)), MAX_DISPLAY_CHARS);
                out.event = Some(EventSnapshot {
                    kind: EventKind::UserMessage,
                    display,
                    timestamp_ms,
                });
                out.is_user_text = true;
            }
            return out;
        }
    };
    // Collect text from content blocks.
    let mut texts = Vec::new();
    for block in blocks {
        if let Some(bt) = block.get("type").and_then(|t| t.as_str()) {
            if bt == "text" {
                if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                    texts.push(t);
                }
            }
        }
    }
    let combined = texts.join(" ");
    let trimmed = combined.trim();
    if trimmed.is_empty() {
        return out;
    }
    out.event = Some(EventSnapshot {
        kind: EventKind::UserMessage,
        display: truncate_display(
            &format!("user: {}", collapse_ws(trimmed)),
            MAX_DISPLAY_CHARS,
        ),
        timestamp_ms,
    });
    out.is_user_text = true;
    out
}

fn parse_pi_assistant(msg: &serde_json::Value, timestamp_ms: i64) -> ParsedLine {
    let mut out = ParsedLine::default();

    // Parse stopReason.
    if let Some(sr) = msg.get("stopReason").and_then(|s| s.as_str()) {
        out.stop_reason = Some(map_pi_stop_reason(sr));
    }

    let Some(blocks) = msg.get("content").and_then(|c| c.as_array()) else {
        return out;
    };

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
            "toolCall" => {
                let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let args = block.get("arguments").unwrap_or(&serde_json::Value::Null);
                last_tool = Some((name, args));
                if let Some(id) = block.get("id").and_then(|i| i.as_str()) {
                    out.tool_use_starts
                        .push((id.to_string(), name.to_string(), timestamp_ms));
                }
            }
            _ => {}
        }
    }

    // Capture the final text block for the question-vs-complete classifier.
    if let Some(t) = last_text {
        out.last_assistant_text = Some(t.to_string());
    }

    // Display: prefer tool_use over text.
    if let Some((name, args)) = last_tool {
        let body = if name == "bash" {
            let cmd = args
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

fn parse_pi_tool_result(msg: &serde_json::Value) -> ParsedLine {
    let mut out = ParsedLine::default();
    // Tool results emit tool_use_resolves (so pending_tool_uses clears) but
    // no display event.
    if let Some(id) = msg.get("toolCallId").and_then(|i| i.as_str()) {
        out.tool_use_resolves.push(id.to_string());
    }
    out
}

/// Map pi's stopReason values to the shared StopReason enum used by
/// WorkspaceEvents classification.
fn map_pi_stop_reason(s: &str) -> StopReason {
    match s {
        "stop" => StopReason::EndTurn,
        "toolUse" => StopReason::ToolUse,
        "length" => StopReason::MaxTokens,
        "error" | "aborted" => StopReason::Other(s.to_string()),
        other => StopReason::Other(other.to_string()),
    }
}

/// Parse a pi timestamp (ISO 8601 like `2026-05-22T18:44:22.032Z`) to epoch
/// milliseconds. Falls back to current time on failure. Also handles
/// epoch-millis numbers (the inner message.timestamp field).
fn parse_pi_timestamp(v: Option<&serde_json::Value>) -> i64 {
    let now_ms = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    };
    let Some(v) = v else { return now_ms() };
    if let Some(n) = v.as_i64() {
        return if n > 1_000_000_000_000 { n } else { n * 1000 };
    }
    let Some(s) = v.as_str() else { return now_ms() };
    crate::events::parse_iso8601_ms(s).unwrap_or_else(now_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_cwd_wraps_with_double_dash() {
        let path = Path::new("/home/eben/work");
        let encoded = encode_cwd(path);
        // Matches pi's actual on-disk encoding (verify with
        // `ls ~/.pi/agent/sessions/`).
        assert_eq!(encoded, "--home-eben-work--");
    }

    #[test]
    fn encode_cwd_empty_path() {
        let path = Path::new("/");
        let encoded = encode_cwd(path);
        // "/" → strip leading '/' → "" → wrap with "--" → "----"
        assert_eq!(encoded, "----");
    }

    #[test]
    fn locate_session_file_finds_newest() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let abs = std::fs::canonicalize(work.path()).unwrap();
        let encoded = encode_cwd(&abs);
        let session_dir = home.path().join(".pi/agent/sessions").join(&encoded);
        std::fs::create_dir_all(&session_dir).unwrap();
        let older = session_dir.join("older.jsonl");
        let newer = session_dir.join("newer.jsonl");
        std::fs::write(&older, "{}").unwrap();
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
    fn parses_user_text_message() {
        let line = r#"{"type":"message","id":"u1","parentId":"p1","timestamp":"2026-05-22T18:44:22.032Z","message":{"role":"user","content":[{"type":"text","text":"how do I add a new migration?"}],"timestamp":1779475462031}}"#;
        let parsed = parse_jsonl_line(line);
        let ev = parsed.event.expect("should parse");
        assert_eq!(ev.kind, EventKind::UserMessage);
        assert!(parsed.is_user_text);
        assert!(ev.display.starts_with("user: how do I add"));
    }

    #[test]
    fn parses_user_text_with_multiple_blocks() {
        let line = r#"{"type":"message","id":"u2","parentId":"p2","timestamp":"2026-05-22T18:44:22.032Z","message":{"role":"user","content":[{"type":"text","text":"hello"},{"type":"text","text":"world"}],"timestamp":1779475462031}}"#;
        let parsed = parse_jsonl_line(line);
        let ev = parsed.event.expect("should parse");
        assert!(ev.display.contains("hello world"));
        assert!(parsed.is_user_text);
    }

    #[test]
    fn parses_assistant_text_message() {
        let line = r#"{"type":"message","id":"a1","parentId":"u1","timestamp":"2026-05-22T18:44:23.000Z","message":{"role":"assistant","content":[{"type":"text","text":"I'll rename the branch."}],"stopReason":"stop","api":"openai-completions","provider":"deepseek","model":"deepseek-v4-pro","usage":{"input":100,"output":50},"timestamp":1779475463000}}"#;
        let parsed = parse_jsonl_line(line);
        let ev = parsed.event.expect("should parse");
        assert_eq!(ev.kind, EventKind::AssistantText);
        assert!(ev.display.contains("I'll rename"));
        assert_eq!(parsed.stop_reason, Some(StopReason::EndTurn));
        assert_eq!(
            parsed.last_assistant_text.as_deref(),
            Some("I'll rename the branch.")
        );
    }

    #[test]
    fn parses_assistant_tool_call() {
        let line = r#"{"type":"message","id":"a2","parentId":"u2","timestamp":"2026-05-22T18:44:24.000Z","message":{"role":"assistant","content":[{"type":"toolCall","id":"call_001","name":"bash","arguments":{"command":"cargo test --workspace"}}],"stopReason":"toolUse","api":"openai-completions","provider":"deepseek","model":"deepseek-v4-pro","usage":{"input":100,"output":50},"timestamp":1779475464000}}"#;
        let parsed = parse_jsonl_line(line);
        let ev = parsed.event.expect("should parse");
        assert_eq!(ev.kind, EventKind::AssistantToolUse);
        assert!(ev.display.contains("ran `cargo test --workspace`"));
        assert_eq!(parsed.stop_reason, Some(StopReason::ToolUse));
        assert_eq!(parsed.tool_use_starts.len(), 1);
        assert_eq!(parsed.tool_use_starts[0].0, "call_001");
        assert_eq!(parsed.tool_use_starts[0].1, "bash");
    }

    #[test]
    fn parses_assistant_other_tool() {
        let line = r#"{"type":"message","id":"a3","parentId":"u3","timestamp":"2026-05-22T18:44:24.000Z","message":{"role":"assistant","content":[{"type":"toolCall","id":"call_002","name":"read","arguments":{"path":"/x"}}],"stopReason":"toolUse","api":"openai-completions","provider":"deepseek","model":"deepseek-v4-pro","usage":{"input":100,"output":50},"timestamp":1779475464000}}"#;
        let parsed = parse_jsonl_line(line);
        let ev = parsed.event.expect("should parse");
        assert_eq!(ev.kind, EventKind::AssistantToolUse);
        assert_eq!(ev.display, "using read");
    }

    #[test]
    fn parses_tool_result() {
        let line = r#"{"type":"message","id":"tr1","parentId":"a2","timestamp":"2026-05-22T18:44:25.000Z","message":{"role":"toolResult","toolCallId":"call_001","toolName":"bash","content":[{"type":"text","text":"ok"}],"details":{},"isError":false,"timestamp":1779475465000}}"#;
        let parsed = parse_jsonl_line(line);
        // Tool results do not produce display events.
        assert!(parsed.event.is_none());
        assert_eq!(parsed.tool_use_resolves, vec!["call_001".to_string()]);
    }

    #[test]
    fn skips_non_message_entries() {
        let session_line = r#"{"type":"session","version":3,"id":"s1","timestamp":"2026-05-22T18:44:10.720Z","cwd":"/home/eben"}"#;
        assert!(parse_jsonl_line(session_line).event.is_none());

        let model_line = r#"{"type":"model_change","id":"m1","parentId":null,"timestamp":"2026-05-22T18:44:10.732Z","provider":"deepseek","modelId":"deepseek-v4-pro"}"#;
        assert!(parse_jsonl_line(model_line).event.is_none());
    }

    #[test]
    fn assistant_without_stop_reason_yields_none() {
        let line = r#"{"type":"message","id":"a4","parentId":"u4","timestamp":"2026-05-22T18:44:23.000Z","message":{"role":"assistant","content":[{"type":"text","text":"thinking"}],"api":"openai-completions","provider":"deepseek","model":"deepseek-v4-pro","usage":{"input":100,"output":50},"timestamp":1779475463000}}"#;
        let parsed = parse_jsonl_line(line);
        assert_eq!(parsed.stop_reason, None);
    }

    #[test]
    fn maps_stop_reasons_correctly() {
        assert_eq!(map_pi_stop_reason("stop"), StopReason::EndTurn);
        assert_eq!(map_pi_stop_reason("toolUse"), StopReason::ToolUse);
        assert_eq!(map_pi_stop_reason("length"), StopReason::MaxTokens);
        matches!(map_pi_stop_reason("error"), StopReason::Other(_));
        matches!(map_pi_stop_reason("aborted"), StopReason::Other(_));
    }

    #[test]
    fn tail_session_reads_all_then_nothing_then_appended() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("s.jsonl");
        let line1 = r#"{"type":"message","id":"u1","parentId":"p1","timestamp":"2026-05-22T18:44:22.032Z","message":{"role":"user","content":[{"type":"text","text":"hi"}],"timestamp":1779475462031}}"#;
        let line2 = r#"{"type":"message","id":"a1","parentId":"u1","timestamp":"2026-05-22T18:44:23.000Z","message":{"role":"assistant","content":[{"type":"text","text":"hello"}],"stopReason":"stop","api":"openai-completions","provider":"deepseek","model":"deepseek-v4-pro","usage":{"input":100,"output":50},"timestamp":1779475463000}}"#;
        std::fs::write(&path, format!("{line1}\n{line2}\n")).unwrap();

        let update = tail_session(&path, 0).unwrap();
        assert_eq!(update.events.len(), 2);
        assert_eq!(update.events[0].kind, EventKind::UserMessage);
        assert_eq!(update.events[1].kind, EventKind::AssistantText);

        let update2 = tail_session(&path, update.new_offset).unwrap();
        assert!(update2.events.is_empty());
        assert_eq!(update2.new_offset, update.new_offset);

        let line3 = r#"{"type":"message","id":"a2","parentId":"u2","timestamp":"2026-05-22T18:44:24.000Z","message":{"role":"assistant","content":[{"type":"toolCall","id":"call_001","name":"bash","arguments":{"command":"ls"}}],"stopReason":"toolUse","api":"openai-completions","provider":"deepseek","model":"deepseek-v4-pro","usage":{"input":100,"output":50},"timestamp":1779475464000}}"#;
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
    fn tail_session_resets_when_offset_exceeds_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("s.jsonl");
        let line = r#"{"type":"message","id":"u1","parentId":"p1","timestamp":"2026-05-22T18:44:22.032Z","message":{"role":"user","content":[{"type":"text","text":"hi"}],"timestamp":1779475462031}}"#;
        std::fs::write(&path, format!("{line}\n")).unwrap();
        let update = tail_session(&path, 9_999_999).unwrap();
        assert_eq!(update.events.len(), 1);
        assert!(update.reset_from_zero);
    }

    #[test]
    fn tail_session_ignores_unterminated_trailing_line() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("s.jsonl");
        let line1 = r#"{"type":"message","id":"u1","parentId":"p1","timestamp":"2026-05-22T18:44:22.032Z","message":{"role":"user","content":[{"type":"text","text":"hi"}],"timestamp":1779475462031}}"#;
        let partial = r#"{"type":"message","id":"u2","parentId":"u1","timestamp":"2026-05-22T18:44:23.000Z","message":{"role":"user","content":[{"type":"text","text":"oops"}"#;
        std::fs::write(&path, format!("{line1}\n{partial}")).unwrap();

        let update = tail_session(&path, 0).unwrap();
        assert_eq!(update.events.len(), 1);
        assert_eq!(update.new_offset as usize, line1.len() + 1);
    }

    #[test]
    fn tool_call_and_result_across_lines() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("s.jsonl");
        let line_a = r#"{"type":"message","id":"a1","parentId":"u1","timestamp":"2026-05-22T18:44:24.000Z","message":{"role":"assistant","content":[{"type":"toolCall","id":"call_x","name":"bash","arguments":{"command":"x"}}],"stopReason":"toolUse","api":"openai-completions","provider":"deepseek","model":"deepseek-v4-pro","usage":{"input":100,"output":50},"timestamp":1779475464000}}"#;
        let line_b = r#"{"type":"message","id":"tr1","parentId":"a1","timestamp":"2026-05-22T18:44:25.000Z","message":{"role":"toolResult","toolCallId":"call_x","toolName":"bash","content":[{"type":"text","text":"ok"}],"details":{},"isError":false,"timestamp":1779475465000}}"#;
        std::fs::write(&path, format!("{line_a}\n{line_b}\n")).unwrap();
        let update = tail_session(&path, 0).unwrap();
        assert_eq!(update.tool_use_starts.len(), 1);
        assert_eq!(update.tool_use_starts[0].0, "call_x");
        assert_eq!(update.tool_use_resolves, vec!["call_x".to_string()]);
    }

    #[test]
    fn last_text_ends_with_question_detection_works() {
        // Verify the pi assistant parser forwards text for the question
        // classifier. The actual classification logic lives in WorkspaceEvents.
        let line = r#"{"type":"message","id":"a1","parentId":"u1","timestamp":"2026-05-22T18:44:23.000Z","message":{"role":"assistant","content":[{"type":"text","text":"Should I proceed?"}],"stopReason":"stop","api":"openai-completions","provider":"deepseek","model":"deepseek-v4-pro","usage":{"input":100,"output":50},"timestamp":1779475463000}}"#;
        let parsed = parse_jsonl_line(line);
        assert_eq!(
            parsed.last_assistant_text.as_deref(),
            Some("Should I proceed?")
        );
    }
}
