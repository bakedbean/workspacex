A shared workspace runs its agent inside a `tmux new-session -A` instead of a plain PTY child. The agent lives in the tmux server, not in wsx's process tree — quitting wsx (or losing your ssh connection) doesn't kill it. Next time wsx starts, it reattaches to the same tmux session automatically.

**Create shared:**

```
wsx workspace create <repo> --shared
```

Or from the dashboard, press `S` (capital) instead of `n`/`N` — it opens the same "new workspace" modal, just pre-set to shared. `Ctrl-s` toggles the shared flag while the modal is open, so you can flip it either way before confirming.

**Convert an existing workspace:**

```
wsx workspace share <repo> <slug>
wsx workspace unshare <repo> <slug>
```

Or press `T` (capital) on a selected workspace row to open a confirmation modal that toggles it. Converting either direction restarts any currently-running agent sessions in that workspace — there's no way to move a live process in or out of tmux — but conversation history isn't lost: the restart resumes via `--continue`, so the agent picks the conversation back up. Non-running instances just flip the flag with nothing to restart. `T` is a no-op on a repo header; sharing is per-workspace.

**Session naming:**

Each agent instance in a shared workspace gets a deterministic tmux session name: `wsx-<repo>-<workspace>` for the primary agent, or `wsx-<repo>-<workspace>-<agent><ordinal>` for additional instances (e.g. `wsx-myrepo-fix-bug-codex2`). Characters outside `[A-Za-z0-9_-]` in the repo/workspace name are replaced with `-`, since tmux rejects `.` and `:` in session names.

**The `◆ detached` status:**

When wsx starts up (or reconciles state), a shared workspace whose tmux session is confirmed alive on the server but has no client attached in the current wsx process shows `◆ detached` on the dashboard. This is the normal state right after a wsx restart, before you've attached to anything — the agent kept running the whole time. Attaching (`Enter` on the row) reattaches wsx's client to the live session; the agent and its history are exactly where you left them.

**Manual access:**

Because the agent is a normal tmux session, you can attach to it directly, bypassing wsx entirely:

```
tmux attach -t wsx-<repo>-<workspace>
```

This works over a plain `ssh` connection today — no wsx-specific networking, remote-control setup, or port-forwarding required. See [Remote access](remote-access.md) for the broader pattern of running wsx itself over ssh/tmux; shared workspaces are the finer-grained, per-workspace version of the same idea; you can `tmux attach` to one agent's session without pulling in the rest of wsx.

**Listing shared workspaces:**

```
wsx shared list
wsx shared list --json
```

Without `--json`, prints one tab-separated line per agent instance: repo, workspace, tmux session name, and `alive`/`(dead)`/`-`. With `--json`, prints the same data as structured records (repo, workspace, branch, worktree path, and each agent's label/kind/session name/liveness) — useful for scripting against.

**v1 limitation — scrollback:**

Reattaching (in wsx or via a bare `tmux attach`) only repaints the tmux session's *current visible screen* — wsx's own scrollback buffer (see [Mouse, scrollback, and text selection](../daily-use/mouse-scrollback-selection.md)) resets with each new client and doesn't carry history across a detach/reattach. tmux's own scrollback for the session is unaffected and still reachable in-session via its usual copy-mode (`Ctrl-b [` with tmux's default prefix). A richer remote-scrollback view is planned for a later phase.
