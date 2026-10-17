# Remote access

Reach wsx from another machine or your phone. Run the whole dashboard in tmux and attach over ssh, drive a single Claude Code session from claude.ai, or attach to one agent with [shared workspaces](shared-workspaces.md).

| You want | Use |
| --- | --- |
| The full dashboard from another computer | [tmux and ssh](#tmux-and-ssh) |
| One reconnect command per machine | [Named remotes](#named-remotes) |
| A Claude Code session from your phone or browser | [Remote control](#remote-control) |
| One agent from another machine, or from the dashboard's host picker | [Shared workspaces](shared-workspaces.md) |

## tmux and ssh

On the host, start wsx inside tmux, or reattach if it's already running:

```bash
tmux new -As wsx 'wsx'
```

From any other machine:

```bash
ssh desktop -t tmux attach -t wsx
```

Workspaces and their agents keep running while you're detached.

- wsx's leader key is `Ctrl-x`, so it doesn't collide with tmux's `Ctrl-b` prefix.
- On a flaky network, use mosh: `mosh desktop -- tmux attach -t wsx`.
- Tailscale or another VPN makes the host reachable by name without port forwarding.

## Named remotes

Save a reconnect command under a short name, then run it with `wsx remote`:

```bash
wsx remote                 # list saved names
wsx remote desktop         # run the saved command
wsx config edit remotes    # edit the list in $EDITOR
```

The `remotes` setting holds one `name=command` per line:

```
desktop=ssh -t desktop 'tmux attach -t wsx'
gpu=ssh gpu-box -t 'tmux -u attach -t main || tmux -u new -s main'
```

- There is no `wsx remote add`. Add a line in the editor, and keep the existing lines unless you mean to drop them.
- Only the first `=` splits name from command, so the command can contain `=`.
- The command runs with `sh -c` and replaces the wsx process. When it exits, you're back at your shell.
- A duplicate name uses its last value.

See [`wsx remote`](../cli-reference/other.md#wsx-remote).

## Remote control

wsx starts Claude Code with `--remote-control`, so each Claude session prints a URL and a QR code you can open on [claude.ai/code](https://claude.ai/code) or the Claude mobile apps. It applies to Claude Code agents only.

| Setting | Effect |
| --- | --- |
| `remote_control` | Set to `false` to keep sessions local-only. |
| `remote_control_sandbox` | Set to `true` to run remote-issued commands in Claude Code's `--sandbox`. |

- The relay uses your claude.ai account. Signed out or offline, the local session still works.
- Session state goes through Anthropic's relay, and anyone who can see your screen can see the session URL.

See [Settings](../configuration/settings.md).
