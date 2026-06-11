# Live-edge rows + context-token detail line — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the dashboard's workspace row show the *live edge* of agent activity (the question being asked, the file/command in flight) while the detail bar gains a live *context-window fill* line — so the two surfaces stop duplicating the same status.

**Architecture:** New activity signals are parsed once in the `sessionx` crate (question topic, current tool action, context tokens, model id), aggregated through its `TailUpdate`, and merged onto `WorkspaceEvents` in wsx's background loop. The wsx row renderer (`column_content.rs`) and the detail bar (`session_summary.rs`) consume the new fields. Two repos, two PRs: **sessionx first**, then wsx bumps the dependency `rev`.

**Tech Stack:** Rust, `serde_json` (JSONL parsing), `ratatui` (TUI rendering). Spec: `docs/superpowers/specs/2026-06-11-live-edge-row-and-context-tokens-design.md`.

---

## File structure

**sessionx** (`github.com/bakedbean/sessionx`, cloned locally for editing):
- Modify `src/activity/events.rs` — add four fields to `ParsedLine`, `TailUpdate`, and `WorkspaceEvents`; parse them in `parse_assistant`; aggregate them in `tail_session`; clear them in `reset_session_state`; add a `file_basename` helper.

**wsx** (this repo):
- Modify `Cargo.toml` / `Cargo.lock` — patch to local sessionx during dev, then bump `rev`.
- Modify `src/app/background.rs` — merge the four new `TailUpdate` fields onto `WorkspaceEvents` (`:88-214`).
- Modify `src/ui/dashboard/column_content.rs` — enrich the `Question` and `Thinking`/`Waiting` arms of `row_column`; update affected tests.
- Modify `src/detail_modules/session_summary.rs` — add `abbreviate_tokens`, `resolve_window`, `format_context_line`; render a context-fill line; add tests.

---

# PHASE A — sessionx (separate repo + PR, land first)

> **LAYOUT NOTE (discovered during execution):** sessionx `main` is ahead of the
> rev wsx currently pins, and has refactored the single `src/activity/events.rs`
> into a module: the Claude events code now lives in
> `src/activity/events/mod.rs` (the `WorkspaceEvents` struct, its `Default`,
> `reset_session_state`, `TailUpdate`, `tail_session`, and the `#[cfg(test)] mod
> tests` block) and `src/activity/events/parse.rs` (`ParsedLine`,
> `parse_jsonl_line`, `parse_assistant`). The code bodies are byte-for-byte the
> same as the old file, so every code block below applies verbatim — only the
> file paths change. `parse.rs` has NO test module, so add all new tests to the
> test module in `src/activity/events/mod.rs` (it already imports the parse fns;
> follow existing tests like `parse_assistant_surfaces_edited_file_paths`).
> `codex_events.rs` / `pi_events.rs` are separate agent backends — do NOT touch
> them. We branch from `main` (not the old pinned rev) so the PR merges cleanly.

### Task A1: Clone sessionx and branch

**Files:** none (setup only)

- [ ] **Step 1: Branch from main**

A clone already exists at `/home/eben/sessionx` on `main` (which is ahead of the
rev wsx pins, including the events.rs → events/ module refactor — see LAYOUT NOTE).

```bash
cd /home/eben/sessionx
git fetch origin
git switch main && git pull --ff-only
git switch -c live-edge-activity-signals
```

- [ ] **Step 2: Verify it builds and tests pass on the baseline**

Run: `cargo test --manifest-path /home/eben/sessionx/Cargo.toml`
Expected: PASS (clean baseline before any change).

---

### Task A2: Parse context tokens + model id in `parse_assistant`

**Files:**
- Modify: `/home/eben/sessionx/src/activity/events.rs` (`ParsedLine` struct ~549-584; `parse_assistant` ~677-695)

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `events.rs`:

```rust
#[test]
fn parse_assistant_captures_context_tokens_and_model() {
    let line = r#"{"type":"assistant","timestamp":"2026-06-11T00:00:00.000Z","message":{"model":"claude-opus-4-8","stop_reason":"end_turn","usage":{"input_tokens":2,"cache_creation_input_tokens":4874,"cache_read_input_tokens":72081,"output_tokens":277},"content":[{"type":"text","text":"hi"}]}}"#;
    let parsed = parse_jsonl_line(line);
    // context = input + cache_creation + cache_read = 2 + 4874 + 72081
    assert_eq!(parsed.context_tokens, Some(76_957));
    assert_eq!(parsed.model_id.as_deref(), Some("claude-opus-4-8"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path /home/eben/sessionx/Cargo.toml parse_assistant_captures_context_tokens`
Expected: FAIL — `no field context_tokens on ParsedLine`.

- [ ] **Step 3: Add the fields to `ParsedLine`**

In the `ParsedLine` struct, immediately before its closing `}` (after the `edited_file_paths` field, ~line 583), add:

```rust
    /// Sum of `input_tokens + cache_creation_input_tokens +
    /// cache_read_input_tokens` from this assistant message's `usage`.
    /// Approximates the current context-window fill (the latest message's
    /// value is the live size). None for non-assistant lines or lines
    /// without a usage block.
    pub context_tokens: Option<u64>,
    /// `message.model` from this assistant line, used downstream to map
    /// to a context-window size. None when absent.
    pub model_id: Option<String>,
    /// A clean, render-ready label for the tool action on this line:
    /// the Bash command, or `now <basename>` for a file mutation. None
    /// for read-only / non-action tools and non-assistant lines.
    pub current_action: Option<String>,
    /// The question topic for an `AskUserQuestion` tool_use on this line
    /// (`questions[0].header`, falling back to `questions[0].question`).
    /// None when no such tool is present.
    pub pending_question_text: Option<String>,
```

- [ ] **Step 4: Parse usage + model in `parse_assistant`**

In `parse_assistant`, immediately after the `stop_reason` block (the `if let Some(sr) = ... { out.stop_reason = Some(...) }` ending ~line 688) and before the `let Some(blocks) = ...` binding, insert:

```rust
    if let Some(usage) = v.get("message").and_then(|m| m.get("usage")) {
        let field = |k: &str| usage.get(k).and_then(|n| n.as_u64()).unwrap_or(0);
        out.context_tokens = Some(
            field("input_tokens")
                + field("cache_creation_input_tokens")
                + field("cache_read_input_tokens"),
        );
    }
    if let Some(model) = v
        .get("message")
        .and_then(|m| m.get("model"))
        .and_then(|s| s.as_str())
    {
        out.model_id = Some(model.to_string());
    }
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --manifest-path /home/eben/sessionx/Cargo.toml parse_assistant_captures_context_tokens`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cd /home/eben/sessionx
git add src/activity/events.rs
git commit -m "feat(events): parse context tokens and model id from assistant usage"
```

---

### Task A3: Capture current action + AskUserQuestion topic in `parse_assistant`

**Files:**
- Modify: `/home/eben/sessionx/src/activity/events.rs` (`parse_assistant` tool loop ~721-749 and last-tool block ~762-782; add `file_basename` helper)

- [ ] **Step 1: Write the failing tests**

Add to `mod tests`:

```rust
#[test]
fn parse_assistant_current_action_is_bash_command() {
    let line = r#"{"type":"assistant","timestamp":"2026-06-11T00:00:00.000Z","message":{"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test --lib"}}]}}"#;
    let parsed = parse_jsonl_line(line);
    assert_eq!(parsed.current_action.as_deref(), Some("cargo test --lib"));
}

#[test]
fn parse_assistant_current_action_is_now_basename_for_edit() {
    let line = r#"{"type":"assistant","timestamp":"2026-06-11T00:00:00.000Z","message":{"content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"/abs/src/ui/dashboard/column_content.rs"}}]}}"#;
    let parsed = parse_jsonl_line(line);
    assert_eq!(parsed.current_action.as_deref(), Some("now column_content.rs"));
}

#[test]
fn parse_assistant_no_current_action_for_read() {
    let line = r#"{"type":"assistant","timestamp":"2026-06-11T00:00:00.000Z","message":{"content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/abs/x.rs"}}]}}"#;
    let parsed = parse_jsonl_line(line);
    assert_eq!(parsed.current_action, None);
}

#[test]
fn parse_assistant_captures_ask_user_question_header() {
    let line = r#"{"type":"assistant","timestamp":"2026-06-11T00:00:00.000Z","message":{"content":[{"type":"tool_use","id":"t1","name":"AskUserQuestion","input":{"questions":[{"header":"Auth method","question":"Which auth approach?"}]}}]}}"#;
    let parsed = parse_jsonl_line(line);
    assert_eq!(parsed.pending_question_text.as_deref(), Some("Auth method"));
}

#[test]
fn parse_assistant_ask_user_question_falls_back_to_question() {
    let line = r#"{"type":"assistant","timestamp":"2026-06-11T00:00:00.000Z","message":{"content":[{"type":"tool_use","id":"t1","name":"AskUserQuestion","input":{"questions":[{"question":"Which auth approach?"}]}}]}}"#;
    let parsed = parse_jsonl_line(line);
    assert_eq!(parsed.pending_question_text.as_deref(), Some("Which auth approach?"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path /home/eben/sessionx/Cargo.toml parse_assistant_current_action parse_assistant_captures_ask parse_assistant_ask_user parse_assistant_no_current`
Expected: FAIL — fields unset / always `None`.

- [ ] **Step 3: Add the `file_basename` helper**

Add as a free function in `events.rs` (e.g. directly above `parse_assistant`):

```rust
/// Last path component of `p`, as an owned `String`. Falls back to the
/// whole string when there is no file component.
fn file_basename(p: &str) -> String {
    std::path::Path::new(p)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.to_string())
}
```

- [ ] **Step 4: Capture the AskUserQuestion topic in the tool loop**

In `parse_assistant`'s `for block in blocks` loop, in the `"tool_use" =>` arm, after the existing `if name == "Agent" { ... }` block (~line 732) and before the `if let Some(id) = ...` block, insert:

```rust
                if name == "AskUserQuestion" {
                    out.pending_question_text = input
                        .get("questions")
                        .and_then(|q| q.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|q0| {
                            q0.get("header")
                                .and_then(|h| h.as_str())
                                .or_else(|| q0.get("question").and_then(|q| q.as_str()))
                        })
                        .map(collapse_ws);
                }
```

- [ ] **Step 5: Compute `current_action` from the last tool**

In `parse_assistant`, inside the `if let Some((name, input)) = last_tool {` block (~line 762), add this as the FIRST statement inside the block (before `let body = ...`):

```rust
        out.current_action = match name {
            "Bash" => input
                .get("command")
                .and_then(|c| c.as_str())
                .map(collapse_ws),
            "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => input
                .get("file_path")
                .and_then(|p| p.as_str())
                .map(|p| format!("now {}", file_basename(p))),
            _ => None,
        };
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test --manifest-path /home/eben/sessionx/Cargo.toml parse_assistant_current_action parse_assistant_captures_ask parse_assistant_ask_user parse_assistant_no_current`
Expected: PASS (all 5).

- [ ] **Step 7: Commit**

```bash
cd /home/eben/sessionx
git add src/activity/events.rs
git commit -m "feat(events): capture current tool action and AskUserQuestion topic"
```

---

### Task A4: Aggregate the new fields through `TailUpdate`

**Files:**
- Modify: `/home/eben/sessionx/src/activity/events.rs` (`TailUpdate` struct ~347-401; `tail_session` loop ~539-541)

- [ ] **Step 1: Write the failing test**

Add to `mod tests`:

```rust
#[test]
fn tail_session_takes_latest_context_tokens_and_question_topic() {
    let dir = std::env::temp_dir();
    let path = dir.join("sessionx_tail_latest_ctx.jsonl");
    let l1 = r#"{"type":"assistant","timestamp":"2026-06-11T00:00:00.000Z","message":{"model":"claude-opus-4-8","usage":{"input_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":9},"content":[{"type":"text","text":"a"}]}}"#;
    let l2 = r#"{"type":"assistant","timestamp":"2026-06-11T00:00:01.000Z","message":{"model":"claude-opus-4-8","usage":{"input_tokens":2,"cache_creation_input_tokens":0,"cache_read_input_tokens":98},"content":[{"type":"tool_use","id":"t1","name":"AskUserQuestion","input":{"questions":[{"header":"Auth method"}]}}]}}"#;
    std::fs::write(&path, format!("{l1}\n{l2}\n")).unwrap();

    let update = tail_session(&path, 0).unwrap();
    // latest assistant line wins for context tokens (2 + 98 = 100)
    assert_eq!(update.context_tokens, Some(100));
    assert_eq!(update.model_id.as_deref(), Some("claude-opus-4-8"));
    assert_eq!(update.pending_question_text.as_deref(), Some("Auth method"));
    let _ = std::fs::remove_file(&path);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path /home/eben/sessionx/Cargo.toml tail_session_takes_latest_context`
Expected: FAIL — `no field context_tokens on TailUpdate`.

- [ ] **Step 3: Add the fields to `TailUpdate`**

In the `TailUpdate` struct, before its closing `}` (after `edited_file_paths`, ~line 400):

```rust
    /// Context-window fill from the LAST assistant message in this batch
    /// (later messages overwrite earlier ones). None when no usage seen.
    pub context_tokens: Option<u64>,
    /// Model id from the last assistant message in this batch.
    pub model_id: Option<String>,
    /// Render-ready label for the most recent tool action in this batch.
    pub current_action: Option<String>,
    /// AskUserQuestion topic from the last such tool_use in this batch.
    pub pending_question_text: Option<String>,
```

`TailUpdate` derives `Default`, so no Default change is needed.

- [ ] **Step 4: Aggregate in `tail_session`**

In `tail_session`'s line loop, immediately after the existing `if let Some(text) = parsed.last_assistant_text { update.last_assistant_text = Some(text); }` (~line 539-541), insert:

```rust
        if let Some(t) = parsed.context_tokens {
            update.context_tokens = Some(t);
        }
        if let Some(m) = parsed.model_id {
            update.model_id = Some(m);
        }
        if let Some(a) = parsed.current_action {
            update.current_action = Some(a);
        }
        if let Some(q) = parsed.pending_question_text {
            update.pending_question_text = Some(q);
        }
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --manifest-path /home/eben/sessionx/Cargo.toml tail_session_takes_latest_context`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cd /home/eben/sessionx
git add src/activity/events.rs
git commit -m "feat(events): aggregate new activity signals through TailUpdate"
```

---

### Task A5: Add fields to `WorkspaceEvents` (Default + reset)

**Files:**
- Modify: `/home/eben/sessionx/src/activity/events.rs` (`WorkspaceEvents` struct ~188; `Default` impl ~208; `reset_session_state` ~239)

- [ ] **Step 1: Write the failing test**

Add to `mod tests`:

```rust
#[test]
fn reset_clears_new_activity_fields() {
    let mut e = WorkspaceEvents {
        context_tokens: Some(123),
        model_id: Some("claude-opus-4-8".to_string()),
        current_action: Some("now x.rs".to_string()),
        pending_question_text: Some("Auth method".to_string()),
        ..WorkspaceEvents::default()
    };
    e.reset_session_state();
    assert_eq!(e.context_tokens, None);
    assert_eq!(e.model_id, None);
    assert_eq!(e.current_action, None);
    assert_eq!(e.pending_question_text, None);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path /home/eben/sessionx/Cargo.toml reset_clears_new_activity`
Expected: FAIL — `no field context_tokens on WorkspaceEvents`.

- [ ] **Step 3: Add the fields to `WorkspaceEvents`**

In the `WorkspaceEvents` struct, before its closing `}` (after `last_completed_turn_text`, ~line 188):

```rust
    /// Latest assistant message's context-window fill (input + cache
    /// creation + cache read). Drives the detail bar's context line.
    /// Cleared on session reset.
    pub context_tokens: Option<u64>,
    /// Latest assistant message's model id, for context-window sizing.
    /// Cleared on session reset.
    pub model_id: Option<String>,
    /// Render-ready label for the agent's most recent tool action
    /// (Bash command or `now <basename>`). Drives the row's live edge
    /// in Thinking/Waiting. Cleared on session reset.
    pub current_action: Option<String>,
    /// Topic of the pending `AskUserQuestion`, if one is in flight.
    /// Drives the row's `asking: <topic>` in the Question state.
    /// Cleared on session reset and when no question tool is pending.
    pub pending_question_text: Option<String>,
```

- [ ] **Step 4: Add to the `Default` impl**

In `impl Default for WorkspaceEvents`, before the closing `}` of the `Self { ... }` literal (after `last_completed_turn_text: None,`, ~line 208):

```rust
            context_tokens: None,
            model_id: None,
            current_action: None,
            pending_question_text: None,
```

- [ ] **Step 5: Add to `reset_session_state`**

In `reset_session_state`, before its closing `}` (after `self.last_completed_turn_text = None;`, ~line 239):

```rust
        self.context_tokens = None;
        self.model_id = None;
        self.current_action = None;
        self.pending_question_text = None;
```

- [ ] **Step 6: Run the full sessionx test suite**

Run: `cargo test --manifest-path /home/eben/sessionx/Cargo.toml`
Expected: PASS (all tests, including the new ones).

- [ ] **Step 7: Format check and commit**

```bash
cargo fmt --manifest-path /home/eben/sessionx/Cargo.toml --check
cd /home/eben/sessionx
git add src/activity/events.rs
git commit -m "feat(events): add context/action/question fields to WorkspaceEvents"
```

---

### Task A6: Push sessionx and open its PR

**Files:** none

- [ ] **Step 1: Push and open the PR**

```bash
cd /home/eben/sessionx
git push -u origin live-edge-activity-signals
gh pr create --title "Capture live-edge activity signals (context tokens, current action, question topic)" \
  --body "Adds context_tokens, model_id, current_action, and pending_question_text to WorkspaceEvents, parsed from the session JSONL. Consumed by wsx for live-edge rows and a context-fill detail line. Paired wsx PR will be cross-linked."
```

- [ ] **Step 2: Record the head commit sha** (needed for the wsx `rev` bump in Task B7)

Run: `git -C /home/eben/sessionx rev-parse HEAD`
Note the sha for Task B7.

---

# PHASE B — wsx (this repo + PR)

### Task B1: Point wsx at local sessionx for development

**Files:**
- Modify: `Cargo.toml` (add a `[patch]` section)

- [ ] **Step 1: Add a patch override**

Append to `Cargo.toml`:

```toml
[patch."https://github.com/bakedbean/sessionx"]
sessionx = { path = "/home/eben/sessionx" }
```

- [ ] **Step 2: Build against the local sessionx**

Run: `cargo build`
Expected: PASS — wsx now compiles against the branch with the new fields.

- [ ] **Step 3: Commit the temporary patch**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore(deps): temporarily patch sessionx to local checkout for dev"
```

---

### Task B2: Merge the new fields onto `WorkspaceEvents`

**Files:**
- Modify: `src/app/background.rs` (destructure ~88-102; merge block ~137-211)

- [ ] **Step 1: Destructure the new `TailUpdate` fields**

In the `let crate::activity::events::TailUpdate { ... } = update;` destructure (~88-102), add the four new fields to the field list (e.g. after `edited_file_paths,`):

```rust
        context_tokens,
        model_id,
        current_action,
        pending_question_text,
```

- [ ] **Step 2: Merge them onto `evt`**

In the merge block, after the recent-files loop `for path in edited_file_paths { evt.push_recent_edited_file(path); }` (~line 202-203), insert:

```rust
        if let Some(t) = context_tokens {
            evt.context_tokens = Some(t);
        }
        if let Some(m) = model_id {
            evt.model_id = Some(m);
        }
        if let Some(a) = current_action {
            evt.current_action = Some(a);
        }
        // Question topic: adopt a freshly-seen one, then clear it once the
        // question is no longer pending. pending_tool_uses is already
        // maintained above, so pending_question_tool() reflects this batch.
        if let Some(q) = pending_question_text {
            evt.pending_question_text = Some(q);
        }
        if evt.pending_question_tool().is_none() {
            evt.pending_question_text = None;
        }
```

- [ ] **Step 3: Build to verify the merge compiles**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/app/background.rs
git commit -m "feat(background): merge live-edge activity fields onto WorkspaceEvents"
```

---

### Task B3: Enrich the `Question` arm of `row_column`

**Files:**
- Modify: `src/ui/dashboard/column_content.rs` (`Question` arm ~40-53; tests ~218-256)

- [ ] **Step 1: Update the affected existing tests + add new ones**

Replace the bodies of the existing `question_surfaces_pending_question_tool_with_status_emphasis`, `question_surfaces_exit_plan_mode_tool`, and `question_falls_back_to_permission_tool` tests, and add two new tests, so the `Question`-arm tests read:

```rust
    #[test]
    fn question_with_topic_renders_asking_topic() {
        let mut e = evt();
        e.pending_tool_uses
            .insert("tu_q".into(), ("AskUserQuestion".into(), 0));
        e.pending_question_text = Some("Auth method".into());
        let c = row_column(Status::Question, Some(&e), 10_000).unwrap();
        assert_eq!(c.text, "asking: Auth method");
        assert_eq!(c.emphasis, ColumnEmphasis::Status);
    }

    #[test]
    fn question_without_topic_renders_asking_ellipsis() {
        let mut e = evt();
        e.pending_tool_uses
            .insert("tu_q".into(), ("AskUserQuestion".into(), 0));
        let c = row_column(Status::Question, Some(&e), 10_000).unwrap();
        assert_eq!(c.text, "asking…");
        assert_eq!(c.emphasis, ColumnEmphasis::Status);
    }

    #[test]
    fn question_exit_plan_mode_renders_review_plan() {
        let mut e = evt();
        e.pending_tool_uses
            .insert("tu_p".into(), ("ExitPlanMode".into(), 0));
        let c = row_column(Status::Question, Some(&e), 10_000).unwrap();
        assert_eq!(c.text, "asking: review plan");
        assert_eq!(c.emphasis, ColumnEmphasis::Status);
    }

    #[test]
    fn question_permission_tool_renders_awaiting_tool() {
        let mut pending = HashMap::new();
        // epoch-0 timestamp guarantees age > the 3s stale threshold.
        pending.insert("tu_b".to_string(), ("Bash".to_string(), 0_i64));
        let e = WorkspaceEvents {
            pending_tool_uses: pending,
            ..WorkspaceEvents::default()
        };
        let c = row_column(Status::Question, Some(&e), 10_000).unwrap();
        assert_eq!(c.text, "awaiting: Bash");
        assert_eq!(c.emphasis, ColumnEmphasis::Status);
    }
```

(The existing `question_with_no_pending_tool_uses_bare_label` test — expecting `"question"` — stays unchanged.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib question_with_topic question_without_topic question_exit_plan question_permission_tool`
Expected: FAIL — current arm produces the bare tool name.

- [ ] **Step 3: Rewrite the `Question` arm**

Replace the `Status::Question => { ... }` arm body in `row_column` with:

```rust
        Status::Question => {
            let body = match evt.pending_question_tool() {
                Some("ExitPlanMode") => "asking: review plan".to_string(),
                Some(_) => match evt.pending_question_text.as_deref() {
                    Some(t) if !t.trim().is_empty() => format!("asking: {}", collapse_ws(t)),
                    _ => "asking…".to_string(),
                },
                None => evt
                    .pending_permission_tool(now_ms, 3_000)
                    .map(|(n, _)| format!("awaiting: {n}"))
                    .unwrap_or_else(|| "question".to_string()),
            };
            Some(RowColumn {
                text: body,
                emphasis: ColumnEmphasis::Status,
            })
        }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib question_with_topic question_without_topic question_exit_plan question_permission_tool question_with_no_pending`
Expected: PASS (all 5).

- [ ] **Step 5: Commit**

```bash
git add src/ui/dashboard/column_content.rs
git commit -m "feat(row): show the live question topic in the Question state"
```

---

### Task B4: Enrich the `Thinking`/`Waiting` arm with the live item

**Files:**
- Modify: `src/ui/dashboard/column_content.rs` (`Thinking | Waiting` arm ~58-69; tests)

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` (the existing `thinking_shows_tool_trace_dim` and `*_ellipsis_label` tests stay unchanged — they assert the no-`current_action` behavior, which is preserved):

```rust
    #[test]
    fn thinking_appends_current_action_to_trace() {
        let mut e = evt();
        e.tool_use_counts.edit = 3;
        e.current_action = Some("now column_content.rs".into());
        let c = row_column(Status::Thinking, Some(&e), 0).unwrap();
        assert_eq!(c.text, "edited 3 files · now column_content.rs");
        assert_eq!(c.emphasis, ColumnEmphasis::Dim);
    }

    #[test]
    fn thinking_appends_bash_command_to_trace() {
        let mut e = evt();
        e.tool_use_counts.bash = 5;
        e.current_action = Some("cargo test --lib".into());
        let c = row_column(Status::Thinking, Some(&e), 0).unwrap();
        assert_eq!(c.text, "ran 5 commands · cargo test --lib");
    }

    #[test]
    fn thinking_shows_action_alone_when_no_counts_yet() {
        let mut e = evt();
        e.current_action = Some("now column_content.rs".into());
        let c = row_column(Status::Thinking, Some(&e), 0).unwrap();
        assert_eq!(c.text, "now column_content.rs");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib thinking_appends thinking_shows_action_alone`
Expected: FAIL — the arm ignores `current_action`.

- [ ] **Step 3: Rewrite the `Thinking | Waiting` arm**

Replace the `Status::Thinking | Status::Waiting => { ... }` arm body in `row_column` with:

```rust
        Status::Thinking | Status::Waiting => {
            let trace = format_tool_trace(&evt.tool_use_counts);
            let live = evt
                .current_action
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let text = match (trace.is_empty(), live) {
                (false, Some(l)) => format!("{trace} · {l}"),
                (false, None) => trace,
                (true, Some(l)) => l.to_string(),
                (true, None) => format!("{}…", status.label()),
            };
            Some(RowColumn {
                text,
                emphasis: ColumnEmphasis::Dim,
            })
        }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib thinking_ waiting_`
Expected: PASS (new + existing thinking/waiting tests).

- [ ] **Step 5: Commit**

```bash
git add src/ui/dashboard/column_content.rs
git commit -m "feat(row): append the live tool action to the activity trace"
```

---

### Task B5: Token-formatting + window-resolution helpers

**Files:**
- Modify: `src/detail_modules/session_summary.rs` (add free functions + tests)

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block in `session_summary.rs`:

```rust
    #[test]
    fn abbreviate_tokens_uses_k_and_m() {
        assert_eq!(abbreviate_tokens(950), "950");
        assert_eq!(abbreviate_tokens(77_081), "77k");
        assert_eq!(abbreviate_tokens(200_000), "200k");
        assert_eq!(abbreviate_tokens(1_000_000), "1M");
        assert_eq!(abbreviate_tokens(1_250_000), "1.2M");
    }

    #[test]
    fn resolve_window_maps_known_models_and_upgrades_past_default() {
        assert_eq!(resolve_window(50_000, Some("claude-opus-4-8")), Some(200_000));
        // current fill above the 200k default → treat as the 1M variant
        assert_eq!(resolve_window(250_000, Some("claude-opus-4-8")), Some(1_000_000));
        assert_eq!(resolve_window(50_000, Some("some-unknown-model")), None);
        assert_eq!(resolve_window(50_000, None), None);
    }

    #[test]
    fn format_context_line_known_window_shows_percent() {
        let evt = WorkspaceEvents {
            context_tokens: Some(100_000),
            model_id: Some("claude-opus-4-8".to_string()),
            ..WorkspaceEvents::default()
        };
        let (text, warn) = format_context_line(&evt).unwrap();
        assert_eq!(text, "context: 100k / 200k · 50%");
        assert!(!warn);
    }

    #[test]
    fn format_context_line_warns_near_limit() {
        let evt = WorkspaceEvents {
            context_tokens: Some(190_000),
            model_id: Some("claude-opus-4-8".to_string()),
            ..WorkspaceEvents::default()
        };
        let (_text, warn) = format_context_line(&evt).unwrap();
        assert!(warn, "expected warn at 95% fill");
    }

    #[test]
    fn format_context_line_unknown_window_shows_raw_tokens() {
        let evt = WorkspaceEvents {
            context_tokens: Some(77_000),
            model_id: None,
            ..WorkspaceEvents::default()
        };
        let (text, warn) = format_context_line(&evt).unwrap();
        assert_eq!(text, "context: 77k tokens");
        assert!(!warn);
    }

    #[test]
    fn format_context_line_none_when_no_tokens() {
        let evt = WorkspaceEvents::default();
        assert!(format_context_line(&evt).is_none());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib abbreviate_tokens resolve_window format_context_line`
Expected: FAIL — functions don't exist.

- [ ] **Step 3: Add the helpers**

Add these free functions to `session_summary.rs` (e.g. directly above `fn format_recent_files`). Add `use crate::activity::events::WorkspaceEvents;` to the file's imports if not already present:

```rust
/// Abbreviate a token count as `950` / `77k` / `1M` / `1.2M`.
fn abbreviate_tokens(n: u64) -> String {
    if n < 1_000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{}k", n / 1_000)
    } else {
        let m = n as f64 / 1_000_000.0;
        if (m - m.round()).abs() < 0.05 {
            format!("{}M", m.round() as u64)
        } else {
            format!("{m:.1}M")
        }
    }
}

/// Resolve the context-window size for a model id. Known families default
/// to 200k; if the current fill already exceeds that, treat the session as
/// the 1M variant (the model id doesn't encode the variant). Unknown or
/// absent model → None (render raw tokens without a percentage).
fn resolve_window(context_tokens: u64, model_id: Option<&str>) -> Option<u64> {
    let base = model_id.and_then(|m| {
        if m.contains("opus") || m.contains("sonnet") || m.contains("haiku") {
            Some(200_000u64)
        } else {
            None
        }
    })?;
    Some(if context_tokens > base { 1_000_000 } else { base })
}

/// Build the detail bar's context-fill line and whether it should render in
/// the warn color. None when there's no token data yet (omit the line).
fn format_context_line(evt: &WorkspaceEvents) -> Option<(String, bool)> {
    let n = evt.context_tokens?;
    match resolve_window(n, evt.model_id.as_deref()) {
        Some(w) => {
            let pct = (n.saturating_mul(100) / w).min(999);
            let text = format!(
                "context: {} / {} · {}%",
                abbreviate_tokens(n),
                abbreviate_tokens(w),
                pct
            );
            Some((text, pct >= 85))
        }
        None => Some((format!("context: {} tokens", abbreviate_tokens(n)), n >= 150_000)),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib abbreviate_tokens resolve_window format_context_line`
Expected: PASS (all 6).

- [ ] **Step 5: Commit**

```bash
git add src/detail_modules/session_summary.rs
git commit -m "feat(detail): add token abbreviation and context-window helpers"
```

---

### Task B6: Render the context-fill line in the detail bar

**Files:**
- Modify: `src/detail_modules/session_summary.rs` (`build_lines`, inside the `Some(evt)` arm ~110-115; tests)

- [ ] **Step 1: Write the failing tests**

Add to `mod tests`:

```rust
    #[test]
    fn render_shows_context_fill_line() {
        let evt: &'static WorkspaceEvents = Box::leak(Box::new(WorkspaceEvents {
            context_tokens: Some(100_000),
            model_id: Some("claude-opus-4-8".to_string()),
            ..WorkspaceEvents::default()
        }));
        let mut ctx = stub_context();
        ctx.events = Some(evt);
        ctx.events_scanned = true;
        ctx.status = Status::Thinking;

        let text = render_to_text(&ctx, 60, 12);
        assert!(text.contains("context:"), "missing context line:\n{text}");
        assert!(text.contains("100k"), "missing token count:\n{text}");
    }

    #[test]
    fn render_omits_context_line_when_no_tokens() {
        let evt: &'static WorkspaceEvents = Box::leak(Box::new(WorkspaceEvents::default()));
        let mut ctx = stub_context();
        ctx.events = Some(evt);
        ctx.events_scanned = true;

        let text = render_to_text(&ctx, 60, 12);
        assert!(
            !text.contains("context:"),
            "expected no context line without token data:\n{text}"
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib render_shows_context_fill render_omits_context_line`
Expected: FAIL — no context line rendered.

- [ ] **Step 3: Render the line**

In `build_lines`, inside the `Some(evt) => { ... }` arm, after the recent-files block (the `if let Some(files_text) = format_recent_files(...) { ... }` ending ~line 115) and before the arm's closing `}`, insert:

```rust
            // Context-window fill: a live signal the row never shows.
            if let Some((ctx_text, warn)) = format_context_line(evt) {
                let style = if warn {
                    theme.warn_style()
                } else {
                    theme.dim_style()
                };
                out.push(Line::from(vec![
                    prefix.clone(),
                    Span::styled(truncate_to_chars(&ctx_text, inner_width), style),
                ]));
            }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib render_shows_context_fill render_omits_context_line`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/detail_modules/session_summary.rs
git commit -m "feat(detail): render a live context-window fill line"
```

---

### Task B7: Swap the patch for the merged sessionx `rev`

**Files:**
- Modify: `Cargo.toml` (remove `[patch]`, bump `rev`), `Cargo.lock`

> Prerequisite: the sessionx PR (Task A6) is merged. Use the merged commit sha (from `gh pr view` on the sessionx PR, or `git -C /home/eben/sessionx rev-parse origin/main` after merge).

- [ ] **Step 1: Remove the dev patch and update the rev**

Delete the `[patch."https://github.com/bakedbean/sessionx"]` section added in Task B1, and update `Cargo.toml:31` to the merged sha:

```toml
sessionx = { git = "https://github.com/bakedbean/sessionx", rev = "<MERGED_SHA>" }
```

- [ ] **Step 2: Refresh the lockfile against the real rev**

Run: `cargo update -p sessionx --precise <MERGED_SHA> && cargo build`
Expected: PASS — wsx builds against the merged sessionx.

- [ ] **Step 3: Full verification (fmt + tests + clippy)**

Run:
```bash
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```
Expected: all PASS. (wsx CI gates on rustfmt — `cargo fmt --check` must be clean.)

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore(deps): bump sessionx to merged live-edge signals rev"
```

---

### Task B8: Open the wsx PR and cross-link

**Files:** none

- [ ] **Step 1: Open the PR**

```bash
git push -u origin HEAD
gh pr create --title "Live-edge workspace rows + context-window fill in the detail bar" \
  --body "Splits the row and detail bar by altitude: the row flex column now shows the live edge (question topic, current file/command) while the detail bar gains a live context-window fill line. Depends on sessionx PR <link>. Merge sessionx first."
```

- [ ] **Step 2: Cross-link both PRs** — edit the sessionx PR body to point at the wsx PR, and confirm the wsx PR body links the sessionx PR. Merge order: sessionx → wsx.

---

## Self-review notes (verification against the spec)

- **Row Question topic** → Task B3 (+ sessionx A3 capture). ✓
- **Row Thinking/Waiting full trace + live item** → Task B4 (+ sessionx A3 `current_action`). Note: count style is the existing full `format_tool_trace` per the user's "full trace + live item" choice. ✓
- **Complete/Idle/Stalled unchanged** → those arms are untouched in B3/B4. ✓
- **Detail context-fill line, cache math (input+creation+read, latest only)** → sessionx A2 (`context_tokens` sum, latest-wins in A4) + wsx B5/B6. ✓
- **Window % with heuristic 1M upgrade** → `resolve_window` (B5). Implemented as "current fill exceeds default" rather than the spec's "ever exceeded"; functionally equivalent since only the latest fill is displayed. ✓
- **Warn coloring** → `format_context_line` returns a warn flag; B6 maps it to `theme.warn_style()`. ✓
- **Detail content otherwise unchanged** → B6 only appends a line. ✓
- **Two-repo rollout, sessionx first** → Phase A then B; dev via `[patch]` (B1), finalized via `rev` bump (B7). ✓
- **No new persisted state / DB** → all new data rides `WorkspaceEvents` in memory. ✓
