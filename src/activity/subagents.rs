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
//! session died under it. A subagent silent for [`QUIET_AFTER_MS`] is
//! shown as quiet rather than dropped, since a long build or a blocking
//! tool writes nothing either; one silent for [`ABANDON_AFTER_MS`] is
//! taken for abandoned.
//!
//! The one-line activity is the subagent's latest tool call, read from the
//! end of its transcript and named the way Claude Code's own footer names
//! it: `Bash(cargo test)`, `Read(main.rs)`.
//!
//! [`scan`] runs on every poll, so it keeps what it learned in a
//! process-wide cache keyed by meta file: a finished subagent is never
//! read again, and a running one's files are only re-read when their size
//! or modification time changes.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};

/// A subagent whose transcript hasn't changed in this long is shown as
/// quiet: still listed, since it may be sitting on a slow command, but
/// with how long it has been silent in place of its activity.
pub const QUIET_AFTER_MS: i64 = 15 * 60 * 1000;

/// A subagent silent this long is taken for abandoned and dropped: a
/// Claude Code that crashed or was killed never writes `finished`, and its
/// subagents would otherwise run forever here.
pub const ABANDON_AFTER_MS: i64 = 2 * 60 * 60 * 1000;

/// How much of a transcript's end is read first for the latest tool call.
/// Lines run to tens of KiB (tool results, request records), so this
/// usually holds several turns.
const TAIL_BYTES: u64 = 256 * 1024;

/// The most of a transcript's end ever read. A tail with no assistant line
/// in it widens toward this, never past it, so one huge tool result can't
/// turn every poll into a read of the whole file.
const MAX_SCAN_BYTES: u64 = 4 * 1024 * 1024;

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
    /// When its transcript last changed (epoch ms).
    pub last_write_ms: i64,
}

impl Subagent {
    /// How long it has been silent, once that passes [`QUIET_AFTER_MS`].
    pub fn quiet_for(&self, now_ms: i64) -> Option<i64> {
        let silent = now_ms.saturating_sub(self.last_write_ms);
        (silent >= QUIET_AFTER_MS).then_some(silent)
    }
}

/// The directory holding `session_file`'s subagents.
pub fn subagents_dir(session_file: &Path) -> PathBuf {
    session_file.with_extension("").join("subagents")
}

/// A file's size and modification time: when neither changed, neither did
/// what was read from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    len: u64,
    mtime_ms: i64,
}

fn stamp(p: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(p).ok()?;
    let t = meta.modified().ok()?;
    let d = t.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(Stamp {
        len: meta.len(),
        mtime_ms: i64::try_from(d.as_millis()).ok()?,
    })
}

enum Cached {
    /// Finished; never read again.
    Finished,
    /// Running when last read, with the stamps of the files it was read
    /// from.
    Running {
        meta: Stamp,
        transcript: Option<Stamp>,
        sub: Subagent,
    },
}

/// What [`scan`] has learned, by meta file path.
static CACHE: LazyLock<Mutex<HashMap<PathBuf, Cached>>> = LazyLock::new(Default::default);

/// The subagents still running under the Claude session whose transcript
/// is `session_file`, oldest first. Empty when it has none, or when the
/// session predates Claude Code's meta files. An error means the
/// directory couldn't be read, which says nothing about what runs there.
pub fn scan(session_file: &Path, now_ms: i64) -> std::io::Result<Vec<Subagent>> {
    let dir = subagents_dir(session_file);
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            prune(&dir, &HashSet::new());
            return Ok(Vec::new());
        }
        Err(e) => return Err(e),
    };
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let Some(id) = name
            .to_str()
            .and_then(|n| n.strip_prefix("agent-"))
            .and_then(|n| n.strip_suffix(".meta.json"))
        else {
            continue;
        };
        if let Some(sub) = check(&path, id, now_ms) {
            out.push(sub);
        }
        seen.insert(path);
    }
    prune(&dir, &seen);
    out.sort_by(|a, b| a.started_ms.cmp(&b.started_ms).then(a.id.cmp(&b.id)));
    Ok(out)
}

/// Forget the cached meta files in `dir` that are no longer there.
fn prune(dir: &Path, seen: &HashSet<PathBuf>) {
    CACHE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|p, _| p.parent() != Some(dir) || seen.contains(p));
}

/// The subagent `meta` describes, when it is still running, read through
/// the cache.
fn check(meta: &Path, id: &str, now_ms: i64) -> Option<Subagent> {
    let prev = {
        let cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
        match cache.get(meta) {
            Some(Cached::Finished) => return None,
            Some(Cached::Running {
                meta,
                transcript,
                sub,
            }) => Some((*meta, *transcript, sub.clone())),
            None => None,
        }
    };
    let meta_stamp = stamp(meta)?;
    let mut sub = match &prev {
        Some((m, _, sub)) if *m == meta_stamp => sub.clone(),
        _ => match read_meta(meta) {
            Ok(None) => {
                CACHE
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(meta.to_path_buf(), Cached::Finished);
                return None;
            }
            Ok(Some(fields)) => Subagent {
                id: id.to_string(),
                agent_type: fields.agent_type,
                description: fields.description,
                background: fields.background,
                activity: prev.as_ref().and_then(|p| p.2.activity.clone()),
                started_ms: prev
                    .as_ref()
                    .map_or(meta_stamp.mtime_ms, |p| p.2.started_ms),
                last_write_ms: meta_stamp.mtime_ms,
            },
            // Caught mid-write, most likely: keep what it said last.
            Err(e) => {
                tracing::debug!(path = %meta.display(), error = %e, "unreadable subagent meta file");
                prev.as_ref()?.2.clone()
            }
        },
    };
    let transcript = meta.with_file_name(format!("agent-{id}.jsonl"));
    let transcript_stamp = stamp(&transcript);
    sub.last_write_ms = transcript_stamp.map_or(sub.started_ms, |t| t.mtime_ms);
    let unchanged = prev
        .as_ref()
        .is_some_and(|p| p.1.is_some() && p.1 == transcript_stamp);
    if !unchanged
        && transcript_stamp.is_some()
        && let Some(a) = latest_activity(&transcript)
    {
        sub.activity = Some(a);
    }
    CACHE.lock().unwrap_or_else(PoisonError::into_inner).insert(
        meta.to_path_buf(),
        Cached::Running {
            meta: meta_stamp,
            transcript: transcript_stamp,
            sub: sub.clone(),
        },
    );
    (now_ms.saturating_sub(sub.last_write_ms) < ABANDON_AFTER_MS).then_some(sub)
}

struct MetaFields {
    agent_type: String,
    description: String,
    background: bool,
}

/// A meta file's fields, or `None` once it says the subagent finished.
fn read_meta(meta: &Path) -> Result<Option<MetaFields>, Box<dyn std::error::Error>> {
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(meta)?)?;
    if json.get("finished").and_then(|v| v.as_bool()) == Some(true) {
        return Ok(None);
    }
    let text = |k: &str| {
        json.get(k)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    Ok(Some(MetaFields {
        agent_type: text("agentType"),
        description: text("description"),
        background: json.get("requestShape").and_then(|v| v.as_str()) == Some("background"),
    }))
}

/// The latest activity in `transcript`, from a tail that widens from
/// [`TAIL_BYTES`] to at most [`MAX_SCAN_BYTES`] until it holds an
/// assistant line. `None` when none does.
fn latest_activity(transcript: &Path) -> Option<String> {
    let mut file = std::fs::File::open(transcript).ok()?;
    let len = file.metadata().ok()?.len();
    let mut window = TAIL_BYTES;
    loop {
        let start = len.saturating_sub(window);
        let tail = read_from(&mut file, start)?;
        // A tail starting mid-file starts mid-line; skip that line.
        let whole_lines = if start > 0 {
            tail.split_once('\n').map_or("", |(_, rest)| rest)
        } else {
            tail.as_str()
        };
        if let Some(a) = activity_in(whole_lines) {
            return Some(a);
        }
        if start == 0 || window >= MAX_SCAN_BYTES {
            return None;
        }
        window = (window * 4).min(MAX_SCAN_BYTES);
    }
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
        let got = scan(&s, now()).unwrap();
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
        assert!(scan(&file, now()).unwrap().is_empty());
    }

    /// Silent past QUIET_AFTER_MS, a subagent is still listed and says how
    /// long it has been quiet; past ABANDON_AFTER_MS it is dropped.
    #[test]
    fn a_silent_subagent_goes_quiet_then_is_taken_for_abandoned() {
        let tmp = tempfile::tempdir().unwrap();
        let s = session(tmp.path());
        write_agent(&s, "a1", json!({"description": "slow build"}), &[]);
        let t = now();
        let fresh = scan(&s, t).unwrap();
        assert_eq!(fresh[0].quiet_for(t), None);
        let later = t + QUIET_AFTER_MS + 60_000;
        let quiet = scan(&s, later).unwrap();
        assert_eq!(quiet.len(), 1);
        assert!(quiet[0].quiet_for(later).is_some());
        assert!(scan(&s, t + ABANDON_AFTER_MS + 60_000).unwrap().is_empty());
    }

    /// A meta file caught mid-write keeps the subagent as it last read.
    #[test]
    fn a_half_written_meta_file_keeps_the_last_good_read() {
        let tmp = tempfile::tempdir().unwrap();
        let s = session(tmp.path());
        write_agent(&s, "a1", json!({"description": "steady"}), &[]);
        assert_eq!(scan(&s, now()).unwrap()[0].description, "steady");
        let meta = subagents_dir(&s).join("agent-a1.meta.json");
        std::fs::write(&meta, r#"{"description": "ste"#).unwrap();
        let got = scan(&s, now()).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].description, "steady");
    }

    /// Once finished, a subagent stays finished without its meta file
    /// being read again.
    #[test]
    fn a_finished_subagent_is_never_reread() {
        let tmp = tempfile::tempdir().unwrap();
        let s = session(tmp.path());
        write_agent(&s, "a1", json!({"finished": true}), &[]);
        assert!(scan(&s, now()).unwrap().is_empty());
        let meta = subagents_dir(&s).join("agent-a1.meta.json");
        std::fs::write(&meta, r#"{"description": "not read"}"#).unwrap();
        assert!(scan(&s, now()).unwrap().is_empty());
    }

    /// A subagents path that can't be listed is an error, not "none".
    #[test]
    fn an_unreadable_directory_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("sess.jsonl");
        std::fs::write(&file, "").unwrap();
        std::fs::create_dir_all(file.with_extension("")).unwrap();
        std::fs::write(subagents_dir(&file), "not a directory").unwrap();
        assert!(scan(&file, now()).is_err());
    }

    /// Past MAX_SCAN_BYTES of output with no assistant line, the read stops
    /// and the subagent keeps the activity it last had.
    #[test]
    fn the_tail_read_is_capped_and_keeps_the_last_activity() {
        let tmp = tempfile::tempdir().unwrap();
        let s = session(tmp.path());
        write_agent(
            &s,
            "a1",
            json!({"description": "big"}),
            &[assistant(
                json!([{"type": "tool_use", "name": "Glob", "input": {"pattern": "*.rs"}}]),
            )],
        );
        assert_eq!(
            scan(&s, now()).unwrap()[0].activity.as_deref(),
            Some("Glob(*.rs)")
        );
        let transcript = subagents_dir(&s).join("agent-a1.jsonl");
        let huge = json!({"type": "user", "message": {"content": "x".repeat(MAX_SCAN_BYTES as usize + 10)}});
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&transcript)
            .unwrap();
        std::io::Write::write_all(&mut f, format!("{huge}\n").as_bytes()).unwrap();
        assert_eq!(latest_activity(&transcript), None);
        assert_eq!(
            scan(&s, now()).unwrap()[0].activity.as_deref(),
            Some("Glob(*.rs)")
        );
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
        assert_eq!(scan(&s, now()).unwrap()[0].activity, None);
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
            scan(&s, now()).unwrap()[0].activity.as_deref(),
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
