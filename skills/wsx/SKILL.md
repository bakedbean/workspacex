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
wsx workspace create <repo> [--name <slug>] [--yolo] [--agent claude|pi|hermes|codex]
wsx workspace path <repo> <slug>            # prints just the worktree path (script-friendly)
wsx workspace rename <repo> <old> <new>     # renames slug AND git branch in sync
wsx workspace archive <repo> <slug> [--keep-worktree] [--force-delete-branch]

wsx repo list
wsx repo set-prefix <repo> <prefix>
wsx repo set-related-repos <repo> <comma-separated-names>

# Multi-agent: operate on the CURRENT workspace — no <repo>/<slug> args.
# The workspace is resolved from $WSX_WORKSPACE_ID, else the cwd's worktree.
wsx agent list                              # peers here; (primary) marks the original agent
wsx agent add <kind>                        # attach another agent: kind = claude|pi|hermes|codex
wsx agent send <label> <message…>           # async message to a peer (label e.g. claude, claude#2)
```

Run `wsx --help` or `wsx <command> --help` to list commands and arguments directly from the CLI.

`--agent` on `create` picks the workspace's first (primary) agent; `wsx agent add` attaches more on top. See [Multi-agent workspaces](#multi-agent-workspaces) below for how peers, labels, and messaging work.

The full reference is the project README's "CLI reference", "Multi-agent workspaces", and "Related repos" sections — consult it for `wsx config` / `wsx remote` / setup scripts.

## Reporting your status

wsx shows each workspace's state on its dashboard. Keep it accurate by pushing your status as you work. The command operates on the **current workspace** — resolved from `$WSX_WORKSPACE_ID`, else the cwd's worktree — so there are no `<repo>/<slug>` args.

```bash
wsx status set working --message "running the test suite"
wsx status set blocked --message "need your call on the auth approach"
wsx status set done    --message "implemented and tests green"
```

**When to call it:**

- `working` — when you begin substantive work on a request.
- `blocked` — when you stop to ask the user a question or need a decision.
- `done` — when the task is complete.

The `--message` is a short one-liner shown on the dashboard. Claude Code hooks also report coarse state automatically, but an explicit `set` with a message is always clearer — prefer it at the transitions above.

## Slug rules (read before typing --name)

A slug is a **2-4 word kebab-case summary of the task**: `add-widgets-endpoint`, `fix-login-redirect`.

It is **NOT** a full branch name. wsx prepends the repo's `branch_prefix` itself. Passing `bakedbean/add-widgets` yields a doubled prefix like `bakedbean/bakedbean/add-widgets`.

| Goal | --name value | Branch wsx creates |
|---|---|---|
| backend `bakedbean/add-widgets` | `add-widgets` | `bakedbean/add-widgets` |
| frontend `eg/add-widgets-ui` | `add-widgets-ui` | `eg/add-widgets-ui` |

Slugs **do not need to match** across related repos — each repo has its own `branch_prefix` and its own natural naming.

If you omit `--name`, wsx auto-generates an adjective-noun slug like `merry-birch`. Rename via `wsx workspace rename <repo> <auto> <real>` — this updates the git branch AND the wsx DB. Using `git branch -m` directly leaves wsx's DB stale.

## Cross-repo orchestration

When a task spans two repos configured as related (you'll see a system-prompt fragment listing read-only source paths like `/work/frontend`), follow this exact sequence:

1. **Finish the contract in this repo first.** Settle the API shape, types, or interface here. Commit it.
2. **Create the sibling workspace from this session:**
   ```
   wsx workspace create <other-repo> --name <slug>
   sibling=$(wsx workspace path <other-repo> <slug>)
   ```
3. **`cd "$sibling"`** and make the corresponding changes. Staying in the same Claude session means your context (API contract, design decisions) carries over — usually the right call.
4. **Two PRs, cross-linked.** Each repo gets its own branch and its own PR. In each description, link the other PR and call out merge order (typically: backend before frontend for new endpoints; frontend before backend for breaking removals).
5. **Tell the user** the PRs are ready and which order to merge. wsx has no atomic-merge primitive — the human is the coordinator.

If the work is large enough that you want separate Claude sessions per repo, the alternative is: create the workspace, then ask the user to attach to it via the wsx dashboard. A fresh Claude there will not share your context — propagate decisions via commits, PR bodies, or a design note checked into the repo.

## Common mistakes (verbatim from baseline testing)

- **Hallucinating syntax.** "I'll just try `wsx workspace create frontend bakedbean/foo`." Always re-read this skill's CLI surface before typing.
- **Passing a full branch name to `--name`.** Yields doubled prefix. Pass only the trailing slug.
- **Editing files in a related repo's source path** (`/work/<repo>`). Those are read-only mirrors on whatever branch the source's main worktree is on. Always `cd` into the path returned by `wsx workspace path`.
- **Committing on a placeholder branch.** If `git branch --show-current` shows the auto-generated slug (e.g. `bakedbean/merry-birch`) and you've decided what you're doing, rename via `wsx workspace rename` BEFORE committing.
- **Assuming a sibling session "knows" what you decided.** Different sessions don't share state — the PR body and commit messages are your handoff channel.

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
  attach one, use `wsx agent add <kind>` (kind = claude | pi | hermes | codex),
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

## When NOT to use

- TUI customization (keybindings, themes, dashboard layout) — those live in `wsx config` keys; see README.
- Editing the wsx source code itself — this skill is about *using* wsx, not developing it.
