//! Tail Codex CLI session events from `~/.codex/sessions/**/rollout-*.jsonl`.
//!
//! Codex rollout files are date-partitioned (`YYYY/MM/DD/`) and store the
//! originating directory INSIDE the file (first line is `session_meta` with a
//! `cwd` field), so locating "this worktree's session" matches by content,
//! not by directory path.

use crate::activity::events::{EventKind, EventSnapshot, StopReason, TailUpdate};
use crate::error::Result;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Cap how many rollout files we content-scan per locate, newest-first, so a
/// long session history can't make the 2s dashboard poll pathological.
const SCAN_CAP: usize = 500;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Result of parsing a single Codex rollout JSONL line.
#[derive(Debug, Default)]
pub struct ParsedLine {
    pub event: Option<EventSnapshot>,
    pub tool_use_starts: Vec<(String, String, i64)>,
    pub tool_use_resolves: Vec<String>,
    pub stop_reason: Option<StopReason>,
    pub is_user_text: bool,
    pub first_user_text: Option<String>,
    pub last_assistant_text: Option<String>,
    pub longest_text_in_message: Option<String>,
}

/// Parse one Codex rollout line. Codex emits two parallel streams; we map a
/// chosen subset to avoid double-counting (see the design doc mapping table):
///   event_msg/user_message   -> user turn
///   event_msg/agent_message  -> assistant narration
///   event_msg/task_complete  -> end_turn + recap text (no separate event)
///   response_item/function_call         -> tool start
///   response_item/function_call_output  -> tool resolve
/// Everything else (response_item/message, reasoning, token_count,
/// session_meta, turn_context, task_started) is ignored.
pub fn parse_jsonl_line(line: &str) -> ParsedLine {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return ParsedLine::default();
    };
    let ts = v
        .get("timestamp")
        .and_then(|t| t.as_str())
        .and_then(crate::activity::events::parse_iso8601_ms)
        .unwrap_or_else(now_ms);
    let Some(kind) = v.get("type").and_then(|t| t.as_str()) else {
        return ParsedLine::default();
    };
    let Some(payload) = v.get("payload") else {
        return ParsedLine::default();
    };
    let ptype = payload.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match (kind, ptype) {
        ("event_msg", "user_message") => parse_user_message(payload, ts),
        ("event_msg", "agent_message") => parse_agent_message(payload, ts),
        ("event_msg", "task_complete") => parse_task_complete(payload),
        ("response_item", "function_call") => parse_function_call(payload, ts),
        ("response_item", "function_call_output") => parse_function_call_output(payload),
        _ => ParsedLine::default(),
    }
}

fn parse_user_message(payload: &serde_json::Value, ts: i64) -> ParsedLine {
    let Some(msg) = payload.get("message").and_then(|m| m.as_str()) else {
        return ParsedLine::default();
    };
    let trimmed = msg.trim();
    if trimmed.is_empty() {
        return ParsedLine::default();
    }
    ParsedLine {
        event: Some(EventSnapshot {
            kind: EventKind::UserMessage,
            display: crate::activity::events::truncate_display(
                &format!("user: {}", crate::activity::events::collapse_ws(trimmed)),
                crate::activity::events::MAX_DISPLAY_CHARS,
            ),
            timestamp_ms: ts,
        }),
        is_user_text: true,
        first_user_text: Some(trimmed.to_string()),
        ..ParsedLine::default()
    }
}

fn parse_agent_message(payload: &serde_json::Value, ts: i64) -> ParsedLine {
    let Some(msg) = payload.get("message").and_then(|m| m.as_str()) else {
        return ParsedLine::default();
    };
    let trimmed = msg.trim();
    if trimmed.is_empty() {
        return ParsedLine::default();
    }
    ParsedLine {
        event: Some(EventSnapshot {
            kind: EventKind::AssistantText,
            display: crate::activity::events::truncate_display(
                &crate::activity::events::collapse_ws(trimmed),
                crate::activity::events::MAX_DISPLAY_CHARS,
            ),
            timestamp_ms: ts,
        }),
        last_assistant_text: Some(trimmed.to_string()),
        longest_text_in_message: Some(trimmed.to_string()),
        ..ParsedLine::default()
    }
}

fn parse_task_complete(payload: &serde_json::Value) -> ParsedLine {
    // Feed the recap text into the trackers but DO NOT push a display event:
    // last_agent_message duplicates the final agent_message we already emitted.
    let (last_assistant_text, longest_text_in_message) =
        if let Some(msg) = payload.get("last_agent_message").and_then(|m| m.as_str()) {
            let trimmed = msg.trim();
            if trimmed.is_empty() {
                (None, None)
            } else {
                (Some(trimmed.to_string()), Some(trimmed.to_string()))
            }
        } else {
            (None, None)
        };
    ParsedLine {
        stop_reason: Some(StopReason::EndTurn),
        last_assistant_text,
        longest_text_in_message,
        ..ParsedLine::default()
    }
}

fn parse_function_call(payload: &serde_json::Value, ts: i64) -> ParsedLine {
    let name = payload.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let tool_use_starts = if let Some(id) = payload.get("call_id").and_then(|i| i.as_str()) {
        vec![(id.to_string(), name.to_string(), ts)]
    } else {
        vec![]
    };
    // Codex `arguments` is a JSON-encoded STRING; exec_command carries a `cmd`.
    let display = if name == "exec_command" {
        let cmd = payload
            .get("arguments")
            .and_then(|a| a.as_str())
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .and_then(|args| args.get("cmd").and_then(|c| c.as_str()).map(str::to_string));
        match cmd {
            Some(c) => format!("ran `{}`", crate::activity::events::collapse_ws(&c)),
            None => "ran a command".to_string(),
        }
    } else if name.is_empty() {
        "using a tool".to_string()
    } else {
        format!("using {name}")
    };
    ParsedLine {
        event: Some(EventSnapshot {
            kind: EventKind::AssistantToolUse,
            display: crate::activity::events::truncate_display(
                &display,
                crate::activity::events::MAX_DISPLAY_CHARS,
            ),
            timestamp_ms: ts,
        }),
        tool_use_starts,
        ..ParsedLine::default()
    }
}

fn parse_function_call_output(payload: &serde_json::Value) -> ParsedLine {
    let tool_use_resolves = if let Some(id) = payload.get("call_id").and_then(|i| i.as_str()) {
        vec![id.to_string()]
    } else {
        vec![]
    };
    ParsedLine {
        tool_use_resolves,
        ..ParsedLine::default()
    }
}

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

/// Read new lines from `path` starting at `offset` and parse them as Codex
/// rollout JSONL. Returns the new committed offset and parsed events.
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
            break; // ignore an unterminated trailing line; reread next tick
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
        if update.first_user_text.is_none()
            && let Some(t) = parsed.first_user_text
        {
            update.first_user_text = Some(t);
        }
        if let Some(longest) = parsed.longest_text_in_message {
            let len = longest.chars().count();
            let beats = update
                .longest_assistant_text_in_batch
                .as_ref()
                .map(|cur| cur.chars().count() < len)
                .unwrap_or(true);
            if beats {
                update.longest_assistant_text_in_batch = Some(longest);
            }
        }
        if let Some(text) = parsed.last_assistant_text {
            update.last_assistant_text = Some(text);
        }
    }
    update.new_offset = consumed;
    Ok(update)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::EnvGuard;

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

        // EnvGuard serializes against sibling tests that mutate HOME (e.g.
        // hermes_events) via the process-wide ENV_LOCK; raw set_var would race.
        let mut env = EnvGuard::new();
        env.set("HOME", home.path());
        let result = locate_session_file(work.path());
        assert_eq!(result, Some(mine));
    }

    #[test]
    fn locate_returns_none_when_no_cwd_matches() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let day = home.path().join(".codex/sessions/2026/06/02");
        write_rollout(&day, "rollout-A.jsonl", "/nowhere/relevant");

        let mut env = EnvGuard::new();
        env.set("HOME", home.path());
        let result = locate_session_file(work.path());
        assert!(result.is_none());
    }

    #[test]
    fn locate_returns_none_when_sessions_dir_missing() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let mut env = EnvGuard::new();
        env.set("HOME", home.path());
        let result = locate_session_file(work.path());
        assert!(result.is_none());
    }

    #[test]
    fn parses_user_message_event() {
        let line = r#"{"timestamp":"2026-06-02T18:56:04.390Z","type":"event_msg","payload":{"type":"user_message","message":"fix the billing bug"}}"#;
        let p = parse_jsonl_line(line);
        let ev = p.event.expect("event");
        assert_eq!(ev.kind, EventKind::UserMessage);
        assert!(ev.display.contains("fix the billing bug"));
        assert!(p.is_user_text);
        assert_eq!(p.first_user_text.as_deref(), Some("fix the billing bug"));
    }

    #[test]
    fn parses_agent_message_event() {
        let line = r#"{"timestamp":"2026-06-02T18:56:09.622Z","type":"event_msg","payload":{"type":"agent_message","message":"I'll trace the billing path first."}}"#;
        let p = parse_jsonl_line(line);
        let ev = p.event.expect("event");
        assert_eq!(ev.kind, EventKind::AssistantText);
        assert!(ev.display.contains("trace the billing path"));
        assert_eq!(
            p.last_assistant_text.as_deref(),
            Some("I'll trace the billing path first.")
        );
    }

    #[test]
    fn task_complete_sets_end_turn_without_duplicate_event() {
        let line = r#"{"timestamp":"2026-06-02T18:57:52.806Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"t1","last_agent_message":"Done. No edits made."}}"#;
        let p = parse_jsonl_line(line);
        assert_eq!(p.stop_reason, Some(StopReason::EndTurn));
        assert!(p.event.is_none(), "no duplicate event for task_complete");
        assert_eq!(
            p.last_assistant_text.as_deref(),
            Some("Done. No edits made.")
        );
    }

    #[test]
    fn parses_function_call_as_tool_use() {
        let line = r#"{"timestamp":"2026-06-02T18:56:09.626Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"rg -n invoice .\",\"workdir\":\"/x\"}","call_id":"call_abc"}}"#;
        let p = parse_jsonl_line(line);
        let ev = p.event.expect("event");
        assert_eq!(ev.kind, EventKind::AssistantToolUse);
        assert!(
            ev.display.contains("ran `rg -n invoice .`"),
            "display: {}",
            ev.display
        );
        assert_eq!(p.tool_use_starts.len(), 1);
        assert_eq!(p.tool_use_starts[0].0, "call_abc");
        assert_eq!(p.tool_use_starts[0].1, "exec_command");
    }

    #[test]
    fn parses_non_exec_function_call_generically() {
        let line = r#"{"timestamp":"2026-06-02T18:56:09.626Z","type":"response_item","payload":{"type":"function_call","name":"apply_patch","arguments":"{}","call_id":"call_p"}}"#;
        let p = parse_jsonl_line(line);
        let ev = p.event.expect("event");
        assert_eq!(ev.display, "using apply_patch");
    }

    #[test]
    fn parses_function_call_output_as_resolve() {
        let line = r#"{"timestamp":"2026-06-02T18:56:09.820Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call_abc","output":"ok"}}"#;
        let p = parse_jsonl_line(line);
        assert!(p.event.is_none());
        assert_eq!(p.tool_use_resolves, vec!["call_abc".to_string()]);
    }

    #[test]
    fn ignores_duplicate_assistant_response_item_and_reasoning_and_context() {
        for line in [
            r#"{"timestamp":"2026-06-02T18:56:09.623Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"dup of agent_message"}]}}"#,
            r#"{"timestamp":"2026-06-02T18:56:11.230Z","type":"response_item","payload":{"type":"reasoning","summary":[],"content":null,"encrypted_content":"xxx"}}"#,
            r#"{"timestamp":"2026-06-02T18:56:04.386Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>...</environment_context>"}]}}"#,
            r#"{"timestamp":"2026-06-02T18:56:04.382Z","type":"event_msg","payload":{"type":"token_count"}}"#,
            r#"{"timestamp":"2026-06-02T18:51:58.969Z","type":"session_meta","payload":{"cwd":"/x"}}"#,
            r#"{"timestamp":"2026-06-02T18:56:04.382Z","type":"turn_context","payload":{}}"#,
        ] {
            let p = parse_jsonl_line(line);
            assert!(p.event.is_none(), "must ignore: {line}");
            assert!(p.tool_use_starts.is_empty(), "no tool starts: {line}");
            assert!(!p.is_user_text, "not user text: {line}");
        }
    }

    #[test]
    fn tail_session_reads_then_appended_then_advances_offset() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("rollout-x.jsonl");
        let l1 = r#"{"timestamp":"2026-06-02T18:56:04.390Z","type":"event_msg","payload":{"type":"user_message","message":"hi"}}"#;
        let l2 = r#"{"timestamp":"2026-06-02T18:56:09.626Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"ls\"}","call_id":"c1"}}"#;
        std::fs::write(&path, format!("{l1}\n{l2}\n")).unwrap();

        let u = tail_session(&path, 0).unwrap();
        assert_eq!(u.events.len(), 2);
        assert_eq!(u.events[0].kind, EventKind::UserMessage);
        assert_eq!(u.events[1].kind, EventKind::AssistantToolUse);
        assert_eq!(u.first_user_text.as_deref(), Some("hi"));
        assert_eq!(u.tool_use_starts.len(), 1);

        let u2 = tail_session(&path, u.new_offset).unwrap();
        assert!(u2.events.is_empty());
        assert_eq!(u2.new_offset, u.new_offset);

        let l3 = r#"{"timestamp":"2026-06-02T18:56:09.820Z","type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"ok"}}"#;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        use std::io::Write;
        writeln!(f, "{l3}").unwrap();
        let u3 = tail_session(&path, u2.new_offset).unwrap();
        assert_eq!(u3.tool_use_resolves, vec!["c1".to_string()]);
    }

    #[test]
    fn tail_session_resets_when_offset_exceeds_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("rollout-x.jsonl");
        let l1 = r#"{"timestamp":"2026-06-02T18:56:04.390Z","type":"event_msg","payload":{"type":"user_message","message":"hi"}}"#;
        std::fs::write(&path, format!("{l1}\n")).unwrap();
        let u = tail_session(&path, 9_999_999).unwrap();
        assert_eq!(u.events.len(), 1);
        assert!(u.reset_from_zero);
    }

    #[test]
    fn tail_session_ignores_unterminated_trailing_line() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("rollout-x.jsonl");
        let l1 = r#"{"timestamp":"2026-06-02T18:56:04.390Z","type":"event_msg","payload":{"type":"user_message","message":"hi"}}"#;
        let partial = r#"{"timestamp":"2026-06-02T18:56:09.626Z","type":"event_msg","payload":{"type":"user_message","message":"oops"#;
        std::fs::write(&path, format!("{l1}\n{partial}")).unwrap();
        let u = tail_session(&path, 0).unwrap();
        assert_eq!(u.events.len(), 1, "partial trailing line must be ignored");
        assert_eq!(
            u.new_offset as usize,
            l1.len() + 1,
            "offset stops after the terminated line"
        );
    }
}
