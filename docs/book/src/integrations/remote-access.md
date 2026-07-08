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

**Saving the invocation**: once you've settled on a working `ssh … tmux attach …` command, save it as a named remote so reconnecting is just `wsx remote <name>`. See [Named remote shortcuts](named-remote-shortcuts.md).

This page covers running the whole wsx TUI over ssh + tmux. For per-workspace sharing — an individual agent session that survives wsx quitting and can be attached to directly, independent of wsx itself — see [Shared workspaces](shared-workspaces.md). For browsing and attaching to shared workspaces on a remote host directly from wsx's dashboard (press `H`), see the "Browsing another machine" section in [Shared workspaces](shared-workspaces.md).
