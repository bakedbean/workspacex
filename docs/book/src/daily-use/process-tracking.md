`[k]` on the dashboard (or `Ctrl-x k` while attached) shows long-running
processes whose current working directory is inside the selected
workspace's worktree — dev servers, watchers, anything you started in
that worktree from a terminal. Workspaces with detected processes show
a `~N` count between the branch and activity columns on the dashboard.

The modal lists each process's PID, command, and full cwd:

    ─── Processes — fix-bug ──────
      PID    COMMAND          CWD
      12345  npm              /home/user/wt/fix-bug
      12389  pytest           /home/user/wt/fix-bug/tests
    ─────────────────────────────
    [↑/↓] move   [r] run   [k] term   [K] kill   [esc] close

`k` sends `SIGTERM` to the highlighted process; `K` sends `SIGKILL`.
After either, wsx immediately re-scans so the list reflects the new
state.

`r` opens a prompt to run a command in the selected workspace's worktree —
handy for starting a dev server without opening a separate terminal. The
command runs via `sh -c` as a background process, with stdout and stderr
captured to a log file under `~/.local/state/wsx/logs/`; the path is shown
after launch. It runs detached (its own session, reparented away from wsx),
so it keeps running if you close the dashboard and survives until it exits or
you stop it. Because it runs in the worktree, it appears in this same list on
the next scan, where `K` stops it.

**Notes:**

- Detection runs once every 10 seconds in the background via `lsof -d cwd`.
- Shells and editors (bash, zsh, nvim, code, etc.) are filtered out so the
  count surfaces what's interesting — your dev server, not the terminal
  hosting it.
- Helper processes spawned by Claude Code and editors (MCP servers,
  language servers) are hidden too, since they inherit the worktree cwd
  but aren't work you launched. The exception is a process holding a
  listening TCP socket: a dev server started from inside Claude Code (e.g.
  a `pnpm dev` on `:3000`) still shows up and can be killed here, while the
  stdio-only helpers stay filtered.
- wsx never starts these processes itself. Launch them however you
  like (the `[t]` terminal keybind is one option). The feature is
  observability plus a kill hook, not lifecycle management.
- Requires `lsof` to be installed (standard on most Linux/macOS setups).
  If it's missing, the count stays at 0 and the modal shows "(no tracked
  processes)" — no errors.
