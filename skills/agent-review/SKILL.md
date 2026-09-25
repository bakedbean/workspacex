---
name: agent-review
description: Use in a wsx workspace to spin up a peer review agent that code-reviews the current branch. Takes the reviewer kind (claude|pi|hermes|codex|omp; asks which when omitted); spawns the peer, hands it branch-diff-vs-main context, and has it report findings back to you.
---

# agent-review

Spin up a peer **review agent** in the current wsx workspace and hand it the
branch's review context. You (the agent invoking this skill) act as the
coordinator: you spawn the reviewer, brief it, and stay available to receive
its findings.

## Argument

A single optional argument: the reviewer **kind**, one of `claude`, `pi`,
`hermes`, `codex`, `omp`.

- `/agent-review codex` → spawn a `codex` reviewer
- `/agent-review omp` → spawn an `omp` (oh-my-pi) reviewer
- `/agent-review` → ask which kind (below), then spawn it

When the argument is empty (e.g. the `agent-review` pinned chip was fired and
submitted without a kind), **ask the user** before doing anything else. Use a
single-choice prompt if your harness has one (Claude Code: `AskUserQuestion`),
otherwise ask in plain text:

> Which reviewer kind? `claude` / `pi` / `hermes` / `codex` / `omp`

List `claude` first as the usual choice. Wait for the answer; do not default.

If an argument (or answer) is given that is not one of the kinds listed above,
stop and tell the user the valid kinds. Do not guess.

## Steps

1. **Confirm you are in a wsx workspace.** This skill operates on the *current*
   workspace. Verify `$WSX_WORKSPACE_ID` is set, or that the cwd is under
   `~/.local/state/wsx/worktrees/`. If neither holds, stop and tell the user
   this skill must run inside a wsx workspace.

2. **Resolve the kind** from the argument, or from the user's answer when the
   argument was empty (validate as above).

3. **Spawn the reviewer peer:**

   ```
   wsx agent add <kind>
   ```

   The command prints `added <label>` — capture `<label>` (e.g. `claude#2`).
   This is the peer you will brief. The new agent shares this worktree and
   branch.

4. **Find your own coordinator label** so you can tell the reviewer who you
   are:

   ```
   wsx agent whoami
   ```

   It prints your `label`, `instance` id, and `workspace`. The reviewer will
   answer with `wsx agent reply`, which routes back to you automatically, so
   the label is only for the brief's context.

5. **Gather a short brief** — do NOT paste the whole diff; the reviewer shares
   the worktree and can read it:

   ```
   git branch --show-current
   git log main..HEAD --oneline
   git diff --stat main...HEAD
   ```

6. **Hand off to the reviewer** with a single message. Write the brief to a
   file (or pipe it on stdin) so backticks and quotes pass through verbatim:

   ```
   wsx agent send --file <brief-file> <label>
   ```

   It prints `queued message #<id> to <label> (<n> bytes)`. Once the reviewer's
   session picks it up, `wsx agent messages --sent` shows it delivered.

   The `<brief>` must instruct the reviewer to:
   - Review the current branch against `main`. Run `git diff main...HEAD`
     itself to see the full change.
   - Produce a **risk assessment** — security, performance, breaking changes,
     edge cases.
   - Produce a **gap analysis** — test coverage, documentation, error handling.
   - Report findings back when done with
     `wsx agent reply <id> --file <findings-file>` (or `… <id> -` piping the
     findings on stdin), where `<id>` is the number in this brief's
     `[message #<id> from …]` banner.

   Include the branch name, commit list, and diff-stat from step 5 so the
   reviewer has orientation without re-deriving it.

7. **Tell the user** the reviewer `<label>` is spawned and working, and that its
   findings will arrive as a `[message #<id> from <label>; …]` in this session.
   Then end your turn — the findings are injected as your next input. (To
   block for them inside a turn instead, use
   `wsx agent wait --from <label> --after <brief-id> --timeout <secs>`.)

## Example handoff message

```
wsx agent send claude#2 - <<'EOF'
You are a code reviewer for this wsx workspace (I am claude, the primary).
Branch: feat/widgets (3 commits, 7 files changed). Review this branch against
main: run `git diff main...HEAD` to see the full change. Provide (1) a risk
assessment — security, performance, breaking changes, edge cases; and (2) a gap
analysis — test coverage, documentation, error handling. When done, write your
findings to a file and send them back with:
wsx agent reply <id from this message's banner> --file <findings-file>
EOF
```

## Notes

- All `wsx agent` commands resolve the current workspace automatically from
  `$WSX_WORKSPACE_ID` or the cwd — you do not pass repo/slug.
- `wsx agent send` is asynchronous; the reviewer receives the brief shortly
  after you send it and works in its own pane.
- The reviewer shares your worktree. Reviewing is read-only, so this is normally
  safe, but avoid large edits while the review runs.
