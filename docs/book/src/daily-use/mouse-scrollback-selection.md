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

The repo PR link sits just before the repo's path, normally in the same
dim colour, so the two read as one cluster identifying the repo. It turns
**green** — the same green a row's open-PR chip uses — when at least one
of that repo's workspaces has a pull request GitHub still counts as open,
so the colour tells you whether the link leads anywhere before you click
it. Drafts and conflicted PRs count, since the link's
`is:pr is:open author:@me` query lists them too; merged and closed ones
have dropped out of that list and leave the link dim. Folding a repo
hides its rows but not this signal.

With [`nerd_fonts`](../configuration/global-settings.md) off the link
renders as the literal text `PR`:

```
▾ ─── wsx  PR  /home/eben/workspace/wsx  ──────────────  ? 1  ! 1    4 ws
           ▲ click here
```

With `nerd_fonts` on it renders instead as the merge-queue octicon
(`nf-oct-git_merge_queue`) at **U+F4DB**. That codepoint lives in the
Private Use Area, so it only appears if your terminal font actually
patches it; if you see a blank or a tofu box in that position, the link
is still there and still clickable.

Repos without a link keep those columns blank rather than closing the
gap, so every path starts in the same column either way.

It appears only on repos whose `origin` remote points at github.com,
so a repo wsx can't build that view for shows nothing rather than a
link that opens a dead tab. Self-hosted GitHub Enterprise remotes are
not recognised. Both actions shell out to
[`gh`](https://cli.github.com), which must be installed and
authenticated.
