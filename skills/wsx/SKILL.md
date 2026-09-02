---
name: wsx
description: Use when working inside a wsx-managed worktree (CWD under ~/.local/state/wsx/worktrees/), when the user asks to create/list/rename/archive wsx workspaces, when adding or messaging peer agents in a workspace (wsx agent add/send/list), or when a system prompt mentions related wsx repos and a task requires changes in more than one of them.
---

# wsx

Drives the `wsx` CLI to manage workspaces (git worktrees + per-workspace Claude sessions) and to orchestrate work across related repos.

## Detecting context

You are in a wsx workspace if your CWD matches `~/.local/state/wsx/worktrees/<repo>/<slug>`. The trailing `<slug>` is the workspace name; the branch is `<branch_prefix>/<slug>` where `<branch_prefix>` is set per-repo. **Do not infer the slug from the branch name** — read it from the path or `wsx workspace list <repo>`.

When orienting, run these first — they're cheap and authoritative:

```
wsx repo list                  # registered repos, source paths, prefixes
wsx workspace list             # all workspaces, TSV: repo, slug, branch, path
wsx workspace list <repo>      # filter to one repo
```

## CLI surface

```
wsx workspace create <repo> [--name <slug>] [--yolo] [--agent claude|pi|hermes|codex|omp]
wsx workspace path <repo> <slug>            # prints just the worktree path (script-friendly)
wsx workspace rename <repo> <old> <new>     # renames slug AND git branch in sync
wsx workspace archive <repo> <slug> [--keep-worktree] [--force-delete-branch]

wsx repo list
wsx repo set-prefix <repo> <prefix>
wsx repo set-related-repos <repo> <comma-separated-names>

# Multi-agent: `list`/`add` operate on the CURRENT workspace — no <repo>/<slug>
# args. The workspace is resolved from $WSX_WORKSPACE_ID, else the cwd's
# worktree. `send` can target another workspace via --workspace.
wsx agent list                              # peers here; (primary) marks the original agent
wsx agent add <kind>                        # attach another agent: kind = claude|pi|hermes|codex|omp
wsx agent send [--workspace <repo>/<slug>] <label> <message…>
                                            # async message to an agent; omit
                                            # --workspace for a peer here.
                                            # label `primary` = that workspace's
                                            # primary agent (always correct for a
                                            # workspace you just created).

wsx context show                            # markdown digest of this workspace (for editor-hosted agents)
wsx context write                           # same, written under the state dir; prints the path
```

Run `wsx --help` or `wsx <command> --help` to list commands and arguments directly from the CLI.

`--agent` on `create` picks the workspace's first (primary) agent; `wsx agent add` attaches more on top. See [Multi-agent workspaces](#multi-agent-workspaces) below for how peers, labels, and messaging work.

When `create` runs from inside a workspace (an agent handing off, or a shell in a worktree), the new workspace inherits that workspace's yolo mode and agent kind. Don't pass `--yolo` when handing off; pass `--agent` only to deliberately pick a different agent. Creates from outside any workspace default to non-yolo and the `coding_agent` setting (claude unless configured otherwise).

The full reference is the project README's "CLI reference", "Multi-agent workspaces", and "Related repos" sections — consult it for `wsx config` / `wsx remote` / setup scripts.

## Reporting your status

wsx shows each workspace's state on its dashboard. Keep it accurate by pushing your status as you work. The command operates on the **current workspace** — resolved from `$WSX_WORKSPACE_ID`, else the cwd's worktree — so there are no `<repo>/<slug>` args.

```bash
wsx status set working --message "running the test suite"
wsx status set blocked --message "need your call on the auth approach"
wsx status set done    --message "implemented and tests green"
```

**When to call it** (the states are `working | waiting | blocked | done`):

- `working` — when you begin substantive work on a request.
- `blocked` — when you stop to ask the user a question or need a decision.
- `waiting` — when parked on something external (a build, CI, a long-running command).
- `done` — when the task is complete.

The `--message` is a short one-liner shown in the PM pane and the waybar menu subtext. Claude Code hooks also report coarse state automatically, but an explicit `set` with a message is always clearer — prefer it at the transitions above.

## Maintaining the workspace recap

Alongside status, maintain the workspace recap — the dashboard's project-manager digest renders these three one-liners:

```sh
wsx recap set --goal "cookie expiry bug from #42" --goal-short "cookie expiry, #42"   # once, when scope is clear
wsx recap set --state "tests added but failing" --state-short "tests failing" \
              --next "debug session token regex" --next-short "debug token regex"
wsx recap show
```

Fields update independently; set `--goal`/`--goal-short` once and refresh the state/next pairs as work progresses. Short forms are keyword distillations for the dashboard row — telegraphic style: identifiers and ticket/PR numbers only, no articles (a/an/the), no filler verbs ("make dashboard PR clickable", not "Make the dashboard PR status column clickable"); aim for ≤40 chars (goal) / ≤24 chars (state, next).

## Slug rules (read before typing --name)

A slug is a **2-4 word kebab-case summary of the task**: `add-widgets-endpoint`, `fix-login-redirect`.

It is **NOT** a full branch name. wsx prepends the repo's `branch_prefix` itself. Passing `bakedbean/add-widgets` yields a doubled prefix like `bakedbean/bakedbean/add-widgets`.

| Goal | --name value | Branch wsx creates |
|---|---|---|
| backend `bakedbean/add-widgets` | `add-widgets` | `bakedbean/add-widgets` |
| frontend `eg/add-widgets-ui` | `add-widgets-ui` | `eg/add-widgets-ui` |

Slugs **do not need to match** across related repos — each repo has its own `branch_prefix` and its own natural naming.

If you omit `--name`, wsx auto-generates an adjective-noun slug like `merry-birch`. Rename via `wsx workspace rename <repo> <auto> <real>` — this updates the git branch AND the wsx DB. Using `git branch -m` directly leaves wsx's DB stale.

## Handing off to a new workspace

Creating a workspace and then working in it yourself defeats the purpose: the
new workspace sits idle on the dashboard while this session's history grows.
Create it, brief its agent, and go back to your own task.

**When.** Two triggers:

- **Hard:** the work ahead needs a new branch. Branching inside this worktree
  is the wrong move — create a workspace instead.
- **Soft:** the work shifts to a concern independent enough that this session's
  history would be noise. It must genuinely stand alone; a subtask of what
  you're already doing does not qualify.

**How.** Two commands:

```
wsx workspace create <repo> --name <slug>
wsx agent send --workspace <repo>/<slug> primary "<brief>"
```

Always pass `--name` — an unnamed workspace forces the new agent to rename it
before it can start. Use `primary` as the label: you cannot run `wsx agent
list` against another workspace, and a fresh workspace has exactly one agent.

**The brief.** It is the receiving agent's *only* context. Write it so it still
makes sense if this session were deleted.

```
TASK:        what to build or fix, and what done looks like
WHY:         the decision or finding that led here
CONTEXT:     contracts, types, names, file:line pointers — anything decided in
             my session that is not yet in the repo
CONSTRAINTS: don't touch X; follow the pattern at path:line; merge after PR #N
START:       the first concrete step
```

**Then.** Tell the user which workspace is now working on what, and return to
your own task.

**Worked example:**

```
wsx workspace create backend --name add-widgets-endpoint
wsx agent send --workspace backend/add-widgets-endpoint primary "
TASK: Add POST /widgets returning 201 with the created Widget. Done when the
handler, its route registration, and a happy-path + validation test are in.
WHY: The frontend work in workspacex/widgets-ui needs this endpoint; I settled
the payload shape there and it is not in any repo yet.
CONTEXT: Request body is {name: string, qty: int}; response is the full Widget
including server-assigned id and created_at. Follow the pattern in
src/api/gadgets.rs:40-88 — same validation helper, same error envelope.
CONSTRAINTS: Don't change the existing GET /widgets response shape. This must
merge BEFORE workspacex/widgets-ui.
START: read src/api/gadgets.rs:40-88, then src/api/mod.rs route table.
"
```

Delivery requires a running `wsx` dashboard — the TUI is what injects queued
messages. If `agent send` warns that none is running, tell the user to open
`wsx`, or the handoff will sit undelivered.

## Cross-repo orchestration

When a task spans two repos configured as related (you'll see a system-prompt fragment listing read-only source paths like `/work/frontend`), follow this exact sequence:

1. **Finish the contract in this repo first.** Settle the API shape, types, or interface here. Commit it.
2. **Create the sibling workspace from this session:**
   ```
   wsx workspace create <other-repo> --name <slug>
   ```
3. **Brief its agent and hand off.**
   ```
   wsx agent send --workspace <other-repo>/<slug> primary "<brief>"
   ```
   See [Handing off to a new workspace](#handing-off-to-a-new-workspace) for
   what the brief must contain. Do NOT `cd` into the sibling worktree and make
   the changes yourself.
4. **Two PRs, cross-linked.** Each repo gets its own branch and its own PR. In each description, link the other PR and call out merge order (typically: backend before frontend for new endpoints; frontend before backend for breaking removals).
5. **Tell the user** the PRs are ready and which order to merge. wsx has no atomic-merge primitive — the human is the coordinator.

The sibling session does not share your context — the brief is your handoff
channel for the initial task. For anything that must outlive either session,
propagate it via commits and PR bodies.

## Common mistakes (verbatim from baseline testing)

- **Hallucinating syntax.** "I'll just try `wsx workspace create frontend bakedbean/foo`." Always re-read this skill's CLI surface before typing.
- **Passing a full branch name to `--name`.** Yields doubled prefix. Pass only the trailing slug.
- **Editing files in a related repo's source path** (`/work/<repo>`). Those are read-only mirrors on whatever branch the source's main worktree is on — never write there. If the task needs changes in that repo, create a workspace and hand it off (see [Handing off to a new workspace](#handing-off-to-a-new-workspace)); `wsx workspace path` is for reading the sibling worktree, not for `cd`-ing in and editing it yourself.
- **Committing on a placeholder branch.** If `git branch --show-current` shows the auto-generated slug (e.g. `bakedbean/merry-birch`) and you've decided what you're doing, rename via `wsx workspace rename` BEFORE committing.
- **Assuming a sibling session "knows" what you decided.** Different sessions don't share state — the PR body and commit messages are your handoff channel.
- **Driving a workspace you created.** Creating a workspace and then `cd`-ing
  into it leaves it idle on the dashboard and piles its history into the wrong
  session. Create, brief, hand off.

## Multi-agent workspaces

A workspace can have more than one agent attached — including more than one of
the same kind. You may be one of several agents sharing the same git worktree
and branch.

- **See your peers:** run `wsx agent list`. Agents are addressed by label — the
  first of a kind is its bare name (`claude`), additional ones get a numeric
  suffix (`claude#2`). The primary (workspace-creation) agent is marked
  `(primary)`.
- **Your identity:** `$WSX_AGENT_INSTANCE_ID` holds your instance id and
  `$WSX_WORKSPACE_ID` holds the workspace id.
- **Message a peer:** `wsx agent send <label> <message>`. Delivery is
  asynchronous — the message is injected into the peer's session shortly after,
  tagged `[message from <you>]` so they know it came from you.
- **Add a peer:** `wsx agent send` only reaches agents already attached. To
  attach one, use `wsx agent add <kind>` (kind = claude | pi | hermes | codex | omp),
  or the `^x a` panel in the TUI. You can use this proactively — e.g. spin up a
  second `claude` to review your diff or work a parallel sub-task, then hand it
  the task with `wsx agent send <its-label> "<instructions>"`. The new agent
  shares this worktree, so scope its work to avoid overlapping edits.

**Example — a reviewer agent pinging the primary about a finding:**

```
wsx agent send claude "I reviewed the diff on this branch. The retry loop in
fetch.rs (line 88) has no upper bound — can you cap it?"
```

Because all agents in a workspace share the worktree, coordinate before making
overlapping edits to the same files — prefer messaging to hand off work.

## External editor agents

The user may open this worktree in an editor whose own AI agent (for example
magenta.nvim in neovim) shares your branch and working tree. That agent
reads a digest produced by `wsx context write` — your recap, status, peers,
recent commits, and your last message — so it already knows what you are
doing. It does not set status or recap; it reports back with
`wsx agent send <your label> "<summary>"`, which reaches you as a bare
`[message]` banner with no sender label.

Treat those messages as the user's follow-up instructions, and run
`git status` / `git diff` before assuming the tree matches your last edit.
You can inspect the same digest yourself with `wsx context show`.

## When NOT to use

- TUI customization (keybindings, themes, dashboard layout) — those live in `wsx config` keys; see README.
- Editing the wsx source code itself — this skill is about *using* wsx, not developing it.
