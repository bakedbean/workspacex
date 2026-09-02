//! The command registry: every group and command wsx exposes, with
//! the usage strings and blurbs `help` renders from.
//!
//! This table is the single source of truth for what the CLI advertises.
//! A command that dispatches in `run` but is missing here is invisible to
//! `wsx help`; `tests::registry_matches_dispatched_groups` guards that.

pub struct CmdInfo {
    pub usage: &'static str,
    pub blurb: &'static str,
}

pub struct GroupInfo {
    pub name: &'static str,
    pub blurb: &'static str,
    pub commands: &'static [CmdInfo],
}

pub static GROUPS: &[GroupInfo] = &[
    GroupInfo {
        name: "workspace",
        blurb: "Create, list, rename, and archive workspaces",
        commands: &[
            CmdInfo {
                usage: "create <repo> [--name <slug>] [--yolo] [--shared] [--agent <kind>] [--prompt <text>]",
                blurb: "Create a workspace (branch + worktree), optionally seeding its agent",
            },
            CmdInfo {
                usage: "list [<repo>]",
                blurb: "List workspaces as TSV rows",
            },
            CmdInfo {
                usage: "path <repo> <slug>",
                blurb: "Print a workspace's worktree path",
            },
            CmdInfo {
                usage: "rename <repo> <old> <new>",
                blurb: "Rename a workspace slug and its branch",
            },
            CmdInfo {
                usage: "archive <repo> <slug> [--keep-worktree] [--force-delete-branch]",
                blurb: "Archive a workspace",
            },
            CmdInfo {
                usage: "share <repo> <slug>",
                blurb: "Convert a workspace to tmux-shared",
            },
            CmdInfo {
                usage: "unshare <repo> <slug>",
                blurb: "Convert a workspace to direct (not tmux)",
            },
        ],
    },
    GroupInfo {
        name: "agent",
        blurb: "List, add, and message agents in a workspace",
        commands: &[
            CmdInfo {
                usage: "list",
                blurb: "Show agents in the current workspace",
            },
            CmdInfo {
                usage: "add <kind>",
                blurb: "Attach an agent (claude|pi|hermes|codex|omp)",
            },
            CmdInfo {
                usage: "send [--workspace <repo>/<slug>] <label> <message...>",
                blurb: "Queue an async message to an agent here or in another workspace",
            },
        ],
    },
    GroupInfo {
        name: "repo",
        blurb: "Register and configure repositories",
        commands: &[
            CmdInfo {
                usage: "add <path> [--name <name>] [--prefix <prefix>]",
                blurb: "Register a repository",
            },
            CmdInfo {
                usage: "list",
                blurb: "List registered repositories",
            },
            CmdInfo {
                usage: "remove <name>",
                blurb: "Unregister a repository",
            },
            CmdInfo {
                usage: "set-prefix <name> <prefix>",
                blurb: "Set the branch prefix",
            },
            CmdInfo {
                usage: "set-base-branch <name> <ref-or-empty>",
                blurb: "Set the base branch",
            },
            CmdInfo {
                usage: "set-instructions <name> <value-or-@file>",
                blurb: "Set custom instructions",
            },
            CmdInfo {
                usage: "set-setup <name> <value-or-@file>",
                blurb: "Set the setup script",
            },
            CmdInfo {
                usage: "set-archive <name> <value-or-@file>",
                blurb: "Set the archive script",
            },
            CmdInfo {
                usage: "edit-setup <name>",
                blurb: "Edit the setup script in $EDITOR",
            },
            CmdInfo {
                usage: "edit-archive <name>",
                blurb: "Edit the archive script in $EDITOR",
            },
            CmdInfo {
                usage: "set-pinned-commands <name> <value-or-@file>",
                blurb: "Set pinned commands",
            },
            CmdInfo {
                usage: "edit-pinned-commands <name>",
                blurb: "Edit pinned commands in $EDITOR",
            },
            CmdInfo {
                usage: "set-name <name> <new-name>",
                blurb: "Rename a repository",
            },
            CmdInfo {
                usage: "set-related-repos <name> <value-or-@file>",
                blurb: "Set related repos",
            },
            CmdInfo {
                usage: "edit-related-repos <name>",
                blurb: "Edit related repos in $EDITOR",
            },
        ],
    },
    GroupInfo {
        name: "config",
        blurb: "Get and set global settings",
        commands: &[
            CmdInfo {
                usage: "get <key>",
                blurb: "Print a setting value",
            },
            CmdInfo {
                usage: "set <key> <value-or-@file>",
                blurb: "Set a setting",
            },
            CmdInfo {
                usage: "list",
                blurb: "List all settings",
            },
            CmdInfo {
                usage: "edit <key>",
                blurb: "Edit a setting in $EDITOR",
            },
        ],
    },
    GroupInfo {
        name: "remote",
        blurb: "Run saved remote shortcuts",
        commands: &[CmdInfo {
            usage: "[<name>]",
            blurb: "List remotes, or run the named remote shortcut",
        }],
    },
    GroupInfo {
        name: "shared",
        blurb: "Inspect tmux-shared workspaces",
        commands: &[CmdInfo {
            usage: "list [--json]",
            blurb: "List shared workspaces and their agent sessions",
        }],
    },
    GroupInfo {
        name: "setup",
        blurb: "One-off setup helpers",
        commands: &[
            CmdInfo {
                usage: "install-skill",
                blurb: "Install the wsx Claude Code skill",
            },
            CmdInfo {
                usage: "waybar",
                blurb: "Install the waybar module into ~/.config/waybar",
            },
            CmdInfo {
                usage: "menubar",
                blurb: "Install the SwiftBar plugin shim",
            },
        ],
    },
    GroupInfo {
        name: "status",
        blurb: "Report agent-driven workspace status",
        commands: &[
            CmdInfo {
                usage: "set <working|waiting|blocked|done> [--message <text>]",
                blurb: "Set workspace status (model push path)",
            },
            CmdInfo {
                usage: "clear",
                blurb: "Clear workspace status",
            },
            CmdInfo {
                usage: "from-hook [--agent <kind>]",
                blurb: "Parse hook JSON from stdin and update status",
            },
        ],
    },
    GroupInfo {
        name: "recap",
        blurb: "Maintain the agent-authored workspace recap",
        commands: &[
            CmdInfo {
                usage: "set [--goal|--state|--next <text>] [--goal-short|--state-short|--next-short <text>]",
                blurb: "Update recap fields (partial; at least one flag). *-short: keyword \
                        distillation for the dashboard row — identifiers, ticket/PR numbers, \
                        no filler (e.g. \"Audit V2 invoices, CV-04964, bug from #2835\")",
            },
            CmdInfo {
                usage: "show",
                blurb: "Print the current recap",
            },
            CmdInfo {
                usage: "clear",
                blurb: "Delete the recap",
            },
        ],
    },
    GroupInfo {
        name: "context",
        blurb: "Workspace context digest for editor-hosted agents",
        commands: &[
            CmdInfo {
                usage: "show",
                blurb: "Print the digest (branch, agents, status, recap, recent commits, \
                        primary agent's last message, instructions for an external agent)",
            },
            CmdInfo {
                usage: "write",
                blurb: "Write the digest to <state>/wsx/context/<repo>/<workspace>.md and print the path",
            },
        ],
    },
    GroupInfo {
        name: "waybar",
        blurb: "Linux waybar status module and workspace jumper",
        commands: &[
            CmdInfo {
                usage: "status",
                blurb: "Print waybar JSON for the custom module",
            },
            CmdInfo {
                usage: "menu",
                blurb: "Pick a workspace in a menu and jump to it",
            },
            CmdInfo {
                usage: "jump <repo> <slug>",
                blurb: "Select the workspace in a running TUI, or launch one",
            },
            CmdInfo {
                usage: "menu-entries [--json]",
                blurb: "Print walker/elephant menu entries as JSON",
            },
            CmdInfo {
                usage: "refresh-prs",
                blurb: "Refresh the cached PR state for all workspaces",
            },
        ],
    },
    GroupInfo {
        name: "menubar",
        blurb: "macOS menubar (SwiftBar) status module and workspace jumper",
        commands: &[
            CmdInfo {
                usage: "plugin",
                blurb: "Print the SwiftBar plugin document",
            },
            CmdInfo {
                usage: "jump <repo> <slug>",
                blurb: "Select the workspace in a running TUI, or launch one",
            },
            CmdInfo {
                usage: "copy-path <repo> <slug>",
                blurb: "Copy the workspace's worktree path to the clipboard",
            },
            CmdInfo {
                usage: "refresh",
                blurb: "Refresh cached git/PR indicators for all workspaces",
            },
        ],
    },
];

pub fn group_name(s: &str) -> Option<&'static str> {
    GROUPS.iter().map(|g| g.name).find(|&n| n == s)
}
