A shared workspace runs its agent inside a `tmux new-session -A` instead of a plain PTY child. The agent lives in the tmux server, not in wsx's process tree — quitting wsx (or losing your ssh connection) doesn't kill it. Next time wsx starts, it reattaches to the same tmux session automatically.

Shared workspaces require **tmux ≥ 3.2** (for the `-e` flag on `new-session`, used to forward wsx's environment into a pre-existing tmux server).

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

These CLI commands flip the shared flag. Running sessions keep their current backend (shared or non-shared) until restarted manually — the command prints a note saying so. New or restarted sessions will pick up the new backend.

To share *and* restart right away from the command line — for example over `ssh` from another machine — add `--restart`:

```
wsx workspace share <repo> <slug> --restart [--json]
```

A direct agent is a child of the wsx dashboard, so only the dashboard can move it into tmux. `--restart` asks the dashboard running on that host to do what `T` does (restart running agents inside tmux, resuming their conversations, and start stopped ones), then waits until every agent's tmux session is live and prints the session names to attach to (`--json` prints the workspace's record instead). An already-shared workspace whose agents are stopped is simply started. If no dashboard picks the request up within a few seconds, the command withdraws it and fails without changing anything. Start `wsx` on the host and try again. A workspace whose agents are already live in tmux returns immediately.

Alternatively, press `T` (capital) on a selected workspace row to open a confirmation modal. This immediately restarts any currently-running agent sessions in that workspace — there's no way to move a live process in or out of tmux — but conversation history isn't lost: the restart resumes via `--continue`, so the agent picks the conversation back up. Non-running instances just flip the flag with nothing to restart. `T` is a no-op on a repo header; sharing is per-workspace.

**Session naming:**

Each agent instance in a shared workspace gets a deterministic tmux session name: `wsx-<repo>-<workspace>` for the primary agent, or `wsx-<repo>-<workspace>-<agent><ordinal>` for additional instances (e.g. `wsx-myrepo-fix-bug-codex2`). Characters outside `[A-Za-z0-9_-]` in the repo/workspace name are replaced with `-`, since tmux rejects `.` and `:` in session names. If two workspaces sanitize to the same name (e.g. repo `a` + workspace `b-c` vs repo `a-b` + workspace `c`), wsx appends the workspace id to disambiguate, so `-A` never attaches to the wrong agent. The name is derived once, stored in `session_ref`, and reused verbatim afterwards — it is never re-derived, so renaming a workspace does not orphan its running agent.

**Dashboard indicator:**

Shared workspaces are marked on the dashboard with a badge just left of the branch name — the tmux logo when nerd fonts are enabled, a hollow `◇` otherwise. The badge is **green** while the workspace's tmux session is alive (attached in this wsx, or detached-but-running on the server) and **red** when the workspace is shared but no live session backs it — the session has exited or was never started, so a remote peer can't attach. Direct workspaces show no badge.

A shared workspace's tmux session can outlive the wsx process that spawned it — right after a wsx restart, the agent is still running on the server even though this wsx holds no client for it. The green badge covers that case too: wsx periodically confirms the session with `tmux has-session`, so the badge stays green while the workspace's row status reads `idle`. Attaching (`Enter` on the row) reattaches wsx's client to the live session; the agent and its history are exactly where you left them.

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
wsx shared list --all
```

`--all` also lists every ready workspace that isn't shared (marked `unshared`). Without `--json`, prints one tab-separated line per agent instance: repo, workspace, tmux session name, and `alive`/`(dead)`/`-`. With `--json`, prints the same data as structured records (repo, workspace, branch, worktree path, and each agent's label/kind/session name/liveness) — useful for scripting against.

**Browsing another machine:**

To browse and attach to shared workspaces running on a remote machine, configure a list of ssh destinations under `wsx config edit shared_hosts`. The setting stores one entry per line as `name=ssh-destination`, e.g.:

```
mini=eben@ebenmini.local
lab=user@lab.example.com
```

On the dashboard, press `H` (capital, mnemonic *hosts*) to open a picker over these configured hosts, sorted by name. If no hosts are configured, an error modal points you at `wsx config edit shared_hosts`.

Selecting a host spawns a background fetch via `ssh <dest> "sh -lc 'wsx shared list --json --all'"` (one pre-quoted remote command, so ssh's argv join preserves it; login shell so wsx is found on the host's PATH). Results render as a list titled "workspaces on `<host>`". Live agent sessions come first, one row per agent instance:

```
agent  branch  repo/workspace
```

After them comes one row for each workspace on the host that has no live session, drawn dim, with `not shared` or `stopped` in place of the agent name. Navigate with `j`/`k` (or `↑`/`↓`), `r` to re-fetch the list, and `Esc` to close.

`Enter` on a live row attaches to it. `Enter` on a `not shared` or `stopped` row asks for confirmation, then runs `wsx workspace share <repo> <slug> --restart --json` on the host (see **Convert an existing workspace** above). The host's dashboard restarts that workspace's agents inside tmux, and wsx attaches to the first one as soon as it is live, so you can pick up a workspace you forgot to share without touching the host. The restart cuts off whatever the agent was doing at that moment; its conversation resumes. This needs wsx running on the host. If it isn't (or the host has no tmux), the list comes back with the host's reason. `Esc` while the share runs returns to the list, and the share still finishes on the host. Repo and workspace names that would need shell quoting (spaces, quotes, `;`, …) can't be shared this way.

A host running an older wsx doesn't understand `--all`. wsx retries without it and shows only that host's live shared sessions, titled "shared workspaces on `<host>`", with no sharing from the picker.

Attaching spawns `ssh -t <dest> -- "sh -lc \"tmux -u attach -t '=<name>'\""` as a PTY session — the remote command is one pre-quoted argument routed through a login `sh` (the same PATH rules as the list fetch; sshd otherwise hands the command to a non-login zsh that reads only `~/.zshenv`, where homebrew's tmux often isn't on PATH), the `=` target is single-quoted so zsh can't expand it as a command path, and `-u` forces UTF-8 (the ssh context has no locale, and without it tmux degrades box-drawing characters to rows of literal `q`s). You interact with the remote agent as if it were local; the exact-match `=` prefix ensures the correct agent is targeted, even if multiple agents sanitize to similar names.

**Detaching and persistence:**

`Ctrl-x d` detaches from the remote session, severing only the local ssh client. The remote agent keeps running in its tmux server — quitting wsx has the same effect. Reattaching resumes the exact session with its full history intact. Detaching lands back on the dashboard; the fetched list is ephemeral and never persisted, and pressing `H` again reopens the host picker with a fresh fetch.

**Failure modes:**

Fetching fails if the host is unreachable, ssh authentication fails, or wsx is missing on the host's login-shell PATH. All fetch errors surface in an error modal carrying ssh's stderr. Sharing from the picker fails if no wsx dashboard is running on the host, the host has no tmux ≥ 3.2, or an agent can't start there (missing worktree or agent binary). The list reappears with the host's reason on its notice line. A row whose session died after the fetch fails to attach; press `r` to re-fetch, and the workspace shows up as `stopped`, ready to share again.

**Requirements:**

- SSH key access to the remote host (password prompts are not supported for the background list fetch — use key-based auth via `ssh-agent` or key files; the attach itself runs in a real terminal but key auth is strongly recommended for a smooth flow).
- wsx **and tmux** installed on the host and reachable via a login `sh`'s PATH (e.g., `ssh <host> "sh -lc 'which wsx tmux'"` should print both — the outer double quotes keep `sh -lc '…'` a single argument, so ssh's space-join back into the host shell preserves the inner quoting). macOS gotcha: PATH additions that live only in zsh config (`~/.zshrc`/`~/.zprofile`, including homebrew's `brew shellenv`) are invisible to `sh -l` — add them to `~/.profile` too.
- The wsx dashboard running on the host, to share a workspace from the picker. Workspaces already shared there (created with `--shared`, or converted with `T`) can be attached without it.
- A local `ssh` binary (no local tmux needed for remote attach).

**v1 limitation — scrollback:**

Reattaching (in wsx or via a bare `tmux attach`) only repaints the tmux session's *current visible screen* — wsx's own scrollback buffer (see [Mouse, scrollback, and text selection](../daily-use/mouse-scrollback-selection.md)) resets with each new client and doesn't carry history across a detach/reattach. tmux's own scrollback for the session is unaffected and still reachable in-session via its usual copy-mode (`Ctrl-b [` with tmux's default prefix). A richer remote-scrollback view is planned for a later phase.
