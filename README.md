# wsx (WorkspaceX)

[![CI](https://img.shields.io/github/actions/workflow/status/bakedbean/workspacex/ci.yml?branch=main&label=CI)](https://github.com/bakedbean/workspacex/actions/workflows/ci.yml) [![Website](https://img.shields.io/badge/website-workspace--x.com-orange)](https://workspace-x.com) [![Docs](https://img.shields.io/badge/docs-bakedbean.github.io-blue)](https://bakedbean.github.io/workspacex/docs/) [![Discord](https://img.shields.io/badge/Discord-join-5865F2?logo=discord&logoColor=white)](https://discord.gg/a9a9Q6jcH) [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/bakedbean/workspacex/blob/main/LICENSE)

Terminal UI for managing Claude Code, Pi, Hermes, Codex, or oh-my-pi sessions in git worktrees.

[Website](https://workspace-x.com) · [Documentation](https://bakedbean.github.io/workspacex/docs/) · [Discord](https://discord.gg/a9a9Q6jcH)

## Key features

- **Parallel agent sessions in git worktrees**: each workspace gets its own branch, worktree, and session.
- **Multiple coding agents**: Claude, Pi, Hermes, Codex, or oh-my-pi (`omp`), chosen per workspace.
- **Multi-agent workspaces**: several agents in one worktree that message each other through the `wsx` CLI.
- **Attention alerts and recaps**: see which sessions need you, and what each one is for and doing.
- **Shared and remote workspaces**: run agents in tmux, on this machine or another host, and attach over ssh.
- **Desktop widgets**: a [waybar module, KDE Plasma applet, and macOS menubar plugin](https://bakedbean.github.io/workspacex/docs/integrations/desktop-widgets.html).
- **Pinned commands, prompt tags, a configurable detail bar, and themes.**

See the [full feature list](https://bakedbean.github.io/workspacex/docs/introduction.html#key-features).

## Install

```bash
brew tap bakedbean/workspacex https://github.com/bakedbean/workspacex
brew install bakedbean/workspacex/wsx
```

For Nix, cargo-binstall, prebuilt tarballs, or a source build, see the
[installation guide](https://bakedbean.github.io/workspacex/docs/getting-started/installation.html).

## Quick start

```bash
wsx repo add /path/to/your/repo
wsx              # launch the TUI; press n for a new workspace, enter to attach
```

See the [quick start guide](https://bakedbean.github.io/workspacex/docs/getting-started/quick-start.html).

## Development

Build and test with `cargo build` and `cargo test`. See the
[development docs](https://bakedbean.github.io/workspacex/docs/development/index.html).

## License

[MIT](LICENSE)
