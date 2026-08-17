wsx enables terminal mouse capture so the trackpad / wheel scrolls
through the session's history (instead of getting translated into
arrow keys that claude reads as prompt-history navigation). One
consequence: native click-and-drag selection no longer works by
default.

To select text from the claude pane, **hold Shift while
dragging** — most modern terminals (Alacritty, Kitty, WezTerm,
iTerm2, GNOME Terminal) bypass mouse capture under Shift and fall
back to OS-native selection. iTerm2 also supports right-click →
"Bypass mouse reporting", and macOS terminals often accept Option
as the modifier instead of Shift.

## Clickable dashboard targets

Mouse capture also makes parts of the dashboard clickable:

- **A workspace row's PR chip** (`⏺ #123 open`) opens that pull
  request in your browser.
- **A repo header's PR link** opens that repo's pull requests
  filtered to your own open ones — the GitHub PRs tab with
  `is:pr is:open author:@me` already applied.

The repo PR link renders as the git-pull-request glyph (``) with
[`nerd_fonts`](../configuration/global-settings.md) on and the
literal `PR` with it off, in the open-PR colour:

```
▾ ─── wsx  ? 1  ! 1    4 ws    ────────────────  /home/eben/workspace/wsx
```

It appears only on repos whose `origin` remote points at github.com,
so a repo wsx can't build that view for shows nothing rather than a
link that opens a dead tab. Self-hosted GitHub Enterprise remotes are
not recognised. Both actions shell out to
[`gh`](https://cli.github.com), which must be installed and
authenticated.
