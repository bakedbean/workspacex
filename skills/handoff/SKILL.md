---
name: handoff
description: Use in a finished wsx workspace (PR merged, work wrapping up) to continue the same feature or epic in a fresh workspace. Takes an optional implementation request; if omitted, asks the user for one. Creates a same-repo workspace, briefs its agent with a summary of this session's context, and forwards the request as its task.
---

# handoff

Continue the current feature/epic in a **new workspace with fresh context**.
You (the agent invoking this skill) are the outgoing agent: you know what was
built, decided, and left open. Your job is to create the next workspace, write
that knowledge into a brief its agent can start from, and hand off. You do
**not** work in the new workspace yourself.

Use this instead of starting a new branch in the current worktree — branching
in place piles unrelated history into one session and confuses the dashboard.

## Argument

Everything after `/handoff` is the user's **implementation request** for the
new workspace — free text, e.g.
`/handoff add a --json flag to widgets list and document it`.

The `handoff` pinned chip submits `/handoff` with no argument. When the
argument is empty, **ask the user** before doing anything else:

> What should the new workspace implement? (Reply `defer` to decide there.)

Wait for the answer. If it is `defer` (or an equivalent), the new agent's task
is to await the user's request; otherwise the answer *is* the task.

## Steps

1. **Confirm you are in a wsx workspace.** Verify `$WSX_WORKSPACE_ID` is set,
   or that the cwd is under `~/.local/state/wsx/worktrees/`. If neither holds,
   stop and tell the user this skill must run inside a wsx workspace.

2. **Resolve the request** — from the argument, or by asking (see above).

3. **Resolve the repo and current slug** from wsx, not from the filesystem:

   ```
   wsx context show | head -1        # "# wsx workspace: <repo>/<slug>"
   ```

   Renaming a workspace changes its slug and branch but **not** its worktree
   directory, so the path's trailing component and the branch name can both
   be stale. If in doubt, match the cwd against the path column of
   `wsx workspace list <repo>`.

4. **Pick a slug for the new workspace**: a 2-4 word kebab-case summary of the
   request (`add-json-list-flag`), or of the epic with a continuation hint
   (`bar-themes-followup`) when the request was deferred. Pass the bare slug —
   wsx prepends the repo's `branch_prefix`. Never reuse the current slug.

5. **Create the workspace** for the **same repo**:

   ```
   wsx workspace create <repo> --name <new-slug>
   ```

   Do not pass `--yolo` or `--agent`; the new workspace inherits this one's
   yolo mode and agent kind.

   If `create` fails, stop and report — do not send a brief. On a slug
   collision pick a different slug and retry; never brief a workspace you did
   not just create, even if one with that name exists. A setup-script warning
   after a successful create is worth relaying to the user but does not block
   the handoff.

6. **Gather the handoff context.** Pull from your own session memory first —
   that is the whole point of this skill — and check it against the repo:

   ```
   wsx recap show
   git log --oneline -15               # what this workspace shipped
   gh pr list --state all --head "$(git branch --show-current)"
   ```

   If `gh` is missing or errors, the PR state is *unknown* — say so in the
   brief rather than asserting "merged". A non-empty `git log main..HEAD` does
   not mean unmerged either (squash merges, stale local `main`).

   Distil, do not dump. The new agent shares the repo and can read code; it
   cannot read your conversation. Capture what is *not* in the repo:
   decisions and their reasons, approaches tried and rejected, gotchas hit,
   follow-ups noted but not done, and the file:line pointers that anchor it.

7. **Send the brief** to the new workspace's primary agent:

   ```
   wsx agent send --workspace <repo>/<new-slug> primary - <<'EOF'
   <brief>
   EOF
   ```

   The trailing `-` reads the body from stdin, and the quoted heredoc
   (`<<'EOF'`) passes it through verbatim — the user's request and your
   pointers may contain `$`, backticks, or quotes that a double-quoted
   argument would mangle or execute. (`--file <path>` works too.)

   Use the wsx skill's brief format. The brief is the new agent's *only*
   context — write it so it still makes sense if this session were deleted:

   ```
   TASK:        the user's request verbatim, plus what done looks like.
                If deferred: "Await the user's implementation request. Do NOT
                start implementing. Acknowledge this brief, summarize the
                inherited context to the user in a few lines, set
                `wsx status set blocked --message 'awaiting your request'`,
                and wait. Once the request arrives, rename the workspace to
                match it (`wsx workspace rename <repo> <new-slug> <slug>`)
                and set the recap goal from it."
   WHY:         the epic this continues and where it stands (PR #s, merged
                or not, what remains).
   CONTEXT:     summary of the previous workspace — decisions + reasons,
                rejected approaches, gotchas, file:line pointers, follow-ups
                already identified. Note the previous workspace as
                <repo>/<slug> so the agent can name its lineage in PRs.
   CONSTRAINTS: patterns to follow (path:line), things not to touch, merge
                ordering, anything the user has asked for before that still
                applies.
   START:       the first concrete step.
   ```

   `agent send` *queues* the brief and prints `queued message #<id> …`; the
   dashboard delivers it when the new agent's session is up
   (`wsx agent messages --sent` shows `DELIVERED` once it has). If it warns that no `wsx` dashboard is running, tell
   the user to open `wsx` — until then the brief sits undelivered. If `send`
   itself fails, fix and resend to the workspace you created; do not mark this
   workspace done until the brief is queued.

8. **Close out here.** Tell the user the brief is queued for
   `<repo>/<new-slug>` and what its agent will do once it starts (work the
   request, or wait for one). Set this workspace's status:

   ```
   wsx status set done --message "handed off to <repo>/<new-slug>"
   ```

   Mention that this workspace can be archived with
   `wsx workspace archive <repo> <slug>` — but do not archive it yourself.

## Example

User fires the chip; you ask; they reply
`add a --json flag to widgets list and document it`.

```
wsx workspace create backend --name add-json-list-flag
wsx agent send --workspace backend/add-json-list-flag primary - <<'EOF'
TASK: Add a --json flag to `widgets list` emitting one object per widget
(id, name, qty, created_at), and document it in docs/cli/widgets.md. Done
when the flag, a test for the JSON shape, and the docs are in and a PR is
open.
WHY: Continues the CLI-output epic from backend/tsv-list-output (PR #361,
merged). TSV shipped first; JSON was deferred to keep that PR small.
CONTEXT: The TSV writer is src/cli/widgets.rs:140-188 — add a sibling
formatter rather than branching inside it; we tried a single fn with a mode
enum and it read worse. Field names come from Widget::FIELDS
(src/model/widget.rs:22); reuse them, do not invent new spellings. Gotcha:
`widgets list --repo X` filters BEFORE formatting — keep that order.
CONSTRAINTS: Don't change the default (TSV) output; scripts depend on it.
Follow the arg pattern at src/cli/mod.rs:210 for the new flag.
START: read src/cli/widgets.rs:140-188, then the test module below it.
EOF
wsx status set done --message "handed off to backend/add-json-list-flag"
```

## Notes

- `wsx workspace create` from inside a workspace inherits yolo mode and agent
  kind — do not pass those flags.
- `wsx agent send` is asynchronous; the new agent receives the brief once its
  session is up, tagged `[message #<id> from <repo>/<slug> <label>; …]`, and
  can answer you with `wsx agent reply <id>`. `wsx workspace
  create --prompt <text>` is the same queue in one step; this skill keeps the
  two commands separate so a failed create is never followed by a brief.
- Do not `cd` into the new worktree or start the work there yourself. Create,
  brief, hand off, return.
