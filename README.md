# wsx (WorkspaceX)

Terminal UI for managing Claude Code, Pi, Hermes, or Codex sessions in git worktrees.

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
- **Multiple coding agents** — run Claude, Pi, Hermes, or Codex per workspace.
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
`wsx waybar jump <repo> <slug>` focuses a running TUI on that workspace or
launches a new one. These commands are Linux-only and error on other
platforms.

## Development

Build and test with `cargo build` / `cargo test`. See the
[Development docs](https://bakedbean.github.io/workspacex/docs/development/index.html).

## License

[MIT](LICENSE)
