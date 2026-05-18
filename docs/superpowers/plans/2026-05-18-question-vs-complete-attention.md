# Question-vs-Complete Attention Detection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split wsx's coarse `ActivityState::Stopped` into `AwaitingAnswer` (Claude is waiting for an answer) and `Complete` (Claude finished a task), and surface that distinction through the dashboard glyphs, the top summary line, and the terminal bell pattern.

**Architecture:** Detection runs in the existing JSONL tailer. wsx already tracks unresolved `tool_use` blocks per workspace in `WorkspaceEvents.pending_tool_uses` (used today for permission prompts); we filter that map for the names `AskUserQuestion` / `ExitPlanMode`. As a fallback, we track the last assistant text block per workspace and check whether it ends with `?` after stripping trailing markdown noise. The classification is consumed by `classify_activity_with_events` and routed to the dashboard renderer and bell-firing helper. No new I/O, no new dependencies, no Claude Code SDK integration.

**Tech Stack:** Rust 2024 edition, ratatui 0.29, serde_json. Tests use `cargo test` (no nextest).

---

## File map

| File | Change |
|---|---|
| `src/events.rs` | Add `last_assistant_text` field + `pending_question_tool()` / `last_text_ends_with_question()` methods on `WorkspaceEvents`; extend `ParsedLine`/`parse_assistant` to capture trailing text; extend `tail_session` to forward it; new unit tests. |
| `src/app.rs` | Split `ActivityState::Stopped` into `AwaitingAnswer`/`Complete`; replace `bool stopped` arg in `classify_activity_with_events` with `Option<StoppedKind>`; refactor bell-firing into `fire_bell(state, store)` with per-state pattern dispatch; read new config settings; update `translate_activity`. |
| `src/ui/dashboard/mod.rs` | Replace `Item::Workspace.stopped: bool` with `stopped_kind: Option<StoppedKind>`; update `workspace_main_row` to render "question"/"complete" labels + nerd-font-aware attn glyph; update `top_summary_line` counts; update `activity_style`. |
| `src/ui/dashboard/tests.rs` | Update fixtures (`Item::Workspace { stopped: false, ... }` → `stopped_kind: None`). |
| `src/ui/updates_bar.rs` | Mirror state split in the re-export enum. |
| `docs/manual-tests/attention-detection.md` | Manual smoke-test procedure (no automated harness for terminal bells / live JSONL). |

---

## Task 1: Detection primitives in events.rs

**Goal:** Give `WorkspaceEvents` the ability to answer "is there a pending AskUserQuestion/ExitPlanMode tool?" and "did the last assistant text block end with `?`?". This task does NOT change any state machine; it adds the building blocks the next task consumes.

**Files:**
- Modify: `src/events.rs` (struct fields, `Default`, `reset_session_state`, new methods, `ParsedLine` extension, `parse_assistant`, `tail_session`)
- Modify: `src/events.rs` test module (add new tests at the end of `mod tests`)

- [ ] **Step 1: Write the failing test for `pending_question_tool`**

Add to `src/events.rs` inside `mod tests { ... }` (before the closing `}`):

```rust
#[test]
fn pending_question_tool_matches_ask_user_question() {
    let mut evt = WorkspaceEvents::default();
    evt.pending_tool_uses
        .insert("t1".into(), ("AskUserQuestion".into(), 1));
    assert_eq!(evt.pending_question_tool(), Some("AskUserQuestion"));
}

#[test]
fn pending_question_tool_matches_exit_plan_mode() {
    let mut evt = WorkspaceEvents::default();
    evt.pending_tool_uses
        .insert("t1".into(), ("ExitPlanMode".into(), 1));
    assert_eq!(evt.pending_question_tool(), Some("ExitPlanMode"));
}

#[test]
fn pending_question_tool_ignores_other_tools() {
    let mut evt = WorkspaceEvents::default();
    evt.pending_tool_uses
        .insert("t1".into(), ("Bash".into(), 1));
    evt.pending_tool_uses
        .insert("t2".into(), ("Read".into(), 2));
    assert_eq!(evt.pending_question_tool(), None);
}
```

- [ ] **Step 2: Run tests to verify they fail**

```
cargo test -p wsx --lib events::tests::pending_question_tool 2>&1 | tail -30
```
Expected: 3 compile errors — `no method named pending_question_tool found`.

- [ ] **Step 3: Add `pending_question_tool` method**

In `src/events.rs`, locate the `impl WorkspaceEvents { ... }` block (around line 141) and add this method inside, after `is_stalled`:

```rust
/// If any pending `tool_use` is `AskUserQuestion` or `ExitPlanMode`,
/// return the tool name. These tools mean "the agent has explicitly
/// asked the human for input" — distinct from a generic permission
/// prompt. Returns the first match (order across HashMap iteration is
/// unspecified, but in practice at most one such tool is pending).
pub fn pending_question_tool(&self) -> Option<&str> {
    for (name, _ts) in self.pending_tool_uses.values() {
        if name == "AskUserQuestion" || name == "ExitPlanMode" {
            return Some(name.as_str());
        }
    }
    None
}
```

- [ ] **Step 4: Run tests to verify they pass**

```
cargo test -p wsx --lib events::tests::pending_question_tool 2>&1 | tail -10
```
Expected: 3 passed.

- [ ] **Step 5: Write failing tests for `last_text_ends_with_question`**

Add to the same test module:

```rust
#[test]
fn last_text_ends_with_question_true_for_simple_question() {
    let mut evt = WorkspaceEvents::default();
    evt.last_assistant_text = Some("Want me to also handle X?".into());
    assert!(evt.last_text_ends_with_question());
}

#[test]
fn last_text_ends_with_question_strips_trailing_markdown() {
    // Claude often writes `Want me to refactor `foo`?*` where the literal
    // final char is `*` — we still want this classified as a question.
    let mut evt = WorkspaceEvents::default();
    evt.last_assistant_text = Some("Want me to refactor `foo`?*".into());
    assert!(evt.last_text_ends_with_question());
}

#[test]
fn last_text_ends_with_question_strips_trailing_whitespace() {
    let mut evt = WorkspaceEvents::default();
    evt.last_assistant_text = Some("Should I proceed?\n   ".into());
    assert!(evt.last_text_ends_with_question());
}

#[test]
fn last_text_ends_with_question_false_for_period_ending() {
    let mut evt = WorkspaceEvents::default();
    evt.last_assistant_text = Some("Done. Let me know if you'd like changes.".into());
    assert!(!evt.last_text_ends_with_question());
}

#[test]
fn last_text_ends_with_question_false_when_question_in_middle() {
    // A `?` in the middle followed by a declarative final sentence should
    // not trip the heuristic. Only the trailing char (after markdown trim)
    // matters.
    let mut evt = WorkspaceEvents::default();
    evt.last_assistant_text = Some("I considered: does this work? Yes, it works.".into());
    assert!(!evt.last_text_ends_with_question());
}

#[test]
fn last_text_ends_with_question_false_for_empty_or_missing() {
    let evt = WorkspaceEvents::default();
    assert!(!evt.last_text_ends_with_question());
    let mut evt = evt;
    evt.last_assistant_text = Some(String::new());
    assert!(!evt.last_text_ends_with_question());
    evt.last_assistant_text = Some("   \n  ".into());
    assert!(!evt.last_text_ends_with_question());
}
```

- [ ] **Step 6: Run tests to verify they fail**

```
cargo test -p wsx --lib events::tests::last_text_ends 2>&1 | tail -30
```
Expected: 6 compile errors — `no field last_assistant_text on type WorkspaceEvents` and `no method last_text_ends_with_question found`.

- [ ] **Step 7: Add `last_assistant_text` field + `last_text_ends_with_question` method**

In `src/events.rs`, find the `WorkspaceEvents` struct (starts ~line 99) and add a new field at the bottom, before the closing `}`:

```rust
    /// The text of the most recent assistant text content block, if any.
    /// Used by the question-vs-complete classifier to decide whether a
    /// stopped turn ended on a trailing `?`. Cleared on session reset.
    pub last_assistant_text: Option<String>,
```

Update `impl Default for WorkspaceEvents` (around line 126) to initialize it:

```rust
impl Default for WorkspaceEvents {
    fn default() -> Self {
        Self {
            latest: None,
            log: VecDeque::with_capacity(MAX_LOG),
            file_path: None,
            byte_offset: 0,
            pending_tool_uses: HashMap::new(),
            last_stop_reason: None,
            user_replied_since_stop: false,
            last_log_activity_ms: 0,
            last_assistant_text: None,
        }
    }
}
```

Update `reset_session_state` (around line 145) to clear it:

```rust
pub fn reset_session_state(&mut self) {
    self.pending_tool_uses.clear();
    self.last_stop_reason = None;
    self.user_replied_since_stop = false;
    self.last_log_activity_ms = 0;
    self.last_assistant_text = None;
}
```

Add the method inside `impl WorkspaceEvents`, after `pending_question_tool`:

```rust
/// True iff the most recent assistant text block ends with `?` (after
/// stripping trailing whitespace and markdown noise — `*`, `_`, `` ` ``).
/// Fallback signal used by the question-vs-complete classifier when
/// neither `AskUserQuestion` nor `ExitPlanMode` was invoked.
pub fn last_text_ends_with_question(&self) -> bool {
    let Some(text) = self.last_assistant_text.as_deref() else {
        return false;
    };
    let trimmed = text.trim_end_matches(|c: char| {
        c.is_whitespace() || matches!(c, '*' | '_' | '`')
    });
    trimmed.chars().next_back() == Some('?')
}
```

- [ ] **Step 8: Run tests to verify they pass**

```
cargo test -p wsx --lib events::tests::last_text_ends 2>&1 | tail -15
```
Expected: 6 passed.

- [ ] **Step 9: Write failing test for parser capturing `last_assistant_text`**

Add to the same test module:

```rust
#[test]
fn parse_assistant_captures_last_text_for_classifier() {
    let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Want me to also run tests?"}],"stop_reason":"end_turn"},"timestamp":"2026-05-14T17:32:14.000Z"}"#;
    let parsed = parse_jsonl_line(line);
    assert_eq!(
        parsed.last_assistant_text.as_deref(),
        Some("Want me to also run tests?")
    );
}

#[test]
fn parse_assistant_skips_capturing_text_when_only_tool_use() {
    // When the assistant message has only tool_use blocks, there is no
    // trailing text to feed the classifier. `last_assistant_text` stays None.
    let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]},"timestamp":"2026-05-14T17:32:14.000Z"}"#;
    let parsed = parse_jsonl_line(line);
    assert_eq!(parsed.last_assistant_text, None);
}
```

- [ ] **Step 10: Run tests to verify they fail**

```
cargo test -p wsx --lib events::tests::parse_assistant_captures 2>&1 | tail -10
cargo test -p wsx --lib events::tests::parse_assistant_skips_capturing 2>&1 | tail -10
```
Expected: 2 compile errors — `no field last_assistant_text on type ParsedLine`.

- [ ] **Step 11: Extend `ParsedLine` and `parse_assistant`**

In `src/events.rs`, locate `ParsedLine` (around line 309) and add a field:

```rust
#[derive(Debug, Default)]
pub struct ParsedLine {
    pub event: Option<EventSnapshot>,
    pub tool_use_starts: Vec<(String, String, i64)>,
    pub tool_use_resolves: Vec<String>,
    pub stop_reason: Option<StopReason>,
    pub is_user_text: bool,
    /// The text of the last `text` content block in this assistant message.
    /// Used by the classifier in app.rs to compute the "trailing `?`"
    /// fallback. None for any non-assistant line, or for assistant
    /// messages with no text blocks.
    pub last_assistant_text: Option<String>,
}
```

Update `parse_assistant` (around line 379). Find the `if let Some(t) = last_text { ... }` block at the end and **add a line above it that captures the text into ParsedLine regardless of whether tool_use also fired**:

```rust
fn parse_assistant(v: &serde_json::Value, timestamp_ms: i64) -> ParsedLine {
    let mut out = ParsedLine::default();
    if let Some(sr) = v
        .get("message")
        .and_then(|m| m.get("stop_reason"))
        .and_then(|s| s.as_str())
    {
        out.stop_reason = Some(StopReason::from_json_str(sr));
    }
    let Some(blocks) = v
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
    else {
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
            "tool_use" => {
                let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let input = block.get("input").unwrap_or(&serde_json::Value::Null);
                last_tool = Some((name, input));
                if let Some(id) = block.get("id").and_then(|i| i.as_str()) {
                    out.tool_use_starts
                        .push((id.to_string(), name.to_string(), timestamp_ms));
                }
            }
            _ => {}
        }
    }
    // Capture the final text block for the classifier BEFORE returning down
    // the tool-use display path. The display preference (tool > text) is
    // unchanged; we just also remember the text for downstream classification.
    if let Some(t) = last_text {
        out.last_assistant_text = Some(t.to_string());
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
```

- [ ] **Step 12: Run tests to verify they pass**

```
cargo test -p wsx --lib events::tests::parse_assistant 2>&1 | tail -15
```
Expected: all assistant-parsing tests pass (5 total — the 2 new ones plus the 3 pre-existing).

- [ ] **Step 13: Extend `TailUpdate` and `tail_session` to carry `last_assistant_text`**

In `src/events.rs`, add a field to `TailUpdate` (around line 178):

```rust
#[derive(Debug, Clone, Default)]
pub struct TailUpdate {
    pub new_offset: u64,
    pub events: Vec<EventSnapshot>,
    pub tool_use_starts: Vec<(String, String, i64)>,
    pub tool_use_resolves: Vec<String>,
    pub last_stop_reason: Option<StopReason>,
    pub human_replied_after_last_stop: bool,
    pub reset_from_zero: bool,
    /// The most recent assistant text block observed in this batch, if
    /// any. The caller stores this on WorkspaceEvents for the classifier.
    /// None means "no new text in this batch" — keep the prior value.
    pub last_assistant_text: Option<String>,
}
```

In `tail_session` (around line 258), inside the read loop, capture the text:

```rust
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
    }
    if parsed.is_user_text {
        update.human_replied_after_last_stop = true;
    }
    if let Some(text) = parsed.last_assistant_text {
        update.last_assistant_text = Some(text);
    }
}
```

- [ ] **Step 14: Wire `last_assistant_text` from TailUpdate into WorkspaceEvents**

In `src/app.rs`, locate the `TailUpdate` destructuring (around line 2064) and add the new field:

```rust
if let Ok(update) = crate::events::tail_session(&file, prev_offset) {
    let crate::events::TailUpdate {
        new_offset,
        events,
        tool_use_starts,
        tool_use_resolves,
        last_stop_reason,
        human_replied_after_last_stop,
        reset_from_zero,
        last_assistant_text,
    } = update;
```

Then after the existing `if let Some(sr) = last_stop_reason { ... }` block (around line 2110-2113), add:

```rust
                    if let Some(text) = last_assistant_text {
                        evt.last_assistant_text = Some(text);
                    }
```

- [ ] **Step 15: Verify the whole crate still builds and all tests pass**

```
cargo build -p wsx 2>&1 | tail -10
cargo test -p wsx --lib 2>&1 | tail -15
```
Expected: clean build, all tests pass (existing tests + ~10 new ones from this task).

- [ ] **Step 16: Commit**

```
git add src/events.rs src/app.rs
git commit -m "feat(events): track pending question-tools + last assistant text

Adds the building blocks for distinguishing 'Claude is waiting for an
answer' from 'Claude finished a task' — a filtered view of the
existing pending_tool_uses map plus a captured copy of the last
assistant text block. No behavior change yet; consumed in the next
commit."
```

---

## Task 2: Split `ActivityState::Stopped` into `AwaitingAnswer` and `Complete`

**Goal:** Replace the single `Stopped` state with two — `AwaitingAnswer` (Claude is waiting for the user to answer something) and `Complete` (Claude finished a turn with no outstanding question). The classifier in `app.rs` uses the primitives added in Task 1 to decide which.

**Files:**
- Modify: `src/app.rs` (`ActivityState`, `is_alertable`, `classify_activity_with_events`, call site at the dashboard render, `translate_activity`)
- Modify: `src/ui/updates_bar.rs` (mirror enum)
- Modify: `src/ui/dashboard/mod.rs` (`Item::Workspace.stopped: bool` → `stopped_kind: Option<StoppedKind>`; consumers)
- Modify: `src/ui/dashboard/tests.rs` (update fixture)

- [ ] **Step 1: Add `StoppedKind` enum + variants to `ActivityState` (will not yet compile)**

In `src/app.rs`, replace the `ActivityState` enum (around line 76) with:

```rust
/// Why the agent paused at end-of-turn. Distinguishes "asked the user
/// something and is waiting for an answer" from "finished a task".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoppedKind {
    /// The agent invoked `AskUserQuestion` or `ExitPlanMode` and the
    /// user hasn't responded yet, OR the final assistant text ended
    /// with `?` (fallback). Maps to the "?" dashboard glyph.
    AwaitingAnswer,
    /// The agent finished without asking the user anything. Maps to
    /// the "✓" dashboard glyph.
    Complete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityState {
    /// The agent has stopped its turn and is waiting for an answer
    /// from the user. Higher priority than PTY-recency states.
    AwaitingAnswer,
    /// The agent has stopped its turn with a completed task and is
    /// awaiting acknowledgment. Higher priority than PTY-recency states.
    Complete,
    /// A tool_use has been pending for ≥3s (almost always a permission
    /// prompt). Higher priority than `AwaitingAnswer` / `Complete`.
    Awaiting,
    /// < 2s since last PTY output.
    Active,
    /// 2–30s since last PTY output.
    Idle,
    /// Claude has stalled between turns: the JSONL log hasn't been
    /// appended for >60s, no tool_use is pending, and we've seen at
    /// least one stop_reason in this session. Alertable.
    Stalled,
    /// More than 30s since last PTY output but no JSONL stop signal.
    /// Retained for the recency column; does NOT drive the bell.
    Waiting,
    /// No session attached at all.
    Off,
}

impl ActivityState {
    /// States that should fire a bell + attention marker when entered.
    pub fn is_alertable(self) -> bool {
        matches!(
            self,
            ActivityState::AwaitingAnswer
                | ActivityState::Complete
                | ActivityState::Awaiting
                | ActivityState::Stalled
        )
    }
}
```

Now update `classify_activity_with_events` (around line 128). Replace the `stopped: bool` parameter with `stopped_kind: Option<StoppedKind>`:

```rust
/// Compute the activity state for a workspace, combining JSONL-derived
/// signals with PTY-output recency.
///
/// Priority: `Awaiting` (permission prompt) > `AwaitingAnswer` /
/// `Complete` (turn ended) > `Stalled` (mid-tool-chain quiet) >
/// PTY-recency > `Off`.
fn classify_activity_with_events(
    secs: Option<u64>,
    running: bool,
    awaiting: bool,
    stopped_kind: Option<StoppedKind>,
    stalled: bool,
) -> ActivityState {
    if awaiting {
        return ActivityState::Awaiting;
    }
    match stopped_kind {
        Some(StoppedKind::AwaitingAnswer) => return ActivityState::AwaitingAnswer,
        Some(StoppedKind::Complete) => return ActivityState::Complete,
        None => {}
    }
    if stalled {
        return ActivityState::Stalled;
    }
    if !running {
        return ActivityState::Off;
    }
    classify_activity(secs)
}
```

- [ ] **Step 2: Update the call site that drives the classifier**

In `src/app.rs`, find the `stopped` boolean computation in the render loop (around line 561-570) and replace with:

```rust
                let stopped_kind = app.workspace_events.get(&ws.id).and_then(|e| {
                    if !e.is_awaiting_user() {
                        return None;
                    }
                    // Tool detection has priority over text heuristic.
                    if e.pending_question_tool().is_some() {
                        Some(StoppedKind::AwaitingAnswer)
                    } else if e.last_text_ends_with_question() {
                        Some(StoppedKind::AwaitingAnswer)
                    } else {
                        Some(StoppedKind::Complete)
                    }
                });
                let stalled = app
                    .workspace_events
                    .get(&ws.id)
                    .is_some_and(|e| e.is_stalled(now_ms, 60_000));
                let activity = classify_activity_with_events(
                    secs,
                    running,
                    awaiting,
                    stopped_kind,
                    stalled,
                );
```

- [ ] **Step 3: Update `translate_activity` to mirror the split**

Find `translate_activity` in `src/app.rs` (around line 1950) and replace:

```rust
fn translate_activity(a: ActivityState) -> crate::ui::updates_bar::ActivityState {
    use crate::ui::updates_bar::ActivityState as U;
    match a {
        ActivityState::AwaitingAnswer => U::AwaitingAnswer,
        ActivityState::Complete => U::Complete,
        ActivityState::Awaiting => U::Awaiting,
        ActivityState::Active => U::Active,
        ActivityState::Idle => U::Idle,
        ActivityState::Stalled => U::Stalled,
        ActivityState::Waiting => U::Waiting,
        ActivityState::Off => U::Off,
    }
}
```

- [ ] **Step 4: Mirror the split in `updates_bar::ActivityState`**

In `src/ui/updates_bar.rs`, replace the enum (around line 15):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityState {
    /// Agent paused waiting for the user to answer a question.
    AwaitingAnswer,
    /// Agent finished a task and is awaiting acknowledgment.
    Complete,
    Awaiting,
    Active,
    Idle,
    /// Claude has stalled mid-tool-chain.
    Stalled,
    Waiting,
    Off,
}
```

Search the file for any other `ActivityState::Stopped` references:

```
grep -n "ActivityState::Stopped\|AS::Stopped\|::Stopped" src/ui/updates_bar.rs
```
If any matches appear, replace each `Stopped` with both new variants (`AwaitingAnswer | Complete`) — every match site must handle both.

- [ ] **Step 5: Update `Item::Workspace.stopped` field in the dashboard**

In `src/ui/dashboard/mod.rs`, find the `Item::Workspace` variant (around line 18) and replace the `stopped: bool` field with `stopped_kind: Option<crate::app::StoppedKind>`. The full variant after change:

```rust
Workspace {
    repo: &'a Repo,
    workspace: &'a Workspace,
    session_running: bool,
    seconds_since_activity: Option<u64>,
    has_prior_session: bool,
    status: Option<crate::git::WorkspaceStatus>,
    latest_event: Option<crate::events::EventSnapshot>,
    needs_attention: bool,
    lifecycle: Option<crate::forge::BranchLifecycle>,
    awaiting_tool: Option<(String, i64)>,
    /// Why the agent paused. `None` when no stop_reason or when
    /// the user has already replied.
    stopped_kind: Option<crate::app::StoppedKind>,
    stalled: bool,
    proc_count: usize,
},
```

Now find every consumer of the old `stopped` field and update. First, the destructure in `render` (around line 122):

```rust
            Item::Workspace {
                repo: _,
                workspace,
                session_running,
                seconds_since_activity,
                has_prior_session,
                status,
                latest_event,
                needs_attention,
                lifecycle,
                awaiting_tool,
                stopped_kind,
                stalled,
                proc_count,
            } => {
```

Update the `workspace_main_row` call (around line 142) — pass `*stopped_kind` instead of `*stopped`:

```rust
                let main = workspace_main_row(
                    workspace,
                    *session_running,
                    *seconds_since_activity,
                    *has_prior_session,
                    *status,
                    *needs_attention,
                    *lifecycle,
                    awaiting_tool,
                    *stopped_kind,
                    *stalled,
                    *proc_count,
                    nerd_fonts,
                    theme,
                    inner_width,
                );
```

Update `workspace_main_row`'s signature (around line 422). Replace the `stopped: bool` parameter with `stopped_kind: Option<crate::app::StoppedKind>`:

```rust
#[allow(clippy::too_many_arguments)]
fn workspace_main_row(
    workspace: &Workspace,
    session_running: bool,
    seconds_since_activity: Option<u64>,
    has_prior_session: bool,
    status: Option<crate::git::WorkspaceStatus>,
    needs_attention: bool,
    lifecycle: Option<crate::forge::BranchLifecycle>,
    awaiting_tool: &Option<(String, i64)>,
    stopped_kind: Option<crate::app::StoppedKind>,
    stalled: bool,
    proc_count: usize,
    nerd: bool,
    theme: &Theme,
    inner_width: usize,
) -> Line<'static> {
```

Update the `activity` selection inside `workspace_main_row` (around line 444):

```rust
    let activity = if awaiting_tool.is_some() {
        "awaiting"
    } else {
        match stopped_kind {
            Some(crate::app::StoppedKind::AwaitingAnswer) => "question",
            Some(crate::app::StoppedKind::Complete) => "complete",
            None if stalled => "stalled",
            None => match (seconds_since_activity, has_prior_session) {
                (Some(s), _) if s < 2 => "active",
                (Some(s), _) if s < 30 => "idle",
                (Some(_), _) => "waiting",
                (None, true) => "resumable",
                (None, false) => "off",
            },
        }
    };
```

Update `activity_style` (around line 409):

```rust
fn activity_style(label: &str, theme: &Theme) -> Style {
    match label {
        "awaiting" | "question" | "stalled" => theme.warn_style(),
        "complete" | "active" => theme.ok_style(),
        "idle" => Style::default(),
        "waiting" | "resumable" | "off" => theme.dim_style(),
        _ => Style::default(),
    }
}
```

- [ ] **Step 6: Update `top_summary_line` to count question + complete separately**

In `src/ui/dashboard/mod.rs`, replace `top_summary_line` (around line 252):

```rust
fn top_summary_line(items: &[Item], theme: &Theme) -> Line<'static> {
    let mut total = 0usize;
    let mut awaiting = 0usize;
    let mut question = 0usize;
    let mut complete = 0usize;
    let mut stalled_n = 0usize;
    for item in items {
        if let Item::Workspace {
            awaiting_tool,
            stopped_kind,
            stalled,
            ..
        } = item
        {
            total += 1;
            // Priority matches `classify_activity_with_events`: awaiting >
            // stopped_kind > stalled. A workspace with multiple flags
            // counts only toward its highest-priority bucket.
            if awaiting_tool.is_some() {
                awaiting += 1;
            } else {
                match stopped_kind {
                    Some(crate::app::StoppedKind::AwaitingAnswer) => question += 1,
                    Some(crate::app::StoppedKind::Complete) => complete += 1,
                    None if *stalled => stalled_n += 1,
                    None => {}
                }
            }
        }
    }
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled("wsx".to_string(), theme.header_style()));
    spans.push(Span::styled(
        format!(" · {total} workspace{}", if total == 1 { "" } else { "s" }),
        theme.dim_style(),
    ));
    if awaiting > 0 {
        spans.push(Span::styled(" · ".to_string(), theme.dim_style()));
        spans.push(Span::styled(format!("{awaiting}"), theme.warn_style()));
        spans.push(Span::styled(" permission".to_string(), theme.dim_style()));
    }
    if question > 0 {
        spans.push(Span::styled(" · ".to_string(), theme.dim_style()));
        spans.push(Span::styled(format!("{question}"), theme.warn_style()));
        spans.push(Span::styled(" question".to_string(), theme.dim_style()));
    }
    if complete > 0 {
        spans.push(Span::styled(" · ".to_string(), theme.dim_style()));
        spans.push(Span::styled(format!("{complete}"), theme.ok_style()));
        spans.push(Span::styled(" complete".to_string(), theme.dim_style()));
    }
    if stalled_n > 0 {
        spans.push(Span::styled(" · ".to_string(), theme.dim_style()));
        spans.push(Span::styled(format!("{stalled_n}"), theme.warn_style()));
        spans.push(Span::styled(" stalled".to_string(), theme.dim_style()));
    }
    Line::from(spans)
}
```

- [ ] **Step 7: Update the dashboard test fixture**

In `src/ui/dashboard/tests.rs`, find the `renders_repo_header_with_indented_workspace` test (around line 51) and replace `stopped: false` with `stopped_kind: None`:

```rust
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
```

Also `grep` for any other `stopped: false` or `stopped: true` in the test file and apply the equivalent fix:

```
grep -n "stopped:" src/ui/dashboard/tests.rs
```
For each match, replace `stopped: false` → `stopped_kind: None`, and `stopped: true` → `stopped_kind: Some(crate::app::StoppedKind::Complete)` (preserving the test's original intent — a stopped state with no extra info defaults to Complete).

- [ ] **Step 8: Update app.rs `Item::Workspace` construction**

In `src/app.rs`, locate the first per-workspace loop that builds the dashboard items (starts around line 458). The block currently computes `stopped: bool` at lines 488-491:

```rust
                    let stopped = app
                        .workspace_events
                        .get(&ws.id)
                        .is_some_and(|e| e.is_awaiting_user());
```

Replace that block (lines 488-491) with the new `stopped_kind` computation:

```rust
                    let stopped_kind = app.workspace_events.get(&ws.id).and_then(|e| {
                        if !e.is_awaiting_user() {
                            return None;
                        }
                        if e.pending_question_tool().is_some()
                            || e.last_text_ends_with_question()
                        {
                            Some(StoppedKind::AwaitingAnswer)
                        } else {
                            Some(StoppedKind::Complete)
                        }
                    });
```

Then update the `Item::Workspace` push (line 496-517) — change the field `stopped,` (line 508) to `stopped_kind,`:

```rust
                    items.push(dashboard::Item::Workspace {
                        repo,
                        workspace: ws,
                        session_running: running,
                        seconds_since_activity: secs,
                        has_prior_session: has_prior,
                        status: app.workspace_status.get(&ws.id).copied(),
                        latest_event: app
                            .workspace_events
                            .get(&ws.id)
                            .and_then(|e| e.latest.clone()),
                        needs_attention,
                        stopped_kind,
                        stalled,
                        lifecycle: app.pr_lifecycle.get(&ws.id).copied(),
                        awaiting_tool: awaiting,
                        proc_count: app
                            .workspace_processes
                            .get(&ws.id)
                            .map(|v| v.len())
                            .unwrap_or(0),
                    });
```

Note: the same logic is intentionally duplicated here and in Step 2's classifier loop (around line 561-570). The two loops are independent — the items push runs once per render, while the activity classification runs once per render-and-tick. Sharing state between them would require a per-workspace cache and isn't worth the complexity. They will agree because they read the same `WorkspaceEvents`.

- [ ] **Step 9: Verify everything builds and tests pass**

```
cargo build -p wsx 2>&1 | tail -10
cargo test -p wsx 2>&1 | tail -20
```
Expected: clean build, all tests pass. If you hit `match` exhaustiveness errors anywhere, that's the safety net catching unhandled `Stopped` references — add arms for `AwaitingAnswer` and `Complete` to each.

- [ ] **Step 10: Sanity-check by greping for any lingering references**

```
grep -rn "ActivityState::Stopped\|StoppedKind" src/ tests/ | head -40
```
Expected: no `ActivityState::Stopped` references remain. `StoppedKind` should appear in `app.rs` (definition + classifier), `dashboard/mod.rs` (Item field + consumers), and `dashboard/tests.rs` (fixture).

- [ ] **Step 11: Commit**

```
git add src/app.rs src/ui/updates_bar.rs src/ui/dashboard/mod.rs src/ui/dashboard/tests.rs
git commit -m "feat(activity): split Stopped into AwaitingAnswer + Complete

The dashboard now distinguishes 'Claude is waiting for an answer'
(question) from 'Claude finished a task' (complete) based on whether
the final turn invoked AskUserQuestion / ExitPlanMode, or as a
fallback whether the last assistant text ends with '?'.

Top summary line now counts question / complete separately:
  wsx · 12 workspaces · 1 permission · 1 question · 2 complete

Bell-firing behavior is unchanged in this commit; the bell-pattern
dispatch lands in the next commit."
```

---

## Task 3: Nerd-font-aware attention marker

**Goal:** Replace the single `!` glyph in the leftmost column with state-aware glyphs — question circle for `AwaitingAnswer`, check circle for `Complete`. Use nerd-font glyphs when enabled, ASCII fallback otherwise. Keep `!` for permission/stalled (those don't need new icons).

**Files:**
- Modify: `src/ui/dashboard/mod.rs` (extend `workspace_main_row` with `attn_glyph` logic)
- Modify: `src/ui/dashboard/tests.rs` (add a test for glyph selection)

- [ ] **Step 1: Write a failing test asserting the question glyph renders**

In `src/ui/dashboard/tests.rs`, append:

```rust
#[test]
fn renders_question_glyph_for_awaiting_answer() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let w = workspace(1, 1, "alpha", "alpha");
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: true,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: Some(crate::app::StoppedKind::AwaitingAnswer),
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| {
        render(
            f,
            f.area(),
            &items,
            None,
            false, // ASCII (nerd_fonts = false)
            &t(),
            &mut state,
        )
    })
    .unwrap();
    let text = dump(&term, 120, 8);
    assert!(text.contains("?"), "expected '?' attention marker: {text}");
    assert!(text.contains("question"), "expected 'question' activity label: {text}");
}

#[test]
fn renders_check_glyph_for_complete() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let w = workspace(1, 1, "alpha", "alpha");
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: true,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: Some(crate::app::StoppedKind::Complete),
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| {
        render(f, f.area(), &items, None, false, &t(), &mut state)
    })
    .unwrap();
    let text = dump(&term, 120, 8);
    assert!(text.contains("\u{2713}"), "expected '✓' attention marker: {text}");
    assert!(text.contains("complete"), "expected 'complete' activity label: {text}");
}
```

- [ ] **Step 2: Run tests to verify they fail**

```
cargo test -p wsx --lib ui::dashboard::tests::renders_question_glyph 2>&1 | tail -10
cargo test -p wsx --lib ui::dashboard::tests::renders_check_glyph 2>&1 | tail -10
```
Expected: both fail — `expected '?' attention marker` because the marker is still `!`, and `expected '✓' attention marker` because the marker is still `!`.

- [ ] **Step 3: Update `workspace_main_row` to compute a state-aware glyph**

In `src/ui/dashboard/mod.rs`, find this line in `workspace_main_row` (around line 492):

```rust
    let attn = if needs_attention { "!" } else { " " };
```

Replace with:

```rust
    let attn = if needs_attention {
        match (awaiting_tool.is_some(), stopped_kind, stalled, nerd) {
            // Permission prompt — single character, both nerd + ascii.
            (true, _, _, _) => "!",
            // Question — nerd-font question circle vs ascii fallback.
            (false, Some(crate::app::StoppedKind::AwaitingAnswer), _, true) => "\u{f128}",
            (false, Some(crate::app::StoppedKind::AwaitingAnswer), _, false) => "?",
            // Complete — nerd-font check circle vs ascii fallback.
            (false, Some(crate::app::StoppedKind::Complete), _, true) => "\u{f058}",
            (false, Some(crate::app::StoppedKind::Complete), _, false) => "\u{2713}",
            // Stalled — keep `!`.
            (false, None, true, _) => "!",
            // Defensive default — should be unreachable when needs_attention is true.
            (false, None, false, _) => " ",
        }
    } else {
        " "
    };
    let attn_style = match (awaiting_tool.is_some(), stopped_kind) {
        (false, Some(crate::app::StoppedKind::Complete)) => theme.ok_style(),
        _ => theme.warn_style(),
    };
```

Then find the line that pushes the attn span (a few lines below, around line 497):

```rust
    spans.push(Span::styled(attn.to_string(), theme.warn_style()));
```

Replace with:

```rust
    spans.push(Span::styled(attn.to_string(), attn_style));
```

- [ ] **Step 4: Run tests to verify they pass**

```
cargo test -p wsx --lib ui::dashboard::tests 2>&1 | tail -15
```
Expected: all dashboard tests pass.

- [ ] **Step 5: Commit**

```
git add src/ui/dashboard/mod.rs src/ui/dashboard/tests.rs
git commit -m "feat(dashboard): nerd-font + ASCII glyphs for question vs complete

Replaces the generic '!' attention marker with state-aware glyphs:
question circle for AwaitingAnswer, check circle for Complete. ASCII
fallbacks ('?' / '✓') for terminals without nerd-font support.
Permission and stalled keep '!'."
```

---

## Task 4: Per-state bell patterns with config

**Goal:** Replace the single shared `\x07` write with a `fire_bell(state, store)` helper that emits a different bell pattern per alertable state. Patterns are configurable via the existing store-backed settings.

**Files:**
- Modify: `src/app.rs` (introduce `fire_bell`, replace the inline write, add a `bell_pattern_for` config reader)

- [ ] **Step 1: Add the `BellPattern` enum + `fire_bell` helper**

In `src/app.rs`, add this after the `notifications_enabled` function (around line 847):

```rust
/// Bell patterns: how many `\x07` bytes to emit, with spacing.
#[derive(Debug, Clone, Copy)]
enum BellPattern {
    Off,
    Single,
    Double,
    Triple,
}

impl BellPattern {
    fn from_setting(s: Option<&str>) -> Option<Self> {
        match s {
            Some("off") | Some("false") | Some("0") => Some(BellPattern::Off),
            Some("single") => Some(BellPattern::Single),
            Some("double") => Some(BellPattern::Double),
            Some("triple") => Some(BellPattern::Triple),
            _ => None, // caller uses its own default
        }
    }
}

/// Pick the bell pattern for a given alertable state. Reads per-state
/// overrides from the store, falling back to sensible defaults.
fn bell_pattern_for(state: ActivityState, store: &crate::store::Store) -> BellPattern {
    let (key, default_pattern) = match state {
        ActivityState::AwaitingAnswer => ("notification_bell_question", BellPattern::Double),
        ActivityState::Complete => ("notification_bell_complete", BellPattern::Single),
        ActivityState::Awaiting => ("notification_bell_permission", BellPattern::Single),
        ActivityState::Stalled => ("notification_bell_stalled", BellPattern::Triple),
        // Non-alertable states never call fire_bell, but be safe.
        _ => return BellPattern::Off,
    };
    let stored = store.get_setting(key).ok().flatten();
    BellPattern::from_setting(stored.as_deref()).unwrap_or(default_pattern)
}

/// Emit a terminal-bell pattern for an alertable state. Multi-bell
/// patterns spawn a detached thread to space the writes (~120ms apart)
/// so the engine event loop isn't blocked.
fn fire_bell(state: ActivityState, store: &crate::store::Store) {
    use std::io::Write;
    let pattern = bell_pattern_for(state, store);
    let count = match pattern {
        BellPattern::Off => return,
        BellPattern::Single => 1,
        BellPattern::Double => 2,
        BellPattern::Triple => 3,
    };
    if count == 1 {
        let _ = std::io::stdout().write_all(b"\x07");
        let _ = std::io::stdout().flush();
        return;
    }
    std::thread::spawn(move || {
        for i in 0..count {
            if i > 0 {
                std::thread::sleep(std::time::Duration::from_millis(120));
            }
            let _ = std::io::stdout().write_all(b"\x07");
            let _ = std::io::stdout().flush();
        }
    });
}
```

- [ ] **Step 2: Replace the inline bell-write in the render loop**

In `src/app.rs`, find the existing bell-firing block (around line 572-582). Replace the inline write with a call to `fire_bell`. The code becomes:

```rust
            let mut bells_to_ring: Vec<ActivityState> = Vec::new();
            for (_rid, ws) in &app.workspaces {
                // ... existing per-workspace classification ...
                let prev = app.workspace_activity.get(&ws.id).copied();
                if activity.is_alertable() && prev != Some(activity) && notifications_on {
                    app.workspace_needs_attention.insert(ws.id);
                    bells_to_ring.push(activity);
                }
                app.workspace_activity.insert(ws.id, activity);
            }
            for state in bells_to_ring {
                fire_bell(state, &app.store);
            }
```

This collects the transitions first, then fires bells outside the per-workspace loop. With per-state patterns, a tick that produces multiple alertable transitions will ring multiple distinct patterns — they may interleave in the terminal, but each pattern's count is still audibly distinguishable.

- [ ] **Step 3: Verify build and existing tests still pass**

```
cargo build -p wsx 2>&1 | tail -10
cargo test -p wsx 2>&1 | tail -15
```
Expected: clean build, all tests pass.

- [ ] **Step 4: Write a unit test for `bell_pattern_for` defaults**

The store creation in tests requires some setup. Locate an existing app/store test for reference; if none has a simple in-memory store helper, skip this step — manual verification in Task 5 covers it. Otherwise, add a test at the bottom of `src/app.rs`:

```rust
#[cfg(test)]
mod bell_tests {
    use super::*;

    #[test]
    fn bell_pattern_off_for_non_alertable() {
        let store = crate::store::Store::open_in_memory().expect("in-memory store");
        assert!(matches!(
            bell_pattern_for(ActivityState::Active, &store),
            BellPattern::Off
        ));
    }

    #[test]
    fn bell_pattern_defaults_match_spec() {
        let store = crate::store::Store::open_in_memory().expect("in-memory store");
        assert!(matches!(
            bell_pattern_for(ActivityState::AwaitingAnswer, &store),
            BellPattern::Double
        ));
        assert!(matches!(
            bell_pattern_for(ActivityState::Complete, &store),
            BellPattern::Single
        ));
        assert!(matches!(
            bell_pattern_for(ActivityState::Stalled, &store),
            BellPattern::Triple
        ));
    }
}
```

Before running, check whether `Store::open_in_memory` exists:

```
grep -n "open_in_memory\|fn open\b" src/store.rs | head -10
```
If `open_in_memory` doesn't exist, look at how other tests in the codebase construct a test Store and adapt — search:

```
grep -rn "Store::" src/ tests/ | grep -i "test\|open\|new" | head -20
```

If you can't find a simple helper, delete this test step — the manual smoke test in Task 5 verifies bell defaults audibly.

- [ ] **Step 5: Run the new bell tests (if added)**

```
cargo test -p wsx --lib bell_tests 2>&1 | tail -10
```
Expected: both pass.

- [ ] **Step 6: Commit**

```
git add src/app.rs
git commit -m "feat(notifications): per-state bell patterns

Replace the single shared terminal bell with state-aware patterns:
  - Permission: single bell (unchanged)
  - Question:   double bell  (configurable)
  - Complete:   single bell  (configurable)
  - Stalled:    triple bell  (configurable)

Multi-bell patterns spawn a detached thread so spacing doesn't
block the engine loop. Each pattern can be overridden via store
settings: notification_bell_{question,complete,permission,stalled}
with values: off|single|double|triple."
```

---

## Task 5: Manual smoke test + documentation

**Goal:** Provide a manual verification procedure for the cases the test suite can't cover (terminal bell audibility, font rendering, live JSONL changes).

**Files:**
- Create: `docs/manual-tests/attention-detection.md`

- [ ] **Step 1: Create the manual test doc**

```bash
mkdir -p docs/manual-tests
```

Then create `docs/manual-tests/attention-detection.md`:

```markdown
# Manual smoke test: question vs complete attention detection

The automated test suite covers classifier logic and dashboard rendering.
This procedure covers what tests can't: audible bells, font rendering,
and the live JSONL flow.

## Setup

1. Start wsx in a terminal with audible bells enabled (Settings → Profiles
   → Audible bell on most macOS terminals).
2. Open at least two workspaces; attach to one and leave the other idle
   on the dashboard.

## Test 1: Complete (task done)

In the attached workspace, ask Claude Code to do something concrete:

> Rename README.md to README.txt and commit the change.

When Claude finishes (no question asked), detach back to the dashboard.

Expected:
- The other workspace marker shows `✓` (or the nerd-font check glyph
  if nerd-fonts are enabled).
- A single bell rings.
- Top summary line includes `1 complete`.
- The `complete` label in the row's activity column is green.

## Test 2: AwaitingAnswer via AskUserQuestion tool

Switch to the other workspace and ask Claude something open-ended that
will trigger AskUserQuestion:

> I'm not sure whether to use foo or bar — what do you think?

Wait for Claude to invoke `AskUserQuestion`. Detach back to the dashboard.

Expected:
- The workspace marker shows `?` (or the nerd-font question glyph).
- A double bell rings (~120ms apart).
- Top summary line includes `1 question`.
- The `question` label in the activity column is in warn style (red/yellow).

## Test 3: AwaitingAnswer via trailing-`?` fallback

In a fresh workspace, send Claude a prompt that will get a question back
without using `AskUserQuestion` (e.g., something where Claude wants to
clarify mid-task):

> Help me refactor my code.

If Claude responds with text ending in `?` (e.g., "Which file should I
start with?"), the trailing-`?` fallback should classify it.

Expected:
- Same `?` glyph, double bell, `question` count as Test 2.

## Test 4: No false positives from code blocks

Ask Claude:

> Show me a Python assertion.

Claude's response will likely end with a triple-backtick code block. The
trimmed text ends with the closing fence, not `?`.

Expected:
- The workspace marker shows `✓` (Complete), single bell.

## Test 5: Permission prompt unchanged

Ask Claude to run a shell command requiring approval. The permission
prompt should still trigger the existing `Awaiting` state.

Expected:
- The workspace marker shows `!` (unchanged).
- A single bell rings.
- Top summary line includes `1 permission`.

## Test 6: Config override

In the wsx settings, set `notification_bell_question` to `single` (via
whatever mechanism the dashboard exposes, or directly via the store).
Re-run Test 2.

Expected:
- The question state now rings a single bell instead of double.
```

- [ ] **Step 2: Commit**

```
git add docs/manual-tests/attention-detection.md
git commit -m "docs: manual smoke test for attention detection

Covers audible bells, font rendering, and live JSONL flow — the cases
the automated test suite can't reach."
```

---

## Self-review

After completing all five tasks, run this checklist:

- [ ] **Build + full test pass:** `cargo build -p wsx && cargo test -p wsx`
- [ ] **No lingering `ActivityState::Stopped` references:** `grep -rn "ActivityState::Stopped" src/ tests/` returns nothing.
- [ ] **No `stopped: bool` in `Item::Workspace`:** `grep -n "stopped:" src/ui/dashboard/` returns only `stopped_kind:` references.
- [ ] **Existing dashboard test still passes:** `cargo test -p wsx --lib ui::dashboard::tests::renders_repo_header_with_indented_workspace`
- [ ] **Manual smoke test:** Run `docs/manual-tests/attention-detection.md` end to end. All six tests behave as documented.

If any check fails, fix it before declaring the work done. The spec calls out two open questions worth confirming during implementation:

- **`ExitPlanMode` casing:** This plan assumes the tool name string in the jsonl is literally `ExitPlanMode`. If during manual testing the plan-mode flow doesn't trip the classifier, verify the actual name in a session jsonl file (`grep -h '"ExitPlanMode"' ~/.claude/projects/*/*.jsonl | head -1`) and adjust `pending_question_tool` accordingly.
- **Marker column width:** If nerd-font glyphs render as a single character but visually push the column alignment, the column-width math in `workspace_main_row` may need to treat the glyph as 2 cells. Inspect alignment in a terminal with nerd-fonts; if columns drift right by one, adjust the `left_w` calculation around line 531.
