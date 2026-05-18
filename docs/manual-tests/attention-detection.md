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
