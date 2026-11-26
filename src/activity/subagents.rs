//! Claude Code subagents running under a session: what the Agent tool
//! dispatched and what each is doing now.
//!
//! Claude Code keeps each subagent beside its parent's transcript:
//!
//! ```text
//! <project>/<session-id>.jsonl                       the parent session
//! <project>/<session-id>/subagents/agent-<id>.jsonl  the subagent's transcript
//! <project>/<session-id>/subagents/agent-<id>.meta.json
//! ```
//!
//! The meta file is written when the subagent starts, with its
//! `agentType`, `description` and `requestShape` (`background` or
//! `foreground`), and rewritten with `"finished": true` when it ends. A
//! meta file without that flag is a subagent still running — unless its
//! session died under it, which [`STALE_AFTER_MS`] guards against.
//!
//! The one-line activity is the subagent's latest tool call, read from the
//! end of its transcript and named the way Claude Code's own footer names
//! it: `Bash(cargo test)`, `Read(main.rs)`.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// A subagent whose transcript hasn't grown in this long is taken for
/// abandoned: a Claude Code that crashed or was killed never writes
/// `finished`, and its subagents would otherwise run forever here. Long
/// enough that a subagent sitting on a slow command (a test suite, a
/// build) keeps its place.
pub const STALE_AFTER_MS: i64 = 15 * 60 * 1000;

/// How much of a transcript's end is read for the latest tool call. Lines
/// run to tens of KiB (tool results, request records), so this holds
/// several turns; a transcript whose tail holds no assistant line is read
/// in full.
const TAIL_BYTES: u64 = 256 * 1024;

/// Longest activity line kept; the UI truncates further to fit.
const MAX_ACTIVITY_CHARS: usize = 200;

/// One running subagent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subagent {
    /// Claude Code's agent id, from the file name.
    pub id: String,
    /// `general-purpose`, `Explore`, a custom agent's name, …
    pub agent_type: String,
    /// The short task description the parent gave it.
    pub description: String,
    /// Dispatched to run in the background rather than blocking the turn.
    pub background: bool,
    /// The latest tool call, as `Tool(detail)`, or the start of its latest
    /// message when that has no tool call. `None` before its first reply.
    pub activity: Option<String>,
    /// When it started (epoch ms), from the meta file.
    pub started_ms: i64,
}

/// The directory holding `session_file`'s subagents.
pub fn subagents_dir(session_file: &Path) -> PathBuf {
    session_file.with_extension("").join("subagents")
}

/// The subagents still running under the Claude session whose transcript
/// is `session_file`, oldest first. Empty when it has none, or when the
/// session predates Claude Code's meta files.
pub fn scan(session_file: &Path, now_ms: i64) -> Vec<Subagent> {
    let Ok(entries) = std::fs::read_dir(subagents_dir(session_file)) else {
        return Vec::new();
    };
    let mut out: Vec<Subagent> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name();
            let id = name
                .to_str()?
                .strip_prefix("agent-")?
                .strip_suffix(".meta.json")?;
            read_running(&e.path(), id, now_ms)
        })
        .collect();
    out.sort_by(|a, b| a.started_ms.cmp(&b.started_ms).then(a.id.cmp(&b.id)));
    out
}

/// The subagent `meta` describes, when it is still running.
fn read_running(meta: &Path, id: &str, now_ms: i64) -> Option<Subagent> {
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(meta).ok()?).ok()?;
    if json.get("finished").and_then(|v| v.as_bool()) == Some(true) {
        return None;
    }
    let transcript = meta.with_file_name(format!("agent-{id}.jsonl"));
    let started_ms = mtime_ms(meta).unwrap_or(now_ms);
    let last_write_ms = mtime_ms(&transcript).unwrap_or(started_ms);
    if now_ms.saturating_sub(last_write_ms) > STALE_AFTER_MS {
        return None;
    }
    let text = |k: &str| {
        json.get(k)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    Some(Subagent {
        id: id.to_string(),
        agent_type: text("agentType"),
        description: text("description"),
        background: json.get("requestShape").and_then(|v| v.as_str()) == Some("background"),
        activity: latest_activity(&transcript),
        started_ms,
    })
}

fn mtime_ms(p: &Path) -> Option<i64> {
    let t = std::fs::metadata(p).ok()?.modified().ok()?;
    let d = t.duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(d.as_millis()).ok()
}

/// The latest activity in `transcript`: its tail first, then the whole
/// file if the tail held no assistant line.
fn latest_activity(transcript: &Path) -> Option<String> {
    let mut file = std::fs::File::open(transcript).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(TAIL_BYTES);
    if start > 0 {
        let tail = read_from(&mut file, start)?;
        // The first line is likely cut short; skip it.
        let whole_lines = tail.split_once('\n').map_or("", |(_, rest)| rest);
        if let Some(a) = activity_in(whole_lines) {
            return Some(a);
        }
    }
    activity_in(&read_from(&mut file, 0)?)
}

fn read_from(file: &mut std::fs::File, start: u64) -> Option<String> {
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// The activity in the last assistant line of `jsonl` that says anything.
fn activity_in(jsonl: &str) -> Option<String> {
    jsonl
        .lines()
        .rev()
        // Cheap filter before parsing: most lines are tool results and
        // request records.
        .filter(|l| l.contains("\"assistant\""))
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v.get("type").and_then(|t| t.as_str()) == Some("assistant"))
        .find_map(|v| activity_of(&v))
}

/// What one assistant message is doing: its last tool call, else the
/// first line of its text.
pub fn activity_of(record: &serde_json::Value) -> Option<String> {
    let blocks = record.get("message")?.get("content")?.as_array()?;
    let kind = |b: &serde_json::Value| b.get("type").and_then(|t| t.as_str()).map(String::from);
    if let Some(tool) = blocks
        .iter()
        .rev()
        .find(|b| kind(b).as_deref() == Some("tool_use"))
    {
        let name = tool.get("name").and_then(|n| n.as_str()).unwrap_or("tool");
        let input = tool.get("input").cloned().unwrap_or_default();
        return Some(clip(&tool_call(name, &input)));
    }
    blocks
        .iter()
        .rev()
        .filter(|b| kind(b).as_deref() == Some("text"))
        .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
        .find_map(|t| t.lines().map(str::trim).find(|l| !l.is_empty()))
        .map(clip)
}

/// A tool call named as Claude Code's footer names it: `Tool(detail)`, or
/// just `Tool` when there's nothing short to say about it.
pub fn tool_call(name: &str, input: &serde_json::Value) -> String {
    let s = |k: &str| {
        input
            .get(k)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
    };
    let detail = match name {
        // The description reads better at a glance than the command.
        "Bash" | "PowerShell" => s("description").or(s("command")).map(String::from),
        "Read" | "Edit" | "MultiEdit" | "Write" => s("file_path").map(basename),
        "NotebookEdit" => s("notebook_path").map(basename),
        "Grep" | "Glob" => s("pattern").map(String::from),
        "WebFetch" => s("url").map(String::from),
        "WebSearch" => s("query").map(String::from),
        "Agent" | "Task" => s("description").map(String::from),
        "Skill" => s("skill").map(String::from),
        _ => None,
    };
    // `mcp__server__tool` reads as `tool`.
    let shown = name.rsplit("__").next().unwrap_or(name);
    match detail {
        Some(d) => format!("{shown}({})", crate::activity::events::collapse_ws(&d)),
        None => shown.to_string(),
    }
}

fn basename(p: &str) -> String {
    Path::new(p)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(p)
        .to_string()
}

fn clip(s: &str) -> String {
    crate::activity::events::truncate_display(
        &crate::activity::events::collapse_ws(s),
        MAX_ACTIVITY_CHARS,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn now() -> i64 {
        crate::util::time::now_ms()
    }

    /// A session file inside a temp dir, with its subagents directory.
    fn session(dir: &Path) -> PathBuf {
        let file = dir.join("sess.jsonl");
        std::fs::write(&file, "").unwrap();
        std::fs::create_dir_all(subagents_dir(&file)).unwrap();
        file
    }

    fn write_agent(
        session_file: &Path,
        id: &str,
        meta: serde_json::Value,
        lines: &[serde_json::Value],
    ) {
        let dir = subagents_dir(session_file);
        std::fs::write(dir.join(format!("agent-{id}.meta.json")), meta.to_string()).unwrap();
        let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
        std::fs::write(dir.join(format!("agent-{id}.jsonl")), body).unwrap();
    }

    fn assistant(content: serde_json::Value) -> serde_json::Value {
        json!({"type": "assistant", "message": {"role": "assistant", "content": content}})
    }

    #[test]
    fn lists_running_subagents_and_skips_finished_ones() {
        let tmp = tempfile::tempdir().unwrap();
        let s = session(tmp.path());
        write_agent(
            &s,
            "a1",
            json!({"agentType": "Explore", "description": "Find the poller", "requestShape": "background"}),
            &[assistant(json!([
                {"type": "tool_use", "name": "Grep", "input": {"pattern": "fn poll"}}
            ]))],
        );
        write_agent(
            &s,
            "a2",
            json!({"agentType": "general-purpose", "description": "Done one", "finished": true}),
            &[],
        );
        let got = scan(&s, now());
        assert_eq!(got.len(), 1);
        let a = &got[0];
        assert_eq!(a.id, "a1");
        assert_eq!(a.agent_type, "Explore");
        assert_eq!(a.description, "Find the poller");
        assert!(a.background);
        assert_eq!(a.activity.as_deref(), Some("Grep(fn poll)"));
    }

    #[test]
    fn a_session_without_subagents_has_none() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("sess.jsonl");
        std::fs::write(&file, "").unwrap();
        assert!(scan(&file, now()).is_empty());
    }

    #[test]
    fn a_long_silent_subagent_is_taken_for_abandoned() {
        let tmp = tempfile::tempdir().unwrap();
        let s = session(tmp.path());
        write_agent(&s, "a1", json!({"description": "orphan"}), &[]);
        assert_eq!(scan(&s, now()).len(), 1);
        assert!(scan(&s, now() + STALE_AFTER_MS + 60_000).is_empty());
    }

    #[test]
    fn no_reply_yet_has_no_activity() {
        let tmp = tempfile::tempdir().unwrap();
        let s = session(tmp.path());
        write_agent(
            &s,
            "a1",
            json!({"description": "fresh"}),
            &[json!({"type": "user", "message": {"role": "user", "content": "go"}})],
        );
        assert_eq!(scan(&s, now())[0].activity, None);
    }

    #[test]
    fn activity_is_the_latest_assistant_line_that_says_something() {
        let lines = [
            assistant(
                json!([{"type": "tool_use", "name": "Read", "input": {"file_path": "/a/b/old.rs"}}]),
            ),
            json!({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "content": "x"}]}}),
            assistant(json!([
                {"type": "text", "text": "Let me check.\n\nMore."},
                {"type": "tool_use", "name": "Bash", "input": {"command": "cargo test", "description": "Run the tests"}}
            ])),
            json!({"type": "attachment"}),
        ];
        let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
        assert_eq!(activity_in(&body).as_deref(), Some("Bash(Run the tests)"));
    }

    #[test]
    fn a_message_without_tools_shows_its_first_line() {
        let v = assistant(json!([{"type": "text", "text": "\n  Found it in poll.rs.\nDetails…"}]));
        assert_eq!(activity_of(&v).as_deref(), Some("Found it in poll.rs."));
    }

    #[test]
    fn a_tail_without_an_assistant_line_falls_back_to_the_whole_file() {
        let tmp = tempfile::tempdir().unwrap();
        let s = session(tmp.path());
        let big = "x".repeat(TAIL_BYTES as usize + 10);
        write_agent(
            &s,
            "a1",
            json!({"description": "big"}),
            &[
                assistant(
                    json!([{"type": "tool_use", "name": "Glob", "input": {"pattern": "**/*.rs"}}]),
                ),
                json!({"type": "user", "message": {"role": "user", "content": big}}),
            ],
        );
        assert_eq!(
            scan(&s, now())[0].activity.as_deref(),
            Some("Glob(**/*.rs)")
        );
    }

    #[test]
    fn tool_calls_read_like_claude_codes_footer() {
        assert_eq!(
            tool_call("Bash", &json!({"command": "ls  -la"})),
            "Bash(ls -la)"
        );
        assert_eq!(
            tool_call("Edit", &json!({"file_path": "/x/y/z.rs"})),
            "Edit(z.rs)"
        );
        assert_eq!(
            tool_call("WebSearch", &json!({"query": "rust"})),
            "WebSearch(rust)"
        );
        assert_eq!(tool_call("TodoWrite", &json!({})), "TodoWrite");
        assert_eq!(tool_call("mcp__github__get_issue", &json!({})), "get_issue");
    }
}
