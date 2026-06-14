---
name: agent-pr
description: Use in a wsx workspace to spin up a peer review agent that code-reviews the current branch. Takes the reviewer kind (claude|pi|hermes|codex, default claude); spawns the peer, hands it branch-diff-vs-main context, and has it report findings back to you.
---

# agent-pr

Spin up a peer **review agent** in the current wsx workspace and hand it the
branch's review context. You (the agent invoking this skill) act as the
coordinator: you spawn the reviewer, brief it, and stay available to receive
its findings.

## Argument

A single optional argument: the reviewer **kind**, one of `claude`, `pi`,
`hermes`, `codex`. Defaults to `claude` when omitted (e.g. when fired from the
`agent-pr` pinned chip, which submits `/agent-pr` with no argument).

- `/agent-pr` → spawn a `claude` reviewer
- `/agent-pr codex` → spawn a `codex` reviewer

If an argument is given that is not one of the four kinds, stop and tell the
user the valid kinds. Do not guess.

## Steps

1. **Confirm you are in a wsx workspace.** This skill operates on the *current*
   workspace. Verify `$WSX_WORKSPACE_ID` is set, or that the cwd is under
   `~/.local/state/wsx/worktrees/`. If neither holds, stop and tell the user
   this skill must run inside a wsx workspace.

2. **Resolve the kind** from the argument (default `claude`; validate as above).

3. **Spawn the reviewer peer:**

   ```
   wsx agent add <kind>
   ```

   The command prints `added <label>` — capture `<label>` (e.g. `claude#2`).
   This is the peer you will brief. The new agent shares this worktree and
   branch.

4. **Find your own coordinator label** so the reviewer knows where to send
   findings:

   ```
   wsx agent list
   ```

   The workspace's original agent is marked `(primary)`. Use your own label
   (the one matching `$WSX_AGENT_INSTANCE_ID`, or the primary if you are it).

5. **Gather a short brief** — do NOT paste the whole diff; the reviewer shares
   the worktree and can read it:

   ```
   git branch --show-current
   git log main..HEAD --oneline
   git diff --stat main...HEAD
   ```

6. **Hand off to the reviewer** with a single message:

   ```
   wsx agent send <label> "<brief>"
   ```

   The `<brief>` must instruct the reviewer to:
   - Review the current branch against `main`. Run `git diff main...HEAD`
     itself to see the full change.
   - Produce a **risk assessment** — security, performance, breaking changes,
     edge cases.
   - Produce a **gap analysis** — test coverage, documentation, error handling.
   - Report findings back to the coordinator with
     `wsx agent send <your-label> "<findings>"` when done.

   Include the branch name, commit list, and diff-stat from step 5 so the
   reviewer has orientation without re-deriving it.

7. **Tell the user** the reviewer `<label>` is spawned and working, and that its
   findings will arrive as a `[message from <label>]` in this session.

## Example handoff message

```
wsx agent send claude#2 "You are a code reviewer for this wsx workspace.
Branch: feat/widgets (3 commits, 7 files changed). Review this branch against
main: run \`git diff main...HEAD\` to see the full change. Provide (1) a risk
assessment — security, performance, breaking changes, edge cases; and (2) a gap
analysis — test coverage, documentation, error handling. When done, send your
findings back to me with: wsx agent send <your-label> \"<your findings>\"."
```

## Notes

- All `wsx agent` commands resolve the current workspace automatically from
  `$WSX_WORKSPACE_ID` or the cwd — you do not pass repo/slug.
- `wsx agent send` is asynchronous; the reviewer receives the brief shortly
  after you send it and works in its own pane.
- The reviewer shares your worktree. Reviewing is read-only, so this is normally
  safe, but avoid large edits while the review runs.
