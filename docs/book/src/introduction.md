# Introduction

wsx is a terminal UI that runs Claude Code, Pi, Hermes, Codex, or oh-my-pi
sessions in their own git worktrees. Use it to launch, watch, and steer many
agent sessions in parallel from one dashboard, including several agents
working together in one workspace.

## Key features

- **Parallel sessions in worktrees**: each workspace gets its own branch, worktree, and agent session. See [Dashboard](dashboard/index.md).
- **Multiple coding agents**: pick Claude, Pi, Hermes, Codex, or oh-my-pi per workspace. See [Coding agents](agents/coding-agents.md).
- **Multi-agent workspaces**: run several agents in one worktree and have them message each other. See [Multi-agent workspaces](agents/multi-agent-workspaces.md).
- **Attention alerts**: status glyphs, a terminal bell, and a live activity line show which sessions need you. See [Status indicators](dashboard/status-indicators.md).
- **Agent status and recaps**: agents report their status and a goal/state/next recap with `wsx status` and `wsx recap`. See [Status and recap](agents/status-and-recap.md).
- **Project manager digest**: one pane summarizes what every workspace is for and what's next. See [Project manager digest](dashboard/project-manager-digest.md).
- **Configurable detail bar**: up to four containers of built-in or custom modules. See [Detail bar](dashboard/detail-bar.md).
- **Tags and hiding**: label workspaces and hide them by tag or PR state. See [Tags and hiding](dashboard/tags-and-hiding.md).
- **Split panes and saved layouts**: watch other workspaces while attached. See [Updates panel and split panes](attached-view/updates-and-splits.md).
- **Pinned commands and prompt tags**: fire saved prompts with `Ctrl-x <digit>` and insert XML-wrapped prompt sections. See [Pinned commands](attached-view/pinned-commands.md) and [Prompt tags](attached-view/prompt-tags.md).
- **Change chronology**: a timeline of every file edit the agent made. See [Change chronology](attached-view/change-chronology.md).
- **Bundled agent skills**: `wsx`, `agent-review`, and `handoff`. See [Agent skills](agents/skills.md).
- **Editor, terminal, and diff hooks**: open a workspace in your tools with one key. See [Editor, terminal, and diff](integrations/editor-terminal-diff.md).
- **Editor-hosted agents**: give an agent in your editor the workspace's context with `wsx context`. See [Editor-hosted agents](integrations/editor-agent-context.md).
- **Related repos**: let agents read sibling repos without editing them. See [Related repos](integrations/related-repos.md).
- **Remote and shared workspaces**: attach over ssh, or run agents in tmux on this or another host. See [Remote access](integrations/remote-access.md) and [Shared workspaces](integrations/shared-workspaces.md).
- **Desktop widgets**: a waybar module, a KDE Plasma applet, and a macOS menubar plugin. See [Desktop widgets](integrations/desktop-widgets.md).
- **Less setup per workspace**: auto-rename branches from your first prompt, and run per-repo setup and archive scripts. See [Auto-rename](configuration/auto-rename.md) and [Setup and archive scripts](configuration/setup-scripts.md).

## Where to go next

- New to wsx: [Installation](getting-started/installation.md), then [Quick start](getting-started/quick-start.md).
- Looking up a key: [Keybindings](dashboard/keybindings.md).
- Looking up a command or setting: [CLI reference](cli-reference/index.md) and [Settings](configuration/settings.md).
