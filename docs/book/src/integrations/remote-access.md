Running wsx on one machine (e.g. your desktop) and attaching from another (e.g. a laptop) works cleanly with tmux + ssh — no wsx-specific networking required.

**On the host machine:**

```
tmux new -As wsx 'wsx'
```

This starts wsx inside a tmux session named `wsx` (or reattaches to it if one already exists).

**From any other machine:**

```
ssh desktop -t tmux attach -t wsx
```

Workspaces — and the claude sessions running inside them — keep running while you're detached, so picking up where you left off from a different machine just works.

**Notes:**

- wsx's leader key is `Ctrl-x`, chosen specifically to not collide with tmux's default `Ctrl-b` prefix (or anyone's `Ctrl-a` customization). No tmux config needed.
- **Mosh** drops in cleanly if your network is flaky: `mosh desktop -- tmux attach -t wsx`.
- **Tailscale** (or any VPN) makes the host reachable from anywhere by a stable name without port-forwarding.

**Saving the invocation**: once you've settled on a working `ssh … tmux attach …` command, save it as a named remote so reconnecting is just `wsx remote <name>`. See [Named remote shortcuts](remote-access.md).

This page covers running the whole wsx TUI over ssh + tmux. For per-workspace sharing — an individual agent session that survives wsx quitting and can be attached to directly, independent of wsx itself — see [Shared workspaces](shared-workspaces.md). For browsing and attaching to shared workspaces on a remote host directly from wsx's dashboard (press `H`), see the "Browsing another machine" section in [Shared workspaces](shared-workspaces.md).

Claude Code's `--remote-control` flag exposes a running session to
[claude.ai/code](https://claude.ai/code) and the Claude iOS/Android
apps. The local PTY behavior is unchanged — claude prints a session
URL and a QR code at startup that you can scan from your phone or
open in a browser to attach remotely.

wsx passes `--remote-control` to every claude spawn by default, so
any session is reachable from your phone without extra setup.

**Toggle**: disable with `wsx config set remote_control false`. With
it off, sessions are local-only and nothing is sent to Anthropic's
relay servers.

**Sandbox**: claude offers `--sandbox` as an extra safety wrapper for
remote-issued commands. Disabled by default in wsx; enable with
`wsx config set remote_control_sandbox true`.

**Auth**: the relay rides on your claude.ai account. If you're not
signed in or you're offline, the local session continues to work and
the remote relay just fails silently.

**Privacy**: enabling remote control routes session state through
Anthropic's relay infrastructure. The session URL emitted in the PTY
is also visible to anyone seeing your screen.

```
wsx remote                 # list configured names (alphabetized), one per line
wsx remote <name>          # exec the stored command — process-replaces wsx
wsx config edit remotes    # opens $EDITOR on the blob
```

Stores frequently-used remote shell commands — typically `ssh -t host '…tmux attach…'` for reattaching a wsx session running on another machine (see [Remote access](remote-access.md)) — under short names. The value is an arbitrary shell command run through `sh -c`, so nested quoting works as you'd type it at a terminal.

The `remotes` setting is a newline-separated blob, one `name=command` per line. **There is no `wsx remote add`** — `wsx config edit remotes` opens the existing blob in `$EDITOR`, and you add a remote by appending a new line. Clearing the buffer and typing only the new line replaces every other remote, so always keep the existing lines unless you mean to drop them. Example:

```
ebenmini=ssh -4 -t ebenmini.local "zsh -lc 'tmux attach'"
gpu=ssh gpu-box -t 'tmux -u attach -t main || tmux -u new -s main'
```

Parser rules: only the **first** `=` separates name from command (so `=` inside the command, e.g. an inline env-var, is preserved); whitespace around `=` is trimmed; blank lines are skipped; lines with an empty name or command are dropped; duplicate names take the last value.

`wsx remote <name>` `exec`-replaces the wsx process with `sh -c <command>`, so signals and TTY state flow straight through to the remote session; when it exits you're back at your local shell with no wsx parent process. Unknown names error out with the list of available names.
