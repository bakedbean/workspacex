# Question-vs-Complete Attention Detection

## Background

wsx's dashboard surfaces a generic `!` marker and a single terminal bell whenever a workspace's Claude Code session reaches an "alertable" state — currently any of `Awaiting` (permission prompt), `Stopped` (turn ended), or `Stalled` (no jsonl activity for ≥60s). Users cannot tell from the dashboard whether a workspace is waiting for an answer to a question or has finished a task.

The Claudette desktop IDE achieves this distinction because it runs the agent in-process via the Pi SDK and watches `ContentBlockStart` events for the `AskUserQuestion` and `ExitPlanMode` tools. When either tool fires mid-turn, Claudette marks the session "Ask" or "Plan" and plays the corresponding sound; when a turn completes without firing those tools, it plays the "Finished" sound. The classifier is purely deterministic — no LLM call, no text heuristics.

wsx does not run the agent in-process, but it already tails Claude Code's session jsonl (`src/events.rs:258-305`) and extracts `tool_use` content blocks. The same signal Claudette reads from the SDK stream is available to wsx through the jsonl. We can match Claudette's capability without changing how wsx integrates with Claude Code.

## Goal

Split wsx's coarse `Stopped` state into two distinct states — `AwaitingAnswer` (Claude has asked the user something) and `Complete` (Claude has finished a task) — and surface that distinction in the dashboard glyphs, the summary line counts, and the terminal-bell pattern.

## Non-goals

- Replacing the existing `Awaiting` permission-prompt state. That state remains as-is.
- OS-level notifications (macOS Notification Center, etc). Out of scope; bell-only.
- LLM-based classification. The deterministic tool signal plus one conservative text heuristic gives enough recall without the cost.
- Multi-platform sound files. The terminal bell maps to the user's terminal-app sound configuration.

## Approach

**Tool-first detection with a single text fallback.** Scan the most recent assistant turn that triggered `stop_reason` for unresolved `AskUserQuestion` or `ExitPlanMode` tool_use blocks; if either is present, classify as `AwaitingAnswer`. If neither tool fired, examine the final sentence of the last text block — if it ends with `?` (after trimming trailing whitespace and markdown noise), classify as `AwaitingAnswer { TrailingQuestionMark }`. Otherwise, classify as `Complete`.

## State model

`src/app.rs:76-113` currently defines:

```rust
pub enum ActivityState {
    Active,
    Idle,
    Awaiting,        // tool_use pending ≥3s (permission prompt)
    Stopped,         // stop_reason = end_turn | max_tokens | stop_sequence
    Stalled,         // no jsonl activity ≥60s, prior stop_reason exists
}
```

After this change:

```rust
pub enum ActivityState {
    Active,
    Idle,
    Awaiting,        // unchanged — tool_use pending ≥3s (permission prompt)
    AwaitingAnswer,  // NEW — AskUserQuestion/ExitPlanMode unresolved, OR final sentence ends with '?'
    Complete,        // NEW — stop_reason set, no unresolved question tools, no trailing '?'
    Stalled,         // unchanged
}
```

`is_alertable()` returns true for `Awaiting | AwaitingAnswer | Complete | Stalled`.

Introducing two new enum variants instead of a sub-kind on `Stopped` forces every `match` site in the codebase to acknowledge the new states at compile time — the dashboard renderer, the summary-line counter, the bell-firing helper, and any downstream consumers all surface as compile errors until handled. This is the desired safety net.

## Classification logic

The classification splits across two layers: **primitives on `WorkspaceEvents`** (in `src/events.rs`) and a **derivation helper in `src/app.rs`** that combines them with the existing `is_awaiting_user()` and `pending_tool_uses` signals.

**Primitives on `WorkspaceEvents`** (added to the existing struct, not to `TailUpdate`):

```rust
// New field on WorkspaceEvents — populated by parse_assistant whenever
// an assistant message has a text content block. Carried through
// TailUpdate.last_assistant_text and stored on WorkspaceEvents.
pub last_assistant_text: Option<String>,

// New methods on WorkspaceEvents:
pub fn pending_question_tool(&self) -> Option<&str>;        // filters pending_tool_uses for AskUserQuestion / ExitPlanMode
pub fn last_text_ends_with_question(&self) -> bool;         // trims trailing whitespace + `*` `_` `` ` ``, then checks last char
```

This keeps the existing `TailUpdate` shape unchanged. No `TurnOutcome` or `AnswerReason` enums — the primitives are simple enough that the derivation lives in one place (in `app.rs`) and consumers read state directly off `WorkspaceEvents`.

**Derivation in `src/app.rs`** (`fn derive_stopped_kind(e: &WorkspaceEvents) -> Option<StoppedKind>`):

```rust
fn derive_stopped_kind(e: &WorkspaceEvents) -> Option<StoppedKind> {
    // Question tools fire even mid-turn — the model has explicitly
    // asked the user something. stop_reason at this point is ToolUse,
    // so is_awaiting_user() is false. Short-circuit BEFORE that gate.
    if e.pending_question_tool().is_some() {
        return Some(StoppedKind::AwaitingAnswer);
    }
    if !e.is_awaiting_user() {
        return None;
    }
    if e.last_text_ends_with_question() {
        Some(StoppedKind::AwaitingAnswer)
    } else {
        Some(StoppedKind::Complete)
    }
}
```

**Coordinated change in `App::awaiting_permission()`:** the existing permission-prompt detector iterates `pending_tool_uses` for entries older than 3 seconds. To prevent AskUserQuestion / ExitPlanMode from being misclassified as permission prompts (they live in the same map), the detector now skips entries with those tool names.

**Effective priority** in `classify_activity_with_events`:

1. `Awaiting` — permission-eligible pending tool ≥3s (excludes question tools)
2. `AwaitingAnswer` — from `derive_stopped_kind`: question tool pending OR end-of-turn with trailing `?`
3. `Complete` — from `derive_stopped_kind`: end-of-turn with no question signal
4. `Stalled` — JSONL quiet >60s mid-tool-chain
5. PTY recency states

**Edge cases covered:**

- *Mid-turn AskUserQuestion:* `stop_reason` is `tool_use`, so `is_awaiting_user()` returns false — but the question-tool short-circuit fires first → `AwaitingAnswer`. ✓
- *Resolved question tools:* once a `tool_result` arrives, the tool_use is removed from `pending_tool_uses`. The next stop_reason-bearing turn becomes the candidate (`is_awaiting_user()` gate).
- *End-of-turn complete:* `pending_tool_uses` empty, `is_awaiting_user()` true, text doesn't end with `?` → `Complete`. ✓
- *End-of-turn question (text-based fallback):* same as above but text ends with `?` → `AwaitingAnswer`. ✓
- *Code blocks:* `last_assistant_text` captures the last *text* content block, not code blocks. Trailing code never trips the heuristic.
- *Trailing markdown noise:* e.g., ``Want me to refactor `foo`?* `` ends with `*` literally; the trim_end_matches step strips `*` `_` `` ` `` before checking the final char.
- *Stop reason `tool_use` for non-question tools:* routes to `Awaiting` (permission flow). Unchanged.

## Bell patterns

Refactor the bell-firing code at `src/app.rs:572-582` into a `fire_bell(state)` helper.

| State | Pattern | Notes |
|---|---|---|
| `AwaitingAnswer` | `\x07 \x07` (two bells, ~120ms apart) | "I need you" — distinctive double-ping |
| `Complete` | `\x07` (single bell) | Familiar "done" chime |
| `Awaiting` (permission) | `\x07` (single bell) | Unchanged — existing behavior |
| `Stalled` | `\x07 \x07 \x07` (three bells) | Rare + alarming |

Spacing is implemented via a detached thread that performs the writes and sleeps between them, so the engine event loop is not blocked. One thread per bell sequence; each exits within ~500ms.

## Dashboard glyphs

The leftmost marker column at `src/ui/dashboard/mod.rs:492`:

| State | Nerd-font glyph | ASCII fallback | Color |
|---|---|---|---|
| `AwaitingAnswer` | `` (question circle) | `?` | warn |
| `Complete` | `` (check circle) | `✓` | success (green) |
| `Awaiting` (permission) | `` (warning triangle) | `!` | warn |
| `Stalled` | `` (info alt) | `!` | warn |

Reuses the existing nerd-font detection helper already used for branch lifecycle glyphs in `dashboard/mod.rs`. The marker column likely needs to widen from 1 to 2 cells to accommodate double-width nerd-font glyphs; verify against the existing column-width math before assuming this is a one-line change.

**Top summary line** in `dashboard/mod.rs`: add a `complete` count and rename `stopped` → `question`. New format:

```
12 workspaces · 1 question · 2 complete · 1 permission · 0 stalled
```

Each count is styled to match its row glyph color.

## Configuration

Settings live in the existing `store.get_setting(key)` table (sqlite-backed key/value store), read by `bell_pattern_for` near the existing `notifications_enabled` reader. Per-state bell pattern overrides:

| Key | Default | Accepts |
|---|---|---|
| `notification_bell_question` | `double` | `off` \| `single` \| `double` \| `triple` |
| `notification_bell_complete` | `single` | `off` \| `single` \| `double` \| `triple` |
| `notification_bell_permission` | `single` | `off` \| `single` \| `double` \| `triple` |
| `notification_bell_stalled` | `triple` | `off` \| `single` \| `double` \| `triple` |

The existing `notifications` setting (default on) still gates whether any bell fires. Unset keys use the defaults above; no config migration needed.

Nerd-font / ASCII glyph selection reuses the existing `nerd_fonts_enabled(&store)` helper; no new setting was added for `glyph_style`.

## Backward compatibility

- Older Claude Code versions that predate `AskUserQuestion` silently fall through to the trailing-`?` heuristic, then to `Complete`. No errors, no degraded behavior; these turns simply land on the new `Complete` glyph and single-bell pattern.
- The old `Stopped` variant was never persisted to disk, so no user config or state on disk is invalidated.
- The change adds enum variants and a struct field; nothing is removed. External consumers (none today) would continue to compile.

## Testing

**Unit tests** in `events.rs` covering each classification rule. Each test parses a fixture jsonl line through the existing `parse_jsonl_line` path and asserts the resulting `TurnOutcome`:

1. Turn with `AskUserQuestion` tool_use + no matching tool_result → `AwaitingAnswer { AskUserQuestionTool }`.
2. Turn with `ExitPlanMode` tool_use + no matching tool_result → `AwaitingAnswer { ExitPlanModeTool }`.
3. Turn with `AskUserQuestion` tool_use + matching tool_result → not classified (turn isn't current).
4. Turn ending in `"Done."` → `Complete`.
5. Turn ending in `"Want me to also handle X?"` → `AwaitingAnswer { TrailingQuestionMark }`.
6. Turn ending in a real code block (`assert(x == 1);` inside triple-backtick fence) → `Complete`. This is the load-bearing test for the text heuristic; if it breaks, the whole approach unwinds.
7. Turn ending in `` "Want me to refactor `foo`?*" `` (trailing markdown noise) → `AwaitingAnswer`.
8. Turn with `?` in the middle but a plain declarative final sentence → `Complete` (final-sentence scoping holds).
9. Turn with only tool_use blocks, no text → handled by the tool path.
10. Turn with `stop_reason: tool_use` → routes to existing `Awaiting`, unchanged.

**Integration tests:**

- One end-to-end test that feeds a synthetic multi-turn jsonl through `tail_session` and asserts `WorkspaceEvents.activity` transitions correctly across turns (e.g., `Active → AwaitingAnswer → Active → Complete`).
- One test that `fire_bell(AwaitingAnswer)` emits the expected byte sequence without panicking on the detached thread.

**Out of scope:**

- Whether the terminal actually rings (terminal-app config dependent).
- Whether nerd-font glyphs render correctly (font dependent).
- Visual layout assertions on the dashboard (existing renderer tests cover that).

## Files touched

- `src/events.rs` — add `TurnOutcome`, `AnswerReason`; extend `parse_jsonl_line` / `tail_session` to classify; reuse existing pending-tool tracker.
- `src/app.rs` — change `ActivityState` enum; refactor bell-firing into `fire_bell(state)`; spawn detached threads for multi-bell sequences.
- `src/ui/dashboard/mod.rs` — update marker-column glyph selection; widen column if needed; update top summary line counts and labels.
- `src/store.rs` (only if it references `ActivityState::Stopped` directly).
- Config-loading site near `app.rs:842` — add new notification settings with defaults.
- `tests/fixtures/` — new jsonl fixtures for classifier unit tests.

## Risk + open questions

- **Marker column width:** verify the existing dashboard layout math handles double-width nerd-font glyphs before assuming a one-line change. If the column has hardcoded 1-cell width, the layout downstream may shift.
- **Detached threads for bells:** if the user has hundreds of workspaces all transitioning at once, we'd spawn many short-lived threads. Cheap, but worth confirming no thread-pool starvation pattern exists in the engine.
- **`ExitPlanMode` tool name stability:** verify the exact tool name string used by Claude Code today. The Claudette code matches `ExitPlanMode`; we should confirm the same casing in current jsonl output.
