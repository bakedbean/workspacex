# wsx (WorkspaceX)

Terminal UI for managing Claude Code, Pi, Hermes, Codex, or oh-my-pi sessions in git worktrees.

## Parallel Agent Sessions
### Deploy multiple workspaces at once all working in parallel with real time feedback 
https://github.com/user-attachments/assets/17962906-abde-4589-81e1-58737212645b

## Multi Agent Sessions
### Deploy multiple agents to the same workspace, orchestrate with the wsx CLI
https://github.com/user-attachments/assets/30c68dc1-9954-4dc6-b1a1-a8559ea5d665

## 📖 Documentation

**Full documentation: https://bakedbean.github.io/workspacex/docs/**

Searchable, navigable docs covering keybindings, configuration, the CLI,
integrations, and more.

## Key features

- **Parallel agent sessions in git worktrees** — every workspace is its own
  branch + worktree; switch with one key.
- **Multiple coding agents** — run Claude, Pi, Hermes, Codex, or oh-my-pi (`omp`) per workspace.
- **Multi-agent workspaces** — attach several agents to one worktree, switch
  focus with a keypress, and have them message each other via the `wsx` CLI.
- **Cross-session attention alerts** and a per-workspace activity sub-line so you
  see what every session is doing at a glance.
- **Configurable detail bar, themes, remote access, pinned commands, and MCP
  inheritance.**

See the
[full feature list](https://bakedbean.github.io/workspacex/docs/overview/key-features.html).

## Quick start

```bash
cargo build --release
./target/release/wsx repo add /path/to/your/repo
./target/release/wsx              # launch TUI
```

Press `n` to create your first workspace, then `enter` to attach. Claude Code
spawns inside the worktree. See the
[Quick start guide](https://bakedbean.github.io/workspacex/docs/overview/quick-start.html)
for the full walkthrough and next steps.

## Waybar indicator (Linux)

On Linux, a custom waybar module shows a git-branch glyph plus your live
workspace count across every repo, colored by the most urgent status
(blocked/done/waiting/working); hover it for a per-workspace tooltip.

```bash
wsx setup waybar
```

writes `wsx.jsonc`/`wsx.css` into `~/.config/waybar/` and patches
`config.jsonc` to include them (with a timestamped backup). Under the hood:
`wsx waybar status` emits the module JSON, `wsx waybar menu` opens a picker
(walker by default; override with `WSX_WAYBAR_MENU`), and
`wsx waybar jump <repo> <slug>` focuses a running TUI and opens that
workspace (attaching as if you pressed Enter on it), or launches a new TUI
already attached. These commands are Linux-only and error on other
platforms.

## macOS menubar (SwiftBar)

On macOS, a [SwiftBar](https://github.com/swiftbar/SwiftBar) plugin mirrors
the waybar indicator: a branch glyph plus your live workspace count, tinted
by the most urgent status, with a dropdown listing every repo and its
workspaces (status glyph, dirty marker, diff stats, PR number once known).

```bash
brew install swiftbar   # if you don't have it; launch once, pick a plugin folder
wsx setup menubar
```

installs a `wsx-menubar.10s.sh` shim into that plugin folder. Under the
hood: `wsx menubar plugin` renders the dropdown (polled by SwiftBar every
10s from cache) while `wsx menubar refresh` sweeps git/PR facts in the
background (≤ ~70s staleness). Each workspace row's submenu has Jump, Open
PR, Copy worktree path, and Reveal in Finder; jump prefers a running TUI via
its unix socket and falls back to spawning your terminal — set
`wsx config set terminal_cmd '<cmd with {cmd}>'` to control which one, or
let it use iTerm2/Terminal automatically. Note that `terminal_cmd` is shared
with the TUI's own open-in-terminal shortcut, which substitutes cwd rather
than `{cmd}` — if you use that shortcut, prefer leaving `terminal_cmd`
unset on macOS and letting jump auto-detect iTerm2/Terminal. These commands
are macOS-only and error on other platforms.

Below the workspace list, a **Project Manager** submenu shows each
workspace's agent-authored recap — the `goal` / `state` / `next` one-liners
maintained via `wsx recap set` — ordered blocked → waiting → least-recently
active. It is the menubar counterpart of the TUI's `p` view, rendered from
the same SQLite data, and clicking a workspace's line jumps to it.

## Development

Build and test with `cargo build` / `cargo test`. See the
[Development docs](https://bakedbean.github.io/workspacex/docs/development/index.html).

## License

[MIT](LICENSE)
