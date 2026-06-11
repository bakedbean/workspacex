# Live-edge workspace rows + context-token usage in the detail bar

Date: 2026-06-11
Status: Approved (design) — ready for implementation planning
Repos touched: `sessionx` (github.com/bakedbean/sessionx) and `wsx`

## Problem

The dashboard renders the same per-workspace state in two valuable display areas:
the workspace **row** and the workspace **detail bar**. Both currently sit at the
same altitude — a "session summary" — so the row is effectively a truncated copy
of the detail bar. When an agent asks a question, both show essentially the same
thing. This wastes two scarce display surfaces on duplicate content, and the row's
flex column (`column_content.rs::row_column`) has free space it isn't earning,
especially in the `Question` state (which shows only a bare tool name).

## Goal

Make the row and the detail bar answer **different questions** by splitting them by
altitude, and add a genuinely new, high-value signal (token/context usage) to the
detail bar.

- **Row flex column = compact + live** — *what's happening right now*: the question
  being asked, the file being edited, the command running.
- **Detail bar SESSION SUMMARY = complete + historical** — the cumulative ledger
  (full tool counts, recent-files list, first prompt, timestamps), **plus** a new
  live context-fill line.

The redundancy dissolves because the row carries ephemeral live-edge information the
detail bar never shows, while the detail bar carries the cumulative/historical view
the row never shows.

## Non-goals

- No dollar-cost estimation (no per-model pricing table). Token *counts* only.
- No cumulative token totals in this iteration (context-fill only).
- No changes to the row's Complete / Idle / Stalled arms.
- No changes to the detail bar's existing lines (only an addition).
- No new persisted state / DB schema. All new data is derived from the session
  JSONL via `sessionx`, in memory, like the existing activity data.

## Data availability (verified)

Investigated in `sessionx` (`~/.cargo/git/checkouts/sessionx-*/.../src/activity/events.rs`)
and against a live Claude Code transcript.

**Available today, already parsed:**
- `recent_edited_files: VecDeque<String>` — most-recent-first ring (cap 7) of
  `file_path`s from `Edit`/`MultiEdit`/`Write`/`NotebookEdit`. `.front()` is the
  single most-recent edited file. (events.rs ~744-748)
- Bash command text — formatted into the `EventSnapshot.display` string
  (`ran \`<cmd>\``) at parse time. (events.rs ~763-768)
- `pending_tool_uses: HashMap<id, (tool_name, ts_ms)>` — in-flight tool **names**
  with first-seen timestamps. Newest = current in-flight tool.
- `tool_use_counts: { read, edit, write, bash, other }` — cumulative tallies.
- `last_assistant_text: Option<String>` — full last assistant text block.

**NOT available today — requires sessionx changes:**
- `AskUserQuestion` / `ExitPlanMode` topic text. Only the tool **name** is retained;
  the tool **input** (`question`/`header`/plan) is discarded for these tools.
- Token usage. The `usage` object exists on every assistant message in the JSONL but
  `sessionx` ignores it.

**Confirmed JSONL shape (live transcript):**
```json
"usage": { "input_tokens": 2, "cache_creation_input_tokens": 4874,
           "cache_read_input_tokens": 72081, "output_tokens": 277 }
"model": "claude-opus-4-8"
```
Critical: with prompt caching, `input_tokens` alone is a trap (2 here). True context
size on a message = `input_tokens + cache_creation_input_tokens + cache_read_input_tokens`
(≈ 77k here). The **latest** message's value is the current context-window fill.
`cache_read` must NOT be summed across turns (it re-reads the same context each turn).

## Design

### sessionx changes (land first)

One additional pass over data already being parsed in `events.rs`.

1. **Capture question topic.** Where `file_path`/`command` are already pulled from
   tool input, also pull the topic for the user-question tools:
   - `AskUserQuestion`: `input.questions[0].header` preferred (short, ≤12-char label
     by design), fall back to `input.questions[0].question` (trimmed/collapsed).
   - `ExitPlanMode`: no topic needed — the row will render a fixed `review plan`.
   Store on `WorkspaceEvents` as a new field, e.g.
   `pending_question_text: Option<String>`, cleared on session reset and when the
   pending question resolves.

2. **Capture context-fill tokens.** Parse the `usage` object on assistant messages.
   Expose on `WorkspaceEvents`:
   - `context_tokens: Option<u64>` — from the **latest** assistant message:
     `input_tokens + cache_creation_input_tokens + cache_read_input_tokens`.
   - `model_id: Option<String>` — `message.model` from the latest assistant message
     (so wsx can map to a context-window size).
   Reset on session reset. (Cumulative sums are explicitly out of scope this round.)

3. **Tests** (sessionx): fixtures covering — AskUserQuestion header capture,
   AskUserQuestion question-text fallback when header absent, ExitPlanMode (no topic),
   permission tool (no topic), `context_tokens` summing the three usage fields,
   `context_tokens` reflecting only the latest message (not a sum across turns),
   model id capture, and reset clearing all new fields.

### wsx — row flex column (`src/ui/dashboard/column_content.rs::row_column`)

Only the `Question` and `Thinking`/`Waiting` arms change.

**Question** (emphasis: `Status`, unchanged):
- AskUserQuestion pending → `asking: <pending_question_text>` (the new field).
- ExitPlanMode pending → `asking: review plan`.
- Permission tool pending (e.g. Bash) → `awaiting: <tool>`.
- Nothing identifiable → `question`.

**Thinking / Waiting** (emphasis: `Dim`, unchanged): full cumulative trace **+**
live item, joined with ` · `:
- Trace: existing `format_tool_trace(counts)` output (e.g. `read 14, edited 3, ran 5`).
- Live item, chosen from the newest in-flight tool (newest `pending_tool_uses` entry,
  else latest `AssistantToolUse` event):
  - Edit/Write/etc → `now <basename>` from `recent_edited_files.front()`.
  - Bash → ` <command>` derived from the latest tool-use event `display`
    (strip the `ran \`…\`` wrapper).
  - Read / nothing identifiable → omit the live item; fall back to the trace alone,
    or `thinking…`/`waiting…` when the trace is empty (today's behavior).
- Result examples: `read 14, edited 3, ran 5 · now column_content.rs`,
  `read 14, edited 3, ran 5 · cargo test --lib`.
- **Truncation:** the flex column already ellipsizes; ensure the live item is what
  gets clipped first when the row is narrow (trace is the more stable signal). The
  renderer (`row.rs`) handles char-based truncation of the whole column — verify the
  composed string degrades sensibly; no new truncation logic unless needed.

**Complete / Idle / Stalled:** unchanged.

### wsx — detail bar (`src/detail_modules/session_summary.rs`)

Existing lines unchanged. Add **one** line: context fill.

- Source: `WorkspaceEvents.context_tokens` (+ `model_id`).
- Format:
  - Window unknown → `context: 77k tokens`.
  - Window known → `context: 77k / 200k · 39%`.
- Window resolution: a small `model_id → window_tokens` map (default 200k). Heuristic
  1M upgrade: if `context_tokens` for a given workspace ever exceeds the mapped
  default, treat that session's window as 1M (prevents a bogus >100% on 1M sessions
  whose model id doesn't encode the variant).
- Coloring: dim normally; warn color as fill approaches the limit (e.g. ≥ ~85% when
  window known, or an absolute threshold like ≥ ~150k when unknown). Exact threshold
  is an implementation detail.
- Number formatting: `k`/`M` abbreviation consistent with existing dashboard style.

### wsx — dependency bump

- Update `Cargo.toml:31` `sessionx` `rev` to the merged sessionx commit; update
  `Cargo.lock`.
- During local development, use a temporary `[patch."https://github.com/bakedbean/sessionx"]`
  / path override pointing at the local sessionx worktree so wsx compiles against the
  unmerged change. Swap to the real merged `rev` and remove the patch before the wsx
  PR is finalized.

## Architecture / data flow

```
Claude Code session JSONL
   │  (sessionx parses, in memory)
   ▼
WorkspaceEvents  ── existing: tool_use_counts, recent_edited_files,
   │                          pending_tool_uses, first_user_text, ...
   │              ── NEW: pending_question_text, context_tokens, model_id
   ├──────────────► row_column()  ── row flex column (live edge)
   └──────────────► session_summary ── detail bar ledger + context line
```

No new storage, no new tick/polling path — these fields ride the existing
`WorkspaceEvents` update that already feeds both render sites.

## Testing strategy

- **sessionx**: fixture-driven unit tests as listed above (topic capture variants,
  token math, latest-vs-sum, reset).
- **wsx `column_content.rs`**: extend the existing table-driven `tests` module —
  Question arm for each topic source (AskUserQuestion header, question fallback,
  ExitPlanMode, permission, none); Thinking/Waiting arm for trace+live-item with an
  edited file, with a Bash command, with Read-only (no live item), and empty trace.
- **wsx detail bar**: tests for the context line — unknown window (raw tokens),
  known window (tokens + %), heuristic 1M upgrade past 200k, and the warn threshold.
- All new pure functions are deterministic over `(events, now_ms)`; follow the
  existing no-wall-clock pattern (`now_ms` passed in).

## Edge cases

- No session / `events == None` → unchanged (em-dash placeholder).
- Multiple pending tools → pick newest by timestamp for the live item / question.
- `context_tokens == None` (no assistant message yet) → omit the context line.
- Whitespace/newlines in captured question text → collapse via existing `collapse_ws`.
- Very long question header/text → rely on flex-column truncation; keep it on one line.
- Model id absent → treat window as unknown (raw tokens, no %).

## Rollout order

1. sessionx: implement + test + PR + merge.
2. wsx: bump `rev`, implement row + detail rendering + tests, PR.
3. Cross-link the two PRs; merge sessionx first, then wsx.

## Open implementation choices (decide during build, low-risk)

- Exact warn threshold(s) for the context line color.
- Whether the Read-only Thinking case shows `read N · thinking…` or just the trace.
- `k`/`M` rounding precision.
