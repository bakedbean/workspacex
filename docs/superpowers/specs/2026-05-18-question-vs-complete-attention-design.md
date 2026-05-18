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

Lives in `src/events.rs`, extending `parse_jsonl_line` (~lines 327-457) and the `TailUpdate` struct returned to `app.rs`.

**New types on `TailUpdate`:**

```rust
pub enum TurnOutcome {
    AwaitingAnswer { reason: AnswerReason },
    Complete,
    // None of these when stop_reason isn't set yet — turn is still in flight.
}

pub enum AnswerReason {
    AskUserQuestionTool,   // model invoked AskUserQuestion, no tool_result yet
    ExitPlanModeTool,      // model invoked ExitPlanMode, no tool_result yet
    TrailingQuestionMark,  // fallback — final text sentence ends with '?'
}
```

**Rules**, evaluated per assistant turn that has just produced a `stop_reason` of `end_turn | max_tokens | stop_sequence`:

1. Scan the turn's content blocks for `tool_use` entries with `name == "AskUserQuestion"` or `name == "ExitPlanMode"`.
2. For each such tool_use, check the existing pending-tool tracker in `events.rs` (already used for permission detection). If no matching `tool_result` exists in subsequent messages, classify as `AwaitingAnswer` with the matching reason and return.
3. Otherwise, take the *last* text content block of the turn. Strip trailing whitespace and trailing markdown noise (`*`, `_`, `` ` ``, closing code fences). Locate the final sentence boundary by splitting on `.!?` followed by whitespace or end-of-string. If the final sentence's last non-whitespace character is `?`, classify as `AwaitingAnswer { TrailingQuestionMark }`.
4. Otherwise, classify as `Complete`.

**Edge cases covered by the rules:**

- *Multi-turn:* only the most recent stop_reason-bearing turn is classified. Earlier resolved AskUserQuestion calls don't matter.
- *Code blocks:* the trailing-`?` check inspects the final sentence of the last *text* block, not code blocks, so trailing code like `assert(x == 1);` cannot trigger a false question.
- *Resolved question tools:* if a `tool_result` arrived, the agent will have continued — the next stop_reason-bearing turn becomes the candidate for classification.
- *Stop reason `tool_use`:* this is the permission-flow path, routed to existing `Awaiting`, not touched.
- *Empty text block + tool_use only:* the tool path resolves it; the text fallback never runs.
- *Trailing markdown noise:* e.g., ``Want me to refactor `foo`?* `` ends with `*` literally; the strip step removes `*` before the final-char check, so the `?` is detected.

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

New settings, added wherever wsx reads config today (near `notifications_enabled` at `app.rs:842`):

```toml
[notifications]
enabled = true                    # existing
question_bell = "double"          # "single" | "double" | "off"
complete_bell = "single"          # "single" | "double" | "off"
permission_bell = "single"        # "single" | "double" | "off"
stalled_bell = "triple"           # "single" | "double" | "triple" | "off"
glyph_style = "auto"              # "nerd" | "ascii" | "auto"
```

All have defaults that preserve current behavior. No config migration needed.

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
