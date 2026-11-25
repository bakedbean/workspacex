# `wsx new`: one-step workspace creation from a phone

Status: awaiting approval

## Intent and why

The user's workflow is to ssh into the desktop from a phone (Termius), create a
workspace, and work with its agent in the Claude mobile app. Today a created
workspace is invisible to the app until it is entered from the dashboard,
because `wsx workspace create` starts no agent. The user's words: "a very
simple one step flow: I connect to the desktop via Termius, do one thing and
then I am good to go."

## Goals

- One command, usable on a phone keyboard: no flags, no slug.
- When it reports success, the agent can be reached from the Claude app.
- Works whether or not a dashboard is running.
- Survives the phone dropping the ssh connection at any point after the
  questions are answered.

## Non-goals

- Changing what `wsx workspace create` or the dashboard's create flow does.
- A full-screen TUI, or managing existing workspaces from the phone.
- Adding Claude-app support for other agent kinds.

## Anti-goals

- Reporting success while the agent is stuck or unreachable.
- Losing the typed task, or delivering it twice.
- Two agents running for one workspace.

## How it works today

- The dashboard owns agent launching: `build_spawn_info` and `tmux_name_for`
  (`src/app/spawn.rs`), `ensure_primary_session` and `force_share_workspace`
  (`src/app/session.rs`).
- `create --start` (`src/commands/shared.rs`, `start_created`) queues a share
  request, the dashboard claims it and starts the agents in tmux. Unclaimed
  after 5s, it is withdrawn and the command fails.
- `--remote-control` is passed to Claude when the `remote_control` setting is
  on (the default).
- Claude Code writes `~/.claude/sessions/<pid>.json` with `cwd` and
  `bridgeSessionId`. Checked on this machine (Claude Code 2.1.331/2.1.332):
  all 10 live `--remote-control` sessions with a file have the id, and the one
  without the flag has none. The format is undocumented.
- Claude's login on this machine is not in `~/.claude/.credentials.json`, so
  it is presumably in the macOS Keychain.

## Approaches

- **A. A dedicated `wsx new` command (picked by the user).** Covers every goal.
  Costs the most: launching must work outside the dashboard.
- **B. A setting that makes every `create` start its agent.** Small, but keeps
  the long command and fails with no dashboard.
- **C. A Termius snippet running `create --start`.** No code, but one snippet
  per repo and fails with no dashboard.

## Behavior

Agreed with the user:

    $ wsx new
     1 workspacex   2 backend   3 frontend
    repo> 1
    task (optional)> fix the login redirect

    ✓ workspacex/merry-birch: agent running, open the Claude app

- `wsx new <repo> [task words...]` asks nothing, so it can be a Termius snippet.
- Repos are listed most recently used first.
- The slug is generated; the agent renames it from the task, as today.
- A running dashboard starts the agent; otherwise `wsx new` starts it in tmux
  and the dashboard picks it up later. The workspace is always tmux-shared.

Calls made here, for the user to check:

- **Recency** is the newest workspace `created_at` in a repo; repos with none
  come last in dashboard order. Bare Enter picks 1. An unlisted number or name
  asks again. One registered repo skips the repo question. Archived workspaces
  are not counted if their rows are gone.
- **Task parsing.** Everything after the repo is the task. Flags go before the
  repo: `wsx new [--yolo] [--agent <kind>] [<repo> [task...]]`. With no
  terminal on stdin and no repo, it is a usage error rather than a hang.
- **Yolo and agent kind** default as in `create`: not yolo, and the
  `coding_agent` setting, with inheritance when run inside a workspace. From
  the phone, permission prompts appear in the Claude app; a snippet can pass
  `--yolo`.
- **Failures before creating anything:** tmux < 3.2 or missing, agent binary
  missing, unknown repo. Claude with `remote_control` off prints that the app
  won't list it, and continues.
- **A failed setup script** warns and still starts the agent, as `create` does.

### Outcomes

| Outcome | Output | Exit |
|---|---|---|
| Reachable | `✓ … agent running, open the Claude app` | 0 |
| Agent up, no bridge id within 60s | `agent started, but not visible to the Claude app yet`, the last screen lines, and how to attach | 1 |
| Non-Claude, or `remote_control` off | `✓ … agent running` | 0 |
| Agent exited or never started | the error, its last screen lines, that the workspace exists | 1 |

Attaching is `tmux attach -t =<session>`, where a trust or login prompt can be
answered from the phone. A task that was not delivered is queued, so the
dashboard delivers it when it next runs; the output says so.

## Design

1. **Parse and ask.** A top-level `new` command in `src/cli`, registered in
   `groups.rs`. Prompts are plain line reads, so they work in any terminal.
2. **Detach.** After the questions, re-run the binary as a hidden worker in a
   new session (`setsid`, via the existing `libc` dependency), logging to a
   file under `Dirs::log_dir()`. The foreground process follows the log and
   exits with the worker's code. Disconnecting or Ctrl-C ends only the
   follower. The task reaches the worker by file or stdin, not argv.
3. **Create** through `data::workspace::create` with `shared = true`.
4. **Start, dashboard first.** Use the share-request path unchanged. Only a
   request successfully withdrawn unclaimed falls through to step 5; any other
   failure is reported, since the dashboard may have started the agent. This
   needs a distinct error for "no dashboard", not string matching.
5. **Start, headless.** Build an `App` in the worker without the startup
   sweeps (`sweep_stale_pending`, `sweep_stale_running`), which belong to a
   dashboard and could disturb another process's create. Call
   `force_share_workspace`, so command construction, MCP mirroring and tmux
   naming stay one code path. Afterwards drop the session: that ends the tmux
   client and leaves the agent running. A dashboard opened later reattaches,
   because the session name is stored and `new-session -A` is idempotent.
6. **Deliver the task.** Dashboard path: enqueue once the agent is live. Headless
   path: inject into the held session with `send_text_when_settled`, and enqueue
   only if that returns false. Keep the session open until this ends. Never run
   the general inbox drain from the worker; it would start other workspaces'
   agents.
7. **Check reachability** (Claude, `remote_control` on). Poll the session files
   for one whose `cwd` is the worktree, whose pid is alive, and whose
   `bridgeSessionId` is set. Compare canonical paths. If the session file exists
   but has no such key, or no files exist at all, treat it as not confirmed. This
   runs alongside step 6, since a blocking prompt would hold up both.

## First thing to build: a spike

Two unknowns decide whether the headless path works on macOS. Test them with a
throwaway before the plan's other tasks, over a real ssh login with no tmux
server already running, and report back to the user:

- **Keychain.** Can Claude in a tmux server started over ssh read its login? An
  existing server started from the desktop session may work where a new one
  doesn't.
- **Directory trust.** Does a non-yolo Claude in a new worktree stop at a trust
  prompt before remote control connects?

If the Keychain test fails, stop and bring options to the user before building
the headless path. The likely ones are `claude setup-token` with a stored
token, or starting the tmux server from a login item. The dashboard path is
useful either way.

## Testing

- Parsing, repo ordering, and prompts with injected input.
- Claim path, withdrawn path, and failed-after-claim (must not fall back).
- Session-file matching against fixtures: missing key, dead pid, other cwd,
  malformed JSON.
- Headless start against a private tmux server with a fake agent binary,
  following the existing tests in `src/pty/session.rs`: the agent survives the
  worker, and a second `spawn` reattaches rather than duplicating.
- The worker survives its parent being hung up.
- By hand: Termius to the Claude app, with and without a dashboard, including
  a disconnect during setup.

## Left to the builder

Module layout, the worker's hidden argument form, log file naming, poll
intervals, and exact wording. Docs: the CLI reference page and the wsx skill's
CLI surface.
