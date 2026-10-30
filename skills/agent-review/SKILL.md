---
name: agent-review
description: Use in a wsx workspace to spin up a peer review agent that code-reviews the current branch. Takes the reviewer kind (claude|pi|hermes|codex|omp; asks which when omitted); spawns the peer, hands it branch-diff-vs-main context (telling it to run a code-review skill, preferring the repo-local one over its harness built-in), and has it report findings back to you.
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
   - **Run a `code-review` skill first, preferring the repo's own.** Look for
     a repo-local one under `.claude/skills/`, `.agents/skills/`, or
     `.github/skills/` (e.g. `.github/skills/code-review/SKILL.md`); if found,
     read its `SKILL.md` and follow it, since it carries the repo's review
     conventions. Otherwise use the harness's built-in (Claude Code ships one
     as `/code-review`). Run it on this branch's changes vs `main` and build
     the report on its findings. Without either, review the diff directly.
   - **Keep the review read-only.** Never post to the PR or edit the working
     tree: skip any step of the skill that posts comments or applies fixes,
     and never pass `--comment`, `--fix`, or `--post`. Findings go back
     through wsx. If the skill reports findings through a UI tool rather than
     text, restate them in the reply.
   - Produce a **risk assessment** — security, performance, breaking changes,
     edge cases.
   - Produce a **gap analysis** — test coverage, documentation, error handling.
   - Report findings back when done with
     `wsx agent reply --file <findings-file> <id>` (or `… <id> -` piping the
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
main: run `git diff main...HEAD` to see the full change. First run a
`code-review` skill on this branch's changes vs main and build on its
findings: prefer the repo's own (check .claude/skills/, .agents/skills/,
.github/skills/ and follow its SKILL.md), else your harness's built-in (e.g.
Claude Code's `/code-review`). Stay read-only: don't post to the PR, apply
fixes, or pass --comment, --fix, or --post. Provide (1) a risk
assessment — security, performance, breaking changes, edge cases; and (2) a gap
analysis — test coverage, documentation, error handling. When done, write your
findings to a file and send them back with:
wsx agent reply --file <findings-file> <id from this message's banner>
EOF
```

## Notes

- All `wsx agent` commands resolve the current workspace automatically from
  `$WSX_WORKSPACE_ID` or the cwd — you do not pass repo/slug.
- `wsx agent send` is asynchronous; the reviewer receives the brief shortly
  after you send it and works in its own pane.
- Which `code-review` skill applies depends on the repo and the reviewer's
  harness, not yours, so the brief states the preference order and the
  reviewer decides. A repo-local skill may live only under `.github/skills/`,
  where non-Claude harnesses won't auto-discover it, hence the explicit paths.
- The reviewer shares your worktree. Reviewing is read-only, so this is normally
  safe, but avoid large edits while the review runs.
